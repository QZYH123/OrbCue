use agent_activity_dock_adapters::{claude_hook, codex_notification, dsh_projection, grok_hook};
use agent_activity_dock_connect::ConnectionManager;
use agent_activity_dock_core::{DockEvent, EventKind, Severity, EVENT_VERSION};
use agent_activity_dock_ipc::{
    default_endpoint, default_state_path, local_connect, local_set_recv_timeout,
    local_set_send_timeout, local_try_clone, IpcRequest, SnapshotView, WireResponse,
};
use agent_activity_dock_service::attach_or_listen;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;
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
    let cli = Cli::parse();
    let endpoint = cli
        .socket
        .or_else(|| std::env::var_os("AGENT_ACTIVITY_DOCK_SOCKET").map(PathBuf::from))
        .unwrap_or_else(default_endpoint);
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
                let discovered = discovered
                    .iter()
                    .map(|agent| serde_json::json!({"name":agent.name,"path":agent.path}))
                    .collect::<Vec<_>>();
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
                    println!("{} — {} ({status})", agent.name, agent.path.display());
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
                if json_output {
                    println!(
                        "{}",
                        serde_json::json!({
                            "name": name,
                            "original": path,
                            "method": connect_method_name(name),
                            "dry_run": true,
                        })
                    );
                } else {
                    println!(
                        "Would connect {name} from {} using a revocable user-level {}.",
                        path.display(),
                        connect_method_label(name)
                    );
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
    let Some(event) = event else {
        if json_output {
            println!("{{\"accepted\":false,\"rejection_reason\":\"unmapped_event\"}}");
        }
        return;
    };
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
        Command::Hook { .. } => return Err("hook is handled before event parsing".to_owned()),
        Command::Agents
        | Command::Connect { .. }
        | Command::Disconnect { .. }
        | Command::Up
        | Command::Down
        | Command::Bridge => return Err("command is handled before event parsing".to_owned()),
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
    if args.requires_user_action {
        event = event.requiring_user_action(true);
    }
    Ok(IpcRequest::Event(event))
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

fn connect_method_name(name: &str) -> &'static str {
    match name {
        "claude" => "ClaudeHook",
        "grok" => "GrokHook",
        _ => "Wrapper",
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
            "dock up: cannot find dockd at {} (install with scripts/install-cli.sh)",
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
    let session = match attach_or_listen(
        endpoint,
        default_state_path(),
        dockd.is_file().then_some(dockd),
    ) {
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
    if let Some(path) =
        std::env::var_os("AGENT_ACTIVITY_DOCK_DOCKD").filter(|value| !value.is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(if cfg!(windows) { "dockd.exe" } else { "dockd" });
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from(if cfg!(windows) { "dockd.exe" } else { "dockd" })
}

fn runtime_state_dir() -> PathBuf {
    default_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
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
