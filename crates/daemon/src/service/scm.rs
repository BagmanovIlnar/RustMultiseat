//! Windows SCM integration: install/uninstall + StartServiceCtrlDispatcher.
//!
//! Session 0 service process; no interactive UI. Local clients talk over the
//! Named Pipe (`PIPE_NAME` / `WINDOWS_PIPE_NAME`).

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tracing::{error, info, warn};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_CALL_NOT_IMPLEMENTED, NO_ERROR};
use windows::Win32::System::Services::{
    ChangeServiceConfig2W, CloseServiceHandle, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, RegisterServiceCtrlHandlerExW, SetServiceStatus, StartServiceCtrlDispatcherW,
    SC_ACTION, SC_ACTION_RESTART, SC_HANDLE, SC_MANAGER_ALL_ACCESS, SERVICE_ACCEPT_SHUTDOWN,
    SERVICE_ACCEPT_STOP, SERVICE_ALL_ACCESS, SERVICE_AUTO_START, SERVICE_CONFIG_FAILURE_ACTIONS,
    SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL,
    SERVICE_FAILURE_ACTIONSW, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS,
    SERVICE_STATUS_CURRENT_STATE, SERVICE_STATUS_HANDLE, SERVICE_STOPPED, SERVICE_STOP_PENDING,
    SERVICE_TABLE_ENTRYW, SERVICE_WIN32_OWN_PROCESS,
};

use super::{ServiceControl, WindowsServiceHost, PIPE_NAME};
use crate::handler::DaemonState;
use crate::server::run_server;

/// SCM service key / internal name.
pub const SERVICE_NAME: &str = "RustMultiseat";
/// User-visible display name in services.msc.
pub const SERVICE_DISPLAY_NAME: &str = "RustMultiseat Daemon";

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Raw `SERVICE_STATUS_HANDLE` stored as usize (SCM handle is process-local).
static STATUS_HANDLE_BITS: OnceLock<usize> = OnceLock::new();
static RUNTIME: OnceLock<ServiceRuntime> = OnceLock::new();

struct ServiceRuntime {
    bind: String,
    data_dir: PathBuf,
}

/// Default durable state directory under ProgramData.
pub fn default_service_data_dir() -> PathBuf {
    std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("RustMultiseat")
}

/// Builds the ImagePath registered with SCM (`"exe" --service --data-dir "…"`).
pub fn service_image_path(exe: &Path, data_dir: &Path) -> OsString {
    let mut cmd = OsString::new();
    cmd.push("\"");
    cmd.push(exe);
    cmd.push("\" --service --bind \"");
    cmd.push(PIPE_NAME);
    cmd.push("\" --data-dir \"");
    cmd.push(data_dir);
    cmd.push("\"");
    cmd
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn to_wide_os(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// SCM-backed host: install/uninstall via CreateService, run via dispatcher.
pub struct ScmServiceHost {
    stop: Arc<AtomicBool>,
    on_shutdown: Option<Box<dyn FnMut() + Send>>,
    bind: String,
    data_dir: PathBuf,
    /// When true, create with SERVICE_AUTO_START; otherwise demand-start.
    auto_start: bool,
}

impl ScmServiceHost {
    /// Creates a host that will listen on `bind` with state under `data_dir`.
    pub fn new(bind: impl Into<String>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            on_shutdown: None,
            bind: bind.into(),
            data_dir: data_dir.into(),
            auto_start: false,
        }
    }

    /// Prefer automatic start after boot.
    pub fn with_auto_start(mut self, auto_start: bool) -> Self {
        self.auto_start = auto_start;
        self
    }

    /// Shared stop flag (also mirrored into the SCM control handler).
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// True after Stop/Shutdown was observed.
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst) || STOP_REQUESTED.load(Ordering::SeqCst)
    }
}

impl WindowsServiceHost for ScmServiceHost {
    fn install(&self) -> std::io::Result<()> {
        install_service(&self.data_dir, self.auto_start)
    }

    fn uninstall(&self) -> std::io::Result<()> {
        uninstall_service()
    }

    fn run(&mut self) -> std::io::Result<()> {
        let _ = RUNTIME.set(ServiceRuntime {
            bind: self.bind.clone(),
            data_dir: self.data_dir.clone(),
        });
        STOP_REQUESTED.store(false, Ordering::SeqCst);
        dispatch_service()
    }

    fn handle_control(&mut self, control: ServiceControl) -> std::io::Result<()> {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                info!(?control, "SCM control received");
                STOP_REQUESTED.store(true, Ordering::SeqCst);
                self.stop.store(true, Ordering::SeqCst);
            }
            ServiceControl::DeviceEvent => {
                warn!("device event received; inventory rescan is deferred to enumerator");
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> std::io::Result<()> {
        info!("SCM shutdown: fail-open then stop ipc/agents");
        if let Some(hook) = self.on_shutdown.as_mut() {
            hook();
        }
        Ok(())
    }
}

fn install_service(data_dir: &Path, auto_start: bool) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let image = service_image_path(&exe, data_dir);
    let name = to_wide(SERVICE_NAME);
    let display = to_wide(SERVICE_DISPLAY_NAME);
    let bin = to_wide_os(&image);
    let start_type = if auto_start {
        SERVICE_AUTO_START
    } else {
        SERVICE_DEMAND_START
    };

    std::fs::create_dir_all(data_dir)?;

    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;

        let svc = CreateServiceW(
            scm,
            PCWSTR(name.as_ptr()),
            PCWSTR(display.as_ptr()),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            start_type,
            SERVICE_ERROR_NORMAL,
            PCWSTR(bin.as_ptr()),
            None,
            None,
            None,
            None,
            None,
        );

        match svc {
            Ok(handle) => {
                let _ = configure_restart_on_failure(handle);
                let _ = CloseServiceHandle(handle);
                let _ = CloseServiceHandle(scm);
                info!(
                    service = SERVICE_NAME,
                    ?image,
                    "service installed (Session 0; no UI)"
                );
                Ok(())
            }
            Err(err) => {
                let _ = CloseServiceHandle(scm);
                Err(std::io::Error::other(format!("CreateServiceW: {err}")))
            }
        }
    }
}

fn uninstall_service() -> std::io::Result<()> {
    let name = to_wide(SERVICE_NAME);
    unsafe {
        let scm = OpenSCManagerW(None, None, SC_MANAGER_ALL_ACCESS)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::PermissionDenied, e))?;
        let svc = OpenServiceW(scm, PCWSTR(name.as_ptr()), SERVICE_ALL_ACCESS);
        match svc {
            Ok(handle) => {
                let deleted = DeleteService(handle);
                let _ = CloseServiceHandle(handle);
                let _ = CloseServiceHandle(scm);
                deleted.map_err(|e| std::io::Error::other(format!("DeleteService: {e}")))?;
                info!(service = SERVICE_NAME, "service uninstalled");
                Ok(())
            }
            Err(err) => {
                let _ = CloseServiceHandle(scm);
                Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("OpenServiceW: {err}"),
                ))
            }
        }
    }
}

unsafe fn configure_restart_on_failure(service: SC_HANDLE) -> windows::core::Result<()> {
    let mut actions = [
        SC_ACTION {
            Type: SC_ACTION_RESTART,
            Delay: 5_000,
        },
        SC_ACTION {
            Type: SC_ACTION_RESTART,
            Delay: 30_000,
        },
        SC_ACTION {
            Type: SC_ACTION_RESTART,
            Delay: 60_000,
        },
    ];
    let mut failure = SERVICE_FAILURE_ACTIONSW {
        dwResetPeriod: 86_400,
        lpRebootMsg: PWSTR::null(),
        lpCommand: PWSTR::null(),
        cActions: actions.len() as u32,
        lpsaActions: actions.as_mut_ptr(),
    };
    ChangeServiceConfig2W(
        service,
        SERVICE_CONFIG_FAILURE_ACTIONS,
        Some((&mut failure as *mut SERVICE_FAILURE_ACTIONSW).cast()),
    )
}

fn dispatch_service() -> std::io::Result<()> {
    let mut name = to_wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR(name.as_mut_ptr()),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: PWSTR::null(),
            lpServiceProc: None,
        },
    ];
    unsafe {
        StartServiceCtrlDispatcherW(table.as_ptr()).map_err(|e| {
            std::io::Error::other(format!(
                "StartServiceCtrlDispatcherW: {e} (run with --service only under SCM)"
            ))
        })
    }
}

unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
    let name = to_wide(SERVICE_NAME);
    let handle = match RegisterServiceCtrlHandlerExW(
        PCWSTR(name.as_ptr()),
        Some(service_ctrl),
        None,
    ) {
        Ok(h) => h,
        Err(err) => {
            error!(error = %err, "RegisterServiceCtrlHandlerExW failed");
            return;
        }
    };
    let _ = STATUS_HANDLE_BITS.set(handle.0 as usize);
    report_status(SERVICE_START_PENDING, 3_000);

    let runtime = match RUNTIME.get() {
        Some(r) => r,
        None => {
            error!("service runtime config missing");
            report_status(SERVICE_STOPPED, 0);
            return;
        }
    };

    report_status(SERVICE_RUNNING, 0);
    info!(
        bind = %runtime.bind,
        data_dir = %runtime.data_dir.display(),
        "SCM service running"
    );

    let state = DaemonState::with_data_dir(&runtime.data_dir);
    let bind = runtime.bind.clone();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(err) => {
            error!(error = %err, "tokio runtime failed");
            report_status(SERVICE_STOPPED, 0);
            return;
        }
    };

    rt.block_on(async {
        let server_state = Arc::clone(&state);
        let server = tokio::spawn(async move { run_server(&bind, server_state).await });
        while !STOP_REQUESTED.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        report_status(SERVICE_STOP_PENDING, 5_000);
        let _ = state
            .handle(multiseat_core::ClientRequest::Stop)
            .await;
        server.abort();
    });

    report_status(SERVICE_STOPPED, 0);
    info!("SCM service stopped");
}

unsafe extern "system" fn service_ctrl(
    control: u32,
    _event_type: u32,
    _event_data: *mut core::ffi::c_void,
    _context: *mut core::ffi::c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            STOP_REQUESTED.store(true, Ordering::SeqCst);
            report_status(SERVICE_STOP_PENDING, 5_000);
            NO_ERROR.0
        }
        _ => ERROR_CALL_NOT_IMPLEMENTED.0,
    }
}

fn report_status(state: SERVICE_STATUS_CURRENT_STATE, wait_hint_ms: u32) {
    let Some(bits) = STATUS_HANDLE_BITS.get() else {
        return;
    };
    let handle = SERVICE_STATUS_HANDLE(*bits as *mut core::ffi::c_void);
    let accepts = SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN;
    let status = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: accepts,
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: wait_hint_ms,
    };
    unsafe {
        if let Err(err) = SetServiceStatus(handle, &status) {
            warn!(error = %err, "SetServiceStatus failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn image_path_quotes_exe_and_pipe() {
        let path = service_image_path(
            Path::new(r"C:\Program Files\RustMultiseat\multiseat-daemon.exe"),
            Path::new(r"C:\ProgramData\RustMultiseat"),
        );
        let s = path.to_string_lossy();
        assert!(s.contains("--service"));
        assert!(s.contains(PIPE_NAME));
        assert!(s.starts_with('"'));
    }

    #[test]
    fn default_data_dir_ends_with_product() {
        let dir = default_service_data_dir();
        assert!(dir.ends_with("RustMultiseat"));
    }
}
