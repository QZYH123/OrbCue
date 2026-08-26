use crate::wsl_session;
use orbcue_connect::{
    choose_packaged_linux_dock, decode_console_output, dock_version_matches,
    is_infrastructure_wsl_distro, packaged_linux_dock_candidates, packaged_linux_dock_is_usable,
    parse_wsl_distro_list, wsl_dock_cli_is_missing, wsl_dock_install_shell, wsl_runtime_is_absent,
};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

enum InstallOutcome {
    Done,
    Failed,
}

static INSTALL_STATE: Mutex<Option<InstallOutcome>> = Mutex::new(None);

pub fn spawn(app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("orb-wsl-cli-install".to_owned())
        .spawn(move || {
            let _ = ensure_installed(&app);
        });
}

pub fn ensure_installed(app: &AppHandle) -> Option<String> {
    let mut state = match INSTALL_STATE.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(InstallOutcome::Done) = *state {
        return None;
    }
    match install(app) {
        Ok(()) => {
            *state = Some(InstallOutcome::Done);
            None
        }
        Err(error) => {
            eprintln!("OrbCue: cannot install WSL orb CLI: {error}");
            *state = Some(InstallOutcome::Failed);
            Some(error)
        }
    }
}

fn install(app: &AppHandle) -> Result<(), String> {
    let Some(source) = find_packaged_linux_dock(app) else {
        eprintln!("OrbCue: packaged Linux orb (orb-wsl) was not found; skipping WSL CLI install");
        return Ok(());
    };
    match probe_wsl() {
        WslPresence::Absent => return Ok(()),
        WslPresence::Error(error) => return Err(error),
        WslPresence::Present => {}
    }
    let expected = env!("CARGO_PKG_VERSION");
    let pinned = env::var("ORBCUE_WSL_DISTRO")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    if pinned.is_some() {
        return install_into_wsl(&source, expected, None);
    }
    match list_wsl_distros() {
        Ok(names) if !names.is_empty() => install_into_listed_distros(&source, expected, &names),
        _ => install_into_wsl(&source, expected, None),
    }
}

fn list_wsl_distros() -> Result<Vec<String>, String> {
    let mut command = wsl_session::wsl_list_command();
    let output = run_wsl(&mut command)
        .map_err(|error| format!("cannot list WSL distros via wsl.exe ({error})"))?;
    if !output.status.success() {
        return Err(format!(
            "cannot list WSL distros ({})",
            output_detail(&output)
        ));
    }
    Ok(parse_wsl_distro_list(&output.stdout))
}

fn install_into_listed_distros(
    source: &Path,
    expected: &str,
    names: &[String],
) -> Result<(), String> {
    let default = names.first().map(String::as_str);
    let mut other_errors = Vec::new();
    for name in names
        .iter()
        .filter(|name| !is_infrastructure_wsl_distro(name))
    {
        if let Err(error) = install_into_wsl(source, expected, Some(name)) {
            if default == Some(name.as_str()) {
                return Err(error);
            }
            other_errors.push(format!("{name}: {error}"));
        }
    }
    if !other_errors.is_empty() {
        eprintln!(
            "OrbCue: WSL CLI install failed in non-default distros: {}",
            other_errors.join("; ")
        );
    }
    Ok(())
}

fn install_into_wsl(source: &Path, expected: &str, distro: Option<&str>) -> Result<(), String> {
    match installed_version_output(distro)? {
        Some(output) if dock_version_matches(&output, expected) => Ok(()),
        Some(_) | None => copy_into_wsl(source, distro),
    }
}

fn wsl_command(distro: Option<&str>) -> Command {
    match distro {
        Some(name) => wsl_session::wsl_command_for_distro(name),
        None => wsl_session::wsl_base_command(),
    }
}

fn find_packaged_linux_dock(app: &AppHandle) -> Option<PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let resource_dir = app.path().resource_dir().ok();
    let candidates = packaged_linux_dock_candidates(exe_dir.as_deref(), resource_dir.as_deref());
    choose_packaged_linux_dock(&candidates, packaged_linux_dock_is_usable)
}

enum WslPresence {
    Absent,
    Present,
    Error(String),
}

fn probe_wsl() -> WslPresence {
    let mut command = wsl_session::wsl_base_command();
    command.args(["-e", "sh", "-c", "exit 0"]);
    match run_wsl(&mut command) {
        Err(error) => {
            let message = format!("cannot start WSL orb via wsl.exe ({error})");
            if wsl_runtime_is_absent(&message) {
                WslPresence::Absent
            } else {
                WslPresence::Error(message)
            }
        }
        Ok(output) if output.status.success() => WslPresence::Present,
        Ok(output) => {
            let detail = output_detail(&output);
            if wsl_runtime_is_absent(&detail) {
                WslPresence::Absent
            } else {
                WslPresence::Error(format!("WSL is unavailable ({detail})"))
            }
        }
    }
}

fn installed_version_output(distro: Option<&str>) -> Result<Option<String>, String> {
    let mut command = wsl_command(distro);
    command.args(["-e", "sh", "-c", r#"exec "$HOME/.local/bin/orb" --version"#]);
    let output = run_wsl(&mut command)
        .map_err(|error| format!("cannot query WSL dock version via wsl.exe ({error})"))?;
    let stdout = decode_console_output(&output.stdout);
    if output.status.success() {
        return Ok(Some(stdout));
    }
    let detail = output_detail(&output);
    if wsl_runtime_is_absent(&detail) {
        return Ok(None);
    }
    if wsl_dock_cli_is_missing(&detail) {
        return Ok(None);
    }
    Err(format!("cannot read WSL dock version ({detail})"))
}

fn copy_into_wsl(windows_path: &Path, distro: Option<&str>) -> Result<(), String> {
    let linux_path = wslpath(windows_path, distro)?;
    let script = wsl_dock_install_shell(&linux_path);
    let mut command = wsl_command(distro);
    command.args(["-e", "sh", "-c", &script]);
    let output = run_wsl(&mut command)
        .map_err(|error| format!("cannot copy dock into WSL via wsl.exe ({error})"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "cannot install WSL dock ({})",
        output_detail(&output)
    ))
}

fn wslpath(windows_path: &Path, distro: Option<&str>) -> Result<String, String> {
    let mut command = wsl_command(distro);
    command.arg("-e").arg("wslpath").arg("-a").arg(windows_path);
    let output = run_wsl(&mut command)
        .map_err(|error| format!("cannot convert path with wslpath ({error})"))?;
    let path = decode_console_output(&output.stdout).trim().to_owned();
    if output.status.success() && !path.is_empty() {
        return Ok(path);
    }
    Err(format!(
        "wslpath failed for {} ({})",
        windows_path.display(),
        output_detail(&output)
    ))
}

fn run_wsl(command: &mut Command) -> std::io::Result<Output> {
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
}

fn output_detail(output: &Output) -> String {
    let stderr = decode_console_output(&output.stderr);
    let stdout = decode_console_output(&output.stdout);
    let status = match output.status.code() {
        Some(code) => format!("exit status: {code}"),
        None => output.status.to_string(),
    };
    let stderr = stderr.trim();
    let stdout = stdout.trim();
    if !stderr.is_empty() {
        format!("{status}: {stderr}")
    } else if !stdout.is_empty() {
        format!("{status}: {stdout}")
    } else {
        status
    }
}
