//! Blocking daemon RPC bridged into Iced tasks via a worker thread.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use bytes::BytesMut;
use iced::Subscription;
use multiseat_core::{ClientRequest, DaemonEvent, ServerResponse, PROTOCOL_VERSION};
use multiseat_daemon::DaemonClientSync;
use multiseat_ipc::{
    decode_frame, decode_payload, encode_frame, encode_payload, MessageKind,
};

/// Runs one framed request on a background thread (Named Pipe / TCP).
pub async fn request_async(
    addr: String,
    request: ClientRequest,
) -> Result<ServerResponse, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut client = DaemonClientSync::connect(&addr).map_err(|e| e.to_string())?;
            client.request(&request).map_err(|e| e.to_string())
        })();
        let _ = tx.send(result);
    });
    match rx.recv() {
        Ok(result) => result,
        Err(err) => Err(err.to_string()),
    }
}

/// Long-lived subscription: Hello → Subscribe → push [`DaemonEvent`]s.
///
/// Uses a dedicated OS thread so the Iced UI never blocks on Named Pipe / TCP
/// reads. On disconnect the worker exits; the UI falls back to TickRefresh.
pub fn event_subscription(addr: String) -> Subscription<DaemonEvent> {
    Subscription::run_with(addr, event_stream)
}

// iced `Subscription::run_with` passes `&D` where `D: String` for restart keys.
#[allow(clippy::ptr_arg)]
fn event_stream(addr: &String) -> impl iced::futures::Stream<Item = DaemonEvent> {
    use iced::futures::SinkExt;

    let addr = addr.clone();
    // Same `async |mut output|` shape as tray.rs — IntelliJ/RA resolve
    // iced 0.14 `AsyncFnOnce` cleanly; sync-closure + inner async often red-squiggles.
    iced::stream::channel(32, async move |mut output| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<DaemonEvent>(64);
        let worker_addr = addr.clone();
        let spawn = std::thread::Builder::new()
            .name("multiseat-gui-events".into())
            .spawn(move || {
                if let Err(err) = event_worker(worker_addr, tx) {
                    tracing::debug!(error = %err, "GUI event worker stopped");
                }
            });
        if spawn.is_err() {
            return;
        }

        loop {
            while let Ok(event) = rx.try_recv() {
                let _ = output.send(event).await;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
}

fn event_worker(
    addr: String,
    tx: std::sync::mpsc::SyncSender<DaemonEvent>,
) -> Result<(), String> {
    // Dedicated connection (not DaemonClientSync::request loop): after Subscribe
    // the server pushes MessageKind::Event frames on the same socket.
    let mut stream = open_tcp_or_pipe(&addr)?;
    hello_and_subscribe(stream.as_mut())?;

    let mut buf = BytesMut::with_capacity(8 * 1024);
    let mut scratch = [0u8; 8192];
    loop {
        let n = stream.read(&mut scratch).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("daemon closed event stream".into());
        }
        buf.extend_from_slice(&scratch[..n]);
        loop {
            match decode_frame(&mut buf).map_err(|e| e.to_string())? {
                Some(frame) if frame.kind == MessageKind::Event => {
                    let event: DaemonEvent =
                        decode_payload(&frame.payload).map_err(|e| e.to_string())?;
                    if tx.send(event).is_err() {
                        return Ok(());
                    }
                }
                Some(frame) if frame.kind == MessageKind::Response => {
                    // Subscribe ack (or stray response) — ignore payload.
                    let _: ServerResponse =
                        decode_payload(&frame.payload).map_err(|e| e.to_string())?;
                }
                Some(_) => continue,
                None => break,
            }
        }
    }
}

fn hello_and_subscribe(stream: &mut dyn ReadWrite) -> Result<(), String> {
    let mut next_id = 1u64;
    write_request(
        stream,
        &mut next_id,
        &ClientRequest::Hello {
            client_version: PROTOCOL_VERSION,
        },
    )?;
    let hello = read_response(stream)?;
    match hello {
        ServerResponse::HelloAck { .. } => {}
        other => return Err(format!("hello failed: {other:?}")),
    }
    write_request(stream, &mut next_id, &ClientRequest::Subscribe)?;
    let sub = read_response(stream)?;
    match sub {
        ServerResponse::Ok => Ok(()),
        other => Err(format!("subscribe failed: {other:?}")),
    }
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

fn write_request(
    stream: &mut dyn ReadWrite,
    next_id: &mut u64,
    req: &ClientRequest,
) -> Result<(), String> {
    let request_id = *next_id;
    *next_id = next_id.saturating_add(1);
    let payload = encode_payload(req).map_err(|e| e.to_string())?;
    let mut out = BytesMut::new();
    encode_frame(MessageKind::Request, 0, request_id, &payload, &mut out)
        .map_err(|e| e.to_string())?;
    stream.write_all(&out).map_err(|e| e.to_string())
}

fn read_response(stream: &mut dyn ReadWrite) -> Result<ServerResponse, String> {
    let mut buf = BytesMut::with_capacity(8 * 1024);
    let mut scratch = [0u8; 8192];
    loop {
        let n = stream.read(&mut scratch).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("eof waiting for response".into());
        }
        buf.extend_from_slice(&scratch[..n]);
        match decode_frame(&mut buf).map_err(|e| e.to_string())? {
            Some(frame) if frame.kind == MessageKind::Response => {
                return decode_payload(&frame.payload).map_err(|e| e.to_string());
            }
            Some(_) => continue,
            None => continue,
        }
    }
}

fn open_tcp_or_pipe(addr: &str) -> Result<Box<dyn ReadWrite + Send>, String> {
    if multiseat_ipc::is_named_pipe_path(addr) {
        #[cfg(windows)]
        {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(addr)
                .map_err(|e| e.to_string())?;
            return Ok(Box::new(file));
        }
        #[cfg(not(windows))]
        {
            return Err("Named Pipe requires Windows".into());
        }
    }
    let stream = TcpStream::connect(addr).map_err(|e| e.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    Ok(Box::new(stream))
}
