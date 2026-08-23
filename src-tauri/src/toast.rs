use agent_activity_dock_core::{NotificationSink, ToastSpec};
use tauri::AppHandle;

/// Unpackaged Win32 toasts can use this identity when our AUMID is not yet
/// registered. The banner then says PowerShell, but the toast still appears.
pub(crate) const POWERSHELL_TOAST_APP_ID: &str =
    "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

pub struct PresenterToastSink {
    pub app: AppHandle,
}

pub fn prepare_windows_notifications(app: &AppHandle) {
    #[cfg(windows)]
    windows_toast::prepare(app);
    #[cfg(not(windows))]
    let _ = app;
}

pub fn preview_attention_toast(app: &AppHandle) -> Result<(), String> {
    PresenterToastSink { app: app.clone() }.show(&ToastSpec {
        source: "dock".to_owned(),
        session_id: "preview".to_owned(),
        title: "系统通知已打开".to_owned(),
        body: "等待输入、授权或失败时会再弹出一次".to_owned(),
    })
}

impl NotificationSink for PresenterToastSink {
    fn show(&self, toast: &ToastSpec) -> Result<(), String> {
        #[cfg(windows)]
        {
            windows_toast::show(&self.app, toast)
        }
        #[cfg(not(windows))]
        {
            let _ = (self, toast);
            Ok(())
        }
    }
}

pub(crate) fn show_with_fallback(
    identifier: &str,
    mut show: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    match show(identifier) {
        Ok(()) => Ok(()),
        Err(first) => match show(POWERSHELL_TOAST_APP_ID) {
            Ok(()) => {
                eprintln!(
                    "Agent Activity Dock: toast app id {identifier} failed ({first}); used PowerShell identity"
                );
                Ok(())
            }
            Err(second) => Err(format!("{first}; fallback: {second}")),
        },
    }
}

#[cfg(windows)]
mod windows_toast {
    use super::{show_with_fallback, ToastSpec};
    use std::path::{Path, PathBuf};
    use tauri::{AppHandle, Manager};

    pub fn prepare(app: &AppHandle) {
        let identifier = app.config().identifier.clone();
        let name = app
            .config()
            .product_name
            .clone()
            .unwrap_or_else(|| "Agent Activity Dock".into());
        register_aumid(&identifier, &name, toast_icon_path(app).as_deref());
        set_process_aumid(&identifier);
    }

    pub fn show(app: &AppHandle, toast: &ToastSpec) -> Result<(), String> {
        let identifier = app.config().identifier.clone();
        let spec = toast.clone();
        show_with_fallback(&identifier, |app_id| show_with_app_id(app_id, &spec))
    }

    fn show_with_app_id(app_id: &str, toast: &ToastSpec) -> Result<(), String> {
        tauri_winrt_notification::Toast::new(app_id)
            .title(&toast.title)
            .text1(&toast.body)
            .show()
            .map_err(|error| error.to_string())
    }

    fn toast_icon_path(app: &AppHandle) -> Option<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(dir) = app.path().resource_dir() {
            candidates.push(dir.join("icons").join("icon.png"));
            candidates.push(dir.join("icon.png"));
            candidates.push(dir.join("icon.ico"));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(parent.join("icon.ico"));
                candidates.push(parent.join("icons").join("icon.png"));
            }
        }
        candidates.into_iter().find(|path| path.is_file())
    }

    fn register_aumid(identifier: &str, name: &str, icon: Option<&Path>) {
        let path = format!(r"SOFTWARE\Classes\AppUserModelId\{identifier}");
        match windows_registry::CURRENT_USER.create(&path) {
            Ok(key) => {
                if let Err(error) = key.set_string("DisplayName", name) {
                    eprintln!("Agent Activity Dock: cannot set toast DisplayName: {error}");
                }
                if let Some(icon) = icon {
                    if let Err(error) = key.set_string("IconUri", &icon.to_string_lossy()) {
                        eprintln!("Agent Activity Dock: cannot set toast IconUri: {error}");
                    }
                }
            }
            Err(error) => {
                eprintln!("Agent Activity Dock: cannot register toast AppUserModelId: {error}");
            }
        }
    }

    fn set_process_aumid(identifier: &str) {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
        let wide: Vec<u16> = identifier.encode_utf16().chain(std::iter::once(0)).collect();
        if let Err(error) =
            unsafe { SetCurrentProcessExplicitAppUserModelID(PCWSTR(wide.as_ptr())) }
        {
            eprintln!("Agent Activity Dock: cannot set process AppUserModelId: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{show_with_fallback, POWERSHELL_TOAST_APP_ID};
    use std::cell::RefCell;

    #[test]
    fn uses_app_identifier_when_it_shows() {
        let seen = RefCell::new(Vec::new());
        show_with_fallback("dev.agentactivitydock", |id| {
            seen.borrow_mut().push(id.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(*seen.borrow(), ["dev.agentactivitydock"]);
    }

    #[test]
    fn falls_back_to_powershell_identity() {
        let seen = RefCell::new(Vec::new());
        show_with_fallback("dev.agentactivitydock", |id| {
            seen.borrow_mut().push(id.to_owned());
            if id == POWERSHELL_TOAST_APP_ID {
                Ok(())
            } else {
                Err("no aumid".to_owned())
            }
        })
        .unwrap();
        assert_eq!(
            *seen.borrow(),
            [
                "dev.agentactivitydock".to_owned(),
                POWERSHELL_TOAST_APP_ID.to_owned()
            ]
        );
    }

    #[test]
    fn surfaces_both_errors_when_nothing_shows() {
        let error = show_with_fallback("dev.agentactivitydock", |id| Err(format!("fail {id}")))
            .unwrap_err();
        assert!(error.contains("fail dev.agentactivitydock"));
        assert!(error.contains(POWERSHELL_TOAST_APP_ID));
    }
}
