//! KMDF filter presence probe for Stage 0 input isolation.

use multiseat_core::InputProbeResult;

/// Expected keyboard filter service name (future driver package).
#[cfg_attr(not(windows), allow(dead_code))]
pub const KEYBOARD_FILTER_SERVICE: &str = "RustMultiseatKbFilter";
/// Expected mouse filter service name (future driver package).
#[cfg_attr(not(windows), allow(dead_code))]
pub const POINTER_FILTER_SERVICE: &str = "RustMultiseatMouFilter";
/// Lab override: allow soft bookkeeping `apply` when filter presence is claimed (`1`).
pub const INPUT_SOFT_APPLY_ENV: &str = "MULTISEAT_INPUT_SOFT_APPLY";

/// Probes whether an input filter is installed and fail-open is the safe default.
///
/// Lab override: `MULTISEAT_INPUT_FILTER=1` marks the filter as present without
/// enabling apply (routing still requires a real Stage 0 PoC).
pub fn probe_input(keyboard_kits: usize, pointer_kits: usize) -> InputProbeResult {
    let filter_installed = filter_present();
    // Without an installed filter there is nothing suppressing host input.
    let fail_open_ready = true;
    InputProbeResult::evaluate(
        filter_installed,
        fail_open_ready,
        keyboard_kits,
        pointer_kits,
    )
}

fn filter_present() -> bool {
    if std::env::var_os("MULTISEAT_INPUT_FILTER").is_some_and(|v| v == "1") {
        return true;
    }

    #[cfg(windows)]
    {
        service_exists(KEYBOARD_FILTER_SERVICE) && service_exists(POINTER_FILTER_SERVICE)
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn service_exists(name: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
    };

    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let Ok(scm) = OpenSCManagerW(None, None, SC_MANAGER_CONNECT) else {
            return false;
        };
        let svc = OpenServiceW(scm, PCWSTR(wide.as_ptr()), SERVICE_QUERY_STATUS);
        let present = svc.is_ok();
        if let Ok(handle) = svc {
            let _ = CloseServiceHandle(handle);
        }
        let _ = CloseServiceHandle(scm);
        present
    }
}
