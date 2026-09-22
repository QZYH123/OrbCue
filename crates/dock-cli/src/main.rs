#![cfg_attr(windows, windows_subsystem = "windows")]

mod hop;
mod liveness;
mod terminal;

use clap::{Parser, Subcommand, ValueEnum};
use hop::{
    looks_like_wsl, should_delegate_run_to_windows, should_trampoline_argv,
    should_trampoline_to_windows, trampoline_to_windows,
};
#[cfg(test)]
use hop::{newest_windows_dock_under_mnt, stays_on_agent_os, trampoline_to_windows_predicate};
#[cfg(unix)]
use liveness::is_short_lived_hook_parent;
#[cfg(test)]
use liveness::liveness_from_env;
use liveness::{attach_liveness, platform_parent_liveness, run_liveness_check};
#[cfg(any(windows, test))]
use liveness::{is_short_lived_windows_hook_parent, resolve_windows_liveness_pid};
#[cfg(windows)]
use liveness::{windows_process_tree, WindowsProcess};
use orbcue_adapters::{claude_hook, codex_hook, cursor_hook, grok_hook};
use orbcue_connect::{ConnectionManager, ConnectionMethod, ConnectionPreview, PreviewAction};
use orbcue_core::{
    dock_tab_title, dock_terminal_marker, session_terminal_title, DockEvent, EventKind, Severity,
    EVENT_VERSION,
};
#[cfg(unix)]
use orbcue_ipc::parse_proc_stat;
use orbcue_ipc::{
    default_endpoint, default_state_path, encode_request, local_connect, local_set_recv_timeout,
    local_set_send_timeout, IpcRequest, SnapshotView, WireResponse, WINDOWS_APP_FOLDER,
};
use orbcue_service::connect_or_spawn_detached;
use serde_json::Value;
#[cfg(windows)]
use std::collections::HashMap;
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
#[command(name = "orb", version, about = "OrbCue event emitter")]
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
pub(crate) enum Command {
    Start(EventArgs),
    Working(EventArgs),
    Waiting(EventArgs),
    Permission(EventArgs),
    #[command(aliases = ["completed", "stop"])]
    Complete(EventArgs),
    #[command(alias = "error")]
    Fail(EventArgs),
    Cancel(EventArgs),
    Status,
    Acknowledge(AcknowledgeArgs),
    /// Remove stale tracking state without sending a lifecycle event.
    #[command(alias = "clear")]
    Reset(AcknowledgeArgs),
    /// Translate one structured adapter payload from stdin and emit it.
    Hook {
        provider: HookProvider,
        /// Return immediately and deliver the event from a child process.
        #[arg(long, hide = true)]
        detach: bool,
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
    /// Receive one DockEvent JSON on stdin and send it to the local daemon.
    #[command(hide = true)]
    Emit,
    /// Report whether the given Linux PIDs are still the original processes.
    #[command(hide = true)]
    LivenessCheck,
    /// Install or remove a short command for `orb run`.
    Alias {
        /// Command name. Omit to print the current alias.
        name: Option<String>,
        /// Remove the current alias.
        #[arg(long)]
        clear: bool,
    },
    /// Persist whether `orb run` replaces the current tab. Used by the panel.
    #[command(hide = true, name = "replace-tab")]
    ReplaceTab {
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
    },
    /// Start an Agent in a dedicated Windows Terminal tab.
    Run {
        /// Windows Terminal profile name or GUID. Defaults to the current tab.
        #[arg(long)]
        profile: Option<String>,
        /// After the new tab starts, close this interactive shell tab.
        #[arg(long)]
        close: bool,
        /// Hidden: Windows orb.exe consumes a WSL-prepared run spec on stdin.
        #[arg(long, hide = true)]
        from_wsl: bool,
        /// Agent command, such as grok, claude, or codex.
        #[arg(required_unless_present = "from_wsl")]
        agent: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum HookProvider {
    Claude,
    Codex,
    Cursor,
    Grok,
}

#[derive(Debug, clap::Args)]
struct EventArgs {
    /// Stable session identifier from the Agent integration.
    session_id: String,
    /// Source integration name, such as claude, grok, codex or cursor.
    #[arg(long, default_value = "manual")]
    source: String,
    #[arg(long)]
    deep_link: Option<String>,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long)]
    workspace_root: Option<String>,
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
    let cli = Cli::parse();
    let endpoint = cli
        .socket
        .clone()
        .or_else(|| std::env::var_os("ORBCUE_SOCKET").map(PathBuf::from))
        .unwrap_or_else(default_endpoint);
    if should_trampoline_argv(&cli.command) {
        std::process::exit(trampoline_to_windows());
    }
    if let Some(status) = run_early_command(&cli.command, &endpoint, cli.json) {
        std::process::exit(status);
    }
    let request = match request_for(&cli.command) {
        Ok(request) => request,
        Err(error) => {
            eprintln!("orb: {error}");
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
            eprintln!("orb: cannot reach Dock at {}: {error}", endpoint.display());
            eprintln!("Start the daemon from any directory with: orb up");
            std::process::exit(2);
        }
    }
}

fn run_early_command(command: &Command, endpoint: &Path, json_output: bool) -> Option<i32> {
    Some(match command {
        Command::Alias { name, clear } => run_alias_command(name.as_deref(), *clear, json_output),
        Command::ReplaceTab { enable, disable } => {
            run_replace_tab_command(*enable, *disable, json_output)
        }
        Command::Agents | Command::Connect { .. } | Command::Disconnect { .. } => {
            run_connection_command(command, json_output)
        }
        Command::Up | Command::Down => run_daemon_command(command, endpoint, json_output),
        Command::Run {
            agent,
            args,
            profile,
            close,
            from_wsl,
        } => {
            if *from_wsl {
                terminal::run_from_wsl_stdin(json_output)
            } else {
                let Some(agent) = agent.as_deref() else {
                    eprintln!("orb run: missing agent name");
                    return Some(1);
                };
                let close = *close || orbcue_connect::replace_tab_on_run();
                if should_delegate_run_to_windows() {
                    run_via_windows_terminal(agent, args, profile.as_deref(), close, json_output)
                } else {
                    terminal::run_command(agent, args, profile.as_deref(), close, json_output)
                }
            }
        }
        Command::LivenessCheck => run_liveness_check(),
        Command::Emit => run_emit(endpoint, json_output),
        Command::Hook { provider, detach } => {
            if *detach {
                detach_hook(*provider);
            } else {
                run_hook(*provider, endpoint, json_output);
            }
            0
        }
        _ => return None,
    })
}

fn print_setting(
    json_output: bool,
    view: &impl serde::Serialize,
    human: &str,
    ok: bool,
    label: &str,
) -> i32 {
    if json_output {
        println!(
            "{}",
            serde_json::to_string(view).expect("setting view serializes")
        );
    } else if ok {
        println!("{human}");
    } else {
        eprintln!("{label}: {human}");
    }
    if ok {
        0
    } else {
        1
    }
}

fn run_alias_command(name: Option<&str>, clear: bool, json_output: bool) -> i32 {
    let result = if clear || name.is_some_and(|value| value.trim().is_empty()) {
        orbcue_connect::set_run_alias(None)
    } else if let Some(name) = name {
        orbcue_connect::set_run_alias(Some(name))
    } else {
        Ok(orbcue_connect::current_run_alias())
    };
    match result {
        Ok(alias) => {
            let human = match &alias {
                Some(alias) => format!("{alias} grok 等同 orb run grok"),
                None => "没有启动别名".to_owned(),
            };
            print_setting(
                json_output,
                &orbcue_connect::run_alias_ok(alias),
                &human,
                true,
                "orb alias",
            )
        }
        Err(error) => print_setting(
            json_output,
            &orbcue_connect::run_alias_err(error.clone()),
            &error,
            false,
            "orb alias",
        ),
    }
}

fn run_replace_tab_command(enable: bool, disable: bool, json_output: bool) -> i32 {
    let result = if enable {
        orbcue_connect::set_replace_tab_on_run(true)
    } else if disable {
        orbcue_connect::set_replace_tab_on_run(false)
    } else {
        Ok(orbcue_connect::replace_tab_on_run())
    };
    match result {
        Ok(enabled) => {
            let human = if enabled {
                "orb run 会替换当前标签页"
            } else {
                "orb run 会留下当前标签页"
            };
            print_setting(
                json_output,
                &orbcue_connect::replace_tab_ok(enabled),
                human,
                true,
                "orb replace-tab",
            )
        }
        Err(error) => print_setting(
            json_output,
            &orbcue_connect::replace_tab_err(error.clone()),
            &error,
            false,
            "orb replace-tab",
        ),
    }
}

fn run_connection_command(command: &Command, json_output: bool) -> i32 {
    let dock_binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("orb"));
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
                    if agent.origin == orbcue_connect::AgentOrigin::Windows {
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
                eprintln!("orb connect: {name} is not on PATH; pass --original");
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
                        eprintln!("orb connect: {error}");
                        return 1;
                    }
                }
                return 0;
            }
            match manager.connect(name, &path) {
                Ok(record) if json_output => {
                    print_connect_warnings(&manager.connect_warnings(name));
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&record)
                            .expect("connection record serializes")
                    );
                }
                Ok(record) => {
                    println!("Connected {} via {:?}.", record.name, record.method);
                    if !record.limitation.is_empty() {
                        println!("Limitation: {}", record.limitation);
                    }
                    print_connect_warnings(&manager.connect_warnings(name));
                }
                Err(error) => {
                    eprintln!("orb connect: {error}");
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
                    eprintln!("orb disconnect: {error}");
                    return 1;
                }
            }
            0
        }
        _ => unreachable!("connection command dispatched separately"),
    }
}

fn run_hook(provider: HookProvider, endpoint: &Path, json_output: bool) {
    let cursor_ack = matches!(provider, HookProvider::Cursor) && !json_output;
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("orb hook: cannot read stdin: {error}");
        acknowledge_cursor_hook(cursor_ack);
        return;
    }
    let payload: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("orb hook: invalid JSON ({error})");
            acknowledge_cursor_hook(cursor_ack);
            return;
        }
    };
    let event = match provider {
        HookProvider::Claude => claude_hook(&payload),
        HookProvider::Codex => codex_hook(&payload),
        HookProvider::Cursor => cursor_hook(&payload),
        HookProvider::Grok => grok_hook(&payload),
    };
    let Some(mut event) = event else {
        if json_output {
            println!("{{\"accepted\":false,\"rejection_reason\":\"unmapped_event\"}}");
        } else {
            acknowledge_cursor_hook(cursor_ack);
        }
        return;
    };
    event.source = hook_source_from_identities(&event.source, &hook_invoker_identities());
    attach_terminal_id(&mut event);
    attach_liveness(&mut event);
    maybe_set_terminal_title(&event);
    // Observer hook stdout is an agent control channel; trampoline summaries
    // must not land there. Grok/Claude treat exit 2 as a Stop or PreToolUse
    // gate, so delivery failures stay fail-open.
    if should_trampoline_to_windows(&Command::Hook {
        provider,
        detach: false,
    }) {
        let _ = trampoline_emit_with_stdout(&event, false);
        acknowledge_cursor_hook(cursor_ack);
        return;
    }
    if ensure_cli_daemon(endpoint).is_err() {
        acknowledge_cursor_hook(cursor_ack);
        return;
    }
    match send(endpoint, &IpcRequest::Event(event)) {
        Ok(response) => {
            if json_output {
                println!(
                    "{}",
                    serde_json::to_string(&response).expect("response serializes")
                );
            } else {
                acknowledge_cursor_hook(cursor_ack);
            }
        }
        Err(error) => {
            eprintln!("orb hook: cannot reach Dock: {error}");
            acknowledge_cursor_hook(cursor_ack);
        }
    }
}

fn acknowledge_cursor_hook(cursor_ack: bool) {
    if cursor_ack {
        println!("{{}}");
    }
}

fn hook_provider_arg(provider: HookProvider) -> &'static str {
    match provider {
        HookProvider::Claude => "claude",
        HookProvider::Codex => "codex",
        HookProvider::Cursor => "cursor",
        HookProvider::Grok => "grok",
    }
}

fn detach_hook(provider: HookProvider) {
    let mut input = Vec::new();
    if let Err(error) = std::io::stdin().read_to_end(&mut input) {
        eprintln!("orb hook: cannot read stdin: {error}");
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("orb hook: cannot locate orb: {error}");
            return;
        }
    };
    let mut command = ProcessCommand::new(exe);
    command.arg("hook").arg(hook_provider_arg(provider));
    command.stdin(Stdio::piped());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    inherit_hook_identity(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match spawn_detached_hook(&mut command) {
        Ok(child) => child,
        Err(error) => {
            eprintln!("orb hook: cannot spawn detached hook: {error}");
            return;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(error) = stdin.write_all(&input) {
            eprintln!("orb hook: cannot write detached hook stdin: {error}");
        }
    }
}

/// Snapshot tty + agent liveness while this process is still in the agent's
/// tree, then copy them onto the detached child. After `--detach` the parent
/// exits; on WSL the child is reparented to the distro `/init` Relay, which
/// has no tty and outlives the session.
fn inherit_hook_identity(command: &mut ProcessCommand) {
    if std::env::var_os("ORBCUE_TERMINAL_ID").is_none() {
        if let Some(terminal_id) = resolve_terminal_id() {
            command.env("ORBCUE_TERMINAL_ID", terminal_id);
        }
    }
    if std::env::var_os("ORBCUE_AGENT_PID").is_none() {
        if let Some((pid, starttime)) = platform_parent_liveness() {
            command.env("ORBCUE_AGENT_PID", pid.to_string());
            command.env("ORBCUE_AGENT_STARTTIME", starttime.to_string());
        }
    }
}

fn spawn_detached_hook(command: &mut ProcessCommand) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;
        command.creation_flags(
            CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB,
        );
        match command.spawn() {
            Ok(child) => return Ok(child),
            Err(_) => {
                command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
            }
        }
    }
    command.spawn()
}

fn request_for(command: &Command) -> Result<IpcRequest, String> {
    let request = match command {
        Command::Status => IpcRequest::Snapshot,
        Command::Acknowledge(args) => IpcRequest::Acknowledge {
            source: args.source.clone(),
            session_id: args.session_id.clone(),
            terminal_id: None,
        },
        Command::Reset(args) => IpcRequest::Reset {
            source: args.source.clone(),
            session_id: args.session_id.clone(),
            terminal_id: None,
        },
        Command::Start(args) => event_request(args, EventKind::Started)?,
        Command::Working(args) => event_request(args, EventKind::Working)?,
        Command::Waiting(args) => event_request(args, EventKind::WaitingInput)?,
        Command::Permission(args) => event_request(args, EventKind::PermissionRequested)?,
        Command::Complete(args) => event_request(args, EventKind::Completed)?,
        Command::Fail(args) => event_request(args, EventKind::Failed)?,
        Command::Cancel(args) => event_request(args, EventKind::Cancelled)?,
        Command::Hook { .. } | Command::Emit | Command::LivenessCheck => {
            return Err("hook is handled before event parsing".to_owned())
        }
        Command::Agents
        | Command::Connect { .. }
        | Command::Disconnect { .. }
        | Command::Up
        | Command::Down
        | Command::Alias { .. }
        | Command::ReplaceTab { .. }
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
    if let Some(deep_link) = &args.deep_link {
        event.deep_link = Some(deep_link.clone());
    }
    if let Some(cwd) = &args.cwd {
        event.cwd = Some(cwd.clone());
    }
    if let Some(workspace_root) = &args.workspace_root {
        event.workspace_root = Some(workspace_root.clone());
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

/// Return the current process id and a compact parent/name index for the
/// Windows process tree. Both hook liveness and source detection need the same
/// Toolhelp snapshot; keeping the FFI and enumeration in one place avoids two
/// subtly different copies of it.
fn resolve_terminal_id() -> Option<String> {
    match std::env::var("ORBCUE_TERMINAL_ID") {
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
    std::env::var("ORBCUE_NO_TITLE")
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

/// WSL session plumbing outlives every agent in the tab. A detached hook
/// reparented here must not stamp that PID as the agent, or resume-fork
/// opens a ghost row that liveness never reaps.
fn hook_source_from_identities(provider: &str, identities: &[String]) -> String {
    if !provider.eq_ignore_ascii_case("claude") {
        return provider.to_owned();
    }
    for identity in identities {
        if looks_like_claude_cli_identity(identity) {
            return "claude".to_owned();
        }
        if looks_like_cursor_cli_identity(identity) {
            return "cursor".to_owned();
        }
    }
    "claude".to_owned()
}

fn looks_like_cursor_cli_identity(identity: &str) -> bool {
    identity
        .replace('\\', "/")
        .split(|character: char| character == '/' || character.is_ascii_whitespace())
        .any(|segment| path_segment_stem(segment).eq_ignore_ascii_case("cursor-agent"))
}

fn looks_like_claude_cli_identity(identity: &str) -> bool {
    let lower = identity.replace('\\', "/").to_ascii_lowercase();
    if lower.contains("/claude/versions/") {
        return true;
    }
    lower.split_whitespace().any(|token| {
        path_segment_stem(token.rsplit('/').next().unwrap_or(token)).eq_ignore_ascii_case("claude")
    })
}

fn path_segment_stem(segment: &str) -> &str {
    for suffix in [".exe", ".cmd", ".ps1", ".bat", ".com"] {
        let start = segment.len().saturating_sub(suffix.len());
        if segment
            .get(start..)
            .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
            && start > 0
        {
            return &segment[..start];
        }
    }
    segment
}

fn hook_invoker_identities() -> Vec<String> {
    #[cfg(unix)]
    {
        return linux_hook_invoker_identities();
    }
    #[cfg(windows)]
    {
        return windows_hook_invoker_identities();
    }
    #[cfg(not(any(unix, windows)))]
    Vec::new()
}

#[cfg(unix)]
fn linux_hook_invoker_identities() -> Vec<String> {
    let mut pid = unsafe { libc::getppid() };
    let mut identities = Vec::new();
    for _ in 0..8 {
        if pid <= 1 {
            break;
        }
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(_) => break,
        };
        let Some((ppid, _, _)) = parse_proc_stat(&stat) else {
            break;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if !is_short_lived_hook_parent(comm.trim()) {
            if let Some(identity) = linux_process_identity(pid as u32, comm.trim()) {
                identities.push(identity);
            }
        }
        if ppid <= 1 || ppid == pid {
            break;
        }
        pid = ppid;
    }
    identities
}

#[cfg(unix)]
fn linux_process_identity(pid: u32, comm: &str) -> Option<String> {
    let mut parts = Vec::new();
    if let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) {
        parts.push(exe.to_string_lossy().into_owned());
    }
    if let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) {
        let text = String::from_utf8_lossy(&cmdline);
        let trimmed: String = text
            .chars()
            .take(4096)
            .map(|character| if character == '\0' { ' ' } else { character })
            .collect();
        if !trimmed.trim().is_empty() {
            parts.push(trimmed);
        }
    }
    if !comm.is_empty() {
        parts.push(comm.to_owned());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

#[cfg(windows)]
fn windows_hook_invoker_identities() -> Vec<String> {
    let (current, by_pid) = windows_process_tree().unwrap_or_default();
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn QueryFullProcessImageNameW(
            process: isize,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    unsafe {
        let mut pid = match by_pid.get(&current) {
            Some(process) => process.parent,
            None => return Vec::new(),
        };
        let mut identities = Vec::new();
        for _ in 0..8 {
            if pid == 0 {
                break;
            }
            let Some(process) = by_pid.get(&pid) else {
                break;
            };
            if !is_short_lived_windows_hook_parent(&process.name) {
                let mut identity = process.name.clone();
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if handle != 0 {
                    let mut buf = [0u16; 1024];
                    let mut size = buf.len() as u32;
                    if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) != 0
                        && size > 0
                    {
                        identity.push(' ');
                        identity.push_str(&String::from_utf16_lossy(&buf[..size as usize]));
                    }
                    CloseHandle(handle);
                }
                identities.push(identity);
            }
            if process.parent == 0 || process.parent == pid {
                break;
            }
            pid = process.parent;
        }
        identities
    }
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

fn run_via_windows_terminal(
    agent: &str,
    args: &[String],
    profile: Option<&str>,
    close: bool,
    json_output: bool,
) -> i32 {
    let spec = match terminal::prepare_wsl_run(agent, args, profile) {
        Ok(spec) => spec,
        Err(error) => return terminal::print_run_error(&error, json_output),
    };
    match trampoline_from_wsl_run(&spec) {
        Ok(mut value) => {
            let ok = value.get("ok").and_then(Value::as_bool).unwrap_or(false);
            if !ok {
                return terminal::print_run_error(
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("Windows orb.exe run failed"),
                    json_output,
                );
            }
            let closed = terminal::close_launcher_after_spawn(close);
            if json_output {
                value["closed_launcher"] = Value::Bool(closed);
                println!("{value}");
            } else {
                terminal::print_run_started(
                    value.get("agent").and_then(Value::as_str).unwrap_or(agent),
                    value
                        .get("marker")
                        .and_then(Value::as_str)
                        .unwrap_or(&spec.marker),
                    closed,
                    close,
                );
            }
            0
        }
        Err(status) => status,
    }
}

fn trampoline_from_wsl_run(spec: &terminal::WslRunSpec) -> Result<Value, i32> {
    let payload = serde_json::to_vec(spec).map_err(|error| {
        eprintln!("orb: cannot serialize run spec: {error}");
        2
    })?;
    let output =
        hop::trampoline_payload(&["--json", "run", "--from-wsl"], &payload, Stdio::piped())?;
    serde_json::from_slice(&output.stdout).map_err(|error| {
        eprintln!(
            "orb: Windows run did not return JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        );
        output.status.code().unwrap_or(2)
    })
}

fn trampoline_emit(event: &DockEvent) -> i32 {
    trampoline_emit_with_stdout(event, true)
}

fn trampoline_emit_with_stdout(event: &DockEvent, inherit_stdout: bool) -> i32 {
    let payload = match serde_json::to_vec(event) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!("orb: cannot serialize event: {error}");
            return 2;
        }
    };
    match hop::trampoline_payload(
        &["emit"],
        &payload,
        if inherit_stdout {
            Stdio::inherit()
        } else {
            Stdio::null()
        },
    ) {
        Ok(output) => output.status.code().unwrap_or(1),
        Err(status) => status,
    }
}

fn run_emit(endpoint: &Path, json_output: bool) -> i32 {
    if let Err(status) = ensure_cli_daemon(endpoint) {
        return status;
    }
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("orb emit: cannot read stdin: {error}");
        return 2;
    }
    let event: DockEvent = match serde_json::from_str(input.trim()) {
        Ok(event) => event,
        Err(error) => {
            eprintln!("orb emit: invalid event JSON ({error})");
            return 2;
        }
    };
    match send(endpoint, &IpcRequest::Event(event)) {
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
                "orb emit: cannot reach Dock at {}: {error}",
                endpoint.display()
            );
            2
        }
    }
}

fn ensure_cli_daemon(endpoint: &Path) -> Result<(), i32> {
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
            eprintln!("orb: {error}");
            Err(2)
        }
    }
}

fn send(endpoint: &Path, request: &IpcRequest) -> Result<WireResponse, String> {
    let mut stream = local_connect(endpoint).map_err(|error| error.to_string())?;
    local_set_recv_timeout(&stream, Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    local_set_send_timeout(&stream, Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    let line = encode_request(request).map_err(|error| error.to_string())?;
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
        connect_method_label(preview.method)
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
    if !preview.warnings.is_empty() {
        println!("Warnings:");
        for warning in &preview.warnings {
            println!("  - {warning}");
        }
    }
}

fn print_connect_warnings(warnings: &[String]) {
    for warning in warnings {
        eprintln!("orb connect: {warning}");
    }
}

fn connect_method_label(method: ConnectionMethod) -> &'static str {
    match method {
        ConnectionMethod::Wrapper => "wrapper",
        ConnectionMethod::ClaudeHook
        | ConnectionMethod::GrokHook
        | ConnectionMethod::CodexHook
        | ConnectionMethod::CursorHook => "native hook",
    }
}

fn print_daemon_ready(json_output: bool, already_running: bool, response: &WireResponse) -> i32 {
    if json_output {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "already_running": already_running,
                "snapshot": response.snapshot
            })
        );
    } else {
        println!(
            "{}",
            if already_running {
                "Dock daemon already running"
            } else {
                "orbd ready"
            }
        );
        print_summary(response);
    }
    0
}

fn run_daemon_command(command: &Command, endpoint: &Path, json_output: bool) -> i32 {
    match command {
        Command::Up => start_daemon(endpoint, json_output),
        Command::Down => stop_daemon(endpoint, json_output),
        _ => unreachable!("daemon command dispatched separately"),
    }
}

fn start_daemon(endpoint: &Path, json_output: bool) -> i32 {
    if let Ok(response) = send(endpoint, &IpcRequest::Snapshot) {
        return print_daemon_ready(json_output, true, &response);
    }

    let dockd = dockd_binary();
    if !dockd.is_file() {
        eprintln!(
            "orb up: cannot find orbd at {}. Start the presenter or install orbd.exe.",
            dockd.display()
        );
        return 1;
    }

    let state_dir = runtime_state_dir();
    if let Err(error) = fs::create_dir_all(&state_dir) {
        eprintln!("orb up: {error}");
        return 1;
    }
    let log_path = state_dir.join("orbd.log");
    let log = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("orb up: cannot open {}: {error}", log_path.display());
            return 1;
        }
    };
    let log_err = match log.try_clone() {
        Ok(file) => file,
        Err(error) => {
            eprintln!("orb up: {error}");
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
            let _ = fs::write(state_dir.join("orbd.pid"), child.id().to_string());
        }
        Err(error) => {
            eprintln!("orb up: failed to start {}: {error}", dockd.display());
            return 1;
        }
    }

    for _ in 0..25 {
        if let Ok(response) = send(endpoint, &IpcRequest::Snapshot) {
            return print_daemon_ready(json_output, false, &response);
        }
        thread::sleep(Duration::from_millis(80));
    }
    eprintln!(
        "orb up: daemon did not become ready; see {}",
        log_path.display()
    );
    1
}

fn stop_daemon(endpoint: &Path, json_output: bool) -> i32 {
    let state_dir = runtime_state_dir();
    let pid_path = state_dir.join("orbd.pid");
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
        let _ = ProcessCommand::new("pkill").args(["-x", "orbd"]).status();
    }
    thread::sleep(Duration::from_millis(200));
    if send(endpoint, &IpcRequest::Snapshot).is_ok() {
        eprintln!(
            "orb down: daemon is still reachable at {}",
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

fn dockd_binary() -> PathBuf {
    let file_name = if cfg!(windows) { "orbd.exe" } else { "orbd" };
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("ORBCUE_ORBD").filter(|value| !value.is_empty()) {
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
                .join(WINDOWS_APP_FOLDER)
                .join("orbd.exe"),
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
        hook_source_from_identities, is_short_lived_windows_hook_parent,
        looks_like_claude_cli_identity, looks_like_cursor_cli_identity,
        newest_windows_dock_under_mnt, resolve_windows_liveness_pid, stays_on_agent_os,
        trampoline_to_windows_predicate, Command, WINDOWS_APP_FOLDER,
    };
    use orbcue_core::{DockEvent, EventKind};
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn run_command() -> Command {
        Command::Run {
            profile: None,
            close: false,
            from_wsl: false,
            agent: Some("grok".to_owned()),
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
    fn claude_hook_from_cursor_cli_is_labeled_cursor() {
        assert!(looks_like_cursor_cli_identity(
            "/home/u/.local/share/cursor-agent/versions/2026.08.11-e8db854/node"
        ));
        assert!(looks_like_cursor_cli_identity(
            "/home/u/.local/bin/agent /home/u/.local/share/cursor-agent/versions/x/index.js"
        ));
        assert!(looks_like_cursor_cli_identity(
            r"C:\Users\u\AppData\Local\cursor-agent\versions\x\node.exe"
        ));
        assert!(looks_like_cursor_cli_identity("cursor-agent.exe"));
        assert!(!looks_like_cursor_cli_identity("/home/u/.grok/bin/agent"));
        assert!(!looks_like_cursor_cli_identity("/usr/bin/node"));
        assert!(looks_like_claude_cli_identity(
            "/home/u/.local/share/claude/versions/2.1.226"
        ));
        assert!(looks_like_claude_cli_identity("/home/u/.local/bin/claude"));
        assert!(looks_like_claude_cli_identity("claude.exe"));
        assert!(!looks_like_claude_cli_identity(
            "/home/u/.config/orbcue/claude-hook.sh"
        ));
        assert!(looks_like_cursor_cli_identity(
            r"C:\Users\claude\AppData\Local\cursor-agent\versions\x\node.exe"
        ));
        assert!(!looks_like_claude_cli_identity(
            r"C:\Users\claude\AppData\Local\cursor-agent\versions\x\node.exe"
        ));

        assert_eq!(
            hook_source_from_identities(
                "claude",
                &[
                    "/home/u/.local/share/cursor-agent/versions/x/node".to_owned(),
                    "WindowsTerminal.exe".to_owned()
                ]
            ),
            "cursor"
        );
        assert_eq!(
            hook_source_from_identities(
                "claude",
                &[
                    "/home/u/.local/bin/claude".to_owned(),
                    "/home/u/.local/share/cursor-agent/versions/x/node".to_owned()
                ]
            ),
            "claude"
        );
        assert_eq!(
            hook_source_from_identities("claude", &["/usr/bin/bash".to_owned()]),
            "claude"
        );
        assert_eq!(
            hook_source_from_identities("cursor", &["cursor-agent.exe".to_owned()]),
            "cursor"
        );
        assert_eq!(hook_source_from_identities("claude", &[]), "claude");
    }

    #[test]
    fn windows_hook_skips_cmd_wrapper_and_keeps_the_agent() {
        assert!(is_short_lived_windows_hook_parent("cmd.exe"));
        assert!(is_short_lived_windows_hook_parent("Cmd.EXE"));
        assert!(is_short_lived_windows_hook_parent(
            r"C:\Windows\System32\cmd.exe"
        ));
        assert!(is_short_lived_windows_hook_parent("conhost"));
        assert!(is_short_lived_windows_hook_parent("Conhost.exe"));
        assert!(is_short_lived_windows_hook_parent("orb.exe"));
        assert!(is_short_lived_windows_hook_parent("ORB.EXE"));
        assert!(!is_short_lived_windows_hook_parent("claude.exe"));
        assert!(!is_short_lived_windows_hook_parent("powershell.exe"));
        assert!(!is_short_lived_windows_hook_parent("WindowsTerminal.exe"));
        assert!(!is_short_lived_windows_hook_parent("grok.exe"));

        let tree = [
            (10, 9, "orb.exe"),
            (9, 8, "cmd.exe"),
            (8, 1, "claude.exe"),
            (1, 0, "WindowsTerminal.exe"),
        ];
        assert_eq!(resolve_windows_liveness_pid(10, &tree), Some(8));

        let direct = [(10, 8, "orb.exe"), (8, 1, "cursor-agent.exe")];
        assert_eq!(resolve_windows_liveness_pid(10, &direct), Some(8));

        let only_cmd = [(10, 9, "orb.exe"), (9, 0, "cmd.exe")];
        assert_eq!(resolve_windows_liveness_pid(10, &only_cmd), None);

        let detached = [
            (30, 20, "orb.exe"),
            (20, 10, "orb.exe"),
            (10, 1, "grok.exe"),
        ];
        assert_eq!(
            resolve_windows_liveness_pid(30, &detached),
            Some(10),
            "detach child must walk past the short-lived orb parent to grok"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_hook_skips_detached_orb_parent() {
        assert!(super::is_short_lived_hook_parent("orb"));
        assert!(super::is_short_lived_hook_parent("sh"));
        assert!(super::is_short_lived_hook_parent("dash"));
        assert!(!super::is_short_lived_hook_parent("bash"));
        assert!(!super::is_short_lived_hook_parent("zsh"));
        assert!(!super::is_short_lived_hook_parent("fish"));
        assert!(!super::is_short_lived_hook_parent("grok"));
        assert!(!super::is_short_lived_hook_parent("claude"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_hook_skips_wsl_session_plumbing() {
        assert!(super::liveness::is_wsl_session_plumbing("Relay(50997)"));
        assert!(super::liveness::is_wsl_session_plumbing("SessionLeader"));
        assert!(super::liveness::is_wsl_session_plumbing(
            "init-systemd(Ubuntu)"
        ));
        assert!(!super::liveness::is_wsl_session_plumbing("grok"));
        assert!(!super::liveness::is_wsl_session_plumbing("zsh"));
        assert!(!super::liveness::is_wsl_session_plumbing("init"));
        assert!(super::liveness::should_skip_linux_liveness_parent(
            "Relay(50997)"
        ));
        assert!(super::liveness::should_skip_linux_liveness_parent("sh"));
        assert!(!super::liveness::should_skip_linux_liveness_parent("bash"));
        assert!(!super::liveness::should_skip_linux_liveness_parent("grok"));
    }

    #[test]
    fn liveness_from_env_requires_pid_and_starttime() {
        let _guard = lock_env();
        let previous_pid = std::env::var_os("ORBCUE_AGENT_PID");
        let previous_start = std::env::var_os("ORBCUE_AGENT_STARTTIME");
        std::env::remove_var("ORBCUE_AGENT_PID");
        std::env::remove_var("ORBCUE_AGENT_STARTTIME");
        assert_eq!(super::liveness_from_env(), None);

        std::env::set_var("ORBCUE_AGENT_PID", "51326");
        assert_eq!(super::liveness_from_env(), None);

        std::env::set_var("ORBCUE_AGENT_STARTTIME", "3827748");
        assert_eq!(super::liveness_from_env(), Some((51326, 3827748)));

        std::env::set_var("ORBCUE_AGENT_PID", "0");
        assert_eq!(super::liveness_from_env(), None);

        restore_env("ORBCUE_AGENT_PID", previous_pid);
        restore_env("ORBCUE_AGENT_STARTTIME", previous_start);
    }

    #[test]
    fn run_and_agents_do_not_trampoline() {
        assert!(stays_on_agent_os(&Command::Agents));
        assert!(stays_on_agent_os(&connect_command()));
        assert!(stays_on_agent_os(&Command::Disconnect {
            name: "grok".to_owned()
        }));
        assert!(stays_on_agent_os(&run_command()));
        assert!(stays_on_agent_os(&Command::ReplaceTab {
            enable: false,
            disable: false
        }));
        assert!(trampoline_to_windows_predicate(true, true, false, false));
        assert!(!stays_on_agent_os(&Command::Status));
        assert!(!stays_on_agent_os(&Command::Up));
        assert!(!stays_on_agent_os(&Command::Emit));
        assert!(stays_on_agent_os(&Command::LivenessCheck));
    }

    #[test]
    fn completed_hooks_do_not_attach_liveness() {
        let mut event = orbcue_core::DockEvent::new("e-stop", EventKind::Completed, "claude", "s1");
        super::attach_liveness(&mut event);
        assert!(
            !event.metadata.contains_key("agent_pid"),
            "Stop/completed must not rewrite liveness to a dying hook parent"
        );
    }

    #[test]
    fn wrapper_event_path_does_not_attach_liveness() {
        let request = super::event_request(
            &super::EventArgs {
                session_id: "s1".to_owned(),
                source: "grok".to_owned(),
                deep_link: None,
                cwd: None,
                workspace_root: None,
            },
            EventKind::Started,
        )
        .unwrap();
        let orbcue_ipc::IpcRequest::Event(event) = request else {
            panic!("expected event");
        };
        assert!(!event.metadata.contains_key("agent_pid"));
        assert!(!event.metadata.contains_key("agent_os"));
        assert!(trampoline_to_windows_predicate(true, true, false, false));
        assert!(!trampoline_to_windows_predicate(true, true, false, true));
        assert!(!trampoline_to_windows_predicate(true, true, true, false));
        assert!(!trampoline_to_windows_predicate(true, false, false, false));
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
        let previous_override = std::env::var_os("ORBCUE_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::set_var("ORBCUE_TERMINAL_ID", "pts-override");
        std::env::set_var("WT_SESSION", "wt-should-lose");
        let resolved = super::resolve_terminal_id();
        restore_env("ORBCUE_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        assert_eq!(resolved.as_deref(), Some("pts-override"));
    }

    #[test]
    fn terminal_id_own_tty_beats_wt_session() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("ORBCUE_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::remove_var("ORBCUE_TERMINAL_ID");
        std::env::set_var("WT_SESSION", "wt-should-lose");
        let own = super::own_tty_id();
        let resolved = super::resolve_terminal_id();
        restore_env("ORBCUE_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        if let Some(tty) = own {
            assert_eq!(resolved.as_deref(), Some(tty.as_str()));
            assert_ne!(resolved.as_deref(), Some("wt-should-lose"));
        }
    }

    #[test]
    fn terminal_id_falls_back_to_tty_without_env_ids() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("ORBCUE_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::remove_var("ORBCUE_TERMINAL_ID");
        std::env::remove_var("WT_SESSION");
        let resolved = super::resolve_terminal_id();
        let expected = super::own_tty_id().or_else(super::ancestor_tty_id);
        restore_env("ORBCUE_TERMINAL_ID", previous_override);
        restore_env("WT_SESSION", previous_wt);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn unix_ignores_wt_session_as_terminal_id() {
        let _guard = lock_env();
        let previous_override = std::env::var_os("ORBCUE_TERMINAL_ID");
        let previous_wt = std::env::var_os("WT_SESSION");
        std::env::remove_var("ORBCUE_TERMINAL_ID");
        std::env::set_var("WT_SESSION", "wt-should-lose");
        let resolved = super::resolve_terminal_id();
        restore_env("ORBCUE_TERMINAL_ID", previous_override);
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
    fn title_env_skips_setting() {
        let _guard = lock_env();
        let previous = std::env::var_os("ORBCUE_NO_TITLE");
        std::env::set_var("ORBCUE_NO_TITLE", "1");
        let skipped = super::title_setting_suppressed();
        restore_env("ORBCUE_NO_TITLE", previous);
        assert!(skipped);
    }

    #[test]
    fn title_only_for_main_session_lifecycle() {
        let _guard = lock_env();
        let previous = std::env::var_os("ORBCUE_NO_TITLE");
        std::env::remove_var("ORBCUE_NO_TITLE");
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
            Some(value) => std::env::set_var("ORBCUE_NO_TITLE", value),
            None => std::env::remove_var("ORBCUE_NO_TITLE"),
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
        with_marker.terminal_id = Some("orb:ab12cd".to_owned());
        assert_eq!(
            super::lifecycle_terminal_title(&with_marker),
            "agent-activity-dock · grok · orb:ab12cd"
        );

        let mut without_marker = DockEvent::new("e2", EventKind::Started, "grok", "s2");
        without_marker.cwd = Some("/home/qingz/projects/agent-activity-dock".to_owned());
        assert_eq!(
            super::lifecycle_terminal_title(&without_marker),
            "agent-activity-dock · grok"
        );

        let mut marker_only = DockEvent::new("e3", EventKind::Started, "claude", "s3");
        marker_only.terminal_id = Some("orb:00ffaa".to_owned());
        assert_eq!(
            super::lifecycle_terminal_title(&marker_only),
            "claude · orb:00ffaa"
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

    fn write_orb_exe(path: &Path, modified: SystemTime) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"orb").unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    fn temp_mnt_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("orbcue-mnt-orb-{nonce}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn orb_under_user(mnt: &Path, drive: &str, user: &str) -> PathBuf {
        mnt.join(drive)
            .join("Users")
            .join(user)
            .join("AppData")
            .join("Local")
            .join(WINDOWS_APP_FOLDER)
            .join("orb.exe")
    }

    #[test]
    fn enumerates_newest_windows_orb_under_mnt() {
        let root = temp_mnt_root();
        let older = orb_under_user(&root, "c", "Alice");
        let newer = orb_under_user(&root, "c", "Bob");
        let now = SystemTime::now();
        write_orb_exe(&older, now - Duration::from_secs(120));
        write_orb_exe(&newer, now);
        assert_eq!(newest_windows_dock_under_mnt(&root), Some(newer));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enumerates_windows_dock_under_mnt_returns_none_without_hits() {
        let root = temp_mnt_root();
        fs::create_dir_all(root.join("c").join("Users").join("Alice")).unwrap();
        assert_eq!(newest_windows_dock_under_mnt(&root), None);
        fs::remove_dir_all(root).unwrap();
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
