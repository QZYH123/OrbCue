#![cfg(unix)]

use agent_activity_dock_connect::{
    AgentOrigin, ConnectionManager, ConnectionMethod, PreviewAction,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

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
    let _guard = lock_env();
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
    assert!(document.contains("PreToolUse"));
    assert!(document.contains("PostToolUse"));
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

fn file_contents(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    fn walk(dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, files);
            } else {
                files.insert(path.clone(), fs::read(&path).unwrap());
            }
        }
    }
    walk(root, &mut files);
    files
}

fn all_paths(root: &Path) -> BTreeSet<PathBuf> {
    let mut paths = BTreeSet::new();
    fn walk(dir: &Path, paths: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            paths.insert(path.clone());
            if path.is_dir() {
                walk(&path, paths);
            }
        }
    }
    walk(root, &mut paths);
    paths
}

fn with_shell(shell: &str, body: impl FnOnce()) {
    let _guard = lock_env();
    let previous = std::env::var_os("SHELL");
    std::env::set_var("SHELL", shell);
    body();
    match previous {
        Some(value) => std::env::set_var("SHELL", value),
        None => std::env::remove_var("SHELL"),
    }
}

#[test]
fn preview_is_side_effect_free() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "export EXISTING=1\n").unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );
    let before_paths = all_paths(&root);
    let before_files = file_contents(&root);
    with_shell("/bin/bash", || {
        let preview = manager.preview("codex", &original).unwrap();
        assert!(preview.dry_run);
        assert_eq!(preview.method, ConnectionMethod::Wrapper);
        assert!(preview
            .will_not
            .iter()
            .any(|line| line.contains("不替换 Agent 本体")));
        assert!(preview
            .will_not
            .iter()
            .any(|line| line.contains("不修改、不删除用户其他 Hook")));
        assert!(preview
            .will_not
            .iter()
            .any(|line| line.contains("transcript")));
    });
    assert_eq!(all_paths(&root), before_paths);
    assert_eq!(file_contents(&root), before_files);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn connect_writes_exactly_the_previewed_paths() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "export EXISTING=1\n").unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );
    with_shell("/bin/bash", || {
        let before = file_contents(&root);
        let preview = manager.preview("codex", &original).unwrap();
        manager.connect("codex", &original).unwrap();
        let after = file_contents(&root);
        let changed: BTreeSet<_> = after
            .iter()
            .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
            .map(|(path, _)| path.clone())
            .collect();
        let previewed: BTreeSet<_> = preview.files.iter().map(|file| file.path.clone()).collect();
        assert_eq!(changed, previewed);
        assert!(preview
            .files
            .iter()
            .any(|file| file.entries.iter().any(|entry| entry == "started")));
        assert!(preview.files.iter().any(|file| {
            file.path.file_name().and_then(|name| name.to_str()) == Some(".bashrc")
                && file.action == PreviewAction::Modify
        }));
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refusing_non_dock_overwrite_still_works() {
    let root = temp_root();
    let home = root.join("home");
    let data = root.join("data");
    fs::create_dir_all(&home).unwrap();
    let wrapper = data.join("agent-activity-dock").join("codex");
    fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
    fs::write(&wrapper, "user-owned wrapper\n").unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(home, root.join("config"), data, root.join("dock"));
    with_shell("/bin/bash", || {
        let preview = manager.preview("codex", &original).unwrap();
        assert!(preview.files.iter().any(|file| file.path == wrapper));
        let error = manager.connect("codex", &original).unwrap_err();
        assert!(error.contains("refusing to overwrite non-Dock file"));
        assert_eq!(
            fs::read_to_string(&wrapper).unwrap(),
            "user-owned wrapper\n"
        );
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_preview_notes_backup_and_disconnect_keeps_other_hooks() {
    let root = temp_root();
    let home = root.join("home");
    let claude_dir = root.join("claude-config");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&claude_dir).unwrap();
    let settings = claude_dir.join("settings.json");
    fs::write(
        &settings,
        br#"{"hooks":{"UserEvent":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
    )
    .unwrap();
    let original = root.join("claude-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );
    let _guard = lock_env();
    let previous = std::env::var_os("CLAUDE_CONFIG_DIR");
    std::env::set_var("CLAUDE_CONFIG_DIR", &claude_dir);
    let preview = manager.preview("claude", &original).unwrap();
    assert_eq!(preview.method, ConnectionMethod::ClaudeHook);
    assert!(preview
        .notes
        .iter()
        .any(|note| note.contains("settings.json") && note.contains("备份")));
    assert!(preview.files.iter().any(|file| {
        file.path == settings && file.entries.iter().any(|entry| entry == "SessionStart")
    }));
    manager.connect("claude", &original).unwrap();
    let connected = fs::read_to_string(&settings).unwrap();
    assert!(connected.contains("SessionStart"));
    assert!(connected.contains("PreToolUse"));
    assert!(connected.contains("user-hook"));
    assert!(manager.disconnect("claude").unwrap());
    let remaining = fs::read_to_string(&settings).unwrap();
    assert!(remaining.contains("user-hook"));
    assert!(!remaining.contains("claude-hook"));
    match previous {
        Some(value) => std::env::set_var("CLAUDE_CONFIG_DIR", value),
        None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
    }
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
    let path = std::env::join_paths([managed_bin, real_bin.clone()]).unwrap();
    let discovered = manager.discover_from_path(&path);

    assert_eq!(discovered[0].name, "codex");
    assert_eq!(discovered[0].path, original);
    assert_eq!(discovered[0].origin, AgentOrigin::Wsl);
    assert!(discovered[0].connectable);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discover_from_path_finds_local_bin_and_prefers_wsl_over_windows() {
    let root = temp_root();
    let local_bin = root.join("home").join(".local").join("bin");
    let windows_bin = root
        .join("mnt")
        .join("c")
        .join("Users")
        .join("u")
        .join("AppData")
        .join("Roaming")
        .join("npm");
    fs::create_dir_all(&local_bin).unwrap();
    fs::create_dir_all(&windows_bin).unwrap();
    let wsl_claude = local_bin.join("claude");
    let windows_claude = windows_bin.join("claude");
    executable(&wsl_claude, "#!/bin/sh\nexit 0\n");
    executable(&windows_claude, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        root.join("home"),
        root.join("config"),
        root.join("data"),
        root.join("dock"),
    );

    let windows_first = std::env::join_paths([&windows_bin, &local_bin]).unwrap();
    let discovered = manager.discover_from_path(&windows_first);
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].path, wsl_claude);
    assert_eq!(discovered[0].origin, AgentOrigin::Wsl);
    assert!(discovered[0].connectable);

    fs::remove_file(&wsl_claude).unwrap();
    let windows_only = manager.discover_from_path(&windows_first);
    assert!(
        windows_only.is_empty(),
        "/mnt/* Windows interop agents are discovered on the Windows side: {windows_only:?}"
    );
    let _ = windows_claude;
    fs::remove_dir_all(root).unwrap();
}
