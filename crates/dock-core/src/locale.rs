//! Process language for user-facing copy.
//! Chinese when the locale is `zh`. English for every other explicit locale.
//! Unset stays Chinese, so tests and a missing locale keep the original copy.

use std::cell::Cell;
use std::sync::OnceLock;

thread_local! {
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

static PROCESS_LANG: OnceLock<Lang> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

#[cfg(test)]
pub(crate) struct LangGuard {
    previous: Option<Lang>,
}

#[cfg(test)]
impl Drop for LangGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|cell| cell.set(self.previous));
    }
}

#[cfg(test)]
pub(crate) fn force_lang(lang: Lang) -> LangGuard {
    LangGuard {
        previous: OVERRIDE.with(|cell| cell.replace(Some(lang))),
    }
}

pub fn apply_process_lang() {
    let _ = PROCESS_LANG.set(detect_process_lang());
}

pub fn ui_lang() -> Lang {
    if let Some(lang) = OVERRIDE.with(|cell| cell.get()) {
        return lang;
    }
    PROCESS_LANG.get().copied().unwrap_or(Lang::Zh)
}

pub fn pick<'a>(zh: &'a str, en: &'a str) -> &'a str {
    match ui_lang() {
        Lang::Zh => zh,
        Lang::En => en,
    }
}

fn classify_locale_tag(value: &str) -> Lang {
    let tag = value.split(['.', '@']).next().unwrap_or(value);
    let primary = tag.split(['_', '-']).next().unwrap_or(tag);
    if primary.eq_ignore_ascii_case("zh") {
        Lang::Zh
    } else {
        Lang::En
    }
}

fn detect_process_lang() -> Lang {
    if let Ok(value) = std::env::var("ORBCUE_LANG") {
        let value = value.trim();
        if !value.is_empty() {
            return classify_locale_tag(value);
        }
    }
    #[cfg(windows)]
    {
        return windows_ui_lang();
    }
    #[cfg(not(windows))]
    {
        unix_env_lang()
    }
}

#[cfg(windows)]
fn windows_ui_lang() -> Lang {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // LANG_CHINESE is 0x04, including zh-CN, zh-TW, and zh-HK.
    let primary = unsafe { GetUserDefaultUILanguage() } & 0x3ff;
    if primary == 0x04 {
        Lang::Zh
    } else {
        Lang::En
    }
}

#[cfg(not(windows))]
fn unix_env_lang() -> Lang {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        let Ok(value) = std::env::var(key) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value == "C" || value.starts_with("C.") {
            continue;
        }
        return classify_locale_tag(value);
    }
    Lang::Zh
}

#[macro_export]
macro_rules! t {
    ($zh:literal, $en:literal $(, $($arg:tt)+)?) => {{
        match $crate::ui_lang() {
            $crate::Lang::Zh => format!($zh $(, $($arg)+)?),
            $crate::Lang::En => format!($en $(, $($arg)+)?),
        }
    }};
}

#[cfg(test)]
mod tests {
    use super::{classify_locale_tag, force_lang, pick, Lang};

    #[test]
    fn tags_split_chinese_from_everything_else() {
        assert_eq!(classify_locale_tag("zh_CN.UTF-8"), Lang::Zh);
        assert_eq!(classify_locale_tag("zh-TW"), Lang::Zh);
        assert_eq!(classify_locale_tag("zh_HK"), Lang::Zh);
        assert_eq!(classify_locale_tag("en_US.UTF-8"), Lang::En);
        assert_eq!(classify_locale_tag("en"), Lang::En);
        assert_eq!(classify_locale_tag("fr_FR"), Lang::En);
    }

    #[test]
    fn default_copy_stays_chinese() {
        assert_eq!(pick("等待输入", "Waiting for input"), "等待输入");
    }

    #[test]
    fn forced_english_is_thread_local() {
        let _guard = force_lang(Lang::En);
        assert_eq!(pick("等待输入", "Waiting for input"), "Waiting for input");
    }
}
