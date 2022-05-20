use std::env;
use tokio::net::{TcpListener, TcpStream};

pub use connection::Connection;
use frame::{Action, Error, Frame, FrameHeader, FrameType, KVList, ListOfMessages, TypedData};

mod connection;
mod frame;
mod otel;
mod proplists;

use otel::{handle_notify as otel_spoa_notify, init_tracer, new_otel_context, OtelContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::var_os("RUST_LOG").is_none() {
        // Set `RUST_LOG=jwtd=debug` to see debug logs,
        // info - only shows access logs.
        env::set_var("RUST_LOG", "haproxy_spoa_rust=debug");
    }
    pretty_env_logger::init_timed();

    let port = match env::var("PORT") {
        Ok(v) => v.parse::<u32>(),
        Err(e) => panic!("No port defined: {}", e),
    }
    .unwrap();
    let service_name = match env::var("SERVICE_NAME") {
        Ok(v) => v,
        Err(_) => "spoa".to_string(),
    };

    let addr = format!("0.0.0.0:{}", port);
    log::info!("Starting Agent on {}", addr);
    let listener = TcpListener::bind(addr).await?;

    let _ = init_tracer(service_name);
    let otel_ctx = new_otel_context();
    loop {
        let (socket, addr) = listener.accept().await?;
        log::info!("New socket opened from {:?}", addr);

        let otel_ctx: OtelContext = otel_ctx.clone();
        tokio::spawn(async move {
            // Process each socket concurrently.
            process(socket, otel_ctx, handle_notify).await
        });
    }
}

type NotifyHandler = fn(
    otel_ctx: &OtelContext,
    header: &FrameHeader,
    messages: &ListOfMessages,
) -> Result<Option<Frame>, Error>;

pub fn handle_notify(
    otel_ctx: &OtelContext,
    header: &FrameHeader,
    messages: &ListOfMessages,
) -> Result<Option<Frame>, Error> {
    otel_spoa_notify(otel_ctx, header, messages).map(|actions_opt| {
        actions_opt.map(|actions| Frame::Ack {
            header: header.reply_header(&FrameType::ACK),
            actions,
        })
    })
}

fn handle_frame(
    frame: &Frame,
    otel_ctx: &OtelContext,
    notify_handler: NotifyHandler,
) -> Result<Frame, Error> {
    match frame {
        Frame::HAProxyHello { header, content: _ } => {
            // TODO: consider provided supported versions...
            // let supported_versions = content.get("supported-versions").unwrap();

            let mut response_content = KVList::new();
            response_content.push(("version".to_string(), TypedData::STRING("2.0".to_string())));
            response_content.push(("max-frame-size".to_string(), TypedData::UINT32(16380_u32)));
            response_content.push((
                "capabilities".to_string(),
                TypedData::STRING("pipelining".to_string()),
            ));

            Ok(Frame::AgentHello {
                header: header.reply_header(&FrameType::AGENT_HELLO),
                content: response_content,
            })
        }
        Frame::Notify { header, messages } => {
            notify_handler(otel_ctx, header, messages).map(|rep| {
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

async fn process(socket: TcpStream, otel_ctx: OtelContext, notify_handler: NotifyHandler) {
    // The `Connection` lets us read/write redis **frames** instead of
    // byte streams. The `Connection` type is defined by mini-redis.
    let mut connection = Connection::new(socket);

    loop {
        if let Some(frame) = connection.read_frame().await.unwrap() {
            log::debug!("Processing frame {:?}", frame);

            match handle_frame(&frame, &otel_ctx, notify_handler) {
                Ok(response) => {
                    log::debug!("Response {:?}", response);
                    connection.write_frame(&response).await.unwrap();
                }
                Err(Error::Disconnect) => {
                    log::info!("Disconnecting");
                    // break the loop
                    return;
                }
                Err(err) => {
                    log::error!("ERROR: {:?}", err);
                }
            }
        }
    }
}
