use crate::frame::{Action, ActionVarScope, Error, Frame, FrameHeader, TypedData};
use std::collections::HashMap;

pub fn handle_notify(
    header: &FrameHeader,
    messages: &HashMap<String, HashMap<String, TypedData>>,
) -> Result<Option<Vec<Action>>, Error> {
    for (k, _v) in messages {
        println!("======================");
        println!("MSG: {}", k);
        println!("======================");
    }

    match messages.get("opentracing:frontend_tcp_request") {
        Some(details) => {

        }
        None => ()
    }

    let mut actions: Vec<Action> = vec![];
    actions.push(Action::SetVar {
        scope: ActionVarScope::REQUEST,
        name: "trace-id".to_string(),
        value: TypedData::STRING("aef34-x35-bb9".to_string()),
    });

    Ok(Some(actions))
}
