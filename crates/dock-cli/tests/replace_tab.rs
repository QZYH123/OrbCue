#![cfg(unix)]

mod common;

use common::{isolated_root, orb_cmd};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn replace_tab_json(root: &Path, args: &[&str]) -> Value {
    let output = orb_cmd()
        .args(args)
        .env("HOME", root.join("home"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("ORBCUE_SOCKET", root.join("orb.sock"))
        .env("ORBCUE_BACKEND", "wsl")
        .env("ORBCUE_ORBD", root.join("missing-orbd"))
        .env_remove("XDG_RUNTIME_DIR")
        .output()
        .expect("run replace-tab");
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
fn set_get_and_clear_replace_tab_preference() {
    let root = isolated_root("orbcue-replace-tab");
    fs::create_dir_all(root.join("home")).unwrap();
    let unset = replace_tab_json(&root, &["replace-tab", "--json"]);
    assert_eq!(unset["ok"], true, "{unset}");
    assert_eq!(unset["replace_tab"], false);

    let enabled = replace_tab_json(&root, &["replace-tab", "--enable", "--json"]);
    assert_eq!(enabled["ok"], true, "{enabled}");
    assert_eq!(enabled["replace_tab"], true);
    assert_eq!(
        fs::read_to_string(root.join("state").join("orbcue").join("run-replace-tab"))
            .unwrap()
            .trim(),
        "1"
    );

    let got = replace_tab_json(&root, &["replace-tab", "--json"]);
    assert_eq!(got["replace_tab"], true);

    let disabled = replace_tab_json(&root, &["replace-tab", "--disable", "--json"]);
    assert_eq!(disabled["ok"], true, "{disabled}");
    assert_eq!(disabled["replace_tab"], false);
    assert!(!root
        .join("state")
        .join("orbcue")
        .join("run-replace-tab")
        .exists());

    fs::remove_dir_all(root).unwrap();
}
