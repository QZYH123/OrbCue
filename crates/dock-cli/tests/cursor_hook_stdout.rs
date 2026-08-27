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
    let root = std::env::temp_dir().join(format!("orbcue-cursor-hook-stdout-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_exec(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn official_session_start() -> &'static [u8] {
    br#"{"hook_event_name":"sessionStart","conversation_id":"cursor-stdout","session_id":"cursor-stdout","workspace_roots":["/tmp/workspace"],"cursor_version":"2026.08.25-3e8eec8"}"#
}

fn run_cursor_hook(root: &Path, extra: &dyn Fn(&mut Command)) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_orb"));
    command
        .args(["hook", "cursor"])
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_SOCKET", root.join("missing.sock"))
        .env("ORBCUE_BACKEND", "local")
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("ORBCUE_FORWARD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    extra(&mut command);
    let mut child = command.spawn().expect("spawn orb hook cursor");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(official_session_start())
        .unwrap();
    child.wait_with_output().expect("wait orb hook cursor")
}

#[test]
fn cursor_hook_prints_empty_json_object_when_dock_is_missing() {
    let root = isolated_root();
    let output = run_cursor_hook(&root, &|command| {
        command
            .env("ORBCUE_HOP", "wsl")
            .env_remove("ORBCUE_WINDOWS_ORB");
    });
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Cursor hook must stay fail-open: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert_eq!(
        stdout.trim(),
        "{}",
        "Cursor CLI treats empty/non-JSON stdout as a failed hook; got {stdout:?}\nstderr: {stderr}"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn cursor_hook_hides_windows_trampoline_summary() {
    let root = isolated_root();
    let stub = root.join("orb.exe");
    write_exec(
        &stub,
        "#!/bin/sh\necho 'accepted — cursor · pending 1'\nexit 0\n",
    );
    let output = run_cursor_hook(&root, &|command| {
        command
            .env("ORBCUE_WINDOWS_ORB", &stub)
            .env_remove("ORBCUE_HOP");
    });
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "Cursor trampoline hook must stay fail-open: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert_eq!(
        stdout.trim(),
        "{}",
        "Windows emit summary must not leak into Cursor stdout: {stdout:?}\nstderr: {stderr}"
    );
    assert!(
        !stdout.contains("accepted"),
        "human trampoline summary leaked: {stdout:?}"
    );
    let _ = fs::remove_dir_all(root);
}
