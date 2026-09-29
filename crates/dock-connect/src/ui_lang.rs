//! Saved language for the panel, notifications, and `orb`.
//! Missing file means follow the system language.

use orbcue_core::Lang;
use orbcue_ipc::default_state_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct UiLangView {
    pub ok: bool,
    #[serde(default)]
    pub lang: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn saved_lang() -> Option<Lang> {
    fs::read_to_string(state_path())
        .ok()
        .as_deref()
        .and_then(parse_file)
}

pub fn pref() -> &'static str {
    match saved_lang() {
        Some(Lang::Zh) => "zh",
        Some(Lang::En) => "en",
        None => "system",
    }
}

pub fn set(value: &str) -> Result<&'static str, String> {
    let pref = parse_pref(value).ok_or_else(|| {
        orbcue_core::t!(
            "语言只能是系统、中文或 English",
            "Language must be system, zh, or en"
        )
    })?;
    let path = state_path();
    if pref == "system" {
        if path.exists() {
            fs::remove_file(&path).map_err(|error| error.to_string())?;
        }
        return Ok("system");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&path, format!("{pref}\n")).map_err(|error| error.to_string())?;
    Ok(pref)
}

pub fn view_ok(lang: &str) -> UiLangView {
    UiLangView {
        ok: true,
        lang: lang.to_owned(),
        error: None,
    }
}

pub fn view_err(error: String) -> UiLangView {
    UiLangView {
        ok: false,
        lang: pref().to_owned(),
        error: Some(error),
    }
}

pub fn parse_pref(value: &str) -> Option<&'static str> {
    match value.trim() {
        "system" => Some("system"),
        "zh" => Some("zh"),
        "en" => Some("en"),
        _ => None,
    }
}

fn parse_file(text: &str) -> Option<Lang> {
    match text.trim() {
        "zh" => Some(Lang::Zh),
        "en" => Some(Lang::En),
        _ => None,
    }
}

fn state_path() -> PathBuf {
    default_state_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ui-lang")
}

#[cfg(test)]
mod tests {
    use super::{parse_file, parse_pref};
    use orbcue_core::Lang;

    #[test]
    fn file_keeps_only_an_explicit_language() {
        assert_eq!(parse_file("zh\n"), Some(Lang::Zh));
        assert_eq!(parse_file("en"), Some(Lang::En));
        assert_eq!(parse_file("system"), None);
        assert_eq!(parse_file(""), None);
        assert_eq!(parse_file("fr"), None);
    }

    #[test]
    fn pref_accepts_system_zh_and_en() {
        assert_eq!(parse_pref(" system "), Some("system"));
        assert_eq!(parse_pref("zh"), Some("zh"));
        assert_eq!(parse_pref("en"), Some("en"));
        assert_eq!(parse_pref("english"), None);
    }
}
