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
use std::io;
use std::path::{Path, PathBuf};
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
    stream.set_recv_timeout(timeout)
}

pub fn local_set_send_timeout(stream: &LocalStream, timeout: Option<Duration>) -> io::Result<()> {
    stream.set_send_timeout(timeout)
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
