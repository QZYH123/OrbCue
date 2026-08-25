use crate::{ServiceError, ServiceHandle, SnapshotMessage};
use agent_activity_dock_ipc::{
    encode_request, local_connect, local_set_recv_timeout, local_set_send_timeout, IpcRequest,
    SnapshotView, WireResponse,
};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const QUERY_TIMEOUT: Duration = Duration::from_millis(500);

pub enum DockSession {
    Owned(ServiceHandle),
    Remote { endpoint: PathBuf },
}

impl DockSession {
    pub fn owns_daemon(&self) -> bool {
        matches!(self, Self::Owned(_))
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Owned(_) => "owned",
            Self::Remote { .. } => "remote",
        }
    }

    pub fn endpoint(&self) -> &Path {
        match self {
            Self::Owned(handle) => handle.endpoint(),
            Self::Remote { endpoint } => endpoint,
        }
    }

    pub fn snapshot(&self) -> Result<SnapshotView, String> {
        match self {
            Self::Owned(handle) => Ok(SnapshotView::from(&handle.snapshot())),
            Self::Remote { endpoint } => {
                Ok(query_service(endpoint, &IpcRequest::Snapshot)?.snapshot)
            }
        }
    }

    pub fn acknowledge(
        &self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> Result<SnapshotView, String> {
        match self {
            Self::Owned(handle) => Ok(handle.acknowledge(source, session_id, terminal_id).snapshot),
            Self::Remote { endpoint } => Ok(query_service(
                endpoint,
                &IpcRequest::Acknowledge {
                    source: source.to_owned(),
                    session_id: session_id.to_owned(),
                    terminal_id: terminal_id.map(str::to_owned),
                },
            )?
            .snapshot),
        }
    }

    pub fn reset(
        &self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> Result<SnapshotView, String> {
        match self {
            Self::Owned(handle) => Ok(handle.reset(source, session_id, terminal_id).snapshot),
            Self::Remote { endpoint } => Ok(query_service(
                endpoint,
                &IpcRequest::Reset {
                    source: source.to_owned(),
                    session_id: session_id.to_owned(),
                    terminal_id: terminal_id.map(str::to_owned),
                },
            )?
            .snapshot),
        }
    }

    pub fn subscribe(&self) -> mpsc::Receiver<SnapshotMessage> {
        match self {
            Self::Owned(handle) => handle.subscribe_updates(),
            Self::Remote { endpoint } => subscribe_remote(endpoint.clone()),
        }
    }

    pub fn request_shutdown(&self) {
        if let Self::Owned(handle) = self {
            handle.request_shutdown();
        }
    }

    pub fn wait_for_shutdown(&self) {
        if let Self::Owned(handle) = self {
            handle.wait_for_shutdown();
        }
    }
}

pub fn attach_or_listen(
    endpoint: impl Into<PathBuf>,
    state_path: impl Into<PathBuf>,
    dockd: Option<PathBuf>,
) -> Result<DockSession, ServiceError> {
    let endpoint = endpoint.into();
    let state_path = state_path.into();
    if query_service(&endpoint, &IpcRequest::Snapshot).is_ok() {
        return Ok(DockSession::Remote { endpoint });
    }
    if let Some(binary) = dockd.filter(|path| path.is_file()) {
        let log_path = state_path
            .parent()
            .map(|parent| parent.join("dockd.log"))
            .unwrap_or_else(|| PathBuf::from("dockd.log"));
        let pid_path = state_path
            .parent()
            .map(|parent| parent.join("dockd.pid"))
            .unwrap_or_else(|| PathBuf::from("dockd.pid"));
        if spawn_detached_daemon(&binary, &log_path, &pid_path).is_ok() {
            for _ in 0..25 {
                if query_service(&endpoint, &IpcRequest::Snapshot).is_ok() {
                    return Ok(DockSession::Remote { endpoint });
                }
                thread::sleep(Duration::from_millis(80));
            }
        }
    }
    match crate::spawn_persistent(&endpoint, &state_path) {
        Ok(handle) => Ok(DockSession::Owned(handle)),
        Err(ServiceError::AlreadyRunning(_)) => {
            if query_service(&endpoint, &IpcRequest::Snapshot).is_ok() {
                Ok(DockSession::Remote { endpoint })
            } else {
                Err(ServiceError::AlreadyRunning(endpoint))
            }
        }
        Err(error) => Err(error),
    }
}

/// CLI short processes use this instead of `attach_or_listen`.
/// Never `spawn_persistent`: exiting `dock emit` must not take the daemon with it.
#[derive(Debug)]
pub enum DetachedConnectError {
    NeedPresenterOrDockd,
    Io(std::io::Error),
}

impl std::fmt::Display for DetachedConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeedPresenterOrDockd => write!(
                f,
                "cannot reach Dock named pipe; start the presenter or `dock up` (requires dockd.exe)"
            ),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl From<std::io::Error> for DetachedConnectError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn connect_or_spawn_detached(
    endpoint: impl Into<PathBuf>,
    state_path: impl Into<PathBuf>,
    dockd: Option<PathBuf>,
) -> Result<PathBuf, DetachedConnectError> {
    let endpoint = endpoint.into();
    let state_path = state_path.into();
    if query_service(&endpoint, &IpcRequest::Snapshot).is_ok() {
        return Ok(endpoint);
    }
    let log_path = state_path
        .parent()
        .map(|parent| parent.join("dockd.log"))
        .unwrap_or_else(|| PathBuf::from("dockd.log"));
    let pid_path = state_path
        .parent()
        .map(|parent| parent.join("dockd.pid"))
        .unwrap_or_else(|| PathBuf::from("dockd.pid"));
    if let Some(binary) = dockd.filter(|path| path.is_file()) {
        spawn_detached_daemon(&binary, &log_path, &pid_path)?;
        for _ in 0..25 {
            if query_service(&endpoint, &IpcRequest::Snapshot).is_ok() {
                return Ok(endpoint);
            }
            thread::sleep(Duration::from_millis(80));
        }
    }
    Err(DetachedConnectError::NeedPresenterOrDockd)
}

pub fn query_service(endpoint: &Path, request: &IpcRequest) -> Result<WireResponse, String> {
    let mut stream = local_connect(endpoint).map_err(|error| error.to_string())?;
    local_set_recv_timeout(&stream, Some(QUERY_TIMEOUT)).map_err(|error| error.to_string())?;
    local_set_send_timeout(&stream, Some(QUERY_TIMEOUT)).map_err(|error| error.to_string())?;
    let line = encode_request(request).map_err(|error| error.to_string())?;
    stream.write_all(&line).map_err(|error| error.to_string())?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|error| error.to_string())?;
    if response.trim().is_empty() {
        return Err("empty Dock response".to_owned());
    }
    serde_json::from_str(&response).map_err(|error| error.to_string())
}

fn subscribe_remote(endpoint: PathBuf) -> mpsc::Receiver<SnapshotMessage> {
    let (sender, receiver) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("dock-ui-subscribe".to_owned())
        .spawn(move || {
            let mut backoff = Duration::from_millis(200);
            loop {
                match subscribe_once(&endpoint, &sender) {
                    Ok(()) => break,
                    Err(_) => {
                        thread::sleep(backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(2));
                    }
                }
            }
        });
    receiver
}

fn subscribe_once(endpoint: &Path, sender: &mpsc::Sender<SnapshotMessage>) -> Result<(), String> {
    let mut stream = local_connect(endpoint).map_err(|error| error.to_string())?;
    stream
        .write_all(b"{\"query\":\"subscribe\"}\n")
        .map_err(|error| error.to_string())?;
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            return Err("Dock subscribe closed".to_owned());
        }
        let message: SnapshotMessage =
            serde_json::from_str(&line).map_err(|error| error.to_string())?;
        if sender.send(message).is_err() {
            return Ok(());
        }
    }
}

fn spawn_detached_daemon(binary: &Path, log_path: &Path, pid_path: &Path) -> std::io::Result<()> {
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let log_err = log.try_clone()?;
    let mut command = Command::new(binary);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let child = command.spawn()?;
    let _ = fs::write(pid_path, child.id().to_string());
    Ok(())
}
