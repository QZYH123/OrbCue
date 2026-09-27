#![cfg(unix)]

mod common;

use common::{isolated_root, orb_cmd};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn direct_json(root: &Path, args: &[&str]) -> Value {
    let output = orb_cmd()
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_SOCKET", root.join("orb.sock"))
        .env_remove("WSL_DISTRO_NAME")
        .env_remove("WSL_INTEROP")
        .env_remove("ORBCUE_WINDOWS_ORB")
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env_remove("XDG_RUNTIME_DIR")
        .env("SHELL", "/bin/bash")
        .output()
        .expect("run orb direct-run");
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
fn enable_and_disable_without_a_connected_agent() {
    let root = isolated_root("orbcue-direct-run");
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "export KEEP=1\n").unwrap();

    let enabled = direct_json(&root, &["direct-run", "--enable", "--json"]);
    assert_eq!(enabled["ok"], true, "{enabled}");
    assert_eq!(enabled["enabled"], true);
    assert!(enabled["commands"].as_array().unwrap().is_empty());
    assert_eq!(
        fs::read_to_string(home.join(".bashrc")).unwrap(),
        "export KEEP=1\n"
    );
    assert!(root
        .join("state")
        .join("orbcue")
        .join("direct-run")
        .is_file());

    let again = direct_json(&root, &["direct-run", "--json"]);
    assert_eq!(again["enabled"], true);

    fs::write(
        home.join(".bashrc"),
        "export KEEP=1\n# >>> orbcue direct-run >>>\nalias grok='command orb run grok'\n# <<< orbcue direct-run <<<\n",
    )
    .unwrap();
    let disabled = direct_json(&root, &["direct-run", "--disable", "--json"]);
    assert_eq!(disabled["ok"], true, "{disabled}");
    assert_eq!(disabled["enabled"], false);
    assert_eq!(
        fs::read_to_string(home.join(".bashrc")).unwrap(),
        "export KEEP=1\n"
    );
    assert!(!root
        .join("state")
        .join("orbcue")
        .join("direct-run")
        .exists());
}
