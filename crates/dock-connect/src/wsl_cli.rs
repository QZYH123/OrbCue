//! Pure helpers for shipping a Linux `orb` into WSL from the Windows presenter.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const PACKAGED_LINUX_DOCK_NAME: &str = "orb-wsl";
pub const WSL_DOCK_BIN_DIR: &str = "$HOME/.local/bin";
pub const WSL_DOCK_TMP_NAME: &str = ".orb.tmp";
pub const WSL_DOCK_DEST_NAME: &str = "orb";

pub fn parse_dock_version_output(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let first = parts.next()?;
    let version = if first.eq_ignore_ascii_case("orb") {
        parts.next()?
    } else {
        first
    };
    if parts.next().is_some() || !looks_like_version(version) {
        return None;
    }
    Some(version.to_owned())
}

fn looks_like_version(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|character| character.is_ascii_digit())
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '+')
        })
}

pub fn dock_version_matches(output: &str, expected: &str) -> bool {
    parse_dock_version_output(output).as_deref() == Some(expected)
}

pub fn packaged_linux_dock_candidates(
    exe_dir: Option<&Path>,
    resource_dir: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = exe_dir {
        candidates.push(dir.join(PACKAGED_LINUX_DOCK_NAME));
    }
    if let Some(dir) = resource_dir {
        let direct = dir.join(PACKAGED_LINUX_DOCK_NAME);
        if !candidates.contains(&direct) {
            candidates.push(direct);
        }
        let nested = dir.join("resources").join(PACKAGED_LINUX_DOCK_NAME);
        if !candidates.contains(&nested) {
            candidates.push(nested);
        }
    }
    candidates
}

pub fn choose_packaged_linux_dock(
    candidates: &[PathBuf],
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    candidates
        .iter()
        .find(|path| exists(path.as_path()))
        .cloned()
}

pub fn looks_like_linux_dock(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x7fELF")
}

pub fn packaged_linux_dock_is_usable(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && looks_like_linux_dock(&magic)
}

pub fn wsl_dock_install_shell(source_linux_path: &str) -> String {
    let source = sh_single_quote(source_linux_path);
    let dir = WSL_DOCK_BIN_DIR;
    let tmp = format!("{dir}/{WSL_DOCK_TMP_NAME}");
    let dest = format!("{dir}/{WSL_DOCK_DEST_NAME}");
    format!("mkdir -p \"{dir}\" && cp {source} \"{tmp}\" && chmod 755 \"{tmp}\" && mv -f \"{tmp}\" \"{dest}\"")
}

fn sh_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn decode_console_output(bytes: &[u8]) -> String {
    if looks_like_utf16_le(bytes) {
        decode_utf16_le(bytes)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn looks_like_utf16_le(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return true;
    }
    if bytes.len() < 4 {
        return false;
    }
    let nul_count = bytes.iter().filter(|byte| **byte == 0).count();
    if nul_count * 4 >= bytes.len() {
        return true;
    }
    let even_len = bytes.len() - (bytes.len() % 2);
    if even_len < 4 {
        return false;
    }
    let pairs = even_len / 2;
    let even_nuls = (0..even_len).step_by(2).filter(|&i| bytes[i] == 0).count();
    let odd_nuls = (1..even_len).step_by(2).filter(|&i| bytes[i] == 0).count();
    even_nuls * 2 >= pairs || odd_nuls * 2 >= pairs
}

pub fn parse_wsl_distro_list(bytes: &[u8]) -> Vec<String> {
    decode_console_output(bytes)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn is_infrastructure_wsl_distro(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with("docker-desktop") || lowered.starts_with("rancher-desktop")
}

pub fn parse_installable_wsl_distros(bytes: &[u8]) -> Vec<String> {
    parse_wsl_distro_list(bytes)
        .into_iter()
        .filter(|name| !is_infrastructure_wsl_distro(name))
        .collect()
}

fn decode_utf16_le(bytes: &[u8]) -> String {
    let bytes = if bytes.starts_with(&[0xFF, 0xFE]) {
        &bytes[2..]
    } else {
        bytes
    };
    let even = bytes.len() - (bytes.len() % 2);
    let units: Vec<u16> = bytes[..even]
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clap_version_line() {
        assert_eq!(
            parse_dock_version_output("orb 0.2.0").as_deref(),
            Some("0.2.0")
        );
        assert!(dock_version_matches("orb 0.2.0\n", "0.2.0"));
        assert!(!dock_version_matches("orb 0.1.0", "0.2.0"));
    }

    #[test]
    fn rejects_empty_and_abnormal_version_output() {
        assert_eq!(parse_dock_version_output(""), None);
        assert_eq!(parse_dock_version_output("   \n"), None);
        assert_eq!(parse_dock_version_output("command not found"), None);
        assert_eq!(parse_dock_version_output("orb"), None);
        assert_eq!(parse_dock_version_output("orb 0.2.0 extra"), None);
        assert!(!dock_version_matches("", "0.2.0"));
        assert!(!dock_version_matches("oops", "0.2.0"));
    }

    #[test]
    fn packaged_linux_dock_prefers_exe_sibling() {
        let exe = Path::new("/portable");
        let resources = Path::new("/install/resources");
        let candidates = packaged_linux_dock_candidates(Some(exe), Some(resources));
        assert_eq!(candidates[0], PathBuf::from("/portable/orb-wsl"));
        assert_eq!(
            choose_packaged_linux_dock(&candidates, |_| true),
            Some(PathBuf::from("/portable/orb-wsl"))
        );
        assert_eq!(
            choose_packaged_linux_dock(&candidates, |path| path
                == Path::new("/install/resources/orb-wsl")),
            Some(PathBuf::from("/install/resources/orb-wsl"))
        );
        assert_eq!(packaged_linux_dock_candidates(None, None).len(), 0);
        assert_eq!(choose_packaged_linux_dock(&candidates, |_| false), None);
    }

    #[test]
    fn install_shell_replaces_atomically() {
        let command = wsl_dock_install_shell("/mnt/c/App/orb-wsl");
        assert!(command.contains("mkdir -p \"$HOME/.local/bin\""));
        assert!(command.contains("chmod 755"));
        assert!(command.contains("mv -f"));
        assert!(command.contains("$HOME/.local/bin/.orb.tmp"));
        assert!(command.contains("\"$HOME/.local/bin/orb\""));
        assert!(command.contains("'/mnt/c/App/orb-wsl'"));
    }

    #[test]
    fn usable_linux_dock_requires_elf_magic() {
        assert!(looks_like_linux_dock(b"\x7fELF\x02\x01"));
        assert!(!looks_like_linux_dock(b""));
        assert!(!looks_like_linux_dock(b"MZ"));
    }

    fn utf16_le_bytes(text: &str, bom: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if bom {
            bytes.extend_from_slice(&[0xFF, 0xFE]);
        }
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn utf16_le_no_distro_message_is_absent() {
        let message = "There are no installed distributions.";
        let with_bom = utf16_le_bytes(message, true);
        let without_bom = utf16_le_bytes(message, false);
        assert!(crate::wsl_side_is_absent(&decode_console_output(&with_bom)));
        assert!(crate::wsl_side_is_absent(&decode_console_output(
            &without_bom
        )));
    }

    #[test]
    fn utf8_console_output_is_unchanged() {
        let message = "There are no installed distributions.";
        assert_eq!(decode_console_output(message.as_bytes()), message);
        assert!(crate::wsl_side_is_absent(&decode_console_output(
            message.as_bytes()
        )));
        assert_eq!(decode_console_output(b"dock 0.2.0\n"), "dock 0.2.0\n");
        assert_eq!(
            decode_console_output(b"invalid orb bridge response"),
            "invalid orb bridge response"
        );
    }

    #[test]
    fn odd_length_utf16_does_not_panic() {
        let mut bytes = utf16_le_bytes("There are no installed distributions.", false);
        bytes.push(0x41);
        let decoded = decode_console_output(&bytes);
        assert!(decoded.contains("There are no installed distributions."));
        assert!(crate::wsl_side_is_absent(&decoded));
    }

    #[test]
    fn parse_wsl_distro_list_decodes_utf16_and_skips_infra() {
        let text = "Ubuntu-24.04\r\ndocker-desktop\r\ndocker-desktop-data\r\nrancher-desktop\r\nDebian\r\n";
        let bytes = utf16_le_bytes(text, true);
        assert_eq!(
            parse_wsl_distro_list(&bytes),
            [
                "Ubuntu-24.04",
                "docker-desktop",
                "docker-desktop-data",
                "rancher-desktop",
                "Debian"
            ]
        );
        assert_eq!(
            parse_installable_wsl_distros(&bytes),
            ["Ubuntu-24.04", "Debian"]
        );
        assert!(is_infrastructure_wsl_distro("docker-desktop"));
        assert!(is_infrastructure_wsl_distro("Docker-Desktop-Data"));
        assert!(is_infrastructure_wsl_distro("rancher-desktop"));
        assert!(!is_infrastructure_wsl_distro("Ubuntu-24.04"));
    }

    #[test]
    fn parse_wsl_distro_list_accepts_utf8_and_drops_blank_lines() {
        let text = "\nUbuntu\n\n  \ndocker-desktop\nFedoraLinux-42\n";
        assert_eq!(
            parse_installable_wsl_distros(text.as_bytes()),
            ["Ubuntu", "FedoraLinux-42"]
        );
    }
}
