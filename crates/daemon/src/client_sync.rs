//! Blocking framed TCP client for GUI / non-async callers.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use bytes::BytesMut;
use multiseat_core::{ClientRequest, ServerResponse, PROTOCOL_VERSION};
use multiseat_ipc::{decode_frame, decode_payload, encode_frame, encode_payload, MessageKind};

use crate::client::ClientError;

/// Blocking TCP client with automatic Hello.
pub struct DaemonClientSync {
    stream: TcpStream,
    next_id: u64,
}

impl DaemonClientSync {
    /// Connects, sets timeouts, and completes handshake.
    pub fn connect(addr: &str) -> Result<Self, ClientError> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
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
