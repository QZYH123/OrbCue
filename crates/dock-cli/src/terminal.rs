//! Windows Terminal adapter for `dock run`.
//!
//! Spawn lives in this process. Focus of the same `dock:` marker lives in the
//! Windows presenter (`src-tauri/src/focus.rs`).

use agent_activity_dock_connect::ConnectionManager;
use agent_activity_dock_core::{dock_tab_title, dock_terminal_marker, format_dock_marker};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait TerminalAdapter {
    fn spawn(&self, request: &SpawnRequest) -> Result<SpawnPlan, String>;
}

pub struct SpawnRequest {
    pub agent: String,
    pub marker: String,
    /// Windows Terminal profile name or GUID. Inherited from the current tab
    /// when omitted (`WT_PROFILE_ID`).
    pub profile: Option<String>,
    pub inner: InnerCommand,
}

pub enum InnerCommand {
    Wsl(WslInner),
    Native(NativeInner),
}

pub struct WslInner {
    pub distro: String,
    pub shell: String,
    pub cwd: PathBuf,
    /// Linux path of the tab bootstrap script. Avoids `;` / quotes on the WT
    /// command line — WT treats `;` as a command separator.
    pub run_script: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WslRunSpec {
    pub agent: String,
    pub marker: String,
    pub profile: Option<String>,
    pub distro: String,
    pub shell: String,
    pub cwd: String,
    pub run_script: String,
}

pub struct NativeInner {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub extra_env: Vec<(String, String)>,
}

pub struct WindowsTerminalAdapter {
    pub wt: PathBuf,
    pub wsl: Option<PathBuf>,
}

impl TerminalAdapter for WindowsTerminalAdapter {
    fn spawn(&self, request: &SpawnRequest) -> Result<SpawnPlan, String> {
        let plan = spawn_plan(&self.wt, self.wsl.as_deref(), request)?;
        execute_plan(&plan)?;
        Ok(plan)
    }
}

fn execute_plan(plan: &SpawnPlan) -> Result<(), String> {
    if launch_via_windows_shell(&plan.program) {
        return launch_wt_via_wscript(plan);
    }
    run_process(&plan.program, &plan.args)
}

fn run_process(program: &Path, args: &[String]) -> Result<(), String> {
    match ProcessCommand::new(program).args(args).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "Windows Terminal 退出码 {}",
            status.code().unwrap_or(1)
        )),
        Err(error) => Err(format!("无法启动 Windows Terminal（{}）", error)),
    }
}

/// Direct WSL exec of the WindowsApps `wt.exe` alias (2-byte MZ stub) flashes a
/// console and often returns 0 without creating a tab. Launch from a Windows
/// GUI host so the alias resolves, and pass `wsl.exe` as a Windows name.
fn launch_via_windows_shell(wt: &Path) -> bool {
    if cfg!(windows) {
        return false;
    }
    let text = wt.to_string_lossy();
    text.contains("WindowsApps")
        || text.contains("/mnt/c/")
        || text.contains("/mnt/C/")
        || text.contains(":\\")
}

fn launch_wt_via_wscript(plan: &SpawnPlan) -> Result<(), String> {
    let command = format!("wt.exe {}", wt_windows_command_line(&plan.args));
    let vbs = format!(
        "CreateObject(\"WScript.Shell\").Run {}, 0, False\n",
        vbs_string_literal(&command)
    );
    let script = windows_temp_script("aadock-wt", "vbs")?;
    write_utf16_le_bom(&script, &vbs)?;
    let windows_script = to_windows_path(&script);
    let wscript = find_wscript()
        .ok_or_else(|| "找不到 wscript.exe，无法从 WSL 启动 Windows Terminal".to_owned())?;
    let result = run_process(&wscript, &["//nologo".to_owned(), windows_script]);
    let _ = fs::remove_file(&script);
    result
}

pub fn wt_windows_command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| windows_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn windows_quote(arg: &str) -> String {
    if arg.is_empty() || arg.bytes().any(|byte| matches!(byte, b' ' | b'\t' | b'"')) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_owned()
    }
}

fn vbs_string_literal(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn write_utf16_le_bom(path: &Path, text: &str) -> Result<(), String> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(path, bytes).map_err(|error| format!("无法写入启动脚本：{error}"))
}

fn windows_temp_script(prefix: &str, ext: &str) -> Result<PathBuf, String> {
    let name = format!("{}-{}.{}", prefix, std::process::id(), ext);
    if let Some(temp) = windows_temp_dir() {
        fs::create_dir_all(&temp).map_err(|error| error.to_string())?;
        return Ok(temp.join(name));
    }
    Ok(env::temp_dir().join(name))
}

fn windows_temp_dir() -> Option<PathBuf> {
    if let Some(local) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        return Some(local.join("Temp"));
    }
    let user = env::var("USER")
        .ok()
        .or_else(|| env::var("USERNAME").ok())?;
    let temp = PathBuf::from(format!("/mnt/c/Users/{user}/AppData/Local/Temp"));
    temp.is_dir().then_some(temp)
}

fn to_windows_path(path: &Path) -> String {
    if let Ok(output) = ProcessCommand::new("wslpath").arg("-w").arg(path).output() {
        if output.status.success() {
            let converted = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if !converted.is_empty() {
                return converted;
            }
        }
    }
    path.display().to_string()
}

fn find_wscript() -> Option<PathBuf> {
    look_on_path("wscript.exe")
        .or_else(|| existing_path(PathBuf::from("/mnt/c/Windows/System32/wscript.exe")))
        .or_else(|| existing_path(PathBuf::from("/mnt/c/WINDOWS/Sysnative/wscript.exe")))
        .or_else(|| existing_path(PathBuf::from("/mnt/c/WINDOWS/system32/wscript.exe")))
        .or_else(|| existing_path(PathBuf::from(r"C:\Windows\System32\wscript.exe")))
}

pub struct SpawnPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub title: String,
}

static MARKERS: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static MARKER_SEQ: AtomicU64 = AtomicU64::new(1);

pub fn run_command(
    agent: &str,
    args: &[String],
    profile: Option<&str>,
    close: bool,
    json_output: bool,
) -> i32 {
    match run_command_inner(agent, args, profile) {
        Ok(started) => {
            let closed_launcher = close_launcher_after_spawn(close);
            if json_output {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "marker": started.marker,
                        "title": started.title,
                        "agent": started.agent,
                        "profile": started.profile,
                        "closed_launcher": closed_launcher,
                    })
                );
            } else {
                print!(
                    "Started {} in Windows Terminal tab {}",
                    started.agent, started.marker
                );
                if closed_launcher {
                    print!("; closing this tab");
                } else if close {
                    print!("; current stdin is not a TTY, launcher tab kept");
                }
                println!();
            }
            0
        }
        Err(error) => {
            if json_output {
                println!("{}", json!({ "ok": false, "error": error }));
            } else {
                eprintln!("dock run: {error}");
            }
            1
        }
    }
}

pub fn close_launcher_after_spawn(requested: bool) -> bool {
    if !should_close_launcher(requested, stdin_is_terminal()) {
        return false;
    }
    let _ = std::io::Write::flush(&mut std::io::stdout());
    close_launcher_shell()
}

pub(crate) fn should_close_launcher(requested: bool, stdin_is_terminal: bool) -> bool {
    requested && stdin_is_terminal
}

fn stdin_is_terminal() -> bool {
    std::io::IsTerminal::is_terminal(&std::io::stdin())
}

#[cfg(unix)]
fn close_launcher_shell() -> bool {
    let ppid = unsafe { libc::getppid() };
    if ppid <= 1 {
        return false;
    }
    unsafe { libc::kill(ppid, libc::SIGHUP) == 0 }
}

#[cfg(not(unix))]
fn close_launcher_shell() -> bool {
    false
}

pub struct StartedTab {
    pub agent: String,
    pub marker: String,
    pub title: String,
    pub profile: Option<String>,
}

fn run_command_inner(
    agent: &str,
    args: &[String],
    profile: Option<&str>,
) -> Result<StartedTab, String> {
    let cwd = env::current_dir().map_err(|error| format!("无法读取当前目录：{error}"))?;
    let marker = allocate_dock_marker();
    let profile = resolve_wt_profile(profile)?;
    let inner = if native_windows_run() {
        native_inner(agent, args, &cwd, &marker)?
    } else {
        wsl_inner(agent, args, &cwd, &marker)?
    };
    let adapter = WindowsTerminalAdapter {
        wt: find_wt()?,
        wsl: match &inner {
            InnerCommand::Wsl(_) => Some(find_wsl()?),
            InnerCommand::Native(_) => None,
        },
    };
    let request = SpawnRequest {
        agent: agent.to_owned(),
        marker: marker.clone(),
        profile: profile.clone(),
        inner,
    };
    let plan = adapter.spawn(&request)?;
    Ok(StartedTab {
        agent: agent.to_owned(),
        marker,
        title: plan.title,
        profile,
    })
}

pub fn prepare_wsl_run(
    agent: &str,
    args: &[String],
    profile: Option<&str>,
) -> Result<WslRunSpec, String> {
    let cwd = env::current_dir().map_err(|error| format!("无法读取当前目录：{error}"))?;
    let marker = allocate_dock_marker();
    let profile = resolve_wt_profile(profile)?;
    let InnerCommand::Wsl(inner) = wsl_inner(agent, args, &cwd, &marker)? else {
        return Err("WSL dock run produced a native inner command".to_owned());
    };
    Ok(WslRunSpec {
        agent: agent.to_owned(),
        marker,
        profile,
        distro: inner.distro,
        shell: inner.shell,
        cwd: inner.cwd.to_string_lossy().into_owned(),
        run_script: inner.run_script.to_string_lossy().into_owned(),
    })
}

pub fn spawn_from_wsl_spec(spec: &WslRunSpec) -> Result<StartedTab, String> {
    if dock_terminal_marker(&spec.marker).is_none() {
        return Err(format!("invalid dock marker {}", spec.marker));
    }
    let adapter = WindowsTerminalAdapter {
        wt: find_wt()?,
        wsl: Some(find_wsl()?),
    };
    let request = SpawnRequest {
        agent: spec.agent.clone(),
        marker: spec.marker.clone(),
        profile: spec.profile.clone(),
        inner: InnerCommand::Wsl(WslInner {
            distro: spec.distro.clone(),
            shell: spec.shell.clone(),
            cwd: PathBuf::from(&spec.cwd),
            run_script: PathBuf::from(&spec.run_script),
        }),
    };
    let plan = adapter.spawn(&request)?;
    Ok(StartedTab {
        agent: spec.agent.clone(),
        marker: spec.marker.clone(),
        title: plan.title,
        profile: spec.profile.clone(),
    })
}

pub fn run_from_wsl_stdin(json_output: bool) -> i32 {
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("dock run: cannot read WSL spec: {error}");
        return 2;
    }
    let spec: WslRunSpec = match serde_json::from_str(input.trim()) {
        Ok(spec) => spec,
        Err(error) => {
            eprintln!("dock run: invalid WSL spec ({error})");
            return 2;
        }
    };
    match spawn_from_wsl_spec(&spec) {
        Ok(started) => {
            if json_output {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "marker": started.marker,
                        "title": started.title,
                        "agent": started.agent,
                        "profile": started.profile,
                        "closed_launcher": false,
                    })
                );
            } else {
                println!(
                    "Started {} in Windows Terminal tab {}",
                    started.agent, started.marker
                );
            }
            0
        }
        Err(error) => {
            if json_output {
                println!("{}", json!({ "ok": false, "error": error }));
            } else {
                eprintln!("dock run: {error}");
            }
            1
        }
    }
}

fn native_windows_run() -> bool {
    cfg!(windows)
        && env::var("WSL_DISTRO_NAME")
            .ok()
            .map(|value| value.trim().is_empty())
            .unwrap_or(true)
}

fn wsl_inner(
    agent: &str,
    args: &[String],
    cwd: &Path,
    marker: &str,
) -> Result<InnerCommand, String> {
    let command = resolve_agent(agent)?;
    let distro = wsl_distro()?;
    let shell = env::var("SHELL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/bin/bash".to_owned());
    let extra_exports = forwarded_exports();
    let run_script = write_wsl_run_script(marker, &command, args, &extra_exports, &shell)?;
    Ok(InnerCommand::Wsl(WslInner {
        distro,
        shell,
        cwd: cwd.to_path_buf(),
        run_script,
    }))
}

fn native_inner(
    agent: &str,
    args: &[String],
    cwd: &Path,
    marker: &str,
) -> Result<InnerCommand, String> {
    let program = PathBuf::from(resolve_agent(agent)?);
    Ok(InnerCommand::Native(NativeInner {
        program,
        args: args.to_vec(),
        cwd: cwd.to_path_buf(),
        extra_env: vec![(
            "AGENT_ACTIVITY_DOCK_TERMINAL_ID".to_owned(),
            marker.to_owned(),
        )],
    }))
}

pub fn spawn_plan(
    wt: &Path,
    wsl: Option<&Path>,
    request: &SpawnRequest,
) -> Result<SpawnPlan, String> {
    if dock_terminal_marker(&request.marker).is_none() {
        return Err(format!("invalid dock marker {}", request.marker));
    }
    let cwd = inner_cwd(&request.inner)
        .to_str()
        .ok_or_else(|| "当前目录不是有效 UTF-8".to_owned())?;
    let title = dock_tab_title(&request.agent, Some(cwd), &request.marker);
    let mut args = vec!["-w".to_owned(), "0".to_owned(), "nt".to_owned()];
    if let Some(profile) = request
        .profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.push("--profile".to_owned());
        args.push(profile.to_owned());
    }
    args.extend([
        "--title".to_owned(),
        title.clone(),
        "--suppressApplicationTitle".to_owned(),
    ]);
    match &request.inner {
        InnerCommand::Wsl(inner) => {
            let wsl =
                wsl.ok_or_else(|| "找不到 wsl.exe。WSL 侧 dock run 需要 Win+WSL。".to_owned())?;
            args.extend(wsl_inner_args(wsl, inner)?);
        }
        InnerCommand::Native(inner) => args.extend(native_inner_args(inner)?),
    }
    Ok(SpawnPlan {
        program: wt.to_path_buf(),
        args,
        title,
    })
}

fn inner_cwd(inner: &InnerCommand) -> &Path {
    match inner {
        InnerCommand::Wsl(inner) => inner.cwd.as_path(),
        InnerCommand::Native(inner) => inner.cwd.as_path(),
    }
}

pub fn wsl_inner_args(wsl: &Path, inner: &WslInner) -> Result<Vec<String>, String> {
    if inner.distro.trim().is_empty() {
        return Err(
            "缺少 WSL 发行版名。在 WSL 里运行，或设置 AGENT_ACTIVITY_DOCK_WSL_DISTRO。".to_owned(),
        );
    }
    let cwd = inner
        .cwd
        .to_str()
        .ok_or_else(|| "当前目录不是有效 UTF-8".to_owned())?;
    Ok(vec![
        "--".to_owned(),
        windows_wsl_command(wsl),
        "-d".to_owned(),
        inner.distro.clone(),
        "--cd".to_owned(),
        cwd.to_owned(),
        "--".to_owned(),
        inner.shell.clone(),
        "-l".to_owned(),
        inner.run_script.display().to_string(),
    ])
}

pub fn native_inner_args(inner: &NativeInner) -> Result<Vec<String>, String> {
    let cwd = inner
        .cwd
        .to_str()
        .ok_or_else(|| "当前目录不是有效 UTF-8".to_owned())?;
    let mut args = vec!["--startingDirectory".to_owned(), cwd.to_owned()];
    for (key, value) in &inner.extra_env {
        if !valid_export_key(key) {
            continue;
        }
        args.push("--env".to_owned());
        args.push(format!("{key}={value}"));
    }
    args.push("--".to_owned());
    args.push(inner.program.display().to_string());
    args.extend(inner.args.iter().cloned());
    if args.iter().any(|arg| arg.contains(';')) {
        return Err("native dock run arguments cannot contain ';'".to_owned());
    }
    Ok(args)
}

fn windows_wsl_command(wsl: &Path) -> String {
    match wsl.file_name().and_then(|name| name.to_str()) {
        Some(name) if name.eq_ignore_ascii_case("wsl.exe") || name.eq_ignore_ascii_case("wsl") => {
            "wsl.exe".to_owned()
        }
        _ => wsl.display().to_string(),
    }
}

pub fn inner_script(
    command: &str,
    args: &[String],
    extra_exports: &[(String, String)],
    marker: &str,
    shell: &str,
) -> String {
    let mut parts = vec![posix_single_quote(command)];
    parts.extend(args.iter().map(|arg| posix_single_quote(arg)));
    let command = parts.join(" ");
    let mut lines = Vec::new();
    for (key, value) in extra_exports {
        if !valid_export_key(key) {
            continue;
        }
        lines.push(format!("export {key}={}", posix_single_quote(value)));
    }
    lines.push(format!(
        "export AGENT_ACTIVITY_DOCK_TERMINAL_ID={}",
        posix_single_quote(marker)
    ));
    lines.push(command);
    lines.push("rm -f -- \"$0\"".to_owned());
    lines.push(format!("exec {} -l", posix_single_quote(shell)));
    lines.join("\n")
}

fn write_wsl_run_script(
    marker: &str,
    command: &str,
    args: &[String],
    extra_exports: &[(String, String)],
    shell: &str,
) -> Result<PathBuf, String> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let name = format!("aadock-{}.sh", marker.replace(':', "-"));
    let path = dir.join(name);
    fs::write(
        &path,
        inner_script(command, args, extra_exports, marker, shell),
    )
    .map_err(|error| format!("无法写入启动脚本：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("无法设置启动脚本权限：{error}"))?;
    }
    Ok(path)
}

pub fn allocate_dock_marker() -> String {
    let mut guard = MARKERS.lock().unwrap_or_else(|error| error.into_inner());
    let taken = guard.get_or_insert_with(HashSet::new);
    for _ in 0..64 {
        let marker = format_dock_marker(next_marker_suffix());
        if taken.insert(marker.clone()) {
            return marker;
        }
    }
    let fallback = format_dock_marker(next_marker_suffix() ^ 0x00A5_A5A5);
    taken.insert(fallback.clone());
    fallback
}

fn next_marker_suffix() -> u32 {
    let seq = MARKER_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    (nanos ^ ((std::process::id() as u64) << 16) ^ seq.wrapping_mul(0x9E37_79B9)) as u32
}

fn resolve_agent(name: &str) -> Result<String, String> {
    if !valid_run_agent_name(name) {
        return Err("agent 名只能包含字母、数字、'.'、'_' 或 '-'".to_owned());
    }
    let dock_binary = env::current_exe().unwrap_or_else(|_| PathBuf::from("dock"));
    let manager = ConnectionManager::from_environment(dock_binary);
    if let Some(path) = look_on_path(name) {
        return Ok(path.to_string_lossy().into_owned());
    }
    if let Some(agent) = manager
        .discover()
        .into_iter()
        .find(|agent| agent.name == name)
    {
        return Ok(agent.path.to_string_lossy().into_owned());
    }
    if let Some(record) = manager
        .records()
        .into_iter()
        .find(|record| record.name == name)
    {
        let path = record.wrapper.unwrap_or(record.original);
        return Ok(path.to_string_lossy().into_owned());
    }
    Err(format!(
        "`{name}` 未连接，也不在 PATH 上。先执行 `dock connect {name}`，或确认该命令可用。"
    ))
}

fn resolve_wt_profile(explicit: Option<&str>) -> Result<Option<String>, String> {
    choose_wt_profile(
        explicit,
        env::var("AGENT_ACTIVITY_DOCK_WT_PROFILE").ok().as_deref(),
        env::var("WT_PROFILE_ID").ok().as_deref(),
    )
}

fn choose_wt_profile(
    explicit: Option<&str>,
    env_override: Option<&str>,
    wt_profile_id: Option<&str>,
) -> Result<Option<String>, String> {
    if let Some(value) = explicit {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        if !valid_wt_profile(trimmed) {
            return Err("Windows Terminal 配置文件名无效".to_owned());
        }
        return Ok(Some(trimmed.to_owned()));
    }
    for value in [env_override, wt_profile_id].into_iter().flatten() {
        let trimmed = value.trim();
        if valid_wt_profile(trimmed) {
            return Ok(Some(trimmed.to_owned()));
        }
    }
    Ok(None)
}

fn valid_wt_profile(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.chars().any(|character| character.is_control())
}

fn valid_run_agent_name(name: &str) -> bool {
    name != "."
        && name != ".."
        && !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
}

fn forwarded_exports() -> Vec<(String, String)> {
    [
        "PATH",
        "HOME",
        "AGENT_ACTIVITY_DOCK_SOCKET",
        "XDG_STATE_HOME",
        "XDG_RUNTIME_DIR",
    ]
    .into_iter()
    .filter_map(|key| {
        env::var(key)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(|value| (key.to_owned(), value))
    })
    .collect()
}

fn valid_export_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn wsl_distro() -> Result<String, String> {
    for key in ["WSL_DISTRO_NAME", "AGENT_ACTIVITY_DOCK_WSL_DISTRO"] {
        if let Ok(value) = env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_owned());
            }
        }
    }
    Err("缺少 WSL 发行版名。在 WSL 里运行，或设置 AGENT_ACTIVITY_DOCK_WSL_DISTRO。".to_owned())
}

fn find_wt() -> Result<PathBuf, String> {
    if let Some(path) = env_executable("AGENT_ACTIVITY_DOCK_WT") {
        return Ok(path);
    }
    look_on_path("wt.exe")
        .or_else(|| look_on_path("wt"))
        .or_else(windows_apps_wt)
        .ok_or_else(|| "找不到 Windows Terminal（wt.exe）。".to_owned())
}

fn find_wsl() -> Result<PathBuf, String> {
    if let Some(path) = env_executable("AGENT_ACTIVITY_DOCK_WSL") {
        return Ok(path);
    }
    look_on_path("wsl.exe")
        .or_else(|| look_on_path("wsl"))
        .or_else(|| existing_path(PathBuf::from("/mnt/c/Windows/System32/wsl.exe")))
        .or_else(|| existing_path(PathBuf::from(r"C:\Windows\System32\wsl.exe")))
        .ok_or_else(|| "找不到 wsl.exe。WSL 侧 dock run 需要 Win+WSL。".to_owned())
}

fn env_executable(key: &str) -> Option<PathBuf> {
    env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn windows_apps_wt() -> Option<PathBuf> {
    if let Some(local) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let candidate = local.join("Microsoft").join("WindowsApps").join("wt.exe");
        if let Some(path) = existing_path(candidate) {
            return Some(path);
        }
    }
    let user = env::var("USER")
        .ok()
        .or_else(|| env::var("USERNAME").ok())?;
    existing_path(PathBuf::from(format!(
        "/mnt/c/Users/{user}/AppData/Local/Microsoft/WindowsApps/wt.exe"
    )))
}

fn look_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path).find_map(|dir| {
        existing_path(dir.join(name)).or_else(|| {
            if cfg!(windows) || name.ends_with(".exe") {
                None
            } else {
                existing_path(dir.join(format!("{name}.exe")))
            }
        })
    })
}

fn existing_path(path: PathBuf) -> Option<PathBuf> {
    path.is_file().then_some(path)
}

pub fn posix_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{
        allocate_dock_marker, choose_wt_profile, inner_script, native_inner_args,
        posix_single_quote, should_close_launcher, spawn_plan, windows_quote,
        wt_windows_command_line, InnerCommand, NativeInner, SpawnRequest, WslInner,
    };
    use agent_activity_dock_core::{dock_terminal_marker, DOCK_MARKER_HEX_LEN};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn request() -> SpawnRequest {
        SpawnRequest {
            agent: "grok".to_owned(),
            marker: "dock:ab12cd".to_owned(),
            profile: None,
            inner: InnerCommand::Wsl(WslInner {
                distro: "Ubuntu".to_owned(),
                shell: "/bin/zsh".to_owned(),
                cwd: PathBuf::from("/home/qingz/projects/agent-activity-dock"),
                run_script: PathBuf::from("/tmp/aadock-dock-ab12cd.sh"),
            }),
        }
    }

    #[test]
    fn spawn_plan_names_the_tab_and_injects_the_marker() {
        let plan = spawn_plan(
            Path::new("/tmp/wt.exe"),
            Some(Path::new("/mnt/c/Windows/System32/wsl.exe")),
            &request(),
        )
        .unwrap();
        assert_eq!(plan.program, PathBuf::from("/tmp/wt.exe"));
        assert_eq!(plan.args[..4], ["-w", "0", "nt", "--title"]);
        assert_eq!(plan.title, "agent-activity-dock · grok · dock:ab12cd");
        assert!(plan.args.contains(&"--suppressApplicationTitle".to_owned()));
        assert_eq!(plan.args[4], plan.title);
        assert!(plan.args.contains(&"wsl.exe".to_owned()));
        assert!(plan.args.contains(&"--cd".to_owned()));
        assert_eq!(
            &plan.args[plan.args.len() - 2..],
            ["-l", "/tmp/aadock-dock-ab12cd.sh"]
        );
        assert!(
            !plan.args.iter().any(|arg| arg.contains(';')),
            "{:?}",
            plan.args
        );
        assert!(!plan.args.iter().any(|arg| arg == "-lc"));
        assert_eq!(
            dock_terminal_marker("dock:ab12cd").map(str::len),
            Some("dock:".len() + DOCK_MARKER_HEX_LEN)
        );
        assert!(!plan.args.contains(&"--profile".to_owned()));
    }

    #[test]
    fn spawn_plan_passes_the_windows_terminal_profile() {
        let mut req = request();
        req.profile = Some("{49e41c3b-ba28-5ee9-9084-d161a8acb68e}".to_owned());
        let plan = spawn_plan(
            Path::new("/tmp/wt.exe"),
            Some(Path::new("/mnt/c/Windows/System32/wsl.exe")),
            &req,
        )
        .unwrap();
        assert_eq!(
            plan.args[..6],
            [
                "-w",
                "0",
                "nt",
                "--profile",
                "{49e41c3b-ba28-5ee9-9084-d161a8acb68e}",
                "--title"
            ]
        );
    }

    #[test]
    fn resolve_wt_profile_prefers_explicit_then_env() {
        assert_eq!(
            choose_wt_profile(Some("Ubuntu-24.04"), Some("FromEnv"), Some("{guid}")).unwrap(),
            Some("Ubuntu-24.04".to_owned())
        );
        assert_eq!(
            choose_wt_profile(None, Some("FromEnv"), Some("{guid}")).unwrap(),
            Some("FromEnv".to_owned())
        );
        assert_eq!(
            choose_wt_profile(None, None, Some("{49e41c3b-ba28-5ee9-9084-d161a8acb68e}")).unwrap(),
            Some("{49e41c3b-ba28-5ee9-9084-d161a8acb68e}".to_owned())
        );
        assert!(choose_wt_profile(Some("bad\nname"), None, None).is_err());
        assert_eq!(choose_wt_profile(Some("  "), None, None).unwrap(), None);
        assert_eq!(choose_wt_profile(None, None, None).unwrap(), None);
    }

    #[test]
    fn inner_script_forwards_socket_and_quotes_args() {
        let script = inner_script(
            "/home/qingz/.local/bin/grok",
            &["--foo".to_owned(), "bar baz".to_owned()],
            &[(
                "AGENT_ACTIVITY_DOCK_SOCKET".to_owned(),
                "/tmp/dock.sock".to_owned(),
            )],
            "dock:ab12cd",
            "/bin/zsh",
        );
        assert!(script.contains("export AGENT_ACTIVITY_DOCK_SOCKET='/tmp/dock.sock'"));
        assert!(script.contains("export AGENT_ACTIVITY_DOCK_TERMINAL_ID='dock:ab12cd'"));
        assert!(script.contains("'/home/qingz/.local/bin/grok' '--foo' 'bar baz'"));
        assert!(script.contains("rm -f -- \"$0\""));
        assert!(
            !script.contains(';'),
            "script uses newlines so WT never sees ';': {script}"
        );
        assert_eq!(posix_single_quote("it's"), "'it'\\''s'");
        assert_eq!(windows_quote("nt"), "nt");
        assert_eq!(
            windows_quote("grok · dock — dock:ab12cd"),
            "\"grok · dock — dock:ab12cd\""
        );
        let line = wt_windows_command_line(&[
            "-w".to_owned(),
            "0".to_owned(),
            "nt".to_owned(),
            "--title".to_owned(),
            "grok · app — dock:ab12cd".to_owned(),
        ]);
        assert_eq!(line, "-w 0 nt --title \"grok · app — dock:ab12cd\""); // quoting only
    }

    #[test]
    fn native_inner_args_set_directory_env_and_program() {
        let args = native_inner_args(&NativeInner {
            program: PathBuf::from(r"C:\Users\qingz\AppData\Local\grok.exe"),
            args: vec!["--foo".to_owned()],
            cwd: PathBuf::from(r"C:\work\app"),
            extra_env: vec![(
                "AGENT_ACTIVITY_DOCK_TERMINAL_ID".to_owned(),
                "dock:ab12cd".to_owned(),
            )],
        })
        .unwrap();
        assert_eq!(
            args[..4],
            [
                "--startingDirectory",
                r"C:\work\app",
                "--env",
                "AGENT_ACTIVITY_DOCK_TERMINAL_ID=dock:ab12cd"
            ]
        );
        assert_eq!(
            args[4..],
            ["--", r"C:\Users\qingz\AppData\Local\grok.exe", "--foo"]
        );
        assert!(!args.iter().any(|arg| arg.contains(';')));
    }

    #[test]
    fn wsl_run_spec_round_trips_into_spawn_plan() {
        let spec = super::WslRunSpec {
            agent: "grok".to_owned(),
            marker: "dock:ab12cd".to_owned(),
            profile: None,
            distro: "Ubuntu".to_owned(),
            shell: "/bin/zsh".to_owned(),
            cwd: "/home/qingz/app".to_owned(),
            run_script: "/tmp/aadock-dock-ab12cd.sh".to_owned(),
        };
        let encoded = serde_json::to_string(&spec).unwrap();
        let decoded: super::WslRunSpec = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, spec);
        let request = SpawnRequest {
            agent: spec.agent.clone(),
            marker: spec.marker.clone(),
            profile: spec.profile.clone(),
            inner: InnerCommand::Wsl(WslInner {
                distro: spec.distro,
                shell: spec.shell,
                cwd: PathBuf::from(spec.cwd),
                run_script: PathBuf::from(spec.run_script),
            }),
        };
        let plan = spawn_plan(
            Path::new("/tmp/wt.exe"),
            Some(Path::new("/mnt/c/Windows/System32/wsl.exe")),
            &request,
        )
        .unwrap();
        assert!(plan.args.contains(&"Ubuntu".to_owned()));
        assert!(plan.args.contains(&"/tmp/aadock-dock-ab12cd.sh".to_owned()));
        assert!(plan.args.contains(&"wsl.exe".to_owned()));
    }

    #[test]
    fn spawn_plan_native_does_not_invoke_wsl() {
        let request = SpawnRequest {
            agent: "grok".to_owned(),
            marker: "dock:ab12cd".to_owned(),
            profile: None,
            inner: InnerCommand::Native(NativeInner {
                program: PathBuf::from(r"C:\grok.exe"),
                args: Vec::new(),
                cwd: PathBuf::from(r"C:\work"),
                extra_env: vec![(
                    "AGENT_ACTIVITY_DOCK_TERMINAL_ID".to_owned(),
                    "dock:ab12cd".to_owned(),
                )],
            }),
        };
        let plan = spawn_plan(Path::new("/tmp/wt.exe"), None, &request).unwrap();
        assert!(!plan.args.iter().any(|arg| arg.contains("wsl")));
        assert!(plan.args.contains(&"--startingDirectory".to_owned()));
        assert_eq!(plan.title, "work · grok · dock:ab12cd");
    }

    #[test]
    fn close_flag_only_closes_an_interactive_tty() {
        assert!(!should_close_launcher(false, true));
        assert!(!should_close_launcher(true, false));
        assert!(!should_close_launcher(false, false));
        assert!(should_close_launcher(true, true));
    }

    #[test]
    fn markers_are_unique_in_process() {
        let mut seen = HashSet::new();
        for _ in 0..32 {
            let marker = allocate_dock_marker();
            assert_eq!(dock_terminal_marker(&marker), Some(marker.as_str()));
            assert!(seen.insert(marker), "marker should not repeat in-process");
        }
    }
}
