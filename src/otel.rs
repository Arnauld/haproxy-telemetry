use crate::frame::{Action, ActionVarScope, Error, Frame, FrameHeader, TypedData, ListOfMessages};
use opentelemetry::global::{BoxedSpan, ObjectSafeTracer};
use opentelemetry::sdk::Resource;
use opentelemetry::trace::{Span, TraceError, TraceFlags};
use opentelemetry::{global, sdk, sdk::trace as sdktrace, trace::Tracer, Key, KeyValue};
use std::borrow::{Borrow, BorrowMut};
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


const TRACEPARENT_HEADER: &str = "traceparent";
const TRACESTATE_HEADER: &str = "tracestate";

pub fn handle_notify(
    db: &OtelContext,
    header: &FrameHeader,
    messages: &ListOfMessages,
) -> Result<Option<Vec<Action>>, Error> {
    for (k, _v) in messages {
        println!("======================");
        println!("MSG: {}", k);
        println!("======================");
    }

    let mut actions: Vec<Action> = vec![];

    if let Some(details) = messages.get("opentracing:frontend_tcp_request") {
        let tracer = global::tracer("my_service");
        let mut span = tracer.start("my_span");
        for (k, v) in details {
            let mut key = Key::new(k.to_owned());
            let attr = v.as_value(key);
            span.set_attribute(attr);
        }

        let span_context = span.span_context();
        if span_context.is_valid() {
            let header_value = format!(
                "{:02x}-{:032x}-{:016x}-{:02x}",
                1, //SUPPORTED_VERSION,
                span_context.trace_id(),
                span_context.span_id(),
                span_context.trace_flags() & TraceFlags::SAMPLED
            );

            actions.push(Action::SetVar {
                scope: ActionVarScope::REQUEST,
                name: TRACEPARENT_HEADER.to_string(),
                value: TypedData::STRING(header_value),
            });
            actions.push(Action::SetVar {
                scope: ActionVarScope::REQUEST,
                name: TRACESTATE_HEADER.to_string(),
                value: TypedData::STRING(span_context.trace_state().header()),
            });
        }

        let mut db = db.lock().unwrap();
        db.insert(header.stream_id.to_string(), OtelSpanContext { span });
    } else if let Some(_details) = messages.get("opentracing:http_response") {
        let mut db = db.lock().unwrap();
        let id: String = header.stream_id.to_string();
        if let Some(ctx) = db.remove(&id) {
            let mut span = ctx.span;
            span.end();
        }
    }



    Ok(Some(actions))
}

impl TypedData {
    pub fn as_value(&self, key: Key) -> KeyValue {
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
