//! Revocable, zero-reinstall Agent connections.
//!
//! Discovers Claude, Grok, Codex, Cursor, OpenCode, and Pi already on PATH or in
//! folders the user adds, then writes native hooks. It never replaces an Agent
//! executable.
//! Other tools emit events with `orb start` / `orb complete`; leftover wrapper
//! records can still be disconnected.

mod direct_run;
mod discover;
mod grok_compat;
mod replace_tab;
mod run_alias;
mod ui_lang;
mod user_path;
mod wsl_cli;

pub use direct_run::{
    current as direct_run_enabled, refresh as refresh_direct_run, set as set_direct_run,
    status as direct_run_status, view_err as direct_run_err, DirectRunView,
};
pub use discover::looks_like_cursor_cli_path;
pub use grok_compat::GROK_COMPAT_CURSOR_HOOKS_WARNING;
pub use replace_tab::{
    current as replace_tab_on_run, set as set_replace_tab_on_run, view_err as replace_tab_err,
    view_ok as replace_tab_ok, ReplaceTabView,
};
pub use run_alias::{
    current as current_run_alias, preferred as preferred_run_alias, set as set_run_alias,
    validate as validate_run_alias, view_err as run_alias_err, view_ok as run_alias_ok,
    wsl_dock_cli_is_missing, wsl_runtime_is_absent, wsl_side_is_absent, AliasView,
};
pub use ui_lang::{
    pref as ui_lang_pref, saved_lang as saved_ui_lang, set as set_ui_lang, view_err as ui_lang_err,
    view_ok as ui_lang_ok, UiLangView,
};
pub use user_path::install_windows_cli;
pub use wsl_cli::{
    choose_packaged_linux_dock, decode_console_output, dock_version_matches,
    is_infrastructure_wsl_distro, packaged_linux_dock_candidates, packaged_linux_dock_is_usable,
    parse_wsl_distro_list, wsl_dock_install_shell,
};

use grok_compat::connection_warnings;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

static CONNECTION_IO: Mutex<()> = Mutex::new(());

fn lock_connection_io() -> MutexGuard<'static, ()> {
    CONNECTION_IO
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

const PATH_START: &str = "# >>> orbcue PATH >>>";
const PATH_END: &str = "# <<< orbcue PATH <<<";
const LEGACY_PATH_START: &str = "# >>> agent-activity-dock PATH >>>";
const LEGACY_PATH_END: &str = "# <<< agent-activity-dock PATH <<<";

/// Agents the connections page can attach through native hooks.
pub(crate) const FIRST_PARTY_AGENTS: &[&str] =
    &["claude", "codex", "cursor", "grok", "opencode", "pi"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionMethod {
    /// Leftover records from before hook-only connect. Disconnect still clears them; new connects never create these.
    Wrapper,
    ClaudeHook,
    GrokHook,
    CodexHook,
    CursorHook,
    OpenCodeHook,
    PiHook,
}

impl ConnectionMethod {
    pub fn limitation(self) -> &'static str {
        match self {
            Self::Wrapper => orbcue_core::pick(
                "遗留包装连接，看不到「正在等你输入」。可断开后改用 `orb start` / `orb complete`",
                "Leftover wrapper connection. It can't see “waiting for input”. Disconnect it and use `orb start` / `orb complete`.",
            ),
            Self::ClaudeHook => "",
            Self::GrokHook => "",
            Self::CodexHook => orbcue_core::pick(
                "用 Esc 或 Ctrl+C 打断时不会离开「工作中」，对话报错也不会显示为失败；可用「清除」，或退出 Codex 后任务会消失",
                "Esc or Ctrl+C does not leave Working, and a chat error is not shown as a failure. Use Clear, or the row disappears when you quit Codex.",
            ),
            Self::CursorHook => orbcue_core::pick(
                "打印模式（-p）没有回合结束通知，条目要等进程退出后才消失",
                "Print mode (-p) has no end-of-turn signal. The row stays until the process exits.",
            ),
            Self::OpenCodeHook => orbcue_core::pick(
                "点回去会回到 OpenCode 所在的终端，不会定位到里面的某一段对话",
                "Jump back focuses the terminal running OpenCode, not one conversation inside it.",
            ),
            Self::PiHook => orbcue_core::pick(
                "点回去回到 Pi 所在的终端。自带工具不会标成等待授权，只有扩展弹出的确认框才会",
                "Jump back focuses the terminal running Pi. Built-in tools are not marked as waiting for approval. Only an extension confirm dialog is.",
            ),
        }
    }
}

fn connection_record(
    name: &str,
    original: &Path,
    method: ConnectionMethod,
    wrapper: Option<PathBuf>,
    hook_script: Option<PathBuf>,
) -> ConnectionRecord {
    ConnectionRecord {
        name: name.to_owned(),
        original: original.to_owned(),
        method,
        wrapper,
        hook_script,
        limitation: method.limitation().to_owned(),
    }
}

fn native_notify_note() -> &'static str {
    orbcue_core::pick(
        "该 Agent 自己也可能弹系统通知；Dock 的通知可在设置里关掉",
        "This agent may also show its own notifications. You can turn OrbCue's off in Settings.",
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionRecord {
    pub name: String,
    pub original: PathBuf,
    pub method: ConnectionMethod,
    pub wrapper: Option<PathBuf>,
    pub hook_script: Option<PathBuf>,
    pub limitation: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentOrigin {
    #[default]
    Wsl,
    Windows,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredAgent {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub origin: AgentOrigin,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewAction {
    Create,
    Modify,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviewFile {
    pub path: PathBuf,
    pub action: PreviewAction,
    pub entries: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionPreview {
    pub name: String,
    pub original: PathBuf,
    pub method: ConnectionMethod,
    pub files: Vec<PreviewFile>,
    pub will_not: Vec<String>,
    pub notes: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConnectionFile {
    version: u16,
    agents: std::collections::BTreeMap<String, ConnectionRecord>,
    #[serde(default)]
    extra_dirs: Vec<PathBuf>,
    /// Shell profiles were already scanned for leftover PATH blocks.
    /// New connects no longer write those blocks, so the scan runs once.
    #[serde(default, skip_serializing_if = "is_false")]
    legacy_path_cleaned: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub struct ConnectionManager {
    home: PathBuf,
    grok_home: PathBuf,
    codex_home: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
    config_path: PathBuf,
    dock_binary: PathBuf,
}

impl ConnectionManager {
    pub fn from_environment(dock_binary: impl Into<PathBuf>) -> Self {
        let home = env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let config_home = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("APPDATA").map(PathBuf::from))
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    home.join("AppData").join("Roaming")
                } else {
                    home.join(".config")
                }
            });
        let data_home = env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("LOCALAPPDATA").map(PathBuf::from))
            .unwrap_or_else(|| {
                if cfg!(windows) {
                    home.join("AppData").join("Local")
                } else {
                    home.join(".local").join("share")
                }
            });
        let grok_home = env::var_os("GROK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".grok"));
        let codex_home = env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"));
        let mut manager = Self::new(home, config_home, data_home, dock_binary.into());
        manager.grok_home = grok_home;
        manager.codex_home = codex_home;
        manager
    }

    pub fn new(
        home: PathBuf,
        config_home: PathBuf,
        data_home: PathBuf,
        dock_binary: PathBuf,
    ) -> Self {
        let config_dir = config_home.join("orbcue");
        let data_dir = data_home.join("orbcue");
        let grok_home = home.join(".grok");
        let codex_home = home.join(".codex");
        Self {
            home,
            grok_home,
            codex_home,
            config_path: config_dir.join("connections.json"),
            config_dir,
            data_dir,
            dock_binary,
        }
    }

    pub fn discover(&self) -> Vec<DiscoveredAgent> {
        self.discover_from_path(&discover::discovery_path())
    }

    pub fn discover_from_path(&self, path: &OsStr) -> Vec<DiscoveredAgent> {
        discover::discover_agents_with_extras(path, &self.scan_dirs(), Some(&self.data_dir))
    }

    pub fn add_scan_dir(&self, dir: &Path) -> Result<Vec<DiscoveredAgent>, String> {
        if !dir.is_dir() {
            return Err(orbcue_core::t!("请选择一个文件夹", "Pick a folder."));
        }
        let found = discover::agents_in_dir(dir);
        if found.is_empty() {
            return Err(orbcue_core::t!(
                "这个文件夹里没有支持的工具（Claude、Grok、Codex、Cursor、OpenCode 或 Pi）",
                "This folder has no supported tool (Claude, Grok, Codex, Cursor, OpenCode, or Pi)."
            ));
        }
        let mut file = self.load();
        if !file.extra_dirs.iter().any(|existing| existing == dir) {
            file.extra_dirs.push(dir.to_path_buf());
            self.save(&file)?;
        }
        Ok(found)
    }

    fn scan_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![
            self.home.join(".local").join("bin"),
            self.grok_home.join("bin"),
            self.home.join("AppData").join("Local").join("cursor-agent"),
            self.home.join("AppData").join("Roaming").join("npm"),
            self.home.join(".opencode").join("bin"),
        ];
        dirs.extend(self.load().extra_dirs);
        dirs
    }

    pub fn records(&self) -> Vec<ConnectionRecord> {
        let _io = lock_connection_io();
        self.repair_connected_artifacts();
        self.load()
            .agents
            .into_values()
            .map(|mut record| {
                record.limitation = record.method.limitation().to_owned();
                record
            })
            .collect()
    }

    pub fn preview(&self, name: &str, original: &Path) -> Result<ConnectionPreview, String> {
        let method = validate_connection_request(name, original)?;
        Ok(ConnectionPreview {
            name: name.to_owned(),
            original: original.to_owned(),
            method,
            files: self.preview_files(name, method),
            will_not: vec![
                orbcue_core::t!(
                    "不替换 Agent 本体（{shown}）",
                    "Won't replace the agent program ({shown})",
                    shown = original.display()
                ),
                orbcue_core::pick(
                    "不修改、不删除用户其他 Hook",
                    "Won't change or delete your other hooks",
                )
                .to_owned(),
                orbcue_core::pick(
                    "不读取 transcript / prompt / 命令 / 代码",
                    "Won't read transcripts, prompts, commands, or code",
                )
                .to_owned(),
            ],
            notes: preview_notes(method),
            warnings: self.connect_warnings(name),
        })
    }

    pub fn connect_warnings(&self, name: &str) -> Vec<String> {
        connection_warnings(name, &self.grok_home)
    }

    pub fn connect(&self, name: &str, original: &Path) -> Result<ConnectionRecord, String> {
        let _io = lock_connection_io();
        let method = validate_connection_request(name, original)?;
        let mut file = self.load();
        if let Some(existing) = file.agents.get(name) {
            if existing.original == original && existing.method == method {
                self.reinstall_artifacts(method)?;
                let mut record = existing.clone();
                if let Some(hook) = self.current_hook_script(method) {
                    record.hook_script = Some(hook);
                }
                if record != *existing {
                    file.agents.insert(name.to_owned(), record.clone());
                    self.save(&file)?;
                }
                self.drop_wrapper_path_if_unused(&mut file)?;
                return Ok(record);
            }
        }
        let Some(agent) = hook_agent(method) else {
            return Err(unsupported_connect_name(name));
        };
        let hook = self.install_hook(agent)?;
        let record = connection_record(name, original, method, None, Some(hook));
        if let Some(existing) = file.agents.get(name) {
            // The new artifact is installed before this cleanup. Different
            // methods use different paths, so a cleanup failure must not
            // invalidate the newly working connection.
            if existing.method != record.method {
                if let Err(error) = self.remove_artifacts(existing) {
                    eprintln!("OrbCue could not remove old connection: {error}");
                }
            }
        }
        file.agents.insert(name.to_owned(), record.clone());
        self.save(&file)?;
        self.drop_wrapper_path_if_unused(&mut file)?;
        Ok(record)
    }

    pub fn disconnect(&self, name: &str) -> Result<bool, String> {
        let _io = lock_connection_io();
        let mut file = self.load();
        let Some(record) = file.agents.remove(name) else {
            return Ok(false);
        };
        self.remove_artifacts(&record)?;
        self.save(&file)?;
        self.drop_wrapper_path_if_unused(&mut file)?;
        Ok(true)
    }

    fn repair_connected_artifacts(&self) {
        let mut file = self.load();
        let mut dirty = false;
        let names: Vec<String> = file.agents.keys().cloned().collect();
        for name in names {
            let Some(record) = file.agents.get(&name).cloned() else {
                continue;
            };
            let Some(hook) = self.current_hook_script(record.method) else {
                continue;
            };
            if self.hook_install_is_current(record.method, &hook) {
                if record.hook_script.as_ref() != Some(&hook) {
                    if let Some(updated) = file.agents.get_mut(&name) {
                        updated.hook_script = Some(hook);
                        dirty = true;
                    }
                }
                continue;
            }
            if let Err(error) = self.reinstall_artifacts(record.method) {
                eprintln!(
                    "OrbCue could not repair {} connection: {error}",
                    record.name
                );
                continue;
            }
            if let Some(updated) = file.agents.get_mut(&name) {
                if updated.hook_script.as_ref() != Some(&hook) {
                    updated.hook_script = Some(hook);
                    dirty = true;
                }
            }
        }
        if dirty {
            if let Err(error) = self.save(&file) {
                eprintln!("OrbCue could not save repaired connections: {error}");
            }
        }
    }

    fn preview_files(&self, name: &str, method: ConnectionMethod) -> Vec<PreviewFile> {
        let Some(agent) = hook_agent(method) else {
            unreachable!("new connections are hook-only: {name}");
        };
        if let Some(files) = self.plugin_files(agent.layout) {
            let mut preview: Vec<_> = files
                .into_iter()
                .map(|file| PreviewFile {
                    action: preview_action(&file.path),
                    path: file.path,
                    entries: file.entries,
                })
                .collect();
            preview.push(self.preview_connections_file());
            return preview;
        }
        let hook = hook_path(&self.config_dir, agent.name);
        let config = self.hooks_config_path(agent);
        let events = hook_spec_labels(agent.specs());
        let mut files = vec![
            PreviewFile {
                path: hook.clone(),
                action: preview_action(&hook),
                entries: events.clone(),
            },
            PreviewFile {
                path: config.clone(),
                action: preview_action(&config),
                entries: events,
            },
        ];
        if let Some(backup_name) = agent.backup_name() {
            if config.is_file() {
                files.push(PreviewFile {
                    path: config.with_file_name(backup_name),
                    action: PreviewAction::Create,
                    entries: vec![orbcue_core::pick(
                        "仅在备份不存在时创建",
                        "Created only if no backup exists yet",
                    )
                    .to_owned()],
                });
            }
        }
        files.push(self.preview_connections_file());
        files
    }

    fn preview_connections_file(&self) -> PreviewFile {
        PreviewFile {
            path: self.config_path.clone(),
            action: preview_action(&self.config_path),
            entries: vec!["connection record".to_owned()],
        }
    }

    fn write_hook_script(&self, name: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.config_dir).map_err(|error| error.to_string())?;
        set_mode(&self.config_dir, 0o700)?;
        let hook = hook_path(&self.config_dir, name);
        atomic_write(
            &hook,
            hook_script(&self.dock_binary, name).as_bytes(),
            0o700,
        )?;
        Ok(hook)
    }

    fn reinstall_artifacts(&self, method: ConnectionMethod) -> Result<(), String> {
        match hook_agent(method) {
            Some(agent) => self.install_hook(agent).map(|_| ()),
            None => Ok(()),
        }
    }

    fn hook_install_is_current(&self, method: ConnectionMethod, hook: &Path) -> bool {
        if let Some(agent) = hook_agent(method) {
            if let Some(files) = self.plugin_files(agent.layout) {
                return files
                    .iter()
                    .all(|file| generated_file_matches(&file.path, &file.contents));
            }
        }
        if !hook.is_file() || !hook_script_forwards_args(hook) {
            return false;
        }
        let Some(agent) = hook_agent(method) else {
            return true;
        };
        if matches!(agent.layout, HookLayout::Cursor) {
            return cursor_hooks_registered(&self.cursor_hooks_file(), hook);
        }
        let path = self.hooks_config_path(agent);
        let Ok(bytes) = fs::read(&path) else {
            return false;
        };
        let Ok(document) = serde_json::from_slice::<Value>(&bytes) else {
            return false;
        };
        let Some(hooks) = document.get("hooks").and_then(Value::as_object) else {
            return false;
        };
        agent.specs().iter().all(|spec| {
            hooks
                .get(spec.event)
                .and_then(Value::as_array)
                .is_some_and(|entries| {
                    entries
                        .iter()
                        .any(|entry| dock_handler_matches(entry, hook, spec))
                })
        })
    }

    fn drop_wrapper_path_if_unused(&self, file: &mut ConnectionFile) -> Result<(), String> {
        if file.agents.values().any(|item| item.wrapper.is_some()) {
            if file.legacy_path_cleaned {
                file.legacy_path_cleaned = false;
                self.save(file)?;
            }
            return Ok(());
        }
        if file.legacy_path_cleaned {
            return Ok(());
        }
        self.remove_path_snippet()?;
        self.remove_empty_data_dir();
        file.legacy_path_cleaned = true;
        self.save(file)
    }

    fn hooks_config_path(&self, agent: &HookAgent) -> PathBuf {
        match agent.name {
            "claude" => claude_settings_path(),
            "grok" => self.grok_hooks_file(),
            "codex" => self.codex_hooks_file(),
            "cursor" => self.cursor_hooks_file(),
            _ => unreachable!("unknown hook agent {}", agent.name),
        }
    }

    fn opencode_plugin_dir(&self) -> PathBuf {
        opencode_config_dir(&self.home)
            .join("plugins")
            .join("orbcue")
    }

    fn opencode_plugin_path(&self) -> PathBuf {
        self.opencode_plugin_dir().join("index.js")
    }

    fn opencode_tui_plugin_path(&self) -> PathBuf {
        self.opencode_plugin_dir().join("tui.js")
    }

    fn opencode_legacy_plugin_path(&self) -> PathBuf {
        opencode_config_dir(&self.home)
            .join("plugins")
            .join("orbcue.js")
    }

    fn pi_extension_path(&self) -> PathBuf {
        pi_agent_dir(&self.home)
            .join("extensions")
            .join("orbcue.js")
    }

    fn install_hook(&self, agent: &HookAgent) -> Result<PathBuf, String> {
        if let Some(files) = self.plugin_files(agent.layout) {
            return self.write_plugin_files(agent.layout, &files);
        }
        let dest = hook_path(&self.config_dir, agent.name);
        let existed = dest.is_file();
        let hook = self.write_hook_script(agent.name)?;
        let result = match agent.layout {
            HookLayout::Nested { backup } => install_nested_hooks_at(
                &self.hooks_config_path(agent),
                &hook,
                agent.specs(),
                backup,
            ),
            HookLayout::GrokFile => install_grok_hooks(&self.grok_hooks_file(), &hook),
            HookLayout::Cursor => install_cursor_hooks_at(&self.cursor_hooks_file(), &hook),
            HookLayout::OpenCode | HookLayout::Pi => unreachable!(),
        };
        match result {
            Ok(()) => Ok(hook),
            Err(error) => {
                if !existed || !matches!(agent.layout, HookLayout::Cursor) {
                    let _ = fs::remove_file(&hook);
                }
                Err(format!("cannot update {} hooks: {error}", agent.label()))
            }
        }
    }

    fn grok_hooks_file(&self) -> PathBuf {
        self.grok_home.join("hooks").join("orbcue.json")
    }

    fn codex_hooks_file(&self) -> PathBuf {
        self.codex_home.join("hooks.json")
    }

    fn cursor_hooks_file(&self) -> PathBuf {
        self.home.join(".cursor").join("hooks.json")
    }

    fn remove_artifacts(&self, record: &ConnectionRecord) -> Result<(), String> {
        if let Some(wrapper) = &record.wrapper {
            if wrapper.exists() {
                fs::remove_file(wrapper).map_err(|error| error.to_string())?;
            }
        }
        if let Some(hook) = &record.hook_script {
            if let Some(agent) = hook_agent(record.method) {
                match agent.layout {
                    HookLayout::GrokFile => {
                        let grok_hooks = self.grok_hooks_file();
                        if grok_hooks.exists() {
                            fs::remove_file(&grok_hooks).map_err(|error| error.to_string())?;
                        }
                    }
                    HookLayout::Nested { .. } => {
                        uninstall_nested_hooks_at(&self.hooks_config_path(agent), hook)?
                    }
                    HookLayout::Cursor => {
                        uninstall_cursor_hooks_at(&self.cursor_hooks_file(), hook)?
                    }
                    HookLayout::OpenCode => self.remove_opencode_plugin(hook)?,
                    HookLayout::Pi => self.remove_pi_extension(hook)?,
                }
                if hook.exists() && !matches!(agent.layout, HookLayout::OpenCode | HookLayout::Pi) {
                    fs::remove_file(hook).map_err(|error| error.to_string())?;
                }
            } else if hook.exists() {
                fs::remove_file(hook).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn remove_path_snippet(&self) -> Result<(), String> {
        for profile in self.profile_candidates() {
            let Ok(old) = fs::read_to_string(&profile) else {
                continue;
            };
            let Some(cleaned) = strip_path_block(&old, PATH_START, PATH_END)
                .or_else(|| strip_path_block(&old, LEGACY_PATH_START, LEGACY_PATH_END))
            else {
                continue;
            };
            atomic_write(&profile, cleaned.as_bytes(), existing_mode(&profile, 0o600))?;
        }
        Ok(())
    }

    fn remove_empty_data_dir(&self) {
        let _ = fs::remove_dir(&self.data_dir);
    }

    fn profile_candidates(&self) -> Vec<PathBuf> {
        #[cfg(windows)]
        {
            vec![
                self.home
                    .join("Documents")
                    .join("PowerShell")
                    .join("Microsoft.PowerShell_profile.ps1"),
                self.home
                    .join("Documents")
                    .join("WindowsPowerShell")
                    .join("Microsoft.PowerShell_profile.ps1"),
                self.home
                    .join(".config")
                    .join("powershell")
                    .join("Microsoft.PowerShell_profile.ps1"),
            ]
        }
        #[cfg(not(windows))]
        {
            vec![
                self.home.join(".zshrc"),
                self.home.join(".zprofile"),
                self.home.join(".zshenv"),
                self.home.join(".bashrc"),
                self.home.join(".bash_profile"),
                self.home.join(".profile"),
                self.linux_powershell_profile(),
                self.home.join(".config").join("fish").join("config.fish"),
            ]
        }
    }

    #[cfg(not(windows))]
    fn linux_powershell_profile(&self) -> PathBuf {
        self.home
            .join(".config")
            .join("powershell")
            .join("Microsoft.PowerShell_profile.ps1")
    }

    fn load(&self) -> ConnectionFile {
        fs::read(&self.config_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| ConnectionFile {
                version: 1,
                agents: Default::default(),
                extra_dirs: Vec::new(),
                legacy_path_cleaned: false,
            })
    }

    fn save(&self, file: &ConnectionFile) -> Result<(), String> {
        fs::create_dir_all(&self.config_dir).map_err(|error| error.to_string())?;
        set_mode(&self.config_dir, 0o700)?;
        let bytes = serde_json::to_vec_pretty(file).map_err(|error| error.to_string())?;
        atomic_write(&self.config_path, &bytes, 0o600)
    }
}

pub(crate) fn candidate_names(name: &str) -> Vec<String> {
    let mut names = vec![name.to_owned()];
    let extensions = env::var_os("PATHEXT")
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                ".COM;.EXE;.BAT;.CMD".to_owned()
            } else {
                ".exe;.cmd;.bat;.ps1".to_owned()
            }
        });
    for extension in extensions
        .split(';')
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
    {
        let extension = if extension.starts_with('.') {
            extension.to_owned()
        } else {
            format!(".{extension}")
        };
        push_unique_ignore_case(&mut names, format!("{name}{extension}"));
        push_unique_ignore_case(
            &mut names,
            format!("{name}{}", extension.to_ascii_lowercase()),
        );
    }
    names
}

fn push_unique_ignore_case(names: &mut Vec<String>, candidate: String) {
    if !names
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&candidate))
    {
        names.push(candidate);
    }
}

#[cfg(test)]
fn install_claude_settings_at(settings_path: &Path, hook: &Path) -> Result<PathBuf, String> {
    install_nested_hooks_at(
        settings_path,
        hook,
        claude_hook_specs(),
        "settings.json.orbcue.bak",
    )?;
    Ok(settings_path.with_file_name("settings.json.orbcue.bak"))
}

#[cfg(test)]
fn uninstall_claude_settings_at(settings_path: &Path, hook: &Path) -> Result<(), String> {
    uninstall_nested_hooks_at(settings_path, hook)
}

fn read_json_document(path: &Path, missing: Value) -> Result<(Option<Vec<u8>>, Value), String> {
    let existing = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let document = match existing.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| format!("{} is not valid JSON: {error}", file_label(path)))?,
        None => missing,
    };
    Ok((existing, document))
}

fn write_json_with_backup(
    path: &Path,
    existing: Option<Vec<u8>>,
    backup_name: &str,
    document: &Value,
) -> Result<(), String> {
    if let Some(bytes) = existing {
        let backup_path = path.with_file_name(backup_name);
        if !backup_path.exists() {
            atomic_write(&backup_path, &bytes, existing_mode(path, 0o600))?;
        }
    }
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    atomic_write(path, &bytes, existing_mode(path, 0o600))
}

fn upsert_hook_events(
    hooks: &mut serde_json::Map<String, Value>,
    specs: &[HookSpec],
    hook: &Path,
    mut make_entry: impl FnMut(&HookSpec) -> Value,
) -> Result<(), String> {
    for spec in specs {
        let event = spec.event.to_owned();
        let entries = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let entries = entries
            .as_array_mut()
            .ok_or_else(|| format!("hook {event} must be an array"))?;
        entries.retain(|entry| !is_dock_managed_hook(entry, hook));
        entries.push(make_entry(spec));
    }
    strip_unwanted_dock_hooks(hooks, &hook_spec_names(specs), hook);
    Ok(())
}

fn hooks_object_mut<'a>(
    document: &'a mut Value,
    label: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, String> {
    let hooks = document
        .as_object_mut()
        .ok_or_else(|| format!("{label} must be a JSON object"))?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    hooks
        .as_object_mut()
        .ok_or_else(|| format!("{label} hooks must be an object"))
}

fn install_hooks_json(
    path: &Path,
    missing: Value,
    backup_name: &str,
    label: &str,
    prepare: impl FnOnce(&mut Value) -> Result<(), String>,
    specs: &[HookSpec],
    hook: &Path,
    make_entry: impl FnMut(&HookSpec) -> Value,
) -> Result<(), String> {
    let (existing, mut document) = read_json_document(path, missing)?;
    prepare(&mut document)?;
    {
        let hooks = hooks_object_mut(&mut document, label)?;
        upsert_hook_events(hooks, specs, hook, make_entry)?;
    }
    write_json_with_backup(path, existing, backup_name, &document)
}

fn install_nested_hooks_at(
    settings_path: &Path,
    hook: &Path,
    specs: &[HookSpec],
    backup_name: &str,
) -> Result<(), String> {
    let label = file_label(settings_path);
    install_hooks_json(
        settings_path,
        json!({}),
        backup_name,
        &label,
        |_| Ok(()),
        specs,
        hook,
        |spec| dock_hook_group(hook, spec),
    )
}

fn claude_settings_path() -> PathBuf {
    let config_dir = env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .or_else(|| env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
        });
    config_dir.join("settings.json")
}

fn uninstall_nested_hooks_at(settings_path: &Path, hook: &Path) -> Result<(), String> {
    uninstall_hooks_file(settings_path, hook, false)
}

fn install_cursor_hooks_at(hooks_path: &Path, hook: &Path) -> Result<(), String> {
    install_hooks_json(
        hooks_path,
        json!({"version": 1, "hooks": {}}),
        "hooks.json.orbcue.bak",
        "Cursor",
        |document| {
            let object = document
                .as_object_mut()
                .ok_or_else(|| "Cursor hooks.json must be a JSON object".to_owned())?;
            if !object.contains_key("version") {
                object.insert("version".to_owned(), json!(1));
            }
            Ok(())
        },
        cursor_hook_specs(),
        hook,
        |spec| cursor_hook_entry(hook, spec),
    )
}

fn cursor_hooks_registered(hooks_path: &Path, hook: &Path) -> bool {
    let Ok(bytes) = fs::read(hooks_path) else {
        return false;
    };
    let Ok(document) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    let Some(hooks) = document.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    let command = hook.to_string_lossy();
    cursor_hook_specs().iter().all(|spec| {
        hooks
            .get(spec.event)
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries.iter().any(|entry| {
                    entry.get("command").and_then(Value::as_str) == Some(command.as_ref())
                })
            })
    })
}

fn uninstall_cursor_hooks_at(hooks_path: &Path, hook: &Path) -> Result<(), String> {
    uninstall_hooks_file(hooks_path, hook, true)
}

fn uninstall_hooks_file(path: &Path, hook: &Path, drop_empty: bool) -> Result<(), String> {
    let Ok(bytes) = fs::read(path) else {
        return Ok(());
    };
    let Ok(mut settings) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(());
    };
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for entries in hooks.values_mut() {
            if let Some(entries) = entries.as_array_mut() {
                entries.retain(|entry| !is_dock_managed_hook(entry, hook));
            }
        }
        if drop_empty {
            retain_nonempty_hook_arrays(hooks);
        }
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    atomic_write(path, &bytes, existing_mode(path, 0o600))
}

fn retain_nonempty_hook_arrays(hooks: &mut serde_json::Map<String, Value>) {
    hooks.retain(|_, value| match value.as_array() {
        Some(entries) => !entries.is_empty(),
        None => true,
    });
}

pub(crate) fn could_not_write(path: &Path, error: impl std::fmt::Display) -> String {
    orbcue_core::t!(
        "无法写入 {path}: {error}",
        "Couldn't write {path}: {error}",
        path = path.display()
    )
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    set_mode(&temp, mode)?;
    fs::rename(temp, path).map_err(|error| error.to_string())
}

pub(crate) fn existing_mode(path: &Path, fallback: u32) -> u32 {
    #[cfg(unix)]
    {
        return fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(fallback);
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        fallback
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn preview_action(path: &Path) -> PreviewAction {
    if path.is_file() {
        PreviewAction::Modify
    } else {
        PreviewAction::Create
    }
}

fn preview_notes(method: ConnectionMethod) -> Vec<String> {
    let settings_backup = orbcue_core::pick(
        "首次修改前备份 settings.json",
        "Backs up settings.json before the first edit",
    );
    let hooks_backup = orbcue_core::pick(
        "首次修改前备份 hooks.json",
        "Backs up hooks.json before the first edit",
    );
    let trust = orbcue_core::pick(
        "Codex 可能要求在 /hooks 里信任新命令",
        "Codex may ask you to trust the new command under /hooks",
    );
    match method {
        ConnectionMethod::ClaudeHook => {
            vec![settings_backup.to_owned(), native_notify_note().to_owned()]
        }
        ConnectionMethod::CodexHook => vec![
            hooks_backup.to_owned(),
            ConnectionMethod::CodexHook.limitation().to_owned(),
            trust.to_owned(),
            native_notify_note().to_owned(),
        ],
        ConnectionMethod::CursorHook => vec![
            hooks_backup.to_owned(),
            ConnectionMethod::CursorHook.limitation().to_owned(),
            native_notify_note().to_owned(),
        ],
        ConnectionMethod::Wrapper | ConnectionMethod::GrokHook => Vec::new(),
        ConnectionMethod::OpenCodeHook => vec![
            orbcue_core::pick(
                "已经开着的终端客户端会热重载终端插件。服务进程要重新启动后才加载服务端插件",
                "An open terminal client hot-reloads the terminal plugin. The server process loads the server plugin after a restart",
            )
            .to_owned(),
            native_notify_note().to_owned(),
        ],
        ConnectionMethod::PiHook => vec![
            orbcue_core::pick(
                "已经打开的 Pi 要重新启动后才会加载",
                "Pi that is already running loads this after a restart",
            )
            .to_owned(),
            ConnectionMethod::PiHook.limitation().to_owned(),
            native_notify_note().to_owned(),
        ],
    }
}

fn strip_unwanted_dock_hooks(
    hooks: &mut serde_json::Map<String, Value>,
    wanted: &[String],
    hook: &Path,
) {
    for (name, entries) in hooks.iter_mut() {
        if wanted.iter().any(|event| event == name) {
            continue;
        }
        if let Some(entries) = entries.as_array_mut() {
            entries.retain(|entry| !is_dock_managed_hook(entry, hook));
        }
    }
    retain_nonempty_hook_arrays(hooks);
}

fn is_dock_managed_hook(entry: &Value, hook: &Path) -> bool {
    let text = entry.to_string();
    let hook = hook.to_string_lossy();
    text.contains(hook.as_ref())
        || text.contains("orbcue")
        || text.contains("agent-activity-dock")
        || text.contains("claude-hook")
        || text.contains("codex-hook")
        || text.contains("cursor-hook")
        || text.contains("hook claude")
        || text.contains("hook codex")
        || text.contains("hook cursor")
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("hooks.json")
        .to_owned()
}

struct HookSpec {
    event: &'static str,
    matcher: Option<&'static str>,
    async_command: bool,
    detach: bool,
}

const fn hook(event: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: None,
        async_command: false,
        detach: false,
    }
}

const fn hook_match(event: &'static str, matcher: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: Some(matcher),
        async_command: false,
        detach: false,
    }
}

const fn async_hook(event: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: None,
        async_command: true,
        detach: false,
    }
}

const fn detach_hook(event: &'static str) -> HookSpec {
    HookSpec {
        event,
        matcher: None,
        async_command: false,
        detach: true,
    }
}

fn dock_handler_matches(entry: &Value, hook: &Path, spec: &HookSpec) -> bool {
    let expected = dock_command_handler(hook, spec);
    let Some(actual) = entry
        .get("hooks")
        .and_then(Value::as_array)
        .and_then(|hooks| hooks.first())
        .or(Some(entry))
    else {
        return false;
    };
    actual.get("command") == expected.get("command") && actual.get("async") == expected.get("async")
}

fn dock_hook_group(hook: &Path, spec: &HookSpec) -> Value {
    let mut group = serde_json::Map::new();
    if let Some(matcher) = spec.matcher {
        group.insert("matcher".to_owned(), json!(matcher));
    }
    group.insert(
        "hooks".to_owned(),
        json!([dock_command_handler(hook, spec)]),
    );
    Value::Object(group)
}

fn hook_script_forwards_args(hook: &Path) -> bool {
    let Ok(text) = fs::read_to_string(hook) else {
        return false;
    };
    #[cfg(windows)]
    {
        text.contains("%*")
    }
    #[cfg(not(windows))]
    {
        text.contains("\"$@\"")
    }
}

fn dock_command_handler(hook: &Path, spec: &HookSpec) -> Value {
    let mut command = hook.to_string_lossy().into_owned();
    if spec.detach {
        command.push_str(" --detach");
    }
    let mut handler = serde_json::Map::new();
    handler.insert("type".to_owned(), json!("command"));
    handler.insert("command".to_owned(), json!(command));
    handler.insert("timeout".to_owned(), json!(5));
    if spec.async_command {
        handler.insert("async".to_owned(), json!(true));
    }
    Value::Object(handler)
}

fn hook_spec_names(specs: &[HookSpec]) -> Vec<String> {
    specs.iter().map(|spec| spec.event.to_owned()).collect()
}

fn hook_spec_labels(specs: &[HookSpec]) -> Vec<String> {
    specs
        .iter()
        .map(|spec| match spec.matcher {
            Some(matcher) => format!("{} ({matcher})", spec.event),
            None => spec.event.to_owned(),
        })
        .collect()
}

fn claude_hook_specs() -> &'static [HookSpec] {
    const SPECS: &[HookSpec] = &[
        hook("SessionStart"),
        hook("UserPromptSubmit"),
        hook("PermissionRequest"),
        hook("PermissionDenied"),
        hook("Notification"),
        hook("Stop"),
        hook("StopFailure"),
        hook("SessionEnd"),
        hook_match("PreToolUse", "AskUserQuestion"),
        async_hook("PostToolUse"),
        async_hook("PostToolUseFailure"),
    ];
    SPECS
}

fn grok_hook_specs() -> &'static [HookSpec] {
    const SPECS: &[HookSpec] = &[
        hook("SessionStart"),
        hook("UserPromptSubmit"),
        hook("Notification"),
        hook("PermissionDenied"),
        hook("Stop"),
        hook("StopFailure"),
        hook("StopCancelled"),
        hook("SessionEnd"),
        hook_match("PreToolUse", "ask_user_question"),
        detach_hook("PostToolUse"),
        detach_hook("PostToolUseFailure"),
    ];
    SPECS
}

fn codex_hook_specs() -> &'static [HookSpec] {
    const SPECS: &[HookSpec] = &[
        hook("SessionStart"),
        hook("UserPromptSubmit"),
        hook("PermissionRequest"),
        hook("Stop"),
        hook("SessionEnd"),
        hook_match("PreToolUse", "AskUserQuestion|ask_user_question"),
        async_hook("PostToolUse"),
    ];
    SPECS
}

fn cursor_hook_specs() -> &'static [HookSpec] {
    const SPECS: &[HookSpec] = &[
        hook("sessionStart"),
        hook("beforeSubmitPrompt"),
        hook("afterAgentResponse"),
        hook("stop"),
        hook("subagentStart"),
        hook("subagentStop"),
        hook("sessionEnd"),
    ];
    SPECS
}

fn cursor_unbounded_events() -> &'static [&'static str] {
    &[
        "sessionStart",
        "afterAgentResponse",
        "stop",
        "subagentStart",
        "subagentStop",
        "sessionEnd",
    ]
}

fn cursor_hook_entry(hook: &Path, spec: &HookSpec) -> Value {
    let mut entry = serde_json::Map::new();
    entry.insert("command".to_owned(), json!(hook.to_string_lossy().as_ref()));
    entry.insert("timeout".to_owned(), json!(5));
    if let Some(matcher) = spec.matcher {
        entry.insert("matcher".to_owned(), json!(matcher));
    }
    if cursor_unbounded_events()
        .iter()
        .any(|name| name == &spec.event)
    {
        entry.insert("loop_limit".to_owned(), Value::Null);
    }
    Value::Object(entry)
}

fn connection_method_for(name: &str) -> Option<ConnectionMethod> {
    hook_agent_named(name).map(|agent| agent.method)
}

#[derive(Clone, Copy)]
enum HookLayout {
    Nested { backup: &'static str },
    GrokFile,
    Cursor,
    OpenCode,
    Pi,
}

#[derive(Clone, Copy)]
struct HookAgent {
    name: &'static str,
    method: ConnectionMethod,
    layout: HookLayout,
}

impl HookAgent {
    fn specs(self) -> &'static [HookSpec] {
        match self.method {
            ConnectionMethod::ClaudeHook => claude_hook_specs(),
            ConnectionMethod::GrokHook => grok_hook_specs(),
            ConnectionMethod::CodexHook => codex_hook_specs(),
            ConnectionMethod::CursorHook => cursor_hook_specs(),
            ConnectionMethod::OpenCodeHook | ConnectionMethod::PiHook => &[],
            ConnectionMethod::Wrapper => &[],
        }
    }

    fn backup_name(self) -> Option<&'static str> {
        match self.layout {
            HookLayout::Nested { backup } => Some(backup),
            HookLayout::Cursor => Some("hooks.json.orbcue.bak"),
            HookLayout::GrokFile | HookLayout::OpenCode | HookLayout::Pi => None,
        }
    }

    fn label(self) -> &'static str {
        match self.name {
            "claude" => "Claude",
            "grok" => "Grok",
            "codex" => "Codex",
            "cursor" => "Cursor",
            "opencode" => "OpenCode",
            "pi" => "Pi",
            other => other,
        }
    }
}

const HOOK_AGENTS: &[HookAgent] = &[
    HookAgent {
        name: "claude",
        method: ConnectionMethod::ClaudeHook,
        layout: HookLayout::Nested {
            backup: "settings.json.orbcue.bak",
        },
    },
    HookAgent {
        name: "grok",
        method: ConnectionMethod::GrokHook,
        layout: HookLayout::GrokFile,
    },
    HookAgent {
        name: "codex",
        method: ConnectionMethod::CodexHook,
        layout: HookLayout::Nested {
            backup: "hooks.json.orbcue.bak",
        },
    },
    HookAgent {
        name: "cursor",
        method: ConnectionMethod::CursorHook,
        layout: HookLayout::Cursor,
    },
    HookAgent {
        name: "opencode",
        method: ConnectionMethod::OpenCodeHook,
        layout: HookLayout::OpenCode,
    },
    HookAgent {
        name: "pi",
        method: ConnectionMethod::PiHook,
        layout: HookLayout::Pi,
    },
];

fn hook_agent(method: ConnectionMethod) -> Option<&'static HookAgent> {
    HOOK_AGENTS.iter().find(|agent| agent.method == method)
}

fn hook_agent_named(name: &str) -> Option<&'static HookAgent> {
    HOOK_AGENTS.iter().find(|agent| agent.name == name)
}

fn unsupported_connect_name(name: &str) -> String {
    orbcue_core::t!(
        "OrbCue 只连接 Claude、Grok、Codex、Cursor、OpenCode 和 Pi（{name} 不行）。其他工具请用 `orb start` / `orb complete` 接入",
        "OrbCue only connects Claude, Grok, Codex, Cursor, OpenCode, and Pi ({name} is not one of them). Other tools can use `orb start` / `orb complete`."
    )
}

fn hook_path(config_dir: &Path, name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        config_dir.join(format!("{name}-hook.cmd"))
    }
    #[cfg(not(windows))]
    {
        config_dir.join(format!("{name}-hook.sh"))
    }
}

impl ConnectionManager {
    fn current_hook_script(&self, method: ConnectionMethod) -> Option<PathBuf> {
        let agent = hook_agent(method)?;
        if let Some(files) = self.plugin_files(agent.layout) {
            return files.into_iter().next().map(|file| file.path);
        }
        Some(hook_path(&self.config_dir, agent.name))
    }

    fn plugin_files(&self, layout: HookLayout) -> Option<Vec<PluginFile>> {
        let orb = &self.dock_binary;
        match layout {
            HookLayout::OpenCode => Some(vec![
                PluginFile {
                    path: self.opencode_plugin_path(),
                    contents: render_template(OPENCODE_PLUGIN_TEMPLATE, orb),
                    entries: opencode_plugin_events(),
                    marker: OPENCODE_PLUGIN_MARKER,
                },
                PluginFile {
                    path: self.opencode_tui_plugin_path(),
                    contents: render_template(OPENCODE_TUI_PLUGIN_TEMPLATE, orb),
                    entries: opencode_tui_events(),
                    marker: OPENCODE_PLUGIN_MARKER,
                },
            ]),
            HookLayout::Pi => Some(vec![PluginFile {
                path: self.pi_extension_path(),
                contents: render_template(PI_EXTENSION_TEMPLATE, orb),
                entries: pi_extension_events(),
                marker: PI_EXTENSION_MARKER,
            }]),
            HookLayout::Nested { .. } | HookLayout::GrokFile | HookLayout::Cursor => None,
        }
    }

    fn write_plugin_files(
        &self,
        layout: HookLayout,
        files: &[PluginFile],
    ) -> Result<PathBuf, String> {
        for file in files {
            refuse_foreign_file(&file.path, file.marker)?;
        }
        for file in files {
            if let Some(parent) = file.path.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            atomic_write(&file.path, file.contents.as_bytes(), 0o644)?;
        }
        if matches!(layout, HookLayout::OpenCode) {
            remove_marked_file(&self.opencode_legacy_plugin_path(), OPENCODE_PLUGIN_MARKER)?;
        }
        files
            .first()
            .map(|file| file.path.clone())
            .ok_or_else(|| "plugin layout has no files".to_owned())
    }

    fn remove_opencode_plugin(&self, recorded: &Path) -> Result<(), String> {
        let mut paths = vec![
            self.opencode_plugin_path(),
            self.opencode_tui_plugin_path(),
            self.opencode_legacy_plugin_path(),
            recorded.to_path_buf(),
        ];
        let mut dirs = vec![self.opencode_plugin_dir()];
        if recorded.file_name().and_then(|name| name.to_str()) == Some("index.js") {
            if let Some(dir) = recorded.parent() {
                paths.push(dir.join("tui.js"));
                dirs.push(dir.to_path_buf());
            }
        }
        for path in paths {
            remove_marked_file(&path, OPENCODE_PLUGIN_MARKER)?;
        }
        for dir in dirs {
            let _ = fs::remove_dir(dir);
        }
        Ok(())
    }

    fn remove_pi_extension(&self, recorded: &Path) -> Result<(), String> {
        remove_marked_file(&self.pi_extension_path(), PI_EXTENSION_MARKER)?;
        remove_marked_file(recorded, PI_EXTENSION_MARKER)
    }
}

struct PluginFile {
    path: PathBuf,
    contents: String,
    entries: Vec<String>,
    marker: &'static str,
}

const OPENCODE_PLUGIN_MARKER: &str = "// OrbCue generated OpenCode plugin.";

fn opencode_config_dir(home: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("OPENCODE_CONFIG_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }
    if let Some(xdg) = env::var_os("XDG_CONFIG_HOME") {
        let xdg = PathBuf::from(xdg);
        if !xdg.as_os_str().is_empty() {
            return xdg.join("opencode");
        }
    }
    home.join(".config").join("opencode")
}

fn opencode_plugin_events() -> Vec<String> {
    [
        "session.created",
        "session.status",
        "session.idle",
        "session.error",
        "session.deleted",
        "permission.asked",
        "permission.replied",
        "question.asked",
        "question.replied",
        "question.rejected",
        "form.created",
        "form.replied",
        "form.cancelled",
        "session.execution.started",
        "session.execution.succeeded",
        "session.execution.failed",
        "session.execution.interrupted",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn opencode_tui_events() -> Vec<String> {
    ["session.deleted", "client.exited"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn js_string_literal(orb: &Path) -> String {
    serde_json::to_string(orb.to_string_lossy().as_ref()).unwrap_or_else(|_| "\"orb\"".to_owned())
}

fn render_template(template: &str, orb: &Path) -> String {
    template.replace("__ORB_JSON__", &js_string_literal(orb))
}

fn generated_file_matches(path: &Path, expected: &str) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes == expected.as_bytes())
}

fn refuse_foreign_file(path: &Path, marker: &str) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(path) {
        if !existing.contains(marker) {
            return Err(format!(
                "refusing to overwrite non-OrbCue file {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn remove_marked_file(path: &Path, marker: &str) -> Result<(), String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Ok(());
    };
    if text.contains(marker) {
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

const OPENCODE_PLUGIN_TEMPLATE: &str = r#"// OrbCue generated OpenCode plugin.
// Forwards lifecycle only. It does not send prompts, questions, commands, or file contents.
// Tab closes and client exit are tui.js. This process outlives the terminal.
import { spawn } from "node:child_process"

const ORB = __ORB_JSON__

const PARENTS = new Map()
const DIRS = new Map()

const CHILD_EVENTS = new Set([
  "permission.asked",
  "permission.replied",
  "question.asked",
  "question.replied",
  "question.rejected",
])

function text(value) {
  if (typeof value !== "string") return undefined
  const trimmed = value.trim()
  return trimmed ? trimmed : undefined
}

function fitPath(value) {
  const path = text(value)
  if (path && path.length <= 256) return path
}

function remember(sessionID, parentID) {
  if (sessionID && parentID && sessionID !== parentID) PARENTS.set(sessionID, parentID)
}

// OpenCode 2 loads a default object that has an id and setup. A named export is rejected.
// server() remains for OpenCode 1, which calls that function instead of setup.
export default {
  id: "orbcue",
  setup(ctx) {
    const controller = new AbortController()
    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
          try {
            await forward(ctx, event)
          } catch {
            // Observer only. A delivery failure must not break OpenCode.
          }
        }
      } catch {
        // subscribe ending is normal when OpenCode stops the plugin.
      }
    })()
    return () => {
      controller.abort()
    }
  },
  async server() {
    return {
      event: async ({ event }) => {
        try {
          await forward(null, event)
        } catch {
          // Observer only. A delivery failure must not break OpenCode.
        }
      },
    }
  },
}

async function forward(ctx, event) {
  const mapped = mapEvent(event)
  if (!mapped) return
  const sessionID = mapped.sessionID
  if (!sessionID || sessionID === "global") return
  const parentID = mapped.parentID || PARENTS.get(sessionID)
  remember(sessionID, parentID)
  // data.location is the session directory. The event envelope location is
  // whichever OpenCode instance published the event, so it is not a project path.
  if (mapped.type === "session.moved") {
    if (!parentID) rememberDir(sessionID, mapped.cwd)
    return
  }
  if (parentID && !CHILD_EVENTS.has(mapped.type)) return
  if (!parentID) rememberDir(sessionID, mapped.cwd)
  const owner = parentID || sessionID
  let cwd = DIRS.get(owner)
  if (!cwd) cwd = await directoryOf(ctx, owner)
  const payload = { type: mapped.type, sessionID: owner }
  if (cwd) payload.cwd = cwd
  if (mapped.status) payload.status = mapped.status
  if (mapped.errorName) payload.errorName = mapped.errorName
  send(payload)
}

function rememberDir(sessionID, cwd) {
  const path = fitPath(cwd)
  if (path) DIRS.set(sessionID, path)
}

async function directoryOf(ctx, sessionID) {
  const known = DIRS.get(sessionID)
  if (known) return known
  const get = ctx && ctx.session && ctx.session.get
  if (typeof get !== "function") return
  try {
    const info = await get({ sessionID })
    const location = info && info.location
    const directory = fitPath(location && location.directory)
    if (directory) DIRS.set(sessionID, directory)
    return directory
  } catch {
    return
  }
}

function mapEvent(event) {
  if (!event || typeof event.type !== "string") return
  const data = event.data && typeof event.data === "object" ? event.data : event.properties || {}
  const info = data.info && typeof data.info === "object" ? data.info : {}
  const form = data.form && typeof data.form === "object" ? data.form : {}
  const sessionID = text(data.sessionID) || text(info.id) || text(form.sessionID)
  const parentID = text(data.parentID) || text(info.parentID)
  // Session directory only. The envelope location belongs to the publisher.
  const cwd = (data.location && text(data.location.directory)) || text(info.directory)
  const status = statusOf(data.status)
  const base = { sessionID, parentID, cwd }
  switch (event.type) {
    case "session.created":
      return { type: "session.created", ...base }
    case "session.moved":
      return { type: "session.moved", ...base }
    case "session.deleted":
      return { type: "session.deleted", ...base }
    case "session.idle":
    case "session.execution.succeeded":
      return { type: "session.idle", ...base }
    case "session.status":
      return status ? { type: "session.status", ...base, status } : undefined
    case "session.execution.started":
      return { type: "session.status", ...base, status: "busy" }
    case "session.error":
    case "session.execution.failed":
      return { type: "session.error", ...base, errorName: errorNameOf(data) }
    case "session.execution.interrupted":
      if (data.reason !== "user") return
      return { type: "session.error", ...base, errorName: "MessageAbortedError" }
    case "permission.asked":
      return { type: "permission.asked", ...base }
    case "permission.replied":
      return { type: "permission.replied", ...base }
    case "question.asked":
    case "form.created":
      return { type: "question.asked", ...base }
    case "question.replied":
    case "form.replied":
      return { type: "question.replied", ...base }
    case "question.rejected":
    case "form.cancelled":
      return { type: "question.rejected", ...base }
    default:
      return
  }
}

function statusOf(status) {
  const value = text(status && status.type) || text(status)
  if (value === "idle" || value === "busy" || value === "retry") return value
}

function errorNameOf(data) {
  const error = data.error && typeof data.error === "object" ? data.error : {}
  const name = text(error.name) || text(error.type) || text(data.errorName)
  if (name && /^[A-Za-z]{1,64}$/.test(name)) return name
}

function send(payload) {
  let child
  try {
    child = spawn(ORB, ["hook", "opencode"], {
      stdio: ["pipe", "ignore", "ignore"],
      windowsHide: true,
    })
  } catch {
    return
  }
  child.on("error", () => {})
  child.unref()
  try {
    child.stdin.end(JSON.stringify(payload))
  } catch {
    // ignore
  }
}
"#;

const OPENCODE_TUI_PLUGIN_TEMPLATE: &str = r#"// OrbCue generated OpenCode plugin.
// Terminal client only. Forwards tab closes and client exit. It does not send titles.
import { spawn, spawnSync } from "node:child_process"

const ORB = __ORB_JSON__

// Hot reload calls cleanup, then loads a new instance in the same process.
// The exit hook is registered once and reads whichever instance is current.
function exitSlot() {
  const root = globalThis
  const key = Symbol.for("orbcue.opencode.exit")
  if (root[key]) return root[key]
  const slot = { current: null }
  root[key] = slot
  process.on("exit", () => {
    const live = slot.current
    if (!live || live.delivered) return
    live.delivered = true
    deliverExit(live)
  })
  return slot
}

export function sessionIDsFromTabs(list) {
  const ids = new Map()
  if (!Array.isArray(list)) return ids
  for (const tab of list) {
    const id = text(tab && tab.sessionID)
    if (!id) continue
    ids.set(id, directoryFromTab(tab))
  }
  return ids
}

export function sessionIDsFromRouter(route) {
  const ids = new Map()
  const id = text(route && route.sessionID)
  if (id) ids.set(id, undefined)
  return ids
}

export function sessionsLeft(previous, next) {
  const left = []
  const keys = previous && previous.keys ? previous.keys() : []
  for (const id of keys) {
    if (!next || !next.has || !next.has(id)) left.push(id)
  }
  return left
}

function text(value) {
  if (typeof value !== "string") return undefined
  const trimmed = value.trim()
  return trimmed ? trimmed : undefined
}

function fitPath(value) {
  const path = text(value)
  if (path && path.length <= 256) return path
}

function directoryFromTab(tab) {
  if (!tab || typeof tab !== "object") return undefined
  const direct = text(tab.cwd) || text(tab.directory)
  if (direct) return direct
  const location = tab.location
  return text(location && location.directory)
}

function readOpen(ui) {
  const tabs = ui && ui.tabs
  if (
    tabs &&
    typeof tabs.enabled === "function" &&
    tabs.enabled() &&
    typeof tabs.list === "function"
  ) {
    return sessionIDsFromTabs(tabs.list())
  }
  const router = ui && ui.router
  const route = router && typeof router.current === "function" ? router.current() : undefined
  return sessionIDsFromRouter(route)
}

function rememberDirs(context, state, next) {
  const session = context && context.client && context.client.session
  const get = session && session.get
  for (const [id, cwd] of next) {
    const fitted = fitPath(cwd)
    if (fitted) state.dirs.set(id, fitted)
    if (state.dirs.has(id) || typeof get !== "function") continue
    void Promise.resolve()
      .then(() => get({ sessionID: id }))
      .then((info) => {
        const data = info && (info.data ?? info)
        const location = data && data.location
        const directory = fitPath(location && location.directory)
        if (directory) state.dirs.set(id, directory)
      })
      .catch(() => {})
  }
}

function observe(context, state) {
  const next = readOpen(context && context.ui)
  rememberDirs(context, state, next)
  if (!state.baseline) {
    state.open = next
    state.baseline = true
    return
  }
  for (const id of sessionsLeft(state.open, next)) {
    const payload = { type: "session.deleted", sessionID: id }
    const cwd = state.dirs.get(id)
    if (cwd) payload.cwd = cwd
    send(payload)
  }
  state.open = next
}

function deliverExit(state) {
  if (process.platform === "win32") return
  const clientPid = process.pid
  for (const id of state.open.keys()) {
    const payload = { type: "client.exited", sessionID: id, clientPid }
    const cwd = state.dirs.get(id)
    if (cwd) payload.cwd = cwd
    sendSync(payload)
  }
}

export default {
  id: "orbcue",
  setup(context) {
    const state = { open: new Map(), dirs: new Map(), baseline: false, delivered: false }
    const slot = exitSlot()
    slot.current = state
    try {
      observe(context, state)
    } catch {
      // A bad first read must not close sessions. The next poll retries the baseline.
    }
    const timer = setInterval(() => {
      try {
        observe(context, state)
      } catch {
        // A bad read must not close every session or break OpenCode.
      }
    }, 500)
    if (typeof timer.unref === "function") timer.unref()
    return () => {
      clearInterval(timer)
    }
  },
}

function send(payload) {
  let child
  try {
    child = spawn(ORB, ["hook", "opencode"], spawnOptions(false))
  } catch {
    return
  }
  child.on("error", () => {})
  child.unref()
  try {
    child.stdin.end(JSON.stringify(payload))
  } catch {
    // ignore
  }
}

function sendSync(payload) {
  try {
    spawnSync(ORB, ["hook", "opencode"], {
      ...spawnOptions(true),
      input: JSON.stringify(payload),
    })
  } catch {
    // The exit hook must not throw.
  }
}

function spawnOptions(sync) {
  const options = {
    stdio: ["pipe", "ignore", "ignore"],
    windowsHide: true,
  }
  if (process.platform !== "win32") options.detached = true
  if (sync) options.timeout = 1500
  return options
}
"#;

const PI_EXTENSION_MARKER: &str = "// OrbCue generated Pi extension.";

fn pi_agent_dir(home: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("PI_CODING_AGENT_DIR") {
        let dir = PathBuf::from(dir);
        if !dir.as_os_str().is_empty() {
            return dir;
        }
    }
    home.join(".pi").join("agent")
}

fn pi_extension_events() -> Vec<String> {
    [
        "session_start",
        "agent_start",
        "agent_before_settle",
        "agent_settled",
        "ui_prompt_start",
        "ui_prompt_end",
        "session_shutdown",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

const PI_EXTENSION_TEMPLATE: &str = r#"// OrbCue generated Pi extension.
// Forwards lifecycle only. It does not send prompts, questions, commands, or file contents.
import { spawn, spawnSync } from "node:child_process"

const ORB = __ORB_JSON__
let outcome = "completed"
let running = false

// A dead terminal makes Pi call process.exit without session_shutdown.
// One exit hook covers that path. Quit still waits in the handler below.
function exitSlot() {
  const root = globalThis
  const key = Symbol.for("orbcue.pi.close")
  if (root[key]) return root[key]
  const slot = { active: null, closed: false }
  root[key] = slot
  process.on("exit", () => {
    if (slot.closed) return
    const active = slot.active
    if (!active || !active.sessionID) return
    slot.closed = true
    sendQuitSync(active)
  })
  return slot
}

function fitPath(value) {
  if (typeof value !== "string") return
  const trimmed = value.trim()
  if (!trimmed || trimmed.length > 256) return
  return trimmed
}

function targetOf(ctx) {
  const sessionID = sessionIdOf(ctx)
  if (!sessionID) return
  return { sessionID, cwd: fitPath(cwdOf(ctx)) }
}

function remember(target) {
  if (!target) return
  exitSlot().active = target
}

function payloadOf(target, type, outcomeValue) {
  const payload = { type, sessionID: target.sessionID }
  if (target.cwd) payload.cwd = target.cwd
  if (outcomeValue) payload.outcome = outcomeValue
  return payload
}

function spawnOptions(quit, sync) {
  const options = {
    stdio: ["pipe", "ignore", "ignore"],
    windowsHide: true,
  }
  if (quit && process.platform !== "win32") options.detached = true
  if (sync) options.timeout = 1500
  return options
}

export default function (pi) {
  exitSlot()
  pi.on("session_start", (_event, ctx) => {
    outcome = "completed"
    running = false
    send(ctx, "session.started")
  })
  pi.on("agent_start", (_event, ctx) => {
    running = true
    send(ctx, "agent.started")
  })
  pi.on("agent_before_settle", (event) => {
    const value = event && event.outcome
    if (value === "completed" || value === "aborted" || value === "error") outcome = value
  })
  pi.on("agent_settled", (_event, ctx) => {
    const settled = outcome
    outcome = "completed"
    running = false
    send(ctx, "agent.settled", settled)
  })
  pi.on("ui_prompt_start", (event, ctx) => {
    const kind = event && event.kind
    if (kind === "confirm") send(ctx, "permission.asked")
    else if (kind === "select" || kind === "input" || kind === "editor" || kind === "custom") {
      send(ctx, "question.asked")
    }
  })
  pi.on("ui_prompt_end", (_event, ctx) => {
    send(ctx, running ? "agent.started" : "session.started")
  })
  pi.on("session_shutdown", (event, ctx) => {
    if (event && event.reason === "quit") return sendQuit(ctx)
  })
}

function sendQuit(ctx) {
  return new Promise((resolve) => {
    let settled = false
    let child
    const finish = () => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      if (child && typeof child.unref === "function") child.unref()
      resolve()
    }
    const timer = setTimeout(finish, 1500)
    try {
      const target = targetOf(ctx)
      if (!target) {
        finish()
        return
      }
      remember(target)
      child = spawn(ORB, ["hook", "pi"], spawnOptions(true, false))
      child.on("error", finish)
      child.on("exit", (code) => {
        if (code === 0) exitSlot().closed = true
        finish()
      })
      child.stdin.end(JSON.stringify(payloadOf(target, "session.closed")))
    } catch {
      finish()
    }
  })
}

function sendQuitSync(target) {
  try {
    if (!target || !target.sessionID) return
    const options = spawnOptions(true, true)
    options.input = JSON.stringify(payloadOf(target, "session.closed"))
    spawnSync(ORB, ["hook", "pi"], options)
  } catch {
    // The exit hook must not throw.
  }
}

function send(ctx, type, outcomeValue) {
  try {
    const target = targetOf(ctx)
    remember(target)
    if (!target) return
    const child = spawn(ORB, ["hook", "pi"], spawnOptions(false, false))
    child.on("error", () => {})
    child.unref()
    child.stdin.end(JSON.stringify(payloadOf(target, type, outcomeValue)))
  } catch {
    // Observer only. A delivery failure must not break Pi.
  }
}

function sessionIdOf(ctx) {
  const manager = ctx && ctx.sessionManager
  const get = manager && manager.getSessionId
  if (typeof get !== "function") return
  try {
    const id = get.call(manager)
    if (typeof id !== "string") return
    const trimmed = id.trim()
    return trimmed ? trimmed : undefined
  } catch {
    return
  }
}

function cwdOf(ctx) {
  const manager = ctx && ctx.sessionManager
  const get = manager && manager.getCwd
  if (typeof get === "function") {
    try {
      const value = get.call(manager)
      if (typeof value === "string" && value.trim()) return value.trim()
    } catch {
      // Fall through to the context directory.
    }
  }
  const cwd = ctx && ctx.cwd
  if (typeof cwd !== "string") return
  const trimmed = cwd.trim()
  return trimmed ? trimmed : undefined
}
"#;

fn hook_script(dock_binary: &Path, provider: &str) -> String {
    #[cfg(windows)]
    {
        return format!(
            "@echo off\r\nrem OrbCue generated {provider} hook.\r\n{} hook {provider} %*\r\nexit /b 0\r\n",
            windows_batch_quote(&dock_binary.to_string_lossy())
        );
    }
    #[cfg(not(windows))]
    {
        format!(
            "#!/bin/sh\n# OrbCue generated {provider} hook.\n# exec so orb's PPID is the agent; liveness reaps that PID.\nexec {} hook {provider} \"$@\"\n",
            shell_quote(&dock_binary.to_string_lossy())
        )
    }
}

fn install_grok_hooks(hooks_path: &Path, hook: &Path) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(hooks_path) {
        if !existing.contains("orbcue")
            && !existing.contains("agent-activity-dock")
            && !existing.contains("hook grok")
        {
            return Err(format!(
                "refusing to overwrite non-Dock file {}",
                hooks_path.display()
            ));
        }
    }
    let mut hooks = serde_json::Map::new();
    upsert_hook_events(&mut hooks, grok_hook_specs(), hook, |spec| {
        dock_hook_group(hook, spec)
    })?;
    let document = json!({
        "name": "orbcue",
        "hooks": hooks
    });
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    atomic_write(hooks_path, &bytes, 0o600)
}

pub(crate) fn strip_path_block(old: &str, start: &str, end: &str) -> Option<String> {
    let marker_start = old.find(start)?;
    let line_start = old[..marker_start]
        .rfind('\n')
        .map(|position| position + 1)
        .unwrap_or(marker_start);
    let marker_end = old[marker_start..].find(end)?;
    let mut end_pos = marker_start + marker_end + end.len();
    if old.as_bytes().get(end_pos) == Some(&b'\r') {
        end_pos += 1;
    }
    if old.as_bytes().get(end_pos) == Some(&b'\n') {
        end_pos += 1;
    }
    let mut cleaned = old.to_owned();
    cleaned.replace_range(line_start..end_pos, "");
    Some(cleaned)
}

#[cfg(windows)]
fn windows_batch_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(not(windows))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn valid_agent_name(name: &str) -> bool {
    name != "."
        && name != ".."
        && !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

fn validate_connection_request(name: &str, original: &Path) -> Result<ConnectionMethod, String> {
    if !valid_agent_name(name) {
        return Err("agent name must contain only letters, numbers, '.', '_' or '-'".to_owned());
    }
    if !original.is_file() {
        return Err(format!(
            "original executable does not exist: {}",
            original.display()
        ));
    }
    connection_method_for(name).ok_or_else(|| unsupported_connect_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("orbcue-settings-{nonce}"));
        fs::create_dir_all(&root).expect("create temporary settings directory");
        root
    }

    #[test]
    fn invalid_claude_settings_are_not_overwritten() {
        let root = temp_root();
        let settings = root.join("settings.json");
        let hook = root.join("claude-hook.sh");
        fs::write(&settings, b"{not-json").unwrap();

        let error = install_claude_settings_at(&settings, &hook).unwrap_err();
        assert!(error.contains("not valid JSON"));
        assert_eq!(fs::read(&settings).unwrap(), b"{not-json");
        assert!(!settings.with_file_name("settings.json.orbcue.bak").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_settings_backup_keeps_the_first_original() {
        let root = temp_root();
        let settings = root.join("settings.json");
        let hook = root.join("claude-hook.sh");
        let original =
            br#"{"hooks":{"UserEvent":[{"hooks":[{"type":"command","command":"user-hook"}]}]}}"#;
        fs::write(&settings, original).unwrap();

        let backup = install_claude_settings_at(&settings, &hook).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), original);
        let changed = fs::read(&settings).unwrap();
        assert!(String::from_utf8_lossy(&changed).contains("claude-hook.sh"));

        fs::write(&settings, br#"{"custom":true}"#).unwrap();
        let second_backup = install_claude_settings_at(&settings, &hook).unwrap();
        assert_eq!(second_backup, backup);
        assert_eq!(fs::read(&backup).unwrap(), original);

        uninstall_claude_settings_at(&settings, &hook).unwrap();
        assert!(!String::from_utf8_lossy(&fs::read(&settings).unwrap()).contains("claude-hook.sh"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reconnect_drops_legacy_tool_hooks() {
        let root = temp_root();
        let settings = root.join("settings.json");
        let hook = root.join("claude-hook.sh");
        fs::write(
            &settings,
            br#"{
              "hooks": {
                "PreToolUse": [{"hooks":[{"type":"command","command":"/tmp/claude-hook.sh"}]}],
                "PostToolUse": [{"hooks":[{"type":"command","command":"user-hook"}]}],
                "UserPromptSubmit": [{"hooks":[{"type":"command","command":"user-hook"}]}]
              }
            }"#,
        )
        .unwrap();
        install_claude_settings_at(&settings, &hook).unwrap();
        let connected = fs::read_to_string(&settings).unwrap();
        assert!(connected.contains("PreToolUse"));
        assert!(connected.contains("AskUserQuestion"));
        assert!(!connected.contains("/tmp/claude-hook.sh"));
        assert!(connected.contains("PostToolUse"));
        assert!(connected.contains("UserPromptSubmit"));
        assert!(connected.contains("user-hook"));
        assert!(connected.contains("claude-hook.sh"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn connection_preview_warnings_default_when_absent() {
        let preview: ConnectionPreview = serde_json::from_str(
            r#"{
                "name":"cursor",
                "original":"/bin/cursor-agent",
                "method":"CursorHook",
                "files":[],
                "will_not":[],
                "notes":[]
            }"#,
        )
        .unwrap();
        assert!(preview.warnings.is_empty());
    }

    #[test]
    fn invalid_cursor_hooks_are_not_overwritten() {
        let root = temp_root();
        let hooks = root.join("hooks.json");
        let hook = root.join("cursor-hook.sh");
        fs::write(&hooks, b"{not-json").unwrap();

        let error = install_cursor_hooks_at(&hooks, &hook).unwrap_err();
        assert!(error.contains("not valid JSON"));
        assert_eq!(fs::read(&hooks).unwrap(), b"{not-json");
        fs::remove_dir_all(root).unwrap();
    }
}
