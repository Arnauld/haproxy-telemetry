use std::borrow::{Borrow, BorrowMut};
use crate::frame::{Action, ActionVarScope, Error, Frame, FrameHeader, TypedData};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use opentelemetry::{global, sdk, sdk::export::trace::stdout, trace::Tracer};
use opentelemetry::global::BoxedSpan;
use opentelemetry::trace::{Span, TraceError};

pub struct OtelSpanContext {
    span: BoxedSpan
}

pub type OtelContext = Arc<Mutex<HashMap<String, OtelSpanContext>>>;

pub fn init_tracer () -> Result<sdk::trace::Tracer, TraceError> {
    global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
    opentelemetry_jaeger::new_pipeline().install_simple()
}

pub fn new_otel_context() -> OtelContext {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn handle_notify(
    db: &OtelContext,
    header: &FrameHeader,
    messages: &HashMap<String, HashMap<String, TypedData>>,
) -> Result<Option<Vec<Action>>, Error> {
    for (k, _v) in messages {
        println!("======================");
        println!("MSG: {}", k);
        println!("======================");
    }

    if let Some(details) = messages.get("opentracing:frontend_tcp_request") {
            let tracer = global::tracer("my_service");
            let mut span = tracer.start("my_span");
            let mut db = db.lock().unwrap();
            println!("SPAN {:?}", &span);
            db.insert(header.stream_id.to_string(), OtelSpanContext { span });
    }
    else if let Some(details) = messages.get("opentracing:tcp_response") {
        let mut db = db.lock().unwrap();
        let id: String = header.stream_id.to_string();
        if let Some(ctx) = db.remove(&id) {
            let mut span = ctx.span;
            span.end();
        }
    }


    let mut actions: Vec<Action> = vec![];
    actions.push(Action::SetVar {
        scope: ActionVarScope::REQUEST,
        name: "trace-id".to_string(),
        value: TypedData::STRING("aef34-x35-bb9".to_string()),
    });

    Ok(Some(actions))
}
