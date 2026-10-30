//! Fault-injection integration tests (план §10):
//! corrupted journal, partial apply over IPC, client disconnect, IPC timeout.
//!
//! Все сценарии гоняются на mock-бэкендах поверх TCP-транспорта, поэтому
//! работают на любой dev-хосте (Windows-стенд не требуется).

use std::net::TcpListener;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use multiseat_core::{
    ClientRequest, JournalEntry, JournalStore, MultiseatConfig, OperationJournal, SeatState,
    ServerResponse, JOURNAL_SCHEMA_VERSION,
};
use multiseat_daemon::{
    client::ClientError, run_server, DaemonClient, DaemonState, FileJournalStore,
};
use multiseat_platform_windows::MOCK_FAIL_STAGE_ENV;
use tokio::time::{sleep, Duration};

/// Serializes env-mutating tests inside this binary (mock stage switches).
static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

fn env_lock() -> MutexGuard<'static, ()> {
    ENV_MUTEX
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn free_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("127.0.0.1:{port}")
}

fn unique_data_dir(label: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("multiseat-fi-{label}-{stamp}"))
}

async fn spawn_daemon(
    addr: &str,
    data_dir: &std::path::Path,
) -> tokio::task::JoinHandle<Result<(), std::io::Error>> {
    let state = DaemonState::mock_with_data_dir(data_dir);
    let bind = addr.to_string();
    tokio::spawn(async move { run_server(&bind, state).await })
}

async fn connect_with_retry(addr: &str) -> DaemonClient {
    for _ in 0..50 {
        match DaemonClient::connect(addr).await {
            Ok(client) => return client,
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }
    panic!("daemon did not come up at {addr}");
}

fn active_journal() -> OperationJournal {
    let mut journal = OperationJournal::begin(MultiseatConfig::default());
    journal.push(JournalEntry::SessionCreated {
        seat_id: multiseat_core::SeatId(1),
        session_id: multiseat_core::SessionId(7),
    });
    journal
}

/// Boot with a corrupted journal must be safe: no panic, no Active/Failed
/// state, and the daemon stays fully operable afterwards.
async fn assert_boots_safe_with_journal(label: &str, tamper: impl FnOnce(&[u8]) -> Vec<u8>) {
    // Serialize the whole file: env-based fault stages are process-wide.
    let _env = env_lock();
    let addr = free_addr();
    let data_dir = unique_data_dir(label);
    let store = FileJournalStore::in_dir(&data_dir);
    store.save(&active_journal()).unwrap();
    let raw = std::fs::read(store.path()).unwrap();
    std::fs::write(store.path(), tamper(&raw)).unwrap();

    let server = spawn_daemon(&addr, &data_dir).await;
    let mut client = connect_with_retry(&addr).await;

    let snapshot = client.request(&ClientRequest::GetSnapshot).await.unwrap();
    match snapshot {
        ServerResponse::Snapshot { state, .. } => {
            assert_eq!(
                state,
                SeatState::Idle,
                "corrupt journal must not resurrect state"
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    // Daemon remains fully operable: start/stop roundtrip succeeds.
    let started = client
        .request(&ClientRequest::Start(multiseat_daemon::lab_sample_config()))
        .await
        .unwrap();
    assert_eq!(started, ServerResponse::Ok);
    let stopped = client.request(&ClientRequest::Stop).await.unwrap();
    assert_eq!(stopped, ServerResponse::Ok);

    client
        .request(&ClientRequest::Hello {
            client_version: multiseat_core::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    server.abort();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn corrupted_journal_garbage_boots_safe() {
    assert_boots_safe_with_journal("garbage", |_| b"\x00not json at all{{{".to_vec()).await;
}

#[tokio::test]
async fn corrupted_journal_truncated_boots_safe() {
    assert_boots_safe_with_journal("truncated", |raw| raw[..raw.len() / 2].to_vec()).await;
}

#[tokio::test]
async fn corrupted_journal_wrong_schema_boots_safe() {
    assert_boots_safe_with_journal("schema", |raw| {
        let text = String::from_utf8_lossy(raw).replace(
            &format!("\"schema_version\": {JOURNAL_SCHEMA_VERSION}"),
            "\"schema_version\": 999999",
        );
        text.into_bytes()
    })
    .await;
}

/// Start fails mid-transaction at `stage`; the failure is reported over IPC,
/// state ends in Failed, and a retry after clearing the fault fully recovers.
async fn partial_apply_recovers(label: &str, stage: &str) {
    let _env = env_lock();
    std::env::set_var(MOCK_FAIL_STAGE_ENV, stage);
    let addr = free_addr();
    let data_dir = unique_data_dir(label);
    let server = spawn_daemon(&addr, &data_dir).await;
    let mut client = connect_with_retry(&addr).await;

    let config = multiseat_daemon::lab_sample_config();
    let failed = client.request(&ClientRequest::Start(config.clone())).await;
    match failed {
        Ok(ServerResponse::Err(_)) => {}
        other => panic!("expected daemon error for stage {stage}, got {other:?}"),
    }

    let snapshot = client.request(&ClientRequest::GetSnapshot).await.unwrap();
    match snapshot {
        ServerResponse::Snapshot { state, .. } => {
            assert!(matches!(state, SeatState::Failed { .. }));
        }
        other => panic!("unexpected {other:?}"),
    }

    // Clear the fault: retry must fully recover to Active.
    std::env::remove_var(MOCK_FAIL_STAGE_ENV);
    let started = client.request(&ClientRequest::Start(config)).await.unwrap();
    assert_eq!(started, ServerResponse::Ok);

    let snapshot = client.request(&ClientRequest::GetSnapshot).await.unwrap();
    match snapshot {
        ServerResponse::Snapshot { state, .. } => assert_eq!(state, SeatState::Active),
        other => panic!("unexpected {other:?}"),
    }

    let stopped = client.request(&ClientRequest::Stop).await.unwrap();
    assert_eq!(stopped, ServerResponse::Ok);

    server.abort();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn partial_apply_session_create_fails_then_recovers() {
    partial_apply_recovers("pa-session", "session_create").await;
}

#[tokio::test]
async fn partial_apply_agent_start_fails_then_recovers() {
    partial_apply_recovers("pa-agent", "agent_start").await;
}

#[tokio::test]
async fn partial_apply_display_prepare_fails_then_recovers() {
    partial_apply_recovers("pa-disp-prep", "display_prepare").await;
}

#[tokio::test]
async fn partial_apply_display_attach_fails_then_recovers() {
    partial_apply_recovers("pa-disp-att", "display_attach").await;
}

#[tokio::test]
async fn partial_apply_input_apply_fails_then_recovers() {
    partial_apply_recovers("pa-input", "input_apply").await;
}

#[tokio::test]
async fn client_disconnect_during_active_daemon_survives() {
    let _env = env_lock();
    let addr = free_addr();
    let data_dir = unique_data_dir("disconnect");
    let server = spawn_daemon(&addr, &data_dir).await;

    let mut client = connect_with_retry(&addr).await;
    let started = client
        .request(&ClientRequest::Start(multiseat_daemon::lab_sample_config()))
        .await
        .unwrap();
    assert_eq!(started, ServerResponse::Ok);

    // Abrupt disconnect (no graceful Stop): the daemon owns the state.
    drop(client);

    let mut second = connect_with_retry(&addr).await;
    let snapshot = second.request(&ClientRequest::GetSnapshot).await.unwrap();
    match snapshot {
        ServerResponse::Snapshot { state, .. } => assert_eq!(state, SeatState::Active),
        other => panic!("unexpected {other:?}"),
    }

    let stopped = second.request(&ClientRequest::Stop).await.unwrap();
    assert_eq!(stopped, ServerResponse::Ok);

    second
        .request(&ClientRequest::Hello {
            client_version: multiseat_core::PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    server.abort();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn request_times_out_when_daemon_silent() {
    let _env = env_lock();
    // A TCP peer that accepts the connection but never speaks back: the
    // framed client must surface `ClientError::Timeout` (5s request budget)
    // instead of hanging forever.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let hold = std::sync::Arc::new(listener);
    let acceptor = tokio::task::spawn_blocking(move || {
        // Keep the (single) connection open without answering.
        let (_conn, _) = hold.accept().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(8));
    });

    let started = std::time::Instant::now();
    let err = match DaemonClient::connect(&addr).await {
        Ok(_) => panic!("silent peer must not complete the handshake"),
        Err(err) => err,
    };
    assert!(matches!(err, ClientError::Timeout), "got {err:?}");
    assert!(started.elapsed().as_secs() >= 4, "timeout fired too early");
    acceptor.abort();
}
