//! Windows Service host skeleton and foreground runner.
//!
//! Real SCM registration is Windows-only. Non-Windows builds expose the same
//! control surface so CI and local development share one entrypoint.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tracing::{info, warn};

/// Service control events that the host must honor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceControl {
    /// Stop requested by SCM or Ctrl+C.
    Stop,
    /// System shutdown.
    Shutdown,
    /// Device arrival/removal notification (Windows).
    DeviceEvent,
}

/// Minimal service host interface from `agent.md`.
pub trait WindowsServiceHost {
    /// Installs the service (installer path, not runtime).
    fn install(&self) -> std::io::Result<()>;

    /// Removes the service.
    fn uninstall(&self) -> std::io::Result<()>;

    /// Registers control handling and runs until stop.
    fn run(&mut self) -> std::io::Result<()>;

    /// Handles one control code.
    fn handle_control(&mut self, control: ServiceControl) -> std::io::Result<()>;

    /// Fail-open inputs, then stop agents/IPC.
    fn shutdown(&mut self) -> std::io::Result<()>;
}

/// Foreground process host used for development and non-Windows CI.
pub struct ForegroundServiceHost {
    stop: Arc<AtomicBool>,
    /// Optional hook invoked on shutdown (fail-open / cleanup).
    on_shutdown: Option<Box<dyn FnMut() + Send>>,
}

impl ForegroundServiceHost {
    /// Creates a host with no shutdown hook.
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            on_shutdown: None,
        }
    }

    /// Creates a host with a shutdown callback.
    pub fn with_shutdown<F>(on_shutdown: F) -> Self
    where
        F: FnMut() + Send + 'static,
    {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
            on_shutdown: Some(Box::new(on_shutdown)),
        }
    }

    /// Shared stop flag for cooperative cancellation.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// Returns true after Stop/Shutdown was observed.
    pub fn should_stop(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

impl Default for ForegroundServiceHost {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsServiceHost for ForegroundServiceHost {
    fn install(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "service install is only implemented for Windows SCM packaging",
        ))
    }

    fn uninstall(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "service uninstall is only implemented for Windows SCM packaging",
        ))
    }

    fn run(&mut self) -> std::io::Result<()> {
        info!("foreground service host running (not registered with SCM)");
        while !self.should_stop() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.shutdown()
    }

    fn handle_control(&mut self, control: ServiceControl) -> std::io::Result<()> {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                info!(?control, "service control received");
                self.stop.store(true, Ordering::SeqCst);
            }
            ServiceControl::DeviceEvent => {
                warn!("device event received; inventory rescan is deferred to enumerator");
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> std::io::Result<()> {
        info!("service shutdown: fail-open then stop ipc/agents");
        if let Some(hook) = self.on_shutdown.as_mut() {
            hook();
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows_scm {
    //! Placeholder for future `windows-service` crate integration.
    //!
    //! Production path will register `service_main`, map STOP/SHUTDOWN to
    //! [`super::ServiceControl`], and never show UI from Session 0.

    /// Planned pipe name for local GUI/CLI clients.
    pub const PIPE_NAME: &str = r"\\.\pipe\RustMultiseatDaemon";
}

#[cfg(windows)]
pub use windows_scm::PIPE_NAME;
