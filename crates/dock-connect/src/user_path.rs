//! Put Dock's CLI directory on the current user's PATH.
//!
//! Shell profile snippets still exist for Unix. Windows cmd / PowerShell pick
//! up new terminals from the user Environment key, not from a profile file.

use std::path::{Path, PathBuf};

pub fn merge_path_entries(
    existing: &str,
    dir: &str,
    separator: char,
    ignore_case: bool,
) -> Option<String> {
    let dir = trim_path_entry(dir);
    if dir.is_empty() {
        return None;
    }
    if path_contains(existing, dir, separator, ignore_case) {
        return None;
    }
    if existing.trim().is_empty() {
        return Some(dir.to_owned());
    }
    Some(format!("{dir}{separator}{existing}"))
}

fn path_contains(existing: &str, dir: &str, separator: char, ignore_case: bool) -> bool {
    existing.split(separator).any(|part| {
        let part = trim_path_entry(part);
        if ignore_case {
            part.eq_ignore_ascii_case(dir)
        } else {
            part == dir
        }
    })
}

fn trim_path_entry(value: &str) -> &str {
    value.trim().trim_matches('"').trim_end_matches(['/', '\\'])
}

pub fn default_windows_cli_dir() -> Option<PathBuf> {
    orbcue_ipc::windows_app_data_dir().map(|dir| dir.join(orbcue_ipc::WINDOWS_APP_FOLDER))
}

pub fn install_windows_cli(source_exe: &Path) -> Result<PathBuf, String> {
    let dir = default_windows_cli_dir().ok_or_else(|| "LOCALAPPDATA is not set".to_owned())?;
    install_windows_cli_into(&dir, source_exe)
}

fn install_windows_cli_into(dir: &Path, source_exe: &Path) -> Result<PathBuf, String> {
    fs_create_dir_all(dir)?;
    let dest = dir.join("orb.exe");
    let mut errors = Vec::new();
    if source_exe.is_file() && !same_exe_file(source_exe, &dest) {
        if let Err(error) = copy_exe_replacing(source_exe, &dest) {
            errors.push(error);
        }
    }
    let cmd = dir.join("orb.cmd");
    if let Err(error) = std::fs::write(&cmd, b"@echo off\r\n\"%~dp0orb.exe\" %*\r\n") {
        errors.push(format!("cannot write {}: {error}", cmd.display()));
    }
    if let Err(error) = ensure_dir_on_user_path(dir) {
        errors.push(error);
    }
    if errors.is_empty() {
        Ok(dir.to_path_buf())
    } else {
        Err(errors.join("; "))
    }
}

fn copy_exe_replacing(source: &Path, dest: &Path) -> Result<(), String> {
    match std::fs::copy(source, dest) {
        Ok(_) => Ok(()),
        Err(error) => replace_locked_exe(source, dest, error),
    }
}

fn replace_locked_exe(source: &Path, dest: &Path, original: std::io::Error) -> Result<(), String> {
    if !dest.exists() {
        return Err(format!("cannot install {}: {original}", dest.display()));
    }
    let old = dest.with_file_name(format!(
        "{}.old",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&old);
    if std::fs::rename(dest, &old).is_err() {
        return Err(format!("cannot install {}: {original}", dest.display()));
    }
    match std::fs::copy(source, dest) {
        Ok(_) => {
            let _ = std::fs::remove_file(&old);
            Ok(())
        }
        Err(retry) => {
            restore_replaced_exe(dest, &old);
            Err(format!("cannot install {}: {retry}", dest.display()))
        }
    }
}

fn restore_replaced_exe(dest: &Path, old: &Path) {
    let _ = std::fs::remove_file(dest);
    if std::fs::rename(old, dest).is_ok() {
        return;
    }
    let _ = std::fs::copy(old, dest);
}

fn same_exe_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(windows)]
pub fn read_user_path() -> Option<String> {
    let key = windows_registry::CURRENT_USER.open("Environment").ok()?;
    let value = key.get_string("Path").ok()?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn ensure_dir_on_user_path(dir: &Path) -> Result<bool, String> {
    #[cfg(windows)]
    {
        windows_ensure_dir_on_user_path(dir)
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        Ok(false)
    }
}

#[cfg(any(windows, test))]
fn user_path_from_registry(
    string_value: Result<String, String>,
    type_exists: bool,
) -> Result<String, String> {
    match string_value {
        Ok(value) => Ok(value),
        Err(error) if type_exists => Err(format!(
            "user PATH exists but is not a readable string ({error}); refusing to overwrite"
        )),
        Err(_) => Ok(String::new()),
    }
}

fn fs_create_dir_all(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))
}

#[cfg(windows)]
fn windows_ensure_dir_on_user_path(dir: &Path) -> Result<bool, String> {
    let dir = dir
        .canonicalize()
        .unwrap_or_else(|_| dir.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_owned();
    let key = windows_registry::CURRENT_USER
        .create("Environment")
        .map_err(|error| format!("cannot open HKCU Environment: {error}"))?;
    let existing = user_path_from_registry(
        key.get_string("Path").map_err(|error| error.to_string()),
        key.get_type("Path").is_ok(),
    )?;
    let ty = key.get_type("Path").ok();
    let Some(merged) = merge_path_entries(&existing, &dir, ';', true) else {
        return Ok(false);
    };
    let write = if matches!(ty, Some(windows_registry::Type::ExpandString)) {
        key.set_expand_string("Path", &merged)
    } else {
        key.set_string("Path", &merged)
    };
    write.map_err(|error| format!("cannot write user PATH: {error}"))?;
    broadcast_environment_change();
    Ok(true)
}

#[cfg(windows)]
fn broadcast_environment_change() {
    const HWND_BROADCAST: isize = 0xffff;
    const WM_SETTINGCHANGE: u32 = 0x001A;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    #[link(name = "user32")]
    extern "system" {
        fn SendMessageTimeoutW(
            hwnd: isize,
            msg: u32,
            wparam: usize,
            lparam: *const u16,
            flags: u32,
            timeout: u32,
            result: *mut usize,
        ) -> isize;
    }
    let mut env: Vec<u16> = "Environment"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let mut result = 0usize;
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            env.as_mut_ptr(),
            SMTO_ABORTIFHUNG,
            1000,
            &mut result,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        install_windows_cli_into, merge_path_entries, path_contains, replace_locked_exe,
        user_path_from_registry,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("orbcue-user-path-{nonce}"));
        fs::create_dir_all(&root).expect("create temporary user-path directory");
        root
    }

    #[test]
    fn prepends_missing_windows_dir() {
        assert_eq!(
            merge_path_entries(
                r"C:\Windows;C:\Windows\System32",
                r"C:\Users\u\AppData\Local\OrbCue",
                ';',
                true,
            ),
            Some(r"C:\Users\u\AppData\Local\OrbCue;C:\Windows;C:\Windows\System32".into())
        );
    }

    #[test]
    fn skips_when_already_present_ignoring_case_and_slash() {
        assert_eq!(
            merge_path_entries(
                r"C:\Windows;C:\Users\u\AppData\Local\OrbCue\;C:\Windows\System32",
                r"c:\users\u\appdata\local\orbcue",
                ';',
                true,
            ),
            None
        );
        assert!(path_contains(
            r"C:\Windows;C:\Users\u\AppData\Local\OrbCue",
            r"C:\Users\u\AppData\Local\OrbCue",
            ';',
            true,
        ));
    }

    #[test]
    fn quoted_trailing_slash_matches_unquoted_dir() {
        assert_eq!(
            merge_path_entries(r#""C:\dir\";C:\Windows"#, r"C:\dir", ';', true),
            None
        );
    }

    #[test]
    fn empty_existing_becomes_the_dir() {
        assert_eq!(
            merge_path_entries("", r"C:\Dock", ';', true),
            Some(r"C:\Dock".into())
        );
        assert_eq!(merge_path_entries("  ", "", ';', true), None);
    }

    #[test]
    fn unreadable_existing_path_is_not_treated_as_empty() {
        assert_eq!(
            user_path_from_registry(Ok(r"C:\Windows".into()), true).unwrap(),
            r"C:\Windows"
        );
        assert_eq!(
            user_path_from_registry(Err("missing".into()), false).unwrap(),
            ""
        );
        let error = user_path_from_registry(Err("binary".into()), true).unwrap_err();
        assert!(error.contains("refusing to overwrite"));
    }

    #[test]
    fn install_runs_remaining_steps_after_exe_copy_fails() {
        let root = temp_root();
        let dest_dir = root.join("cli");
        fs::create_dir_all(dest_dir.join("orb.exe")).unwrap();
        fs::create_dir_all(dest_dir.join("orb.exe.old")).unwrap();
        fs::write(dest_dir.join("orb.exe.old").join("keep"), b"x").unwrap();
        let source = root.join("orb-src.exe");
        fs::write(&source, b"new").unwrap();

        let error = install_windows_cli_into(&dest_dir, &source).unwrap_err();
        assert!(error.contains("cannot install"));
        assert!(dest_dir.join("orb.cmd").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn install_keeps_orb_exe_when_source_is_the_same_file() {
        let root = temp_root();
        let dest_dir = root.join("cli");
        fs::create_dir_all(&dest_dir).unwrap();
        let dest = dest_dir.join("orb.exe");
        fs::write(&dest, b"cli-bytes").unwrap();
        fs::create_dir_all(dest_dir.join("sub")).unwrap();
        let source = dest_dir.join("sub").join("..").join("orb.exe");
        assert_ne!(source, dest);

        install_windows_cli_into(&dest_dir, &source).unwrap();

        assert!(dest.is_file(), "orb.exe must remain a file");
        assert_eq!(fs::read(&dest).unwrap(), b"cli-bytes");
        assert!(!dest_dir.join("orb.exe.old").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn install_restores_orb_exe_when_locked_replace_cannot_copy() {
        let root = temp_root();
        let dest = root.join("orb.exe");
        fs::write(&dest, b"keep-me").unwrap();
        let source = root.join("orb-src");
        fs::create_dir_all(&source).unwrap();

        let error = replace_locked_exe(
            &source,
            &dest,
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"),
        )
        .unwrap_err();
        assert!(error.contains("cannot install"));
        assert!(
            dest.is_file(),
            "orb.exe must be restored after a failed replace"
        );
        assert_eq!(fs::read(&dest).unwrap(), b"keep-me");
        fs::remove_dir_all(root).unwrap();
    }
}
