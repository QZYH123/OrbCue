#![cfg(unix)]

mod common;

use common::{isolated_root, run_orb_hook, write_exec};

fn official_session_start() -> &'static [u8] {
    br#"{"hook_event_name":"sessionStart","conversation_id":"cursor-stdout","session_id":"cursor-stdout","workspace_roots":["/tmp/workspace"],"cursor_version":"2026.08.25-3e8eec8"}"#
}

fn run_cursor_hook(
    root: &std::path::Path,
    extra: &dyn Fn(&mut std::process::Command),
) -> std::process::Output {
    run_orb_hook(root, "cursor", official_session_start(), extra)
}

#[test]
fn cursor_hook_prints_empty_json_object_when_dock_is_missing() {
    let root = isolated_root("orbcue-cursor-hook-stdout");
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
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cursor_hook_hides_windows_trampoline_summary() {
    let root = isolated_root("orbcue-cursor-hook-stdout");
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
    let _ = std::fs::remove_dir_all(root);
}
