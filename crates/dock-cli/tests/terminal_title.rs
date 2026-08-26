#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

const PROJECT: &str = "/tmp/title-e2e/agent-activity-dock";
const OSC_NEEDLE: &str = "]0;agent-activity-dock · grok";

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("orbcue-title-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn isolate<'a>(command: &'a mut Command, root: &Path, socket: &Path) -> &'a mut Command {
    command
        .env("ORBCUE_SOCKET", socket)
        .env("ORBCUE_BACKEND", "wsl")
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HOME", root.join("home"))
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("ORBCUE_NO_TITLE")
        .env_remove("ORBCUE_TERMINAL_ID")
        .env_remove("WT_SESSION")
}

fn orb_status(root: &Path, socket: &Path) -> Value {
    let output = isolate(
        Command::new(env!("CARGO_BIN_EXE_orb"))
            .args(["--socket", socket.to_str().unwrap(), "--json", "status"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    )
    .output()
    .expect("dock status");
    assert!(
        output.status.success(),
        "dock status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn hook_payload() -> String {
    format!(
        r#"{{"hookEventName":"session_start","sessionId":"title-sess","event_id":"title-e1","cwd":"{PROJECT}"}}"#
    )
}

fn stop_payload() -> String {
    format!(
        r#"{{"hookEventName":"stop","sessionId":"title-sess","event_id":"title-stop","reason":"end_turn","cwd":"{PROJECT}"}}"#
    )
}

fn run_hook_in_pty(
    root: &Path,
    socket: &Path,
    capture: &Path,
    no_title: bool,
) -> std::process::ExitStatus {
    let payload = root.join("payload.json");
    fs::write(&payload, hook_payload()).unwrap();
    let inner = format!(
        "{} --socket {} --json hook grok < {}",
        env!("CARGO_BIN_EXE_orb"),
        socket.display(),
        payload.display()
    );
    let mut command = Command::new("script");
    isolate(
        command
            .args(["-q", "-e", "-c", &inner, capture.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    );
    if no_title {
        command.env("ORBCUE_NO_TITLE", "1");
    }
    let output = command.output().expect("script pty hook");
    assert!(
        output.status.success(),
        "script/dock hook failed ({:?}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.status
}

#[test]
fn hook_writes_osc_title_on_a_real_pty() {
    let root = isolated_root();
    let socket = root.join("orb.sock");
    fs::create_dir_all(root.join("home")).unwrap();
    let service = orbcue_service::spawn(&socket).expect("spawn isolated orbd");

    let capture = root.join("title-capture");
    let status = run_hook_in_pty(&root, &socket, &capture, false);
    assert!(status.success(), "hook exit should be 0: {status:?}");

    let bytes = fs::read(&capture).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(OSC_NEEDLE),
        "PTY capture should contain OSC title {OSC_NEEDLE:?}, got:\n{text}"
    );

    let snapshot = orb_status(&root, &socket);
    let ids: Vec<&str> = snapshot["snapshot"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["session_id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["title-sess"],
        "event should still be delivered: {snapshot}"
    );
    assert_eq!(snapshot["snapshot"]["tracked_count"], 1);

    let skip_capture = root.join("title-skip");
    let skip_status = run_hook_in_pty(&root, &socket, &skip_capture, true);
    assert!(skip_status.success());
    let skip_bytes = fs::read(&skip_capture).unwrap_or_default();
    let skip_text = String::from_utf8_lossy(&skip_bytes);
    assert!(
        !skip_text.contains(OSC_NEEDLE),
        "NO_TITLE=1 should skip OSC, got:\n{skip_text}"
    );

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}

fn run_setsid_hook_in_pty(
    root: &Path,
    socket: &Path,
    capture: &Path,
    payload_body: &str,
    no_title: bool,
) -> std::process::ExitStatus {
    let payload = root.join("payload-setsid.json");
    fs::write(&payload, payload_body).unwrap();
    let inner = format!(
        "setsid -w {} --socket {} --json hook grok < {}",
        env!("CARGO_BIN_EXE_orb"),
        socket.display(),
        payload.display()
    );
    let mut command = Command::new("script");
    isolate(
        command
            .args(["-q", "-e", "-c", &inner, capture.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    );
    if no_title {
        command.env("ORBCUE_NO_TITLE", "1");
    }
    let output = command.output().expect("script setsid hook");
    assert!(
        output.status.success(),
        "setsid title hook failed ({:?}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output.status
}

#[test]
fn setsid_hook_writes_osc_via_ancestor_tty() {
    let root = isolated_root();
    let socket = root.join("orb.sock");
    fs::create_dir_all(root.join("home")).unwrap();
    let service = orbcue_service::spawn(&socket).expect("spawn isolated orbd");

    let capture = root.join("title-setsid");
    let status = run_setsid_hook_in_pty(&root, &socket, &capture, &hook_payload(), false);
    assert!(status.success(), "setsid hook exit should be 0: {status:?}");
    let capture_bytes = fs::read(&capture).unwrap_or_default();
    let text = String::from_utf8_lossy(&capture_bytes);
    assert!(
        text.contains(OSC_NEEDLE),
        "ancestor tty write should emit OSC {OSC_NEEDLE:?}, got:\n{text}"
    );

    let skip_capture = root.join("title-setsid-skip");
    let skip_status = run_setsid_hook_in_pty(&root, &socket, &skip_capture, &hook_payload(), true);
    assert!(skip_status.success());
    let skip_bytes = fs::read(&skip_capture).unwrap_or_default();
    let skip_text = String::from_utf8_lossy(&skip_bytes);
    assert!(
        !skip_text.contains(OSC_NEEDLE),
        "NO_TITLE=1 should skip ancestor OSC, got:\n{skip_text}"
    );

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn setsid_stop_hook_writes_osc_via_ancestor_tty() {
    let root = isolated_root();
    let socket = root.join("orb.sock");
    fs::create_dir_all(root.join("home")).unwrap();
    let service = orbcue_service::spawn(&socket).expect("spawn isolated orbd");

    let capture = root.join("title-stop");
    let status = run_setsid_hook_in_pty(&root, &socket, &capture, &stop_payload(), false);
    assert!(status.success(), "stop hook exit should be 0: {status:?}");
    let capture_bytes = fs::read(&capture).unwrap_or_default();
    let text = String::from_utf8_lossy(&capture_bytes);
    assert!(
        text.contains(OSC_NEEDLE),
        "completed/stop should emit OSC {OSC_NEEDLE:?}, got:\n{text}"
    );

    let skip_capture = root.join("title-stop-skip");
    let skip_status = run_setsid_hook_in_pty(&root, &socket, &skip_capture, &stop_payload(), true);
    assert!(skip_status.success());
    let skip_bytes = fs::read(&skip_capture).unwrap_or_default();
    let skip_text = String::from_utf8_lossy(&skip_bytes);
    assert!(
        !skip_text.contains(OSC_NEEDLE),
        "NO_TITLE=1 should skip stop OSC, got:\n{skip_text}"
    );

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}
