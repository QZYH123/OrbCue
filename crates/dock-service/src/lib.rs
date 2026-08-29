//! Event-driven local service used by the Tauri process and headless modes.

mod client;

pub use client::{
    attach_or_listen, connect_or_spawn_detached, query_service, DetachedConnectError, DockSession,
};

use orbcue_core::{DockState, PersistedState};
use orbcue_ipc::{
    encode_line, local_accept, local_connect, local_listener, parse_request, IpcRequest,
    LocalStream, SnapshotView, WireResponse, MAX_FRAME_BYTES,
};
#[cfg(windows)]
use orbcue_ipc::{resolve_backend, DockBackend};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use thiserror::Error;

#[cfg(windows)]
fn hide_windows_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

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
    pub attention: Option<orbcue_core::Attention>,
}

impl SnapshotMessage {
    pub fn snapshot(snapshot: SnapshotView, attention: Option<orbcue_core::Attention>) -> Self {
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

    pub fn snapshot(&self) -> orbcue_core::DockSnapshot {
        self.state.lock().expect("state lock").snapshot()
    }

    pub fn acknowledge(
        &self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> WireResponse {
        self.mutate_and_broadcast("acknowledgement", |state| {
            state.acknowledge(source, session_id, terminal_id)
        })
    }

    pub fn reset(&self, source: &str, session_id: &str, terminal_id: Option<&str>) -> WireResponse {
        self.mutate_and_broadcast("reset", |state| {
            state.reset(source, session_id, terminal_id)
        })
    }

    fn mutate_and_broadcast(
        &self,
        persist_label: &str,
        mutate: impl FnOnce(&mut DockState) -> orbcue_core::DockSnapshot,
    ) -> WireResponse {
        let mut state = self.state.lock().expect("state lock");
        let snapshot = mutate(&mut state);
        if let Some(path) = &self.state_path {
            if let Err(error) = persist_state(path, &state) {
                eprintln!("OrbCue could not persist {persist_label}: {error}");
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

    schedule_wsl_state_migration(Arc::clone(&state), Arc::clone(&updates), state_path.clone());
    schedule_liveness_reaper(
        Arc::clone(&state),
        Arc::clone(&updates),
        state_path.clone(),
        Arc::clone(&stopping),
    );

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
            let _ = write_json(&mut stream, &WireResponse::rejected(&snapshot, &reason));
            return;
        }
    };
    let request = match request {
        Ok(request) => request,
        Err(error) => {
            let snapshot = state.lock().expect("state lock").snapshot();
            let _ = write_json(
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
            let _ = write_json(&mut stream, &response);
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
        IpcRequest::Acknowledge {
            source,
            session_id,
            terminal_id,
        } => {
            WireResponse::accepted(&state.acknowledge(&source, &session_id, terminal_id.as_deref()))
        }
        IpcRequest::Reset {
            source,
            session_id,
            terminal_id,
        } => WireResponse::accepted(&state.reset(&source, &session_id, terminal_id.as_deref())),
    };
    if response.accepted && is_mutating {
        if let Some(path) = state_path {
            if let Err(error) = persist_state(path, &state) {
                eprintln!("OrbCue could not persist state: {error}");
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

#[cfg(any(test, windows))]
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MigrationReason {
    Copied,
    DestNonEmpty,
    #[allow(dead_code)]
    AlreadyMarked,
    Timeout,
    WslMissing,
    InvalidJson,
    EmptySource,
}

#[cfg(any(test, windows))]
#[cfg_attr(not(windows), allow(dead_code))]
impl MigrationReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Copied => "copied",
            Self::DestNonEmpty => "dest_non_empty",
            Self::AlreadyMarked => "already_marked",
            Self::Timeout => "timeout",
            Self::WslMissing => "wsl_missing",
            Self::InvalidJson => "invalid_json",
            Self::EmptySource => "empty_source",
        }
    }
}

#[cfg(windows)]
fn migration_marker_path(state_path: &Path) -> PathBuf {
    state_path.with_file_name("state.migrated-from-wsl")
}

fn schedule_wsl_state_migration(
    state: Arc<Mutex<DockState>>,
    updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    state_path: Option<PathBuf>,
) {
    #[cfg(not(windows))]
    {
        let _ = (state, updates, state_path);
    }
    #[cfg(windows)]
    {
        if resolve_backend() != DockBackend::Local {
            return;
        }
        let Some(state_path) = state_path else {
            return;
        };
        let _ = thread::Builder::new()
            .name("orb-wsl-migrate".to_owned())
            .spawn(move || migrate_wsl_state(state, updates, state_path));
    }
}

#[cfg(any(test, windows))]
fn apply_copied_sessions(
    state: &Arc<Mutex<DockState>>,
    updates: &Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    state_path: &Path,
    copied: PersistedState,
) -> MigrationReason {
    let copied_len = copied.sessions.len();
    let mut guard = state.lock().expect("state lock");
    if !guard.snapshot().sessions.is_empty() {
        return MigrationReason::DestNonEmpty;
    }
    if copied_len == 0 {
        return MigrationReason::EmptySource;
    }
    *guard = DockState::from_persisted(copied);
    let snapshot = SnapshotView::from(&guard.snapshot());
    if let Err(error) = persist_state(state_path, &guard) {
        eprintln!("OrbCue could not persist migrated state: {error}");
    }
    drop(guard);
    broadcast(updates, SnapshotMessage::snapshot(snapshot, None));
    MigrationReason::Copied
}

#[cfg(windows)]
fn write_migration_marker(
    state_path: &Path,
    reason: MigrationReason,
    copied_sessions: usize,
    source: &str,
) {
    let at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_owned());
    let payload = serde_json::json!({
        "version": 1,
        "at": at,
        "source": source,
        "copied_sessions": copied_sessions,
        "reason": reason.as_str(),
    });
    if let Some(parent) = state_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(
        migration_marker_path(state_path),
        serde_json::to_vec_pretty(&payload).unwrap_or_default(),
    );
}

#[cfg(windows)]
fn migrate_wsl_state(
    state: Arc<Mutex<DockState>>,
    updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    state_path: PathBuf,
) {
    if migration_marker_path(&state_path).is_file() {
        return;
    }
    let (bytes, source) = match cat_wsl_state(Duration::from_secs(2)) {
        Ok(value) => value,
        Err(reason) => {
            write_migration_marker(&state_path, reason, 0, "unknown");
            return;
        }
    };
    if bytes.trim().is_empty() {
        write_migration_marker(&state_path, MigrationReason::EmptySource, 0, &source);
        return;
    }
    let copied: PersistedState = match serde_json::from_str(bytes.trim()) {
        Ok(value) => value,
        Err(_) => {
            write_migration_marker(&state_path, MigrationReason::InvalidJson, 0, &source);
            return;
        }
    };
    let copied_sessions = copied.sessions.len();
    let reason = apply_copied_sessions(&state, &updates, &state_path, copied);
    write_migration_marker(&state_path, reason, copied_sessions, &source);
}

#[cfg(windows)]
fn cat_wsl_state(timeout: Duration) -> Result<(String, String), MigrationReason> {
    let mut command = std::process::Command::new("wsl.exe");
    hide_windows_console(&mut command);
    if let Ok(distro) = std::env::var("ORBCUE_WSL_DISTRO") {
        if !distro.is_empty() {
            command.args(["-d", &distro]);
        }
    }
    command.args([
        "-e",
        "sh",
        "-c",
        r#"cat "${XDG_STATE_HOME:-$HOME/.local/state}/orbcue/state.json""#,
    ]);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let child = command.spawn().map_err(|_| MigrationReason::WslMissing)?;
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            Ok((stdout, "wsl-state.json".to_owned()))
        }
        Ok(Ok(output)) if output.stdout.is_empty() => Err(MigrationReason::EmptySource),
        Ok(Ok(_)) => Err(MigrationReason::InvalidJson),
        Ok(Err(_)) => Err(MigrationReason::WslMissing),
        Err(_) => {
            let mut kill = std::process::Command::new("taskkill");
            hide_windows_console(&mut kill);
            let _ = kill.args(["/PID", &pid.to_string(), "/F"]).status();
            Err(MigrationReason::Timeout)
        }
    }
}

fn schedule_liveness_reaper(
    state: Arc<Mutex<DockState>>,
    updates: Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    state_path: Option<PathBuf>,
    stopping: Arc<AtomicBool>,
) {
    let _ = thread::Builder::new()
        .name("orb-liveness".to_owned())
        .spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                thread::sleep(Duration::from_secs(15));
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                reap_dead_sessions(&state, &updates, state_path.as_deref());
            }
        });
}

fn reap_dead_sessions(
    state: &Arc<Mutex<DockState>>,
    updates: &Arc<Mutex<Vec<mpsc::Sender<SnapshotMessage>>>>,
    state_path: Option<&Path>,
) {
    let targets = state.lock().expect("state lock").liveness_targets();
    let dead = find_dead_sessions(&targets);
    for (source, session_id, liveness) in dead {
        let event_id = orbcue_core::liveness_closed_event_id(
            &source,
            &session_id,
            liveness.pid,
            liveness.starttime,
        );
        let mut event = orbcue_core::DockEvent::new(
            &event_id,
            orbcue_core::EventKind::Closed,
            &source,
            &session_id,
        );
        event
            .metadata
            .insert("agent_os".to_owned(), liveness.os.clone());
        event
            .metadata
            .insert("agent_pid".to_owned(), liveness.pid.to_string());
        event
            .metadata
            .insert("agent_starttime".to_owned(), liveness.starttime.to_string());
        if let Some(distro) = liveness.distro.as_ref() {
            event
                .metadata
                .insert("agent_wsl_distro".to_owned(), distro.clone());
        }
        let response = dispatch(IpcRequest::Event(event), state, state_path);
        if response.accepted {
            broadcast(
                updates,
                SnapshotMessage::snapshot(response.snapshot.clone(), response.attention.clone()),
            );
        }
    }
}

fn find_dead_sessions(
    targets: &[(String, String, orbcue_core::AgentLiveness)],
) -> Vec<(String, String, orbcue_core::AgentLiveness)> {
    let mut dead = Vec::new();
    #[cfg(windows)]
    {
        let mut by_distro: std::collections::BTreeMap<
            String,
            Vec<(String, String, orbcue_core::AgentLiveness)>,
        > = std::collections::BTreeMap::new();
        for (source, session_id, liveness) in targets {
            match liveness.os.as_str() {
                "windows" => {
                    if windows_pid_is_dead(liveness.pid, liveness.starttime) == Some(true) {
                        dead.push((source.clone(), session_id.clone(), liveness.clone()));
                    }
                }
                "linux" => {
                    let Some(distro) = liveness.distro.as_deref().filter(|value| !value.is_empty())
                    else {
                        continue;
                    };
                    by_distro.entry(distro.to_owned()).or_default().push((
                        source.clone(),
                        session_id.clone(),
                        liveness.clone(),
                    ));
                }
                _ => {}
            }
        }
        for (distro, group) in by_distro {
            dead.extend(wsl_dead_sessions(&distro, &group));
        }
    }
    #[cfg(not(windows))]
    {
        for (source, session_id, liveness) in targets {
            if liveness.os != "linux" {
                continue;
            }
            if linux_pid_is_dead(liveness.pid, liveness.starttime) == Some(true) {
                dead.push((source.clone(), session_id.clone(), liveness.clone()));
            }
        }
    }
    dead
}

#[cfg(not(windows))]
fn linux_pid_is_dead(pid: u32, starttime: u64) -> Option<bool> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let recorded = parse_proc_starttime(&stat);
            Some(match recorded {
                Some(value) => value != starttime,
                None => true,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
        Err(_) => None,
    }
}

#[cfg(not(windows))]
fn parse_proc_starttime(contents: &str) -> Option<u64> {
    let end = contents.rfind(')')?;
    let mut fields = contents.get(end + 1..)?.split_whitespace();
    let _state = fields.next()?;
    let _ppid = fields.next()?;
    let _pgrp = fields.next()?;
    let _session = fields.next()?;
    let _tty_nr = fields.next()?;
    for _ in 0..14 {
        fields.next()?;
    }
    fields.next()?.parse().ok()
}

#[cfg(windows)]
fn windows_pid_is_dead(pid: u32, starttime: u64) -> Option<bool> {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const ERROR_ACCESS_DENIED: u32 = 5;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut u64,
            exit: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return if GetLastError() == ERROR_ACCESS_DENIED {
                Some(false)
            } else {
                Some(true)
            };
        }
        let mut creation = 0u64;
        let mut exit = 0u64;
        let mut kernel = 0u64;
        let mut user = 0u64;
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(creation != starttime)
    }
}

#[cfg(windows)]
fn wsl_dead_sessions(
    distro: &str,
    group: &[(String, String, orbcue_core::AgentLiveness)],
) -> Vec<(String, String, orbcue_core::AgentLiveness)> {
    let queries: Vec<serde_json::Value> = group
        .iter()
        .map(|(source, session_id, liveness)| {
            serde_json::json!({
                "source": source,
                "session_id": session_id,
                "pid": liveness.pid,
                "starttime": liveness.starttime,
            })
        })
        .collect();
    let mut command = std::process::Command::new("wsl.exe");
    hide_windows_console(&mut command);
    command.args([
        "-d",
        distro,
        "-e",
        "sh",
        "-c",
        r#"exec "$HOME/.local/bin/orb" "$@""#,
        "sh",
        "liveness-check",
    ]);
    command.env("ORBCUE_HOP", "wsl");
    command.env("ORBCUE_BACKEND", "local");
    command.stdin(std::process::Stdio::piped());
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return Vec::new(),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&serde_json::to_vec(&queries).unwrap_or_default());
    }
    let pid = child.id();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    let output = match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(Ok(output)) if output.status.success() => output,
        Err(_) => {
            let mut kill = std::process::Command::new("taskkill");
            hide_windows_console(&mut kill);
            let _ = kill.args(["/PID", &pid.to_string(), "/F"]).status();
            return Vec::new();
        }
        _ => return Vec::new(),
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let Some(dead_list) = parsed.get("dead").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    let mut dead = Vec::new();
    for item in dead_list {
        let Some(session_id) = item.get("session_id").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(source) = item.get("source").and_then(|value| value.as_str()) else {
            continue;
        };
        let pid = item
            .get("pid")
            .and_then(|value| value.as_u64())
            .map(|value| value as u32);
        if let Some(found) = group.iter().find(|(src, sid, live)| {
            src == source && sid == session_id && pid.map(|pid| pid == live.pid).unwrap_or(true)
        }) {
            dead.push(found.clone());
        }
    }
    dead
}

#[cfg(test)]
mod tests {
    use super::{apply_copied_sessions, SnapshotMessage};
    use orbcue_core::{DockEvent, DockState, EventKind};
    use std::sync::{mpsc, Arc, Mutex};

    #[test]
    fn copied_wsl_state_fills_an_empty_daemon() {
        let state = Arc::new(Mutex::new(DockState::new()));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let (sender, receiver) = mpsc::channel();
        updates.lock().unwrap().push(sender);
        let dir = std::env::temp_dir().join(format!(
            "dock-migrate-empty-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        let mut seeded = DockState::new();
        seeded.apply(DockEvent::new("e1", EventKind::Started, "grok", "s1"));
        assert_eq!(
            apply_copied_sessions(&state, &updates, &path, seeded.persisted()),
            super::MigrationReason::Copied
        );
        assert_eq!(state.lock().unwrap().snapshot().sessions.len(), 1);
        let message: SnapshotMessage = receiver.recv().unwrap();
        assert_eq!(message.snapshot.sessions.len(), 1);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn nonempty_dest_does_not_replace_live_sessions() {
        let mut live = DockState::new();
        live.apply(DockEvent::new("e1", EventKind::Started, "claude", "live"));
        let state = Arc::new(Mutex::new(live));
        let updates = Arc::new(Mutex::new(Vec::new()));
        let mut incoming = DockState::new();
        incoming.apply(DockEvent::new("e2", EventKind::Started, "grok", "copied"));
        let dir = std::env::temp_dir().join(format!(
            "dock-migrate-full-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        assert_eq!(
            apply_copied_sessions(&state, &updates, &path, incoming.persisted()),
            super::MigrationReason::DestNonEmpty
        );
        assert_eq!(
            state.lock().unwrap().snapshot().sessions[0].session_id,
            "live"
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn linux_host_reaps_without_wsl_distro() {
        let dead = super::linux_pid_is_dead(u32::MAX, 1);
        assert_eq!(dead, Some(true));
        let liveness = orbcue_core::AgentLiveness {
            os: "linux".to_owned(),
            pid: u32::MAX,
            starttime: 1,
            distro: None,
        };
        let found = super::find_dead_sessions(&[("grok".to_owned(), "s1".to_owned(), liveness)]);
        assert_eq!(found.len(), 1);
    }
}
