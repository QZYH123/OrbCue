//! Opt-in: a connected Agent's own command runs `orb run`.
//!
//! Interactive shells get an alias (`grok` → `command orb run grok`).
//! Cursor CLI is invoked as `agent` or `cursor-agent`, so both names alias
//! to `orb run cursor`. `command grok` skips that alias and runs the real
//! program. The Agent executable is not replaced. Windows also drops a `.cmd`
//! into the OrbCue directory so cmd and PowerShell resolve the same name;
//! discovery and `orb run` skip that shim.

use crate::{ConnectionManager, ConnectionRecord, FIRST_PARTY_AGENTS};
use orbcue_ipc::default_state_path;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const BLOCK_START: &str = "# >>> orbcue direct-run >>>";
const BLOCK_END: &str = "# <<< orbcue direct-run <<<";
const SHIM_MARKER: &str = "orbcue direct-run";

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectCommand {
    invoke: String,
    agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DirectRunView {
    pub ok: bool,
    pub enabled: bool,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn current() -> bool {
    read_saved(&state_path()).enabled
}

pub fn status() -> DirectRunView {
    let saved = read_saved(&state_path());
    if saved.enabled {
        view_ok(true, invoke_names(&saved.commands))
    } else {
        view_ok(false, Vec::new())
    }
}

pub fn set(enabled: bool) -> Result<DirectRunView, String> {
    if enabled {
        let commands = enable_now()?;
        Ok(view_ok(true, invoke_names(&commands)))
    } else {
        disable_now()?;
        Ok(view_ok(false, Vec::new()))
    }
}

pub fn refresh() -> Result<(), String> {
    if !current() {
        return Ok(());
    }
    enable_now().map(|_| ())
}

fn view_ok(enabled: bool, commands: Vec<String>) -> DirectRunView {
    DirectRunView {
        ok: true,
        enabled,
        commands,
        error: None,
    }
}

pub fn view_err(error: String) -> DirectRunView {
    DirectRunView {
        ok: false,
        enabled: false,
        commands: Vec::new(),
        error: Some(error),
    }
}

pub(crate) fn is_direct_run_shim(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || metadata.len() > 4096 {
        return false;
    }
    fs::read_to_string(path)
        .map(|text| text.contains(SHIM_MARKER))
        .unwrap_or(false)
}

fn commands_from(records: &[ConnectionRecord]) -> Vec<DirectCommand> {
    let mut commands = Vec::new();
    for record in records {
        for command in commands_for(record) {
            if commands.iter().any(|existing: &DirectCommand| {
                existing.invoke.eq_ignore_ascii_case(&command.invoke)
            }) {
                continue;
            }
            commands.push(command);
        }
    }
    commands.sort_by(|left, right| {
        left.invoke
            .to_ascii_lowercase()
            .cmp(&right.invoke.to_ascii_lowercase())
    });
    commands
}

fn commands_for(record: &ConnectionRecord) -> Vec<DirectCommand> {
    if !FIRST_PARTY_AGENTS.contains(&record.name.as_str()) {
        return Vec::new();
    }
    let Some(stem) = record.original.file_stem().and_then(|stem| stem.to_str()) else {
        return Vec::new();
    };
    let mut invokes = Vec::new();
    push_invoke(&mut invokes, stem);
    if record.name == "cursor" {
        // Cursor CLI installs both names as links to the same program.
        // The connection stores one path, so the other name is added here.
        if stem.eq_ignore_ascii_case("cursor-agent") {
            push_invoke(&mut invokes, "agent");
        } else if stem.eq_ignore_ascii_case("agent") {
            push_invoke(&mut invokes, "cursor-agent");
        }
    }
    invokes
        .into_iter()
        .map(|invoke| DirectCommand {
            invoke,
            agent: record.name.clone(),
        })
        .collect()
}

fn push_invoke(invokes: &mut Vec<String>, name: &str) {
    if !invoke_allowed(name)
        || invokes
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
    {
        return;
    }
    invokes.push(name.to_owned());
}

fn invoke_allowed(name: &str) -> bool {
    crate::run_alias::validate(name).is_ok() && !name.eq_ignore_ascii_case("cursor")
}

fn enable_now() -> Result<Vec<DirectCommand>, String> {
    let manager = ConnectionManager::from_environment(
        env::current_exe().unwrap_or_else(|_| PathBuf::from("orb")),
    );
    let commands = commands_from(&manager.records());
    apply_commands(&commands)?;
    write_saved(&state_path(), &commands)?;
    Ok(commands)
}

fn disable_now() -> Result<(), String> {
    apply_commands(&[])?;
    let path = state_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn apply_commands(commands: &[DirectCommand]) -> Result<(), String> {
    sync_shell(&home_dir()?, env::var("SHELL").ok().as_deref(), commands)?;
    if let Some(dir) = windows_cli_dir() {
        let previous = read_saved(&state_path()).commands;
        sync_cmd_shims(&dir, commands, &previous)?;
        if !commands.is_empty() {
            crate::user_path::ensure_dir_on_user_path(&dir)?;
        }
    }
    Ok(())
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "找不到用户目录".to_owned())
}

fn windows_cli_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        crate::user_path::default_windows_cli_dir()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn sync_shell(home: &Path, shell: Option<&str>, commands: &[DirectCommand]) -> Result<(), String> {
    let mut targets = existing_profiles(home);
    if !commands.is_empty() {
        if let Some(preferred) = extra_profile(home, shell) {
            if !targets.iter().any(|path| path == &preferred) {
                targets.push(preferred);
            }
        }
    }
    for path in targets {
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let next = if commands.is_empty() {
            match strip_block(&existing) {
                Some(cleaned) => cleaned,
                None => continue,
            }
        } else {
            upsert_block(&existing, &render_block(profile_kind(&path), commands))
        };
        if next != existing {
            write_profile(&path, &next)?;
        }
    }
    Ok(())
}

fn extra_profile(home: &Path, shell: Option<&str>) -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let _ = (home, shell);
        None
    }
    #[cfg(not(windows))]
    {
        Some(preferred_profile(home, shell))
    }
}

fn shell_rc(home: &Path, shell: &str) -> PathBuf {
    match shell {
        "zsh" => home.join(".zshrc"),
        "fish" => home.join(".config").join("fish").join("config.fish"),
        _ => home.join(".bashrc"),
    }
}

#[cfg(not(windows))]
fn preferred_profile(home: &Path, shell: Option<&str>) -> PathBuf {
    let name = shell
        .and_then(|value| Path::new(value).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");
    shell_rc(
        home,
        match name {
            "zsh" | "fish" => name,
            _ => "bash",
        },
    )
}

fn existing_profiles(home: &Path) -> Vec<PathBuf> {
    ["bash", "zsh", "fish"]
        .into_iter()
        .map(|shell| shell_rc(home, shell))
        .filter(|path| path.is_file())
        .collect()
}

fn profile_kind(path: &Path) -> ProfileKind {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("fish") => ProfileKind::Fish,
        _ => ProfileKind::Posix,
    }
}

enum ProfileKind {
    Posix,
    Fish,
}

fn render_block(kind: ProfileKind, commands: &[DirectCommand]) -> String {
    let mut lines = vec![BLOCK_START.to_owned()];
    lines.extend(commands.iter().map(|command| match kind {
        ProfileKind::Posix => {
            format!(
                "alias {}='command orb run {}'",
                command.invoke, command.agent
            )
        }
        ProfileKind::Fish => format!(
            "function {}\n    command orb run {} $argv\nend",
            command.invoke, command.agent
        ),
    }));
    lines.push(BLOCK_END.to_owned());
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn upsert_block(original: &str, block: &str) -> String {
    let base = strip_block(original).unwrap_or_else(|| original.to_owned());
    let trimmed = base.trim_end_matches(['\n', '\r']);
    if trimmed.is_empty() {
        return block.to_owned();
    }
    format!("{trimmed}\n{block}")
}

fn strip_block(original: &str) -> Option<String> {
    super::strip_path_block(original, BLOCK_START, BLOCK_END)
}

fn write_profile(path: &Path, text: &str) -> Result<(), String> {
    super::atomic_write(path, text.as_bytes(), super::existing_mode(path, 0o644))
        .map_err(|error| format!("无法写入 {}: {error}", path.display()))
}

fn sync_cmd_shims(
    dir: &Path,
    commands: &[DirectCommand],
    previous: &[DirectCommand],
) -> Result<(), String> {
    if commands.is_empty() && !dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(dir).map_err(|error| format!("无法创建 {}: {error}", dir.display()))?;
    for old in previous {
        if commands.iter().any(|command| command.invoke == old.invoke) {
            continue;
        }
        let path = dir.join(format!("{}.cmd", old.invoke));
        if is_direct_run_shim(&path) {
            let _ = fs::remove_file(path);
        }
    }
    for command in commands {
        let path = dir.join(format!("{}.cmd", command.invoke));
        if path.exists() && !is_direct_run_shim(&path) {
            return Err(format!(
                "已有同名命令 {}，没有覆盖",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
        fs::write(&path, cmd_shim(&command.agent))
            .map_err(|error| format!("无法写入 {}: {error}", path.display()))?;
    }
    Ok(())
}

fn cmd_shim(agent: &str) -> String {
    format!("@echo off\r\nrem {SHIM_MARKER}\r\n\"%~dp0orb.exe\" run {agent} %*\r\n")
}

struct Saved {
    enabled: bool,
    commands: Vec<DirectCommand>,
}

fn read_saved(path: &Path) -> Saved {
    let Ok(text) = fs::read_to_string(path) else {
        return Saved {
            enabled: false,
            commands: Vec::new(),
        };
    };
    let mut lines = text.lines();
    let enabled = lines.next().is_some_and(|line| line.trim() == "1");
    let mut commands = Vec::new();
    if enabled {
        for line in lines {
            let Some((invoke, agent)) = line.trim().split_once('=') else {
                continue;
            };
            if !invoke_allowed(invoke) || !FIRST_PARTY_AGENTS.contains(&agent) {
                continue;
            }
            commands.push(DirectCommand {
                invoke: invoke.to_owned(),
                agent: agent.to_owned(),
            });
        }
    }
    Saved { enabled, commands }
}

fn write_saved(path: &Path, commands: &[DirectCommand]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut text = String::from("1\n");
    for command in commands {
        text.push_str(&command.invoke);
        text.push('=');
        text.push_str(&command.agent);
        text.push('\n');
    }
    fs::write(path, text).map_err(|error| error.to_string())
}

fn invoke_names(commands: &[DirectCommand]) -> Vec<String> {
    commands
        .iter()
        .map(|command| command.invoke.clone())
        .collect()
}

fn state_path() -> PathBuf {
    default_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("direct-run")
}

#[cfg(test)]
mod tests {
    use super::{
        commands_from, render_block, sync_cmd_shims, sync_shell, upsert_block, DirectCommand,
        ProfileKind, BLOCK_END, BLOCK_START, SHIM_MARKER,
    };
    use crate::{ConnectionMethod, ConnectionRecord};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_home(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let home = std::env::temp_dir().join(format!("orbcue-direct-{label}-{nonce}"));
        fs::create_dir_all(&home).unwrap();
        home
    }

    fn record(name: &str, original: &str) -> ConnectionRecord {
        let method = match name {
            "claude" => ConnectionMethod::ClaudeHook,
            "codex" => ConnectionMethod::CodexHook,
            "cursor" => ConnectionMethod::CursorHook,
            _ => ConnectionMethod::GrokHook,
        };
        ConnectionRecord {
            name: name.to_owned(),
            original: PathBuf::from(original),
            method,
            wrapper: None,
            hook_script: None,
            limitation: String::new(),
        }
    }

    fn command(invoke: &str, agent: &str) -> DirectCommand {
        DirectCommand {
            invoke: invoke.to_owned(),
            agent: agent.to_owned(),
        }
    }

    #[test]
    fn commands_follow_the_connected_executable_name() {
        let commands = commands_from(&[
            record("grok", "/home/u/.grok/bin/grok"),
            record("cursor", "/home/u/.local/bin/cursor-agent"),
            record("claude", "/home/u/.local/bin/claude.exe"),
            record("codex", "/usr/bin/cursor"),
            record("other", "/home/u/bin/other"),
        ]);
        assert_eq!(
            commands,
            vec![
                command("agent", "cursor"),
                command("claude", "claude"),
                command("cursor-agent", "cursor"),
                command("grok", "grok"),
            ]
        );
        assert_eq!(
            commands_from(&[record("cursor", "/home/u/.local/bin/agent")]),
            vec![
                command("agent", "cursor"),
                command("cursor-agent", "cursor"),
            ]
        );
        assert!(commands_from(&[record("cursor", "/usr/bin/cursor")]).is_empty());
    }

    #[test]
    fn posix_and_fish_blocks_call_orb_run() {
        let commands = [command("grok", "grok"), command("cursor-agent", "cursor")];
        let posix = render_block(ProfileKind::Posix, &commands);
        assert!(posix.contains("alias grok='command orb run grok'"));
        assert!(posix.contains("alias cursor-agent='command orb run cursor'"));
        assert!(posix.starts_with(BLOCK_START));
        assert!(posix.contains(BLOCK_END));
        let fish = render_block(ProfileKind::Fish, &commands);
        assert!(fish.contains("function cursor-agent\n    command orb run cursor $argv\nend"));
    }

    #[test]
    fn shell_block_replaces_itself_and_keeps_other_lines() {
        let home = temp_home("shell");
        fs::write(home.join(".bashrc"), "export KEEP=1\n").unwrap();
        fs::write(home.join(".zshrc"), "export Z=1\n").unwrap();
        let grok = [command("grok", "grok")];
        sync_shell(&home, Some("/bin/bash"), &grok).unwrap();
        let bashrc = fs::read_to_string(home.join(".bashrc")).unwrap();
        assert!(bashrc.contains("export KEEP=1"));
        assert!(bashrc.contains("alias grok='command orb run grok'"));
        assert!(fs::read_to_string(home.join(".zshrc"))
            .unwrap()
            .contains("alias grok="));
        assert!(!home
            .join(".config")
            .join("fish")
            .join("config.fish")
            .exists());

        sync_shell(&home, Some("/bin/zsh"), &[command("claude", "claude")]).unwrap();
        let updated = fs::read_to_string(home.join(".bashrc")).unwrap();
        assert_eq!(updated.matches("alias ").count(), 1);
        assert!(updated.contains("alias claude='command orb run claude'"));
        assert!(updated.contains("export KEEP=1"));

        sync_shell(&home, Some("/bin/bash"), &[]).unwrap();
        assert_eq!(
            fs::read_to_string(home.join(".bashrc")).unwrap(),
            "export KEEP=1\n"
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn missing_shell_rc_is_created_for_the_current_shell() {
        let home = temp_home("create");
        sync_shell(&home, Some("/usr/bin/zsh"), &[command("grok", "grok")]).unwrap();
        assert!(home.join(".zshrc").is_file());
        assert!(!home.join(".bashrc").exists());
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn upsert_is_stable() {
        let block = render_block(ProfileKind::Posix, &[command("grok", "grok")]);
        let once = upsert_block("export KEEP=1\n", &block);
        assert_eq!(upsert_block(&once, &block), once);
        assert!(once.contains("export KEEP=1"));
        assert_eq!(once.matches("alias grok=").count(), 1);
    }

    #[test]
    fn cmd_shim_is_replaced_and_removed_without_clobbering() {
        let dir = temp_home("cmd");
        fs::write(dir.join("grok.cmd"), "echo foreign\n").unwrap();
        let error = sync_cmd_shims(&dir, &[command("grok", "grok")], &[]).unwrap_err();
        assert!(error.contains("同名"));
        assert_eq!(
            fs::read_to_string(dir.join("grok.cmd")).unwrap(),
            "echo foreign\n"
        );

        sync_cmd_shims(&dir, &[command("claude", "claude")], &[]).unwrap();
        let claude = fs::read_to_string(dir.join("claude.cmd")).unwrap();
        assert!(claude.contains(SHIM_MARKER));
        assert!(claude.contains("run claude"));

        sync_cmd_shims(
            &dir,
            &[command("codex", "codex")],
            &[command("claude", "claude")],
        )
        .unwrap();
        assert!(!dir.join("claude.cmd").exists());
        assert!(dir.join("codex.cmd").is_file());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn bash_command_skips_the_alias() {
        let home = temp_home("bash");
        let bin = home.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(
            bin.join("orb"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$HOME/orb-args\"\n",
        )
        .unwrap();
        fs::write(
            bin.join("grok"),
            "#!/bin/sh\nprintf '%s\\n' real >> \"$HOME/grok-out\"\n",
        )
        .unwrap();
        set_executable(&bin.join("orb"));
        set_executable(&bin.join("grok"));
        sync_shell(&home, Some("/bin/bash"), &[command("grok", "grok")]).unwrap();
        let path =
            std::env::join_paths([bin, PathBuf::from("/usr/bin"), PathBuf::from("/bin")]).unwrap();
        let status = std::process::Command::new("bash")
            .args([
                "--noprofile",
                "--norc",
                "-c",
                "shopt -s expand_aliases; source \"$HOME/.bashrc\"; eval 'grok --flag value'; command grok",
            ])
            .env("HOME", &home)
            .env("PATH", path)
            .status()
            .expect("bash");
        assert!(status.success(), "bash alias check failed: {status}");
        assert_eq!(
            fs::read_to_string(home.join("orb-args")).unwrap(),
            "run grok --flag value\n"
        );
        assert_eq!(fs::read_to_string(home.join("grok-out")).unwrap(), "real\n");
        fs::remove_dir_all(home).unwrap();
    }

    #[cfg(unix)]
    fn set_executable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}
