//! ClickIt: a desktop auto clicker for Windows. The web view draws the
//! interface, everything else lives here.

pub mod agent;
pub mod app;
pub mod commands;
pub mod engine;
pub mod external;
pub mod error;
pub mod harness;
pub mod input;
pub mod journal;
pub mod model;
pub mod recorder;
pub mod runtime;
pub mod scheduler;
pub mod screen;
pub mod shell;
pub mod storage;
pub mod target;
pub mod transfer;
pub mod update;

use tauri::RunEvent;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    let built = tauri::Builder::default()
        // Has to be the first plugin: a second copy hands over to the running one and exits.
        .plugin(tauri_plugin_single_instance::init(|handle, args, _cwd| app::second_instance(handle, &args)))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|handle, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        app::hotkey_pressed(handle, shortcut);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![app::AUTOSTART_FLAG])))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        // Only the core talks to the updater: the web view gets no permission for it.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|setup| app::setup(setup.handle()))
        .on_window_event(app::window_event)
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::app_quit,
            commands::settings_get,
            commands::settings_set,
            commands::autostart_get,
            commands::autostart_set,
            commands::update_status,
            commands::update_check,
            commands::harness_reply,
            commands::window_show,
            commands::agent_info,
            commands::agent_key_set,
            commands::agent_chats,
            commands::agent_chat_create,
            commands::agent_chat_delete,
            commands::agent_chat_rename,
            commands::agent_history,
            commands::agent_send,
            commands::harness_dialogs,
            commands::harness_dialog_forget,
            commands::harness_mcp_snippet,
            commands::scenario_list,
            commands::scenario_get,
            commands::scenario_create,
            commands::scenario_save,
            commands::scenario_duplicate,
            commands::scenario_delete,
            commands::scenario_validate,
            commands::scenario_meta,
            commands::scenario_to_canvas,
            commands::scenario_to_list,
            commands::scenario_extract,
            commands::scenario_move,
            commands::folder_list,
            commands::folder_create,
            commands::folder_rename,
            commands::folder_delete,
            commands::folder_set_collapsed,
            commands::folder_move,
            commands::condition_check,
            commands::snippet_list,
            commands::snippet_save,
            commands::snippet_use,
            commands::snippet_delete,
            commands::scenario_export,
            commands::scenario_import,
            commands::command_pick_file,
            commands::command_pick_folder,
            commands::template_preview,
            commands::queue_list,
            commands::queue_get,
            commands::queue_create,
            commands::queue_save,
            commands::queue_duplicate,
            commands::queue_delete,
            commands::queue_validate,
            commands::schedule_get,
            commands::schedule_set,
            commands::run_start,
            commands::run_stop,
            commands::run_status,
            commands::run_step,
            commands::run_continue,
            commands::run_trace,
            commands::run_trace_clear,
            commands::cleanup_info,
            commands::cleanup_run,
            commands::record_start,
            commands::record_stop,
            commands::capture_start,
            commands::capture_cancel,
            commands::region_start,
            commands::region_mark,
            commands::region_finish,
            commands::region_cancel,
            commands::journal_get,
            commands::windows_list,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start ClickIt");

    built.run(|_handle, event| {
        // ClickIt lives in the tray: it ends only through its own "Quit", which passes an exit code.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
