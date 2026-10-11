//! Cross-platform TCP transport for the mock daemon.
//!
//! On Windows production builds this will be replaced/supplemented with a
//! Named Pipe listener. The frame codec stays identical.

use std::sync::Arc;

use bytes::BytesMut;
use multiseat_core::{ClientRequest, DaemonEvent, ServerResponse};
use multiseat_ipc::{
    decode_frame, decode_payload, encode_frame, encode_payload, MessageKind, MAX_FRAME_PAYLOAD,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::handler::DaemonState;

/// Default bind address for local development.
pub const DEFAULT_BIND: &str = "127.0.0.1:17890";

/// Runs the accept loop until the process is cancelled.
pub async fn run_server(bind: &str, state: Arc<DaemonState>) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    info!(%bind, "multiseat daemon listening");

    loop {
        let (socket, peer) = listener.accept().await?;
        info!(%peer, "client connected");
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(socket, state).await {
                warn!(error = %err, "client session ended with error");
            }
        });
    }
}

async fn handle_connection(mut socket: TcpStream, state: Arc<DaemonState>) -> std::io::Result<()> {
    let mut buf = BytesMut::with_capacity(8 * 1024);
    let mut events = state.events.subscribe();
    let mut subscribed = false;

    loop {
        tokio::select! {
            read = socket.read_buf(&mut buf) => {
                let n = read?;
                if n == 0 {
                    return Ok(());
                }
                if buf.len() > MAX_FRAME_PAYLOAD as usize + 1024 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "read buffer exceeded limit",
                    ));
                }

                loop {
                    match decode_frame(&mut buf) {
                        Ok(Some(frame)) => {
                            if frame.kind != MessageKind::Request {
                                warn!("ignoring non-request frame from client");
                                continue;
                            }
                            let request: ClientRequest = match decode_payload(&frame.payload) {
                                Ok(v) => v,
                                Err(err) => {
                                    let resp = ServerResponse::Err(
                                        multiseat_core::ProtocolError::CorruptPayload {
                                            message: err.to_string(),
                                        },
                                    );
                                    write_response(&mut socket, frame.request_id, &resp).await?;
                                    continue;
                                }
                            };

                            if matches!(request, ClientRequest::Subscribe) {
                                subscribed = true;
                            }

                            let response = state.handle(request).await;
                            write_response(&mut socket, frame.request_id, &response).await?;
                        }
                        Ok(None) => break,
                        Err(err) => {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                err.to_string(),
                            ));
                        }
                    }
                }
            }
            event = events.recv(), if subscribed => {
                match event {
                    Ok(event) => write_event(&mut socket, &event).await?,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "event subscriber lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
}

async fn write_response(
    socket: &mut TcpStream,
    request_id: u64,
    response: &ServerResponse,
) -> std::io::Result<()> {
    let payload = encode_payload(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut out = BytesMut::new();
    encode_frame(MessageKind::Response, 0, request_id, &payload, &mut out)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    socket.write_all(&out).await
}

async fn write_event(socket: &mut TcpStream, event: &DaemonEvent) -> std::io::Result<()> {
    let payload = encode_payload(event)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut out = BytesMut::new();
    encode_frame(MessageKind::Event, 0, 0, &payload, &mut out)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    socket.write_all(&out).await
}
