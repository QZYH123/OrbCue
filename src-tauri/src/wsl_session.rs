use orbcue_connect::{ConnectionPreview, ConnectionRecord, DiscoveredAgent};
use serde::Deserialize;
use std::env;
use std::process::{Command, Stdio};
use std::time::Duration;

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
        Err(error) if orbcue_connect::wsl_side_is_absent(&error) => Ok((Vec::new(), Vec::new())),
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
    json_ok(parsed.ok, parsed.alias, parsed.error, "无法更新启动别名")
}

fn json_ok<T>(ok: bool, value: T, error: Option<String>, fallback: &str) -> Result<T, String> {
    if ok {
        Ok(value)
    } else {
        Err(error.unwrap_or_else(|| fallback.to_owned()))
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

#[derive(Debug, Deserialize)]
struct ReplaceTabJson {
    ok: bool,
    #[serde(default)]
    replace_tab: bool,
    #[serde(default)]
    error: Option<String>,
}

fn replace_tab_from_json(parsed: ReplaceTabJson) -> Result<bool, String> {
    json_ok(
        parsed.ok,
        parsed.replace_tab,
        parsed.error,
        "无法更新替换标签页设置",
    )
}

pub fn set_replace_tab_on_run(enabled: bool) -> Result<bool, String> {
    let parsed = if enabled {
        wsl_dock_json::<ReplaceTabJson>(&["replace-tab", "--enable", "--json"])?
    } else {
        wsl_dock_json::<ReplaceTabJson>(&["replace-tab", "--disable", "--json"])?
    };
    replace_tab_from_json(parsed)
}

fn wsl_dock_json<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<T, String> {
    let output = run_with_timeout(&mut wsl_dock_command(args)?, Duration::from_secs(8))
        .map_err(|error| missing_wsl_or_dock(error))?;
    let stdout = orbcue_connect::decode_console_output(&output.stdout);
    let stderr = orbcue_connect::decode_console_output(&output.stderr);
    if !output.status.success() {
        return Err(wsl_orb_failed(format_exit_status(output.status), &stderr));
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

pub(crate) fn wsl_base_command() -> Command {
    if let Ok(distro) = env::var("ORBCUE_WSL_DISTRO") {
        if !distro.is_empty() {
            return wsl_command_for_distro(&distro);
        }
    }
    let mut command = Command::new("wsl.exe");
    orbcue_ipc::hide_windows_console(&mut command);
    command
}

pub(crate) fn wsl_command_for_distro(distro: &str) -> Command {
    let mut command = Command::new("wsl.exe");
    if !distro.is_empty() {
        command.args(["-d", distro]);
    }
    orbcue_ipc::hide_windows_console(&mut command);
    command
}

pub(crate) fn wsl_list_command() -> Command {
    let mut command = Command::new("wsl.exe");
    command.args(["-l", "-q"]);
    orbcue_ipc::hide_windows_console(&mut command);
    command
}

fn wsl_dock_command(args: &[&str]) -> Result<Command, String> {
    let mut command = wsl_base_command();
    command.args([
        "-e",
        "sh",
        "-c",
        r#"exec "$HOME/.local/bin/orb" "$@""#,
        "sh",
    ]);
    command.args(args);
    command.env("ORBCUE_BACKEND", "local");
    let extra = "ORBCUE_BACKEND/u";
    match env::var("WSLENV") {
        Ok(existing)
            if existing
                .split(':')
                .any(|part| part.starts_with("ORBCUE_BACKEND")) => {}
        Ok(existing) if !existing.is_empty() => {
            command.env("WSLENV", format!("{existing}:{extra}"));
        }
        _ => {
            command.env("WSLENV", extra);
        }
    }
    Ok(command)
}

pub(crate) fn run_with_timeout(
    command: &mut Command,
    timeout: Duration,
) -> std::io::Result<std::process::Output> {
    orbcue_ipc::hide_windows_console(command);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    orbcue_ipc::wait_child_timeout(command.spawn()?, timeout)
}

fn format_exit_status(status: std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!(" (exit status: {code})"),
        None => format!(" ({status})"),
    }
}

fn missing_wsl_or_dock(error: std::io::Error) -> String {
    format!(
        "cannot start WSL orb via wsl.exe ({error}). Install WSL, or start OrbCue so it can install the WSL CLI"
    )
}

fn wsl_orb_failed(status: String, stderr: &str) -> String {
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("WSL orb failed{status}")
    } else {
        format!("WSL orb failed{status}: {stderr}")
    }
}
