//! Shared framed IPC client used by CLI and integration tests.

use bytes::BytesMut;
use multiseat_core::{ClientRequest, ServerResponse, PROTOCOL_VERSION};
use multiseat_ipc::{
    decode_frame, decode_payload, encode_frame, encode_payload, is_named_pipe_path, MessageKind,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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

enum ClientStream {
    Tcp(TcpStream),
    #[cfg(windows)]
    Pipe(tokio::net::windows::named_pipe::NamedPipeClient),
}

impl AsyncRead for ClientStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_read(cx, buf),
            #[cfg(windows)]
            Self::Pipe(s) => std::pin::Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ClientStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_write(cx, buf),
            #[cfg(windows)]
            Self::Pipe(s) => std::pin::Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_flush(cx),
            #[cfg(windows)]
            Self::Pipe(s) => std::pin::Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), std::io::Error>> {
        match self.get_mut() {
            Self::Tcp(s) => std::pin::Pin::new(s).poll_shutdown(cx),
            #[cfg(windows)]
            Self::Pipe(s) => std::pin::Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Framed IPC client with automatic Hello.
pub struct DaemonClient {
    stream: ClientStream,
    next_id: u64,
}

impl DaemonClient {
    /// Connects and completes protocol handshake (TCP or Named Pipe).
    pub async fn connect(addr: &str) -> Result<Self, ClientError> {
        let stream = connect_stream(addr).await?;
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

async fn connect_stream(addr: &str) -> Result<ClientStream, ClientError> {
    if is_named_pipe_path(addr) {
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            let client = ClientOptions::new().open(addr)?;
            return Ok(ClientStream::Pipe(client));
        }
        #[cfg(not(windows))]
        {
            return Err(ClientError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Named Pipe client requires Windows",
            )));
        }
    }
    let stream = TcpStream::connect(addr).await?;
    Ok(ClientStream::Tcp(stream))
}
