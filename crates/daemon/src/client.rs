//! Shared framed TCP client used by CLI and integration tests.

use bytes::BytesMut;
use multiseat_core::{ClientRequest, ServerResponse, PROTOCOL_VERSION};
use multiseat_ipc::{decode_frame, decode_payload, encode_frame, encode_payload, MessageKind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

/// Errors talking to the daemon.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// IO failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Framing/codec failure.
    #[error(transparent)]
    Frame(#[from] multiseat_ipc::FrameError),
    /// Handshake rejected.
    #[error("handshake failed: {0:?}")]
    Handshake(ServerResponse),
    /// Timed out waiting for a response.
    #[error("timed out waiting for daemon response")]
    Timeout,
}

/// Framed TCP client with automatic Hello.
pub struct DaemonClient {
    stream: TcpStream,
    next_id: u64,
}

impl DaemonClient {
    /// Connects and completes protocol handshake.
    pub async fn connect(addr: &str) -> Result<Self, ClientError> {
        let stream = TcpStream::connect(addr).await?;
        let mut client = Self { stream, next_id: 1 };
        let ack = client
            .request(&ClientRequest::Hello {
                client_version: PROTOCOL_VERSION,
            })
            .await?;
        match ack {
            ServerResponse::HelloAck { .. } => Ok(client),
            other => Err(ClientError::Handshake(other)),
        }
    }

    /// Sends a request and waits for the correlated response.
    pub async fn request(&mut self, req: &ClientRequest) -> Result<ServerResponse, ClientError> {
        let request_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let payload = encode_payload(req)?;
        let mut out = BytesMut::new();
        encode_frame(MessageKind::Request, 0, request_id, &payload, &mut out)?;
        self.stream.write_all(&out).await?;

        let mut buf = BytesMut::with_capacity(8 * 1024);
        let read_loop = async {
            loop {
                self.stream.read_buf(&mut buf).await?;
                match decode_frame(&mut buf)? {
                    Some(frame) if frame.kind == MessageKind::Response => {
                        let response: ServerResponse = decode_payload(&frame.payload)?;
                        return Ok(response);
                    }
                    Some(_) => continue,
                    None => continue,
                }
            }
        };

        match timeout(Duration::from_secs(5), read_loop).await {
            Ok(result) => result,
            Err(_) => Err(ClientError::Timeout),
        }
    }
}
