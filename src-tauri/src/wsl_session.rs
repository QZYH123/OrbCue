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

    fn acknowledge(&self, source: &str, session_id: &str) -> Result<SnapshotView, String> {
        Ok(query_bridge(&IpcRequest::Acknowledge {
            source: source.to_owned(),
            session_id: session_id.to_owned(),
        })?
        .snapshot)
    }

    fn reset(&self, source: &str, session_id: &str) -> Result<SnapshotView, String> {
        Ok(query_bridge(&IpcRequest::Reset {
            source: source.to_owned(),
            session_id: session_id.to_owned(),
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

pub fn agent_inventory() -> crate::AgentInventory {
    match wsl_dock_json::<InventoryJson>(&["agents", "--json"]) {
        Ok(inventory) => crate::AgentInventory {
            discovered: inventory.discovered,
            connected: inventory.connected,
        },
        Err(error) => {
            eprintln!("Agent Activity Dock: {error}");
            crate::AgentInventory {
                discovered: Vec::new(),
                connected: Vec::new(),
            }
        }
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
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(bridge_failure(format!(" ({})", output.status), &stderr));
    }
    serde_json::from_str(stdout.trim())
        .map_err(|error| format!("cannot parse WSL dock JSON ({error}): {}", stdout.trim()))
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

fn wsl_dock_command(args: &[&str]) -> Result<Command, String> {
    let mut command = Command::new("wsl.exe");
    if let Ok(distro) = env::var("AGENT_ACTIVITY_DOCK_WSL_DISTRO") {
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
    command.args(args);
    hide_console(&mut command);
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
        Ok(status) => format!(" ({status})"),
        Err(error) => format!(" ({error})"),
    }
}

fn read_utf8(reader: &mut impl Read) -> String {
    let mut text = String::new();
    let _ = reader.read_to_string(&mut text);
    text
}

fn missing_wsl_or_dock(error: std::io::Error) -> String {
    format!(
        "cannot start WSL dock via wsl.exe ({error}). Install WSL and run `bash scripts/install-cli.sh`, or set AGENT_ACTIVITY_DOCK_BRIDGE_COMMAND"
    )
}

fn bridge_failure(status: String, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("WSL dock bridge failed{status}. Is `$HOME/.local/bin/dock` installed inside WSL?")
    } else {
        format!("WSL dock bridge failed{status}: {stderr}")
    }
}
