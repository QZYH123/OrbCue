//! Event-driven local service used by the Tauri process and headless modes.

mod client;

pub use client::{
    attach_or_listen, connect_or_spawn_detached, query_service, DetachedConnectError, DockSession,
};

use agent_activity_dock_core::{DockState, PersistedState};
use agent_activity_dock_ipc::{
    encode_line, local_accept, local_connect, local_listener, parse_request, IpcRequest,
    LocalStream, SnapshotView, WireResponse, MAX_FRAME_BYTES,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("cannot prepare endpoint: {0}")]
    Endpoint(#[from] std::io::Error),
    #[error("another Dock service is already listening on {0}")]
    AlreadyRunning(PathBuf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMessage {
    #[serde(rename = "type")]
    pub message_type: String,
    pub snapshot: SnapshotView,
    pub attention: Option<agent_activity_dock_core::Attention>,
}

impl SnapshotMessage {
    pub fn snapshot(
        snapshot: SnapshotView,
        attention: Option<agent_activity_dock_core::Attention>,
    ) -> Self {
        Self {
            message_type: "snapshot".to_owned(),
            snapshot,
            attention,
        }
    }

    pub fn subscribed(snapshot: SnapshotView) -> Self {
        Self {
            message_type: "subscribed".to_owned(),
            snapshot,
            attention: None,
        }
    }
}

#[derive(Debug)]
pub struct ServiceHandle {
    endpoint: PathBuf,
    state_path: Option<PathBuf>,
    stopping: Arc<AtomicBool>,
    state: Arc<Mutex<DockState>>,
    updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl ServiceHandle {
    pub fn endpoint(&self) -> &Path {
        &self.endpoint
    }

    pub fn subscribe_updates(&self) -> mpsc::Receiver<SnapshotMessage> {
        let (sender, receiver) = mpsc::channel();
        self.updates
            .lock()
            .expect("update subscribers lock")
            .push(sender);
        receiver
    }

    pub fn snapshot(&self) -> agent_activity_dock_core::DockSnapshot {
        self.state.lock().expect("state lock").snapshot()
    }

    pub fn acknowledge(&self, source: &str, session_id: &str) -> WireResponse {
        let mut state = self.state.lock().expect("state lock");
        let snapshot = state.acknowledge(source, session_id);
        if let Some(path) = &self.state_path {
            if let Err(error) = persist_state(path, &state) {
                eprintln!("Agent Activity Dock could not persist acknowledgement: {error}");
            }
        }
        drop(state);
        broadcast(
            &self.updates,
            SnapshotMessage::snapshot(SnapshotView::from(&snapshot), None),
        );
        WireResponse::accepted(&snapshot)
    }

    pub fn reset(&self, source: &str, session_id: &str) -> WireResponse {
        let mut state = self.state.lock().expect("state lock");
        let snapshot = state.reset(source, session_id);
        if let Some(path) = &self.state_path {
            if let Err(error) = persist_state(path, &state) {
                eprintln!("Agent Activity Dock could not persist reset: {error}");
            }
        }
        drop(state);
        broadcast(
            &self.updates,
            SnapshotMessage::snapshot(SnapshotView::from(&snapshot), None),
        );
        WireResponse::accepted(&snapshot)
    }

    pub fn request_shutdown(&self) {
        self.stopping.store(true, Ordering::Release);
        self.updates
            .lock()
            .expect("update subscribers lock")
            .clear();
        // Wake the blocking accept() without introducing a polling timer.
        let _ = local_connect(&self.endpoint);
    }

    /// Wait for the accept loop to remove the endpoint after requesting a
    /// shutdown. This is useful for owners held behind `Arc` (for example the
    /// Tauri application state).
    pub fn wait_for_shutdown(&self) {
        if let Some(join) = self.join.lock().expect("service join lock").take() {
            let _ = join.join();
        }
    }

    pub fn shutdown(self) {
        self.request_shutdown();
        self.wait_for_shutdown();
    }
}

pub fn spawn(endpoint: impl Into<PathBuf>) -> Result<ServiceHandle, ServiceError> {
    spawn_internal(endpoint.into(), None)
}

pub fn spawn_persistent(
    endpoint: impl Into<PathBuf>,
    state_path: impl Into<PathBuf>,
) -> Result<ServiceHandle, ServiceError> {
    spawn_internal(endpoint.into(), Some(state_path.into()))
}

fn spawn_internal(
    endpoint: PathBuf,
    state_path: Option<PathBuf>,
) -> Result<ServiceHandle, ServiceError> {
    prepare_endpoint(&endpoint)?;
    let listener = local_listener(&endpoint).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AddrInUse {
            ServiceError::AlreadyRunning(endpoint.clone())
        } else {
            ServiceError::Endpoint(error)
        }
    })?;
    set_endpoint_permissions(&endpoint)?;

    let stopping = Arc::new(AtomicBool::new(false));
    let updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>> = Arc::new(Mutex::new(Vec::new()));
    let state = Arc::new(Mutex::new(
        state_path
            .as_deref()
            .map(load_state)
            .unwrap_or_else(DockState::new),
    ));
    let thread_stopping = Arc::clone(&stopping);
    let thread_updates = Arc::clone(&updates);
    let thread_state = Arc::clone(&state);
    let thread_endpoint = endpoint.clone();
    let thread_state_path = state_path.clone();
    let join = thread::Builder::new()
        .name("dock-ipc-accept".to_owned())
        .spawn(move || {
            while !thread_stopping.load(Ordering::Acquire) {
                let stream = match local_accept(&listener) {
                    Ok(value) => value,
                    Err(_) if thread_stopping.load(Ordering::Acquire) => break,
                    Err(_) => continue,
                };
                let state = Arc::clone(&thread_state);
                let updates = Arc::clone(&thread_updates);
                let stopping = Arc::clone(&thread_stopping);
                let state_path = thread_state_path.clone();
                thread::spawn(move || handle_client(stream, state, updates, stopping, state_path));
            }
            cleanup_endpoint(&thread_endpoint);
        })
        .expect("spawn Dock IPC accept thread");

    Ok(ServiceHandle {
        endpoint,
        state_path,
        stopping,
        state,
        updates,
        join: Mutex::new(Some(join)),
    })
}

#[cfg(unix)]
fn prepare_endpoint(endpoint: &Path) -> Result<(), ServiceError> {
    if let Some(parent) = endpoint.parent() {
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)?;
        if !parent_existed {
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }
    if endpoint.exists() {
        let metadata = fs::symlink_metadata(endpoint)?;
        if !metadata.file_type().is_socket() {
            return Err(ServiceError::AlreadyRunning(endpoint.to_owned()));
        }
        match local_connect(endpoint) {
            Ok(_) => return Err(ServiceError::AlreadyRunning(endpoint.to_owned())),
            Err(_) => fs::remove_file(endpoint)?,
        }
    }
    Ok(())
}

#[cfg(windows)]
fn prepare_endpoint(_endpoint: &Path) -> Result<(), ServiceError> {
    Ok(())
}

#[cfg(unix)]
fn set_endpoint_permissions(endpoint: &Path) -> Result<(), ServiceError> {
    fs::set_permissions(endpoint, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn set_endpoint_permissions(_endpoint: &Path) -> Result<(), ServiceError> {
    Ok(())
}

#[cfg(unix)]
fn cleanup_endpoint(endpoint: &Path) {
    let _ = fs::remove_file(endpoint);
}

#[cfg(windows)]
fn cleanup_endpoint(_endpoint: &Path) {}

fn handle_client(
    mut stream: LocalStream,
    state: Arc<Mutex<DockState>>,
    updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    stopping: Arc<AtomicBool>,
    state_path: Option<PathBuf>,
) {
    let request = match read_request(&stream) {
        Ok(bytes) => parse_request(&bytes),
        Err(reason) => {
            let snapshot = state.lock().expect("state lock").snapshot();
            let _ = write_response(&mut stream, &WireResponse::rejected(&snapshot, &reason));
            return;
        }
    };
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            let snapshot = state.lock().expect("state lock").snapshot();
            let _ = write_response(
                &mut stream,
                &WireResponse::rejected(&snapshot, &error.to_string()),
            );
            return;
        }
    };
    match request {
        IpcRequest::Subscribe => handle_subscription(stream, state, updates, stopping),
        request => {
            let response = dispatch(request, &state, state_path.as_deref());
            let _ = write_response(&mut stream, &response);
            if response.accepted {
                broadcast(
                    &updates,
                    SnapshotMessage::snapshot(
                        response.snapshot.clone(),
                        response.attention.clone(),
                    ),
                );
            }
        }
    }
}

fn dispatch(
    request: IpcRequest,
    state: &Arc<Mutex<DockState>>,
    state_path: Option<&Path>,
) -> WireResponse {
    let mut state = state.lock().expect("state lock");
    let is_mutating = matches!(
        &request,
        IpcRequest::Event(_) | IpcRequest::Acknowledge { .. } | IpcRequest::Reset { .. }
    );
    let response = match request {
        IpcRequest::Event(event) => WireResponse::from_apply(&state.apply(event)),
        IpcRequest::Snapshot | IpcRequest::Subscribe => WireResponse::accepted(&state.snapshot()),
        IpcRequest::Acknowledge { source, session_id } => {
            WireResponse::accepted(&state.acknowledge(&source, &session_id))
        }
        IpcRequest::Reset { source, session_id } => {
            WireResponse::accepted(&state.reset(&source, &session_id))
        }
    };
    if response.accepted && is_mutating {
        if let Some(path) = state_path {
            if let Err(error) = persist_state(path, &state) {
                eprintln!("Agent Activity Dock could not persist state: {error}");
            }
        }
    }
    response
}

fn handle_subscription(
    mut stream: LocalStream,
    state: Arc<Mutex<DockState>>,
    updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    stopping: Arc<AtomicBool>,
) {
    let (sender, receiver) = mpsc::channel();
    updates
        .lock()
        .expect("update subscribers lock")
        .push(sender);
    let initial = SnapshotMessage::subscribed(SnapshotView::from(
        &state.lock().expect("state lock").snapshot(),
    ));
    if write_json(&mut stream, &initial).is_err() {
        return;
    }
    while !stopping.load(Ordering::Acquire) {
        match receiver.recv() {
            Ok(message) if write_json(&mut stream, &message).is_err() => break,
            Ok(_) => {}
            Err(_) => break,
        }
    }
}

fn broadcast(updates: &Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>, message: SnapshotMessage) {
    let mut subscribers = updates.lock().expect("update subscribers lock");
    subscribers.retain(|sender| sender.send(message.clone()).is_ok());
}

fn read_request(stream: &LocalStream) -> Result<Vec<u8>, String> {
    let reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    let mut limited = reader.take((MAX_FRAME_BYTES + 1) as u64);
    limited
        .read_until(b'\n', &mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err("message_too_large".to_owned());
    }
    Ok(bytes)
}

fn write_response(stream: &mut LocalStream, response: &WireResponse) -> std::io::Result<()> {
    write_json(stream, response)
}

fn write_json<T: Serialize>(stream: &mut LocalStream, value: &T) -> std::io::Result<()> {
    let line = encode_line(value)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    stream.write_all(&line)
}

fn load_state(path: &Path) -> DockState {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PersistedState>(&bytes).ok())
        .map(DockState::from_persisted)
        .unwrap_or_else(DockState::new)
}

fn persist_state(path: &Path, state: &DockState) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)?;
        if !parent_existed {
            set_private_permissions(parent, 0o700)?;
        }
    }
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(&state.persisted())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    fs::write(&temp, bytes)?;
    set_private_permissions(&temp, 0o600)?;
    fs::rename(temp, path)
}

#[cfg(unix)]
fn set_private_permissions(path: &Path, mode: u32) -> std::io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(windows)]
fn set_private_permissions(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}
