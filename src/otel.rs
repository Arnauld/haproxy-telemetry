use crate::frame::{Action, ActionVarScope, Error, FrameHeader, KVList, ListOfMessages, TypedData};
use crate::proplists::*;
use opentelemetry::global::BoxedSpan;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::sdk::propagation::TraceContextPropagator;
use opentelemetry::sdk::Resource;
use opentelemetry::trace::{Span, SpanContext, TraceContextExt, TraceError, TraceFlags};
use opentelemetry::{global, sdk, sdk::trace as sdktrace, trace::Tracer, Key, KeyValue};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct OtelSpanContext {
    span: BoxedSpan,
}

pub type OtelContext = Arc<Mutex<HashMap<String, OtelSpanContext>>>;

pub fn init_tracer(service_name: String) -> Result<sdk::trace::Tracer, TraceError> {
    global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    opentelemetry_jaeger::new_pipeline()
        //.with_agent_endpoint("http://localhost:14268/api/traces")
        .with_trace_config(
            sdktrace::config().with_resource(Resource::new(vec![KeyValue::new(
                opentelemetry_semantic_conventions::resource::SERVICE_NAME,
                service_name,
            )])),
        )
        .install_simple()
}

pub fn new_otel_context() -> OtelContext {
    Arc::new(Mutex::new(HashMap::new()))
}

const SERVICE_NAME: &str = "haproxy_spoa";

pub fn handle_notify(
    db: &OtelContext,
    header: &FrameHeader,
    messages: &ListOfMessages,
) -> Result<Option<Vec<Action>>, Error> {
    let msgs: Vec<String> = messages.iter().map(|(k, _)| k.to_string()).collect();
    log::debug!("Notify/Messages {:?}", msgs);

    let mut actions: Vec<Action> = vec![];

    for (message, details) in messages {
        let key = key_of(header, details);

        if message.eq_ignore_ascii_case("opentracing:frontend_tcp_request") {
            let tracer = global::tracer(SERVICE_NAME);
            let mut span = tracer.start("frontend_tcp_request");
            enrich_span_with_tags(&mut span, details);

            track_span(db, key, span);
        } else if message.eq_ignore_ascii_case("opentracing:frontend_http_request") {
            log::info!(
                "=========================================================================="
            );
            log::info!("otel/frame details {:?}", details);
            log::info!(
                "=========================================================================="
            );
            //
            let key: String = key_of(header, details);

            // terminate previous span, if any
            end_span(db, &key);

            let propagator = TraceContextPropagator::new();
            //
            let extractor = &KVListExtractor(details);
            let context = propagator.extract(extractor);
            let tracer = global::tracer(SERVICE_NAME);
            let mut span = if context.has_active_span() {
                log::info!("Active span detected!");
                tracer.start_with_context("frontend_http_request", &context)
            } else {
                log::info!("No active span detected :s");
                tracer.start("frontend_http_request")
            };

            enrich_span_with_tags(&mut span, details);

            log::info!("Span context {:?}", &span.span_context());
            let injector = &mut ActionInjector(&mut actions);
            //-- span is not in *current* context...
            // propagator.inject(injector);
            injector.apply_context(span.span_context());

            track_span(db, key, span);
        } else if message.eq_ignore_ascii_case("opentracing:http_response") {
            let key: String = key_of(header, details);
            end_span(db, &key);
        }
    }

    Ok(Some(actions))
}

fn track_span(db: &OtelContext, key: String, span: BoxedSpan) {
    log::debug!("otel/frame tracking span {}", key);
    let mut db = db.lock().unwrap();
    db.insert(key, OtelSpanContext { span });
}

fn end_span(db: &OtelContext, key: &String) {
    let mut db = db.lock().unwrap();
    if let Some(ctx) = db.remove(key) {
        log::info!("otel/frame discarding span [{}]", key);
        let mut span = ctx.span;
        span.end();
    } else {
        log::warn!("otel/frame no span found corresponding to key [{}]", key);
    }
}

trait TagAware {
    fn set_tag(&mut self, attr: KeyValue);
}

impl TagAware for BoxedSpan {
    fn set_tag(&mut self, attr: KeyValue) {
        self.set_attribute(attr);
    }
}

fn unknown_tag_extra(dst: &mut PropLists<String>, key: &String, value: &TypedData) {
    if key == "id" {
        let raw = value.to_string();
        dst.push("server.tx_id", raw.to_owned());
        if let Some(index) = raw.find(":") {
            dst.push("server.name", format!("{}", &raw[..index]));
        }
    }
}

fn enrich_span_with_tags<S: TagAware>(span: &mut S, details: &KVList) {
    let tags = extract_tags(details, unknown_tag_extra);
    for (k, v) in tags {
        let attr = Key::new(k.to_owned()).string(v);
        span.set_tag(attr);
    }
}

fn key_of(header: &FrameHeader, details: &KVList) -> String {
    let str_id = match details.iter().find(|(k, _)| k == "id").unwrap() {
        (_, TypedData::STRING(s)) => s,
        _ => "::",
    };
    format!("{}::{}", header.stream_id.to_string(), str_id)
}

impl TypedData {
    pub fn as_key_value(&self, key: Key) -> KeyValue {
        match self {
            TypedData::NULL => key.string("<null>"),
            TypedData::BOOL(v) => key.bool(*v),
            TypedData::INT32(v) => key.i64(*v as i64),
            TypedData::UINT32(v) => key.i64(*v as i64),
            TypedData::INT64(v) => key.i64(*v as i64),
            TypedData::UINT64(v) => key.i64(*v as i64),
            TypedData::IPV4(addr) => key.string(addr.to_string().to_owned()),
            TypedData::IPV6(addr) => key.string(addr.to_string().to_owned()),
            TypedData::STRING(s) => key.string(s.to_owned()),
            TypedData::BINARY(_) => key.string("<bin>"),
        }
    }
}

fn extract_tags(
    details: &KVList,
    unknown_tag: fn(&mut PropLists<String>, &String, &TypedData),
) -> PropLists<String> {
    let mut props = PropLists::new();

    let mut tag_name: Option<String> = None;
    let mut s = String::new();
    for (k, v) in details {
        // close previous potential tag
        if k != "" {
            if let Some(tag) = tag_name {
                props.push(&tag, s.to_owned());
                tag_name = None;
                s = String::new();
            }
            if k == "tag" {
                tag_name = Some(v.to_string());
            } else {
                unknown_tag(&mut props, k, v)
            }
        }
        // append only if within tag
        else if tag_name.is_some() {
            s.push_str(&v.to_string());
        }
    }
    // any unclosed tag ?
    if let Some(tag) = tag_name {
        props.push(&tag, s.to_owned());
    }
    props
}

pub struct KVListExtractor<'a>(pub &'a KVList);

impl<'a> Extractor for KVListExtractor<'a> {
    /// Get a value for a key from the KVList.  If the value is not valid ASCII, returns None.
    fn get(&self, key: &str) -> Option<&str> {
        match self.0.iter().find(|(k, _)| k == key).unwrap() {
            (_, TypedData::STRING(s)) => Some(s),
            _ => None,
        }
    }

    /// Collect all the keys from the KVList.
    fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>()
    }
}

const SUPPORTED_VERSION: u8 = 0;
const TRACEPARENT_HEADER: &str = "traceparent";
const TRACESTATE_HEADER: &str = "tracestate";

pub struct ActionInjector<'a>(pub &'a mut Vec<Action>);

impl<'a> ActionInjector<'a> {
    pub(crate) fn apply_context(&mut self, span_context: &SpanContext) {
        // https://github.com/open-telemetry/opentelemetry-rust/blob/main/opentelemetry-sdk/src/propagation/trace_context.rs#L115
        if span_context.is_valid() {
            let header_value = format!(
                "{:02x}-{:032x}-{:016x}-{:02x}",
                SUPPORTED_VERSION,
                span_context.trace_id(),
                span_context.span_id(),
                span_context.trace_flags() & TraceFlags::SAMPLED
            );
            self.set(TRACEPARENT_HEADER, header_value);
            self.set(TRACESTATE_HEADER, span_context.trace_state().header());
        }
    }
}

impl<'a> Injector for ActionInjector<'a> {
    /// Set a key and value in the HeaderMap.  Does nothing if the key or value are not valid inputs.
    fn set(&mut self, key: &str, value: String) {
        self.0.push(Action::SetVar {
            scope: ActionVarScope::REQUEST,
            name: key.to_string(),
            value: TypedData::STRING(value),
        });
    }
}

impl TagAware for PropLists<String> {
    fn set_tag(&mut self, attr: KeyValue) {
        self.push(&attr.key.to_string(), attr.value.to_string())
    }
}

#[cfg(test)]
mod tests {

    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    fn unknown_tag_noop(_dst: &mut PropLists<String>, _key: &String, _value: &TypedData) {}

    fn sample_kv_list() -> KVList {
        let details = &mut KVList::new();
        details.push(as_tuple(
            "id",
            "haproxy-2:d9e05a62-79e4-4457-967d-a129ea6cf6c3:0008",
        ));
        details.push(as_tuple("span", "Frontend HTTP request"));
        details.push(as_tuple("follows-from", "Frontend TCP request"));
        details.push(as_tuple(
            "traceparent",
            "00-2ccb154527b07c593856c7bd539f5ee5-e79f6d458b7f9104-01",
        ));
        details.push(as_tuple("tracestate", ""));
        details.push(as_tuple("tag", "http.method"));
        details.push(as_tuple("", "GET"));
        details.push(as_tuple("tag", "http.url"));
        details.push(as_tuple("", "/"));
        details.push(as_tuple("tag", "http.version"));
        details.push(as_tuple("", "HTTP/"));
        details.push(as_tuple("", "1.1"));
        details.push(as_tuple("finish", "Frontend TCP request"));
        details.to_vec()
    }

    fn as_tuple(k: &str, d: &str) -> (String, TypedData) {
        (k.to_string(), TypedData::STRING(d.to_string()))
    }

    #[test]
    fn test_extract_tags() {
        let details = sample_kv_list();
        let tags = extract_tags(&details, unknown_tag_noop);
        assert_eq!(tags.first("http.method"), Some(&"GET".to_string()));
        assert_eq!(tags.first("http.url"), Some(&"/".to_string()));
        assert_eq!(tags.first("http.version"), Some(&"HTTP/1.1".to_string()));
        assert_eq!(tags.len(), 3);
    }

    #[test]
    fn test_enrich_span_with_tags() {
        let details = sample_kv_list();
        let mut tags: PropLists<String> = PropLists::new();
        enrich_span_with_tags(&mut tags, &details);
        assert_eq!(tags.first("http.method"), Some(&"GET".to_string()));
        assert_eq!(tags.first("http.url"), Some(&"/".to_string()));
        assert_eq!(tags.first("http.version"), Some(&"HTTP/1.1".to_string()));
        assert_eq!(tags.first("http.version"), Some(&"HTTP/1.1".to_string()));
        assert_eq!(tags.first("server.tx_id"), Some(&"haproxy-2:d9e05a62-79e4-4457-967d-a129ea6cf6c3:0008".to_string()));
        assert_eq!(tags.first("server.name"), Some(&"haproxy-2".to_string()));
        assert_eq!(tags.len(), 5);
    }
}
