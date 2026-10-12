//! Blocking daemon RPC bridged into Iced tasks via a worker thread.

use multiseat_core::{ClientRequest, ServerResponse};
use multiseat_daemon::DaemonClientSync;

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
