//! Multiseat daemon entrypoint (foreground mock service).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use clap::Parser;
use multiseat_daemon::{
    run_server, DaemonState, ForegroundServiceHost, ServiceControl, WindowsServiceHost,
    DEFAULT_BIND,
};
use tokio::signal;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(name = "multiseat-daemon", about = "RustMultiseat mock daemon")]
struct Args {
    /// Bind endpoint: TCP `host:port` or Windows Named Pipe `\\.\pipe\Name`.
    #[arg(long, default_value = DEFAULT_BIND)]
    bind: String,

    /// Directory for durable journal / runtime state.
    #[arg(long, default_value = "")]
    data_dir: String,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();

    let args = Args::parse();
    let state = if args.data_dir.is_empty() {
        DaemonState::with_data_dir(std::env::temp_dir().join("multiseat-daemon-dev"))
    } else {
        DaemonState::with_data_dir(&args.data_dir)
    };

    let mut host = ForegroundServiceHost::new();
    let stop = host.stop_flag();

    let bind = args.bind.clone();
    let server_state = Arc::clone(&state);
    let server = tokio::spawn(async move { run_server(&bind, server_state).await });

    info!("press Ctrl+C to stop");
    signal::ctrl_c().await?;
    let _ = host.handle_control(ServiceControl::Stop);
    stop.store(true, Ordering::SeqCst);
    host.shutdown()?;

    // Best-effort stop of active mock session before exit.
    let _ = state.handle(multiseat_core::ClientRequest::Stop).await;

    server.abort();
    Ok(())
}
