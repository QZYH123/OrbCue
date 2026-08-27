#![cfg(unix)]

mod common;

use common::{isolated_root, run_orb_hook, write_exec};
use std::path::Path;
use std::time::{Duration, Instant};

fn run_hook(
    root: &Path,
    provider: &str,
    payload: &[u8],
    extra: &dyn Fn(&mut std::process::Command),
) -> std::process::Output {
    run_orb_hook(root, provider, payload, extra)
}

fn run_grok_stop_hook(root: &Path, windows_dock: &Path) -> std::process::Output {
    run_hook(
        root,
        "grok",
        br#"{"hookEventName":"stop","sessionId":"other-project","reason":"end_turn"}"#,
        &|command| {
            command.env("ORBCUE_WINDOWS_ORB", windows_dock);
        },
    )
}

#[test]
fn grok_stop_hook_is_fail_open_when_windows_emit_exits_2() {
    let root = isolated_root("orbcue-hook-fail-open");
    let stub = root.join("orb.exe");
    write_exec(
        &stub,
        "#!/bin/sh\necho 'orb: cannot reach Dock named pipe; start the presenter or `orb up` (requires orbd.exe)' >&2\nexit 2\n",
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
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn observer_hooks_hide_windows_trampoline_summary() {
    let cases: [(&str, &[u8]); 3] = [
        (
            "claude",
            br#"{"hook_event_name":"Stop","session_id":"claude-stdout"}"#,
        ),
        (
            "codex",
            br#"{"hook_event_name":"Stop","session_id":"codex-stdout"}"#,
        ),
        (
            "grok",
            br#"{"hookEventName":"stop","sessionId":"grok-stdout","reason":"end_turn"}"#,
        ),
    ];
    for (provider, payload) in cases {
        let root = isolated_root("orbcue-hook-fail-open");
        let stub = root.join("orb.exe");
        write_exec(
            &stub,
            "#!/bin/sh\necho 'accepted — 1/1 · pending 0'\nexit 0\n",
        );
        let output = run_hook(&root, provider, payload, &|command| {
            command.env("ORBCUE_WINDOWS_ORB", &stub);
        });
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{provider} trampoline hook must stay fail-open: {:?}\nstdout: {stdout}\nstderr: {stderr}",
            output.status.code()
        );
        assert!(
            stdout.trim().is_empty(),
            "{provider} must not print the Windows emit summary into the agent: {stdout:?}\nstderr: {stderr}"
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn grok_detach_returns_before_windows_trampoline_finishes() {
    let root = isolated_root("orbcue-hook-fail-open");
    let stub = root.join("orb.exe");
    write_exec(
        &stub,
        "#!/bin/sh\nsleep 2\necho 'accepted — 1/1 · pending 0'\nexit 0\n",
    );
    let started = Instant::now();
    let output = run_hook(
        &root,
        "grok",
        br#"{"hookEventName":"post_tool_use","sessionId":"grok-detach","reason":"end_turn"}"#,
        &|command| {
            command.arg("--detach").env("ORBCUE_WINDOWS_ORB", &stub);
        },
    );
    let elapsed = started.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(0),
        "detach must stay fail-open: {:?}\nstdout: {stdout}\nstderr: {stderr}",
        output.status.code()
    );
    assert!(
        stdout.trim().is_empty(),
        "detach parent must not print trampoline summary: {stdout:?}\nstderr: {stderr}"
    );
    assert!(
        elapsed < Duration::from_millis(1500),
        "Grok PostToolUse --detach must return before the 2s trampoline: {elapsed:?}\nstderr: {stderr}"
    );
    let _ = std::fs::remove_dir_all(root);
}
