//! The main window and the two helper windows: the recording badge and the
//! region overlay. Helper windows are created when needed and destroyed after.
//!
//! Creating a window waits for the main thread, so `open_*` must not be called
//! from it: commands that open windows run on the worker pool.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::runtime::Rect;

pub const MAIN: &str = "main";
pub const BADGE: &str = "badge";
pub const REGION: &str = "region";

const BADGE_WIDTH: f64 = 340.0;
const BADGE_HEIGHT: f64 = 60.0;
const BADGE_MARGIN: f64 = 24.0;

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

pub fn main_visible(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN).is_some_and(|window| window.is_visible().unwrap_or(false))
}

fn destroy(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
}

/// Shows the recording badge in the top-right corner of the primary monitor,
/// away from the corner that stops a recording.
pub fn open_badge(app: &AppHandle) -> tauri::Result<()> {
    destroy(app, BADGE);
    let (x, y) = match app.primary_monitor()? {
        Some(monitor) => {
            let scale = monitor.scale_factor();
            let left = f64::from(monitor.position().x) / scale;
            let top = f64::from(monitor.position().y) / scale;
            let width = f64::from(monitor.size().width) / scale;
            (left + width - BADGE_WIDTH - BADGE_MARGIN, top + BADGE_MARGIN)
        }
        None => (BADGE_MARGIN, BADGE_MARGIN),
    };
    WebviewWindowBuilder::new(app, BADGE, WebviewUrl::App("index.html#badge".into()))
        .title("ClickIt")
        .inner_size(BADGE_WIDTH, BADGE_HEIGHT)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .build()?;
    Ok(())
}

pub fn close_badge(app: &AppHandle) {
    destroy(app, BADGE);
}

/// Covers the whole virtual screen with the transparent overlay for selecting a rectangle.
pub fn open_region(app: &AppHandle, screen: Rect) -> tauri::Result<()> {
    destroy(app, REGION);
    let window = WebviewWindowBuilder::new(app, REGION, WebviewUrl::App("index.html#region".into()))
        .title("ClickIt")
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    window.set_position(PhysicalPosition::new(screen.x, screen.y))?;
    window.set_size(PhysicalSize::new(screen.width.max(1) as u32, screen.height.max(1) as u32))?;
    window.show()?;
    // Esc cancels the selection, so the overlay needs the keyboard.
    window.set_focus()?;
    Ok(())
}

pub fn close_region(app: &AppHandle) {
    destroy(app, REGION);
}
