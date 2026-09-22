//! Shared process queries for hook liveness and session reaping.
//!
//! `dock-cli` records a PID's start time; `dock-service` later asks whether
//! that same process is still the original one. One parser and one Windows
//! clock query keep those two answers from drifting.

/// `ppid`, `tty_nr`, and `starttime` from `/proc/<pid>/stat`.
pub fn parse_proc_stat(contents: &str) -> Option<(i32, u32, u64)> {
    let end = contents.rfind(')')?;
    let mut fields = contents.get(end + 1..)?.split_whitespace();
    let _state = fields.next()?;
    let ppid = fields.next()?.parse().ok()?;
    let _pgrp = fields.next()?;
    let _session = fields.next()?;
    let tty_nr = fields.next()?.parse().ok()?;
    for _ in 0..14 {
        fields.next()?;
    }
    let starttime = fields.next()?.parse().ok()?;
    Some((ppid, tty_nr, starttime))
}

/// `Some(true)` when the PID is gone or has been reused.
/// `Some(false)` when it is still the recorded process.
/// `None` when the process exists but cannot be inspected.
pub fn linux_pid_is_dead(pid: u32, starttime: u64) -> Option<bool> {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => Some(match parse_proc_stat(&stat) {
            Some((_, _, recorded)) => recorded != starttime,
            None => true,
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(true),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
        Err(_) => None,
    }
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessCreation {
    Time(u64),
    AccessDenied,
    Missing,
    Unavailable,
}

#[cfg(windows)]
pub fn process_creation(pid: u32) -> ProcessCreation {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const ERROR_ACCESS_DENIED: u32 = 5;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> isize;
        fn GetProcessTimes(
            process: isize,
            creation: *mut u64,
            exit: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
        fn CloseHandle(handle: isize) -> i32;
        fn GetLastError() -> u32;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 {
            return if GetLastError() == ERROR_ACCESS_DENIED {
                ProcessCreation::AccessDenied
            } else {
                ProcessCreation::Missing
            };
        }
        let mut creation = 0u64;
        let mut exit = 0u64;
        let mut kernel = 0u64;
        let mut user = 0u64;
        let ok = GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 {
            ProcessCreation::Unavailable
        } else {
            ProcessCreation::Time(creation)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_proc_stat;

    #[test]
    fn proc_stat_reads_ppid_tty_and_starttime() {
        let line = "35022 (grok) S 1000 35022 35022 34821 35022 0 0 0 0 0 0 0 0 0 0 0 0 0 12345";
        assert_eq!(parse_proc_stat(line), Some((1000, 34821, 12345)));
        let spaced = "12 (my (weird) name) R 99 12 12 0 12 0 0 0 0 0 0 0 0 0 0 0 0 0 4242";
        assert_eq!(parse_proc_stat(spaced), Some((99, 0, 4242)));
    }
}
