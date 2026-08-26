#![cfg(unix)]

use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("orbcue-alias-{nonce}"));
    fs::create_dir_all(&root).unwrap();
    root
}

fn orb_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_orb"))
}

fn alias_json(root: &Path, args: &[&str]) -> Value {
    let output = orb_cmd()
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_BIN", root.join("bin"))
        .env("ORBCUE_SOCKET", root.join("orb.sock"))
        .env("ORBCUE_BACKEND", "wsl")
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run dock alias");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    serde_json::from_str(stdout.trim()).unwrap_or_else(|_| {
        serde_json::json!({
            "ok": false,
            "stdout": stdout,
            "stderr": String::from_utf8_lossy(&output.stderr),
            "status": output.status.code(),
        })
    })
}

#[test]
fn set_get_and_clear_alias() {
    let root = isolated_root();
    fs::create_dir_all(root.join("home").join(".local").join("bin")).unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    let set = alias_json(&root, &["alias", "dr", "--json"]);
    assert_eq!(set["ok"], true);
    assert_eq!(set["alias"], "dr");

    let shim = root.join("bin").join("dr");
    let text = fs::read_to_string(&shim).unwrap();
    assert!(text.contains("orb run"));
    assert_eq!(shim.metadata().unwrap().permissions().mode() & 0o111, 0o111);

    let got = alias_json(&root, &["alias", "--json"]);
    assert_eq!(got["alias"], "dr");

    let renamed = alias_json(&root, &["alias", "r", "--json"]);
    assert_eq!(renamed["alias"], "r");
    assert!(!shim.exists());
    assert!(root.join("bin").join("r").exists());

    let cleared = alias_json(&root, &["alias", "--clear", "--json"]);
    assert_eq!(cleared["ok"], true);
    assert!(cleared["alias"].is_null());
    assert!(!root.join("bin").join("r").exists());
}

#[test]
fn refuses_to_clobber_existing_command() {
    let root = isolated_root();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin").join("dr"), "echo nope\n").unwrap();
    let value = alias_json(&root, &["alias", "dr", "--json"]);
    assert_eq!(value["ok"], false);
    assert!(value["error"].as_str().unwrap().contains("同名"));
}
