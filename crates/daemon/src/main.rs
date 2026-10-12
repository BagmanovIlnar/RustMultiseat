//! Multiseat daemon entrypoint (foreground or Windows SCM service).

use std::path::PathBuf;
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
#[command(name = "multiseat-daemon", about = "RustMultiseat daemon")]
struct Args {
    /// Bind endpoint: TCP `host:port` or Windows Named Pipe `\\.\pipe\Name`.
    #[arg(long, default_value = DEFAULT_BIND)]
    bind: String,

    /// Directory for durable journal / runtime state.
    #[arg(long, default_value = "")]
    data_dir: String,

    /// Install the Windows service (requires elevation).
    #[arg(long)]
    install: bool,

    /// Remove the Windows service (requires elevation).
    #[arg(long)]
    uninstall: bool,

    /// Run under SCM (`StartServiceCtrlDispatcher`); used as ImagePath, not interactively.
    #[arg(long)]
    service: bool,

    /// Register with SERVICE_AUTO_START when installing.
    #[arg(long)]
    auto_start: bool,
}

fn resolve_data_dir(args: &Args) -> PathBuf {
    if !args.data_dir.is_empty() {
        return PathBuf::from(&args.data_dir);
    }
    #[cfg(windows)]
    {
        if args.service || args.install {
            return multiseat_daemon::default_service_data_dir();
        }
    }
    std::env::temp_dir().join("multiseat-daemon-dev")
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .init();
}

fn main() -> std::io::Result<()> {
    init_tracing();
    let args = Args::parse();
    let data_dir = resolve_data_dir(&args);

    if args.install {
        return run_install(&args.bind, &data_dir, args.auto_start);
    }
    if args.uninstall {
        return run_uninstall(&args.bind, &data_dir);
    }
    if args.service {
        // Must not run under a Tokio runtime: StartServiceCtrlDispatcher blocks.
        return run_scm_service(&args.bind, &data_dir);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(run_foreground(&args.bind, data_dir))
}

fn run_install(bind: &str, data_dir: &std::path::Path, auto_start: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let host =
            multiseat_daemon::ScmServiceHost::new(bind, data_dir).with_auto_start(auto_start);
        host.install()?;
        info!(?data_dir, "installed; start with: sc start RustMultiseat");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = (bind, data_dir, auto_start);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "service install requires Windows",
        ))
    }
}

fn run_uninstall(bind: &str, data_dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let host = multiseat_daemon::ScmServiceHost::new(bind, data_dir);
        host.uninstall()
    }
    #[cfg(not(windows))]
    {
        let _ = (bind, data_dir);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "service uninstall requires Windows",
        ))
    }
}

fn run_scm_service(bind: &str, data_dir: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let mut host = multiseat_daemon::ScmServiceHost::new(bind, data_dir);
        host.run()
    }
    #[cfg(not(windows))]
    {
        let _ = (bind, data_dir);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "SCM service mode requires Windows",
        ))
    }
}

async fn run_foreground(bind: &str, data_dir: PathBuf) -> std::io::Result<()> {
    let state = DaemonState::with_data_dir(data_dir);
    let mut host = ForegroundServiceHost::new();
    let stop = host.stop_flag();

    let bind = bind.to_string();
    let server_state = Arc::clone(&state);
    let server = tokio::spawn(async move { run_server(&bind, server_state).await });

    info!("press Ctrl+C to stop (foreground; not registered with SCM)");
    signal::ctrl_c().await?;
    let _ = host.handle_control(ServiceControl::Stop);
    stop.store(true, Ordering::SeqCst);
    host.shutdown()?;

    let _ = state.handle(multiseat_core::ClientRequest::Stop).await;
    server.abort();
    Ok(())
}
