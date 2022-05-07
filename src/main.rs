use std::collections::HashMap;

use tokio::net::{TcpListener, TcpStream};

pub use connection::Connection;
use frame::{Action, Error, Frame, FrameFlags, FrameHeader, FrameType, TypedData};

use crate::frame::ActionVarScope;

mod connection;

mod frame;

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
                header: FrameHeader {
                    frame_id: header.frame_id,
                    stream_id: header.stream_id,
                    flags: FrameFlags::new(true, false),
                    r#type: FrameType::AGENT_HELLO,
                },
                content: response_content,
            })
        }
        Frame::Notify {
            header,
            messages: _,
        } => {
            let mut actions: Vec<Action> = vec![];
            actions.push(Action::SetVar {
                scope: ActionVarScope::REQUEST,
                name: "trace-id".to_string(),
                value: TypedData::STRING("aef34-x35-bb9".to_string()),
            });

            Ok(Frame::Ack {
                header: FrameHeader {
                    frame_id: header.frame_id,
                    stream_id: header.stream_id,
                    flags: FrameFlags::new(true, false),
                    r#type: FrameType::ACK,
                },
                actions,
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
