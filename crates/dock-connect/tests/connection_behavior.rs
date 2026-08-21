#![cfg(unix)]

use agent_activity_dock_connect::{ConnectionManager, ConnectionMethod};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root() -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("aadock-connect-{nonce}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn executable(path: &std::path::Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn wrapper_preserves_arguments_and_exit_code_while_emitting_lifecycle() {
    let root = temp_root();
    let home = root.join("home");
    let config = root.join("config");
    let data = root.join("data");
    fs::create_dir_all(&home).unwrap();
    let dock = root.join("dock");
    let original = root.join("codex-real");
    executable(
        &dock,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$AADOCK_TEST_LOG\"\n",
    );
    executable(
        &original,
        "#!/bin/sh\n[ \"$1\" = '--model' ] || exit 99\nexit 17\n",
    );
    let log = root.join("events.log");
    let manager = ConnectionManager::new(home.clone(), config, data.clone(), dock);

    let record = manager.connect("codex", &original).unwrap();
    assert_eq!(record.method, ConnectionMethod::Wrapper);
    let wrapper = record.wrapper.unwrap();
    assert_eq!(wrapper, data.join("agent-activity-dock").join("codex"));
    let result = Command::new(&wrapper)
        .arg("--model")
        .arg("gpt-test")
        .env("AADOCK_TEST_LOG", &log)
        .status()
        .unwrap();
    assert_eq!(result.code(), Some(17));
    let events = fs::read_to_string(log).unwrap();
    assert!(events.contains("start"));
    assert!(events.contains("fail"));
    let written_profiles = [".profile", ".zshrc", ".bashrc"]
        .into_iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    assert!(!written_profiles.is_empty());
    assert!(written_profiles.iter().all(|path| {
        fs::read_to_string(path)
            .unwrap()
            .contains("agent-activity-dock PATH")
    }));
    let failed_refresh = manager.connect("codex", &wrapper);
    assert!(failed_refresh.is_err());
    assert!(wrapper.exists());
    assert!(manager.disconnect("codex").unwrap());
    assert!(!wrapper.exists());
    assert!(written_profiles.iter().all(|path| {
        !fs::read_to_string(path)
            .unwrap()
            .contains("agent-activity-dock PATH")
    }));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn wrapper_path_is_injected_into_every_existing_shell_profile() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(home.join(".config").join("powershell")).unwrap();
    fs::create_dir_all(home.join(".config").join("fish")).unwrap();
    fs::write(home.join(".bashrc"), "export EXISTING=1\n").unwrap();
    fs::write(home.join(".zshrc"), "export EXISTING=1\n").unwrap();
    fs::write(
        home.join(".config")
            .join("powershell")
            .join("Microsoft.PowerShell_profile.ps1"),
        "Write-Host existing\n",
    )
    .unwrap();
    fs::write(
        home.join(".config").join("fish").join("config.fish"),
        "set -gx EXISTING 1\n",
    )
    .unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home.clone(),
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );

    manager.connect("codex", &original).unwrap();
    let bashrc = fs::read_to_string(home.join(".bashrc")).unwrap();
    let zshrc = fs::read_to_string(home.join(".zshrc")).unwrap();
    let pwsh = fs::read_to_string(
        home.join(".config")
            .join("powershell")
            .join("Microsoft.PowerShell_profile.ps1"),
    )
    .unwrap();
    let fish = fs::read_to_string(home.join(".config").join("fish").join("config.fish")).unwrap();
    assert!(bashrc.contains("export PATH="));
    assert!(zshrc.contains("export PATH="));
    assert!(pwsh.contains("$env:PATH"));
    assert!(fish.contains("set -gx PATH"));
    assert!(manager.disconnect("codex").unwrap());
    assert!(!fs::read_to_string(home.join(".zshrc"))
        .unwrap()
        .contains("agent-activity-dock PATH"));
    assert!(!fs::read_to_string(
        home.join(".config")
            .join("powershell")
            .join("Microsoft.PowerShell_profile.ps1")
    )
    .unwrap()
    .contains("agent-activity-dock PATH"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zsh_users_get_a_zshrc_snippet_even_when_only_bashrc_exists() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "export EXISTING=1\n").unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home.clone(),
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );
    let old_shell = std::env::var_os("SHELL");
    std::env::set_var("SHELL", "/usr/bin/zsh");
    manager.connect("codex", &original).unwrap();
    match old_shell {
        Some(value) => std::env::set_var("SHELL", value),
        None => std::env::remove_var("SHELL"),
    }
    let zshrc = fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(zshrc.contains("export PATH="));
    assert!(fs::read_to_string(home.join(".bashrc"))
        .unwrap()
        .contains("export PATH="));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn grok_connect_writes_a_revocable_hook_file() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let original = root.join("grok-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home.clone(),
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );

    let record = manager.connect("grok", &original).unwrap();
    assert_eq!(record.method, ConnectionMethod::GrokHook);
    let hooks = home
        .join(".grok")
        .join("hooks")
        .join("agent-activity-dock.json");
    let document = fs::read_to_string(&hooks).unwrap();
    assert!(document.contains("SessionStart"));
    assert!(document.contains("UserPromptSubmit"));
    assert!(document.contains("SessionEnd"));
    assert!(document.contains("agent-activity-dock"));
    assert!(document.contains("grok-hook"));
    assert!(manager.disconnect("grok").unwrap());
    assert!(!hooks.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn connection_names_cannot_escape_the_managed_data_directory() {
    let root = temp_root();
    let original = root.join("agent");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        root.join("home"),
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );

    let error = manager.connect("../../outside", &original).unwrap_err();
    assert!(error.contains("agent name"));
    assert!(!root.join("outside").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_skips_the_managed_wrapper_and_finds_the_real_agent() {
    let root = temp_root();
    let home = root.join("home");
    let config = root.join("config");
    let data = root.join("data");
    let real_bin = root.join("real-bin");
    fs::create_dir_all(&real_bin).unwrap();
    fs::create_dir_all(&home).unwrap();
    let original = real_bin.join("codex");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = agent_activity_dock_connect::ConnectionManager::new(
        home,
        config,
        data.clone(),
        root.join("dock"),
    );
    manager.connect("codex", &original).unwrap();

    let managed_bin = data.join("agent-activity-dock");
    let old_path = std::env::var_os("PATH");
    let path = std::env::join_paths([managed_bin, real_bin.clone()]).unwrap();
    std::env::set_var("PATH", path);
    let discovered = manager.discover();
    match old_path {
        Some(path) => std::env::set_var("PATH", path),
        None => std::env::remove_var("PATH"),
    }

    assert_eq!(discovered[0].name, "codex");
    assert_eq!(discovered[0].path, original);
    fs::remove_dir_all(root).unwrap();
}
