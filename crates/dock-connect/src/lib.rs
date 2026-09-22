//! Revocable, zero-reinstall Agent connections.
//!
//! Discovers Claude, Grok, Codex, and Cursor already on PATH or in folders the
//! user adds, then writes native hooks. It never replaces an Agent executable.
//! Other tools emit events with `orb start` / `orb complete`; leftover wrapper
//! records can still be disconnected.

mod discover;
mod grok_compat;
mod replace_tab;
mod run_alias;
mod user_path;
mod wsl_cli;

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
pub(crate) const FIRST_PARTY_AGENTS: &[&str] = &["claude", "codex", "cursor", "grok"];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionMethod {
    /// Leftover records from before hook-only connect. Disconnect still clears them; new connects never create these.
    Wrapper,
    ClaudeHook,
    GrokHook,
    CodexHook,
    CursorHook,
}

impl ConnectionMethod {
    pub fn limitation(self) -> &'static str {
        match self {
            Self::Wrapper => {
                "遗留包装连接，看不到「正在等你输入」。可断开后改用 `orb start` / `orb complete`"
            }
            Self::ClaudeHook => "",
            Self::GrokHook => "",
            Self::CodexHook => {
                "用 Esc 或 Ctrl+C 打断时不会离开「工作中」，对话报错也不会显示为失败；可用「清除」，或退出 Codex 后任务会消失"
            }
            Self::CursorHook => "偶尔不会通知已经结束，任务会停在「工作中」，直到进程退出",
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

const NATIVE_NOTIFY_NOTE: &str = "该 Agent 自己也可能弹系统通知；Dock 的通知可在设置里关掉";

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
            return Err("请选择一个文件夹".to_owned());
        }
        let found = discover::agents_in_dir(dir);
        if found.is_empty() {
            return Err("这个文件夹里没有支持的工具（Claude、Grok、Codex 或 Cursor）".to_owned());
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
                format!("不替换 Agent 本体（{}）", original.display()),
                "不修改、不删除用户其他 Hook".to_owned(),
                "不读取 transcript / prompt / 命令 / 代码".to_owned(),
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
                if let Some(hook) = current_hook_script(&self.config_dir, method) {
                    record.hook_script = Some(hook);
                }
                if record != *existing {
                    file.agents.insert(name.to_owned(), record.clone());
                    self.save(&file)?;
                }
                self.drop_wrapper_path_if_unused(&file)?;
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
        self.drop_wrapper_path_if_unused(&file)?;
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
        self.drop_wrapper_path_if_unused(&file)?;
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
            let Some(hook) = current_hook_script(&self.config_dir, record.method) else {
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
                    entries: vec!["仅在备份不存在时创建".to_owned()],
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

    fn drop_wrapper_path_if_unused(&self, file: &ConnectionFile) -> Result<(), String> {
        if file.agents.values().any(|item| item.wrapper.is_some()) {
            return Ok(());
        }
        self.remove_path_snippet()?;
        self.remove_empty_data_dir();
        Ok(())
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

    fn install_hook(&self, agent: &HookAgent) -> Result<PathBuf, String> {
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
            match hook_agent(record.method) {
                Some(agent) => match agent.layout {
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
                },
                None => {}
            }
            if hook.exists() {
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

fn install_nested_hooks_at(
    settings_path: &Path,
    hook: &Path,
    specs: &[HookSpec],
    backup_name: &str,
) -> Result<(), String> {
    let (existing, mut settings) = read_json_document(settings_path, json!({}))?;
    {
        let hooks = hooks_object_mut(&mut settings, &file_label(settings_path))?;
        upsert_hook_events(hooks, specs, hook, |spec| dock_hook_group(hook, spec))?;
    }
    write_json_with_backup(settings_path, existing, backup_name, &settings)
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
    let (existing, mut document) =
        read_json_document(hooks_path, json!({"version": 1, "hooks": {}}))?;
    {
        let object = document
            .as_object_mut()
            .ok_or_else(|| "Cursor hooks.json must be a JSON object".to_owned())?;
        if !object.contains_key("version") {
            object.insert("version".to_owned(), json!(1));
        }
    }
    {
        let hooks = hooks_object_mut(&mut document, "Cursor")?;
        upsert_hook_events(hooks, cursor_hook_specs(), hook, |spec| {
            cursor_hook_entry(hook, spec)
        })?;
    }
    write_json_with_backup(hooks_path, existing, "hooks.json.orbcue.bak", &document)
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

fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|error| error.to_string())?;
    set_mode(&temp, mode)?;
    fs::rename(temp, path).map_err(|error| error.to_string())
}

fn existing_mode(path: &Path, fallback: u32) -> u32 {
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
    match method {
        ConnectionMethod::ClaudeHook => vec![
            "首次修改前备份 settings.json".to_owned(),
            NATIVE_NOTIFY_NOTE.to_owned(),
        ],
        ConnectionMethod::CodexHook => vec![
            "首次修改前备份 hooks.json".to_owned(),
            ConnectionMethod::CodexHook.limitation().to_owned(),
            "Codex 可能要求在 /hooks 里信任新命令".to_owned(),
            NATIVE_NOTIFY_NOTE.to_owned(),
        ],
        ConnectionMethod::CursorHook => vec![
            "首次修改前备份 hooks.json".to_owned(),
            ConnectionMethod::CursorHook.limitation().to_owned(),
            NATIVE_NOTIFY_NOTE.to_owned(),
        ],
        ConnectionMethod::Wrapper | ConnectionMethod::GrokHook => Vec::new(),
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
        hook("sessionEnd"),
    ];
    SPECS
}

fn cursor_unbounded_events() -> &'static [&'static str] {
    &["sessionStart", "afterAgentResponse", "stop", "sessionEnd"]
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
            ConnectionMethod::Wrapper => &[],
        }
    }

    fn backup_name(self) -> Option<&'static str> {
        match self.layout {
            HookLayout::Nested { backup } => Some(backup),
            HookLayout::Cursor => Some("hooks.json.orbcue.bak"),
            HookLayout::GrokFile => None,
        }
    }

    fn label(self) -> &'static str {
        match self.name {
            "claude" => "Claude",
            "grok" => "Grok",
            "codex" => "Codex",
            "cursor" => "Cursor",
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
];

fn hook_agent(method: ConnectionMethod) -> Option<&'static HookAgent> {
    HOOK_AGENTS.iter().find(|agent| agent.method == method)
}

fn hook_agent_named(name: &str) -> Option<&'static HookAgent> {
    HOOK_AGENTS.iter().find(|agent| agent.name == name)
}

fn unsupported_connect_name(name: &str) -> String {
    format!(
        "OrbCue 只连接 Claude、Grok、Codex 和 Cursor（{name} 不行）。其他工具请用 `orb start` / `orb complete` 接入"
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

fn current_hook_script(config_dir: &Path, method: ConnectionMethod) -> Option<PathBuf> {
    hook_agent(method).map(|agent| hook_path(config_dir, agent.name))
}

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

fn strip_path_block(old: &str, start: &str, end: &str) -> Option<String> {
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
