use tokio::net::{TcpListener, TcpStream};

mod connection;

pub use connection::Connection;

mod frame;

use frame::{Frame, Error};
use crate::Frame::Ack;
use crate::frame::{FrameFlags, FrameHeader, FrameType};

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

fn handle_frame(frame: &Frame) -> Result<Frame, frame::Error> {
    match frame {
        Frame::HAProxyHello { header, content } => {
            Ok(Ack {
                header: FrameHeader {
                    frame_id: header.frame_id,
                    stream_id: header.stream_id,
                    flags: FrameFlags::new(true, false),
                    r#type: FrameType::ACK,
                }
            })
        }
        _ => Err(Error::NotSupported)
    }
}

async fn process(mut socket: TcpStream) {
    // The `Connection` lets us read/write redis **frames** instead of
    // byte streams. The `Connection` type is defined by mini-redis.
    let mut connection = Connection::new(socket);

    if let Some(frame) = connection.read_frame().await.unwrap() {
        println!("GOT: {:?}", frame);

        match handle_frame(&frame) {
            Ok(response) => {
                println!("REP: {:?}", response);
                connection.write_frame(&response).await.unwrap();
            }
            Err(err) => {
                println!("ERR: {:?}", err);
            }
        }
    }
}