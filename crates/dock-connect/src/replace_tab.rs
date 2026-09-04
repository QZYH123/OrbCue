//! Persist whether `orb run` should close the launcher tab.

use orbcue_ipc::default_state_path;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct ReplaceTabView {
    pub ok: bool,
    pub replace_tab: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn current() -> bool {
    fs::read_to_string(state_path())
        .map(|text| parse_enabled(&text))
        .unwrap_or(false)
}

pub fn set(enabled: bool) -> Result<bool, String> {
    let path = state_path();
    if enabled {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, "1\n").map_err(|error| error.to_string())?;
    } else if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    Ok(enabled)
}

pub fn view_ok(enabled: bool) -> ReplaceTabView {
    ReplaceTabView {
        ok: true,
        replace_tab: enabled,
        error: None,
    }
}

pub fn view_err(error: String) -> ReplaceTabView {
    ReplaceTabView {
        ok: false,
        replace_tab: false,
        error: Some(error),
    }
}

pub fn parse_enabled(text: &str) -> bool {
    text.trim() == "1"
}

fn state_path() -> PathBuf {
    default_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("run-replace-tab")
}

#[cfg(test)]
mod tests {
    use super::parse_enabled;

    #[test]
    fn only_explicit_one_enables_replace_tab() {
        assert!(parse_enabled("1\n"));
        assert!(parse_enabled("1"));
        assert!(!parse_enabled(""));
        assert!(!parse_enabled("0\n"));
        assert!(!parse_enabled("true"));
        assert!(!parse_enabled("yes"));
    }
}
