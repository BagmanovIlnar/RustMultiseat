//! Daemon IPC listener: TCP (dev/CI) or Windows Named Pipe (production default).
//!
//! Frame codec is identical on both transports.

use std::sync::Arc;

use bytes::BytesMut;
use multiseat_core::{ClientRequest, DaemonEvent, ServerResponse};
use multiseat_ipc::{
    decode_frame, decode_payload, encode_frame, encode_payload, is_named_pipe_path, MessageKind,
    MAX_FRAME_PAYLOAD,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::handler::DaemonState;

/// Default bind for CLI (`clap`); Windows → Named Pipe, elsewhere → TCP.
#[cfg(windows)]
pub const DEFAULT_BIND: &str = multiseat_ipc::WINDOWS_PIPE_NAME;
/// Default bind for CLI (`clap`); Windows → Named Pipe, elsewhere → TCP.
#[cfg(not(windows))]
pub const DEFAULT_BIND: &str = multiseat_ipc::DEFAULT_TCP_ENDPOINT;

/// Runs the accept loop until the process is cancelled.
pub async fn run_server(bind: &str, state: Arc<DaemonState>) -> std::io::Result<()> {
    if is_named_pipe_path(bind) {
        #[cfg(windows)]
        {
            return run_named_pipe_server(bind, state).await;
        }
        #[cfg(not(windows))]
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Named Pipe listener requires Windows",
            ));
        }
    }
    run_tcp_server(bind, state).await
}

async fn run_tcp_server(bind: &str, state: Arc<DaemonState>) -> std::io::Result<()> {
    let listener = TcpListener::bind(bind).await?;
    info!(%bind, transport = "tcp", "multiseat daemon listening");

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

#[cfg(windows)]
async fn run_named_pipe_server(pipe_name: &str, state: Arc<DaemonState>) -> std::io::Result<()> {
    use multiseat_ipc::transport::named_pipe::PipeAcl;
    use tokio::net::windows::named_pipe::ServerOptions;

    info!(%pipe_name, transport = "named_pipe", "multiseat daemon listening");

    let mut first = true;
    loop {
        let mut acl = PipeAcl::local_daemon()?;
        let attrs = acl.as_security_attributes_ptr();
        let mut options = ServerOptions::new();
        options.first_pipe_instance(first);
        options.reject_remote_clients(true);
        let server = unsafe { options.create_with_security_attributes_raw(pipe_name, attrs) }?;
        // SECURITY_ATTRIBUTES only needed during CreateNamedPipe.
        drop(acl);
        first = false;

        server.connect().await?;
        info!(%pipe_name, "named pipe client connected");

        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(err) = handle_connection(server, state).await {
                warn!(error = %err, "named pipe session ended with error");
            }
        });
    }
}

async fn handle_connection<S>(mut socket: S, state: Arc<DaemonState>) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
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

async fn write_response<S>(
    socket: &mut S,
    request_id: u64,
    response: &ServerResponse,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let payload = encode_payload(response)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut out = BytesMut::new();
    encode_frame(MessageKind::Response, 0, request_id, &payload, &mut out)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    socket.write_all(&out).await
}

async fn write_event<S>(socket: &mut S, event: &DaemonEvent) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let payload = encode_payload(event)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    let mut out = BytesMut::new();
    encode_frame(MessageKind::Event, 0, 0, &payload, &mut out)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    socket.write_all(&out).await
}
