#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("aadock-bridge-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn dock_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dock"))
}

fn isolated_env<'a>(command: &'a mut Command, root: &Path, socket: &Path) -> &'a mut Command {
    command
        .env("AGENT_ACTIVITY_DOCK_SOCKET", socket)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HOME", root.join("home"))
        .env("AGENT_ACTIVITY_DOCK_DOCKD", root.join("missing-dockd"))
        .env_remove("XDG_RUNTIME_DIR")
}

fn spawn_bridge(root: &Path, socket: &Path) -> Child {
    isolated_env(
        dock_cmd()
            .args(["--socket", socket.to_str().unwrap(), "bridge"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    )
    .spawn()
    .expect("spawn dock bridge")
}

fn read_json_line(stdout: &mut BufReader<ChildStdout>) -> Value {
    let mut line = String::new();
    stdout
        .read_line(&mut line)
        .expect("read bridge output line");
    assert!(
        !line.trim().is_empty(),
        "dock bridge closed without a JSON line"
    );
    serde_json::from_str(line.trim()).unwrap_or_else(|error| {
        panic!("invalid bridge JSON ({error}): {line}");
    })
}

fn wait_snapshot(stdout: &mut BufReader<ChildStdout>, check: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..12 {
        let value = read_json_line(stdout);
        if check(&value) {
            return value;
        }
    }
    panic!("subscribe stream never reached the expected snapshot");
}

fn query_bridge(root: &Path, socket: &Path, request: &str) -> Value {
    let mut child = spawn_bridge(root, socket);
    let mut stdin = child.stdin.take().expect("bridge stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("bridge stdout"));
    stdin.write_all(request.as_bytes()).unwrap();
    stdin.flush().unwrap();
    drop(stdin);
    let value = read_json_line(&mut stdout);
    let status = child.wait().expect("wait short-lived bridge");
    assert!(status.success(), "short-lived dock bridge failed: {status}");
    value
}

fn emit(root: &Path, socket: &Path, args: &[&str]) {
    let status = isolated_env(dock_cmd().args(args).stdin(Stdio::null()), root, socket)
        .status()
        .expect("run dock emitter");
    assert!(status.success(), "dock {} failed", args.join(" "));
}

#[test]
fn bridge_forwards_subscribe_ack_and_reset() {
    let root = isolated_root();
    let socket = root.join("dock.sock");
    fs::create_dir_all(root.join("home")).unwrap();

    let mut subscribe = spawn_bridge(&root, &socket);
    let mut stdin = subscribe.stdin.take().expect("subscribe stdin");
    let mut stdout = BufReader::new(subscribe.stdout.take().expect("subscribe stdout"));
    stdin.write_all(b"{\"query\":\"subscribe\"}\n").unwrap();
    stdin.flush().unwrap();

    let subscribed = read_json_line(&mut stdout);
    assert_eq!(subscribed["type"], "subscribed");
    assert_eq!(subscribed["snapshot"]["count_label"], "0/0");

    emit(
        &root,
        &socket,
        &[
            "--socket",
            socket.to_str().unwrap(),
            "start",
            "smoke-bridge",
            "--source",
            "probe",
        ],
    );
    let started = wait_snapshot(&mut stdout, |value| {
        value["snapshot"]["count_label"] == "1/1"
    });
    assert_eq!(started["type"], "snapshot");
    assert_eq!(started["snapshot"]["working_count"], 1);

    emit(
        &root,
        &socket,
        &[
            "--socket",
            socket.to_str().unwrap(),
            "complete",
            "smoke-bridge",
            "--source",
            "probe",
        ],
    );
    let completed = wait_snapshot(&mut stdout, |value| {
        value["snapshot"]["count_label"] == "0/1" && value["snapshot"]["pending_count"] == 1
    });
    assert_eq!(completed["snapshot"]["pending_count"], 1);

    let ack = query_bridge(
        &root,
        &socket,
        "{\"query\":\"acknowledge\",\"source\":\"probe\",\"session_id\":\"smoke-bridge\"}\n",
    );
    assert_eq!(ack["ok"], true, "ack response: {ack}");
    assert_eq!(ack["snapshot"]["tracked_count"], 1, "ack response: {ack}");
    assert_eq!(
        ack["snapshot"]["sessions"][0]["acknowledged"], true,
        "ack response: {ack}"
    );
    let acked = wait_snapshot(&mut stdout, |value| {
        value["snapshot"]["sessions"][0]["acknowledged"] == true
    });
    assert_eq!(acked["type"], "snapshot");

    let reset = query_bridge(
        &root,
        &socket,
        "{\"query\":\"reset\",\"source\":\"probe\",\"session_id\":\"smoke-bridge\"}\n",
    );
    assert_eq!(reset["ok"], true);
    assert_eq!(reset["snapshot"]["tracked_count"], 0);
    let cleared = wait_snapshot(&mut stdout, |value| {
        value["snapshot"]["count_label"] == "0/0"
    });
    assert_eq!(cleared["snapshot"]["tracked_count"], 0);

    drop(stdin);
    let status = subscribe.wait().expect("wait subscribe bridge");
    assert!(status.success(), "subscribe dock bridge failed: {status}");
    fs::remove_dir_all(root).unwrap();
}
