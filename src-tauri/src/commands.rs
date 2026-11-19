//! Commands of the interface, mirrored by `src/api/index.ts`. They all run on
//! the worker pool: some wait for a dialog or open a window, which the main
//! thread must stay free for.

use std::sync::Arc;

use chrono::Local;
use tauri::State;

use serde_json::json;

use crate::agent::{self, AgentEntry, ChatInfo};
use crate::external::{Dialog, McpSnippet};
use crate::app::{CleanupInfo, Core};
use crate::harness;
use crate::engine::conditions::CheckResult;
use crate::engine::{Debug, Resume, Source};
use crate::error::{AppError, AppResult};
use crate::model::queue::{Queue, QUEUE_VERSION};
use crate::model::graph::{Edge, Node};
use crate::model::scenario::{new_id, Condition, Scenario, ScenarioMode, WindowTarget};
use crate::model::schedule::{RunTarget, ScheduleRule};
use crate::model::settings::Settings;
use crate::model::validate::{validate_queue, Issue};
use crate::runtime::{AppInfo, JournalEntry, RegionPurpose, RunStatus, RunTrace, UpdateStatus, WindowInfo};
use crate::screen;
use crate::storage::{CleanupKind, QueueSummary, ScenarioSummary, Snippet};
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

/// Chats with the model, oldest first. There is always at least one.
#[tauri::command(async)]
pub fn agent_chats(core: CoreState<'_>) -> Vec<ChatInfo> {
    agent::chats(core.inner())
}

#[tauri::command(async)]
pub fn agent_chat_create(core: CoreState<'_>) -> ChatInfo {
    agent::chat_create(core.inner())
}

/// Returns the chats that are left. The chat the model is answering in is refused.
#[tauri::command(async)]
pub fn agent_chat_delete(core: CoreState<'_>, id: String) -> AppResult<Vec<ChatInfo>> {
    agent::chat_delete(core.inner(), &id)
}

#[tauri::command(async)]
pub fn agent_chat_rename(core: CoreState<'_>, id: String, title: String) -> AppResult<ChatInfo> {
    agent::chat_rename(core.inner(), &id, &title)
}

#[tauri::command(async)]
pub fn agent_history(core: CoreState<'_>, chat_id: String) -> Vec<AgentEntry> {
    agent::history(core.inner(), &chat_id)
}

#[tauri::command(async)]
pub fn agent_send(core: CoreState<'_>, chat_id: String, text: String) -> AppResult<()> {
    agent::send(core.inner(), chat_id, text)
}

#[tauri::command(async)]
pub fn harness_dialogs(core: CoreState<'_>) -> Vec<Dialog> {
    crate::external::list(core.inner())
}

/// Removes one closed harness connection from the Assistant page.
#[tauri::command(async)]
pub fn harness_dialog_forget(core: CoreState<'_>, id: u64) -> AppResult<()> {
    crate::external::forget(core.inner(), id)
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

/// `format` is `"list"` or `"canvas"`; a canvas scenario starts with its start block.
#[tauri::command(async)]
pub fn scenario_create(core: CoreState<'_>, name: String, format: Option<String>) -> AppResult<Scenario> {
    let name = checked_name(&name)?;
    let scenario = match format.as_deref() {
        Some("canvas") => Scenario::new_canvas(name),
        Some("list") | None => Scenario::new(name),
        Some(_) => return Err(AppError::new("invalid")),
    };
    core.store.save_scenario(&scenario)?;
    core.refresh_tray();
    Ok(scenario)
}

/// What the editor and the list of scenarios show besides the scenario itself.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioMeta {
    /// The list this graph was made from is still there, so it can be returned.
    pub has_backup: bool,
    /// Scenarios whose "Run scenario" blocks run this one.
    pub callers: Vec<ScenarioSummary>,
}

#[tauri::command(async)]
pub fn scenario_meta(core: CoreState<'_>, id: String) -> ScenarioMeta {
    ScenarioMeta { has_backup: core.store.has_backup(&id), callers: core.store.callers(&id) }
}

/// Turns a list into a graph. With `copy_name` the list stays and a new scenario gets the graph.
#[tauri::command(async)]
pub fn scenario_to_canvas(core: CoreState<'_>, id: String, copy_name: Option<String>) -> AppResult<Scenario> {
    let copy_name = copy_name.as_deref().map(checked_name).transpose()?;
    core.scenario_to_canvas(&id, copy_name)
}

/// Brings back the list a graph was made from; what was changed on the canvas is lost.
#[tauri::command(async)]
pub fn scenario_to_list(core: CoreState<'_>, id: String) -> AppResult<Scenario> {
    core.scenario_to_list(&id)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extracted {
    /// The scenario the blocks were taken from, with a "Run scenario" block in their place.
    pub scenario: Scenario,
    pub created: Scenario,
}

/// Moves blocks of a saved graph into a new scenario.
#[tauri::command(async)]
pub fn scenario_extract(
    core: CoreState<'_>,
    id: String,
    node_ids: Vec<String>,
    name: String,
) -> AppResult<Extracted> {
    let (scenario, created) = core.scenario_extract(&id, &node_ids, checked_name(&name)?)?;
    Ok(Extracted { scenario, created })
}

/// "Check now": one look at a condition of a scenario, saved or not. Nothing is clicked.
#[tauri::command(async)]
pub fn condition_check(
    core: CoreState<'_>,
    scenario_id: String,
    mode: ScenarioMode,
    target: WindowTarget,
    condition: Condition,
) -> AppResult<CheckResult> {
    core.check_condition(&scenario_id, mode, &target, &condition)
}

// Snippets

#[tauri::command(async)]
pub fn snippet_list(core: CoreState<'_>) -> Vec<Snippet> {
    core.store.list_snippets()
}

/// Saves blocks with their links and the template images of `scenario_id` they refer to.
#[tauri::command(async)]
pub fn snippet_save(
    core: CoreState<'_>,
    scenario_id: String,
    name: String,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
) -> AppResult<Snippet> {
    core.store.save_snippet(&scenario_id, checked_name(&name)?, nodes, edges)
}

/// Copies the template images of a snippet into the scenario and returns its blocks.
#[tauri::command(async)]
pub fn snippet_use(core: CoreState<'_>, id: String, scenario_id: String) -> AppResult<Snippet> {
    core.store.use_snippet(&id, &scenario_id)
}

#[tauri::command(async)]
pub fn snippet_delete(core: CoreState<'_>, id: String) -> AppResult<()> {
    core.store.delete_snippet(&id)
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
    core.store.issues(&scenario)
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

/// Breakpoints of the open editor. They are not stored anywhere else.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugRequest {
    #[serde(default)]
    pub breakpoints: Vec<String>,
    /// Stop on every block, starting with the first one.
    #[serde(default)]
    pub step: bool,
}

/// `start_node` makes a trial run of a graph: one pass from that block.
#[tauri::command(async)]
pub fn run_start(
    core: CoreState<'_>,
    target: RunTarget,
    start_node: Option<String>,
    debug: Option<DebugRequest>,
) -> AppResult<()> {
    let debug = debug.map(|debug| Debug { breakpoints: debug.breakpoints.into_iter().collect(), step: debug.step });
    core.start_from(&target, Source::Manual, start_node, debug)
}

/// Runs the block a run is paused on and stops on the next one.
#[tauri::command(async)]
pub fn run_step(core: CoreState<'_>) {
    core.resume(Resume::Step);
}

/// Lets a paused run go on until the next breakpoint.
#[tauri::command(async)]
pub fn run_continue(core: CoreState<'_>) {
    core.resume(Resume::Continue);
}

/// The trace of the last run of a graph scenario, finished or still going.
#[tauri::command(async)]
pub fn run_trace(core: CoreState<'_>, id: String) -> Option<RunTrace> {
    core.engine.trace(&id)
}

#[tauri::command(async)]
pub fn run_trace_clear(core: CoreState<'_>, id: String) {
    if !core.scenario_in_use(&id) {
        core.engine.clear_trace(&id);
    }
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

// Cleanup

/// What every "Clear" button of the settings would remove right now.
#[tauri::command(async)]
pub fn cleanup_info(core: CoreState<'_>) -> Vec<CleanupInfo> {
    core.cleanup_info()
}

/// Removes one kind of data and returns the sizes that are left.
#[tauri::command(async)]
pub fn cleanup_run(core: CoreState<'_>, kind: CleanupKind) -> AppResult<Vec<CleanupInfo>> {
    core.cleanup_run(kind)
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
