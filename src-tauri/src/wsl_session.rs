use orbcue_connect::{
    AliasView, ConnectionPreview, ConnectionRecord, DirectRunView, DiscoveredAgent, ReplaceTabView,
};
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

fn alias_from_json(parsed: AliasView) -> Result<Option<String>, String> {
    json_ok(
        parsed.ok,
        parsed.alias,
        parsed.error,
        orbcue_core::pick("无法更新启动别名", "Couldn't update the run alias"),
    )
}

fn json_ok<T>(ok: bool, value: T, error: Option<String>, fallback: &str) -> Result<T, String> {
    if ok {
        Ok(value)
    } else {
        Err(error.unwrap_or_else(|| fallback.to_owned()))
    }
}

pub fn run_alias() -> Result<Option<String>, String> {
    alias_from_json(wsl_dock_json(&["alias", "--json"])?)
}

pub fn set_run_alias(name: Option<&str>) -> Result<Option<String>, String> {
    let parsed = match name {
        None => wsl_dock_json(&["alias", "--clear", "--json"])?,
        Some(name) => wsl_dock_json(&["alias", name, "--json"])?,
    };
    alias_from_json(parsed)
}

fn replace_tab_from_json(parsed: ReplaceTabView) -> Result<bool, String> {
    json_ok(
        parsed.ok,
        parsed.replace_tab,
        parsed.error,
        orbcue_core::pick(
            "无法更新替换标签页设置",
            "Couldn't update the replace-tab setting",
        ),
    )
}

pub fn set_replace_tab_on_run(enabled: bool) -> Result<bool, String> {
    wsl_switch("replace-tab", enabled, replace_tab_from_json)
}

fn direct_run_from_json(parsed: DirectRunView) -> Result<bool, String> {
    json_ok(
        parsed.ok,
        parsed.enabled,
        parsed.error,
        orbcue_core::pick("无法更新原命令启动", "Couldn't update the original command"),
    )
}

pub fn set_direct_run(enabled: bool) -> Result<bool, String> {
    wsl_switch("direct-run", enabled, direct_run_from_json)
}

fn wsl_switch<T, F>(command: &str, enabled: bool, finish: F) -> Result<bool, String>
where
    T: for<'de> Deserialize<'de>,
    F: FnOnce(T) -> Result<bool, String>,
{
    let flag = if enabled { "--enable" } else { "--disable" };
    finish(wsl_dock_json(&[command, flag, "--json"])?)
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

fn pinned_distro() -> Option<String> {
    env::var("ORBCUE_WSL_DISTRO")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(crate) fn wsl_base_command() -> Command {
    orbcue_ipc::wsl_command(pinned_distro().as_deref())
}

pub(crate) fn wsl_command_for_distro(distro: &str) -> Command {
    let distro = distro.trim();
    orbcue_ipc::wsl_command((!distro.is_empty()).then_some(distro))
}

pub(crate) fn wsl_list_command() -> Command {
    let mut command = orbcue_ipc::wsl_command(None);
    command.args(["-l", "-q"]);
    command
}

fn wsl_dock_command(args: &[&str]) -> Result<Command, String> {
    Ok(orbcue_ipc::wsl_orb_command(
        pinned_distro().as_deref(),
        args,
    ))
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
