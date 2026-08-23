#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("aadock-terminal-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn dock_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dock"))
}

fn isolated_env<'a>(command: &'a mut Command, root: &Path, socket: &Path) -> &'a mut Command {
    command
        .env("AGENT_ACTIVITY_DOCK_SOCKET", socket)
        .env("AGENT_ACTIVITY_DOCK_BACKEND", "wsl")
        .env("AGENT_ACTIVITY_DOCK_TERMINAL_ID", "pts-test")
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HOME", root.join("home"))
        .env("AGENT_ACTIVITY_DOCK_DOCKD", root.join("missing-dockd"))
        .env_remove("XDG_RUNTIME_DIR")
}

fn hook_grok(root: &Path, socket: &Path, payload: &str) -> Value {
    let mut child = isolated_env(
        dock_cmd()
            .args([
                "--socket",
                socket.to_str().unwrap(),
                "--json",
                "hook",
                "grok",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    )
    .spawn()
    .expect("spawn dock hook");
    {
        let mut stdin = child.stdin.take().expect("hook stdin");
        stdin.write_all(payload.as_bytes()).unwrap();
    }
    let output = child.wait_with_output().expect("wait dock hook");
    assert!(
        output.status.success(),
        "dock hook failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid hook JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn emit(root: &Path, socket: &Path, args: &[&str]) -> Value {
    let output = isolated_env(
        dock_cmd()
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    )
    .output()
    .expect("run dock");
    assert!(
        output.status.success(),
        "dock {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid dock JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn session_ids(value: &Value) -> Vec<String> {
    value["snapshot"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["session_id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn hook_and_start_replace_sessions_on_the_same_terminal() {
    let root = isolated_root();
    let socket = root.join("dock.sock");
    fs::create_dir_all(root.join("home")).unwrap();
    let service = agent_activity_dock_service::spawn(&socket).expect("spawn isolated dockd");

    let first = hook_grok(
        &root,
        &socket,
        r#"{"hookEventName":"session_start","sessionId":"sess-a","event_id":"hook-a"}"#,
    );
    assert_eq!(session_ids(&first), ["sess-a"]);

    let second = hook_grok(
        &root,
        &socket,
        r#"{"hookEventName":"session_start","sessionId":"sess-b","event_id":"hook-b"}"#,
    );
    assert_eq!(
        session_ids(&second),
        ["sess-b"],
        "second grok session_start should replace the first: {second}"
    );
    assert_eq!(second["snapshot"]["tracked_count"], 1);

    let started = emit(
        &root,
        &socket,
        &[
            "--socket",
            socket.to_str().unwrap(),
            "--json",
            "start",
            "cli-c",
            "--source",
            "probe",
        ],
    );
    assert_eq!(
        session_ids(&started),
        ["cli-c"],
        "dock start should replace the hook session: {started}"
    );
    assert_eq!(started["snapshot"]["tracked_count"], 1);

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}
