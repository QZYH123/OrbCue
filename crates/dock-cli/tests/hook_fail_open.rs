#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("aadock-hook-fail-open-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_exec(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn run_grok_stop_hook(root: &Path, windows_dock: &Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_dock"))
        .args(["hook", "grok"])
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("AGENT_ACTIVITY_DOCK_SOCKET", root.join("missing.sock"))
        .env("AGENT_ACTIVITY_DOCK_BACKEND", "local")
        .env("AGENT_ACTIVITY_DOCK_DOCKD", root.join("missing-dockd"))
        .env("AGENT_ACTIVITY_DOCK_WINDOWS_DOCK", windows_dock)
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("AGENT_ACTIVITY_DOCK_HOP")
        .env_remove("AGENT_ACTIVITY_DOCK_FORWARD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dock hook grok");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"hookEventName":"stop","sessionId":"other-project","reason":"end_turn"}"#)
        .unwrap();
    child.wait_with_output().expect("wait dock hook grok")
}

#[test]
fn grok_stop_hook_is_fail_open_when_windows_emit_exits_2() {
    let root = isolated_root();
    let stub = root.join("dock.exe");
    write_exec(
        &stub,
        "#!/bin/sh\necho 'dock: cannot reach Dock named pipe; start the presenter or `dock up` (requires dockd.exe)' >&2\nexit 2\n",
    );

    let output = run_grok_stop_hook(&root, &stub);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Grok treats hook exit 2 as a Stop gate and continues the agent with stderr; got {:?}\nstdout: {}\nstderr: {stderr}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
    );
    let _ = fs::remove_dir_all(root);
}
