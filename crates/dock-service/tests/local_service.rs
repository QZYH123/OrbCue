use agent_activity_dock_core::{DockEvent, EventKind};
use agent_activity_dock_ipc::{default_endpoint, encode_line, WireResponse};
use agent_activity_dock_service::{attach_or_listen, spawn, spawn_persistent};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn endpoint() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("agent-activity-dock-test-{nonce}.sock"))
}

fn send(path: &std::path::Path, value: impl serde::Serialize) -> WireResponse {
    let mut stream = UnixStream::connect(path).unwrap();
    stream.write_all(&encode_line(&value).unwrap()).unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn cli_like_events_cross_the_real_local_socket() {
    let path = endpoint();
    let service = spawn(&path).unwrap();

    let start = DockEvent::new("e1", EventKind::Started, "claude", "s1");
    let started = send(&path, start);
    assert!(started.accepted);
    assert_eq!(started.snapshot.count_label, "1/1");

    let completed = DockEvent::new("e2", EventKind::Completed, "claude", "s1");
    let response = send(&path, completed);
    assert_eq!(response.snapshot.count_label, "0/1");
    assert_eq!(response.snapshot.pending_count, 1);
    assert_eq!(response.snapshot.pending_mark, "*");
    assert_eq!(response.snapshot.sessions[0].mark, "*");

    let ack = serde_json::json!({
        "query": "acknowledge",
        "source": "claude",
        "session_id": "s1"
    });
    assert_eq!(send(&path, ack).snapshot.pending_count, 1);
    let reset = serde_json::json!({
        "query": "reset",
        "source": "claude",
        "session_id": "s1"
    });
    assert_eq!(send(&path, reset).snapshot.tracked_count, 0);
    service.shutdown();
    assert!(!path.exists());
}

#[test]
fn subscribers_receive_state_changes_without_polling() {
    let path = endpoint();
    let service = spawn(&path).unwrap();
    let mut stream = UnixStream::connect(&path).unwrap();
    stream
        .write_all(
            br#"{"query":"subscribe"}
"#,
        )
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    assert!(line.contains("subscribed"));

    let updates = service.subscribe_updates();
    let start = DockEvent::new("e1", EventKind::Started, "codex", "s1");
    let response = send(&path, start);
    assert_eq!(response.snapshot.working_count, 1);
    let update = updates.recv().unwrap();
    assert_eq!(update.snapshot.count_label, "1/1");
    service.shutdown();
}

#[test]
fn restart_recovers_minimal_state_without_replaying_attention() {
    let first_path = endpoint();
    let state_path = first_path.with_extension("state.json");
    let service = spawn_persistent(&first_path, &state_path).unwrap();
    send(
        &first_path,
        DockEvent::new("e0", EventKind::Started, "claude", "s1"),
    );
    send(
        &first_path,
        DockEvent::new("e1", EventKind::Failed, "claude", "s1")
            .with_summary("must remain ephemeral"),
    );
    service.shutdown();

    let persisted = std::fs::read_to_string(&state_path).unwrap();
    assert!(!persisted.contains("must remain ephemeral"));
    let second_path = endpoint();
    let restored = spawn_persistent(&second_path, &state_path).unwrap();
    let response = send(&second_path, serde_json::json!({"query": "snapshot"}));
    assert_eq!(response.snapshot.pending_count, 1);
    assert_eq!(response.snapshot.pending_mark, "!");
    assert_eq!(response.snapshot.sessions[0].session_id, "s1");
    assert!(response.snapshot.sessions[0].summary.is_none());
    assert!(response.attention.is_none());
    restored.shutdown();
    std::fs::remove_file(state_path).unwrap();
}

#[test]
fn desktop_attaches_to_an_already_running_daemon() {
    let path = endpoint();
    let service = spawn(&path).unwrap();
    let session = attach_or_listen(&path, path.with_extension("state.json"), None).unwrap();
    assert!(!session.owns_daemon());
    assert_eq!(session.kind(), "remote");
    assert_eq!(session.snapshot().unwrap().count_label, "0/0");

    session.request_shutdown();
    let still_alive = send(&path, serde_json::json!({"query": "snapshot"}));
    assert!(still_alive.ok);
    assert_eq!(still_alive.snapshot.count_label, "0/0");
    service.shutdown();
}

#[test]
fn remote_subscribers_follow_daemon_updates() {
    let path = endpoint();
    let service = spawn(&path).unwrap();
    let session = attach_or_listen(&path, path.with_extension("state.json"), None).unwrap();
    let updates = session.subscribe();
    let started = send(
        &path,
        DockEvent::new("e1", EventKind::Started, "claude", "s1"),
    );
    assert_eq!(started.snapshot.count_label, "1/1");

    let mut label = String::new();
    for _ in 0..8 {
        let update = updates.recv_timeout(Duration::from_secs(1)).unwrap();
        label = update.snapshot.count_label.clone();
        if label == "1/1" {
            break;
        }
    }
    assert_eq!(label, "1/1");
    service.shutdown();
}

#[allow(dead_code)]
fn _default_endpoint_is_available_for_smoke_tests() {
    let _ = default_endpoint();
}
