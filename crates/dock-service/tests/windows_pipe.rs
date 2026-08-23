#![cfg(windows)]

use agent_activity_dock_core::{DockEvent, EventKind};
use agent_activity_dock_ipc::{encode_line, local_connect, WireResponse};
use agent_activity_dock_service::{attach_or_listen, spawn};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn endpoint() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from(format!(r"\\.\pipe\agent-activity-dock-test-{nonce}"))
}

fn state_path(endpoint: &Path) -> PathBuf {
    let name = endpoint
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("agent-activity-dock-test");
    std::env::temp_dir().join(format!("{name}.state.json"))
}

fn send(path: &Path, value: impl serde::Serialize) -> WireResponse {
    let mut last_error = String::new();
    for _ in 0..25 {
        match local_connect(path) {
            Ok(mut stream) => {
                stream.write_all(&encode_line(&value).unwrap()).unwrap();
                let mut line = String::new();
                BufReader::new(stream).read_line(&mut line).unwrap();
                return serde_json::from_str(&line).unwrap();
            }
            Err(error) => {
                last_error = error.to_string();
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    panic!(
        "cannot connect to named pipe {}: {last_error}",
        path.display()
    );
}

#[test]
fn attach_or_listen_attaches_to_an_existing_named_pipe() {
    let path = endpoint();
    let service = spawn(&path).unwrap();
    let session = attach_or_listen(&path, state_path(&path), None).unwrap();
    assert!(!session.owns_daemon());
    assert_eq!(session.kind(), "remote");

    let started = send(
        &path,
        DockEvent::new("e1", EventKind::Started, "claude", "s1"),
    );
    assert!(started.accepted);
    assert_eq!(started.snapshot.count_label, "1/1");
    assert_eq!(session.snapshot().unwrap().count_label, "1/1");

    session.request_shutdown();
    let still_alive = send(&path, serde_json::json!({"query": "snapshot"}));
    assert!(still_alive.ok);
    service.shutdown();
}

#[test]
fn attach_or_listen_listens_when_the_named_pipe_is_empty() {
    let path = endpoint();
    let state = state_path(&path);
    let session = attach_or_listen(&path, &state, None).unwrap();
    assert!(session.owns_daemon());
    assert_eq!(session.kind(), "owned");

    let started = send(
        &path,
        DockEvent::new("e1", EventKind::Started, "grok", "s1"),
    );
    assert!(started.accepted);
    assert_eq!(started.snapshot.count_label, "1/1");
    assert_eq!(session.snapshot().unwrap().sessions[0].source, "grok");

    session.request_shutdown();
    session.wait_for_shutdown();
    let _ = std::fs::remove_file(state);
}
