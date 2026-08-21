use agent_activity_dock_core::{NotificationSink, ToastSpec};
use tauri::AppHandle;

pub struct PresenterToastSink {
    pub app: AppHandle,
}

impl NotificationSink for PresenterToastSink {
    fn show(&self, toast: &ToastSpec) -> Result<(), String> {
        #[cfg(windows)]
        {
            use tauri_plugin_notification::NotificationExt;
            self.app
                .notification()
                .builder()
                .title(&toast.title)
                .body(&toast.body)
                .extra("source", &toast.source)
                .extra("session_id", &toast.session_id)
                .show()
                .map_err(|error| error.to_string())
        }
        #[cfg(not(windows))]
        {
            let _ = (self, toast);
            Ok(())
        }
    }
}
