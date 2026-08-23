//! Revocable, zero-reinstall Agent connections.
//!
//! This crate only discovers commands already present on PATH and writes
//! user-level wrappers/hooks. It never replaces an Agent executable.

mod discover;

pub use discover::{
    agent_origin, choose_discovered, parse_login_path_output, probe_login_path,
    InventorySnapshotCache, ProbeOutput, LOGIN_PATH_END, LOGIN_PATH_START,
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

const PATH_START: &str = "# >>> agent-activity-dock PATH >>>";
const PATH_END: &str = "# <<< agent-activity-dock PATH <<<";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConnectionMethod {
    Wrapper,
    ClaudeHook,
    GrokHook,
}

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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ConnectionFile {
    version: u16,
    agents: std::collections::BTreeMap<String, ConnectionRecord>,
}

pub struct ConnectionManager {
    home: PathBuf,
    grok_home: PathBuf,
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
        let mut manager = Self::new(home, config_home, data_home, dock_binary.into());
        manager.grok_home = grok_home;
        manager
    }

    pub fn new(
        home: PathBuf,
        config_home: PathBuf,
        data_home: PathBuf,
        dock_binary: PathBuf,
    ) -> Self {
        let config_dir = config_home.join("agent-activity-dock");
        let data_dir = data_home.join("agent-activity-dock");
        let grok_home = home.join(".grok");
        Self {
            home,
            grok_home,
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
        discover::discover_agents(path, Some(&self.data_dir), &self.grok_home)
    }

    pub fn records(&self) -> Vec<ConnectionRecord> {
        self.load().agents.into_values().collect()
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
        })
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
                if method == ConnectionMethod::GrokHook {
                    self.install_grok_hook()?;
                }
                return Ok(existing.clone());
            }
        }
        let record = match method {
            ConnectionMethod::Wrapper => {
                let wrapper = self.install_wrapper(name, original)?;
                ConnectionRecord {
                    name: name.to_owned(),
                    original: original.to_owned(),
                    method,
                    wrapper: Some(wrapper),
                    hook_script: None,
                    settings_backup: None,
                    capabilities: vec!["started".into(), "completed".into(), "failed".into()],
                    limitation: "wrapper cannot detect waiting for input".into(),
                    installed_at: now_string(),
                }
            }
            ConnectionMethod::ClaudeHook => {
                let (hook, settings_backup) = self.install_claude_hook()?;
                ConnectionRecord {
                    name: name.to_owned(),
                    original: original.to_owned(),
                    method,
                    wrapper: None,
                    hook_script: Some(hook),
                    settings_backup,
                    capabilities: vec![
                        "started".into(),
                        "waiting".into(),
                        "completed".into(),
                        "failed".into(),
                    ],
                    limitation: "reads Claude structured hook metadata only".into(),
                    installed_at: now_string(),
                }
            }
            ConnectionMethod::GrokHook => {
                let hook = self.install_grok_hook()?;
                ConnectionRecord {
                    name: name.to_owned(),
                    original: original.to_owned(),
                    method,
                    wrapper: None,
                    hook_script: Some(hook),
                    settings_backup: None,
                    capabilities: vec![
                        "started".into(),
                        "waiting".into(),
                        "completed".into(),
                        "failed".into(),
                        "cancelled".into(),
                    ],
                    limitation: "reads Grok Build structured hook metadata only".into(),
                    installed_at: now_string(),
                }
            }
        };
        if let Some(existing) = file.agents.get(name) {
            // The new artifact is installed before this cleanup. Different
            // methods use different paths, so a cleanup failure must not
            // invalidate the newly working connection.
            if existing.method != record.method {
                if let Err(error) = self.remove_artifacts(existing) {
                    eprintln!("Agent Activity Dock could not remove old connection: {error}");
                }
            }
        }
        file.agents.insert(name.to_owned(), record.clone());
        self.save(&file)?;
        Ok(record)
    }

    pub fn disconnect(&self, name: &str) -> Result<bool, String> {
        let mut file = self.load();
        let Some(record) = file.agents.remove(name) else {
            return Ok(false);
        };
        self.remove_artifacts(&record)?;
        self.save(&file)?;
        if !file.agents.values().any(|item| item.wrapper.is_some()) {
            self.remove_path_snippet()?;
            self.remove_empty_data_dir();
        }
        Ok(true)
    }

    fn preview_files(&self, name: &str, method: ConnectionMethod) -> Vec<PreviewFile> {
        match method {
            ConnectionMethod::Wrapper => self.preview_wrapper_files(name),
            ConnectionMethod::ClaudeHook => self.preview_claude_files(),
            ConnectionMethod::GrokHook => self.preview_grok_files(),
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
            if existing.contains(PATH_START) {
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
        let events = claude_hook_events();
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
            let backup = settings.with_file_name("settings.json.agent-activity-dock.bak");
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
        let events = grok_hook_events();
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
            if !existing.contains("Agent Activity Dock generated wrapper") {
                return Err(format!(
                    "refusing to overwrite non-Dock file {}",
                    wrapper.display()
                ));
            }
        }
        let script = wrapper_script(name, &self.dock_binary, original);
        atomic_write(&wrapper, script.as_bytes(), 0o700)?;
        self.ensure_path_snippet()?;
        Ok(wrapper)
    }

    fn install_claude_hook(&self) -> Result<(PathBuf, Option<PathBuf>), String> {
        fs::create_dir_all(&self.config_dir).map_err(|error| error.to_string())?;
        set_mode(&self.config_dir, 0o700)?;
        let hook = hook_path(&self.config_dir, "claude");
        let script = hook_script(&self.dock_binary, "claude");
        atomic_write(&hook, script.as_bytes(), 0o700)?;
        let settings_backup = match install_claude_settings(&hook) {
            Ok(path) => path,
            Err(error) => {
                let _ = fs::remove_file(&hook);
                return Err(format!("cannot update Claude settings: {error}"));
            }
        };
        Ok((hook, settings_backup))
    }

    fn install_grok_hook(&self) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.config_dir).map_err(|error| error.to_string())?;
        set_mode(&self.config_dir, 0o700)?;
        let hook = hook_path(&self.config_dir, "grok");
        let script = hook_script(&self.dock_binary, "grok");
        atomic_write(&hook, script.as_bytes(), 0o700)?;
        if let Err(error) = install_grok_hooks(&self.grok_hooks_file(), &hook) {
            let _ = fs::remove_file(&hook);
            return Err(format!("cannot update Grok hooks: {error}"));
        }
        Ok(hook)
    }

    fn grok_hooks_file(&self) -> PathBuf {
        self.grok_home
            .join("hooks")
            .join("agent-activity-dock.json")
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
                ConnectionMethod::Wrapper => {}
            }
            if hook.exists() {
                fs::remove_file(hook).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    fn ensure_path_snippet(&self) -> Result<(), String> {
        for profile in self.profile_targets() {
            let old = fs::read_to_string(&profile).unwrap_or_default();
            if old.contains(PATH_START) {
                continue;
            }
            let block = snippet_for(&profile, &self.data_dir);
            let mode = existing_mode(&profile, 0o600);
            atomic_write(&profile, format!("{old}{block}").as_bytes(), mode)?;
        }
        Ok(())
    }

    fn remove_path_snippet(&self) -> Result<(), String> {
        for profile in self.profile_candidates() {
            let Ok(old) = fs::read_to_string(&profile) else {
                continue;
            };
            let Some(marker_start) = old.find(PATH_START) else {
                continue;
            };
            let line_start = old[..marker_start]
                .rfind('\n')
                .map(|position| position + 1)
                .unwrap_or(marker_start);
            let Some(marker_end) = old[marker_start..].find(PATH_END) else {
                continue;
            };
            let mut end = marker_start + marker_end + PATH_END.len();
            if old.as_bytes().get(end) == Some(&b'\r') {
                end += 1;
            }
            if old.as_bytes().get(end) == Some(&b'\n') {
                end += 1;
            }
            let mut cleaned = old;
            cleaned.replace_range(line_start..end, "");
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
    let existing = match fs::read(&settings_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.to_string()),
    };
    let mut settings: Value = match existing.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes)
            .map_err(|error| format!("settings.json is not valid JSON: {error}"))?,
        None => json!({}),
    };
    let hooks = settings
        .as_object_mut()
        .ok_or_else(|| "Claude settings must be a JSON object".to_owned())?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    let hooks = hooks
        .as_object_mut()
        .ok_or_else(|| "Claude hooks must be an object".to_owned())?;
    for event in claude_hook_events() {
        let entries = hooks.entry(event.clone()).or_insert_with(|| json!([]));
        let entries = entries
            .as_array_mut()
            .ok_or_else(|| format!("Claude hook {event} must be an array"))?;
        entries.retain(|entry| {
            !entry
                .to_string()
                .contains(&hook.to_string_lossy().to_string())
        });
        entries.push(json!({"hooks":[{"type":"command","command":hook.to_string_lossy()}]}));
    }
    let backup = existing.map(|bytes| {
        let backup = settings_path.with_file_name("settings.json.agent-activity-dock.bak");
        (backup, bytes)
    });
    if let Some((backup_path, bytes)) = &backup {
        if !backup_path.exists() {
            atomic_write(backup_path, bytes, existing_mode(&settings_path, 0o600))?;
        }
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    atomic_write(&settings_path, &bytes, existing_mode(&settings_path, 0o600))?;
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
    let Ok(bytes) = fs::read(&settings_path) else {
        return Ok(());
    };
    let Ok(mut settings) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(());
    };
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for entries in hooks.values_mut() {
            if let Some(entries) = entries.as_array_mut() {
                entries.retain(|entry| {
                    !entry
                        .to_string()
                        .contains(&hook.to_string_lossy().to_string())
                });
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    atomic_write(&settings_path, &bytes, existing_mode(&settings_path, 0o600))
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
        ConnectionMethod::ClaudeHook => vec!["首次修改前备份 settings.json".to_owned()],
        ConnectionMethod::Wrapper | ConnectionMethod::GrokHook => Vec::new(),
    }
}

fn claude_hook_events() -> Vec<String> {
    [
        "SessionStart",
        "PreToolUse",
        "PermissionRequest",
        "SessionEnd",
        "StopFailure",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn grok_hook_events() -> Vec<String> {
    [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "Notification",
        "Stop",
        "StopFailure",
        "StopCancelled",
        "SessionEnd",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn connection_method_for(name: &str) -> ConnectionMethod {
    match name {
        "claude" => ConnectionMethod::ClaudeHook,
        "grok" => ConnectionMethod::GrokHook,
        _ => ConnectionMethod::Wrapper,
    }
}

pub(crate) fn grok_binary_in_home(grok_home: &Path) -> Option<PathBuf> {
    candidate_names("grok")
        .into_iter()
        .map(|name| grok_home.join("bin").join(name))
        .find(|path| path.is_file())
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
            "@echo off\r\nrem Agent Activity Dock generated wrapper; never reads Agent content.\r\nsetlocal\r\nif not defined AGENT_ACTIVITY_DOCK_TASK_ID set \"AGENT_ACTIVITY_DOCK_TASK_ID={name}-%RANDOM%\"\r\n{dock} start \"%AGENT_ACTIVITY_DOCK_TASK_ID%\" --source {source} >nul 2>&1\r\ncall {original} %*\r\nset \"CODE=%ERRORLEVEL%\"\r\nif \"%CODE%\"==\"0\" ({dock} complete \"%AGENT_ACTIVITY_DOCK_TASK_ID%\" --source {source} >nul 2>&1) else ({dock} fail \"%AGENT_ACTIVITY_DOCK_TASK_ID%\" --source {source} >nul 2>&1)\r\nexit /b %CODE%\r\n",
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
            "#!/bin/sh\n# Agent Activity Dock generated wrapper; never reads Agent content.\nset -u\nTASK_ID=${{AGENT_ACTIVITY_DOCK_TASK_ID:-{source}-$$-$(date +%s%N)}}\n{dock} start \"$TASK_ID\" --source {source} >/dev/null 2>&1 || true\nCHILD=\"\"\nforward_signal() {{\n  SIGNAL=\"$1\"\n  if [ -n \"${{CHILD:-}}\" ]; then kill -$SIGNAL \"$CHILD\" 2>/dev/null || true; fi\n}}\ntrap 'forward_signal TERM' TERM\ntrap 'forward_signal INT' INT\ntrap 'forward_signal HUP' HUP\ntrap 'forward_signal QUIT' QUIT\n{original} \"$@\" &\nCHILD=$!\nwait \"$CHILD\"\nCODE=$?\ntrap - TERM INT HUP QUIT\nif [ $CODE -eq 0 ]; then {dock} complete \"$TASK_ID\" --source {source} >/dev/null 2>&1 || true; else {dock} fail \"$TASK_ID\" --source {source} >/dev/null 2>&1 || true; fi\nexit $CODE\n",
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
            "@echo off\r\nrem Agent Activity Dock generated {provider} hook.\r\n{} hook {provider}\r\nexit /b 0\r\n",
            windows_batch_quote(&dock_binary.to_string_lossy())
        );
    }
    #[cfg(not(windows))]
    {
        format!(
            "#!/bin/sh\n# Agent Activity Dock generated {provider} hook.\n{} hook {provider} || true\n",
            shell_quote(&dock_binary.to_string_lossy())
        )
    }
}

fn install_grok_hooks(hooks_path: &Path, hook: &Path) -> Result<(), String> {
    if let Ok(existing) = fs::read_to_string(hooks_path) {
        if !existing.contains("agent-activity-dock") && !existing.contains("hook grok") {
            return Err(format!(
                "refusing to overwrite non-Dock file {}",
                hooks_path.display()
            ));
        }
    }
    let command = hook.to_string_lossy().into_owned();
    let mut hooks = serde_json::Map::new();
    for event in grok_hook_events() {
        hooks.insert(
            event,
            json!([{"hooks":[{"type":"command","command": command.as_str(), "timeout": 5}]}]),
        );
    }
    let document = json!({
        "name": "agent-activity-dock",
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
        let root = std::env::temp_dir().join(format!("aadock-settings-{nonce}"));
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
        assert!(!settings
            .with_file_name("settings.json.agent-activity-dock.bak")
            .exists());
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
}
