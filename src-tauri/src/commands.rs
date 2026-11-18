//! Commands of the interface, mirrored by `src/api/index.ts`. They all run on
//! the worker pool: some wait for a dialog or open a window, which the main
//! thread must stay free for.

use std::sync::Arc;

use chrono::Local;
use tauri::State;

use serde_json::json;

use crate::agent::{self, AgentEntry};
use crate::external::{Dialog, McpSnippet};
use crate::app::Core;
use crate::harness;
use crate::engine::Source;
use crate::error::{AppError, AppResult};
use crate::model::queue::{Queue, QUEUE_VERSION};
use crate::model::scenario::{new_id, Scenario, WindowTarget};
use crate::model::schedule::{RunTarget, ScheduleRule};
use crate::model::settings::Settings;
use crate::model::validate::{validate_queue, validate_scenario, Issue};
use crate::runtime::{AppInfo, JournalEntry, RegionPurpose, RunStatus, UpdateStatus, WindowInfo};
use crate::screen;
use crate::storage::{QueueSummary, ScenarioSummary};
use crate::target;
use crate::update;

type CoreState<'a> = State<'a, Arc<Core>>;

fn checked_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        Err(AppError::new("nameEmpty"))
    } else {
        Ok(name)
    }
}

// App and settings

#[tauri::command(async)]
pub fn app_info(core: CoreState<'_>) -> AppInfo {
    core.info()
}

#[tauri::command(async)]
pub fn app_quit(core: CoreState<'_>) {
    core.inner().quit();
}

#[tauri::command(async)]
pub fn settings_get(core: CoreState<'_>) -> Settings {
    core.settings()
}

#[tauri::command(async)]
pub fn settings_set(core: CoreState<'_>, settings: Settings) -> AppResult<Settings> {
    core.set_settings(settings)
}

#[tauri::command(async)]
pub fn autostart_get(core: CoreState<'_>) -> AppResult<bool> {
    core.autostart_enabled()
}

#[tauri::command(async)]
pub fn autostart_set(core: CoreState<'_>, enabled: bool) -> AppResult<()> {
    core.set_autostart(enabled)
}

// Updates

#[tauri::command(async)]
pub fn update_status(core: CoreState<'_>) -> UpdateStatus {
    core.updates.status()
}

/// Starts a check in the background; the progress arrives as `update:status` events.
#[tauri::command(async)]
pub fn update_check(core: CoreState<'_>) {
    update::check_now(core.inner());
}

#[tauri::command(async)]
pub fn harness_reply(core: CoreState<'_>, request_id: u64, accepted: bool) {
    harness::reply(core.inner(), request_id, accepted);
}

#[tauri::command(async)]
pub fn window_show(core: CoreState<'_>) {
    crate::shell::window::show_main(core.app());
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub key_set: bool,
}

#[tauri::command(async)]
pub fn agent_info(core: CoreState<'_>) -> AgentInfo {
    AgentInfo { key_set: !core.store.agent_key().is_empty() }
}

#[tauri::command(async)]
pub fn agent_key_set(core: CoreState<'_>, key: String) -> AppResult<()> {
    core.store.set_agent_key(&key)
}

#[tauri::command(async)]
pub fn agent_history(core: CoreState<'_>) -> Vec<AgentEntry> {
    agent::history(core.inner())
}

#[tauri::command(async)]
pub fn agent_send(core: CoreState<'_>, text: String) -> AppResult<()> {
    agent::send(core.inner(), text)
}

#[tauri::command(async)]
pub fn harness_dialogs(core: CoreState<'_>) -> Vec<Dialog> {
    crate::external::list(core.inner())
}

/// Ready MCP text for this copy of ClickIt. The path points at `clickit-mcp.exe` beside it.
#[tauri::command(async)]
pub fn harness_mcp_snippet() -> McpSnippet {
    crate::external::running_mcp_snippet()
}

// Scenarios

#[tauri::command(async)]
pub fn scenario_list(core: CoreState<'_>) -> Vec<ScenarioSummary> {
    core.store.list_scenarios()
}

#[tauri::command(async)]
pub fn scenario_get(core: CoreState<'_>, id: String) -> AppResult<Scenario> {
    core.store.load_scenario(&id)
}

#[tauri::command(async)]
pub fn scenario_create(core: CoreState<'_>, name: String) -> AppResult<Scenario> {
    let scenario = Scenario::new(checked_name(&name)?);
    core.store.save_scenario(&scenario)?;
    core.refresh_tray();
    Ok(scenario)
}

#[tauri::command(async)]
pub fn scenario_save(core: CoreState<'_>, scenario: Scenario) -> AppResult<Scenario> {
    let value = harness::save_scenario(core.inner(), &json!({"scenario": scenario}), false)?;
    let saved = value.get("scenario").cloned().ok_or_else(|| AppError::new("invalid"))?;
    Ok(serde_json::from_value(saved)?)
}

#[tauri::command(async)]
pub fn scenario_duplicate(core: CoreState<'_>, id: String, name: String) -> AppResult<Scenario> {
    let copy = core.store.duplicate_scenario(&id, checked_name(&name)?)?;
    core.refresh_tray();
    Ok(copy)
}

#[tauri::command(async)]
pub fn scenario_delete(core: CoreState<'_>, id: String) -> AppResult<()> {
    if core.scenario_in_use(&id) {
        return Err(AppError::new("busy"));
    }
    core.store.delete_scenario(&id)?;
    core.refresh_tray();
    Ok(())
}

#[tauri::command(async)]
pub fn scenario_validate(core: CoreState<'_>, scenario: Scenario) -> Vec<Issue> {
    validate_scenario(&scenario, &|file| core.store.template_exists(&scenario.id, file))
}

#[tauri::command(async)]
pub fn scenario_export(core: CoreState<'_>, id: String) -> AppResult<Option<String>> {
    core.export_scenario(&id)
}

#[tauri::command(async)]
pub fn scenario_import(core: CoreState<'_>) -> AppResult<Option<Scenario>> {
    core.import_scenario()
}

#[tauri::command(async)]
pub fn template_preview(core: CoreState<'_>, scenario_id: String, file: String) -> AppResult<String> {
    screen::png_data_url(&core.store.template_path(&scenario_id, &file)?)
}

// Queues

#[tauri::command(async)]
pub fn queue_list(core: CoreState<'_>) -> Vec<QueueSummary> {
    core.store.list_queues()
}

#[tauri::command(async)]
pub fn queue_get(core: CoreState<'_>, id: String) -> AppResult<Queue> {
    core.store.load_queue(&id)
}

#[tauri::command(async)]
pub fn queue_create(core: CoreState<'_>, name: String) -> AppResult<Queue> {
    let queue = Queue::new(checked_name(&name)?);
    core.store.save_queue(&queue)?;
    core.refresh_tray();
    Ok(queue)
}

#[tauri::command(async)]
pub fn queue_save(core: CoreState<'_>, mut queue: Queue) -> AppResult<()> {
    if core.queue_in_use(&queue.id) {
        return Err(AppError::new("busy"));
    }
    // A late save must not bring a deleted queue back.
    core.store.load_queue(&queue.id)?;
    queue.version = QUEUE_VERSION;
    core.store.save_queue(&queue)?;
    // The name and whether the queue can run both show in the tray.
    core.refresh_tray();
    Ok(())
}

#[tauri::command(async)]
pub fn queue_duplicate(core: CoreState<'_>, id: String, name: String) -> AppResult<Queue> {
    let mut copy = core.store.load_queue(&id)?;
    copy.id = new_id("q");
    copy.name = checked_name(&name)?.to_string();
    for item in &mut copy.items {
        item.id = new_id("qi");
    }
    core.store.save_queue(&copy)?;
    core.refresh_tray();
    Ok(copy)
}

#[tauri::command(async)]
pub fn queue_delete(core: CoreState<'_>, id: String) -> AppResult<()> {
    if core.queue_in_use(&id) {
        return Err(AppError::new("busy"));
    }
    core.store.delete_queue(&id)?;
    core.refresh_tray();
    Ok(())
}

#[tauri::command(async)]
pub fn queue_validate(core: CoreState<'_>, queue: Queue) -> Vec<Issue> {
    validate_queue(&queue, &|id| core.store.scenario_exists(id))
}

// Schedule

#[tauri::command(async)]
pub fn schedule_get(core: CoreState<'_>) -> Vec<ScheduleRule> {
    core.scheduler.rules()
}

#[tauri::command(async)]
pub fn schedule_set(core: CoreState<'_>, rules: Vec<ScheduleRule>) -> AppResult<()> {
    core.scheduler.set_rules(rules, Local::now().naive_local())
}

// Run and recording

#[tauri::command(async)]
pub fn run_start(core: CoreState<'_>, target: RunTarget) -> AppResult<()> {
    core.start(&target, Source::Manual)
}

#[tauri::command(async)]
pub fn run_stop(core: CoreState<'_>) {
    core.stop();
}

#[tauri::command(async)]
pub fn run_status(core: CoreState<'_>) -> RunStatus {
    core.hub.get()
}

#[tauri::command(async)]
pub fn record_start(core: CoreState<'_>, scenario_id: String) -> AppResult<()> {
    core.record_start(&scenario_id)
}

#[tauri::command(async)]
pub fn record_stop(core: CoreState<'_>) {
    core.stop();
}

// Capture of a point and selection of a region

#[tauri::command(async)]
pub fn capture_start(core: CoreState<'_>, target: Option<WindowTarget>) -> AppResult<()> {
    core.capture_start(target)
}

#[tauri::command(async)]
pub fn capture_cancel(core: CoreState<'_>) {
    core.capture_cancel();
}

#[tauri::command(async)]
pub fn region_start(
    core: CoreState<'_>,
    scenario_id: String,
    purpose: RegionPurpose,
    target: Option<WindowTarget>,
) -> AppResult<()> {
    core.region_start(&scenario_id, purpose, target)
}

#[tauri::command(async)]
pub fn region_mark(core: CoreState<'_>) {
    core.region_mark();
}

#[tauri::command(async)]
pub fn region_finish(core: CoreState<'_>) {
    core.region_finish();
}

#[tauri::command(async)]
pub fn region_cancel(core: CoreState<'_>) {
    core.region_cancel();
}

// Journal and windows

#[tauri::command(async)]
pub fn journal_get(core: CoreState<'_>) -> Vec<JournalEntry> {
    core.journal.read()
}

/// Visible windows for picking a target, without repeats of the same title and process.
#[tauri::command(async)]
pub fn windows_list() -> Vec<WindowInfo> {
    let mut list: Vec<WindowInfo> = Vec::new();
    for found in target::top_windows() {
        let info = WindowInfo { title: found.title, process: found.process };
        if !list.contains(&info) {
            list.push(info);
        }
    }
    list
}
