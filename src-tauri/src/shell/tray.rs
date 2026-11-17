//! The tray icon: tooltip with the current state and the menu described in the
//! task (open, start or stop, stop recording, scenarios, queues, quit).

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use super::text::text;
use super::window;

const TRAY_ID: &str = "main";
/// Longer lists are cut: a tray menu is not a place to scroll.
const MAX_LIST_ITEMS: usize = 30;

pub const OPEN: &str = "open";
pub const TOGGLE: &str = "toggle";
pub const STOP_RECORDING: &str = "stop-recording";
pub const QUIT: &str = "quit";
pub const SCENARIO_PREFIX: &str = "sc:";
pub const QUEUE_PREFIX: &str = "q:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    Running,
    Recording,
}

/// Everything the menu shows, gathered before the main thread builds it.
#[derive(Debug, Clone, Default)]
pub struct TrayModel {
    pub lang: String,
    pub activity: Activity,
    /// Name of what "Start" runs; `None` when nothing is chosen.
    pub current: Option<String>,
    /// `(id, name)` pairs.
    pub scenarios: Vec<(String, String)>,
    pub queues: Vec<(String, String)>,
}

/// Creates the icon. `on_menu` gets the id of the chosen item.
pub fn create(app: &AppHandle, on_menu: impl Fn(&AppHandle, &str) + Send + Sync + 'static) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("ClickIt")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                window::show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn list(app: &AppHandle, model: &TrayModel, title_key: &str, prefix: &str, items: &[(String, String)]) -> tauri::Result<Submenu<Wry>> {
    let menu = Submenu::new(app, text(&model.lang, title_key, &[]), true)?;
    if items.is_empty() {
        menu.append(&MenuItem::new(app, text(&model.lang, "native.tray.empty", &[]), false, None::<&str>)?)?;
    }
    // Nothing else may start while a run or a recording is on.
    let enabled = model.activity == Activity::Idle;
    for (id, name) in items.iter().take(MAX_LIST_ITEMS) {
        // An ampersand would otherwise turn the next letter into a menu shortcut.
        let label = name.replace('&', "&&");
        menu.append(&MenuItem::with_id(app, format!("{prefix}{id}"), label, enabled, None::<&str>)?)?;
    }
    Ok(menu)
}

fn build_menu(app: &AppHandle, model: &TrayModel) -> tauri::Result<Menu<Wry>> {
    let lang = model.lang.as_str();
    let item = |id: &str, label: String, enabled: bool| MenuItem::with_id(app, id, label, enabled, None::<&str>);
    let menu = Menu::new(app)?;
    menu.append(&item(OPEN, text(lang, "native.tray.open", &[]), true)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    match model.activity {
        Activity::Idle => match &model.current {
            Some(name) => {
                let label = text(lang, "native.tray.start", &[("name", &name.replace('&', "&&"))]);
                menu.append(&item(TOGGLE, label, true)?)?;
            }
            None => menu.append(&item(TOGGLE, text(lang, "native.tray.startNone", &[]), false)?)?,
        },
        Activity::Running => menu.append(&item(TOGGLE, text(lang, "native.tray.stop", &[]), true)?)?,
        Activity::Recording => {
            menu.append(&item(STOP_RECORDING, text(lang, "native.tray.stopRecording", &[]), true)?)?;
        }
    }
    menu.append(&list(app, model, "native.tray.scenarios", SCENARIO_PREFIX, &model.scenarios)?)?;
    menu.append(&list(app, model, "native.tray.queues", QUEUE_PREFIX, &model.queues)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item(QUIT, text(lang, "native.tray.quit", &[]), true)?)?;
    Ok(menu)
}

/// Rebuilds the menu. Safe to call from any thread: the work is posted to the main one.
pub fn set_menu(app: &AppHandle, model: TrayModel) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let (Some(tray), Ok(menu)) = (handle.tray_by_id(TRAY_ID), build_menu(&handle, &model)) {
            let _ = tray.set_menu(Some(menu));
        }
    });
}

pub fn set_tooltip(app: &AppHandle, tooltip: String) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(tooltip));
        }
    });
}
