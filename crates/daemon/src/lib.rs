//! Daemon runtime library used by the service binary and tests.

#![deny(missing_docs)]

pub mod agents;
pub mod client;
pub mod client_sync;
pub mod filter_watch;
pub mod handler;
pub mod persist;
pub mod sample_config;
pub mod server;
pub mod service;

pub use agents::{AgentRegistry, DEFAULT_AGENT_TIMEOUT, DEFAULT_HEARTBEAT_INTERVAL};
pub use client::DaemonClient;
pub use client_sync::DaemonClientSync;
pub use filter_watch::{
    set_inject_unhealthy, FilterWatchAction, FilterWatchdog, DEFAULT_FILTER_FAIL_THRESHOLD,
    FILTER_FAIL_THRESHOLD_ENV, FILTER_HEARTBEAT_FAIL_ENV,
};
pub use handler::{DaemonState, AGENT_HEARTBEAT_ENV, AGENT_TIMEOUT_ENV};
pub use persist::{FileConfigStore, FileJournalStore};
pub use sample_config::lab_sample_config;
pub use server::{run_server, DEFAULT_BIND};
pub use service::{ForegroundServiceHost, ServiceControl, WindowsServiceHost, PIPE_NAME};

#[cfg(windows)]
pub use service::{default_service_data_dir, ScmServiceHost, SERVICE_DISPLAY_NAME, SERVICE_NAME};
