//! Client identity and authorization policy for privileged IPC commands.

use multiseat_core::{ClientRequest, ProtocolError};

/// How the client reached the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// Loopback TCP (dev/CI).
    TcpLoopback,
    /// Windows Named Pipe.
    NamedPipe,
}

/// Authenticated (or best-effort) client identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    /// Transport used for this connection.
    pub transport: TransportKind,
    /// User SID string when known (`S-1-5-…`).
    pub sid: Option<String>,
    /// Token is a member of Builtin Administrators (enabled or via elevation).
    pub is_admin: bool,
    /// Token user is Local System (`S-1-5-18`).
    pub is_local_system: bool,
    /// Identity capture failed; privileged Named Pipe commands must be denied.
    pub unverified: bool,
}

impl ClientIdentity {
    /// Dev/CI TCP clients on loopback — privileged commands allowed.
    pub fn tcp_loopback() -> Self {
        Self {
            transport: TransportKind::TcpLoopback,
            sid: None,
            is_admin: true,
            is_local_system: false,
            unverified: false,
        }
    }

    /// Named Pipe client whose token could not be read.
    pub fn unverified_pipe() -> Self {
        Self {
            transport: TransportKind::NamedPipe,
            sid: None,
            is_admin: false,
            is_local_system: false,
            unverified: true,
        }
    }

    /// True when privileged Named Pipe / service operations are allowed.
    pub fn may_mutate(&self) -> bool {
        match self.transport {
            TransportKind::TcpLoopback => !self.unverified,
            TransportKind::NamedPipe => {
                !self.unverified && (self.is_admin || self.is_local_system)
            }
        }
    }
}

/// Commands that change multiseat runtime / durable config.
pub fn is_privileged_request(request: &ClientRequest) -> bool {
    matches!(
        request,
        ClientRequest::SaveConfig(_)
            | ClientRequest::Start(_)
            | ClientRequest::Stop
            | ClientRequest::AcknowledgeEmergency
    )
}

/// Enforces authorization before the daemon handler runs.
pub fn authorize(
    identity: &ClientIdentity,
    request: &ClientRequest,
) -> Result<(), ProtocolError> {
    if !is_privileged_request(request) {
        return Ok(());
    }
    if identity.may_mutate() {
        return Ok(());
    }
    Err(ProtocolError::Unauthorized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use multiseat_core::MultiseatConfig;

    #[test]
    fn read_only_always_allowed() {
        let id = ClientIdentity::unverified_pipe();
        assert!(authorize(&id, &ClientRequest::GetSnapshot).is_ok());
        assert!(authorize(&id, &ClientRequest::Hello { client_version: 1 }).is_ok());
    }

    #[test]
    fn privileged_denied_for_unverified_pipe() {
        let id = ClientIdentity::unverified_pipe();
        assert_eq!(
            authorize(&id, &ClientRequest::Stop),
            Err(ProtocolError::Unauthorized)
        );
    }

    #[test]
    fn privileged_allowed_for_admin_pipe() {
        let id = ClientIdentity {
            transport: TransportKind::NamedPipe,
            sid: Some("S-1-5-32-544".into()),
            is_admin: true,
            is_local_system: false,
            unverified: false,
        };
        assert!(authorize(&id, &ClientRequest::Stop).is_ok());
        assert!(authorize(&id, &ClientRequest::Start(MultiseatConfig::default())).is_ok());
    }

    #[test]
    fn privileged_allowed_for_tcp_loopback() {
        let id = ClientIdentity::tcp_loopback();
        assert!(authorize(&id, &ClientRequest::Stop).is_ok());
    }
}
