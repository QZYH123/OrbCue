#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("aadock-setsid-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn isolate<'a>(command: &'a mut Command, root: &Path, socket: &Path) -> &'a mut Command {
    command
        .env("AGENT_ACTIVITY_DOCK_SOCKET", socket)
        .env("XDG_STATE_HOME", root.join("state"))
        .env("HOME", root.join("home"))
        .env("AGENT_ACTIVITY_DOCK_DOCKD", root.join("missing-dockd"))
        .env_remove("AGENT_ACTIVITY_DOCK_TERMINAL_ID")
        .env_remove("WT_SESSION")
        .env_remove("XDG_RUNTIME_DIR")
}

fn session_ids(value: &Value) -> Vec<String> {
    value["snapshot"]["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["session_id"].as_str().unwrap().to_owned())
        .collect()
}

fn persisted_terminal_ids(state_path: &Path) -> Vec<String> {
    let persisted: Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    persisted["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|session| session["terminal_id"].as_str().unwrap().to_owned())
        .collect()
}

fn run_script(root: &Path, socket: &Path, capture: &Path, inner: &str) {
    let output = isolate(
        Command::new("script")
            .args(["-q", "-e", "-c", inner, capture.to_str().unwrap()])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
        root,
        socket,
    )
    .output()
    .expect("script");
    assert!(
        output.status.success(),
        "script failed ({:?}): {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap_or_default()).unwrap_or_else(|error| {
        panic!(
            "invalid JSON {path:?} ({error}): {}",
            String::from_utf8_lossy(&fs::read(path).unwrap_or_default())
        )
    })
}

#[test]
fn setsid_hooks_share_ancestor_pty_without_wt_session() {
    let root = isolated_root();
    let socket = root.join("dock.sock");
    fs::create_dir_all(root.join("home")).unwrap();
    let state_path = root.join("state.json");
    let service = agent_activity_dock_service::spawn_persistent(&socket, &state_path)
        .expect("spawn isolated dockd");

    let payload_a = root.join("a.json");
    let payload_prompt = root.join("prompt.json");
    let payload_b = root.join("b.json");
    fs::write(
        &payload_a,
        r#"{"hookEventName":"session_start","sessionId":"sess-a","event_id":"e-a-start"}"#,
    )
    .unwrap();
    fs::write(
        &payload_prompt,
        r#"{"hookEventName":"user_prompt_submit","sessionId":"sess-a","event_id":"e-a-prompt"}"#,
    )
    .unwrap();
    fs::write(
        &payload_b,
        r#"{"hookEventName":"session_start","sessionId":"sess-b","event_id":"e-b-start","source":"new"}"#,
    )
    .unwrap();

    let dock = env!("CARGO_BIN_EXE_dock");
    let pty_file = root.join("pty");
    let out_a = root.join("out-a.json");
    let out_prompt = root.join("out-prompt.json");
    let out_b = root.join("out-b.json");
    let inner = format!(
        "set -eu\ntty > {pty}\nsetsid -w {dock} --socket {sock} --json hook grok < {a} > {oa}\nsetsid -w {dock} --socket {sock} --json hook grok < {p} > {op}\nsetsid -w {dock} --socket {sock} --json hook grok < {b} > {ob}\n",
        pty = pty_file.display(),
        dock = dock,
        sock = socket.display(),
        a = payload_a.display(),
        oa = out_a.display(),
        p = payload_prompt.display(),
        op = out_prompt.display(),
        b = payload_b.display(),
        ob = out_b.display(),
    );
    run_script(&root, &socket, &root.join("capture"), &inner);

    let pty = fs::read_to_string(&pty_file).unwrap().trim().to_owned();
    assert!(
        pty.starts_with("/dev/pts/"),
        "script should expose a pts path, got {pty:?}"
    );

    let start_a = read_json(&out_a);
    assert_eq!(session_ids(&start_a), ["sess-a"], "{start_a}");
    let working_a = read_json(&out_prompt);
    assert_eq!(session_ids(&working_a), ["sess-a"], "{working_a}");
    let start_b = read_json(&out_b);
    assert_eq!(
        session_ids(&start_b),
        ["sess-b"],
        "ancestor tty should replace A with B: {start_b}"
    );
    assert_eq!(start_b["snapshot"]["tracked_count"], 1);

    let terminals = persisted_terminal_ids(&state_path);
    assert_eq!(
        terminals,
        [pty.clone()],
        "terminal_id should be the script pty {pty}: {terminals:?}"
    );

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wrapper_start_and_setsid_hook_share_the_same_pty_id() {
    let root = isolated_root();
    let socket = root.join("dock.sock");
    fs::create_dir_all(root.join("home")).unwrap();
    let state_path = root.join("state.json");
    let service = agent_activity_dock_service::spawn_persistent(&socket, &state_path)
        .expect("spawn isolated dockd");

    let payload_b = root.join("b.json");
    fs::write(
        &payload_b,
        r#"{"hookEventName":"session_start","sessionId":"sess-b","event_id":"e-b-start"}"#,
    )
    .unwrap();

    let dock = env!("CARGO_BIN_EXE_dock");
    let pty_file = root.join("pty");
    let out_start = root.join("out-start.json");
    let out_hook = root.join("out-hook.json");
    let inner = format!(
        "set -eu\ntty > {pty}\n{dock} --socket {sock} --json start wrapper-sess --source probe > {os}\nsetsid -w {dock} --socket {sock} --json hook grok < {b} > {oh}\n",
        pty = pty_file.display(),
        dock = dock,
        sock = socket.display(),
        os = out_start.display(),
        b = payload_b.display(),
        oh = out_hook.display(),
    );
    run_script(&root, &socket, &root.join("capture"), &inner);

    let pty = fs::read_to_string(&pty_file).unwrap().trim().to_owned();
    assert!(pty.starts_with("/dev/pts/"), "got {pty:?}");

    let started = read_json(&out_start);
    assert_eq!(session_ids(&started), ["wrapper-sess"], "{started}");
    let hooked = read_json(&out_hook);
    assert_eq!(
        session_ids(&hooked),
        ["sess-b"],
        "setsid hook should replace wrapper start: {hooked}"
    );
    let terminals = persisted_terminal_ids(&state_path);
    assert_eq!(
        terminals,
        [pty.clone()],
        "wrapper and setsid hook must share {pty}: {terminals:?}"
    );

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}
