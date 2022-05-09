use std::collections::HashMap;

use tokio::net::{TcpListener, TcpStream};

pub use connection::Connection;
use frame::{Action, Error, Frame, FrameFlags, FrameHeader, FrameType, TypedData};

use crate::frame::ActionVarScope;

mod connection;

mod frame;

mod otel;
use crate::otel::handle_notify as otel_spoa_notify;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "0.0.0.0:7001";
    println!("Starting Agent on {}", addr);
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (socket, addr) = listener.accept().await?;
        println!("New socket opened from {:?}", addr);
        tokio::spawn(async move {
            // Process each socket concurrently.
            process(socket).await
        });
    }
}

fn handle_notify(
    header: &FrameHeader,
    messages: &HashMap<String, HashMap<String, TypedData>>,
) -> Result<Option<Frame>, Error> {
    // only impl for now
    otel_spoa_notify(header, messages).map(|actions_opt| {
        actions_opt.map(|actions| Frame::Ack {
            header: header.reply_header(&FrameType::ACK),
            actions,
        })
    })
}

fn handle_frame(frame: &Frame) -> Result<Frame, Error> {
    match frame {
        Frame::HAProxyHello { header, content: _ } => {
            // TODO: consider provided supported versions...
            // let supported_versions = content.get("supported-versions").unwrap();

            let mut response_content = HashMap::<String, TypedData>::new();
            response_content.insert("version".to_string(), TypedData::STRING("2.0".to_string()));
            response_content.insert("max-frame-size".to_string(), TypedData::UINT32(16380_u32));
            response_content.insert(
                "capabilities".to_string(),
                TypedData::STRING("pipelining,async".to_string()),
            );

            Ok(Frame::AgentHello {
                header: header.reply_header(&FrameType::AGENT_HELLO),
                content: response_content,
            })
        }
        Frame::Notify { header, messages } => {
            handle_notify(header, messages).map(|rep| {
                match rep {
                    Some(response_frame) => response_frame,
                    None => {
                        // basic ACK without action
                        let no_actions: Vec<Action> = vec![];
                        Frame::Ack {
                            header: header.reply_header(&FrameType::ACK),
                            actions: no_actions,
                        }
                    }
                }
            })
        }
        Frame::HAProxyDisconnect {
            header: _,
            content: _,
        } => Err(Error::Disconnect),
        _ => Err(Error::NotSupported),
    }
}

async fn process(socket: TcpStream) {
    // The `Connection` lets us read/write redis **frames** instead of
    // byte streams. The `Connection` type is defined by mini-redis.
    let mut connection = Connection::new(socket);

    loop {
        if let Some(frame) = connection.read_frame().await.unwrap() {
            println!("GOT: {:?}", frame);

            match handle_frame(&frame) {
                Ok(response) => {
                    println!("REP: {:?}", response);
                    connection.write_frame(&response).await.unwrap();
                }
                Err(Error::Disconnect) => {
                    println!("Disconnecting");
                    // break the loop
                    return;
                }
                Err(err) => {
                    println!("ERR: {:?}", err);
                }
            }
        }
    }
}
