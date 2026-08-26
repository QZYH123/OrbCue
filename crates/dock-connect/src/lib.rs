//! Revocable, zero-reinstall Agent connections.
//!
//! This crate discovers commands already present on PATH, in common install
//! directories, and in folders the user adds. It writes user-level wrappers/hooks
//! and never replaces an Agent executable.

mod discover;
mod grok_compat;
mod run_alias;
mod user_path;
mod wsl_cli;

pub use discover::{
    agent_is_connectable, agent_origin, agents_in_dir, choose_discovered, discover_agents,
    discover_agents_with_extras, looks_like_cursor_cli_path, merge_search_path,
    parse_login_path_output, probe_login_path, InventorySnapshotCache, ProbeOutput, LOGIN_PATH_END,
    LOGIN_PATH_START,
};
pub use grok_compat::{
    connection_warnings, grok_compat_cursor_hooks_enabled, GROK_COMPAT_CURSOR_HOOKS_WARNING,
};
pub use run_alias::{
    current as current_run_alias, preferred as preferred_run_alias, set as set_run_alias,
    validate as validate_run_alias, view_err as run_alias_err, view_ok as run_alias_ok,
    wsl_dock_cli_is_missing, wsl_runtime_is_absent, wsl_side_is_absent, AliasView,
};
pub use user_path::{
    default_windows_cli_dir, ensure_dir_on_user_path, install_windows_cli, merge_path_entries,
};
pub use wsl_cli::{
    choose_packaged_linux_dock, decode_console_output, dock_version_matches,
    is_infrastructure_wsl_distro, looks_like_linux_dock, packaged_linux_dock_candidates,
    packaged_linux_dock_is_usable, parse_dock_version_output, parse_installable_wsl_distros,
    parse_wsl_distro_list, wsl_dock_install_shell, PACKAGED_LINUX_DOCK_NAME,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const PATH_START: &str = "# >>> orbcue PATH >>>";
const PATH_END: &str = "# <<< orbcue PATH <<<";
const LEGACY_PATH_START: &str = "# >>> agent-activity-dock PATH >>>";
const LEGACY_PATH_END: &str = "# <<< agent-activity-dock PATH <<<";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionMethod {
    Wrapper,
    ClaudeHook,
    GrokHook,
    CodexHook,
    CursorHook,
}

impl ConnectionMethod {
    pub fn limitation(self) -> &'static str {
        match self {
            Self::Wrapper => "看不到「正在等你输入」",
            Self::ClaudeHook => "",
            Self::GrokHook => "",
            Self::CodexHook => {
                "用 Esc 或 Ctrl+C 打断时不会离开「工作中」，对话报错也不会显示为失败；可用「清除」，或退出 Codex 后任务会消失"
            }
            Self::CursorHook => "偶尔不会通知已经结束，任务会停在「工作中」，直到进程退出",
        }
    }

    fn capabilities(self) -> &'static [&'static str] {
        match self {
            Self::Wrapper => &["started", "completed", "failed"],
            Self::ClaudeHook => &["started", "waiting", "completed", "failed"],
            Self::GrokHook | Self::CursorHook => {
                &["started", "waiting", "completed", "failed", "cancelled"]
            }
            Self::CodexHook => &["started", "waiting", "completed"],
        }
    }
}

fn connection_record(
    name: &str,
    original: &Path,
    method: ConnectionMethod,
    wrapper: Option<PathBuf>,
    hook_script: Option<PathBuf>,
    settings_backup: Option<PathBuf>,
) -> ConnectionRecord {
    ConnectionRecord {
        name: name.to_owned(),
        original: original.to_owned(),
        method,
        wrapper,
        hook_script,
        settings_backup,
        capabilities: method
            .capabilities()
            .iter()
            .map(|item| (*item).to_owned())
            .collect(),
        limitation: method.limitation().to_owned(),
        installed_at: now_string(),
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
    #[serde(default)]
    pub settings_backup: Option<PathBuf>,
    pub capabilities: Vec<String>,
    pub limitation: String,
    pub installed_at: String,
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
    #[serde(default = "default_connectable")]
    pub connectable: bool,
}

fn default_connectable() -> bool {
    true
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
    pub dry_run: bool,
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
        if !valid_agent_name(name) {
            return Err(
                "agent name must contain only letters, numbers, '.', '_' or '-'".to_owned(),
            );
        }
        if !original.is_file() {
            return Err(format!(
                "original executable does not exist: {}",
                original.display()
            ));
        }
        let method = connection_method_for(name);
        Ok(ConnectionPreview {
            name: name.to_owned(),
            original: original.to_owned(),
            method,
            dry_run: true,
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
        if !valid_agent_name(name) {
            return Err(
                "agent name must contain only letters, numbers, '.', '_' or '-'".to_owned(),
            );
        }
        if !original.is_file() {
            return Err(format!(
                "original executable does not exist: {}",
                original.display()
            ));
        }
        let method = connection_method_for(name);
        let mut file = self.load();
        if let Some(existing) = file.agents.get(name) {
            if existing.original == original && existing.method == method {
                self.reinstall_artifacts(method)?;
                self.drop_wrapper_path_if_unused(&file)?;
                return Ok(existing.clone());
            }
        }
        let record = match method {
            ConnectionMethod::Wrapper => {
                let wrapper = self.install_wrapper(name, original)?;
                connection_record(name, original, method, Some(wrapper), None, None)
            }
            ConnectionMethod::ClaudeHook => {
                let (hook, settings_backup) = self.install_claude_hook()?;
                connection_record(name, original, method, None, Some(hook), settings_backup)
            }
            ConnectionMethod::GrokHook => {
                let hook = self.install_grok_hook()?;
                connection_record(name, original, method, None, Some(hook), None)
            }
            ConnectionMethod::CodexHook => {
                let (hook, settings_backup) = self.install_codex_hook()?;
                connection_record(name, original, method, None, Some(hook), settings_backup)
            }
            ConnectionMethod::CursorHook => {
                let (hook, settings_backup) = self.install_cursor_hook()?;
                connection_record(name, original, method, None, Some(hook), settings_backup)
            }
        };
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
        let mut file = self.load();
        let Some(record) = file.agents.remove(name) else {
            return Ok(false);
        };
        self.remove_artifacts(&record)?;
        self.save(&file)?;
        self.drop_wrapper_path_if_unused(&file)?;
        Ok(true)
    }

    fn preview_files(&self, name: &str, method: ConnectionMethod) -> Vec<PreviewFile> {
        match method {
            ConnectionMethod::Wrapper => self.preview_wrapper_files(name),
            ConnectionMethod::ClaudeHook => self.preview_claude_files(),
            ConnectionMethod::GrokHook => self.preview_grok_files(),
            ConnectionMethod::CodexHook => self.preview_codex_files(),
            ConnectionMethod::CursorHook => self.preview_cursor_files(),
        }
    }

    fn preview_wrapper_files(&self, name: &str) -> Vec<PreviewFile> {
        let wrapper = wrapper_path(&self.data_dir, name);
        let mut files = vec![
            PreviewFile {
                path: wrapper.clone(),
                action: preview_action(&wrapper),
                entries: vec!["started".into(), "completed".into(), "failed".into()],
            },
            self.preview_connections_file(),
        ];
        for profile in self.profile_targets() {
            let existing = fs::read_to_string(&profile).unwrap_or_default();
            if existing.contains(PATH_START) || existing.contains(LEGACY_PATH_START) {
                continue;
            }
            files.push(PreviewFile {
                path: profile.clone(),
                action: preview_action(&profile),
                entries: vec![PATH_START.to_owned()],
            });
        }
        files
    }

    fn preview_claude_files(&self) -> Vec<PreviewFile> {
        let hook = hook_path(&self.config_dir, "claude");
        let settings = claude_settings_path();
        let events = hook_spec_labels(claude_hook_specs());
        let mut files = vec![
            PreviewFile {
                path: hook.clone(),
                action: preview_action(&hook),
                entries: events.clone(),
            },
            PreviewFile {
                path: settings.clone(),
                action: preview_action(&settings),
                entries: events,
            },
        ];
        if settings.is_file() {
            let backup = settings.with_file_name("settings.json.orbcue.bak");
            let mut entries = Vec::new();
            if backup.is_file() {
                entries.push("仅在备份不存在时创建".to_owned());
            }
            files.push(PreviewFile {
                path: backup,
                action: PreviewAction::Create,
                entries,
            });
        }
        files.push(self.preview_connections_file());
        files
    }

    fn preview_grok_files(&self) -> Vec<PreviewFile> {
        let hook = hook_path(&self.config_dir, "grok");
        let hooks = self.grok_hooks_file();
        let events = hook_spec_labels(grok_hook_specs());
        vec![
            PreviewFile {
                path: hook.clone(),
                action: preview_action(&hook),
                entries: events.clone(),
            },
            PreviewFile {
                path: hooks.clone(),
                action: preview_action(&hooks),
                entries: events,
            },
            self.preview_connections_file(),
        ]
    }

    fn preview_codex_files(&self) -> Vec<PreviewFile> {
        self.preview_shared_hooks_files(
            "codex",
            &self.codex_hooks_file(),
            &hook_spec_labels(codex_hook_specs()),
        )
    }

    fn preview_cursor_files(&self) -> Vec<PreviewFile> {
        self.preview_shared_hooks_files(
            "cursor",
            &self.cursor_hooks_file(),
            &hook_spec_labels(cursor_hook_specs()),
        )
    }

    fn preview_shared_hooks_files(
        &self,
        name: &str,
        hooks: &Path,
        events: &[String],
    ) -> Vec<PreviewFile> {
        let hook = hook_path(&self.config_dir, name);
        let mut files = vec![
            PreviewFile {
                path: hook.clone(),
                action: preview_action(&hook),
                entries: events.to_vec(),
            },
            PreviewFile {
                path: hooks.to_path_buf(),
                action: preview_action(hooks),
                entries: events.to_vec(),
            },
        ];
        if hooks.is_file() {
            files.push(PreviewFile {
                path: hooks.with_file_name(format!(
                    "{}.orbcue.bak",
                    hooks
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("hooks.json")
                )),
                action: PreviewAction::Create,
                entries: vec!["仅在备份不存在时创建".to_owned()],
            });
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

    fn install_wrapper(&self, name: &str, original: &Path) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.data_dir).map_err(|error| error.to_string())?;
        set_mode(&self.data_dir, 0o700)?;
        let wrapper = wrapper_path(&self.data_dir, name);
        if wrapper == original {
            return Err(format!(
                "refusing to replace the real {name} executable with its own wrapper"
            ));
        }
        if wrapper.exists() {
            let existing = fs::read_to_string(&wrapper).map_err(|error| error.to_string())?;
            if !existing.contains("Agent Activity Dock generated wrapper")
                && !existing.contains("OrbCue generated wrapper")
            {
                return Err(format!(
                    "refusing to overwrite non-OrbCue file {}",
                    wrapper.display()
                ));
            }
        }
        let script = wrapper_script(name, &self.dock_binary, original);
        atomic_write(&wrapper, script.as_bytes(), 0o700)?;
        self.ensure_path_snippet()?;
        Ok(wrapper)
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
        match method {
            ConnectionMethod::Wrapper => Ok(()),
            ConnectionMethod::ClaudeHook => self.install_claude_hook().map(|_| ()),
            ConnectionMethod::GrokHook => self.install_grok_hook().map(|_| ()),
            ConnectionMethod::CodexHook => self.install_codex_hook().map(|_| ()),
            ConnectionMethod::CursorHook => self.install_cursor_hook().map(|_| ()),
        }
    }

    fn drop_wrapper_path_if_unused(&self, file: &ConnectionFile) -> Result<(), String> {
        if file.agents.values().any(|item| item.wrapper.is_some()) {
            return Ok(());
        }
        self.remove_path_snippet()?;
        self.remove_empty_data_dir();
        Ok(())
    }

    fn install_claude_hook(&self) -> Result<(PathBuf, Option<PathBuf>), String> {
        let hook = self.write_hook_script("claude")?;
        match install_claude_settings(&hook) {
            Ok(path) => Ok((hook, path)),
            Err(error) => {
                let _ = fs::remove_file(&hook);
                Err(format!("cannot update Claude settings: {error}"))
            }
        }
    }

    fn install_grok_hook(&self) -> Result<PathBuf, String> {
        let hook = self.write_hook_script("grok")?;
        if let Err(error) = install_grok_hooks(&self.grok_hooks_file(), &hook) {
            let _ = fs::remove_file(&hook);
            return Err(format!("cannot update Grok hooks: {error}"));
        }
        Ok(hook)
    }

    fn grok_hooks_file(&self) -> PathBuf {
        self.grok_home.join("hooks").join("orbcue.json")
    }

    fn install_codex_hook(&self) -> Result<(PathBuf, Option<PathBuf>), String> {
        let hook = self.write_hook_script("codex")?;
        match install_nested_hooks_at(
            &self.codex_hooks_file(),
            &hook,
            codex_hook_specs(),
            "hooks.json.orbcue.bak",
        ) {
            Ok(path) => Ok((hook, path)),
            Err(error) => {
                let _ = fs::remove_file(&hook);
                Err(format!("cannot update Codex hooks: {error}"))
            }
        }
    }

    fn install_cursor_hook(&self) -> Result<(PathBuf, Option<PathBuf>), String> {
        let hook = self.write_hook_script("cursor")?;
        match install_cursor_hooks_at(&self.cursor_hooks_file(), &hook) {
            Ok(path) => Ok((hook, path)),
            Err(error) => {
                let _ = fs::remove_file(&hook);
                Err(format!("cannot update Cursor hooks: {error}"))
            }
        }
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
            match record.method {
                ConnectionMethod::ClaudeHook => uninstall_claude_settings(hook)?,
                ConnectionMethod::GrokHook => {
                    let grok_hooks = self.grok_hooks_file();
                    if grok_hooks.exists() {
                        fs::remove_file(&grok_hooks).map_err(|error| error.to_string())?;
                    }
                }
                ConnectionMethod::CodexHook => {
                    uninstall_nested_hooks_at(&self.codex_hooks_file(), hook)?
                }
                ConnectionMethod::CursorHook => {
                    uninstall_cursor_hooks_at(&self.cursor_hooks_file(), hook)?
                }
                ConnectionMethod::Wrapper => {}
            }
            if hook.exists() {
                fs::remove_file(hook).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn ensure_path_snippet(&self) -> Result<(), String> {
        if let Err(error) = crate::ensure_dir_on_user_path(&self.data_dir) {
            eprintln!(
                "OrbCue: could not add {} to user PATH: {error}",
                self.data_dir.display()
            );
        }
        for profile in self.profile_targets() {
            let old = fs::read_to_string(&profile).unwrap_or_default();
            if old.contains(PATH_START) {
                continue;
            }
            let stripped =
                strip_path_block(&old, LEGACY_PATH_START, LEGACY_PATH_END).unwrap_or(old);
            let block = snippet_for(&profile, &self.data_dir);
            let mode = existing_mode(&profile, 0o600);
            atomic_write(&profile, format!("{stripped}{block}").as_bytes(), mode)?;
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

    fn profile_targets(&self) -> Vec<PathBuf> {
        let mut targets: Vec<PathBuf> = self
            .profile_candidates()
            .into_iter()
            .filter(|path| path.is_file())
            .collect();
        #[cfg(not(windows))]
        {
            let shell = env::var("SHELL").unwrap_or_default();
            if shell.rsplit('/').next() == Some("zsh")
                && !targets.iter().any(|path| is_zsh_profile(path))
            {
                targets.push(self.home.join(".zshrc"));
            }
            if matches!(
                shell.rsplit('/').next(),
                Some("pwsh" | "powershell" | "pwsh.exe" | "powershell.exe")
            ) && !targets.iter().any(|path| is_powershell_profile(path))
            {
                targets.push(self.linux_powershell_profile());
            }
            if shell.rsplit('/').next() == Some("fish")
                && !targets.iter().any(|path| is_fish_profile(path))
            {
                targets.push(self.home.join(".config").join("fish").join("config.fish"));
            }
        }
        if targets.is_empty() {
            targets.push(self.default_profile());
        }
        targets
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

    fn default_profile(&self) -> PathBuf {
        #[cfg(windows)]
        {
            self.home
                .join("Documents")
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1")
        }
        #[cfg(not(windows))]
        {
            self.home.join(".profile")
        }
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

fn install_claude_settings(hook: &Path) -> Result<Option<PathBuf>, String> {
    install_claude_settings_at(&claude_settings_path(), hook)
}

fn install_claude_settings_at(
    settings_path: &Path,
    hook: &Path,
) -> Result<Option<PathBuf>, String> {
    install_nested_hooks_at(
        settings_path,
        hook,
        claude_hook_specs(),
        "settings.json.orbcue.bak",
    )
}

fn install_nested_hooks_at(
    settings_path: &Path,
    hook: &Path,
    specs: &[HookSpec],
    backup_name: &str,
) -> Result<Option<PathBuf>, String> {
    let existing = match fs::read(settings_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let mut settings: Value = match existing.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| format!("{} is not valid JSON: {error}", file_label(settings_path)))?,
        None => json!({}),
    };
    let hooks = settings
        .as_object_mut()
        .ok_or_else(|| format!("{} must be a JSON object", file_label(settings_path)))?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| format!("{} hooks must be an object", file_label(settings_path)))?;
    for spec in specs {
        let event = spec.event.to_owned();
        let entries = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let entries = entries
            .as_array_mut()
            .ok_or_else(|| format!("hook {event} must be an array"))?;
        entries.retain(|entry| !is_dock_managed_hook(entry, hook));
        let mut group = serde_json::Map::new();
        if let Some(matcher) = spec.matcher {
            group.insert("matcher".to_owned(), json!(matcher));
        }
        group.insert(
            "hooks".to_owned(),
            json!([{
                "type": "command",
                "command": hook.to_string_lossy(),
                "timeout": 5
            }]),
        );
        entries.push(Value::Object(group));
    }
    strip_unwanted_dock_hooks(hooks, &hook_spec_names(specs), hook);
    let backup = existing.map(|bytes| {
        let backup = settings_path.with_file_name(backup_name);
        (backup, bytes)
    });
    if let Some((backup_path, bytes)) = &backup {
        if !backup_path.exists() {
            atomic_write(backup_path, bytes, existing_mode(settings_path, 0o600))?;
        }
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    atomic_write(settings_path, &bytes, existing_mode(settings_path, 0o600))?;
    Ok(backup.map(|(path, _)| path))
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

fn uninstall_claude_settings(hook: &Path) -> Result<(), String> {
    uninstall_claude_settings_at(&claude_settings_path(), hook)
}

fn uninstall_claude_settings_at(settings_path: &Path, hook: &Path) -> Result<(), String> {
    uninstall_nested_hooks_at(settings_path, hook)
}

fn uninstall_nested_hooks_at(settings_path: &Path, hook: &Path) -> Result<(), String> {
    let Ok(bytes) = fs::read(settings_path) else {
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
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    atomic_write(settings_path, &bytes, existing_mode(settings_path, 0o600))
}

fn install_cursor_hooks_at(hooks_path: &Path, hook: &Path) -> Result<Option<PathBuf>, String> {
    let existing = match fs::read(hooks_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let mut document: Value = match existing.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| format!("hooks.json is not valid JSON: {error}"))?,
        None => json!({"version": 1, "hooks": {}}),
    };
    let object = document
        .as_object_mut()
        .ok_or_else(|| "Cursor hooks.json must be a JSON object".to_owned())?;
    if !object.contains_key("version") {
        object.insert("version".to_owned(), json!(1));
    }
    let hooks = object.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| "Cursor hooks must be an object".to_owned())?;
    let command = hook.to_string_lossy();
    for spec in cursor_hook_specs() {
        let event = spec.event.to_owned();
        let entries = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let entries = entries
            .as_array_mut()
            .ok_or_else(|| format!("Cursor hook {event} must be an array"))?;
        entries.retain(|entry| !is_dock_managed_hook(entry, hook));
        let mut entry = serde_json::Map::new();
        entry.insert("command".to_owned(), json!(command.as_ref()));
        entry.insert("timeout".to_owned(), json!(5));
        if let Some(matcher) = spec.matcher {
            entry.insert("matcher".to_owned(), json!(matcher));
        }
        if cursor_unbounded_events().iter().any(|name| name == &event) {
            entry.insert("loop_limit".to_owned(), Value::Null);
        }
        entries.push(Value::Object(entry));
    }
    strip_unwanted_dock_hooks(hooks, &hook_spec_names(cursor_hook_specs()), hook);
    let backup = existing.map(|bytes| {
        let backup = hooks_path.with_file_name("hooks.json.orbcue.bak");
        (backup, bytes)
    });
    if let Some((backup_path, bytes)) = &backup {
        if !backup_path.exists() {
            atomic_write(backup_path, bytes, existing_mode(hooks_path, 0o600))?;
        }
    }
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    atomic_write(hooks_path, &bytes, existing_mode(hooks_path, 0o600))?;
    Ok(backup.map(|(path, _)| path))
}

fn uninstall_cursor_hooks_at(hooks_path: &Path, hook: &Path) -> Result<(), String> {
    uninstall_nested_hooks_at(hooks_path, hook)
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

fn wrapper_path(data_dir: &Path, name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        data_dir.join(format!("{name}.cmd"))
    }
    #[cfg(not(windows))]
    {
        data_dir.join(name)
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
    hooks.retain(|_, value| match value.as_array() {
        Some(entries) => !entries.is_empty(),
        None => true,
    });
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
    &[
        HookSpec {
            event: "SessionStart",
            matcher: None,
        },
        HookSpec {
            event: "UserPromptSubmit",
            matcher: None,
        },
        HookSpec {
            event: "PermissionRequest",
            matcher: None,
        },
        HookSpec {
            event: "PermissionDenied",
            matcher: None,
        },
        HookSpec {
            event: "Notification",
            matcher: None,
        },
        HookSpec {
            event: "Stop",
            matcher: None,
        },
        HookSpec {
            event: "StopFailure",
            matcher: None,
        },
        HookSpec {
            event: "SessionEnd",
            matcher: None,
        },
        HookSpec {
            event: "PreToolUse",
            matcher: Some("AskUserQuestion"),
        },
        HookSpec {
            event: "PostToolUse",
            matcher: None,
        },
        HookSpec {
            event: "PostToolUseFailure",
            matcher: None,
        },
    ]
}

fn grok_hook_specs() -> &'static [HookSpec] {
    &[
        HookSpec {
            event: "SessionStart",
            matcher: None,
        },
        HookSpec {
            event: "UserPromptSubmit",
            matcher: None,
        },
        HookSpec {
            event: "Notification",
            matcher: None,
        },
        HookSpec {
            event: "PermissionDenied",
            matcher: None,
        },
        HookSpec {
            event: "Stop",
            matcher: None,
        },
        HookSpec {
            event: "StopFailure",
            matcher: None,
        },
        HookSpec {
            event: "StopCancelled",
            matcher: None,
        },
        HookSpec {
            event: "SessionEnd",
            matcher: None,
        },
        HookSpec {
            event: "PreToolUse",
            matcher: Some("ask_user_question"),
        },
        HookSpec {
            event: "PostToolUse",
            matcher: None,
        },
        HookSpec {
            event: "PostToolUseFailure",
            matcher: None,
        },
    ]
}

fn codex_hook_specs() -> &'static [HookSpec] {
    &[
        HookSpec {
            event: "SessionStart",
            matcher: None,
        },
        HookSpec {
            event: "UserPromptSubmit",
            matcher: None,
        },
        HookSpec {
            event: "PermissionRequest",
            matcher: None,
        },
        HookSpec {
            event: "Stop",
            matcher: None,
        },
        HookSpec {
            event: "SessionEnd",
            matcher: None,
        },
        HookSpec {
            event: "PreToolUse",
            matcher: Some("AskUserQuestion|ask_user_question"),
        },
        HookSpec {
            event: "PostToolUse",
            matcher: None,
        },
    ]
}

fn cursor_hook_specs() -> &'static [HookSpec] {
    &[
        HookSpec {
            event: "sessionStart",
            matcher: None,
        },
        HookSpec {
            event: "beforeSubmitPrompt",
            matcher: None,
        },
        HookSpec {
            event: "afterAgentResponse",
            matcher: None,
        },
        HookSpec {
            event: "stop",
            matcher: None,
        },
        HookSpec {
            event: "sessionEnd",
            matcher: None,
        },
    ]
}

fn cursor_unbounded_events() -> &'static [&'static str] {
    &["sessionStart", "afterAgentResponse", "stop", "sessionEnd"]
}

fn connection_method_for(name: &str) -> ConnectionMethod {
    match name {
        "claude" => ConnectionMethod::ClaudeHook,
        "grok" => ConnectionMethod::GrokHook,
        "codex" => ConnectionMethod::CodexHook,
        "cursor" => ConnectionMethod::CursorHook,
        _ => ConnectionMethod::Wrapper,
    }
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

fn wrapper_script(name: &str, dock_binary: &Path, original: &Path) -> String {
    #[cfg(windows)]
    {
        let dock = windows_batch_quote(&dock_binary.to_string_lossy());
        let original = windows_batch_quote(&original.to_string_lossy());
        let source = windows_batch_quote(name);
        return format!(
            "@echo off\r\nrem OrbCue generated wrapper; never reads Agent content.\r\nsetlocal\r\nif not defined ORBCUE_TASK_ID set \"ORBCUE_TASK_ID={name}-%RANDOM%\"\r\n{dock} start \"%ORBCUE_TASK_ID%\" --source {source} >nul 2>&1\r\ncall {original} %*\r\nset \"CODE=%ERRORLEVEL%\"\r\nif \"%CODE%\"==\"0\" ({dock} complete \"%ORBCUE_TASK_ID%\" --source {source} >nul 2>&1) else ({dock} fail \"%ORBCUE_TASK_ID%\" --source {source} >nul 2>&1)\r\nexit /b %CODE%\r\n",
            name = name,
            dock = dock,
            original = original,
            source = source,
        );
    }
    #[cfg(not(windows))]
    {
        let dock = shell_quote(&dock_binary.to_string_lossy());
        let original = shell_quote(&original.to_string_lossy());
        let source = shell_quote(name);
        format!(
            "#!/bin/sh\n# OrbCue generated wrapper; never reads Agent content.\nset -u\nTASK_ID=${{ORBCUE_TASK_ID:-{source}-$$-$(date +%s%N)}}\n{dock} start \"$TASK_ID\" --source {source} >/dev/null 2>&1 || true\nCHILD=\"\"\nforward_signal() {{\n  SIGNAL=\"$1\"\n  if [ -n \"${{CHILD:-}}\" ]; then kill -$SIGNAL \"$CHILD\" 2>/dev/null || true; fi\n}}\ntrap 'forward_signal TERM' TERM\ntrap 'forward_signal INT' INT\ntrap 'forward_signal HUP' HUP\ntrap 'forward_signal QUIT' QUIT\n{original} \"$@\" &\nCHILD=$!\nwait \"$CHILD\"\nCODE=$?\ntrap - TERM INT HUP QUIT\nif [ $CODE -eq 0 ]; then {dock} complete \"$TASK_ID\" --source {source} >/dev/null 2>&1 || true; else {dock} fail \"$TASK_ID\" --source {source} >/dev/null 2>&1 || true; fi\nexit $CODE\n",
            source = source,
            dock = dock,
            original = original,
        )
    }
}

fn hook_script(dock_binary: &Path, provider: &str) -> String {
    #[cfg(windows)]
    {
        return format!(
            "@echo off\r\nrem OrbCue generated {provider} hook.\r\n{} hook {provider}\r\nexit /b 0\r\n",
            windows_batch_quote(&dock_binary.to_string_lossy())
        );
    }
    #[cfg(not(windows))]
    {
        format!(
            "#!/bin/sh\n# OrbCue generated {provider} hook.\n# exec so orb's PPID is the agent; liveness reaps that PID.\nexec {} hook {provider}\n",
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
    let command = hook.to_string_lossy().into_owned();
    let mut hooks = serde_json::Map::new();
    for spec in grok_hook_specs() {
        let mut group = serde_json::Map::new();
        if let Some(matcher) = spec.matcher {
            group.insert("matcher".to_owned(), json!(matcher));
        }
        group.insert(
            "hooks".to_owned(),
            json!([{"type":"command","command": command.as_str(), "timeout": 5}]),
        );
        hooks.insert(spec.event.to_owned(), json!([group]));
    }
    let document = json!({
        "name": "orbcue",
        "hooks": hooks
    });
    let bytes = serde_json::to_vec_pretty(&document).map_err(|error| error.to_string())?;
    atomic_write(hooks_path, &bytes, 0o600)
}

fn snippet_for(profile: &Path, data_dir: &Path) -> String {
    if is_powershell_profile(profile) {
        powershell_path_snippet(data_dir)
    } else if is_fish_profile(profile) {
        fish_path_snippet(data_dir)
    } else {
        posix_path_snippet(data_dir)
    }
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

fn wrap_path_block(body: &str) -> String {
    #[cfg(windows)]
    {
        format!("\r\n{PATH_START}\r\n{body}\r\n{PATH_END}\r\n")
    }
    #[cfg(not(windows))]
    {
        format!("\n{PATH_START}\n{body}\n{PATH_END}\n")
    }
}

fn posix_path_snippet(data_dir: &Path) -> String {
    wrap_path_block(&format!(
        "export PATH=\"{}:$PATH\"",
        escape_double_quoted(&data_dir.to_string_lossy())
    ))
}

fn powershell_path_snippet(data_dir: &Path) -> String {
    let separator = if cfg!(windows) { ";" } else { ":" };
    wrap_path_block(&format!(
        "$env:PATH = \"{}{separator}\" + $env:PATH",
        powershell_double_quoted(&data_dir.to_string_lossy())
    ))
}

fn fish_path_snippet(data_dir: &Path) -> String {
    wrap_path_block(&format!(
        "set -gx PATH \"{}\" $PATH",
        escape_double_quoted(&data_dir.to_string_lossy())
    ))
}

fn is_powershell_profile(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("ps1")
        || path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("PowerShell_profile"))
}

fn is_fish_profile(path: &Path) -> bool {
    path.file_name().and_then(|name| name.to_str()) == Some("config.fish")
}

#[cfg(not(windows))]
fn is_zsh_profile(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".zshrc" | ".zprofile" | ".zshenv")
    )
}

#[cfg(windows)]
fn windows_batch_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn powershell_double_quoted(value: &str) -> String {
    value.replace('`', "``").replace('"', "`\"")
}

fn escape_double_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
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

fn now_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let backup = install_claude_settings_at(&settings, &hook)
            .unwrap()
            .unwrap();
        assert_eq!(fs::read(&backup).unwrap(), original);
        let changed = fs::read(&settings).unwrap();
        assert!(String::from_utf8_lossy(&changed).contains("claude-hook.sh"));

        fs::write(&settings, br#"{"custom":true}"#).unwrap();
        let second_backup = install_claude_settings_at(&settings, &hook)
            .unwrap()
            .unwrap();
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
                "dry_run":true,
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
