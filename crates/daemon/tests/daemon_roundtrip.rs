//! Integration tests: TCP daemon ↔ framed client.

use std::net::TcpListener;
use std::time::{SystemTime, UNIX_EPOCH};

use multiseat_core::{ClientRequest, SeatId, SeatState, ServerResponse};
use multiseat_daemon::{lab_sample_config, run_server, DaemonClient, DaemonState};
use tokio::time::{sleep, Duration};

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
    std::env::temp_dir().join(format!("multiseat-it-{label}-{stamp}"))
}

#[tokio::test]
async fn validate_start_stop_roundtrip() {
    let addr = free_addr();
    let data_dir = unique_data_dir("roundtrip");
    let state = DaemonState::mock_with_data_dir(&data_dir);
    let bind = addr.clone();
    let server = tokio::spawn(async move { run_server(&bind, state).await });

    let mut client = None;
    for _ in 0..50 {
        match DaemonClient::connect(&addr).await {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }
    let mut client = client.expect("connect");
    let validation = client
        .request(&ClientRequest::ValidateConfig(lab_sample_config()))
        .await
        .unwrap();
    match validation {
        ServerResponse::Validation(report) => assert!(!report.has_errors()),
        other => panic!("unexpected {other:?}"),
    }

    let started = client
        .request(&ClientRequest::Start(lab_sample_config()))
        .await
        .unwrap();
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
async fn saves_and_reloads_config() {
    let data_dir = unique_data_dir("config");
    let addr = free_addr();
    let state = DaemonState::mock_with_data_dir(&data_dir);
    let bind = addr.clone();
    let server = tokio::spawn(async move { run_server(&bind, state).await });

    let mut client = None;
    for _ in 0..50 {
        match DaemonClient::connect(&addr).await {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }
    let mut client = client.expect("connect");
    let saved = client
        .request(&ClientRequest::SaveConfig(lab_sample_config()))
        .await
        .unwrap();
    assert_eq!(saved, ServerResponse::Ok);
    server.abort();

    // New daemon instance must load the saved config.
    let state2 = DaemonState::mock_with_data_dir(&data_dir);
    let addr2 = free_addr();
    let bind2 = addr2.clone();
    let server2 = tokio::spawn(async move { run_server(&bind2, state2).await });
    let mut client2 = None;
    for _ in 0..50 {
        match DaemonClient::connect(&addr2).await {
            Ok(c) => {
                client2 = Some(c);
                break;
            }
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }
    let mut client2 = client2.expect("connect");
    let snapshot = client2.request(&ClientRequest::GetSnapshot).await.unwrap();
    match snapshot {
        ServerResponse::Snapshot { config, .. } => {
            assert_eq!(config.seats.len(), lab_sample_config().seats.len());
        }
        other => panic!("unexpected {other:?}"),
    }
    server2.abort();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn recovers_active_journal_on_boot() {
    use multiseat_core::{JournalStore, OperationJournal};
    use multiseat_daemon::FileJournalStore;

    let data_dir = unique_data_dir("recover");
    let store = FileJournalStore::in_dir(&data_dir);
    let mut journal = OperationJournal::begin(lab_sample_config());
    journal.mark_completed();
    store.save(&journal).unwrap();

    // Booting a new daemon must recover and clear the journal.
    let _state = DaemonState::mock_with_data_dir(&data_dir);
    assert!(store.load().unwrap().is_none());

    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn feasibility_report_is_no_go_until_session_poc() {
    let addr = free_addr();
    let data_dir = unique_data_dir("feasibility");
    let state = DaemonState::mock_with_data_dir(&data_dir);
    let bind = addr.clone();
    let server = tokio::spawn(async move { run_server(&bind, state).await });

    let mut client = None;
    for _ in 0..50 {
        match DaemonClient::connect(&addr).await {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }
    let mut client = client.expect("connect");
    let response = client
        .request(&ClientRequest::GetFeasibilityReport)
        .await
        .unwrap();
    match response {
        ServerResponse::Feasibility(report) => {
            assert!(!report.go);
            assert!(report.summary.starts_with("NO-GO"));
            let dual = report
                .checks
                .iter()
                .find(|c| c.id == "dual_session")
                .expect("dual_session");
            assert!(matches!(
                dual.status,
                multiseat_core::CheckStatus::Blocked | multiseat_core::CheckStatus::Unknown
            ));
        }
        other => panic!("unexpected {other:?}"),
    }
    server.abort();
    let _ = std::fs::remove_dir_all(data_dir);
}

#[tokio::test]
async fn session_agent_hello_heartbeat_and_fail_open() {
    let addr = free_addr();
    let data_dir = unique_data_dir("agent");
    let state = DaemonState::mock_with_data_dir(&data_dir);
    let bind = addr.clone();
    let server = tokio::spawn(async move { run_server(&bind, state).await });

    let mut client = None;
    for _ in 0..50 {
        match DaemonClient::connect(&addr).await {
            Ok(c) => {
                client = Some(c);
                break;
            }
            Err(_) => sleep(Duration::from_millis(20)).await,
        }
    }
    let mut client = client.expect("connect");

    let hello = client
        .request(&ClientRequest::AgentHello {
            seat_id: SeatId(1),
            agent_version: 1,
        })
        .await
        .unwrap();
    match hello {
        ServerResponse::AgentAck {
            seat_id,
            heartbeat_interval_ms,
        } => {
            assert_eq!(seat_id, SeatId(1));
            assert!(heartbeat_interval_ms >= 200);
        }
        other => panic!("unexpected {other:?}"),
    }

    let beat = client
        .request(&ClientRequest::AgentHeartbeat {
            seat_id: SeatId(1),
        })
        .await
        .unwrap();
    assert_eq!(beat, ServerResponse::Ok);

    let _ = client
        .request(&ClientRequest::Start(lab_sample_config()))
        .await
        .unwrap();

    let fail = client
        .request(&ClientRequest::AgentFailOpen {
            seat_id: SeatId(1),
            reason: "test".into(),
        })
        .await
        .unwrap();
    assert_eq!(fail, ServerResponse::Ok);

    let snapshot = client.request(&ClientRequest::GetSnapshot).await.unwrap();
    match snapshot {
        ServerResponse::Snapshot { state, .. } => assert_eq!(state, SeatState::Idle),
        other => panic!("unexpected {other:?}"),
    }

    server.abort();
    let _ = std::fs::remove_dir_all(data_dir);
}
