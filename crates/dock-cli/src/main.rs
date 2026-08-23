#![cfg_attr(windows, windows_subsystem = "windows")]

mod terminal;

use agent_activity_dock_adapters::{claude_hook, codex_notification, dsh_projection, grok_hook};
use agent_activity_dock_connect::{ConnectionManager, ConnectionPreview, PreviewAction};
use agent_activity_dock_core::{
    dock_tab_title, dock_terminal_marker, session_terminal_title, DockEvent, EventKind, Severity,
    EVENT_VERSION,
};
use agent_activity_dock_ipc::{
    default_endpoint, default_state_path, local_connect, local_set_recv_timeout,
    local_set_send_timeout, local_try_clone, persist_default_backend_file, resolve_backend,
    DockBackend, IpcRequest, SnapshotView, WireResponse,
};
#[cfg(not(windows))]
use agent_activity_dock_service::attach_or_listen;
use agent_activity_dock_service::connect_or_spawn_detached;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Duration;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[derive(Debug, Parser)]
#[command(name = "dock", version, about = "Agent Activity Dock event emitter")]
struct Cli {
    /// Override the current-user local socket path.
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    /// Print protocol JSON instead of the compact human summary.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Start(EventArgs),
    Working(EventArgs),
    Waiting(EventArgs),
    Permission(EventArgs),
    Complete(EventArgs),
    #[command(alias = "stop")]
    Completed(EventArgs),
    Fail(EventArgs),
    Error(EventArgs),
    Cancel(EventArgs),
    Status,
    Acknowledge(AcknowledgeArgs),
    /// Remove stale tracking state without sending a lifecycle event.
    #[command(alias = "clear")]
    Reset(AcknowledgeArgs),
    /// Translate one structured adapter payload from stdin and emit it.
    Hook {
        provider: HookProvider,
    },
    /// List detected and connected Agent tools.
    Agents,
    /// Connect one already-installed Agent without replacing it.
    Connect {
        name: String,
        #[arg(long)]
        original: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove only Dock-managed wrapper or hook artifacts.
    Disconnect {
        name: String,
    },
    /// Start the local Dock daemon. Works from any directory.
    Up,
    /// Stop the local Dock daemon.
    Down,
    /// Forward stdin/stdout NDJSON to the current-user Dock socket.
    Bridge,
    /// Receive one DockEvent JSON on stdin and send it to the local daemon.
    #[command(hide = true)]
    Emit,
    /// Report whether the given Linux PIDs are still the original processes.
    #[command(hide = true)]
    LivenessCheck,
    /// Start an Agent in a dedicated Windows Terminal tab.
    Run {
        /// Windows Terminal profile name or GUID. Defaults to the current tab.
        #[arg(long)]
        profile: Option<String>,
        /// Agent command, such as grok, claude, or codex.
        agent: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum HookProvider {
    Claude,
    Codex,
    Dsh,
    Grok,
}

#[derive(Debug, clap::Args)]
struct EventArgs {
    /// Stable session identifier from the Agent integration.
    session_id: String,
    /// Source integration name, such as claude, grok, codex or dsh.
    #[arg(long, default_value = "manual")]
    source: String,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long)]
    deep_link: Option<String>,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long)]
    workspace_root: Option<String>,
    #[arg(long)]
    window_title: Option<String>,
    #[arg(long)]
    requires_user_action: bool,
}

#[derive(Debug, clap::Args)]
struct AcknowledgeArgs {
    #[arg(long, default_value = "*")]
    source: String,
    #[arg(long, default_value = "*")]
    session_id: String,
}

fn main() {
    #[cfg(windows)]
    attach_parent_console();
    persist_default_backend_file();
    let cli = Cli::parse();
    let endpoint = cli
        .socket
        .clone()
        .or_else(|| std::env::var_os("AGENT_ACTIVITY_DOCK_SOCKET").map(PathBuf::from))
        .unwrap_or_else(default_endpoint);
    if should_forward_to_wsl(&cli.command, &endpoint) {
        std::process::exit(forward_to_wsl());
    }
    if should_trampoline_argv(&cli.command) {
        std::process::exit(trampoline_to_windows());
    }
    if matches!(
        &cli.command,
        Command::Agents | Command::Connect { .. } | Command::Disconnect { .. }
    ) {
        let status = run_connection_command(&cli.command, cli.json);
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    if matches!(&cli.command, Command::Up | Command::Down) {
        let status = run_daemon_command(&cli.command, &endpoint, cli.json);
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    if matches!(&cli.command, Command::Bridge) {
        let status = run_bridge(&endpoint);
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    if let Command::Run {
        agent,
        args,
        profile,
    } = &cli.command
    {
        let status = terminal::run_command(agent, args, profile.as_deref(), cli.json);
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    if matches!(&cli.command, Command::LivenessCheck) {
        let status = run_liveness_check();
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    if matches!(&cli.command, Command::Emit) {
        let status = run_emit(&endpoint, cli.json);
        if status != 0 {
            std::process::exit(status);
        }
        return;
    }
    if let Command::Hook { provider } = &cli.command {
        run_hook(*provider, &endpoint, cli.json);
        return;
    }
    let request = match request_for(&cli.command) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("dock: {error}");
            std::process::exit(2);
        }
    };
    if let IpcRequest::Event(event) = &request {
        if should_trampoline_to_windows(&cli.command) {
            std::process::exit(trampoline_emit(event));
        }
    }
    if let Err(status) = ensure_cli_daemon(&endpoint) {
        std::process::exit(status);
    }
    match send(&endpoint, &request) {
        Ok(response) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&response).expect("response serializes")
                );
            } else {
                print_summary(&response);
            }
            if !response.ok {
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("dock: cannot reach Dock at {}: {error}", endpoint.display());
            eprintln!("Start the daemon from any directory with: dock up");
            std::process::exit(2);
        }
    }
}

fn run_connection_command(command: &Command, json_output: bool) -> i32 {
    let dock_binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("dock"));
    let manager = ConnectionManager::from_environment(dock_binary);
    match command {
        Command::Agents => {
            let discovered = manager.discover();
            let connected = manager.records();
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "discovered": discovered,
                        "connected": connected,
                    }))
                    .expect("connection inventory serializes")
                );
            } else if discovered.is_empty() && connected.is_empty() {
                println!("No supported Agents found on PATH yet.");
            } else {
                for agent in discovered {
                    let status = if connected.iter().any(|record| record.name == agent.name) {
                        "connected"
                    } else {
                        "available"
                    };
                    if agent.origin == agent_activity_dock_connect::AgentOrigin::Windows {
                        println!(
                            "{} — {} ({status}, Windows PATH)",
                            agent.name,
                            agent.path.display()
                        );
                    } else {
                        println!("{} — {} ({status})", agent.name, agent.path.display());
                    }
                }
            }
            0
        }
        Command::Connect {
            name,
            original,
            dry_run,
        } => {
            let path = original.clone().or_else(|| {
                manager
                    .discover()
                    .into_iter()
                    .find(|agent| agent.name == *name)
                    .map(|agent| agent.path)
            });
            let Some(path) = path else {
                eprintln!("dock connect: {name} is not on PATH; pass --original");
                return 1;
            };
            if *dry_run {
                match manager.preview(name, &path) {
                    Ok(preview) if json_output => println!(
                        "{}",
                        serde_json::to_string_pretty(&preview)
                            .expect("connection preview serializes")
                    ),
                    Ok(preview) => print_connect_preview(&preview),
                    Err(error) => {
                        eprintln!("dock connect: {error}");
                        return 1;
                    }
                }
                return 0;
            }
            match manager.connect(name, &path) {
                Ok(record) if json_output => println!(
                    "{}",
                    serde_json::to_string_pretty(&record).expect("connection record serializes")
                ),
                Ok(record) => {
                    println!("Connected {} via {:?}.", record.name, record.method);
                    if !record.limitation.is_empty() {
                        println!("Limitation: {}", record.limitation);
                    }
                }
                Err(error) => {
                    eprintln!("dock connect: {error}");
                    return 1;
                }
            }
            0
        }
        Command::Disconnect { name } => {
            match manager.disconnect(name) {
                Ok(disconnected) if json_output => println!(
                    "{}",
                    serde_json::json!({ "name": name, "disconnected": disconnected })
                ),
                Ok(true) => println!("Disconnected {name}; the original Agent was not changed."),
                Ok(false) => println!("{name} was not connected."),
                Err(error) => {
                    eprintln!("dock disconnect: {error}");
                    return 1;
                }
            }
            0
        }
        _ => unreachable!("connection command dispatched separately"),
    }
}

fn run_hook(provider: HookProvider, endpoint: &PathBuf, json_output: bool) {
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("dock hook: cannot read stdin: {error}");
        return;
    }
    let payload: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("dock hook: invalid JSON ({error})");
            return;
        }
    };
    let event = match provider {
        HookProvider::Claude => claude_hook(&payload),
        HookProvider::Codex => codex_notification(&payload),
        HookProvider::Dsh => dsh_projection(&payload),
        HookProvider::Grok => grok_hook(&payload),
    };
    let Some(mut event) = event else {
        if json_output {
            println!("{{\"accepted\":false,\"rejection_reason\":\"unmapped_event\"}}");
        }
        return;
    };
    attach_terminal_id(&mut event);
    attach_liveness(&mut event);
    maybe_set_terminal_title(&event);
    if should_trampoline_to_windows(&Command::Hook { provider }) {
        std::process::exit(trampoline_emit(&event));
    }
    if let Err(status) = ensure_cli_daemon(endpoint) {
        std::process::exit(status);
    }
    match send(endpoint, &IpcRequest::Event(event)) {
        Ok(response) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string(&response).expect("response serializes")
                );
            }
        }
        Err(error) => eprintln!("dock hook: cannot reach Dock: {error}"),
    }
}

fn request_for(command: &Command) -> Result<IpcRequest, String> {
    let request = match command {
        Command::Status => IpcRequest::Snapshot,
        Command::Acknowledge(args) => IpcRequest::Acknowledge {
            source: args.source.clone(),
            session_id: args.session_id.clone(),
        },
        Command::Reset(args) => IpcRequest::Reset {
            source: args.source.clone(),
            session_id: args.session_id.clone(),
        },
        Command::Start(args) => event_request(args, EventKind::Started)?,
        Command::Working(args) => event_request(args, EventKind::Working)?,
        Command::Waiting(args) => event_request(args, EventKind::WaitingInput)?,
        Command::Permission(args) => event_request(args, EventKind::PermissionRequested)?,
        Command::Complete(args) | Command::Completed(args) => {
            event_request(args, EventKind::Completed)?
        }
        Command::Fail(args) | Command::Error(args) => event_request(args, EventKind::Failed)?,
        Command::Cancel(args) => event_request(args, EventKind::Cancelled)?,
        Command::Hook { .. } | Command::Emit | Command::LivenessCheck => {
            return Err("hook is handled before event parsing".to_owned())
        }
        Command::Agents
        | Command::Connect { .. }
        | Command::Disconnect { .. }
        | Command::Up
        | Command::Down
        | Command::Bridge
        | Command::Run { .. } => return Err("command is handled before event parsing".to_owned()),
    };
    Ok(request)
}

fn event_request(args: &EventArgs, kind: EventKind) -> Result<IpcRequest, String> {
    let event_id = format!(
        "dock-{}-{}-{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        args.session_id
    );
    let severity = match kind {
        EventKind::Failed => Severity::Error,
        EventKind::WaitingInput | EventKind::PermissionRequested => Severity::Attention,
        _ => Severity::Info,
    };
    let mut event =
        DockEvent::new(&event_id, kind, &args.source, &args.session_id).with_severity(severity);
    event.version = EVENT_VERSION;
    event.occurred_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| format!("cannot format event timestamp: {error}"))?;
    if let Some(summary) = &args.summary {
        event = event.with_summary(summary.clone());
    }
    if let Some(deep_link) = &args.deep_link {
        event.deep_link = Some(deep_link.clone());
    }
    if let Some(cwd) = &args.cwd {
        event.cwd = Some(cwd.clone());
    }
    if let Some(workspace_root) = &args.workspace_root {
        event.workspace_root = Some(workspace_root.clone());
    }
    if let Some(window_title) = &args.window_title {
        event.window_title = Some(window_title.clone());
    }
    if args.requires_user_action {
        event = event.requiring_user_action(true);
    }
    attach_terminal_id(&mut event);
    maybe_set_terminal_title(&event);
    Ok(IpcRequest::Event(event))
}

fn attach_terminal_id(event: &mut DockEvent) {
    if event.terminal_id.is_none() {
        event.terminal_id = resolve_terminal_id();
    }
}

fn attach_liveness(event: &mut DockEvent) {
    if event
        .parent_session_id
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return;
    }
    #[cfg(unix)]
    {
        let pid = unsafe { libc::getppid() };
        if pid <= 1 {
            return;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return;
        };
        let Some((_, _, starttime)) = parse_proc_stat(&stat) else {
            return;
        };
        event
            .metadata
            .insert("agent_os".to_owned(), "linux".to_owned());
        event
            .metadata
            .insert("agent_pid".to_owned(), pid.to_string());
        event
            .metadata
            .insert("agent_starttime".to_owned(), starttime.to_string());
        if let Ok(distro) = std::env::var("WSL_DISTRO_NAME") {
            let trimmed = distro.trim();
            if !trimmed.is_empty() {
                event
                    .metadata
                    .insert("agent_wsl_distro".to_owned(), trimmed.to_owned());
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some((pid, starttime)) = windows_parent_liveness() {
            event
                .metadata
                .insert("agent_os".to_owned(), "windows".to_owned());
            event
                .metadata
                .insert("agent_pid".to_owned(), pid.to_string());
            event
                .metadata
                .insert("agent_starttime".to_owned(), starttime.to_string());
        }
    }
}

#[cfg(windows)]
fn windows_parent_liveness() -> Option<(u32, u64)> {
    windows_parent_liveness_inner()
}

#[cfg(windows)]
fn windows_parent_liveness_inner() -> Option<(u32, u64)> {
    use std::mem::{size_of, zeroed};
    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcessId() -> u32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
        fn Process32FirstW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: isize, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut u64,
            exit: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
        fn GetLastError() -> u32;
    }
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const INVALID: isize = -1;
    unsafe {
        let current = GetCurrentProcessId();
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == 0 || snapshot == INVALID {
            return None;
        }
        let mut entry: ProcessEntry32W = zeroed();
        entry.dw_size = size_of::<ProcessEntry32W>() as u32;
        let mut parent = 0;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32_process_id == current {
                    parent = entry.th32_parent_process_id;
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        if parent == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, parent);
        if handle == 0 {
            let _ = GetLastError();
            return None;
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
        Some((parent, creation))
    }
}

fn linux_pid_is_dead(pid: u32, starttime: u64) -> Option<bool> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => {
            let parsed = parse_proc_stat(&stat);
            Some(match parsed {
                Some((_, _, recorded)) => recorded != starttime,
                None => true,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
        Err(_) => None,
    }
}

fn run_liveness_check() -> i32 {
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("dock liveness-check: cannot read stdin: {error}");
        return 2;
    }
    let queries: Vec<Value> = match serde_json::from_str(input.trim()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("dock liveness-check: invalid JSON ({error})");
            return 2;
        }
    };
    let mut dead = Vec::new();
    for query in queries {
        let Some(pid) = query
            .get("pid")
            .and_then(Value::as_u64)
            .map(|value| value as u32)
        else {
            continue;
        };
        let Some(starttime) = query.get("starttime").and_then(Value::as_u64) else {
            continue;
        };
        if linux_pid_is_dead(pid, starttime) != Some(true) {
            continue;
        }
        dead.push(serde_json::json!({
            "source": query.get("source").cloned().unwrap_or(Value::Null),
            "session_id": query.get("session_id").cloned().unwrap_or(Value::Null),
            "pid": pid,
            "starttime": starttime,
        }));
    }
    println!("{}", serde_json::json!({ "dead": dead }));
    0
}

fn resolve_terminal_id() -> Option<String> {
    match std::env::var("AGENT_ACTIVITY_DOCK_TERMINAL_ID") {
        Ok(value) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Err(_) => choose_terminal_id(
            None,
            own_tty_id().as_deref(),
            ancestor_tty_id().as_deref(),
            wt_session_id().as_deref(),
        ),
    }
}

fn choose_terminal_id(
    explicit: Option<&str>,
    own_tty: Option<&str>,
    ancestor_tty: Option<&str>,
    wt_session: Option<&str>,
) -> Option<String> {
    [explicit, own_tty, ancestor_tty, wt_session]
        .into_iter()
        .find_map(|value| value.map(str::trim).filter(|value| !value.is_empty()))
        .map(str::to_owned)
}

fn wt_session_id() -> Option<String> {
    #[cfg(windows)]
    {
        std::env::var("WT_SESSION")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn own_tty_id() -> Option<String> {
    #[cfg(unix)]
    {
        unix_tty_id()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn ancestor_tty_id() -> Option<String> {
    #[cfg(unix)]
    {
        unix_ancestor_tty_id()
    }
    #[cfg(not(unix))]
    {
        None
    }
}

fn maybe_set_terminal_title(event: &DockEvent) {
    if !should_set_terminal_title(event) {
        return;
    }
    write_terminal_title(&lifecycle_terminal_title(event));
}

fn lifecycle_terminal_title(event: &DockEvent) -> String {
    let path = event
        .workspace_root
        .as_deref()
        .or(event.cwd.as_deref())
        .filter(|value| !value.is_empty());
    match event.terminal_id.as_deref().and_then(dock_terminal_marker) {
        Some(marker) => dock_tab_title(&event.source, path, marker),
        None => session_terminal_title(&event.source, path),
    }
}

fn should_set_terminal_title(event: &DockEvent) -> bool {
    if title_setting_suppressed() {
        return false;
    }
    if event
        .parent_session_id
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return false;
    }
    matches!(
        event.kind,
        EventKind::Started
            | EventKind::Working
            | EventKind::Idle
            | EventKind::Completed
            | EventKind::Failed
            | EventKind::WaitingInput
            | EventKind::PermissionRequested
    )
}

fn title_setting_suppressed() -> bool {
    std::env::var("AGENT_ACTIVITY_DOCK_NO_TITLE")
        .ok()
        .is_some_and(|value| value == "1")
}

fn write_terminal_title(title: &str) {
    #[cfg(unix)]
    {
        let sequence = format!("\x1b]0;{title}\x07");
        if let Some(mut tty) = open_controlling_tty() {
            let _ = tty.write_all(sequence.as_bytes());
            let _ = tty.flush();
            return;
        }
        if let Some(path) = ancestor_tty_id() {
            if let Some(mut tty) = open_tty_write_only(&path) {
                let _ = tty.write_all(sequence.as_bytes());
                let _ = tty.flush();
            }
        }
    }
    #[cfg(windows)]
    {
        windows_console::set_title(title);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = title;
    }
}

#[cfg(windows)]
mod windows_console {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleTitleW(title: *const u16) -> i32;
    }

    pub fn set_title(title: &str) {
        let mut wide: Vec<u16> = std::ffi::OsStr::new(title).encode_wide().collect();
        wide.push(0);
        unsafe {
            let _ = SetConsoleTitleW(wide.as_ptr());
        }
    }
}

#[cfg(unix)]
fn open_controlling_tty() -> Option<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .ok()
}

#[cfg(unix)]
fn open_tty_write_only(path: impl AsRef<std::path::Path>) -> Option<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(path)
        .ok()
}

#[cfg(unix)]
fn ttyname_string(fd: i32) -> Option<String> {
    unsafe {
        let ptr = libc::ttyname(fd);
        if ptr.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(ptr)
            .to_str()
            .ok()
            .map(str::to_owned)
    }
}

#[cfg(unix)]
fn unix_tty_id() -> Option<String> {
    use std::os::fd::AsRawFd;
    ttyname_string(libc::STDERR_FILENO)
        .or_else(|| ttyname_string(libc::STDIN_FILENO))
        .or_else(|| open_controlling_tty().and_then(|file| ttyname_string(file.as_raw_fd())))
}

#[cfg(unix)]
fn canonical_tty_id(path: &std::path::Path) -> Option<String> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOCTTY)
        .open(path)
        .ok()
        .or_else(|| open_tty_write_only(path))?;
    if unsafe { libc::isatty(file.as_raw_fd()) } == 0 {
        return None;
    }
    ttyname_string(file.as_raw_fd())
}

#[cfg(unix)]
fn looks_like_tty_path(path: &str) -> bool {
    path != "/dev/tty"
        && (path.starts_with("/dev/pts/")
            || path
                .strip_prefix("/dev/tty")
                .is_some_and(|rest| !rest.is_empty()))
}

fn parse_proc_stat(contents: &str) -> Option<(i32, u32, u64)> {
    let end = contents.rfind(')')?;
    let mut fields = contents.get(end + 1..)?.split_whitespace();
    let _state = fields.next()?;
    let ppid = fields.next()?.parse().ok()?;
    let _pgrp = fields.next()?;
    let _session = fields.next()?;
    let tty_nr = fields.next()?.parse().ok()?;
    for _ in 0..14 {
        fields.next()?;
    }
    let starttime = fields.next()?.parse().ok()?;
    Some((ppid, tty_nr, starttime))
}

#[cfg(unix)]
fn new_decode_dev(dev: u32) -> (u32, u32) {
    let major = (dev & 0xfff00) >> 8;
    let minor = (dev & 0xff) | ((dev >> 12) & 0xfff00);
    (major, minor)
}

#[cfg(unix)]
fn tty_path_for_dev(major: u32, minor: u32) -> Option<String> {
    match major {
        136 => Some(format!("/dev/pts/{minor}")),
        137..=143 => Some(format!("/dev/pts/{}", (major - 136) * 256 + minor)),
        4 if minor < 64 => Some(format!("/dev/tty{minor}")),
        4 => Some(format!("/dev/ttyS{}", minor.saturating_sub(64))),
        _ => None,
    }
}

#[cfg(unix)]
fn path_from_tty_nr(tty_nr: u32) -> Option<std::path::PathBuf> {
    if tty_nr == 0 {
        return None;
    }
    let (major, minor) = new_decode_dev(tty_nr);
    let path = std::path::PathBuf::from(tty_path_for_dev(major, minor)?);
    path.exists().then_some(path)
}

#[cfg(unix)]
fn ancestor_fd_tty_path(pid: i32, fd: u8) -> Option<std::path::PathBuf> {
    let link = std::fs::read_link(format!("/proc/{pid}/fd/{fd}")).ok()?;
    let text = link.to_str()?;
    looks_like_tty_path(text).then_some(link)
}

#[cfg(unix)]
fn unix_ancestor_tty_id() -> Option<String> {
    let mut pid = unsafe { libc::getppid() };
    for _ in 0..10 {
        if pid <= 1 {
            break;
        }
        for fd in [0_u8, 1, 2] {
            if let Some(path) = ancestor_fd_tty_path(pid, fd) {
                if let Some(id) = canonical_tty_id(&path) {
                    return Some(id);
                }
            }
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            break;
        };
        let Some((ppid, tty_nr, _)) = parse_proc_stat(&stat) else {
            break;
        };
        if let Some(path) = path_from_tty_nr(tty_nr) {
            if let Some(id) = canonical_tty_id(&path) {
                return Some(id);
            }
        }
        if ppid == pid {
            break;
        }
        pid = ppid;
    }
    None
}

fn is_forwardable_command(command: &Command) -> bool {
    !matches!(
        command,
        Command::Agents
            | Command::Connect { .. }
            | Command::Disconnect { .. }
            | Command::Up
            | Command::Down
    )
}

fn stays_on_agent_os(command: &Command) -> bool {
    matches!(
        command,
        Command::Agents
            | Command::Connect { .. }
            | Command::Disconnect { .. }
            | Command::Run { .. }
            | Command::LivenessCheck
    )
}

fn explicit_wsl_forward() -> bool {
    std::env::var("AGENT_ACTIVITY_DOCK_FORWARD")
        .ok()
        .is_some_and(|value| value.eq_ignore_ascii_case("wsl"))
}

fn hop_token() -> Option<String> {
    std::env::var("AGENT_ACTIVITY_DOCK_HOP")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn looks_like_wsl() -> bool {
    env_nonempty("WSL_DISTRO_NAME")
        || env_nonempty("WSL_INTEROP")
        || env_nonempty("AGENT_ACTIVITY_DOCK_WINDOWS_DOCK")
}

fn env_nonempty(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn looks_like_windows_pipe(value: &OsString) -> bool {
    let text = value.to_string_lossy();
    text.starts_with(r"\\.\pipe\") || text.starts_with("//./pipe/")
}

fn local_daemon_present(endpoint: &Path) -> bool {
    local_connect(endpoint).is_ok()
}

fn forward_to_wsl_predicate(
    forwardable: bool,
    is_windows: bool,
    hop_set: bool,
    backend: DockBackend,
    explicit_forward: bool,
    pipe_present: bool,
) -> bool {
    if !is_windows || !forwardable || hop_set {
        return false;
    }
    match backend {
        DockBackend::Local => explicit_forward,
        DockBackend::Wsl => explicit_forward || !pipe_present,
    }
}

fn trampoline_to_windows_predicate(
    is_unix: bool,
    like_wsl: bool,
    hop_set: bool,
    backend: DockBackend,
    stays_on_agent_os: bool,
) -> bool {
    is_unix && like_wsl && !hop_set && backend == DockBackend::Local && !stays_on_agent_os
}

fn should_forward_to_wsl(command: &Command, endpoint: &Path) -> bool {
    let hop_set = hop_token().is_some();
    let backend = resolve_backend();
    if hop_set
        && cfg!(windows)
        && forward_to_wsl_predicate(
            is_forwardable_command(command),
            true,
            false,
            backend,
            explicit_wsl_forward(),
            local_daemon_present(endpoint),
        )
    {
        eprintln!("dock: refusing hop, AGENT_ACTIVITY_DOCK_HOP already set");
    }
    if backend == DockBackend::Local && explicit_wsl_forward() && !hop_set && cfg!(windows) {
        eprintln!("dock: AGENT_ACTIVITY_DOCK_FORWARD=wsl with BACKEND=local is unsupported");
    }
    forward_to_wsl_predicate(
        is_forwardable_command(command),
        cfg!(windows),
        hop_set,
        backend,
        explicit_wsl_forward(),
        local_daemon_present(endpoint),
    )
}

fn should_trampoline_to_windows(command: &Command) -> bool {
    let hop_set = hop_token().is_some();
    let would = trampoline_to_windows_predicate(
        cfg!(unix),
        looks_like_wsl(),
        false,
        resolve_backend(),
        stays_on_agent_os(command),
    );
    if hop_set && would {
        eprintln!("dock: refusing hop, AGENT_ACTIVITY_DOCK_HOP already set");
    }
    trampoline_to_windows_predicate(
        cfg!(unix),
        looks_like_wsl(),
        hop_set,
        resolve_backend(),
        stays_on_agent_os(command),
    )
}

fn should_trampoline_argv(command: &Command) -> bool {
    should_trampoline_to_windows(command) && !needs_local_event_prep(command)
}

fn needs_local_event_prep(command: &Command) -> bool {
    matches!(
        command,
        Command::Hook { .. }
            | Command::Start(_)
            | Command::Working(_)
            | Command::Waiting(_)
            | Command::Permission(_)
            | Command::Complete(_)
            | Command::Completed(_)
            | Command::Fail(_)
            | Command::Error(_)
            | Command::Cancel(_)
    )
}

fn inject_wsl_backend(command: &mut ProcessCommand, backend: DockBackend) {
    command.env("AGENT_ACTIVITY_DOCK_BACKEND", backend.as_str());
    let extra = "AGENT_ACTIVITY_DOCK_BACKEND/u";
    match std::env::var("WSLENV") {
        Ok(existing)
            if existing
                .split(':')
                .any(|part| part.starts_with("AGENT_ACTIVITY_DOCK_BACKEND")) => {}
        Ok(existing) if !existing.is_empty() => {
            command.env("WSLENV", format!("{existing}:{extra}"));
        }
        _ => {
            command.env("WSLENV", extra);
        }
    }
}

#[cfg(windows)]
fn attach_parent_console() {
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(pid: u32) -> i32;
    }
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn hide_windows_console(command: &mut ProcessCommand) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = command;
}

fn forward_to_wsl() -> i32 {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut command = ProcessCommand::new("wsl.exe");
    hide_windows_console(&mut command);
    if let Ok(distro) = std::env::var("AGENT_ACTIVITY_DOCK_WSL_DISTRO") {
        if !distro.is_empty() {
            command.args(["-d", &distro]);
        }
    }
    command.args([
        "-e",
        "sh",
        "-c",
        r#"exec "$HOME/.local/bin/dock" "$@""#,
        "sh",
    ]);
    command.args(&args);
    command.env_remove("AGENT_ACTIVITY_DOCK_FORWARD");
    command.env("AGENT_ACTIVITY_DOCK_HOP", "wsl");
    inject_wsl_backend(&mut command, DockBackend::Wsl);
    if std::env::var_os("AGENT_ACTIVITY_DOCK_SOCKET")
        .as_ref()
        .is_some_and(looks_like_windows_pipe)
    {
        command.env_remove("AGENT_ACTIVITY_DOCK_SOCKET");
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("dock: cannot forward to WSL via wsl.exe ({error})");
            2
        }
    }
}

fn apply_windows_hop_env(command: &mut ProcessCommand) {
    command.env("AGENT_ACTIVITY_DOCK_HOP", "windows");
    command.env("AGENT_ACTIVITY_DOCK_BACKEND", resolve_backend().as_str());
    command.env_remove("AGENT_ACTIVITY_DOCK_SOCKET");
    command.env_remove("XDG_RUNTIME_DIR");
}

fn trampoline_to_windows() -> i32 {
    let exe = match find_windows_dock() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("dock: {error}");
            return 2;
        }
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut command = ProcessCommand::new(&exe);
    command.args(&args);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    apply_windows_hop_env(&mut command);
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("dock: cannot trampoline to {}: {error}", exe.display());
            2
        }
    }
}

fn trampoline_emit(event: &DockEvent) -> i32 {
    let exe = match find_windows_dock() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("dock: {error}");
            return 2;
        }
    };
    let mut command = ProcessCommand::new(&exe);
    command.arg("emit");
    command.stdin(Stdio::piped());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    apply_windows_hop_env(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("dock: cannot trampoline to {}: {error}", exe.display());
            return 2;
        }
    };
    let Some(mut stdin) = child.stdin.take() else {
        eprintln!("dock: trampoline emit lost stdin");
        return 2;
    };
    match serde_json::to_vec(event) {
        Ok(mut line) => {
            line.push(b'\n');
            if let Err(error) = stdin.write_all(&line) {
                eprintln!("dock: cannot write emit payload: {error}");
                return 2;
            }
        }
        Err(error) => {
            eprintln!("dock: cannot serialize event: {error}");
            return 2;
        }
    }
    drop(stdin);
    match child.wait() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("dock: trampoline emit failed: {error}");
            2
        }
    }
}

fn find_windows_dock() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("AGENT_ACTIVITY_DOCK_WINDOWS_DOCK")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "AGENT_ACTIVITY_DOCK_WINDOWS_DOCK is not a file: {}",
            path.display()
        ));
    }
    if let Some(path) = cached_windows_dock() {
        return Ok(path);
    }
    Err(
        "cannot find Windows dock.exe; install the presenter, or set AGENT_ACTIVITY_DOCK_WINDOWS_DOCK"
            .to_owned(),
    )
}

fn cached_windows_dock() -> Option<PathBuf> {
    static CACHED: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHED.get_or_init(discover_windows_dock).clone()
}

fn discover_windows_dock() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(user) = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
    {
        candidates.push(PathBuf::from(format!(
            "/mnt/c/Users/{user}/AppData/Local/Agent Activity Dock/dock.exe"
        )));
    }
    if let Some(local) = agent_activity_dock_ipc::windows_app_data_dir() {
        candidates.push(local.join("Agent Activity Dock").join("dock.exe"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn run_emit(endpoint: &Path, json_output: bool) -> i32 {
    if let Err(status) = ensure_cli_daemon(endpoint) {
        return status;
    }
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("dock emit: cannot read stdin: {error}");
        return 2;
    }
    let event: DockEvent = match serde_json::from_str(input.trim()) {
        Ok(event) => event,
        Err(error) => {
            eprintln!("dock emit: invalid event JSON ({error})");
            return 2;
        }
    };
    match send(&endpoint.to_path_buf(), &IpcRequest::Event(event)) {
        Ok(response) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string(&response).expect("response serializes")
                );
            } else {
                print_summary(&response);
            }
            if response.ok {
                0
            } else {
                1
            }
        }
        Err(error) => {
            eprintln!(
                "dock emit: cannot reach Dock at {}: {error}",
                endpoint.display()
            );
            2
        }
    }
}

fn ensure_cli_daemon(endpoint: &Path) -> Result<(), i32> {
    if resolve_backend() != DockBackend::Local {
        return Ok(());
    }
    if looks_like_wsl() {
        return Ok(());
    }
    let dockd = dockd_binary();
    match connect_or_spawn_detached(
        endpoint,
        default_state_path(),
        dockd.is_file().then_some(dockd),
    ) {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("dock: {error}");
            Err(2)
        }
    }
}

fn send(endpoint: &PathBuf, request: &IpcRequest) -> Result<WireResponse, String> {
    let mut stream = local_connect(endpoint).map_err(|error| error.to_string())?;
    local_set_recv_timeout(&stream, Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    local_set_send_timeout(&stream, Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    let payload = match request {
        IpcRequest::Event(event) => serde_json::to_vec(event).map_err(|error| error.to_string())?,
        IpcRequest::Snapshot => br#"{"query":"snapshot"}"#.to_vec(),
        IpcRequest::Subscribe => br#"{"query":"subscribe"}"#.to_vec(),
        IpcRequest::Acknowledge { source, session_id } => serde_json::to_vec(
            &serde_json::json!({"query":"acknowledge", "source":source, "session_id":session_id}),
        )
        .map_err(|error| error.to_string())?,
        IpcRequest::Reset { source, session_id } => serde_json::to_vec(
            &serde_json::json!({"query":"reset", "source":source, "session_id":session_id}),
        )
        .map_err(|error| error.to_string())?,
    };
    let mut line = payload;
    line.push(b'\n');
    stream.write_all(&line).map_err(|error| error.to_string())?;
    let mut line = String::new();
    BufReader::new(stream)
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    serde_json::from_str(&line).map_err(|error| error.to_string())
}

fn print_connect_preview(preview: &ConnectionPreview) {
    println!(
        "Would connect {} from {} using a revocable user-level {}.",
        preview.name,
        preview.original.display(),
        connect_method_label(&preview.name)
    );
    println!("Files:");
    for file in &preview.files {
        let action = match file.action {
            PreviewAction::Create => "create",
            PreviewAction::Modify => "modify",
        };
        println!("  {action}  {}", file.path.display());
        for entry in &file.entries {
            println!("    - {entry}");
        }
    }
    println!("Will not:");
    for line in &preview.will_not {
        println!("  - {line}");
    }
    if !preview.notes.is_empty() {
        println!("Notes:");
        for note in &preview.notes {
            println!("  - {note}");
        }
    }
}

fn connect_method_label(name: &str) -> &'static str {
    match name {
        "claude" | "grok" => "native hook",
        _ => "wrapper",
    }
}

fn run_daemon_command(command: &Command, endpoint: &Path, json_output: bool) -> i32 {
    match command {
        Command::Up => start_daemon(endpoint, json_output),
        Command::Down => stop_daemon(endpoint, json_output),
        _ => unreachable!("daemon command dispatched separately"),
    }
}

fn start_daemon(endpoint: &Path, json_output: bool) -> i32 {
    if let Ok(response) = send(&endpoint.to_path_buf(), &IpcRequest::Snapshot) {
        if json_output {
            println!(
                "{}",
                serde_json::json!({"ok":true,"already_running":true,"snapshot":response.snapshot})
            );
        } else {
            println!("Dock daemon already running");
            print_summary(&response);
        }
        return 0;
    }

    let dockd = dockd_binary();
    if !dockd.is_file() {
        eprintln!(
            "dock up: cannot find dockd at {}. Start the presenter or install dockd.exe.",
            dockd.display()
        );
        return 1;
    }

    let state_dir = runtime_state_dir();
    if let Err(error) = fs::create_dir_all(&state_dir) {
        eprintln!("dock up: {error}");
        return 1;
    }
    let log_path = state_dir.join("dockd.log");
    let log = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("dock up: cannot open {}: {error}", log_path.display());
            return 1;
        }
    };
    let log_err = match log.try_clone() {
        Ok(file) => file,
        Err(error) => {
            eprintln!("dock up: {error}");
            return 1;
        }
    };

    let mut command = ProcessCommand::new(&dockd);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    match command.spawn() {
        Ok(child) => {
            let _ = fs::write(state_dir.join("dockd.pid"), child.id().to_string());
        }
        Err(error) => {
            eprintln!("dock up: failed to start {}: {error}", dockd.display());
            return 1;
        }
    }

    for _ in 0..25 {
        if let Ok(response) = send(&endpoint.to_path_buf(), &IpcRequest::Snapshot) {
            if json_output {
                println!(
                    "{}",
                    serde_json::json!({"ok":true,"already_running":false,"snapshot":response.snapshot})
                );
            } else {
                println!("dockd ready");
                print_summary(&response);
            }
            return 0;
        }
        thread::sleep(Duration::from_millis(80));
    }
    eprintln!(
        "dock up: daemon did not become ready; see {}",
        log_path.display()
    );
    1
}

fn stop_daemon(endpoint: &Path, json_output: bool) -> i32 {
    let state_dir = runtime_state_dir();
    let pid_path = state_dir.join("dockd.pid");
    if let Ok(pid_text) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_text.trim().parse::<u32>() {
            #[cfg(unix)]
            {
                let _ = ProcessCommand::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
            #[cfg(windows)]
            {
                let _ = ProcessCommand::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/F"])
                    .status();
            }
        }
        let _ = fs::remove_file(&pid_path);
    }
    #[cfg(unix)]
    {
        let _ = ProcessCommand::new("pkill").args(["-x", "dockd"]).status();
    }
    thread::sleep(Duration::from_millis(200));
    if send(&endpoint.to_path_buf(), &IpcRequest::Snapshot).is_ok() {
        eprintln!(
            "dock down: daemon is still reachable at {}",
            endpoint.display()
        );
        return 1;
    }
    if json_output {
        println!("{{\"ok\":true}}");
    } else {
        println!("Dock daemon stopped.");
    }
    0
}

fn run_bridge(endpoint: &Path) -> i32 {
    let dockd = dockd_binary();
    let dockd = dockd.is_file().then_some(dockd);
    #[cfg(windows)]
    {
        if let Err(error) = connect_or_spawn_detached(endpoint, default_state_path(), dockd) {
            eprintln!("dock bridge: {error}");
            return 2;
        }
        return match forward_stdio(endpoint) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("dock bridge: {error}");
                2
            }
        };
    }
    #[cfg(not(windows))]
    {
        let session = match attach_or_listen(endpoint, default_state_path(), dockd) {
            Ok(session) => session,
            Err(error) => {
                eprintln!("dock bridge: {error}");
                return 2;
            }
        };
        let status = match forward_stdio(endpoint) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("dock bridge: {error}");
                2
            }
        };
        if session.owns_daemon() {
            session.request_shutdown();
            session.wait_for_shutdown();
        }
        status
    }
}

fn forward_stdio(endpoint: &Path) -> Result<(), String> {
    let stream = local_connect(endpoint).map_err(|error| error.to_string())?;
    let mut writer = local_try_clone(&stream).map_err(|error| error.to_string())?;
    let writer_thread = thread::spawn(move || -> Result<(), String> {
        let mut stdin = BufReader::new(std::io::stdin());
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = stdin
                .read_until(b'\n', &mut line)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            writer.write_all(&line).map_err(|error| error.to_string())?;
        }
        Ok(())
    });
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let reader_thread = thread::spawn(move || -> Result<(), String> {
        let mut reader = BufReader::new(stream);
        let mut stdout = std::io::stdout();
        let mut line = Vec::new();
        let result = loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break Ok(()),
                Ok(_) => {
                    if stdout.write_all(&line).is_err() || stdout.flush().is_err() {
                        break Ok(());
                    }
                }
                Err(error) => break Err(error.to_string()),
            }
        };
        let _ = done_tx.send(());
        result
    });
    let write_result = writer_thread
        .join()
        .unwrap_or_else(|_| Err("stdin forwarder panicked".to_owned()));
    // One-shot queries close stdin after the request; wait briefly so the
    // response can still reach stdout. A live subscribe keeps stdin open.
    let _ = done_rx.recv_timeout(Duration::from_secs(2));
    let _ = reader_thread;
    write_result
}

fn dockd_binary() -> PathBuf {
    let file_name = if cfg!(windows) { "dockd.exe" } else { "dockd" };
    let mut candidates = Vec::new();
    if let Some(path) =
        std::env::var_os("AGENT_ACTIVITY_DOCK_DOCKD").filter(|value| !value.is_empty())
    {
        candidates.push(PathBuf::from(path));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(file_name));
            candidates.push(dir.join("binaries").join(file_name));
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
    if let Some(found) = candidates.into_iter().find(|path| path.is_file()) {
        return found;
    }
    PathBuf::from(file_name)
}

fn runtime_state_dir() -> PathBuf {
    default_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::{
        forward_to_wsl_predicate, is_forwardable_command, looks_like_windows_pipe,
        stays_on_agent_os, trampoline_to_windows_predicate, Command,
    };
    use agent_activity_dock_core::{DockEvent, EventKind};
    use agent_activity_dock_ipc::DockBackend;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn run_command() -> Command {
        Command::Run {
            profile: None,
            agent: "grok".to_owned(),
            args: Vec::new(),
        }
    }

    fn connect_command() -> Command {
        Command::Connect {
            name: "grok".to_owned(),
            original: None,
            dry_run: false,
        }
    }

    #[test]
    fn event_and_query_commands_can_forward() {
        assert!(is_forwardable_command(&Command::Status));
        assert!(is_forwardable_command(&run_command()));
        assert!(!is_forwardable_command(&Command::Agents));
        assert!(!is_forwardable_command(&Command::Up));
        assert!(!is_forwardable_command(&Command::Down));
    }

    #[test]
    fn hop_blocks_empty_pipe_forward() {
        assert!(!forward_to_wsl_predicate(
            true,
            true,
            true,
            DockBackend::Wsl,
            false,
            false,
        ));
        assert!(forward_to_wsl_predicate(
            true,
            true,
            false,
            DockBackend::Wsl,
            false,
            false,
        ));
        assert!(!forward_to_wsl_predicate(
            true,
            true,
            false,
            DockBackend::Local,
            false,
            false,
        ));
        assert!(forward_to_wsl_predicate(
            true,
            true,
            false,
            DockBackend::Local,
            true,
            false,
        ));
        assert!(!forward_to_wsl_predicate(
            true,
            false,
            false,
            DockBackend::Wsl,
            true,
            false,
        ));
    }

    #[test]
    fn run_and_agents_do_not_trampoline() {
        assert!(stays_on_agent_os(&Command::Agents));
        assert!(stays_on_agent_os(&connect_command()));
        assert!(stays_on_agent_os(&Command::Disconnect {
            name: "grok".to_owned()
        }));
        assert!(stays_on_agent_os(&run_command()));
        assert!(!stays_on_agent_os(&Command::Status));
        assert!(!stays_on_agent_os(&Command::Up));
        assert!(!stays_on_agent_os(&Command::Bridge));
        assert!(!stays_on_agent_os(&Command::Emit));
        assert!(stays_on_agent_os(&Command::LivenessCheck));
    }

    #[test]
    fn wrapper_event_path_does_not_attach_liveness() {
        let request = super::event_request(
            &super::EventArgs {
                session_id: "s1".to_owned(),
                source: "grok".to_owned(),
                summary: None,
                deep_link: None,
                cwd: None,
                workspace_root: None,
                window_title: None,
                requires_user_action: false,
            },
            EventKind::Started,
        )
        .unwrap();
        let agent_activity_dock_ipc::IpcRequest::Event(event) = request else {
            panic!("expected event");
        };
        assert!(!event.metadata.contains_key("agent_pid"));
        assert!(!event.metadata.contains_key("agent_os"));
        assert!(trampoline_to_windows_predicate(
            true,
            true,
            false,
            DockBackend::Local,
            false,
        ));
        assert!(!trampoline_to_windows_predicate(
            true,
            true,
            false,
            DockBackend::Local,
            true,
        ));
        assert!(!trampoline_to_windows_predicate(
            true,
            true,
            true,
            DockBackend::Local,
            false,
        ));
        assert!(!trampoline_to_windows_predicate(
            true,
            true,
            false,
            DockBackend::Wsl,
            false,
        ));
        assert!(!trampoline_to_windows_predicate(
            true,
            false,
            false,
            DockBackend::Local,
            false,
        ));
    }

    #[test]
    fn non_windows_builds_do_not_forward() {
        let _guard = lock_env();
        let previous = std::env::var_os("AGENT_ACTIVITY_DOCK_FORWARD");
        std::env::set_var("AGENT_ACTIVITY_DOCK_FORWARD", "wsl");
        let forwarded = super::should_forward_to_wsl(
            &Command::Status,
            std::path::Path::new("/tmp/unused.sock"),
        );
        restore_env("AGENT_ACTIVITY_DOCK_FORWARD", previous);
        if !cfg!(windows) {
            assert!(!forwarded);
        }
    }

    #[test]
    fn terminal_id_order_prefers_explicit_then_own_then_ancestor_then_wt() {
        assert_eq!(
            super::choose_terminal_id(
                Some("pts-override"),
                Some("/dev/pts/1"),
                Some("/dev/pts/2"),
                Some("wt-guid")
            )
            .as_deref(),
            Some("pts-override")
        );
        assert_eq!(
            super::choose_terminal_id(
                None,
                Some("/dev/pts/1"),
                Some("/dev/pts/2"),
                Some("wt-guid")
            )
            .as_deref(),
            Some("/dev/pts/1")
        );
        assert_eq!(
            super::choose_terminal_id(None, None, Some("/dev/pts/5"), Some("wt-guid")).as_deref(),
            Some("/dev/pts/5")
        );
        assert_eq!(
            super::choose_terminal_id(None, None, None, Some("wt-guid")).as_deref(),
            Some("wt-guid")
        );
    }

    #[test]
    fn terminal_id_env_override_wins() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::set_var("AGENT_ACTIVITY_DOCK_TERMINAL_ID", "pts-override");
        std::env::set_var("WT_SESSION", "wt-should-lose");
        let resolved = super::resolve_terminal_id();
        restore_env("AGENT_ACTIVITY_DOCK_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        assert_eq!(resolved.as_deref(), Some("pts-override"));
    }

    #[test]
    fn terminal_id_own_tty_beats_wt_session() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::remove_var("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        std::env::set_var("WT_SESSION", "wt-should-lose");
        let own = super::own_tty_id();
        let resolved = super::resolve_terminal_id();
        restore_env("AGENT_ACTIVITY_DOCK_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        if let Some(tty) = own {
            assert_eq!(resolved.as_deref(), Some(tty.as_str()));
            assert_ne!(resolved.as_deref(), Some("wt-should-lose"));
        }
    }

    #[test]
    fn terminal_id_falls_back_to_tty_without_env_ids() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::remove_var("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        std::env::remove_var("WT_SESSION");
        let resolved = super::resolve_terminal_id();
        let expected = super::own_tty_id().or_else(super::ancestor_tty_id);
        restore_env("AGENT_ACTIVITY_DOCK_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn unix_ignores_wt_session_as_terminal_id() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::remove_var("AGENT_ACTIVITY_DOCK_TERMINAL_ID");
        std::env::set_var("WT_SESSION", "wt-should-lose");
        let resolved = super::resolve_terminal_id();
        restore_env("AGENT_ACTIVITY_DOCK_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        if !cfg!(windows) {
            assert_ne!(resolved.as_deref(), Some("wt-should-lose"));
        }
    }

    fn restore_env(key: &str, previous: Option<OsString>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn windows_pipe_paths_are_detected() {
        assert!(looks_like_windows_pipe(&OsString::from(
            r"\\.\pipe\agent-activity-dock"
        )));
        assert!(looks_like_windows_pipe(&OsString::from(
            "//./pipe/agent-activity-dock"
        )));
        assert!(!looks_like_windows_pipe(&OsString::from(
            "/tmp/agent-activity-dock.sock"
        )));
    }

    #[test]
    fn title_env_skips_setting() {
        let _guard = lock_env();
        let previous = std::env::var_os("AGENT_ACTIVITY_DOCK_NO_TITLE");
        std::env::set_var("AGENT_ACTIVITY_DOCK_NO_TITLE", "1");
        let skipped = super::title_setting_suppressed();
        restore_env("AGENT_ACTIVITY_DOCK_NO_TITLE", previous);
        assert!(skipped);
    }

    #[test]
    fn title_only_for_main_session_lifecycle() {
        let _guard = lock_env();
        let previous = std::env::var_os("AGENT_ACTIVITY_DOCK_NO_TITLE");
        std::env::remove_var("AGENT_ACTIVITY_DOCK_NO_TITLE");
        let started = DockEvent::new("e1", EventKind::Started, "grok", "s1");
        let parent = DockEvent::new("e2", EventKind::Working, "grok", "s2")
            .with_parent_session_id("parent-1");
        let completed = DockEvent::new("e3", EventKind::Completed, "grok", "s3");
        let cancelled = DockEvent::new("e4", EventKind::Cancelled, "grok", "s4");
        let waiting = DockEvent::new("e5", EventKind::WaitingInput, "grok", "s5");
        let allow_started = super::should_set_terminal_title(&started);
        let allow_parent = super::should_set_terminal_title(&parent);
        let allow_completed = super::should_set_terminal_title(&completed);
        let allow_cancelled = super::should_set_terminal_title(&cancelled);
        let allow_waiting = super::should_set_terminal_title(&waiting);
        match previous {
            Some(value) => std::env::set_var("AGENT_ACTIVITY_DOCK_NO_TITLE", value),
            None => std::env::remove_var("AGENT_ACTIVITY_DOCK_NO_TITLE"),
        }
        assert!(allow_started);
        assert!(!allow_parent);
        assert!(allow_completed);
        assert!(allow_waiting);
        assert!(!allow_cancelled);
    }

    #[test]
    fn lifecycle_title_is_project_then_agent_then_dock_marker() {
        let mut with_marker = DockEvent::new("e1", EventKind::Started, "grok", "s1");
        with_marker.cwd = Some("/home/qingz/projects/agent-activity-dock".to_owned());
        with_marker.terminal_id = Some("dock:ab12cd".to_owned());
        assert_eq!(
            super::lifecycle_terminal_title(&with_marker),
            "agent-activity-dock · grok · dock:ab12cd"
        );

        let mut without_marker = DockEvent::new("e2", EventKind::Started, "grok", "s2");
        without_marker.cwd = Some("/home/qingz/projects/agent-activity-dock".to_owned());
        assert_eq!(
            super::lifecycle_terminal_title(&without_marker),
            "agent-activity-dock · grok"
        );

        let mut marker_only = DockEvent::new("e3", EventKind::Started, "claude", "s3");
        marker_only.terminal_id = Some("dock:00ffaa".to_owned());
        assert_eq!(
            super::lifecycle_terminal_title(&marker_only),
            "claude · dock:00ffaa"
        );
    }

    #[cfg(unix)]
    #[test]
    fn proc_stat_tty_nr_decodes_like_unix98_pts() {
        let line = "35022 (grok) S 1000 35022 35022 34821 35022 0 0 0 0 0 0 0 0 0 0 0 0 0 12345";
        assert_eq!(super::parse_proc_stat(line), Some((1000, 34821, 12345)));
        assert_eq!(super::new_decode_dev(34821), (136, 5));
        assert_eq!(
            super::tty_path_for_dev(136, 5).as_deref(),
            Some("/dev/pts/5")
        );
        let spaced = "12 (my (weird) name) R 99 12 12 0 12 0 0 0 0 0 0 0 0 0 0 0 0 0 4242";
        assert_eq!(super::parse_proc_stat(spaced), Some((99, 0, 4242)));
    }
}

fn print_summary(response: &WireResponse) {
    let snapshot: &SnapshotView = &response.snapshot;
    if response.accepted {
        println!(
            "accepted — {} · pending {}{}",
            snapshot.count_label,
            snapshot.pending_count,
            if snapshot.pending_mark.is_empty() {
                String::new()
            } else {
                format!(" {}", snapshot.pending_mark)
            }
        );
    } else {
        println!(
            "rejected: {}",
            response.rejection_reason.as_deref().unwrap_or("unknown")
        );
    }
    for session in &snapshot.sessions {
        let marker = if session.mark.is_empty() {
            " "
        } else {
            session.mark.as_str()
        };
        let state = format!("{:?}", session.state).to_lowercase();
        println!(
            "  [{marker}] {}:{} — {state}",
            session.source, session.session_id
        );
    }
}
