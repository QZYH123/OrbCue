//! Agent process liveness for hooks and `orb liveness-check`.
use orbcue_core::{DockEvent, EventKind};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;

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
        if orbcue_ipc::linux_pid_is_dead(pid, starttime) != Some(true) {
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
    let mut pid = unsafe { libc::getppid() };
    if pid <= 1 {
        return None;
    }
    for _ in 0..8 {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (ppid, _, starttime) = orbcue_ipc::parse_proc_stat(&stat)?;
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if should_skip_linux_liveness_parent(comm.trim()) {
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
pub(crate) fn should_skip_linux_liveness_parent(comm: &str) -> bool {
    is_short_lived_hook_parent(comm) || is_wsl_session_plumbing(comm)
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
