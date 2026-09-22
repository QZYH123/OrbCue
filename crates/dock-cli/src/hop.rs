//! WSL -> Windows trampoline. Event/status hop to the GUI-OS orb.exe.
use crate::Command;
use orbcue_ipc::WINDOWS_APP_FOLDER;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Output, Stdio};

pub(crate) fn stays_on_agent_os(command: &Command) -> bool {
    matches!(
        command,
        Command::Agents
            | Command::Connect { .. }
            | Command::Disconnect { .. }
            | Command::Alias { .. }
            | Command::ReplaceTab { .. }
            | Command::Run { .. }
            | Command::LivenessCheck
    )
}

pub(crate) fn hop_token() -> Option<String> {
    std::env::var("ORBCUE_HOP")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub(crate) fn looks_like_wsl() -> bool {
    env_nonempty("WSL_DISTRO_NAME")
        || env_nonempty("WSL_INTEROP")
        || env_nonempty("ORBCUE_WINDOWS_ORB")
}

pub(crate) fn env_nonempty(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

pub(crate) fn trampoline_to_windows_predicate(
    is_unix: bool,
    like_wsl: bool,
    hop_set: bool,
    stays_on_agent_os: bool,
) -> bool {
    is_unix && like_wsl && !hop_set && !stays_on_agent_os
}

pub(crate) fn should_trampoline_to_windows(command: &Command) -> bool {
    let hop_set = hop_token().is_some();
    let would = trampoline_to_windows_predicate(
        cfg!(unix),
        looks_like_wsl(),
        false,
        stays_on_agent_os(command),
    );
    if hop_set && would {
        eprintln!("orb: refusing hop, ORBCUE_HOP already set");
    }
    trampoline_to_windows_predicate(
        cfg!(unix),
        looks_like_wsl(),
        hop_set,
        stays_on_agent_os(command),
    )
}

pub(crate) fn should_trampoline_argv(command: &Command) -> bool {
    should_trampoline_to_windows(command) && !needs_local_event_prep(command)
}

pub(crate) fn needs_local_event_prep(command: &Command) -> bool {
    matches!(
        command,
        Command::Hook { .. }
            | Command::Start(_)
            | Command::Working(_)
            | Command::Waiting(_)
            | Command::Permission(_)
            | Command::Complete(_)
            | Command::Fail(_)
            | Command::Cancel(_)
    )
}

pub(crate) fn apply_windows_hop_env(command: &mut ProcessCommand) {
    command.env("ORBCUE_HOP", "windows");
    command.env_remove("ORBCUE_SOCKET");
    command.env_remove("XDG_RUNTIME_DIR");
}

fn windows_orb_command(
    args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
) -> Result<(PathBuf, ProcessCommand), i32> {
    let exe = match find_windows_dock() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("orb: {error}");
            return Err(2);
        }
    };
    let mut command = ProcessCommand::new(&exe);
    command.args(args);
    command.stderr(Stdio::inherit());
    apply_windows_hop_env(&mut command);
    Ok((exe, command))
}

pub(crate) fn trampoline_to_windows() -> i32 {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (exe, mut command) = match windows_orb_command(&args) {
        Ok(value) => value,
        Err(status) => return status,
    };
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("orb: cannot trampoline to {}: {error}", exe.display());
            2
        }
    }
}

pub(crate) fn trampoline_payload(
    args: &[&str],
    payload: &[u8],
    stdout: Stdio,
) -> Result<Output, i32> {
    let (exe, mut command) = windows_orb_command(args)?;
    command.stdin(Stdio::piped());
    command.stdout(stdout);
    let mut child = command.spawn().map_err(|error| {
        eprintln!("orb: cannot trampoline to {}: {error}", exe.display());
        2
    })?;
    let Some(mut stdin) = child.stdin.take() else {
        eprintln!("orb: trampoline lost stdin");
        return Err(2);
    };
    let mut line = payload.to_vec();
    line.push(b'\n');
    stdin.write_all(&line).map_err(|error| {
        eprintln!("orb: cannot write hop payload: {error}");
        2
    })?;
    drop(stdin);
    child.wait_with_output().map_err(|error| {
        eprintln!("orb: trampoline failed: {error}");
        2
    })
}

pub(crate) fn should_delegate_run_to_windows() -> bool {
    trampoline_to_windows_predicate(cfg!(unix), looks_like_wsl(), hop_token().is_some(), false)
}

pub(crate) fn find_windows_dock() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ORBCUE_WINDOWS_ORB")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "ORBCUE_WINDOWS_ORB is not a file: {}",
            path.display()
        ));
    }
    if let Some(path) = cached_windows_dock() {
        return Ok(path);
    }
    Err("cannot find Windows orb.exe; install the presenter, or set ORBCUE_WINDOWS_ORB".to_owned())
}

pub(crate) fn cached_windows_dock() -> Option<PathBuf> {
    static CACHED: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    CACHED.get_or_init(discover_windows_dock).clone()
}

pub(crate) fn discover_windows_dock() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(user) = std::env::var("USER")
        .ok()
        .or_else(|| std::env::var("USERNAME").ok())
    {
        candidates.push(PathBuf::from(format!(
            "/mnt/c/Users/{user}/AppData/Local/{WINDOWS_APP_FOLDER}/orb.exe"
        )));
    }
    if let Some(local) = orbcue_ipc::windows_app_data_dir() {
        candidates.push(local.join(WINDOWS_APP_FOLDER).join("orb.exe"));
    }
    if let Some(found) = candidates.into_iter().find(|path| path.is_file()) {
        return Some(found);
    }
    newest_windows_dock_under_mnt(Path::new("/mnt"))
}

pub(crate) fn newest_windows_dock_under_mnt(mnt_root: &Path) -> Option<PathBuf> {
    let mut hits = Vec::new();
    let mounts = fs::read_dir(mnt_root).ok()?;
    for mount in mounts.flatten() {
        let name = mount.file_name();
        let Some(letter) = name.to_str() else {
            continue;
        };
        if letter.len() != 1
            || !letter
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphabetic())
        {
            continue;
        }
        let users = mount.path().join("Users");
        let Ok(entries) = fs::read_dir(users) else {
            continue;
        };
        for user in entries.flatten() {
            let dock = user
                .path()
                .join("AppData")
                .join("Local")
                .join(WINDOWS_APP_FOLDER)
                .join("orb.exe");
            if !dock.is_file() {
                continue;
            }
            let mtime = fs::metadata(&dock).and_then(|meta| meta.modified()).ok();
            hits.push((mtime, dock));
        }
    }
    hits.into_iter()
        .max_by_key(|(mtime, _)| *mtime)
        .map(|(_, path)| path)
}
