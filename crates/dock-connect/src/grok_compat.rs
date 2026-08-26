//! Read-only detection for Grok's Cursor-hook compatibility setting.

use serde_json::Value;
use std::fs;
use std::path::Path;

pub const GROK_COMPAT_CURSOR_HOOKS_WARNING: &str =
    "检测到 Grok 的 compat.cursor.hooks 已开启：Grok 会话可能重复执行 Cursor 的钩子，导致任务重复计数。建议在 ~/.grok/settings.json 里关掉它。";

pub fn grok_compat_cursor_hooks_enabled(grok_home: &Path) -> bool {
    let Ok(bytes) = fs::read(grok_home.join("settings.json")) else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return false;
    };
    match value.pointer("/compat/cursor/hooks") {
        Some(Value::Bool(true)) => true,
        Some(value) => value.get("enabled") == Some(&Value::Bool(true)),
        None => false,
    }
}

pub fn connection_warnings(name: &str, grok_home: &Path) -> Vec<String> {
    match name {
        "cursor" | "grok" if grok_compat_cursor_hooks_enabled(grok_home) => {
            vec![GROK_COMPAT_CURSOR_HOOKS_WARNING.to_owned()]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_grok_home() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("orbcue-grok-compat-{nonce}"));
        fs::create_dir_all(&root).expect("create temporary grok home");
        root
    }

    #[test]
    fn missing_settings_are_not_enabled() {
        let root = temp_grok_home();
        assert!(!grok_compat_cursor_hooks_enabled(&root));
        assert!(connection_warnings("cursor", &root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_settings_are_not_enabled_and_do_not_panic() {
        let root = temp_grok_home();
        fs::write(root.join("settings.json"), b"{not-json").unwrap();
        assert!(!grok_compat_cursor_hooks_enabled(&root));
        assert!(connection_warnings("grok", &root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn boolean_true_enables_the_warning() {
        let root = temp_grok_home();
        fs::write(
            root.join("settings.json"),
            br#"{"compat":{"cursor":{"hooks":true}}}"#,
        )
        .unwrap();
        assert!(grok_compat_cursor_hooks_enabled(&root));
        assert_eq!(
            connection_warnings("cursor", &root),
            vec![GROK_COMPAT_CURSOR_HOOKS_WARNING.to_owned()]
        );
        assert_eq!(
            connection_warnings("grok", &root),
            vec![GROK_COMPAT_CURSOR_HOOKS_WARNING.to_owned()]
        );
        assert!(connection_warnings("claude", &root).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn object_enabled_true_enables_the_warning() {
        let root = temp_grok_home();
        fs::write(
            root.join("settings.json"),
            br#"{"compat":{"cursor":{"hooks":{"enabled":true}}}}"#,
        )
        .unwrap();
        assert!(grok_compat_cursor_hooks_enabled(&root));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn false_and_missing_fields_are_not_enabled() {
        let root = temp_grok_home();
        fs::write(
            root.join("settings.json"),
            br#"{"compat":{"cursor":{"hooks":false}}}"#,
        )
        .unwrap();
        assert!(!grok_compat_cursor_hooks_enabled(&root));
        fs::write(
            root.join("settings.json"),
            br#"{"compat":{"cursor":{"hooks":{"enabled":false}}}}"#,
        )
        .unwrap();
        assert!(!grok_compat_cursor_hooks_enabled(&root));
        fs::write(root.join("settings.json"), br#"{"other":true}"#).unwrap();
        assert!(!grok_compat_cursor_hooks_enabled(&root));
        fs::remove_dir_all(root).unwrap();
    }
}
