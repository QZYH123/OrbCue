#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn isolated_root(prefix: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("{prefix}-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

pub fn write_exec(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn run_orb_hook(
    root: &Path,
    provider: &str,
    payload: &[u8],
    extra: &dyn Fn(&mut Command),
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_orb"));
    command
        .args(["hook", provider])
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_SOCKET", root.join("missing.sock"))
        .env("ORBCUE_BACKEND", "local")
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env_remove("XDG_RUNTIME_DIR")
        .env_remove("ORBCUE_HOP")
        .env_remove("ORBCUE_FORWARD")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    extra(&mut command);
    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("spawn orb hook {provider}: {error}"));
    child.stdin.as_mut().unwrap().write_all(payload).unwrap();
    child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("wait orb hook {provider}: {error}"))
}
