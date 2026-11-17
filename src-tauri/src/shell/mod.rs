//! What ClickIt shows outside its main window: the tray icon, notifications,
//! the recording badge, the region overlay, and the global hotkeys.

pub mod hotkeys;
pub mod text;
pub mod tray;
pub mod window;

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Shows a notification next to the tray icon. A failure is not worth stopping for.
pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}
