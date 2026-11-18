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
pub const HUD: &str = "hud";

const BADGE_WIDTH: f64 = 340.0;
const BADGE_HEIGHT: f64 = 60.0;
const BADGE_MARGIN: f64 = 24.0;

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    crate::app::with_core(app, |core| core.sync_hud());
}

pub fn hide_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

pub fn main_visible(app: &AppHandle) -> bool {
    app.get_webview_window(MAIN).is_some_and(|window| window.is_visible().unwrap_or(false))
}

/// The main window is out of the way: hidden into the tray, or minimized.
/// A minimized window still reports itself as visible, so that flag alone is not enough.
pub fn main_hidden(app: &AppHandle) -> bool {
    match app.get_webview_window(MAIN) {
        Some(window) => {
            let visible = window.is_visible().unwrap_or(false);
            let minimized = window.is_minimized().unwrap_or(false);
            !visible || minimized
        }
        None => true,
    }
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

const HUD_WIDTH: f64 = 340.0;
const HUD_HEIGHT: f64 = 64.0;
const HUD_GAP: f64 = 8.0;

pub fn hud_open(app: &AppHandle) -> bool {
    app.get_webview_window(HUD).is_some()
}

/// Small always-on-top window just inside the screen from the notification area.
pub fn open_hud(app: &AppHandle) -> tauri::Result<()> {
    destroy(app, HUD);
    let (x, y) = hud_origin(app);
    WebviewWindowBuilder::new(app, HUD, WebviewUrl::App("index.html#hud".into()))
        .title("ClickIt")
        .inner_size(HUD_WIDTH, HUD_HEIGHT)
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

pub fn close_hud(app: &AppHandle) {
    destroy(app, HUD);
}

fn hud_origin(app: &AppHandle) -> (f64, f64) {
    let scale = app.primary_monitor().ok().flatten().map(|monitor| monitor.scale_factor()).unwrap_or(1.0);
    let fallback = || {
        let monitor = app.primary_monitor().ok().flatten();
        let (width, height, left, top) = monitor
            .map(|monitor| {
                let scale = monitor.scale_factor();
                (
                    f64::from(monitor.size().width) / scale,
                    f64::from(monitor.size().height) / scale,
                    f64::from(monitor.position().x) / scale,
                    f64::from(monitor.position().y) / scale,
                )
            })
            .unwrap_or((1280.0, 720.0, 0.0, 0.0));
        (left + width - HUD_WIDTH - HUD_GAP, top + height - HUD_HEIGHT - 48.0)
    };
    let Some((edge, left, top, right, bottom)) = taskbar_rect() else {
        return fallback();
    };
    let dip = |value: i32| f64::from(value) / scale;
    match edge {
        TaskbarEdge::Bottom => (dip(right) - HUD_WIDTH - HUD_GAP, dip(top) - HUD_HEIGHT - HUD_GAP),
        TaskbarEdge::Top => (dip(right) - HUD_WIDTH - HUD_GAP, dip(bottom) + HUD_GAP),
        TaskbarEdge::Left => (dip(right) + HUD_GAP, dip(bottom) - HUD_HEIGHT - HUD_GAP),
        TaskbarEdge::Right => (dip(left) - HUD_WIDTH - HUD_GAP, dip(bottom) - HUD_HEIGHT - HUD_GAP),
    }
}

enum TaskbarEdge {
    Left,
    Top,
    Right,
    Bottom,
}

/// Physical-pixel rectangle of the taskbar, from `SHAppBarMessage`.
fn taskbar_rect() -> Option<(TaskbarEdge, i32, i32, i32, i32)> {
    use std::mem::size_of;
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::Shell::{SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_GETTASKBARPOS, APPBARDATA};
    let mut data = APPBARDATA { cbSize: size_of::<APPBARDATA>() as u32, rc: RECT::default(), ..Default::default() };
    if unsafe { SHAppBarMessage(ABM_GETTASKBARPOS, &mut data) } == 0 {
        return None;
    }
    let edge = match data.uEdge {
        ABE_LEFT => TaskbarEdge::Left,
        ABE_TOP => TaskbarEdge::Top,
        ABE_RIGHT => TaskbarEdge::Right,
        ABE_BOTTOM => TaskbarEdge::Bottom,
        _ => return None,
    };
    Some((edge, data.rc.left, data.rc.top, data.rc.right, data.rc.bottom))
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
