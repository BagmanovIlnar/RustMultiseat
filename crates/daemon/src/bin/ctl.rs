//! Simple CLI client for the mock daemon.

use clap::{Parser, Subcommand};
use multiseat_core::{ClientRequest, ServerResponse};
use multiseat_daemon::{lab_sample_config, DaemonClient, DEFAULT_BIND};

#[derive(Debug, Parser)]
#[command(name = "multiseat-ctl", about = "RustMultiseat control CLI")]
struct Args {
    /// Daemon address.
    #[arg(long, default_value = DEFAULT_BIND)]
    addr: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Protocol handshake.
    Hello,
    /// Fetch hardware/config snapshot as JSON.
    Snapshot,
    /// Validate the built-in lab sample config.
    ValidateSample,
    /// Save the built-in lab sample config.
    SaveSample,
    /// Start using the built-in lab sample config.
    StartSample,
    /// Stop multiseat.
    Stop,
    /// Print hardware inventory summary from the daemon snapshot.
    Inventory,
    /// Print Stage 0 go/no-go feasibility report (JSON).
    LabReport,
    /// Run local WTS dual-session probe (no daemon required).
    SessionProbe,
    /// Run local display-model probe (no daemon required).
    DisplayProbe {
        /// Attached output count hint (default 2 for lab checklist).
        #[arg(long, default_value_t = 2)]
        outputs: usize,
    },
    /// Run local input-filter probe (no daemon required).
    InputProbe {
        /// Keyboard kit count hint.
        #[arg(long, default_value_t = 2)]
        keyboards: usize,
        /// Pointer kit count hint.
        #[arg(long, default_value_t = 2)]
        pointers: usize,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    match &args.command {
        Command::SessionProbe => {
            let result = multiseat_platform_windows::probe_dual_sessions();
            println!("status={:?} {}", result.status, result.detail());
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }
        Command::DisplayProbe { outputs } => {
            let result = multiseat_platform_windows::probe_display(*outputs);
            println!(
                "status={:?} backend={} {}",
                result.status,
                result.backend_name,
                result.detail()
            );
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }
        Command::InputProbe {
            keyboards,
            pointers,
        } => {
            let result = multiseat_platform_windows::probe_input(*keyboards, *pointers);
            println!(
                "status={:?} filter={} fail_open={} {}",
                result.status,
                result.filter_installed,
                result.fail_open_ready,
                result.detail()
            );
            println!("{}", serde_json::to_string_pretty(&result)?);
            return Ok(());
        }
        _ => {}
    }

    let mut client = DaemonClient::connect(&args.addr).await?;

    let response = match args.command {
        Command::Hello => {
            println!("handshake already completed by DaemonClient::connect");
            return Ok(());
        }
        Command::SessionProbe
        | Command::DisplayProbe { .. }
        | Command::InputProbe { .. } => unreachable!("handled before connect"),
        Command::Snapshot => client.request(&ClientRequest::GetSnapshot).await?,
        Command::ValidateSample => {
            client
                .request(&ClientRequest::ValidateConfig(lab_sample_config()))
                .await?
        }
        Command::SaveSample => {
            client
                .request(&ClientRequest::SaveConfig(lab_sample_config()))
                .await?
        }
        Command::StartSample => {
            client
                .request(&ClientRequest::Start(lab_sample_config()))
                .await?
        }
        Command::Stop => client.request(&ClientRequest::Stop).await?,
        Command::Inventory => {
            let response = client.request(&ClientRequest::GetSnapshot).await?;
            if let ServerResponse::Snapshot { hardware, .. } = &response {
                println!(
                    "revision={} gpus={} displays={} inputs={} usb_roots={} caps={:?}",
                    hardware.revision,
                    hardware.gpus.len(),
                    hardware.displays.len(),
                    hardware.input_devices.len(),
                    hardware.usb_roots.len(),
                    hardware.capabilities
                );
                for gpu in &hardware.gpus {
                    println!("  gpu: {} ({})", gpu.description, gpu.id);
                }
                for display in &hardware.displays {
                    println!(
                        "  display: {} {}x{} attached={}",
                        display.id,
                        display.width.unwrap_or(0),
                        display.height.unwrap_or(0),
                        display.attached
                    );
                }
                for input in &hardware.input_devices {
                    println!("  input: {:?} {}", input.kind, input.name);
                }
            }
            println!("{}", serde_json::to_string_pretty(&response)?);
            return Ok(());
        }
        Command::LabReport => {
            let response = client
                .request(&ClientRequest::GetFeasibilityReport)
                .await?;
            if let ServerResponse::Feasibility(report) = &response {
                println!("{}", report.summary);
                for check in &report.checks {
                    println!(
                        "[{:?}] {} — {}",
                        check.status, check.id, check.detail
                    );
                }
            }
            println!("{}", serde_json::to_string_pretty(&response)?);
            return Ok(());
        }
    };

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
