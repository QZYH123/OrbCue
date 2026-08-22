//! Windows Terminal adapter for `dock run`.
//!
//! Spawn lives in this process. Focus of the same `dock:` marker lives in the
//! Windows presenter (`src-tauri/src/focus.rs`).

use agent_activity_dock_connect::ConnectionManager;
use agent_activity_dock_core::{dock_tab_title, dock_terminal_marker, format_dock_marker};
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::fs;
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
    /// Absolute command used inside the tab. `wsl.exe -- bash script` is not a
    /// login shell, so a bare `grok` is often missing from PATH.
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub marker: String,
    pub distro: String,
    pub shell: String,
    pub extra_exports: Vec<(String, String)>,
    /// Linux path of the tab bootstrap script. Avoids `;` / quotes on the WT
    /// command line — WT treats `;` as a command separator.
    pub run_script: Option<PathBuf>,
}

pub struct WindowsTerminalAdapter {
    pub wt: PathBuf,
    pub wsl: PathBuf,
}

impl TerminalAdapter for WindowsTerminalAdapter {
    fn spawn(&self, request: &SpawnRequest) -> Result<SpawnPlan, String> {
        let plan = spawn_plan(&self.wt, &self.wsl, request)?;
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

pub fn run_command(agent: &str, args: &[String], json_output: bool) -> i32 {
    match run_command_inner(agent, args) {
        Ok(started) => {
            if json_output {
                println!(
                    "{}",
                    json!({
                        "ok": true,
                        "marker": started.marker,
                        "title": started.title,
                        "agent": started.agent,
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

struct StartedTab {
    agent: String,
    marker: String,
    title: String,
}

fn run_command_inner(agent: &str, args: &[String]) -> Result<StartedTab, String> {
    let command = resolve_agent(agent)?;
    let adapter = WindowsTerminalAdapter {
        wt: find_wt()?,
        wsl: find_wsl()?,
    };
    let cwd = env::current_dir().map_err(|error| format!("无法读取当前目录：{error}"))?;
    let distro = wsl_distro()?;
    let shell = env::var("SHELL")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/bin/bash".to_owned());
    let marker = allocate_dock_marker();
    let extra_exports = forwarded_exports();
    let mut request = SpawnRequest {
        agent: agent.to_owned(),
        command,
        args: args.to_vec(),
        cwd,
        marker: marker.clone(),
        distro,
        shell,
        extra_exports,
        run_script: None,
    };
    request.run_script = Some(write_run_script(&request)?);
    let plan = adapter.spawn(&request)?;
    Ok(StartedTab {
        agent: agent.to_owned(),
        marker,
        title: plan.title,
    })
}

pub fn spawn_plan(wt: &Path, wsl: &Path, request: &SpawnRequest) -> Result<SpawnPlan, String> {
    if dock_terminal_marker(&request.marker).is_none() {
        return Err(format!("invalid dock marker {}", request.marker));
    }
    if request.distro.trim().is_empty() {
        return Err(
            "缺少 WSL 发行版名。在 WSL 里运行，或设置 AGENT_ACTIVITY_DOCK_WSL_DISTRO。".to_owned(),
        );
    }
    let cwd = request
        .cwd
        .to_str()
        .ok_or_else(|| "当前目录不是有效 UTF-8".to_owned())?;
    let title = dock_tab_title(&request.agent, Some(cwd), &request.marker);
    let script = request
        .run_script
        .as_ref()
        .ok_or_else(|| "missing dock run bootstrap script".to_owned())?;
    Ok(SpawnPlan {
        program: wt.to_path_buf(),
        args: vec![
            "-w".to_owned(),
            "0".to_owned(),
            "nt".to_owned(),
            "--title".to_owned(),
            title.clone(),
            "--suppressApplicationTitle".to_owned(),
            "--".to_owned(),
            windows_wsl_command(wsl),
            "-d".to_owned(),
            request.distro.clone(),
            "--cd".to_owned(),
            cwd.to_owned(),
            "--".to_owned(),
            request.shell.clone(),
            "-l".to_owned(),
            script.display().to_string(),
        ],
        title,
    })
}

fn windows_wsl_command(wsl: &Path) -> String {
    match wsl.file_name().and_then(|name| name.to_str()) {
        Some(name) if name.eq_ignore_ascii_case("wsl.exe") || name.eq_ignore_ascii_case("wsl") => {
            "wsl.exe".to_owned()
        }
        _ => wsl.display().to_string(),
    }
}

pub fn inner_script(request: &SpawnRequest) -> String {
    let mut parts = vec![posix_single_quote(&request.command)];
    parts.extend(request.args.iter().map(|arg| posix_single_quote(arg)));
    let command = parts.join(" ");
    let mut lines = Vec::new();
    for (key, value) in &request.extra_exports {
        if !valid_export_key(key) {
            continue;
        }
        lines.push(format!("export {key}={}", posix_single_quote(value)));
    }
    lines.push(format!(
        "export AGENT_ACTIVITY_DOCK_TERMINAL_ID={}",
        posix_single_quote(&request.marker)
    ));
    lines.push(command);
    lines.push("rm -f -- \"$0\"".to_owned());
    lines.push(format!("exec {} -l", posix_single_quote(&request.shell)));
    lines.join("\n")
}

fn write_run_script(request: &SpawnRequest) -> Result<PathBuf, String> {
    let dir = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let name = format!("aadock-{}.sh", request.marker.replace(':', "-"));
    let path = dir.join(name);
    fs::write(&path, inner_script(request))
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
        .ok_or_else(|| {
            "找不到 Windows Terminal（wt.exe）。dock run 目前只支持 Win+WSL。".to_owned()
        })
}

fn find_wsl() -> Result<PathBuf, String> {
    if let Some(path) = env_executable("AGENT_ACTIVITY_DOCK_WSL") {
        return Ok(path);
    }
    look_on_path("wsl.exe")
        .or_else(|| look_on_path("wsl"))
        .or_else(|| existing_path(PathBuf::from("/mnt/c/Windows/System32/wsl.exe")))
        .or_else(|| existing_path(PathBuf::from(r"C:\Windows\System32\wsl.exe")))
        .ok_or_else(|| "找不到 wsl.exe。dock run 需要在 Win+WSL 下运行。".to_owned())
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
        allocate_dock_marker, inner_script, posix_single_quote, spawn_plan, windows_quote,
        wt_windows_command_line, SpawnRequest,
    };
    use agent_activity_dock_core::{dock_terminal_marker, DOCK_MARKER_HEX_LEN};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn request() -> SpawnRequest {
        SpawnRequest {
            agent: "grok".to_owned(),
            command: "/home/qingz/.local/bin/grok".to_owned(),
            args: vec!["--foo".to_owned(), "bar baz".to_owned()],
            cwd: PathBuf::from("/home/qingz/projects/agent-activity-dock"),
            marker: "dock:ab12cd".to_owned(),
            distro: "Ubuntu".to_owned(),
            shell: "/bin/zsh".to_owned(),
            extra_exports: vec![(
                "AGENT_ACTIVITY_DOCK_SOCKET".to_owned(),
                "/tmp/dock.sock".to_owned(),
            )],
            run_script: Some(PathBuf::from("/tmp/aadock-dock-ab12cd.sh")),
        }
    }

    #[test]
    fn spawn_plan_names_the_tab_and_injects_the_marker() {
        let plan = spawn_plan(
            Path::new("/tmp/wt.exe"),
            Path::new("/mnt/c/Windows/System32/wsl.exe"),
            &request(),
        )
        .unwrap();
        assert_eq!(plan.program, PathBuf::from("/tmp/wt.exe"));
        assert_eq!(plan.args[..4], ["-w", "0", "nt", "--title"]);
        assert_eq!(plan.title, "dock:ab12cd · grok · agent-activity-dock");
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
    }

    #[test]
    fn inner_script_forwards_socket_and_quotes_args() {
        let script = inner_script(&request());
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
    fn markers_are_unique_in_process() {
        let mut seen = HashSet::new();
        for _ in 0..32 {
            let marker = allocate_dock_marker();
            assert_eq!(dock_terminal_marker(&marker), Some(marker.as_str()));
            assert!(seen.insert(marker), "marker should not repeat in-process");
        }
    }
}
