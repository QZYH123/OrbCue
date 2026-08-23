//! Versioned local wire protocol shared by the desktop process and emitters.

use agent_activity_dock_core::{
    ApplyResult, Attention, AuditEntry, DockEvent, DockSnapshot, SessionSnapshot,
};
use interprocess::local_socket::{
    prelude::*, GenericFilePath, ListenerOptions, Stream as InterprocessStream,
};
use interprocess::TryClone;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::process::Command;
use std::time::Duration;
use thiserror::Error;

pub const MAX_FRAME_BYTES: usize = 16 * 1024;
pub const SOCKET_FILE_NAME: &str = "agent-activity-dock.sock";

pub type LocalListener = interprocess::local_socket::Listener;
pub type LocalStream = InterprocessStream;

pub fn local_listener(endpoint: &Path) -> io::Result<LocalListener> {
    let name = endpoint.to_path_buf().to_fs_name::<GenericFilePath>()?;
    ListenerOptions::new().name(name).create_sync()
}

pub fn local_accept(listener: &LocalListener) -> io::Result<LocalStream> {
    listener.accept()
}

pub fn local_connect(endpoint: &Path) -> io::Result<LocalStream> {
    let name = endpoint.to_path_buf().to_fs_name::<GenericFilePath>()?;
    LocalStream::connect(name)
}

pub fn local_set_recv_timeout(stream: &LocalStream, timeout: Option<Duration>) -> io::Result<()> {
    ignore_unsupported_timeout(stream.set_recv_timeout(timeout))
}

pub fn local_set_send_timeout(stream: &LocalStream, timeout: Option<Duration>) -> io::Result<()> {
    ignore_unsupported_timeout(stream.set_send_timeout(timeout))
}

fn ignore_unsupported_timeout(result: io::Result<()>) -> io::Result<()> {
    match result {
        // Windows named pipes reject SO_RCVTIMEO; treating that as failure
        // made a live daemon look missing.
        Err(error) if error.kind() == io::ErrorKind::Unsupported => Ok(()),
        other => other,
    }
}

pub fn local_try_clone(stream: &LocalStream) -> io::Result<LocalStream> {
    stream.try_clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcRequest {
    Event(DockEvent),
    Snapshot,
    Subscribe,
    Acknowledge { source: String, session_id: String },
    Reset { source: String, session_id: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("message_too_large")]
    MessageTooLarge,
    #[error("invalid_json")]
    InvalidJson,
    #[error("invalid_request")]
    InvalidRequest,
    #[error("unknown_query")]
    UnknownQuery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotView {
    pub working_count: usize,
    pub tracked_count: usize,
    pub pending_count: usize,
    pub pending_mark: String,
    pub count_label: String,
    pub border_state: String,
    pub sessions: Vec<SessionSnapshot>,
    pub audit: Vec<AuditEntry>,
}

impl From<&DockSnapshot> for SnapshotView {
    fn from(snapshot: &DockSnapshot) -> Self {
        Self {
            working_count: snapshot.working_count,
            tracked_count: snapshot.tracked_count,
            pending_count: snapshot.pending_count,
            pending_mark: snapshot.pending_mark.clone(),
            count_label: snapshot.count_label(),
            border_state: if snapshot.is_working() {
                "working".to_owned()
            } else {
                "idle".to_owned()
            },
            sessions: snapshot.sessions.clone(),
            audit: snapshot.audit.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireResponse {
    pub ok: bool,
    pub accepted: bool,
    pub rejection_reason: Option<String>,
    pub attention: Option<Attention>,
    pub snapshot: SnapshotView,
}

impl WireResponse {
    pub fn from_apply(result: &ApplyResult) -> Self {
        Self {
            ok: result.accepted,
            accepted: result.accepted,
            rejection_reason: result.rejection_reason.clone(),
            attention: result.attention.clone(),
            snapshot: SnapshotView::from(&result.snapshot),
        }
    }

    pub fn accepted(snapshot: &DockSnapshot) -> Self {
        Self {
            ok: true,
            accepted: true,
            rejection_reason: None,
            attention: None,
            snapshot: SnapshotView::from(snapshot),
        }
    }

    pub fn rejected(snapshot: &DockSnapshot, reason: &str) -> Self {
        Self {
            ok: false,
            accepted: false,
            rejection_reason: Some(reason.to_owned()),
            attention: None,
            snapshot: SnapshotView::from(snapshot),
        }
    }
}

pub fn parse_request(frame: &[u8]) -> Result<IpcRequest, FrameError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(FrameError::MessageTooLarge);
    }
    let frame = frame.strip_suffix(b"\n").unwrap_or(frame);
    if frame.is_empty() {
        return Err(FrameError::InvalidRequest);
    }
    let value: Value = serde_json::from_slice(frame).map_err(|_| FrameError::InvalidJson)?;
    if let Some(query) = value.get("query").and_then(Value::as_str) {
        return match query {
            "snapshot" => Ok(IpcRequest::Snapshot),
            "subscribe" => Ok(IpcRequest::Subscribe),
            "acknowledge" => {
                let source = value
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("*")
                    .to_owned();
                let session_id = value
                    .get("session_id")
                    .or_else(|| value.get("task_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("*")
                    .to_owned();
                if source.is_empty() || session_id.is_empty() {
                    Err(FrameError::InvalidRequest)
                } else {
                    Ok(IpcRequest::Acknowledge { source, session_id })
                }
            }
            "reset" => {
                let source = value
                    .get("source")
                    .and_then(Value::as_str)
                    .unwrap_or("*")
                    .to_owned();
                let session_id = value
                    .get("session_id")
                    .or_else(|| value.get("task_id"))
                    .and_then(Value::as_str)
                    .unwrap_or("*")
                    .to_owned();
                if source.is_empty() || session_id.is_empty() {
                    Err(FrameError::InvalidRequest)
                } else {
                    Ok(IpcRequest::Reset { source, session_id })
                }
            }
            _ => Err(FrameError::UnknownQuery),
        };
    }
    serde_json::from_value(value)
        .map(IpcRequest::Event)
        .map_err(|_| FrameError::InvalidRequest)
}

pub fn encode_line<T: Serialize>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn encode_request(request: &IpcRequest) -> Result<Vec<u8>, serde_json::Error> {
    match request {
        IpcRequest::Event(event) => encode_line(event),
        IpcRequest::Snapshot => Ok(b"{\"query\":\"snapshot\"}\n".to_vec()),
        IpcRequest::Subscribe => Ok(b"{\"query\":\"subscribe\"}\n".to_vec()),
        IpcRequest::Acknowledge { source, session_id } => encode_line(&serde_json::json!({
            "query": "acknowledge",
            "source": source,
            "session_id": session_id
        })),
        IpcRequest::Reset { source, session_id } => encode_line(&serde_json::json!({
            "query": "reset",
            "source": source,
            "session_id": session_id
        })),
    }
}

pub fn default_endpoint() -> PathBuf {
    if let Some(endpoint) =
        env::var_os("AGENT_ACTIVITY_DOCK_SOCKET").filter(|value| !value.is_empty())
    {
        return PathBuf::from(endpoint);
    }
    #[cfg(windows)]
    {
        return PathBuf::from(r"\\.\pipe\agent-activity-dock");
    }
    #[cfg(not(windows))]
    if let Some(runtime) = env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) {
        return PathBuf::from(runtime)
            .join("agent-activity-dock")
            .join(SOCKET_FILE_NAME);
    }
    #[cfg(not(windows))]
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(not(windows))]
    home.join(".local")
        .join("state")
        .join("agent-activity-dock")
        .join(SOCKET_FILE_NAME)
}

pub fn default_state_path() -> PathBuf {
    #[cfg(windows)]
    {
        let local_app_data = env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("USERPROFILE")
                    .map(|home| PathBuf::from(home).join("AppData").join("Local"))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        return local_app_data
            .join("Agent Activity Dock")
            .join("state.json");
    }
    #[cfg(not(windows))]
    let state_home = env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local").join("state"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(not(windows))]
    state_home.join("agent-activity-dock").join("state.json")
}

/// Canonical daemon topology. Compile default is GUI-OS local listen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockBackend {
    Wsl,
    Local,
}

impl DockBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wsl => "wsl",
            Self::Local => "local",
        }
    }
}

pub fn default_backend_for_build() -> DockBackend {
    DockBackend::Local
}

pub fn parse_backend(value: &str) -> Option<DockBackend> {
    match value.trim() {
        value if value.eq_ignore_ascii_case("local") => Some(DockBackend::Local),
        value if value.eq_ignore_ascii_case("wsl") => Some(DockBackend::Wsl),
        _ => None,
    }
}

pub fn parse_backend_file(contents: &str) -> Option<DockBackend> {
    parse_backend(contents.lines().next().unwrap_or(""))
}

fn env_nonempty(key: &str) -> bool {
    env::var(key)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn should_read_windows_backend_file() -> bool {
    cfg!(windows)
        || env_nonempty("WSL_DISTRO_NAME")
        || env_nonempty("WSL_INTEROP")
        || env_nonempty("AGENT_ACTIVITY_DOCK_WINDOWS_DOCK")
}

pub fn windows_app_data_dir() -> Option<PathBuf> {
    if let Some(local) = env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(local));
    }
    #[cfg(windows)]
    {
        return env::var_os("USERPROFILE")
            .map(|home| PathBuf::from(home).join("AppData").join("Local"));
    }
    #[cfg(not(windows))]
    {
        if let Ok(output) = Command::new("cmd.exe")
            .args(["/c", "echo", "%LOCALAPPDATA%"])
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let windows_path = text.lines().next().unwrap_or("").trim();
                if !windows_path.is_empty() && windows_path != "%LOCALAPPDATA%" {
                    if let Some(unix) = wslpath_unix(windows_path) {
                        return Some(unix);
                    }
                }
            }
        }
        let user = env::var("USER")
            .ok()
            .or_else(|| env::var("USERNAME").ok())?;
        Some(PathBuf::from(format!("/mnt/c/Users/{user}/AppData/Local")))
    }
}

#[cfg(not(windows))]
fn wslpath_unix(windows_path: &str) -> Option<PathBuf> {
    let output = Command::new("wslpath")
        .args(["-u", windows_path])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| PathBuf::from(trimmed))
}

pub fn backend_file_path() -> Option<PathBuf> {
    windows_app_data_dir().map(|dir| dir.join("Agent Activity Dock").join("backend"))
}

pub fn resolve_backend_from_env() -> DockBackend {
    env::var("AGENT_ACTIVITY_DOCK_BACKEND")
        .ok()
        .as_deref()
        .and_then(parse_backend)
        .unwrap_or_else(default_backend_for_build)
}

pub fn resolve_backend() -> DockBackend {
    if let Some(value) = env::var("AGENT_ACTIVITY_DOCK_BACKEND")
        .ok()
        .as_deref()
        .and_then(parse_backend)
    {
        return value;
    }
    if should_read_windows_backend_file() {
        if let Some(backend) = backend_file_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .as_deref()
            .and_then(parse_backend_file)
        {
            return backend;
        }
    } else if !cfg!(windows) {
        return DockBackend::Local;
    }
    default_backend_for_build()
}

pub fn persist_default_backend_file() {
    if env::var("AGENT_ACTIVITY_DOCK_BACKEND")
        .ok()
        .as_deref()
        .and_then(parse_backend)
        .is_some()
    {
        return;
    }
    if default_backend_for_build() != DockBackend::Local {
        return;
    }
    #[cfg(windows)]
    {
        let Some(path) = backend_file_path() else {
            return;
        };
        if path.exists() {
            return;
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, "local\n");
    }
}

#[cfg(test)]
mod tests {
    use super::{default_backend_for_build, parse_backend, parse_backend_file, DockBackend};

    #[test]
    fn backend_env_parses_case_insensitively() {
        assert_eq!(parse_backend("local"), Some(DockBackend::Local));
        assert_eq!(parse_backend("LOCAL"), Some(DockBackend::Local));
        assert_eq!(parse_backend(" wsl "), Some(DockBackend::Wsl));
        assert_eq!(parse_backend("WSL"), Some(DockBackend::Wsl));
        assert_eq!(parse_backend(""), None);
        assert_eq!(parse_backend("probe"), None);
        assert_eq!(default_backend_for_build(), DockBackend::Local);
    }

    #[test]
    fn backend_file_uses_the_first_line() {
        assert_eq!(parse_backend_file("local\n"), Some(DockBackend::Local));
        assert_eq!(
            parse_backend_file("wsl\n{\"note\":\"ignored\"}\n"),
            Some(DockBackend::Wsl)
        );
        assert_eq!(parse_backend_file("# comment"), None);
    }
}
