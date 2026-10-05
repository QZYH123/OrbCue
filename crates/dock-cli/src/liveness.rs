//! Agent process liveness for hooks and `orb liveness-check`.
use orbcue_core::{DockEvent, EventKind};
use serde_json::Value;
#[cfg(unix)]
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::io::Read;
#[cfg(unix)]
use std::path::PathBuf;

pub(crate) fn platform_parent_liveness() -> Option<(u32, u64)> {
    #[cfg(unix)]
    {
        return linux_agent_liveness();
    }
    #[cfg(windows)]
    {
        return windows_parent_liveness();
    }
    #[cfg(not(any(unix, windows)))]
    None
}

pub(crate) fn liveness_from_env() -> Option<(u32, u64)> {
    let pid = std::env::var("ORBCUE_AGENT_PID").ok()?.parse().ok()?;
    let starttime = std::env::var("ORBCUE_AGENT_STARTTIME").ok()?.parse().ok()?;
    (pid > 0).then_some((pid, starttime))
}

pub(crate) fn attach_liveness(event: &mut DockEvent) {
    if event
        .parent_session_id
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        return;
    }
    // Completed/Failed/Cancelled update the live record; a short-lived hook
    // parent must not replace the agent PID. Closed only uses liveness to pick
    // which instance to remove and does not merge onto remaining sessions.
    if matches!(
        event.kind,
        EventKind::Completed | EventKind::Failed | EventKind::Cancelled
    ) {
        return;
    }
    let Some((pid, starttime)) = liveness_from_env().or_else(platform_parent_liveness) else {
        return;
    };
    event
        .metadata
        .insert("agent_os".to_owned(), current_agent_os().to_owned());
    event
        .metadata
        .insert("agent_pid".to_owned(), pid.to_string());
    event
        .metadata
        .insert("agent_starttime".to_owned(), starttime.to_string());
    #[cfg(unix)]
    if let Ok(distro) = std::env::var("WSL_DISTRO_NAME") {
        let trimmed = distro.trim();
        if !trimmed.is_empty() {
            event
                .metadata
                .insert("agent_wsl_distro".to_owned(), trimmed.to_owned());
        }
    }
}

fn current_agent_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn is_short_lived_windows_hook_parent(exe: &str) -> bool {
    let name = exe.rsplit(['/', '\\']).next().unwrap_or(exe).trim();
    let stem = strip_ascii_suffix_ignore_case(name, ".exe");
    // Cursor-agent.exe / node.exe under cursor-agent\versions are the
    // agent and must not be skipped. Windows has no bash -O extglob
    // sandbox; cmd.exe wrappers are already skipped here.
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "cmd" | "conhost" | "cmd.com" | "orb"
    )
}

fn strip_ascii_suffix_ignore_case<'a>(value: &'a str, suffix: &str) -> &'a str {
    let start = value.len().saturating_sub(suffix.len());
    if value
        .get(start..)
        .is_some_and(|end| end.eq_ignore_ascii_case(suffix))
    {
        &value[..start]
    } else {
        value
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn resolve_windows_liveness_pid(
    current: u32,
    processes: &[(u32, u32, &str)],
) -> Option<u32> {
    let mut by_pid = HashMap::with_capacity(processes.len());
    for (pid, parent, name) in processes {
        by_pid.insert(*pid, (*parent, *name));
    }
    let mut pid = by_pid.get(&current)?.0;
    if pid == 0 {
        return None;
    }
    for _ in 0..8 {
        let (parent, name) = *by_pid.get(&pid)?;
        if !is_short_lived_windows_hook_parent(name) {
            return Some(pid);
        }
        if parent == 0 || parent == pid {
            return None;
        }
        pid = parent;
    }
    None
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CloseHandle(handle: isize) -> i32;
}

/// Return the current process id and a compact parent/name index for the
/// Windows process tree. Both hook liveness and source detection need the same
/// Toolhelp snapshot; keeping the FFI and enumeration in one place avoids two
/// subtly different copies of it.
#[cfg(windows)]
pub(crate) fn windows_process_tree() -> Option<(u32, HashMap<u32, WindowsProcess>)> {
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
    }

    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const INVALID: isize = -1;

    unsafe {
        let current = GetCurrentProcessId();
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == 0 || snapshot == INVALID {
            return None;
        }
        let mut entry: ProcessEntry32W = zeroed();
        entry.dw_size = size_of::<ProcessEntry32W>() as u32;
        let mut processes = HashMap::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                processes.insert(
                    entry.th32_process_id,
                    WindowsProcess {
                        parent: entry.th32_parent_process_id,
                        name: utf16_z(&entry.sz_exe_file),
                    },
                );
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        Some((current, processes))
    }
}

#[cfg(windows)]
pub(crate) struct WindowsProcess {
    pub(crate) parent: u32,
    pub(crate) name: String,
}

#[cfg(windows)]
fn windows_parent_liveness() -> Option<(u32, u64)> {
    let (current, by_pid) = windows_process_tree()?;
    let named: Vec<(u32, u32, &str)> = by_pid
        .iter()
        .map(|(pid, process)| (*pid, process.parent, process.name.as_str()))
        .collect();
    let pid = resolve_windows_liveness_pid(current, &named)?;
    match orbcue_ipc::process_creation(pid) {
        orbcue_ipc::ProcessCreation::Time(creation) => Some((pid, creation)),
        _ => None,
    }
}

#[cfg(windows)]
fn utf16_z(buf: &[u16]) -> String {
    let end = buf.iter().position(|&unit| unit == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub(crate) fn run_liveness_check() -> i32 {
    let mut input = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut input) {
        eprintln!("orb liveness-check: cannot read stdin: {error}");
        return 2;
    }
    let queries: Vec<Value> = match serde_json::from_str(input.trim()) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("orb liveness-check: invalid JSON ({error})");
            return 2;
        }
    };
    let mut dead = Vec::new();
    #[cfg(unix)]
    let owned = linux_liveness_index();
    #[cfg(unix)]
    let processes: Vec<(i32, i32, &str, &str, u64)> = owned
        .iter()
        .map(|(pid, ppid, comm, cmdline, starttime)| {
            (*pid, *ppid, comm.as_str(), cmdline.as_str(), *starttime)
        })
        .collect();
    #[cfg(unix)]
    let open_opencode_tabs = opencode_tab_session_ids(&opencode_state_roots());
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
        #[cfg(unix)]
        let mut gone = orbcue_ipc::linux_pid_is_dead(pid, starttime) == Some(true);
        #[cfg(not(unix))]
        let gone = orbcue_ipc::linux_pid_is_dead(pid, starttime) == Some(true);
        // OpenCode's plugin runs inside `opencode serve`, which outlives the
        // terminal tab. No client left means that tab is gone. A client that
        // is still up drops one conversation when its id leaves tabs.json.
        // Titles in that file are ignored.
        #[cfg(unix)]
        if !gone && opencode_serve_without_client(pid, &processes) {
            gone = true;
        }
        #[cfg(unix)]
        if !gone && opencode_serve_pid(pid, &processes) {
            if opencode_tab_is_closed(
                query
                    .get("session_id")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                open_opencode_tabs.as_ref(),
            ) {
                gone = true;
            }
        }
        if !gone {
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

#[cfg(unix)]
fn linux_agent_liveness() -> Option<(u32, u64)> {
    let pid = unsafe { libc::getppid() };
    if pid <= 1 {
        return None;
    }
    let mut nodes = Vec::new();
    let mut current = pid;
    for _ in 0..8 {
        if current <= 1 {
            break;
        }
        let stat = match std::fs::read_to_string(format!("/proc/{current}/stat")) {
            Ok(stat) => stat,
            Err(_) => break,
        };
        let Some((ppid, _, starttime)) = orbcue_ipc::parse_proc_stat(&stat) else {
            break;
        };
        let comm = std::fs::read_to_string(format!("/proc/{current}/comm")).unwrap_or_default();
        let cmdline = linux_process_cmdline(current);
        nodes.push((current, ppid, comm, cmdline, starttime));
        if ppid <= 1 || ppid == current {
            break;
        }
        current = ppid;
    }
    let indexed: Vec<(i32, i32, &str, &str, u64)> = nodes
        .iter()
        .map(|(pid, ppid, comm, cmdline, starttime)| {
            (*pid, *ppid, comm.as_str(), cmdline.as_str(), *starttime)
        })
        .collect();
    resolve_linux_liveness_pid(pid, &indexed)
}

/// Walk from the hook's parent toward the agent. Cursor CLI wraps hooks in a
/// short-lived `bash -O extglob` sandbox; recording that bash makes the 15s
/// reaper emit `session.closed` while `cursor-agent` is still running.
#[cfg(unix)]
pub(crate) fn resolve_linux_liveness_pid(
    start_pid: i32,
    processes: &[(i32, i32, &str, &str, u64)],
) -> Option<(u32, u64)> {
    let mut by_pid = HashMap::with_capacity(processes.len());
    for (pid, ppid, comm, cmdline, starttime) in processes {
        by_pid.insert(*pid, (*ppid, *comm, *cmdline, *starttime));
    }
    let mut pid = start_pid;
    if pid <= 1 {
        return None;
    }
    for _ in 0..8 {
        let (ppid, comm, cmdline, starttime) = *by_pid.get(&pid)?;
        if should_skip_linux_liveness_parent_with_cmd(comm.trim(), cmdline) {
            if ppid <= 1 || ppid == pid {
                return None;
            }
            pid = ppid;
            continue;
        }
        return Some((pid as u32, starttime));
    }
    None
}

#[cfg(unix)]
pub(crate) fn linux_process_cmdline(pid: i32) -> String {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return String::new();
    };
    String::from_utf8_lossy(&bytes)
        .chars()
        .take(4096)
        .map(|character| if character == '\0' { ' ' } else { character })
        .collect()
}

#[cfg(all(unix, test))]
pub(crate) fn should_skip_linux_liveness_parent(comm: &str) -> bool {
    should_skip_linux_liveness_parent_with_cmd(comm, "")
}

#[cfg(unix)]
pub(crate) fn should_skip_linux_liveness_parent_with_cmd(comm: &str, cmdline: &str) -> bool {
    is_short_lived_hook_parent(comm)
        || is_wsl_session_plumbing(comm)
        || is_cursor_sandbox_shell(comm, cmdline)
}

#[cfg(unix)]
pub(crate) fn is_cursor_sandbox_shell(comm: &str, cmdline: &str) -> bool {
    // Interactive bash/zsh must stay visible: walking past a login shell
    // records SessionLeader/Relay and the session is never reaped. Only skip
    // the one-shot wrapper Cursor uses to spawn hook scripts.
    let comm = comm.rsplit('/').next().unwrap_or(comm);
    if !matches!(comm, "bash" | "zsh" | "dash" | "sh" | "fish") {
        return false;
    }
    cmdline.contains("__CURSOR_SANDBOX_ENV_RESTORE")
        || (cmdline.contains("-O extglob") && cmdline.contains(" -c "))
}

#[cfg(unix)]
pub(crate) fn is_short_lived_hook_parent(comm: &str) -> bool {
    // Generated hooks are `#!/bin/sh` + exec. Skip that wrapper (and `orb`
    // trampolines), but not bash/zsh/fish: those are often the user's
    // interactive shell or an agent launcher. Walking past them records a
    // PID that outlives the session, so close is never detected.
    matches!(comm, "sh" | "dash" | "orb")
}

/// WSL session plumbing outlives every agent in the tab. A detached hook
/// reparented here must not stamp that PID as the agent, or resume-fork
/// opens a ghost row that liveness never reaps.
#[cfg(unix)]
pub(crate) fn is_wsl_session_plumbing(comm: &str) -> bool {
    comm == "SessionLeader" || comm.starts_with("Relay(") || comm.starts_with("init-systemd")
}

/// `opencode serve` is the daemon. The process the user closes with the
/// terminal tab is the client, whose argv does not contain `serve`.
#[cfg(unix)]
pub(crate) fn is_opencode_serve(comm: &str, cmdline: &str) -> bool {
    is_opencode_process(comm, cmdline) && cmdline_has_arg(cmdline, "serve")
}

#[cfg(unix)]
pub(crate) fn is_opencode_client(comm: &str, cmdline: &str) -> bool {
    is_opencode_process(comm, cmdline) && !cmdline_has_arg(cmdline, "serve")
}

/// True when an OpenCode client other than `exclude_pid` is still running.
/// The exiting client stays in the process table while its exit hook runs
/// `spawnSync`, so that pid is not another client. `getppid` is not used:
/// `orb` may run only after that client is already gone.
#[cfg(unix)]
pub(crate) fn other_opencode_client_alive(
    exclude_pid: u32,
    processes: &[(i32, i32, &str, &str, u64)],
) -> bool {
    processes.iter().any(|(pid, _, comm, cmdline, _)| {
        let pid = *pid as u32;
        pid != 0 && pid != exclude_pid && is_opencode_client(comm, cmdline)
    })
}

#[cfg(unix)]
fn is_opencode_process(comm: &str, cmdline: &str) -> bool {
    fn is_opencode_name(value: &str) -> bool {
        let name = value.rsplit(['/', '\\']).next().unwrap_or(value);
        name.eq_ignore_ascii_case("opencode") || name.eq_ignore_ascii_case("opencode.exe")
    }
    if is_opencode_name(comm.trim()) {
        return true;
    }
    cmdline
        .split([' ', '\0'])
        .next()
        .is_some_and(is_opencode_name)
}

#[cfg(unix)]
fn cmdline_has_arg(cmdline: &str, arg: &str) -> bool {
    cmdline.split([' ', '\0']).any(|part| part == arg)
}

/// True when this pid is `opencode serve` and no OpenCode client process is
/// left. One client keeps every serve-stamped row, so two terminals that
/// share one daemon clear only after both are gone.
#[cfg(unix)]
pub(crate) fn opencode_serve_without_client(
    pid: u32,
    processes: &[(i32, i32, &str, &str, u64)],
) -> bool {
    let Some((_, _, comm, cmdline, _)) = processes
        .iter()
        .find(|(candidate, ..)| *candidate as u32 == pid)
    else {
        return false;
    };
    if !is_opencode_serve(comm, cmdline) {
        return false;
    }
    !processes.iter().any(|(candidate, _, comm, cmdline, _)| {
        *candidate as u32 != pid && is_opencode_client(comm, cmdline)
    })
}

#[cfg(unix)]
pub(crate) fn opencode_serve_pid(pid: u32, processes: &[(i32, i32, &str, &str, u64)]) -> bool {
    processes.iter().any(|(candidate, _, comm, cmdline, _)| {
        *candidate as u32 == pid && is_opencode_serve(comm, cmdline)
    })
}

/// Session ids in one OpenCode `tabs.json`. Titles are not collected.
#[cfg(unix)]
pub(crate) fn session_ids_in_tab_value(value: &Value) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let Some(root) = value.as_object() else {
        return ids;
    };
    collect_opencode_tab_state(&mut ids, root.get("global"));
    if let Some(cwd) = root.get("cwd").and_then(Value::as_object) {
        for state in cwd.values() {
            collect_opencode_tab_state(&mut ids, Some(state));
        }
    }
    ids
}

#[cfg(unix)]
fn collect_opencode_tab_state(ids: &mut BTreeSet<String>, state: Option<&Value>) {
    let Some(tabs) = state
        .and_then(|state| state.get("tabs"))
        .and_then(Value::as_array)
    else {
        return;
    };
    for tab in tabs {
        if let Some(id) = tab
            .get("sessionID")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            ids.insert(id.to_owned());
        }
    }
}

/// `None` means no tab file could be read, so a missing id is not a close.
/// An empty set means the client has no conversation tabs left.
#[cfg(unix)]
pub(crate) fn opencode_tab_is_closed(session_id: &str, open: Option<&BTreeSet<String>>) -> bool {
    match open {
        Some(open) => !session_id.is_empty() && !open.contains(session_id),
        None => false,
    }
}

#[cfg(unix)]
pub(crate) fn opencode_state_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        let root = PathBuf::from(xdg).join("opencode");
        if root.components().count() > 1 {
            roots.push(root);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let root = PathBuf::from(home).join(".local/state/opencode");
        if !roots.iter().any(|existing| existing == &root) {
            roots.push(root);
        }
    }
    roots
}

#[cfg(unix)]
pub(crate) fn opencode_tab_session_ids(roots: &[PathBuf]) -> Option<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    let mut found = false;
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let path = entry.path().join("tui").join("tabs.json");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(parsed) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            found = true;
            ids.extend(session_ids_in_tab_value(&parsed));
        }
    }
    found.then_some(ids)
}

#[cfg(unix)]
pub(crate) fn linux_liveness_index() -> Vec<(i32, i32, String, String, u64)> {
    let mut rows = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return rows;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if pid <= 0 {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        let Some((ppid, _, starttime)) = orbcue_ipc::parse_proc_stat(&stat) else {
            continue;
        };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let cmdline = linux_process_cmdline(pid);
        rows.push((pid, ppid, comm, cmdline, starttime));
    }
    rows
}
