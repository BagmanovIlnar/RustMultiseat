//! Blocking framed IPC client for GUI / non-async callers.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use bytes::BytesMut;
use multiseat_core::{ClientRequest, ServerResponse, PROTOCOL_VERSION};
use multiseat_ipc::{
    decode_frame, decode_payload, encode_frame, encode_payload, is_named_pipe_path, MessageKind,
};

use crate::client::ClientError;

enum SyncStream {
    Tcp(TcpStream),
    #[cfg(windows)]
    Pipe(std::fs::File),
}

impl Read for SyncStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(buf),
            #[cfg(windows)]
            Self::Pipe(s) => s.read(buf),
        }
    }
}

impl Write for SyncStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(buf),
            #[cfg(windows)]
            Self::Pipe(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Tcp(s) => s.flush(),
            #[cfg(windows)]
            Self::Pipe(s) => s.flush(),
        }
    }
}

/// Blocking IPC client with automatic Hello.
pub struct DaemonClientSync {
    stream: SyncStream,
    next_id: u64,
}

impl DaemonClientSync {
    /// Connects, sets timeouts (TCP), and completes handshake.
    pub fn connect(addr: &str) -> Result<Self, ClientError> {
        let stream = open_stream(addr)?;
        let mut client = Self { stream, next_id: 1 };
        let ack = client.request(&ClientRequest::Hello {
            client_version: PROTOCOL_VERSION,
        })?;
        match ack {
            ServerResponse::HelloAck { .. } => Ok(client),
            other => Err(ClientError::Handshake(other)),
        }
    }

    /// Sends one request and waits for a response.
    pub fn request(&mut self, req: &ClientRequest) -> Result<ServerResponse, ClientError> {
        let request_id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);

        let payload = encode_payload(req)?;
        let mut out = BytesMut::new();
        encode_frame(MessageKind::Request, 0, request_id, &payload, &mut out)?;
        self.stream.write_all(&out)?;

        let mut buf = BytesMut::with_capacity(8 * 1024);
        let mut scratch = [0u8; 8192];
        loop {
            let n = self.stream.read(&mut scratch)?;
            if n == 0 {
                return Err(ClientError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "daemon closed connection",
                )));
            }
            buf.extend_from_slice(&scratch[..n]);
            match decode_frame(&mut buf)? {
                Some(frame) if frame.kind == MessageKind::Response => {
                    let response: ServerResponse = decode_payload(&frame.payload)?;
                    return Ok(response);
                }
                Some(_) => continue,
                None => continue,
            }
        }
    }
}

fn open_stream(addr: &str) -> Result<SyncStream, ClientError> {
    if is_named_pipe_path(addr) {
        #[cfg(windows)]
        {
            use std::fs::OpenOptions;
            let file = OpenOptions::new().read(true).write(true).open(addr)?;
            return Ok(SyncStream::Pipe(file));
        }
        #[cfg(not(windows))]
        {
            return Err(ClientError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Named Pipe client requires Windows",
            )));
        }
    }
    let stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    Ok(SyncStream::Tcp(stream))
}
