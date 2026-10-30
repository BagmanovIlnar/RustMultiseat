//! Simple CLI client for the mock daemon.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use multiseat_core::{
    ClientRequest, FailOpenProbeResult, FeasibilityReport, HardwareTree, InstallPlan,
    MultiseatConfig, SeatId, SeatState, ServerResponse, SoftInputE2eResult, SoftInputE2eStep,
    Stage0LabArtifact,
};
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
    /// Emit Stage 7 install/signing plan (JSON + optional Markdown artifact).
    InstallPlan {
        /// Directory for `install-plan.json` + `INSTALL_PLAN.md` (optional).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Override major.minor.build (lab / CI matrix), e.g. `10.0.22621`.
        #[arg(long)]
        pretend_version: Option<String>,
    },
    /// Print native Stage 0 PoC readiness (probes + soft-path flags + next actions).
    NativeStatus {
        /// Attached output count hint.
        #[arg(long, default_value_t = 2)]
        outputs: usize,
        /// Keyboard kit count hint.
        #[arg(long, default_value_t = 2)]
        keyboards: usize,
        /// Pointer kit count hint.
        #[arg(long, default_value_t = 2)]
        pointers: usize,
    },
    /// Run Stage 0 probes + feasibility and write a lab artifact (JSON + Markdown).
    ///
    /// Exit 0 when the artifact is written (even on NO-GO). Use `--require-go`
    /// to fail CI/lab gate when the host is not yet production-ready.
    LabChecklist {
        /// Directory for `stage0.json` + `REPORT.md` (created if missing).
        #[arg(long)]
        out: PathBuf,
        /// Skip daemon; build report from local probes + empty inventory scaffold.
        #[arg(long)]
        local_only: bool,
        /// Exit 1 unless `report.go == true` (Windows lab gate).
        #[arg(long)]
        require_go: bool,
        /// Skip Start → AgentFailOpen → Idle exercise (on by default when daemon is used).
        #[arg(long)]
        no_probe_fail_open: bool,
    },
    /// One-shot mock E2E: validate → save → start → (agent) → stop; exit 1 on failure.
    Smoke {
        /// Also send AgentHello + AgentHeartbeat for seat 1 while Active.
        #[arg(long)]
        with_agent: bool,
        /// After stop, run soft input-filter E2E (`MULTISEAT_INPUT_FILTER=1` recommended).
        #[arg(long)]
        with_soft_input: bool,
    },
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
    /// Force input-filter fail-open via control device stub (no daemon).
    InputFailOpen,
    /// Print input-filter control status + IOCTL table (no daemon).
    InputFilterStatus,
    /// Build SET_ROUTES preview from lab sample or a JSON config (no driver IOCTL).
    InputRoutesPreview {
        /// Path to `MultiseatConfig` JSON; default = built-in lab sample.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Also run a filter control heartbeat ping after printing routes.
        #[arg(long)]
        ping: bool,
    },
    /// Push SET_ROUTES via filter control and optionally arm (no daemon).
    ///
    /// Requires a present control path (`MULTISEAT_INPUT_FILTER=1` soft or native `.sys`).
    InputRoutesApply {
        /// Path to `MultiseatConfig` JSON; default = built-in lab sample.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Print the table only; do not call set_routes / arm.
        #[arg(long)]
        dry_run: bool,
        /// Push routes but keep fail-open (do not arm suppression).
        #[arg(long)]
        no_arm: bool,
    },
    /// Open filter control, query health; force fail-open if unhealthy (no daemon).
    InputFilterPing,
    /// Soft/native input E2E: preview → apply(--no-arm) → apply → ping → fail-open.
    ///
    /// Writes `soft_input_e2e.json` when `--out` is set. Requires present control
    /// (`MULTISEAT_INPUT_FILTER=1` or loaded driver).
    InputSoftE2e {
        /// Path to `MultiseatConfig` JSON; default = lab sample.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Directory for `soft_input_e2e.json` (optional).
        #[arg(long)]
        out: Option<PathBuf>,
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
        Command::InputFailOpen => {
            multiseat_platform_windows::emergency_fail_open()
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            println!("input-fail-open: ok (absent device ⇒ already fail-open)");
            return Ok(());
        }
        Command::InputFilterStatus => {
            let ctl = multiseat_platform_windows::InputFilterControl::open()
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            println!(
                "device={} present={} soft={} abi={}",
                multiseat_platform_windows::INPUT_FILTER_CONTROL_DEVICE,
                ctl.is_present(),
                ctl.is_soft(),
                multiseat_platform_windows::INPUT_FILTER_ABI_VERSION
            );
            for (name, code) in ctl.ioctl_table() {
                println!("  {name}=0x{code:08X}");
            }
            match ctl.get_version() {
                Ok(v) => println!(
                    "version: abi={} build={} caps=0x{:X}",
                    { v.abi_version },
                    { v.driver_build },
                    { v.caps_flags }
                ),
                Err(err) => println!("version: {err}"),
            }
            let stats = ctl
                .query_stats()
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            println!(
                "stats: seen={} passed={} suppressed={} overflows={} fail_open={}",
                { stats.packets_seen },
                { stats.packets_passed },
                { stats.packets_suppressed },
                { stats.queue_overflows },
                { stats.fail_open }
            );
            return Ok(());
        }
        Command::InputRoutesPreview { config, ping } => {
            run_input_routes_preview(config.as_ref(), *ping)?;
            return Ok(());
        }
        Command::InputRoutesApply {
            config,
            dry_run,
            no_arm,
        } => {
            run_input_routes_apply(config.as_ref(), *dry_run, !*no_arm)?;
            return Ok(());
        }
        Command::InputFilterPing => {
            let health = multiseat_platform_windows::filter_heartbeat()
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            println!(
                "present={} soft={} healthy={} fail_open={} forced_fail_open={} abi={:?}",
                health.present,
                health.soft,
                health.healthy,
                health.fail_open,
                health.forced_fail_open,
                health.abi_version
            );
            println!("detail: {}", health.detail);
            println!("{}", serde_json::to_string_pretty(&health)?);
            if !health.healthy {
                return Err("input-filter-ping: unhealthy".into());
            }
            return Ok(());
        }
        Command::InputSoftE2e { config, out } => {
            let result = run_input_soft_e2e(config.as_ref())?;
            println!("{}", result.detail());
            println!("{}", serde_json::to_string_pretty(&result)?);
            if let Some(dir) = out {
                std::fs::create_dir_all(dir)?;
                let path = dir.join("soft_input_e2e.json");
                std::fs::write(&path, serde_json::to_string_pretty(&result)?)?;
                println!("input-soft-e2e: wrote {}", path.display());
            }
            if !result.ok {
                return Err("input-soft-e2e: failed".into());
            }
            return Ok(());
        }
        Command::LabChecklist {
            out,
            local_only,
            require_go,
            no_probe_fail_open,
        } => {
            let do_fail_open = !*local_only && !*no_probe_fail_open;
            run_lab_checklist(&args.addr, out, *local_only, *require_go, do_fail_open).await?;
            return Ok(());
        }
        Command::NativeStatus {
            outputs,
            keyboards,
            pointers,
        } => {
            let status = multiseat_platform_windows::NativePocStatus::collect(
                *outputs, *keyboards, *pointers,
            );
            println!(
                "soft_display_attach={} soft_input_apply={} soft_backends_armed={}",
                status.soft_display_attach,
                status.soft_input_apply,
                status.soft_backends_armed()
            );
            for action in &status.next_actions {
                println!("next: {action}");
            }
            println!("{}", serde_json::to_string_pretty(&status)?);
            return Ok(());
        }
        Command::InstallPlan {
            out,
            pretend_version,
        } => {
            run_install_plan(out.as_ref(), pretend_version.as_deref())?;
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
        Command::Smoke {
            with_agent,
            with_soft_input,
        } => {
            run_smoke(&mut client, with_agent).await?;
            if with_soft_input {
                // Ensure soft control path for CI/dev hosts without a loaded .sys.
                std::env::set_var("MULTISEAT_INPUT_FILTER", "1");
                let result = run_input_soft_e2e(None)?;
                println!("smoke soft-input: {}", result.detail());
                if !result.ok {
                    return Err("smoke: soft input E2E failed".into());
                }
            }
            return Ok(());
        }
        Command::SessionProbe
        | Command::DisplayProbe { .. }
        | Command::InputProbe { .. }
        | Command::LabChecklist { .. }
        | Command::NativeStatus { .. }
        | Command::InstallPlan { .. }
        | Command::InputFailOpen
        | Command::InputFilterStatus
        | Command::InputRoutesPreview { .. }
        | Command::InputRoutesApply { .. }
        | Command::InputFilterPing
        | Command::InputSoftE2e { .. } => unreachable!("handled before connect"),
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
            let response = client.request(&ClientRequest::GetFeasibilityReport).await?;
            if let ServerResponse::Feasibility(report) = &response {
                println!("{}", report.summary);
                for check in &report.checks {
                    println!("[{:?}] {} — {}", check.status, check.id, check.detail);
                }
            }
            println!("{}", serde_json::to_string_pretty(&response)?);
            return Ok(());
        }
    };

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

/// Mock E2E path used by `scripts/mock_e2e_smoke.sh` and local CI.
///
/// Steps (any failure → non-zero exit via `Err`):
/// 1. Snapshot (inventory present)
/// 2. ValidateSample (0 errors)
/// 3. SaveSample
/// 4. StartSample → Snapshot state == Active
/// 5. Optional AgentHello + AgentHeartbeat for seat 1
/// 6. Stop → Snapshot state == Idle
async fn run_smoke(
    client: &mut DaemonClient,
    with_agent: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let step = |name: &str| println!("smoke: {name}");

    step("snapshot");
    let snap = client.request(&ClientRequest::GetSnapshot).await?;
    match &snap {
        ServerResponse::Snapshot {
            hardware, state, ..
        } => {
            println!(
                "  rev={} inputs={} displays={} state={state:?}",
                hardware.revision,
                hardware.input_devices.len(),
                hardware.displays.len()
            );
            if hardware.input_devices.is_empty() || hardware.displays.is_empty() {
                return Err("snapshot: expected mock inventory with inputs and displays".into());
            }
        }
        other => return Err(format!("snapshot: unexpected {other:?}").into()),
    }

    step("validate sample");
    let validation = client
        .request(&ClientRequest::ValidateConfig(lab_sample_config()))
        .await?;
    match validation {
        ServerResponse::Validation(report) if !report.has_errors() => {
            println!("  ok ({} issue(s))", report.issues.len());
        }
        ServerResponse::Validation(report) => {
            return Err(format!("validate: has errors: {report:?}").into());
        }
        other => return Err(format!("validate: unexpected {other:?}").into()),
    }

    step("save sample");
    expect_ok(
        client
            .request(&ClientRequest::SaveConfig(lab_sample_config()))
            .await?,
        "save",
    )?;

    step("start sample");
    expect_ok(
        client
            .request(&ClientRequest::Start(lab_sample_config()))
            .await?,
        "start",
    )?;

    step("assert Active");
    expect_state(client, SeatState::Active).await?;

    if with_agent {
        step("agent hello");
        let hello = client
            .request(&ClientRequest::AgentHello {
                seat_id: SeatId(1),
                agent_version: 1,
            })
            .await?;
        match hello {
            ServerResponse::AgentAck {
                seat_id,
                heartbeat_interval_ms,
            } => {
                println!("  seat={seat_id} heartbeat_ms={heartbeat_interval_ms}");
                if seat_id != SeatId(1) {
                    return Err("agent hello: seat mismatch".into());
                }
            }
            other => return Err(format!("agent hello: unexpected {other:?}").into()),
        }

        step("agent heartbeat");
        expect_ok(
            client
                .request(&ClientRequest::AgentHeartbeat { seat_id: SeatId(1) })
                .await?,
            "agent heartbeat",
        )?;
    }

    step("stop");
    expect_ok(client.request(&ClientRequest::Stop).await?, "stop")?;

    step("assert Idle");
    expect_state(client, SeatState::Idle).await?;

    println!("smoke: PASS");
    Ok(())
}

fn expect_ok(response: ServerResponse, step: &str) -> Result<(), Box<dyn std::error::Error>> {
    match response {
        ServerResponse::Ok => Ok(()),
        other => Err(format!("{step}: expected Ok, got {other:?}").into()),
    }
}

async fn expect_state(
    client: &mut DaemonClient,
    want: SeatState,
) -> Result<(), Box<dyn std::error::Error>> {
    let snap = client.request(&ClientRequest::GetSnapshot).await?;
    match snap {
        ServerResponse::Snapshot { state, .. } if state == want => {
            println!("  state={state:?}");
            Ok(())
        }
        ServerResponse::Snapshot { state, .. } => {
            Err(format!("expected state {want:?}, got {state:?}").into())
        }
        other => Err(format!("snapshot for state check: unexpected {other:?}").into()),
    }
}

/// Collects Stage 0 probes (+ optional daemon inventory / fail-open) into `out/`.
async fn run_lab_checklist(
    addr: &str,
    out: &PathBuf,
    local_only: bool,
    require_go: bool,
    probe_fail_open: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(out)?;

    let session = multiseat_platform_windows::probe_dual_sessions();
    println!(
        "lab: session status={:?} — {}",
        session.status,
        session.detail()
    );

    let (tree, display, input, daemon_addr, fail_open) = if local_only {
        let display = multiseat_platform_windows::probe_display(2);
        let input = multiseat_platform_windows::probe_input(2, 2);
        println!(
            "lab: display status={:?} backend={}",
            display.status, display.backend_name
        );
        println!(
            "lab: input status={:?} filter={} fail_open={}",
            input.status, input.filter_installed, input.fail_open_ready
        );
        (
            HardwareTree::empty(),
            display,
            input,
            None,
            Some(FailOpenProbeResult::skipped(
                "local-only checklist; automated fail-open requires daemon",
            )),
        )
    } else {
        let mut client = DaemonClient::connect(addr).await?;
        let snap = client.request(&ClientRequest::GetSnapshot).await?;
        let tree = match snap {
            ServerResponse::Snapshot { hardware, .. } => *hardware,
            other => return Err(format!("lab-checklist snapshot: unexpected {other:?}").into()),
        };
        let outputs = tree.displays.iter().filter(|d| d.attached).count();
        let keyboards = tree
            .input_devices
            .iter()
            .filter(|d| matches!(d.kind, multiseat_core::InputDeviceKind::Keyboard))
            .count();
        let pointers = tree
            .input_devices
            .iter()
            .filter(|d| matches!(d.kind, multiseat_core::InputDeviceKind::Pointer))
            .count();
        let display = multiseat_platform_windows::probe_display(outputs);
        let input = multiseat_platform_windows::probe_input(keyboards, pointers);
        println!(
            "lab: display status={:?} backend={}",
            display.status, display.backend_name
        );
        println!(
            "lab: input status={:?} filter={} fail_open={}",
            input.status, input.filter_installed, input.fail_open_ready
        );

        let fail_open = if probe_fail_open {
            Some(run_fail_open_probe(&mut client).await?)
        } else {
            Some(FailOpenProbeResult::skipped(
                "--no-probe-fail-open; orchestrator AgentFailOpen not exercised",
            ))
        };

        (tree, display, input, Some(addr.to_string()), fail_open)
    };

    let report = FeasibilityReport::from_hardware_and_probes_ext(
        &tree,
        Some(&session),
        Some(&display),
        Some(&input),
        fail_open.as_ref(),
    );

    let generated_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let artifact = Stage0LabArtifact {
        generated_at_unix_ms,
        host_os: std::env::consts::OS.to_string(),
        daemon_addr,
        session,
        display,
        input,
        fail_open,
        report,
    };

    let outputs = tree.displays.iter().filter(|d| d.attached).count().max(2);
    let keyboards = tree
        .input_devices
        .iter()
        .filter(|d| matches!(d.kind, multiseat_core::InputDeviceKind::Keyboard))
        .count()
        .max(2);
    let pointers = tree
        .input_devices
        .iter()
        .filter(|d| matches!(d.kind, multiseat_core::InputDeviceKind::Pointer))
        .count()
        .max(2);
    let native = multiseat_platform_windows::NativePocStatus::collect(outputs, keyboards, pointers);

    let json_path = out.join("stage0.json");
    let md_path = out.join("REPORT.md");
    let native_path = out.join("native_poc.json");
    std::fs::write(&json_path, serde_json::to_string_pretty(&artifact)?)?;
    std::fs::write(&md_path, artifact.to_markdown())?;
    std::fs::write(&native_path, serde_json::to_string_pretty(&native)?)?;

    println!("{}", artifact.report.summary);
    for check in &artifact.report.checks {
        println!("  [{:?}] {} — {}", check.status, check.id, check.detail);
    }
    println!(
        "lab: wrote {}, {}, and {}",
        json_path.display(),
        md_path.display(),
        native_path.display()
    );
    println!(
        "lab: native soft_display={} soft_input={} armed={}",
        native.soft_display_attach,
        native.soft_input_apply,
        native.soft_backends_armed()
    );

    if require_go && !artifact.report.go {
        return Err(format!(
            "lab-checklist: --require-go but report is NO-GO ({})",
            artifact.report.summary
        )
        .into());
    }
    Ok(())
}

fn load_routes_config(
    config_path: Option<&PathBuf>,
) -> Result<(String, MultiseatConfig), Box<dyn std::error::Error>> {
    if let Some(path) = config_path {
        let raw = std::fs::read_to_string(path)?;
        let cfg: MultiseatConfig = serde_json::from_str(&raw)?;
        Ok((path.display().to_string(), cfg))
    } else {
        Ok(("lab-sample".into(), lab_sample_config()))
    }
}

fn run_input_routes_preview(
    config_path: Option<&PathBuf>,
    ping: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (source, config) = load_routes_config(config_path)?;
    let table = multiseat_platform_windows::route_table_from_config(&config)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    table
        .validate()
        .map_err(|m| -> Box<dyn std::error::Error> { m.into() })?;
    println!("source={source}");
    print!(
        "{}",
        multiseat_platform_windows::format_route_table_preview(&table)
    );
    if ping {
        let health = multiseat_platform_windows::filter_heartbeat()
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
        println!(
            "ping: present={} soft={} healthy={} fail_open={} detail={}",
            health.present, health.soft, health.healthy, health.fail_open, health.detail
        );
        if !health.healthy {
            return Err("input-routes-preview --ping: filter unhealthy".into());
        }
    }
    Ok(())
}

fn run_input_soft_e2e(
    config_path: Option<&PathBuf>,
) -> Result<SoftInputE2eResult, Box<dyn std::error::Error>> {
    let mut steps = Vec::new();

    let push_step = |steps: &mut Vec<SoftInputE2eStep>, name: &str, ok: bool, detail: String| {
        steps.push(SoftInputE2eStep {
            name: name.into(),
            ok,
            detail,
        });
    };

    let (source, config) = match load_routes_config(config_path) {
        Ok(v) => v,
        Err(err) => {
            push_step(&mut steps, "load_config", false, err.to_string());
            return Ok(SoftInputE2eResult::from_steps(false, steps));
        }
    };

    let table = match multiseat_platform_windows::route_table_from_config(&config) {
        Ok(t) => t,
        Err(err) => {
            push_step(&mut steps, "build_routes", false, err.to_string());
            return Ok(SoftInputE2eResult::from_steps(false, steps));
        }
    };
    if let Err(err) = table.validate() {
        push_step(&mut steps, "validate_routes", false, err.into());
        return Ok(SoftInputE2eResult::from_steps(false, steps));
    }
    push_step(
        &mut steps,
        "preview",
        true,
        format!("source={source} entries={}", table.entry_count),
    );

    let ctl = match multiseat_platform_windows::InputFilterControl::open() {
        Ok(c) => c,
        Err(err) => {
            push_step(&mut steps, "open_control", false, err.to_string());
            return Ok(SoftInputE2eResult::from_steps(false, steps));
        }
    };
    let soft = ctl.is_soft();
    if !ctl.is_present() {
        push_step(
            &mut steps,
            "open_control",
            false,
            format!(
                "absent ({}); set MULTISEAT_INPUT_FILTER=1",
                multiseat_platform_windows::INPUT_FILTER_CONTROL_DEVICE
            ),
        );
        return Ok(SoftInputE2eResult::from_steps(soft, steps));
    }
    push_step(
        &mut steps,
        "open_control",
        true,
        format!("present soft={soft}"),
    );

    match ctl.apply_routes(&table, false) {
        Ok(()) => push_step(
            &mut steps,
            "apply_no_arm",
            true,
            format!("entries={}", table.entry_count),
        ),
        Err(err) => {
            push_step(&mut steps, "apply_no_arm", false, err.to_string());
            return Ok(SoftInputE2eResult::from_steps(soft, steps));
        }
    }

    match ctl.apply_routes(&table, true) {
        Ok(()) => push_step(
            &mut steps,
            "apply_arm",
            true,
            format!("entries={} armed", table.entry_count),
        ),
        Err(err) => {
            push_step(&mut steps, "apply_arm", false, err.to_string());
            let _ = multiseat_platform_windows::emergency_fail_open();
            return Ok(SoftInputE2eResult::from_steps(soft, steps));
        }
    }

    match multiseat_platform_windows::filter_heartbeat() {
        Ok(health) if health.healthy => push_step(
            &mut steps,
            "ping",
            true,
            format!("fail_open={} {}", health.fail_open, health.detail),
        ),
        Ok(health) => {
            push_step(&mut steps, "ping", false, health.detail);
            let _ = multiseat_platform_windows::emergency_fail_open();
            return Ok(SoftInputE2eResult::from_steps(soft, steps));
        }
        Err(err) => {
            push_step(&mut steps, "ping", false, err.to_string());
            let _ = multiseat_platform_windows::emergency_fail_open();
            return Ok(SoftInputE2eResult::from_steps(soft, steps));
        }
    }

    match multiseat_platform_windows::emergency_fail_open() {
        Ok(()) => push_step(
            &mut steps,
            "fail_open",
            true,
            "cleared routes + fail-open".into(),
        ),
        Err(err) => push_step(&mut steps, "fail_open", false, err.to_string()),
    }

    Ok(SoftInputE2eResult::from_steps(soft, steps))
}

fn run_input_routes_apply(
    config_path: Option<&PathBuf>,
    dry_run: bool,
    arm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (source, config) = load_routes_config(config_path)?;
    let table = multiseat_platform_windows::route_table_from_config(&config)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    table
        .validate()
        .map_err(|m| -> Box<dyn std::error::Error> { m.into() })?;
    println!("source={source}");
    print!(
        "{}",
        multiseat_platform_windows::format_route_table_preview(&table)
    );

    if dry_run {
        println!("input-routes-apply: dry-run (no set_routes / arm)");
        return Ok(());
    }

    let ctl = multiseat_platform_windows::InputFilterControl::open()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    if !ctl.is_present() {
        return Err(format!(
            "input-routes-apply: control device absent ({}); set MULTISEAT_INPUT_FILTER=1 for soft lab or install the filter",
            multiseat_platform_windows::INPUT_FILTER_CONTROL_DEVICE
        )
        .into());
    }

    ctl.apply_routes(&table, arm)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let stats = ctl
        .query_stats()
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    println!(
        "input-routes-apply: ok soft={} armed={} entries={} fail_open={}",
        ctl.is_soft(),
        arm,
        table.entry_count,
        { stats.fail_open }
    );
    Ok(())
}

fn run_install_plan(
    out: Option<&PathBuf>,
    pretend_version: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let caps = multiseat_platform_windows::detect_capabilities();
    let version = if let Some(raw) = pretend_version {
        Some(parse_version_triple(raw)?)
    } else {
        multiseat_platform_windows::windows_version_numbers()
    };
    let plan = InstallPlan::from_capabilities(&caps, version);
    println!("{}", plan.summary);
    for p in &plan.packages {
        if p.selected {
            println!(
                "  [selected] {} signing={} status={:?} — {}",
                p.id.as_str(),
                p.signing.label(),
                p.status,
                p.detail
            );
        }
    }
    for a in &plan.next_actions {
        println!("next: {a}");
    }
    println!("{}", serde_json::to_string_pretty(&plan)?);

    if let Some(dir) = out {
        std::fs::create_dir_all(dir)?;
        let json_path = dir.join("install-plan.json");
        let md_path = dir.join("INSTALL_PLAN.md");
        std::fs::write(&json_path, serde_json::to_string_pretty(&plan)?)?;
        std::fs::write(&md_path, plan.to_markdown())?;
        println!(
            "install-plan: wrote {} and {}",
            json_path.display(),
            md_path.display()
        );
    }
    if plan.ready_to_ship {
        println!("install-plan: READY (unexpected until HLK/Stage 0 complete)");
    }
    Ok(())
}

fn parse_version_triple(raw: &str) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    let parts: Vec<_> = raw.split('.').collect();
    if parts.len() != 3 {
        return Err(format!("pretend-version expected major.minor.build, got {raw}").into());
    }
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
}

/// Start lab sample → AgentFailOpen → expect Idle (orchestrator fail-open path).
async fn run_fail_open_probe(
    client: &mut DaemonClient,
) -> Result<FailOpenProbeResult, Box<dyn std::error::Error>> {
    println!("lab: fail-open probe (Start → AgentFailOpen → Idle)");
    expect_ok(
        client
            .request(&ClientRequest::Start(lab_sample_config()))
            .await?,
        "fail-open start",
    )?;
    expect_ok(
        client
            .request(&ClientRequest::AgentFailOpen {
                seat_id: SeatId(1),
                reason: "stage0-lab-checklist".into(),
            })
            .await?,
        "fail-open AgentFailOpen",
    )?;
    let snap = client.request(&ClientRequest::GetSnapshot).await?;
    let restored = matches!(
        &snap,
        ServerResponse::Snapshot {
            state: SeatState::Idle,
            ..
        }
    );
    let detail = match &snap {
        ServerResponse::Snapshot { state, .. } => {
            format!("after AgentFailOpen state={state:?}; restored_to_idle={restored}")
        }
        other => format!("unexpected snapshot {other:?}"),
    };
    let result = FailOpenProbeResult::evaluate(restored, detail);
    println!(
        "lab: fail-open status={:?} restored={}",
        result.status, result.restored_to_idle
    );
    Ok(result)
}
