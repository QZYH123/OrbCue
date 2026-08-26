#![cfg(unix)]

use orbcue_connect::{AgentOrigin, ConnectionManager, ConnectionMethod, PreviewAction};
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
    let path = std::env::temp_dir().join(format!("orbcue-connect-{nonce}"));
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
    let dock = root.join("orb");
    let original = root.join("dsh-real");
    executable(
        &dock,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$ORBCUE_TEST_LOG\"\n",
    );
    executable(
        &original,
        "#!/bin/sh\n[ \"$1\" = '--model' ] || exit 99\nexit 17\n",
    );
    let log = root.join("events.log");
    let manager = ConnectionManager::new(home.clone(), config, data.clone(), dock);

    let record = manager.connect("dsh", &original).unwrap();
    assert_eq!(record.method, ConnectionMethod::Wrapper);
    let wrapper = record.wrapper.unwrap();
    assert_eq!(wrapper, data.join("orbcue").join("dsh"));
    let result = Command::new(&wrapper)
        .arg("--model")
        .arg("gpt-test")
        .env("ORBCUE_TEST_LOG", &log)
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
    assert!(written_profiles
        .iter()
        .all(|path| { fs::read_to_string(path).unwrap().contains("orbcue PATH") }));
    let failed_refresh = manager.connect("dsh", &wrapper);
    assert!(failed_refresh.is_err());
    assert!(wrapper.exists());
    assert!(manager.disconnect("dsh").unwrap());
    assert!(!wrapper.exists());
    assert!(written_profiles
        .iter()
        .all(|path| { !fs::read_to_string(path).unwrap().contains("orbcue PATH") }));
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
    let original = root.join("dsh-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home.clone(),
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );

    manager.connect("dsh", &original).unwrap();
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
    assert!(manager.disconnect("dsh").unwrap());
    assert!(!fs::read_to_string(home.join(".zshrc"))
        .unwrap()
        .contains("orbcue PATH"));
    assert!(!fs::read_to_string(
        home.join(".config")
            .join("powershell")
            .join("Microsoft.PowerShell_profile.ps1")
    )
    .unwrap()
    .contains("orbcue PATH"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn zsh_users_get_a_zshrc_snippet_even_when_only_bashrc_exists() {
    let _guard = lock_env();
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".bashrc"), "export EXISTING=1\n").unwrap();
    let original = root.join("dsh-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home.clone(),
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let old_shell = std::env::var_os("SHELL");
    std::env::set_var("SHELL", "/usr/bin/zsh");
    manager.connect("dsh", &original).unwrap();
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
        root.join("orb"),
    );

    let record = manager.connect("grok", &original).unwrap();
    assert_eq!(record.method, ConnectionMethod::GrokHook);
    let hooks = home.join(".grok").join("hooks").join("orbcue.json");
    let document = fs::read_to_string(&hooks).unwrap();
    assert!(document.contains("SessionStart"));
    assert!(document.contains("UserPromptSubmit"));
    assert!(document.contains("\"Stop\""));
    assert!(document.contains("SessionEnd"));
    assert!(document.contains("\"PreToolUse\""));
    assert!(document.contains("\"PostToolUse\""));
    assert!(document.contains("\"PostToolUseFailure\""));
    assert!(document.contains("\"PermissionDenied\""));
    assert!(document.contains("\"ask_user_question\""));
    let parsed: serde_json::Value = serde_json::from_str(&document).unwrap();
    let pre = parsed["hooks"]["PreToolUse"][0].as_object().unwrap();
    assert_eq!(pre["matcher"], "ask_user_question");
    let post = parsed["hooks"]["PostToolUse"][0].as_object().unwrap();
    assert_eq!(post.get("matcher"), None);
    assert_eq!(parsed["hooks"]["SessionStart"][0].get("matcher"), None);
    assert!(document.contains("orbcue"));
    assert!(document.contains("grok-hook"));
    let script =
        fs::read_to_string(root.join("config").join("orbcue").join("grok-hook.sh")).unwrap();
    assert!(
        script.contains("exec "),
        "hook must exec orb so liveness PPID is the agent: {script}"
    );
    assert!(
        !script.contains("|| true"),
        "|| true would keep a short-lived shell as orb's parent: {script}"
    );
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
        root.join("orb"),
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
    let original = root.join("dsh-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let before_paths = all_paths(&root);
    let before_files = file_contents(&root);
    with_shell("/bin/bash", || {
        let preview = manager.preview("dsh", &original).unwrap();
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
    let original = root.join("dsh-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    with_shell("/bin/bash", || {
        let before = file_contents(&root);
        let preview = manager.preview("dsh", &original).unwrap();
        manager.connect("dsh", &original).unwrap();
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
    let wrapper = data.join("orbcue").join("dsh");
    fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
    fs::write(&wrapper, "user-owned wrapper\n").unwrap();
    let original = root.join("dsh-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(home, root.join("config"), data, root.join("orb"));
    with_shell("/bin/bash", || {
        let preview = manager.preview("dsh", &original).unwrap();
        assert!(preview.files.iter().any(|file| file.path == wrapper));
        let error = manager.connect("dsh", &original).unwrap_err();
        assert!(error.contains("refusing to overwrite non-OrbCue file"));
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
        root.join("orb"),
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
    assert!(connected.contains("UserPromptSubmit"));
    assert!(connected.contains("PermissionDenied"));
    assert!(connected.contains("\"Stop\""));
    assert!(connected.contains("\"PreToolUse\""));
    assert!(connected.contains("\"PostToolUse\""));
    assert!(connected.contains("AskUserQuestion"));
    let parsed: serde_json::Value = serde_json::from_str(&connected).unwrap();
    assert_eq!(
        parsed["hooks"]["PreToolUse"][0]["matcher"],
        "AskUserQuestion"
    );
    assert_eq!(parsed["hooks"]["PostToolUse"][0].get("matcher"), None);
    assert_eq!(parsed["hooks"]["SessionStart"][0].get("matcher"), None);
    assert!(connected.contains("user-hook"));
    fs::write(
        &settings,
        br#"{"hooks":{"UserEvent":[{"hooks":[{"type":"command","command":"user-hook"}]}],"SessionStart":[{"hooks":[{"type":"command","command":"/usr/bin/python3","args":["/tmp/agent-activity-dock/hooks/claude-hook.py"]}]}]}}"#,
    )
    .unwrap();
    manager.connect("claude", &original).unwrap();
    let refreshed = fs::read_to_string(&settings).unwrap();
    assert!(refreshed.contains("UserPromptSubmit"));
    assert!(refreshed.contains("\"Stop\""));
    assert!(!refreshed.contains("python3"));
    assert!(!refreshed.contains("claude-hook.py"));
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
    let original = real_bin.join("dsh");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager =
        orbcue_connect::ConnectionManager::new(home, config, data.clone(), root.join("orb"));
    manager.connect("dsh", &original).unwrap();

    let nested_stub = data.join("orbcue").join("bin").join("dsh");
    fs::create_dir_all(nested_stub.parent().unwrap()).unwrap();
    executable(&nested_stub, "#!/bin/sh\nexit 0\n");
    let managed_bin = data.join("orbcue");
    let path =
        std::env::join_paths([managed_bin.join("bin"), managed_bin, real_bin.clone()]).unwrap();
    let discovered = manager.discover_from_path(&path);

    assert_eq!(discovered[0].name, "dsh");
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
        root.join("orb"),
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

#[test]
fn discover_from_path_names_cursor_agent_cursor() {
    let root = temp_root();
    let local_bin = root.join("home").join(".local").join("bin");
    fs::create_dir_all(&local_bin).unwrap();
    let cursor_agent = local_bin.join("cursor-agent");
    let editor = local_bin.join("cursor");
    executable(&cursor_agent, "#!/bin/sh\nexit 0\n");
    executable(&editor, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        root.join("home"),
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let path = std::env::join_paths([&local_bin]).unwrap();
    let discovered = manager.discover_from_path(&path);
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].name, "cursor");
    assert_eq!(discovered[0].path, cursor_agent);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn discover_from_path_finds_well_known_install_dirs_without_path() {
    let root = temp_root();
    let home = root.join("home");
    let claude = home.join(".local").join("bin").join("claude");
    let grok = home.join(".grok").join("bin").join("grok");
    let cursor = home
        .join("AppData")
        .join("Local")
        .join("cursor-agent")
        .join("agent.cmd");
    fs::create_dir_all(claude.parent().unwrap()).unwrap();
    fs::create_dir_all(grok.parent().unwrap()).unwrap();
    fs::create_dir_all(cursor.parent().unwrap()).unwrap();
    executable(&claude, "#!/bin/sh\nexit 0\n");
    executable(&grok, "#!/bin/sh\nexit 0\n");
    fs::write(&cursor, b"@echo off\r\n").unwrap();
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let discovered = manager.discover_from_path(std::ffi::OsStr::new(""));
    let mut names: Vec<_> = discovered.iter().map(|agent| agent.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["claude", "cursor", "grok"]);
    assert!(discovered.iter().any(|agent| agent.path == claude));
    assert!(discovered.iter().any(|agent| agent.path == grok));
    assert!(discovered.iter().any(|agent| agent.path == cursor));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn add_scan_dir_persists_and_discovers_later() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let custom = root.join("my-tools");
    fs::create_dir_all(&custom).unwrap();
    let claude = custom.join("claude");
    executable(&claude, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let found = manager.add_scan_dir(&custom).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, claude);
    let empty = std::ffi::OsString::new();
    let discovered = manager.discover_from_path(&empty);
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].path, claude);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn add_scan_dir_rejects_folder_without_supported_tools() {
    let root = temp_root();
    let home = root.join("home");
    let empty = root.join("empty");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&empty).unwrap();
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let error = manager.add_scan_dir(&empty).unwrap_err();
    assert!(error.contains("没有支持的工具"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_connect_merges_hooks_json_and_keeps_other_hooks() {
    let root = temp_root();
    let home = root.join("home");
    let codex_dir = home.join(".codex");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&codex_dir).unwrap();
    let hooks = codex_dir.join("hooks.json");
    fs::write(
        &hooks,
        br#"{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#,
    )
    .unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let preview = manager.preview("codex", &original).unwrap();
    assert_eq!(preview.method, ConnectionMethod::CodexHook);
    assert!(preview
        .notes
        .iter()
        .any(|note| note.contains("hooks.json") && note.contains("备份")));
    let record = manager.connect("codex", &original).unwrap();
    assert_eq!(record.method, ConnectionMethod::CodexHook);
    let connected = fs::read_to_string(&hooks).unwrap();
    assert!(connected.contains("SessionStart"));
    assert!(connected.contains("UserPromptSubmit"));
    assert!(connected.contains("\"Stop\""));
    assert!(connected.contains("SessionEnd"));
    assert!(connected.contains("\"PreToolUse\""));
    assert!(connected.contains("AskUserQuestion"));
    let parsed: serde_json::Value = serde_json::from_str(&connected).unwrap();
    assert_eq!(
        parsed["hooks"]["PreToolUse"][0]["matcher"],
        "AskUserQuestion|ask_user_question"
    );
    assert_eq!(parsed["hooks"]["PostToolUse"][0].get("matcher"), None);
    assert!(!connected.contains("PostToolUseFailure"));
    assert!(connected.contains("user-hook"));
    assert!(connected.contains("codex-hook"));
    let script =
        fs::read_to_string(root.join("config").join("orbcue").join("codex-hook.sh")).unwrap();
    assert!(script.contains("exec "), "hook must exec orb: {script}");
    assert!(
        !script.contains("|| true"),
        "|| true would hide the agent PPID: {script}"
    );
    assert!(manager.disconnect("codex").unwrap());
    let remaining = fs::read_to_string(&hooks).unwrap();
    assert!(remaining.contains("user-hook"));
    assert!(!remaining.contains("codex-hook"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cursor_connect_writes_camelcase_hooks_and_keeps_other_hooks() {
    let root = temp_root();
    let home = root.join("home");
    let cursor_dir = home.join(".cursor");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&cursor_dir).unwrap();
    let hooks = cursor_dir.join("hooks.json");
    fs::write(
        &hooks,
        br#"{"version":1,"hooks":{"beforeShellExecution":[{"command":"user-hook"}]}}"#,
    )
    .unwrap();
    let original = root.join("cursor-agent");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let preview = manager.preview("cursor", &original).unwrap();
    assert_eq!(preview.method, ConnectionMethod::CursorHook);
    manager.connect("cursor", &original).unwrap();
    let connected = fs::read_to_string(&hooks).unwrap();
    assert!(connected.contains("\"version\": 1") || connected.contains("\"version\":1"));
    assert!(connected.contains("sessionStart"));
    assert!(connected.contains("beforeSubmitPrompt"));
    assert!(!connected.contains("preToolUse"));
    assert!(!connected.contains("AskQuestion"));
    assert!(connected.contains("afterAgentResponse"));
    assert!(connected.contains("\"stop\""));
    assert!(connected.contains("sessionEnd"));
    assert!(connected.contains("loop_limit"));
    assert!(connected.contains("user-hook"));
    assert!(connected.contains("cursor-hook"));
    let script =
        fs::read_to_string(root.join("config").join("orbcue").join("cursor-hook.sh")).unwrap();
    assert!(script.contains("exec "), "hook must exec orb: {script}");
    assert!(manager.disconnect("cursor").unwrap());
    let remaining = fs::read_to_string(&hooks).unwrap();
    assert!(remaining.contains("user-hook"));
    assert!(!remaining.contains("cursor-hook"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn reconnecting_codex_replaces_an_old_wrapper() {
    let root = temp_root();
    let home = root.join("home");
    let config = root.join("config");
    let data = root.join("data");
    fs::create_dir_all(&home).unwrap();
    let original = root.join("codex-real");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager =
        ConnectionManager::new(home.clone(), config.clone(), data.clone(), root.join("orb"));
    let wrapper = data.join("orbcue").join("codex");
    fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
    fs::write(&wrapper, "# Agent Activity Dock generated wrapper\n").unwrap();
    let connections = config.join("orbcue").join("connections.json");
    fs::create_dir_all(connections.parent().unwrap()).unwrap();
    fs::write(
        &connections,
        serde_json::json!({
            "version": 1,
            "agents": {
                "codex": {
                    "name": "codex",
                    "original": original,
                    "method": "Wrapper",
                    "wrapper": wrapper,
                    "hook_script": null,
                    "settings_backup": null,
                    "capabilities": ["started"],
                    "limitation": "wrapper",
                    "installed_at": "1"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    fs::write(
        home.join(".bashrc"),
        "export EXISTING=1\n# >>> orbcue PATH >>>\nexport PATH=\"old-wrapper:$PATH\"\n# <<< orbcue PATH <<<\n",
    )
    .unwrap();
    with_shell("/bin/bash", || {
        let record = manager.connect("codex", &original).unwrap();
        assert_eq!(record.method, ConnectionMethod::CodexHook);
        assert!(!wrapper.exists());
        assert!(home.join(".codex").join("hooks.json").is_file());
        let bashrc = fs::read_to_string(home.join(".bashrc")).unwrap();
        assert!(bashrc.contains("EXISTING=1"));
        assert!(!bashrc.contains("orbcue PATH"));
    });
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cursor_preview_and_listing_admit_missing_stop() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let original = root.join("cursor-agent");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let preview = manager.preview("cursor", &original).unwrap();
    assert!(preview
        .notes
        .iter()
        .any(|note| note.contains("不会通知已经结束") && note.contains("工作中")));
    assert!(preview
        .notes
        .iter()
        .any(|note| note.contains("系统通知") && note.contains("设置")));
    let record = manager.connect("cursor", &original).unwrap();
    assert!(record.limitation.contains("不会通知已经结束"));
    assert_eq!(
        manager.records()[0].limitation,
        ConnectionMethod::CursorHook.limitation()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_preview_and_listing_admit_interrupt_and_error_gaps() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let original = root.join("codex");
    executable(&original, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let preview = manager.preview("codex", &original).unwrap();
    assert!(preview
        .notes
        .iter()
        .any(|note| note.contains("Esc") && note.contains("工作中") && note.contains("报错")));
    let record = manager.connect("codex", &original).unwrap();
    assert!(record.limitation.contains("Esc"));
    assert!(record.limitation.contains("报错"));
    assert!(!record.capabilities.iter().any(|item| item == "failed"));
    assert_eq!(
        manager.records()[0].limitation,
        ConnectionMethod::CodexHook.limitation()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_and_codex_preview_mention_native_toasts() {
    let root = temp_root();
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let claude = root.join("claude");
    let codex = root.join("codex");
    executable(&claude, "#!/bin/sh\nexit 0\n");
    executable(&codex, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );
    let _guard = lock_env();
    let previous = std::env::var_os("CLAUDE_CONFIG_DIR");
    std::env::set_var("CLAUDE_CONFIG_DIR", root.join("claude-config"));
    for (name, original) in [("claude", claude.as_path()), ("codex", codex.as_path())] {
        let preview = manager.preview(name, original).unwrap();
        assert!(
            preview
                .notes
                .iter()
                .any(|note| note.contains("系统通知") && note.contains("设置")),
            "{name} preview notes: {:?}",
            preview.notes
        );
    }
    match previous {
        Some(value) => std::env::set_var("CLAUDE_CONFIG_DIR", value),
        None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cursor_and_grok_preview_warn_when_grok_compat_hooks_are_on() {
    use orbcue_connect::GROK_COMPAT_CURSOR_HOOKS_WARNING;

    let root = temp_root();
    let home = root.join("home");
    let grok_home = home.join(".grok");
    fs::create_dir_all(&grok_home).unwrap();
    let cursor = root.join("cursor-agent");
    let grok = root.join("grok");
    executable(&cursor, "#!/bin/sh\nexit 0\n");
    executable(&grok, "#!/bin/sh\nexit 0\n");
    let manager = ConnectionManager::new(
        home,
        root.join("config"),
        root.join("data"),
        root.join("orb"),
    );

    let quiet_cursor = manager.preview("cursor", &cursor).unwrap();
    assert!(quiet_cursor.warnings.is_empty());
    fs::write(grok_home.join("settings.json"), b"{not-json").unwrap();
    let invalid = manager.preview("grok", &grok).unwrap();
    assert!(invalid.warnings.is_empty());

    fs::write(
        grok_home.join("settings.json"),
        br#"{"compat":{"cursor":{"hooks":true}}}"#,
    )
    .unwrap();
    let warned_cursor = manager.preview("cursor", &cursor).unwrap();
    let warned_grok = manager.preview("grok", &grok).unwrap();
    assert_eq!(
        warned_cursor.warnings,
        vec![GROK_COMPAT_CURSOR_HOOKS_WARNING.to_owned()]
    );
    assert_eq!(
        warned_grok.warnings,
        vec![GROK_COMPAT_CURSOR_HOOKS_WARNING.to_owned()]
    );
    fs::remove_dir_all(root).unwrap();
}
