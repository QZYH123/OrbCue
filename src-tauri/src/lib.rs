mod focus;
mod region;
mod toast;
mod tray;
#[cfg(windows)]
mod wsl_session;

use agent_activity_dock_connect::{
    AgentOrigin, ConnectionManager, ConnectionPreview, ConnectionRecord, DiscoveredAgent,
};
use agent_activity_dock_core::{
    attention_click_followup, attention_jump, dispatch_attention_toast, highlight_target,
    AttentionClickFollowup, AttentionJump, ToastDispatch,
};
use agent_activity_dock_ipc::{DockBackend, SnapshotView};
use agent_activity_dock_service::{attach_or_listen, DockSession, SnapshotMessage};
use focus::FocusResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, PhysicalPosition, Position, State, WebviewWindow,
};
use tauri_plugin_opener::OpenerExt;
use toast::{prepare_windows_notifications, preview_attention_toast, PresenterToastSink};

struct AppService(Mutex<Option<Arc<dyn PresenterSession>>>);
static LAST_BALL_SAVE_MS: AtomicU64 = AtomicU64::new(0);
static NOTIFICATIONS_ENABLED: AtomicBool = AtomicBool::new(true);
static NOTIFICATION_FAIL_LOGGED: AtomicBool = AtomicBool::new(false);
static INVENTORY_CACHE: Mutex<Option<AgentInventory>> = Mutex::new(None);
static INVENTORY_REFRESH_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static BALL_HIDDEN: AtomicBool = AtomicBool::new(false);

struct TrayBallItem(tauri::menu::MenuItem<tauri::Wry>);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AgentSide {
    Wsl,
    Windows,
}

impl AgentSide {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "wsl" => Ok(Self::Wsl),
            "windows" => Ok(Self::Windows),
            other => Err(format!("invalid side: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InventoryAgent {
    name: String,
    path: PathBuf,
    side: AgentSide,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct InventoryConnection {
    #[serde(flatten)]
    record: ConnectionRecord,
    side: AgentSide,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct AgentInventory {
    discovered: Vec<InventoryAgent>,
    connected: Vec<InventoryConnection>,
}

pub(crate) trait PresenterSession: Send + Sync {
    fn snapshot(&self) -> Result<SnapshotView, String>;
    fn acknowledge(&self, source: &str, session_id: &str) -> Result<SnapshotView, String>;
    fn reset(&self, source: &str, session_id: &str) -> Result<SnapshotView, String>;
    fn subscribe(&self) -> mpsc::Receiver<SnapshotMessage>;
    fn request_shutdown(&self);
    fn wait_for_shutdown(&self);
}

impl PresenterSession for DockSession {
    fn snapshot(&self) -> Result<SnapshotView, String> {
        DockSession::snapshot(self)
    }

    fn acknowledge(&self, source: &str, session_id: &str) -> Result<SnapshotView, String> {
        DockSession::acknowledge(self, source, session_id)
    }

    fn reset(&self, source: &str, session_id: &str) -> Result<SnapshotView, String> {
        DockSession::reset(self, source, session_id)
    }

    fn subscribe(&self) -> mpsc::Receiver<SnapshotMessage> {
        DockSession::subscribe(self)
    }

    fn request_shutdown(&self) {
        DockSession::request_shutdown(self);
    }

    fn wait_for_shutdown(&self) {
        DockSession::wait_for_shutdown(self);
    }
}

fn connection_manager(app: &AppHandle) -> ConnectionManager {
    let dock_binary = dock_binary_path(app);
    ConnectionManager::from_environment(dock_binary)
}

fn dock_binary_path(app: &AppHandle) -> PathBuf {
    if let Some(path) = std::env::var_os("AGENT_ACTIVITY_DOCK_DOCK_BINARY")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return path;
    }

    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("binaries").join(sidecar_name()));
        candidates.push(resource_dir.join("dock").join(sidecar_name()));
        candidates.push(resource_dir.join("dock.exe"));
        candidates.push(resource_dir.join("dock"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join(sidecar_name()));
            candidates.push(parent.join("dock.exe"));
            candidates.push(parent.join("dock"));
            candidates.push(parent.join("binaries").join(sidecar_name()));
        }
    }
    candidates.push(PathBuf::from("dock.exe"));
    candidates.push(PathBuf::from("dock"));
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from("dock"))
}

fn install_windows_trampoline_cli(app: &AppHandle) {
    #[cfg(windows)]
    {
        let source = dock_binary_path(app);
        if !source.is_file() {
            return;
        }
        if let Err(error) = agent_activity_dock_connect::install_windows_cli(&source) {
            eprintln!("Agent Activity Dock: cannot install dock CLI: {error}");
        }
    }
    let _ = app;
}

fn sidecar_name() -> String {
    let target = if cfg!(all(windows, target_arch = "x86_64")) {
        return "dock-x86_64-pc-windows-msvc.exe".to_owned();
    } else if cfg!(all(windows, target_arch = "aarch64")) {
        return "dock-aarch64-pc-windows-msvc.exe".to_owned();
    } else if cfg!(windows) {
        return "dock.exe".to_owned();
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else {
        "dock"
    };
    if target == "dock" {
        target.to_owned()
    } else {
        format!("dock-{target}")
    }
}

fn dockd_binary_path() -> Option<PathBuf> {
    let file_name = if cfg!(windows) { "dockd.exe" } else { "dockd" };
    let mut candidates = Vec::new();
    if let Some(value) =
        std::env::var_os("AGENT_ACTIVITY_DOCK_DOCKD").filter(|value| !value.is_empty())
    {
        candidates.push(PathBuf::from(value));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join(file_name));
            candidates.push(parent.join("binaries").join(file_name));
            candidates.push(parent.join("binaries").join(dockd_sidecar_name()));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        candidates.push(
            PathBuf::from(local)
                .join("Agent Activity Dock")
                .join("dockd.exe"),
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".local/bin").join(file_name));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            candidates.push(dir.join(file_name));
        }
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn dockd_sidecar_name() -> String {
    sidecar_name().replacen("dock", "dockd", 1)
}

fn presenter_backend() -> DockBackend {
    agent_activity_dock_ipc::resolve_backend()
}

fn current_session(state: &AppService) -> Result<Arc<dyn PresenterSession>, String> {
    state
        .0
        .lock()
        .map_err(|_| "service lock poisoned".to_owned())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "service stopped".to_owned())
}

fn empty_snapshot() -> SnapshotView {
    SnapshotView {
        working_count: 0,
        tracked_count: 0,
        pending_count: 0,
        pending_mark: String::new(),
        count_label: "0/0".to_owned(),
        border_state: "idle".to_owned(),
        sessions: Vec::new(),
        audit: Vec::new(),
    }
}

fn cached_inventory() -> AgentInventory {
    INVENTORY_CACHE
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default()
}

fn publish_inventory(app: &AppHandle, inventory: AgentInventory) {
    if let Ok(mut cache) = INVENTORY_CACHE.lock() {
        *cache = Some(inventory.clone());
    }
    let _ = app.emit("dock:inventory", &inventory);
}

fn load_fresh_inventory(app: &AppHandle) -> AgentInventory {
    let mut discovered = Vec::new();
    let mut connected = Vec::new();

    #[cfg(windows)]
    {
        let manager = connection_manager(app);
        discovered.extend(
            manager
                .discover()
                .into_iter()
                .map(|agent| sided_discovered(agent, AgentSide::Windows)),
        );
        connected.extend(
            manager
                .records()
                .into_iter()
                .map(|record| sided_connection(record, AgentSide::Windows)),
        );
        match wsl_session::raw_inventory() {
            Ok((wsl_discovered, wsl_connected)) => {
                discovered.extend(
                    wsl_discovered
                        .into_iter()
                        .filter(|agent| agent.origin != AgentOrigin::Windows)
                        .map(|agent| sided_discovered(agent, AgentSide::Wsl)),
                );
                connected.extend(
                    wsl_connected
                        .into_iter()
                        .map(|record| sided_connection(record, AgentSide::Wsl)),
                );
            }
            Err(error) => eprintln!("Agent Activity Dock: {error}"),
        }
    }

    #[cfg(not(windows))]
    {
        let manager = connection_manager(app);
        discovered.extend(
            manager
                .discover()
                .into_iter()
                .filter(|agent| agent.origin != AgentOrigin::Windows)
                .map(|agent| sided_discovered(agent, AgentSide::Wsl)),
        );
        connected.extend(
            manager
                .records()
                .into_iter()
                .map(|record| sided_connection(record, AgentSide::Wsl)),
        );
    }

    AgentInventory {
        discovered,
        connected,
    }
}

fn sided_discovered(agent: DiscoveredAgent, side: AgentSide) -> InventoryAgent {
    InventoryAgent {
        name: agent.name,
        path: agent.path,
        side,
    }
}

fn sided_connection(record: ConnectionRecord, side: AgentSide) -> InventoryConnection {
    InventoryConnection { record, side }
}

fn spawn_inventory_refresh(app: AppHandle) {
    if INVENTORY_REFRESH_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("dock-inventory-refresh".to_owned())
        .spawn(move || {
            let inventory = load_fresh_inventory(&app);
            publish_inventory(&app, inventory);
            INVENTORY_REFRESH_IN_FLIGHT.store(false, Ordering::SeqCst);
        });
}

#[tauri::command]
fn agent_inventory(app: AppHandle) -> AgentInventory {
    let cached = cached_inventory();
    spawn_inventory_refresh(app);
    cached
}

#[tauri::command]
fn refresh_agents(app: AppHandle) -> AgentInventory {
    let inventory = load_fresh_inventory(&app);
    publish_inventory(&app, inventory.clone());
    inventory
}

#[tauri::command]
fn preview_connect(
    app: AppHandle,
    name: String,
    original: PathBuf,
    side: String,
) -> Result<ConnectionPreview, String> {
    match AgentSide::parse(&side)? {
        AgentSide::Wsl => {
            #[cfg(windows)]
            {
                let _ = app;
                return wsl_session::preview_connect(&name, &original.to_string_lossy());
            }
            #[cfg(not(windows))]
            connection_manager(&app).preview(&name, &original)
        }
        AgentSide::Windows => {
            #[cfg(windows)]
            return connection_manager(&app).preview(&name, &original);
            #[cfg(not(windows))]
            {
                let _ = (app, name, original);
                Err("windows connections are only available on the Windows presenter".to_owned())
            }
        }
    }
}

#[tauri::command]
fn connect_agent(
    app: AppHandle,
    name: String,
    original: PathBuf,
    side: String,
) -> Result<ConnectionRecord, String> {
    match AgentSide::parse(&side)? {
        AgentSide::Wsl => {
            #[cfg(windows)]
            {
                let _ = app;
                return wsl_session::connect_agent(&name, &original.to_string_lossy());
            }
            #[cfg(not(windows))]
            connection_manager(&app).connect(&name, &original)
        }
        AgentSide::Windows => {
            #[cfg(windows)]
            return connection_manager(&app).connect(&name, &original);
            #[cfg(not(windows))]
            {
                let _ = (app, name, original);
                Err("windows connections are only available on the Windows presenter".to_owned())
            }
        }
    }
}

#[tauri::command]
fn run_alias() -> Result<Option<String>, String> {
    let local = agent_activity_dock_connect::current_run_alias();
    let remote = {
        #[cfg(windows)]
        {
            wsl_session::run_alias()
        }
        #[cfg(not(windows))]
        {
            Err("wsl unavailable".to_owned())
        }
    };
    Ok(agent_activity_dock_connect::preferred_run_alias(local, remote))
}

#[tauri::command]
fn set_run_alias(name: String) -> Result<Option<String>, String> {
    let parsed = if name.trim().is_empty() {
        None
    } else {
        Some(agent_activity_dock_connect::validate_run_alias(&name)?)
    };
    let local = agent_activity_dock_connect::set_run_alias(parsed.as_deref())?;
    #[cfg(windows)]
    {
        if let Err(error) = wsl_session::set_run_alias(parsed.as_deref()) {
            if !agent_activity_dock_connect::wsl_side_is_absent(&error) {
                eprintln!("Agent Activity Dock: WSL 启动别名未更新: {error}");
            }
        }
    }
    Ok(local)
}

#[tauri::command]
fn disconnect_agent(app: AppHandle, name: String, side: String) -> Result<bool, String> {
    match AgentSide::parse(&side)? {
        AgentSide::Wsl => {
            #[cfg(windows)]
            {
                let _ = app;
                return wsl_session::disconnect_agent(&name);
            }
            #[cfg(not(windows))]
            connection_manager(&app).disconnect(&name)
        }
        AgentSide::Windows => {
            #[cfg(windows)]
            return connection_manager(&app).disconnect(&name);
            #[cfg(not(windows))]
            {
                let _ = (app, name);
                Err("windows connections are only available on the Windows presenter".to_owned())
            }
        }
    }
}

#[tauri::command]
fn snapshot(state: State<'_, AppService>) -> Result<SnapshotView, String> {
    current_session(&state)?.snapshot()
}

#[tauri::command]
fn acknowledge(
    source: String,
    session_id: String,
    state: State<'_, AppService>,
) -> Result<SnapshotView, String> {
    current_session(&state)?.acknowledge(&source, &session_id)
}

#[tauri::command]
fn reset(
    source: String,
    session_id: String,
    state: State<'_, AppService>,
) -> Result<SnapshotView, String> {
    current_session(&state)?.reset(&source, &session_id)
}

#[tauri::command]
fn focus_source(
    source: String,
    session_id: String,
    terminal_id: Option<String>,
    deep_link: Option<String>,
    app: AppHandle,
) -> FocusResult {
    focus::focus_session(&source, &session_id, deep_link, terminal_id, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
fn set_notification_enabled(enabled: bool) {
    NOTIFICATIONS_ENABLED.store(enabled, Ordering::Relaxed);
}

#[tauri::command]
fn preview_notification(app: AppHandle) -> Result<(), String> {
    preview_attention_toast(&app)
}

#[tauri::command]
fn highlight_session(source: String, session_id: String, app: AppHandle) {
    open_and_highlight(&app, &source, &session_id);
}

#[tauri::command]
fn activate_attention(source: String, session_id: String, app: AppHandle) {
    activate_attention_session(&app, &source, &session_id);
}

pub(crate) fn activate_attention_session(app: &AppHandle, source: &str, session_id: &str) {
    let jump = app.try_state::<AppService>().and_then(|state| {
        let session = current_session(&state).ok()?;
        let snapshot = session.snapshot().ok()?;
        let sessions: Vec<AttentionJump> = snapshot
            .sessions
            .iter()
            .map(|session| AttentionJump {
                source: session.source.clone(),
                session_id: session.session_id.clone(),
                deep_link: session.deep_link.clone(),
                terminal_id: session.terminal_id.clone(),
            })
            .collect();
        attention_jump(&sessions, source, session_id)
    });
    if let Some(jump) = jump {
        let result = focus::focus_session(
            &jump.source,
            &jump.session_id,
            jump.deep_link,
            jump.terminal_id,
            |url| {
                app.opener()
                    .open_url(url, None::<&str>)
                    .map_err(|error| error.to_string())
            },
        );
        if attention_click_followup(result.focused) == AttentionClickFollowup::Stay {
            hide_panel_window(app);
            return;
        }
    }
    open_and_highlight(app, source, session_id);
}

fn open_and_highlight(app: &AppHandle, source: &str, session_id: &str) {
    show_panel(app);
    if let Some(target) = highlight_target(Some(source), Some(session_id)) {
        let _ = app.emit("dock:highlight", &target);
    }
}

#[tauri::command]
fn open_panel(app: AppHandle) {
    show_panel(&app);
}

#[tauri::command]
fn toggle_panel(app: AppHandle) {
    if let Some(panel) = app.get_webview_window("panel") {
        if panel.is_visible().unwrap_or(false) {
            hide_panel_window(&app);
            return;
        }
    }
    show_panel(&app);
}

fn show_panel(app: &AppHandle) {
    position_panel_near_ball(app);
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.show();
        let _ = panel.set_focus();
    }
    refresh_presenter_snapshot(app);
}

fn refresh_presenter_snapshot(app: &AppHandle) {
    let Some(state) = app.try_state::<AppService>() else {
        return;
    };
    let Ok(snapshot) = current_session(&state).and_then(|session| session.snapshot()) else {
        return;
    };
    let _ = app.emit("dock:snapshot", SnapshotMessage::snapshot(snapshot, None));
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    hide_panel_window(&app);
}

fn hide_panel_window(app: &AppHandle) {
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }
}

fn toggle_ball_visibility(app: &AppHandle) {
    if BALL_HIDDEN.load(Ordering::Relaxed) {
        show_ball_window(app);
    } else {
        hide_ball_window(app);
    }
}

fn hide_ball_window(app: &AppHandle) {
    BALL_HIDDEN.store(true, Ordering::Relaxed);
    if let Some(ball) = app.get_webview_window("ball") {
        let _ = ball.hide();
    }
    hide_panel_window(app);
    refresh_tray_ball_label(app);
}

fn show_ball_window(app: &AppHandle) {
    BALL_HIDDEN.store(false, Ordering::Relaxed);
    if let Some(ball) = app.get_webview_window("ball") {
        let _ = ball.show();
    }
    refresh_tray_ball_label(app);
}

fn refresh_tray_ball_label(app: &AppHandle) {
    if let Some(item) = app.try_state::<TrayBallItem>() {
        let _ = item
            .0
            .set_text(tray::ball_toggle_label(BALL_HIDDEN.load(Ordering::Relaxed)));
    }
}

fn start_local_session() -> (
    Arc<dyn PresenterSession>,
    mpsc::Receiver<SnapshotMessage>,
    SnapshotMessage,
) {
    agent_activity_dock_ipc::persist_default_backend_file();
    let session = attach_or_listen(
        agent_activity_dock_ipc::default_endpoint(),
        agent_activity_dock_ipc::default_state_path(),
        dockd_binary_path(),
    )
    .unwrap_or_else(|error| {
        eprintln!("Agent Activity Dock service failed to start: {error}");
        std::process::exit(1);
    });
    eprintln!(
        "Agent Activity Dock {} on {}",
        if session.owns_daemon() {
            "listening"
        } else {
            "attached"
        },
        session.endpoint().display()
    );
    let updates = session.subscribe();
    let initial = match session.snapshot() {
        Ok(snapshot) => SnapshotMessage::subscribed(snapshot),
        Err(_) => SnapshotMessage::subscribed(empty_snapshot()),
    };
    (
        Arc::new(session) as Arc<dyn PresenterSession>,
        updates,
        initial,
    )
}

#[cfg(windows)]
fn start_wsl_bridge_session() -> (
    Arc<dyn PresenterSession>,
    mpsc::Receiver<SnapshotMessage>,
    SnapshotMessage,
) {
    let session: Arc<dyn PresenterSession> = Arc::new(wsl_session::WslSession::connect());
    let updates = session.subscribe();
    let initial = match updates.recv_timeout(Duration::from_secs(8)) {
        Ok(message) => {
            eprintln!("Agent Activity Dock attached via WSL dock bridge");
            message
        }
        Err(_) => {
            eprintln!(
                "Agent Activity Dock: cannot reach WSL dock via wsl.exe. Install WSL and run `bash scripts/install-cli.sh`, or set AGENT_ACTIVITY_DOCK_BRIDGE_COMMAND"
            );
            SnapshotMessage::subscribed(empty_snapshot())
        }
    };
    (session, updates, initial)
}

#[cfg(windows)]
fn start_session() -> (
    Arc<dyn PresenterSession>,
    mpsc::Receiver<SnapshotMessage>,
    SnapshotMessage,
) {
    match presenter_backend() {
        DockBackend::Wsl => start_wsl_bridge_session(),
        DockBackend::Local => start_local_session(),
    }
}

#[cfg(not(windows))]
fn start_session() -> (
    Arc<dyn PresenterSession>,
    mpsc::Receiver<SnapshotMessage>,
    SnapshotMessage,
) {
    if presenter_backend() == DockBackend::Wsl {
        eprintln!("Agent Activity Dock: AGENT_ACTIVITY_DOCK_BACKEND=wsl is ignored on this OS");
    }
    start_local_session()
}

pub fn run() {
    let (session, updates, initial) = start_session();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_panel(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppService(Mutex::new(Some(Arc::clone(&session)))))
        .setup(move |app| {
            let app_handle = app.handle().clone();
            configure_windows(&app_handle);
            prepare_windows_notifications(&app_handle);
            install_windows_trampoline_cli(&app_handle);
            position_ball(&app_handle);
            region::apply_ball_region_for(&app_handle);
            install_tray(app);

            let handle = app.handle().clone();
            handle.emit("dock:snapshot", &initial)?;
            let mut previous_sessions = initial.snapshot.sessions.clone();
            std::thread::Builder::new()
                .name("dock-ui-updates".to_owned())
                .spawn(move || {
                    for update in updates {
                        focus::apply_snapshot_captures(
                            &previous_sessions,
                            &update.snapshot.sessions,
                        );
                        previous_sessions = update.snapshot.sessions.clone();
                        let attention = update.attention.clone();
                        if handle.emit("dock:snapshot", &update).is_err() {
                            break;
                        }
                        let sink = PresenterToastSink {
                            app: handle.clone(),
                        };
                        if let ToastDispatch::Failed { error, .. } = dispatch_attention_toast(
                            &sink,
                            attention.as_ref(),
                            NOTIFICATIONS_ENABLED.load(Ordering::Relaxed),
                        ) {
                            if !NOTIFICATION_FAIL_LOGGED.swap(true, Ordering::Relaxed) {
                                eprintln!(
                                    "Agent Activity Dock: cannot show system notification: {error}"
                                );
                            }
                        }
                    }
                })?;
            spawn_inventory_refresh(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            acknowledge,
            reset,
            agent_inventory,
            refresh_agents,
            preview_connect,
            connect_agent,
            disconnect_agent,
            open_panel,
            toggle_panel,
            hide_panel,
            focus_source,
            set_notification_enabled,
            preview_notification,
            highlight_session,
            activate_attention,
            run_alias,
            set_run_alias
        ])
        .build(tauri::generate_context!())
        .expect("error while building Agent Activity Dock")
        .run(move |app, event| match event {
            tauri::RunEvent::Exit => {
                if let Some(ball) = app.get_webview_window("ball") {
                    if let Ok(position) = ball.outer_position() {
                        save_ball_position(position);
                    }
                }
                if let Some(state) = app.try_state::<AppService>() {
                    if let Ok(mut service) = state.0.lock() {
                        if let Some(session) = service.take() {
                            session.request_shutdown();
                            session.wait_for_shutdown();
                        }
                    }
                }
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Moved(position),
                ..
            } if label == "ball" => {
                throttle_save_ball_position(position);
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Resized(_),
                ..
            } if label == "ball" => {
                region::apply_ball_region_for(app);
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::ScaleFactorChanged { .. },
                ..
            } if label == "ball" => {
                region::apply_ball_region_for(app);
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } if label == "ball" => {
                api.prevent_close();
                hide_panel_window(app);
            }
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Focused(true),
                ..
            } if label == "panel" => {
                refresh_presenter_snapshot(app);
            }
            _ => {}
        });
}

fn install_tray(app: &mut tauri::App) {
    let toggle = match MenuItem::with_id(
        app,
        "toggle-ball",
        tray::ball_toggle_label(false),
        true,
        None::<&str>,
    ) {
        Ok(item) => item,
        Err(error) => {
            eprintln!("Agent Activity Dock tray is unavailable: {error}");
            return;
        }
    };
    let show = match MenuItem::with_id(app, "show", "打开 Dock", true, None::<&str>) {
        Ok(item) => item,
        Err(error) => {
            eprintln!("Agent Activity Dock tray is unavailable: {error}");
            return;
        }
    };
    let quit = match MenuItem::with_id(app, "quit", "退出", true, None::<&str>) {
        Ok(item) => item,
        Err(error) => {
            eprintln!("Agent Activity Dock tray is unavailable: {error}");
            return;
        }
    };
    let menu = match Menu::with_items(app, &[&toggle, &show, &quit]) {
        Ok(menu) => menu,
        Err(error) => {
            eprintln!("Agent Activity Dock tray is unavailable: {error}");
            return;
        }
    };
    app.manage(TrayBallItem(toggle));
    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Agent Activity Dock")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle-ball" => toggle_ball_visibility(app),
            "show" => show_panel(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    if let Err(error) = builder.build(app) {
        eprintln!("Agent Activity Dock tray is unavailable: {error}");
    }
}

fn configure_windows(app: &AppHandle) {
    for label in ["ball", "panel"] {
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        let _ = window.set_always_on_top(true);
        if label == "ball" {
            let size = tauri::LogicalSize::new(56.0, 56.0);
            let _ = window.set_size(tauri::Size::Logical(size));
        }
    }
}

fn position_ball(app: &AppHandle) {
    let Some(ball) = app.get_webview_window("ball") else {
        return;
    };
    let Some(monitor) = monitor_for(&ball) else {
        return;
    };
    let Ok(size) = ball.outer_size() else {
        return;
    };
    let position = load_saved_ball_position()
        .map(|saved| clamp_to_monitor(&monitor, size, saved.x, saved.y))
        .unwrap_or_else(|| {
            let margin = (24.0 * monitor.scale_factor()).round() as i32;
            let origin = monitor.position();
            let area = monitor.size();
            clamp_to_monitor(
                &monitor,
                size,
                origin.x + area.width as i32 - size.width as i32 - margin,
                origin.y + margin,
            )
        });
    let _ = ball.set_position(Position::Physical(position));
}

fn position_panel_near_ball(app: &AppHandle) {
    let Some(ball) = app.get_webview_window("ball") else {
        return;
    };
    let Some(panel) = app.get_webview_window("panel") else {
        return;
    };
    let Some(monitor) = monitor_for(&ball) else {
        return;
    };
    let Ok(ball_pos) = ball.outer_position() else {
        return;
    };
    let Ok(ball_size) = ball.outer_size() else {
        return;
    };
    let Ok(panel_size) = panel.outer_size() else {
        return;
    };
    let gap = (12.0 * monitor.scale_factor()).round() as i32;
    let (origin, work) = monitor_work_area(&monitor);
    let ball_center_x = ball_pos.x + ball_size.width as i32 / 2;
    let work_center_x = origin.x + work.width as i32 / 2;
    let x = if ball_center_x < work_center_x {
        ball_pos.x + ball_size.width as i32 + gap
    } else {
        ball_pos.x - panel_size.width as i32 - gap
    };
    let position = clamp_to_monitor(&monitor, panel_size, x, ball_pos.y);
    let _ = panel.set_position(Position::Physical(position));
}

fn monitor_for(window: &WebviewWindow) -> Option<tauri::Monitor> {
    window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| {
            window
                .available_monitors()
                .ok()
                .and_then(|monitors| monitors.into_iter().next())
        })
}

fn monitor_work_area(
    monitor: &tauri::Monitor,
) -> (PhysicalPosition<i32>, tauri::PhysicalSize<u32>) {
    let work = monitor.work_area();
    (work.position, work.size)
}

fn clamp_to_monitor(
    monitor: &tauri::Monitor,
    size: tauri::PhysicalSize<u32>,
    x: i32,
    y: i32,
) -> PhysicalPosition<i32> {
    let (origin, area) = monitor_work_area(monitor);
    let min_x = origin.x;
    let min_y = origin.y;
    let max_x = origin.x + area.width as i32 - size.width as i32;
    let max_y = origin.y + area.height as i32 - size.height as i32;
    PhysicalPosition::new(
        x.clamp(min_x, max_x.max(min_x)),
        y.clamp(min_y, max_y.max(min_y)),
    )
}

fn ball_position_path() -> PathBuf {
    agent_activity_dock_ipc::default_state_path().with_file_name("ball-position.json")
}

fn load_saved_ball_position() -> Option<PhysicalPosition<i32>> {
    let bytes = fs::read(ball_position_path()).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    Some(PhysicalPosition::new(
        value.get("x")?.as_i64()? as i32,
        value.get("y")?.as_i64()? as i32,
    ))
}

fn save_ball_position(position: PhysicalPosition<i32>) {
    let path = ball_position_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(
        path,
        serde_json::json!({ "x": position.x, "y": position.y }).to_string(),
    );
}

fn throttle_save_ball_position(position: PhysicalPosition<i32>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    let previous = LAST_BALL_SAVE_MS.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < 250 {
        return;
    }
    LAST_BALL_SAVE_MS.store(now, Ordering::Relaxed);
    save_ball_position(position);
}
