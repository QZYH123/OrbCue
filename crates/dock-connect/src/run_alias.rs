//! User-chosen short command that execs `dock run`.

use agent_activity_dock_ipc::default_state_path;
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "agent-activity-dock run-alias";
const RESERVED: &[&str] = &[
    "dock",
    "dockd",
    "wsl",
    "wt",
    "cmd",
    "powershell",
    "pwsh",
    "sudo",
    "ssh",
];

#[derive(Debug, Serialize)]
pub struct AliasView {
    pub ok: bool,
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn validate(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("别名不能为空".to_owned());
    }
    if name.len() > 24 {
        return Err("别名最多 24 个字符".to_owned());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("别名不能为空".to_owned());
    };
    if !first.is_ascii_alphabetic() {
        return Err("别名必须以字母开头".to_owned());
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
        return Err("别名只能用字母、数字、下划线和连字符".to_owned());
    }
    if RESERVED.iter().any(|item| item.eq_ignore_ascii_case(name)) {
        return Err(format!("不能占用 {name} 这个名字"));
    }
    Ok(name.to_owned())
}

pub fn current() -> Option<String> {
    let text = fs::read_to_string(state_path()).ok()?;
    let name = text.trim();
    if name.is_empty() {
        return None;
    }
    Some(name.to_owned())
}

pub fn set(name: Option<&str>) -> Result<Option<String>, String> {
    let previous = current();
    match name {
        None => {
            if let Some(old) = &previous {
                remove_shim(old);
            }
            let _ = fs::remove_file(state_path());
            Ok(None)
        }
        Some(raw) => {
            let name = validate(raw)?;
            write_shim(&name)?;
            if let Some(old) = previous {
                if old != name {
                    remove_shim(&old);
                }
            }
            write_state(&name)?;
            Ok(Some(name))
        }
    }
}

pub fn view_ok(alias: Option<String>) -> AliasView {
    AliasView {
        ok: true,
        alias,
        error: None,
    }
}

pub fn view_err(error: String) -> AliasView {
    AliasView {
        ok: false,
        alias: None,
        error: Some(error),
    }
}

fn state_path() -> PathBuf {
    default_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("run-alias")
}

fn bin_dir() -> PathBuf {
    if let Some(path) = env::var_os("AGENT_ACTIVITY_DOCK_BIN").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    #[cfg(windows)]
    {
        return default_state_path()
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    #[cfg(not(windows))]
    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("bin"))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn shim_path(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        bin_dir().join(format!("{name}.cmd"))
    }
    #[cfg(not(windows))]
    {
        bin_dir().join(name)
    }
}

fn is_ours(path: &Path) -> bool {
    fs::read_to_string(path)
        .map(|text| text.contains(MARKER))
        .unwrap_or(false)
}

fn write_shim(name: &str) -> Result<(), String> {
    let dir = bin_dir();
    fs::create_dir_all(&dir).map_err(|error| format!("无法创建 {dir:?}: {error}"))?;
    let path = shim_path(name);
    if path.exists() && !is_ours(&path) {
        return Err(format!(
            "已有同名命令 {}，换一个名字",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    fs::write(&path, shim_bytes())
        .map_err(|error| format!("无法写入 {}: {error}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("无法设置权限: {error}"))?;
    }
    Ok(())
}

fn remove_shim(name: &str) {
    let path = shim_path(name);
    if is_ours(&path) {
        let _ = fs::remove_file(path);
    }
}

fn write_state(name: &str) -> Result<(), String> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, format!("{name}\n")).map_err(|error| error.to_string())
}

fn shim_bytes() -> Vec<u8> {
    #[cfg(windows)]
    {
        format!("@echo off\r\nrem {MARKER}\r\n\"%~dp0dock.exe\" run %*\r\n").into_bytes()
    }
    #[cfg(not(windows))]
    {
        format!(
            "#!/bin/sh\n# {MARKER}\nbindir=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nif [ -x \"$bindir/dock\" ]; then\n  exec \"$bindir/dock\" run \"$@\"\nfi\nexec dock run \"$@\"\n"
        )
        .into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_short_names() {
        assert_eq!(validate("dr").unwrap(), "dr");
        assert_eq!(validate("run_agent").unwrap(), "run_agent");
        assert_eq!(validate("R1").unwrap(), "R1");
    }

    #[test]
    fn rejects_reserved_and_junk() {
        assert!(validate("dock").is_err());
        assert!(validate("1dr").is_err());
        assert!(validate("dock run").is_err());
        assert!(validate("../x").is_err());
        assert!(validate("").is_err());
    }
}
