//! Transport endpoints for framed IPC.
//!
//! Development and CI use TCP. Windows production will use Named Pipes with
//! the same frame codec.

/// Default development TCP endpoint.
pub const DEFAULT_TCP_ENDPOINT: &str = "127.0.0.1:17890";

/// Planned Windows Named Pipe path.
pub const WINDOWS_PIPE_NAME: &str = r"\\.\pipe\RustMultiseatDaemon";

/// Selected listener/client endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    /// TCP host:port.
    Tcp(String),
    /// Windows Named Pipe path.
    NamedPipe(String),
}

impl Endpoint {
    /// Chooses the platform-default endpoint.
    pub fn platform_default() -> Self {
        #[cfg(windows)]
        {
            // Prefer Named Pipe on Windows once the listener is implemented.
            // Until then TCP remains the active development transport.
            Self::Tcp(DEFAULT_TCP_ENDPOINT.into())
        }
        #[cfg(not(windows))]
        {
            Self::Tcp(DEFAULT_TCP_ENDPOINT.into())
        }
    }

    /// Returns a display string for logs/CLI.
    pub fn as_display(&self) -> &str {
        match self {
            Self::Tcp(addr) => addr,
            Self::NamedPipe(path) => path,
        }
    }
}

/// Named Pipe listener scaffold (Windows-only).
#[cfg(windows)]
pub mod named_pipe {
    use super::WINDOWS_PIPE_NAME;

    /// Creates a Named Pipe server with a restrictive ACL placeholder.
    ///
    /// This will call `CreateNamedPipeW` with an explicit SECURITY_ATTRIBUTES
    /// allowing the service SID, Administrators, and the interactive user.
    /// Remote clients must be rejected.
    pub fn create_server(_pipe_name: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("Named Pipe server not wired yet; planned path is {WINDOWS_PIPE_NAME}"),
        ))
    }

    /// Connects a client to the daemon pipe.
    pub fn connect_client(_pipe_name: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Named Pipe client not wired yet",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_default_is_tcp_for_now() {
        assert!(matches!(Endpoint::platform_default(), Endpoint::Tcp(_)));
    }
}
