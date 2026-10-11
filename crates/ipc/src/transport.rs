//! Transport endpoints for framed IPC.
//!
//! - Non-Windows / `MULTISEAT_IPC=tcp`: TCP `127.0.0.1:17890`
//! - Windows default: Named Pipe `\\.\pipe\RustMultiseatDaemon` with local ACL

/// Default development TCP endpoint.
pub const DEFAULT_TCP_ENDPOINT: &str = "127.0.0.1:17890";

/// Windows Named Pipe path.
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
    ///
    /// Override with `MULTISEAT_IPC=tcp` to force the TCP loopback endpoint.
    pub fn platform_default() -> Self {
        if std::env::var_os("MULTISEAT_IPC").is_some_and(|v| v == "tcp") {
            return Self::Tcp(DEFAULT_TCP_ENDPOINT.into());
        }

        #[cfg(windows)]
        {
            Self::NamedPipe(WINDOWS_PIPE_NAME.into())
        }
        #[cfg(not(windows))]
        {
            Self::Tcp(DEFAULT_TCP_ENDPOINT.into())
        }
    }

    /// Parses a bind/connect string into an endpoint.
    pub fn parse(s: &str) -> Self {
        if is_named_pipe_path(s) {
            Self::NamedPipe(s.to_string())
        } else {
            Self::Tcp(s.to_string())
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

/// True when `s` looks like a Windows Named Pipe path.
pub fn is_named_pipe_path(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with(r"\\.\pipe\") || lower.starts_with(r"//./pipe/")
}

/// Default bind/connect string for CLI defaults.
pub fn default_endpoint_str() -> &'static str {
    #[cfg(windows)]
    {
        if std::env::var_os("MULTISEAT_IPC").is_some_and(|v| v == "tcp") {
            DEFAULT_TCP_ENDPOINT
        } else {
            WINDOWS_PIPE_NAME
        }
    }
    #[cfg(not(windows))]
    {
        DEFAULT_TCP_ENDPOINT
    }
}

/// Named Pipe ACL helpers (Windows-only).
#[cfg(windows)]
pub mod named_pipe {
    use std::ffi::c_void;
    use std::io;
    use std::ptr;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

    use super::WINDOWS_PIPE_NAME;

    /// SDDL: LocalSystem + Administrators + Interactive (no network SID).
    ///
    /// Remote clients are additionally rejected via `PIPE_REJECT_REMOTE_CLIENTS`.
    pub const LOCAL_DAEMON_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;IU)";

    /// Owns a security descriptor used as `SECURITY_ATTRIBUTES` for CreateNamedPipe.
    pub struct PipeAcl {
        descriptor: PSECURITY_DESCRIPTOR,
        attrs: SECURITY_ATTRIBUTES,
    }

    // SAFETY: PipeAcl is only moved, never shared across threads while CreateNamedPipe runs.
    unsafe impl Send for PipeAcl {}

    impl PipeAcl {
        /// Builds the restrictive local-daemon ACL.
        pub fn local_daemon() -> io::Result<Self> {
            Self::from_sddl(LOCAL_DAEMON_SDDL)
        }

        /// Parses an SDDL string into pipe security attributes.
        pub fn from_sddl(sddl: &str) -> io::Result<Self> {
            let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(wide.as_ptr()),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
            }

            let attrs = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };

            Ok(Self { descriptor, attrs })
        }

        /// Raw pointer suitable for `create_with_security_attributes_raw`.
        ///
        /// The pointer is valid for the lifetime of `self`.
        pub fn as_security_attributes_ptr(&mut self) -> *mut c_void {
            (&mut self.attrs as *mut SECURITY_ATTRIBUTES).cast()
        }
    }

    impl Drop for PipeAcl {
        fn drop(&mut self) {
            if !self.descriptor.0.is_null() {
                unsafe {
                    let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
                }
                self.descriptor = PSECURITY_DESCRIPTOR(ptr::null_mut());
                self.attrs.lpSecurityDescriptor = ptr::null_mut();
            }
        }
    }

    /// Creates a Named Pipe server instance with the local ACL.
    ///
    /// Prefer calling this from the daemon via Tokio `ServerOptions` and
    /// [`PipeAcl::as_security_attributes_ptr`].
    pub fn create_server(_pipe_name: &str) -> io::Result<()> {
        // Concrete async creation lives in the daemon (Tokio NamedPipeServer).
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "use daemon Named Pipe listener; ACL SDDL={LOCAL_DAEMON_SDDL} path={WINDOWS_PIPE_NAME}"
            ),
        ))
    }

    /// Opens a client handle path hint (sync open is done by callers).
    pub fn connect_client(pipe_name: &str) -> io::Result<String> {
        if pipe_name.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "empty pipe name",
            ));
        }
        Ok(pipe_name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pipe_paths() {
        assert!(is_named_pipe_path(r"\\.\pipe\RustMultiseatDaemon"));
        assert!(is_named_pipe_path(r"//./pipe/foo"));
        assert!(!is_named_pipe_path("127.0.0.1:17890"));
    }

    #[test]
    fn parse_splits_tcp_and_pipe() {
        assert!(matches!(
            Endpoint::parse("127.0.0.1:9"),
            Endpoint::Tcp(_)
        ));
        assert!(matches!(
            Endpoint::parse(r"\\.\pipe\x"),
            Endpoint::NamedPipe(_)
        ));
    }

    #[test]
    fn platform_default_respects_tcp_override() {
        std::env::set_var("MULTISEAT_IPC", "tcp");
        assert_eq!(
            Endpoint::platform_default(),
            Endpoint::Tcp(DEFAULT_TCP_ENDPOINT.into())
        );
        std::env::remove_var("MULTISEAT_IPC");
    }
}
