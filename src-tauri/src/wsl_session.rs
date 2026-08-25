use crate::PresenterSession;
use agent_activity_dock_connect::{ConnectionPreview, ConnectionRecord, DiscoveredAgent};
use agent_activity_dock_ipc::{encode_request, IpcRequest, SnapshotView, WireResponse};
use agent_activity_dock_service::SnapshotMessage;
use serde::Deserialize;
use std::env;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub struct WslSession;

impl WslSession {
    pub fn connect() -> Self {
        Self
    }
}

impl PresenterSession for WslSession {
    fn snapshot(&self) -> Result<SnapshotView, String> {
        Ok(query_bridge(&IpcRequest::Snapshot)?.snapshot)
    }

    fn acknowledge(
        &self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> Result<SnapshotView, String> {
        Ok(query_bridge(&IpcRequest::Acknowledge {
            source: source.to_owned(),
            session_id: session_id.to_owned(),
            terminal_id: terminal_id.map(str::to_owned),
        })?
        .snapshot)
    }

    fn reset(
        &self,
        source: &str,
        session_id: &str,
        terminal_id: Option<&str>,
    ) -> Result<SnapshotView, String> {
        Ok(query_bridge(&IpcRequest::Reset {
            source: source.to_owned(),
            session_id: session_id.to_owned(),
            terminal_id: terminal_id.map(str::to_owned),
        })?
        .snapshot)
    }

    fn subscribe(&self) -> mpsc::Receiver<SnapshotMessage> {
        subscribe_bridge()
    }

    fn request_shutdown(&self) {}

    fn wait_for_shutdown(&self) {}
}

#[derive(Debug, Deserialize)]
struct InventoryJson {
    #[serde(default)]
    discovered: Vec<DiscoveredAgent>,
    #[serde(default)]
    connected: Vec<ConnectionRecord>,
}

#[derive(Debug, Deserialize)]
struct DisconnectJson {
    disconnected: bool,
}

pub fn raw_inventory() -> Result<(Vec<DiscoveredAgent>, Vec<ConnectionRecord>), String> {
    match wsl_dock_json::<InventoryJson>(&["agents", "--json"]) {
        Ok(inventory) => Ok((inventory.discovered, inventory.connected)),
        Err(error) if agent_activity_dock_connect::wsl_side_is_absent(&error) => {
            Ok((Vec::new(), Vec::new()))
        }
        Err(error) => Err(error),
    }
}

pub fn preview_connect(name: &str, original: &str) -> Result<ConnectionPreview, String> {
    wsl_dock_json(&[
        "connect",
        name,
        "--original",
        original,
        "--dry-run",
        "--json",
    ])
}

pub fn connect_agent(name: &str, original: &str) -> Result<ConnectionRecord, String> {
    wsl_dock_json(&["connect", name, "--original", original, "--json"])
}

pub fn disconnect_agent(name: &str) -> Result<bool, String> {
    Ok(wsl_dock_json::<DisconnectJson>(&["disconnect", name, "--json"])?.disconnected)
}

#[derive(Debug, Deserialize)]
struct AliasJson {
    ok: bool,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

fn alias_from_json(parsed: AliasJson) -> Result<Option<String>, String> {
    if parsed.ok {
        Ok(parsed.alias)
    } else {
        Err(parsed
            .error
            .unwrap_or_else(|| "无法更新启动别名".to_owned()))
    }
}

pub fn run_alias() -> Result<Option<String>, String> {
    alias_from_json(wsl_dock_json::<AliasJson>(&["alias", "--json"])?)
}

pub fn set_run_alias(name: Option<&str>) -> Result<Option<String>, String> {
    let parsed = match name {
        None => wsl_dock_json::<AliasJson>(&["alias", "--clear", "--json"])?,
        Some(name) => wsl_dock_json::<AliasJson>(&["alias", name, "--json"])?,
    };
    alias_from_json(parsed)
}

fn subscribe_bridge() -> mpsc::Receiver<SnapshotMessage> {
    let (sender, receiver) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("dock-ui-subscribe".to_owned())
        .spawn(move || {
            let mut backoff = Duration::from_millis(200);
            loop {
                match subscribe_once(&sender) {
                    Ok(()) => break,
                    Err(error) => {
                        eprintln!("Agent Activity Dock bridge: {error}");
                        thread::sleep(backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(2));
                    }
                }
            }
        });
    receiver
}

fn subscribe_once(sender: &mpsc::Sender<SnapshotMessage>) -> Result<(), String> {
    let mut child = spawn_bridge(Stdio::inherit())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "dock bridge stdin is unavailable".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "dock bridge stdout is unavailable".to_owned())?;
    stdin
        .write_all(b"{\"query\":\"subscribe\"}\n")
        .map_err(|error| format!("cannot write subscribe to dock bridge: {error}"))?;
    stdin
        .flush()
        .map_err(|error| format!("cannot flush dock bridge subscribe: {error}"))?;
    let mut reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("dock bridge subscribe closed: {error}"))?;
        if read == 0 {
            let detail = wait_bridge_detail(&mut child);
            drop(stdin);
            return Err(format!("Dock subscribe closed{detail}"));
        }
        let message: SnapshotMessage = serde_json::from_str(line.trim())
            .map_err(|error| format!("invalid dock bridge snapshot ({error}): {}", line.trim()))?;
        if sender.send(message).is_err() {
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
            return Ok(());
        }
    }
}

fn query_bridge(request: &IpcRequest) -> Result<WireResponse, String> {
    let mut child = spawn_bridge(Stdio::piped())?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "dock bridge stdin is unavailable".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "dock bridge stdout is unavailable".to_owned())?;
    let mut stderr = child.stderr.take();
    let line = encode_request(request).map_err(|error| error.to_string())?;
    stdin
        .write_all(&line)
        .map_err(|error| format!("cannot write to dock bridge: {error}"))?;
    drop(stdin);
    let mut response = String::new();
    BufReader::new(stdout)
        .read_line(&mut response)
        .map_err(|error| format!("cannot read dock bridge: {error}"))?;
    if response.trim().is_empty() {
        let detail = wait_bridge_detail(&mut child);
        let stderr_text = stderr.as_mut().map(read_utf8).unwrap_or_default();
        return Err(bridge_failure(detail, &stderr_text));
    }
    let parsed = serde_json::from_str(response.trim()).map_err(|error| {
        format!(
            "invalid dock bridge response ({error}): {}",
            response.trim()
        )
    })?;
    let _ = child.wait();
    Ok(parsed)
}

fn wsl_dock_json<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<T, String> {
    let output = wsl_dock_command(args)?
        .output()
        .map_err(|error| missing_wsl_or_dock(error))?;
    let stdout = agent_activity_dock_connect::decode_console_output(&output.stdout);
    let stderr = agent_activity_dock_connect::decode_console_output(&output.stderr);
    if !output.status.success() {
        return Err(bridge_failure(format_exit_status(output.status), &stderr));
    }
    parse_wsl_json(&stdout)
}

fn parse_wsl_json<T: for<'de> Deserialize<'de>>(stdout: &str) -> Result<T, String> {
    let trimmed = stdout.trim();
    if let Ok(parsed) = serde_json::from_str(trimmed) {
        return Ok(parsed);
    }
    let start = trimmed
        .find('{')
        .ok_or_else(|| format!("cannot parse WSL dock JSON: {}", trimmed))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| format!("cannot parse WSL dock JSON: {}", trimmed))?;
    serde_json::from_str(&trimmed[start..=end])
        .map_err(|error| format!("cannot parse WSL dock JSON ({error}): {trimmed}"))
}

fn spawn_bridge(stderr: Stdio) -> Result<Child, String> {
    let mut command = bridge_command()?;
    hide_console(&mut command);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .map_err(missing_wsl_or_dock)
}

fn bridge_command() -> Result<Command, String> {
    if let Some(override_cmd) =
        env::var_os("AGENT_ACTIVITY_DOCK_BRIDGE_COMMAND").filter(|value| !value.is_empty())
    {
        let line = override_cmd.to_string_lossy();
        let parts = split_command_line(&line);
        let (program, args) = parts
            .split_first()
            .ok_or_else(|| "AGENT_ACTIVITY_DOCK_BRIDGE_COMMAND is empty".to_owned())?;
        let mut command = Command::new(program);
        command.args(args);
        return Ok(command);
    }
    wsl_dock_command(&["bridge"])
}

pub(crate) fn wsl_base_command() -> Command {
    if let Ok(distro) = env::var("AGENT_ACTIVITY_DOCK_WSL_DISTRO") {
        if !distro.is_empty() {
            return wsl_command_for_distro(&distro);
        }
    }
    let mut command = Command::new("wsl.exe");
    hide_console(&mut command);
    command
}

pub(crate) fn wsl_command_for_distro(distro: &str) -> Command {
    let mut command = Command::new("wsl.exe");
    if !distro.is_empty() {
        command.args(["-d", distro]);
    }
    hide_console(&mut command);
    command
}

pub(crate) fn wsl_list_command() -> Command {
    let mut command = Command::new("wsl.exe");
    command.args(["-l", "-q"]);
    hide_console(&mut command);
    command
}

fn wsl_dock_command(args: &[&str]) -> Result<Command, String> {
    let mut command = wsl_base_command();
    command.args([
        "-e",
        "sh",
        "-c",
        r#"exec "$HOME/.local/bin/dock" "$@""#,
        "sh",
    ]);
    command.args(args);
    let backend = agent_activity_dock_ipc::resolve_backend();
    command.env("AGENT_ACTIVITY_DOCK_BACKEND", backend.as_str());
    let extra = "AGENT_ACTIVITY_DOCK_BACKEND/u";
    match env::var("WSLENV") {
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
    Ok(command)
}

fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

fn split_command_line(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for character in input.chars() {
        match character {
            '"' => in_quotes = !in_quotes,
            character if character.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

fn wait_bridge_detail(child: &mut Child) -> String {
    match child.wait() {
        Ok(status) if status.success() => String::new(),
        Ok(status) => format_exit_status(status),
        Err(error) => format!(" ({error})"),
    }
}

fn format_exit_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!(" (exit status: {code})"),
        None => format!(" ({status})"),
    }
}

fn read_utf8(reader: &mut impl Read) -> String {
    let mut bytes = Vec::new();
    let _ = reader.read_to_end(&mut bytes);
    agent_activity_dock_connect::decode_console_output(&bytes)
}

fn missing_wsl_or_dock(error: std::io::Error) -> String {
    format!(
        "cannot start WSL dock via wsl.exe ({error}). Install WSL and run `bash scripts/install-cli.sh`, or set AGENT_ACTIVITY_DOCK_BRIDGE_COMMAND"
    )
}

fn bridge_failure(status: String, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("WSL dock bridge failed{status}")
    } else {
        format!("WSL dock bridge failed{status}: {stderr}")
    }
}
