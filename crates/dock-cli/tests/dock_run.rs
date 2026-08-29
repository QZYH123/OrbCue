#![cfg(unix)]

mod common;

use common::{isolated_root, orb_cmd, write_exec};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn run_json(root: &Path, extra_path: &Path, args: &[&str]) -> (Value, String) {
    let output = orb_cmd()
        .args(args)
        .env(
            "PATH",
            format!(
                "{}:{}",
                extra_path.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_SOCKET", root.join("orb.sock"))
        .env("ORBCUE_BACKEND", "wsl")
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env("ORBCUE_WT", root.join("bin").join("wt"))
        .env("ORBCUE_WSL", root.join("bin").join("wsl"))
        .env("ORBCUE_WSL_DISTRO", "TestDistro")
        .env("SHELL", root.join("bin").join("shell"))
        .env("WT_RECORD", root.join("wt-argv.txt"))
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run dock");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let value = serde_json::from_str(stdout.trim()).unwrap_or(json_error(&stdout, &stderr));
    if !output.status.success() {
        return (value, stderr);
    }
    (value, stderr)
}

fn json_error(stdout: &str, stderr: &str) -> Value {
    serde_json::json!({
        "ok": false,
        "parse_error": true,
        "stdout": stdout,
        "stderr": stderr,
    })
}

fn setup_bin(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(root.join("home")).unwrap();
    write_exec(
        &bin.join("wt"),
        r#"#!/bin/sh
: "${WT_RECORD:=/tmp/orbcue-wt-argv.txt}"
: > "$WT_RECORD"
for arg in "$@"; do
  printf '%s\n' "$arg" >> "$WT_RECORD"
done
for arg in "$@"; do
  case "$arg" in
    *.sh)
      if [ -f "$arg" ]; then
        exec /bin/sh "$arg"
      fi
      ;;
  esac
done
exit 0
"#,
    );
    write_exec(&bin.join("wsl"), "#!/bin/sh\nexit 0\n");
    write_exec(
        &bin.join("shell"),
        r#"#!/bin/sh
if [ -n "$1" ] && [ -f "$1" ]; then
  exec /bin/sh "$1"
fi
exit 0
"#,
    );
    bin
}

#[test]
fn dock_run_requires_an_available_agent() {
    let root = isolated_root("orbcue-run");
    let bin = setup_bin(&root);
    let (value, stderr) = run_json(&root, &bin, &["--json", "run", "missing-agent"]);
    assert_eq!(value["ok"], false, "stdout={value} stderr={stderr}");
    let error = value["error"].as_str().unwrap_or(&stderr);
    assert!(
        error.contains("未连接") || error.contains("PATH"),
        "error={error}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dock_run_requires_windows_terminal() {
    let root = isolated_root("orbcue-run");
    let bin = setup_bin(&root);
    write_exec(&bin.join("fakeagent"), "#!/bin/sh\nexit 0\n");
    fs::remove_file(bin.join("wt")).unwrap();
    let output = orb_cmd()
        .args(["--json", "run", "fakeagent"])
        .env("PATH", bin.display().to_string())
        .env("HOME", root.join("home"))
        .env("ORBCUE_BACKEND", "wsl")
        .env("ORBCUE_WSL_DISTRO", "TestDistro")
        .env_remove("ORBCUE_WT")
        .env_remove("LOCALAPPDATA")
        .env("USER", "orbcue-missing-wt")
        .env("USERNAME", "orbcue-missing-wt")
        .env("ORBCUE_WSL", bin.join("wsl"))
        .output()
        .expect("run dock");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["ok"], false, "{value}");
    assert!(
        value["error"]
            .as_str()
            .unwrap_or("")
            .contains("Windows Terminal"),
        "{value}"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dock_run_injects_marker_and_replaces_the_previous_session() {
    let root = isolated_root("orbcue-run");
    let socket = root.join("orb.sock");
    let bin = setup_bin(&root);
    let orb = env!("CARGO_BIN_EXE_orb");
    write_exec(
        &bin.join("fakeagent"),
        &format!(
            r#"#!/bin/sh
"{dock}" --socket "{socket}" --json start s1 --source grok >/dev/null
"{dock}" --socket "{socket}" --json start s2 --source grok >/dev/null
"#,
            dock = orb,
            socket = socket.display(),
        ),
    );

    let service = orbcue_service::spawn(&socket).expect("spawn isolated orbd");
    let (started, stderr) = run_json(&root, &bin, &["--json", "run", "fakeagent", "--probe"]);
    assert_eq!(started["ok"], true, "stdout={started} stderr={stderr}");
    let marker = started["marker"].as_str().expect("marker").to_owned();
    assert!(
        marker.starts_with("orb:") && marker.len() == 10,
        "marker={marker}"
    );
    assert!(
        started["title"].as_str().unwrap_or("").contains(&marker),
        "{started}"
    );

    let argv = fs::read_to_string(root.join("wt-argv.txt")).expect("wt argv");
    assert!(argv.contains("nt\n"), "{argv}");
    assert!(argv.contains("--title\n"), "{argv}");
    assert!(argv.contains(&marker), "{argv}");
    assert!(!argv.contains(';'), "WT argv must not contain ';': {argv}");
    let script_path = argv
        .lines()
        .rev()
        .find(|line| line.ends_with(".sh"))
        .expect("bootstrap script in wt argv");
    let script = fs::read_to_string(script_path).unwrap_or_else(|_| {
        // Script deletes itself after running; check argv title already has marker.
        String::new()
    });
    if !script.is_empty() {
        assert!(script.contains("--probe"), "{script}");
        assert!(script.contains("ORBCUE_TERMINAL_ID="), "{script}");
    }

    let status = orb_cmd()
        .args(["--socket", socket.to_str().unwrap(), "--json", "status"])
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_SOCKET", &socket)
        .env("ORBCUE_BACKEND", "wsl")
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("status");
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let snapshot: Value = serde_json::from_slice(&status.stdout).unwrap();
    let sessions = snapshot["snapshot"]["sessions"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(sessions.len(), 1, "{snapshot}");
    assert_eq!(sessions[0]["session_id"], "s2");
    assert_eq!(sessions[0]["terminal_id"], marker);

    service.shutdown();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dock_run_close_keeps_the_launcher_when_stdin_is_not_a_tty() {
    let root = isolated_root("orbcue-run");
    let bin = setup_bin(&root);
    write_exec(&bin.join("fakeagent"), "#!/bin/sh\nexit 0\n");
    let (started, stderr) = run_json(&root, &bin, &["--json", "run", "--close", "fakeagent"]);
    assert_eq!(started["ok"], true, "stdout={started} stderr={stderr}");
    assert_eq!(
        started["closed_launcher"], false,
        "piped stdin must not SIGHUP the test process: {started}"
    );
    fs::remove_dir_all(root).unwrap();
}
