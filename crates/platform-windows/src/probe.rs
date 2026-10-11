//! Lightweight OS capability probe without WinAPI on non-Windows hosts.

use multiseat_core::PlatformCapabilities;

/// Detects best-effort platform capabilities for the current host.
pub fn detect_capabilities() -> PlatformCapabilities {
    #[cfg(windows)]
    {
        PlatformCapabilities {
            os: "windows".into(),
            os_version: windows_version_string(),
            // Real backends are not wired yet; keep false until Stage 0 succeeds.
            display_backend_available: false,
            session_backend_available: false,
            input_router_available: false,
            secure_boot_expected: true,
        }
    }

    #[cfg(not(windows))]
    {
        PlatformCapabilities {
            os: std::env::consts::OS.into(),
            os_version: "dev-host".into(),
            display_backend_available: false,
            session_backend_available: false,
            input_router_available: false,
            secure_boot_expected: false,
        }
    }
}

/// Returns `(major, minor, build)` when the host version can be read.
///
/// On Windows this uses `GetVersionExW` and may be compatibility-shimmed without
/// an embedded manifest; callers should treat it as a best-effort hint.
pub fn windows_version_numbers() -> Option<(u32, u32, u32)> {
    #[cfg(windows)]
    {
        use windows::Win32::System::SystemInformation::{
            GetVersionExW, OSVERSIONINFOEXW, OSVERSIONINFOW,
        };

        unsafe {
            let mut info = OSVERSIONINFOEXW {
                dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
                ..Default::default()
            };
            let as_osvi = &mut info as *mut OSVERSIONINFOEXW as *mut OSVERSIONINFOW;
            if GetVersionExW(as_osvi).is_ok() {
                return Some((
                    info.dwMajorVersion,
                    info.dwMinorVersion,
                    info.dwBuildNumber,
                ));
            }
        }
        None
    }

    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
fn windows_version_string() -> String {
    match windows_version_numbers() {
        Some((major, minor, build)) => {
            format!("{major}.{minor}.{build} (GetVersionExW; may be compatibility-shimmed)")
        }
        None => std::env::var("OS").unwrap_or_else(|_| "windows".into()),
    }
}
