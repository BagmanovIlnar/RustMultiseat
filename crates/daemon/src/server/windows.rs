//! Windows Named Pipe accept loop + client impersonation.

use std::os::windows::io::AsRawHandle;
use std::sync::Arc;

use multiseat_ipc::transport::named_pipe::PipeAcl;
use multiseat_ipc::ClientIdentity;
use tokio::net::windows::named_pipe::ServerOptions;
use tracing::{info, warn};

use super::handle_connection;
use crate::handler::DaemonState;

/// Accepts Named Pipe clients with local ACL and SID checks.
pub async fn run_named_pipe_server(
    pipe_name: &str,
    state: Arc<DaemonState>,
) -> std::io::Result<()> {
    info!(%pipe_name, transport = "named_pipe", "multiseat daemon listening");

    let mut first = true;
    loop {
        let mut acl = PipeAcl::local_daemon()?;
        let attrs = acl.as_security_attributes_ptr();
        let mut options = ServerOptions::new();
        options.first_pipe_instance(first);
        options.reject_remote_clients(true);
        let server = unsafe { options.create_with_security_attributes_raw(pipe_name, attrs) }?;
        drop(acl);
        first = false;

        server.connect().await?;
        info!(%pipe_name, "named pipe client connected");

        let raw = server.as_raw_handle() as isize;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let identity = resolve_pipe_identity(raw);
            if let Err(err) = handle_connection(server, state, identity, Some(raw)).await {
                warn!(error = %err, "named pipe session ended with error");
            }
        });
    }
}

/// Refreshes identity from the server-end pipe handle (impersonation).
pub fn resolve_pipe_identity(raw_handle: isize) -> ClientIdentity {
    match multiseat_ipc::identity_from_named_pipe(raw_handle) {
        Ok(id) => {
            info!(
                sid = ?id.sid,
                admin = id.is_admin,
                system = id.is_local_system,
                "named pipe client identity"
            );
            id
        }
        Err(err) => {
            warn!(error = %err, "named pipe impersonation failed; privileged commands denied");
            ClientIdentity::unverified_pipe()
        }
    }
}
