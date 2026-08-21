//! PATH discovery for Agents. Login-shell PATH is probed once; tests inject PATH.

use crate::{AgentOrigin, ConnectionRecord, DiscoveredAgent};
use std::env;
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

pub const LOGIN_PATH_START: &str = "__AADOCK_PATH_START__";
pub const LOGIN_PATH_END: &str = "__AADOCK_PATH_END__";
/// Quoted `%s\n` so bash printf emits real newlines. Unquoted `%s\n` prints a
/// literal `n` and glues the markers to PATH.
pub const LOGIN_PATH_SCRIPT: &str =
    r#"printf '%s\n' '__AADOCK_PATH_START__' "$PATH" '__AADOCK_PATH_END__'"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutput {
    pub status_success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Default, Clone)]
pub struct InventorySnapshotCache {
    latest: Option<(Vec<DiscoveredAgent>, Vec<ConnectionRecord>)>,
}

impl InventorySnapshotCache {
    pub fn get(&self) -> Option<(Vec<DiscoveredAgent>, Vec<ConnectionRecord>)> {
        self.latest.clone()
    }

    pub fn set(&mut self, discovered: Vec<DiscoveredAgent>, connected: Vec<ConnectionRecord>) {
        self.latest = Some((discovered, connected));
    }
}

pub fn parse_login_path_output(stdout: &str) -> Option<String> {
    let mut lines = stdout.lines();
    while let Some(line) = lines.next() {
        if line.trim() != LOGIN_PATH_START {
            continue;
        }
        let path = lines.next()?.trim();
        let end = lines.next()?.trim();
        if end == LOGIN_PATH_END && !path.is_empty() {
            return Some(path.to_owned());
        }
        return None;
    }
    None
}

pub fn probe_login_path(
    run: impl FnOnce() -> Result<ProbeOutput, String>,
) -> Result<String, String> {
    let output = run()?;
    if !output.status_success {
        return Err(format!(
            "login shell PATH probe exited unsuccessfully{}",
            diagnostic_suffix(&output.stderr)
        ));
    }
    parse_login_path_output(&output.stdout)
        .ok_or_else(|| "login shell PATH probe did not emit a marked PATH".to_owned())
}

pub fn cached_login_path() -> Result<String, String> {
    static LOGIN_PATH: OnceLock<Result<String, String>> = OnceLock::new();
    LOGIN_PATH
        .get_or_init(|| {
            let result = probe_login_path(|| run_login_path_command(Duration::from_secs(2)));
            if let Err(diagnostic) = &result {
                eprintln!("Agent Activity Dock: {diagnostic}; falling back to process PATH");
            }
            result
        })
        .clone()
}

pub fn discovery_path() -> OsString {
    if cfg!(windows) {
        return env::var_os("PATH").unwrap_or_default();
    }
    match cached_login_path() {
        Ok(path) => OsString::from(path),
        Err(_) => env::var_os("PATH").unwrap_or_default(),
    }
}

pub fn agent_origin(path: &Path) -> AgentOrigin {
    if looks_like_windows_binary(path) {
        return AgentOrigin::Windows;
    }
    if let Ok(resolved) = path.canonicalize() {
        if resolved != path && looks_like_windows_binary(&resolved) {
            return AgentOrigin::Windows;
        }
    }
    AgentOrigin::Wsl
}

fn looks_like_windows_binary(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    if is_windows_interop_path(&text) || is_windows_drive_path(&text) {
        return true;
    }
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
}

pub fn choose_discovered(name: &str, candidates: Vec<PathBuf>) -> Option<DiscoveredAgent> {
    select_agent(name, candidates)
}

pub fn discover_agents(
    path: &OsStr,
    excluded_dir: Option<&Path>,
    grok_home: &Path,
) -> Vec<DiscoveredAgent> {
    let mut agents: Vec<DiscoveredAgent> = ["claude", "codex", "dsh", "grok"]
        .into_iter()
        .filter_map(|name| choose_discovered(name, find_all_on_path(name, path, excluded_dir)))
        .collect();
    if !agents.iter().any(|agent| agent.name == "grok") {
        if let Some(path) = crate::grok_binary_in_home(grok_home) {
            agents.push(discovered_agent("grok", path));
        }
    }
    agents
}

fn select_agent(name: &str, candidates: Vec<PathBuf>) -> Option<DiscoveredAgent> {
    let path = candidates
        .iter()
        .find(|path| agent_origin(path) == AgentOrigin::Wsl)
        .or_else(|| candidates.first())
        .cloned()?;
    Some(discovered_agent(name, path))
}

fn discovered_agent(name: &str, path: PathBuf) -> DiscoveredAgent {
    let origin = agent_origin(&path);
    DiscoveredAgent {
        name: name.to_owned(),
        path,
        origin,
        connectable: origin == AgentOrigin::Wsl,
    }
}

fn find_all_on_path(name: &str, path: &OsStr, excluded_dir: Option<&Path>) -> Vec<PathBuf> {
    env::split_paths(path)
        .flat_map(|dir| {
            crate::candidate_names(name)
                .into_iter()
                .map(move |candidate| dir.join(candidate))
        })
        .filter(|candidate| {
            excluded_dir
                .map(|excluded| candidate.parent() != Some(excluded))
                .unwrap_or(true)
        })
        .filter(|candidate| {
            let text = candidate.to_string_lossy().replace('\\', "/");
            !is_windows_interop_path(&text)
        })
        .filter(|candidate| candidate.is_file())
        .collect()
}

fn is_windows_interop_path(text: &str) -> bool {
    if text.starts_with("/mnt/") {
        return true;
    }
    let parts: Vec<&str> = text.split('/').filter(|part| !part.is_empty()).collect();
    parts.windows(2).any(|pair| {
        pair[0] == "mnt"
            && pair[1].len() == 1
            && pair[1]
                .chars()
                .all(|character| character.is_ascii_alphabetic())
    })
}

fn is_windows_drive_path(text: &str) -> bool {
    let stripped = text.strip_prefix("//?/").unwrap_or(text);
    let mut chars = stripped.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(drive), Some(':')) if drive.is_ascii_alphabetic()
    )
}

fn run_login_path_command(timeout: Duration) -> Result<ProbeOutput, String> {
    let shell = env::var("SHELL")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/bin/bash".to_owned());
    let child = Command::new(&shell)
        .arg("-lc")
        .arg(LOGIN_PATH_SCRIPT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start login shell ({error})"))?;
    wait_output(child, timeout)
}

fn wait_output(mut child: Child, timeout: Duration) -> Result<ProbeOutput, String> {
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "login shell stdout is unavailable".to_owned())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "login shell stderr is unavailable".to_owned())?;
    let stdout_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let stderr_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("login shell PATH probe timed out".to_owned());
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(error.to_string()),
        }
    };
    Ok(ProbeOutput {
        status_success: status.success(),
        stdout: String::from_utf8_lossy(&stdout_thread.join().unwrap_or_default()).into_owned(),
        stderr: String::from_utf8_lossy(&stderr_thread.join().unwrap_or_default()).into_owned(),
    })
}

fn diagnostic_suffix(stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_origin, parse_login_path_output, probe_login_path, InventorySnapshotCache,
        ProbeOutput, LOGIN_PATH_END, LOGIN_PATH_SCRIPT, LOGIN_PATH_START,
    };
    use crate::{AgentOrigin, ConnectionRecord, DiscoveredAgent};
    use std::path::{Path, PathBuf};

    #[test]
    fn login_path_parser_skips_shell_banners() {
        let stdout = format!(
            "Welcome to oh-my-zsh\n[oh-my-zsh] would you like to update?\n{LOGIN_PATH_START}\n/home/u/.local/bin:/usr/bin\n{LOGIN_PATH_END}\n"
        );
        assert_eq!(
            parse_login_path_output(&stdout).as_deref(),
            Some("/home/u/.local/bin:/usr/bin")
        );
    }

    #[test]
    fn login_path_parser_rejects_unmarked_stdout() {
        assert!(parse_login_path_output("Welcome\n/usr/bin:/bin\n").is_none());
        assert!(parse_login_path_output("").is_none());
    }

    #[test]
    fn probe_login_path_uses_injected_runner() {
        let path = probe_login_path(|| {
            Ok(ProbeOutput {
                status_success: true,
                stdout: format!("{LOGIN_PATH_START}\n/opt/nvm:/bin\n{LOGIN_PATH_END}\n"),
                stderr: String::new(),
            })
        })
        .unwrap();
        assert_eq!(path, "/opt/nvm:/bin");
    }

    #[test]
    fn probe_login_path_reports_timeout_without_touching_state() {
        let error =
            probe_login_path(|| Err("login shell PATH probe timed out".to_owned())).unwrap_err();
        assert!(error.contains("timed out"));
    }

    #[test]
    fn discover_agents_skips_mnt_windows_interop_paths() {
        let root = std::env::temp_dir().join(format!(
            "aadock-mnt-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos()
        ));
        let local_bin = root.join("home").join(".local").join("bin");
        let windows_bin = root
            .join("mnt")
            .join("c")
            .join("Users")
            .join("u")
            .join("AppData")
            .join("Roaming")
            .join("npm");
        std::fs::create_dir_all(&local_bin).unwrap();
        std::fs::create_dir_all(&windows_bin).unwrap();
        std::fs::write(local_bin.join("claude"), b"").unwrap();
        std::fs::write(windows_bin.join("codex"), b"").unwrap();
        let path = std::env::join_paths([&windows_bin, &local_bin]).unwrap();
        let discovered = super::discover_agents(&path, None, &root.join("missing-grok"));
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].name, "claude");
        assert_eq!(discovered[0].path, local_bin.join("claude"));
        std::fs::remove_file(local_bin.join("claude")).unwrap();
        let windows_only = super::discover_agents(&path, None, &root.join("missing-grok"));
        assert!(
            windows_only.is_empty(),
            "interop /mnt/* agents must be left to Windows discovery: {windows_only:?}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discover_agents_finds_windows_pathext_executables() {
        let root = std::env::temp_dir().join(format!(
            "aadock-pathext-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock is after epoch")
                .as_nanos()
        ));
        let bin = root.join("win-bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("claude.exe"), b"").unwrap();
        std::fs::write(bin.join("codex.cmd"), b"").unwrap();
        std::fs::write(bin.join("dsh.bat"), b"").unwrap();
        std::fs::write(bin.join("grok.ps1"), b"").unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        let discovered = super::discover_agents(&path, None, &root.join("missing-grok"));
        let mut names: Vec<_> = discovered.iter().map(|agent| agent.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["claude", "codex", "dsh", "grok"]);
        assert!(discovered.iter().any(|agent| agent
            .path
            .file_name()
            .and_then(|name| name.to_str())
            == Some("claude.exe")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn choose_discovered_prefers_wsl_and_keeps_windows_only() {
        let wsl = PathBuf::from("/home/u/.local/bin/claude");
        let windows = PathBuf::from("/mnt/c/Users/u/AppData/Roaming/npm/claude");
        let preferred =
            super::choose_discovered("claude", vec![windows.clone(), wsl.clone()]).unwrap();
        assert_eq!(preferred.path, wsl);
        assert_eq!(preferred.origin, AgentOrigin::Wsl);
        assert!(preferred.connectable);

        let only = super::choose_discovered("claude", vec![windows.clone()]).unwrap();
        assert_eq!(only.path, windows);
        assert_eq!(only.origin, AgentOrigin::Windows);
        assert!(!only.connectable);
    }

    #[test]
    fn windows_interop_paths_are_not_connectable() {
        assert_eq!(
            agent_origin(Path::new("/mnt/c/Users/u/AppData/Roaming/npm/claude")),
            AgentOrigin::Windows
        );
        assert_eq!(
            agent_origin(Path::new(r"C:\Users\u\AppData\Roaming\npm\claude.exe")),
            AgentOrigin::Windows
        );
        assert_eq!(
            agent_origin(Path::new(
                "/usr/local/lib/node_modules/@anthropic-ai/claude-code/bin/claude.exe"
            )),
            AgentOrigin::Windows
        );
        assert_eq!(
            agent_origin(Path::new("/home/u/.local/bin/claude")),
            AgentOrigin::Wsl
        );
    }

    #[test]
    fn login_path_script_prints_three_marked_lines() {
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(LOGIN_PATH_SCRIPT)
            .env("PATH", "/home/u/.local/bin:/usr/bin")
            .output()
            .expect("bash -c runs");
        assert!(output.status.success(), "{:?}", output);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            parse_login_path_output(&stdout).as_deref(),
            Some("/home/u/.local/bin:/usr/bin")
        );
    }

    #[test]
    fn missing_origin_fields_default_to_wsl_and_connectable() {
        let agent: DiscoveredAgent =
            serde_json::from_str(r#"{"name":"claude","path":"/usr/bin/claude"}"#).unwrap();
        assert_eq!(agent.origin, AgentOrigin::Wsl);
        assert!(agent.connectable);
    }

    #[test]
    fn cache_hit_returns_previous_inventory() {
        let mut cache = InventorySnapshotCache::default();
        assert!(cache.get().is_none());
        let discovered = vec![DiscoveredAgent {
            name: "claude".to_owned(),
            path: "/home/u/.local/bin/claude".into(),
            origin: AgentOrigin::Wsl,
            connectable: true,
        }];
        cache.set(discovered.clone(), Vec::<ConnectionRecord>::new());
        let hit = cache.get().expect("cached inventory");
        assert_eq!(hit.0, discovered);
        assert!(hit.1.is_empty());
    }
}
