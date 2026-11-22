//! One set of actions for the local pipe (`clickit-mcp`) and for the assistant.
//! The pipe is open only while the harness setting is on. A scenario starts
//! only after a dialog, unless the auto-run setting says otherwise.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::Emitter;

use crate::app::Core;
use crate::engine::{build_plan, Source};
use crate::error::{AppError, AppResult};
use crate::model::scenario::{CommandSpec, Condition, LoopMode, Scenario, Step, StepKind, WindowTarget};
use crate::model::schedule::{RunTarget, TargetKind};
use crate::model::validate::{has_errors, Issue};
use crate::runtime::{JournalEntry, Rect};
use crate::screen;
use crate::shell::text::text;
use crate::shell::window;
use crate::target;

/// How many tool calls one assistant message may make.
pub const MAX_TOOL_CALLS: u32 = 25;
/// Longer side of a picture sent to a model, in pixels.
pub const MAX_PREVIEW_SIDE: u32 = 1600;
const CONFIRM_WAIT: Duration = Duration::from_secs(60);
const MIN_TEMPLATE_SIDE: i32 = 4;
const JOURNAL_DEFAULT: usize = 100;
const JOURNAL_MAX: usize = 500;
/// Local pipe. Only the current user's token is accepted on it.
pub const PIPE_NAME: &str = r"\\.\pipe\clickit-harness";

pub struct ConfirmWait {
    pub id: u64,
    pub reply: Sender<bool>,
}

#[derive(Debug)]
pub struct Request {
    pub id: u64,
    pub method: String,
    pub params: Value,
    /// Name the MCP client sent. Empty when the caller did not say who it is.
    pub client: String,
}

/// The revision a save must store, or `stale` when the caller has an old copy.
pub fn bumped_revision(incoming: u64, stored: u64) -> AppResult<u64> {
    if incoming != stored {
        Err(AppError::new("stale"))
    } else {
        Ok(stored.saturating_add(1))
    }
}

pub fn parse_request(line: &str) -> AppResult<Request> {
    let value: Value = serde_json::from_str(line)?;
    let id = value.get("id").and_then(Value::as_u64).ok_or_else(|| AppError::new("badFile"))?;
    let method = value.get("method").and_then(Value::as_str).unwrap_or("").to_string();
    if method.is_empty() {
        return Err(AppError::new("badFile"));
    }
    let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
    let client = value.get("client").and_then(Value::as_str).unwrap_or("").to_string();
    Ok(Request { id, method, params, client })
}

/// Tool list shared by the assistant and by `clickit-mcp`. Descriptions are English
/// so a model understands them in either interface language.
pub fn tool_definitions() -> Vec<Value> {
    vec![
        tool(
            "list_scenarios",
            "List saved scenarios: id, name, mode (screen or window), format (list or canvas) and the number of steps or blocks.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "get_scenario",
            "Read one scenario with its revision. format tells how it is stored: list has steps, canvas has graph with nodes, edges and notes.",
            json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        ),
        tool(
            "create_scenario",
            "Create an empty scenario. The core assigns the id. format is list (default) or canvas; a canvas scenario starts with one start block. Prefer canvas unless the user asks for a list.",
            json!({
                "type": "object",
                "properties": {"name": {"type": "string"}, "format": {"type": "string", "enum": ["list", "canvas"]}},
                "required": ["name"]
            }),
        ),
        tool(
            "save_scenario",
            concat!(
                "Replace a scenario. Send the object from get_scenario with your edits, including revision. ",
                "The format cannot be changed here: use convert_scenario. Empty ids are filled in. ",
                "Errors are returned and nothing is written; every issue names its step or block in stepId. ",
                "List: steps use type click, move, drag, scroll, key, text, pause, wait, if, loop, breakLoop, stop or command. ",
                "A command step is {type: command, command, onFailure: stop|continue}. command is {run, workDir, wait, timeoutMs, hidden}; ",
                "run is {kind: program, path, args} for a program or a script file with its arguments as one string, ",
                "or {kind: script, shell: cmd|powerShell, text} for script text kept in the step. ",
                "workDir may be empty. wait true waits for the end: exit code 0 is success, another code, a start that failed or timeoutMs running out (0 waits without a limit) is a failure, and the command is ended together with what it started. ",
                "wait false only starts the command and goes on. hidden true starts it without a console window and puts what it prints into the journal. ",
                "Commands run with the rights of the user and only when the user has switched them on in the settings. ",
                "Saving a scenario with a new command and starting a scenario with commands always wait for the user's answer in the ClickIt window, where the command text is shown; a save that was not allowed returns commandsDeclined. ",
                "Add a command only when the user asked for it. ",
                "Canvas: graph is {nodes, edges, notes}. A node is {id, type, title, enabled, pauseAfter, repeat, x, y} plus the fields of its type. ",
                "Types: start (exactly one, output next); click, move, drag, scroll, key, text, pause, stop with the fields of the list steps (output next); ",
                "wait {condition, pollMs, timeoutMs, onFound} (outputs next and timeout); ",
                "branch {condition, pollMs, timeoutMs} where timeoutMs 0 checks once (outputs yes and no); ",
                "counter {times, pauseBetween} (outputs body and done); ",
                "while {condition, until, pauseBetween} checks the condition once: body runs while it holds, or while it does not when until is true (outputs body and done; link the end of the body back); ",
                "focus {target: {title, titleMatch contains|exact, process}} brings that window to the front (output next) and fails the run when it is not open; ",
                "command {command} with the same command object as the list step (outputs next for success and error for a failure; an error output without an edge fails the run); ",
                "A condition is pixel, imageFound, imageGone, window or process, or {kind: all|any, items: [condition, condition]}: all means both, any means either, and the second is skipped once the first decides it. At most 4 levels. ",
                "call {scenarioId} runs one pass of another scenario in the same mode (output next); ",
                "intercept {condition, minIntervalMs >= 500} has no input (output next leads to its own branch). ",
                "An edge is {from, port, to}. One edge per output, any number of edges into a block. ",
                "An output without an edge ends the pass; a timeout or error output without an edge fails the run. ",
                "A loop is an edge back to an earlier block. A counter sends the pass into body `times` times and then into done: link the end of the body back to the counter. ",
                "An intercept is checked before every block and on every poll of a wait or a branch, at most once per minIntervalMs; ",
                "when it holds, its branch runs to an output without an edge and the interrupted block starts over. ",
                "The branch of an intercept must not lead to blocks reachable from start. At most 8 intercepts and 500 blocks. ",
                "Leave x and y out of new nodes: they are laid out automatically; keep x and y of the nodes you did not move. ",
                "Give blocks a short title. notes hold {id, kind, x, y, width, height, text, color} with kind note or frame; ",
                "a frame groups the blocks inside its rectangle. Instead of a rectangle a frame may carry around: [block ids] and is drawn around them. ",
                "Coordinates of clicks are absolute for screen mode and client for window mode. ",
                "Image conditions refer to a template file name from save_template."
            ),
            json!({
                "type": "object",
                "properties": {"scenario": {"type": "object"}},
                "required": ["scenario"]
            }),
        ),
        tool(
            "validate_scenario",
            "Check a scenario, a list or a canvas graph, without writing it. Returns the issues; stepId is the id of the step or block.",
            json!({"type": "object", "properties": {"scenario": {"type": "object"}}, "required": ["scenario"]}),
        ),
        tool(
            "convert_scenario",
            "Turn a list scenario into a canvas graph with an automatic layout. With copy false the scenario is converted in place and its list is kept as a backup the user can return to. With copy true a new scenario is created (name is optional) and the list stays. Returns the graph scenario.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string"}, "copy": {"type": "boolean"}, "name": {"type": "string"}},
                "required": ["id"]
            }),
        ),
        tool(
            "check_condition",
            "Look at a condition once, the way a run does, without clicking anything. Pass nodeId, the id of a wait, branch or intercept block (or of a wait or if step of a list), or a condition object to try before saving. Returns holds, and for an image: found, the centre x and y, score (similarity 0..1) and, when it is not found, wouldMatchAt, the similarity at which it would be. For a pixel: the colour that is there now. The main window hides for the look.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string"}, "nodeId": {"type": "string"}, "condition": {"type": "object"}},
                "required": ["id"]
            }),
        ),
        tool(
            "list_windows",
            "Titles and process names of visible windows, for the window mode target. No screenshots.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "get_journal",
            "Recent journal entries: time, code, params and text in the application language. limit defaults to 100 and cannot exceed 500.",
            json!({"type": "object", "properties": {"limit": {"type": "integer"}}}),
        ),
        tool(
            "run_status",
            "What a run or a recording is doing right now. For a canvas scenario block names the current block: id, number with title, and type.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "run_scenario",
            "Start a scenario by id. Does not wait until it finishes: a scenario may run until stopped. Returns outcome started or declined. Unless auto-run is enabled, ClickIt asks the user first; a scenario with command steps always asks and does not start while commands are switched off in the settings. For a canvas scenario startNode makes a trial run: one pass from that block, without the repeats of the scenario.",
            json!({
                "type": "object",
                "properties": {"id": {"type": "string"}, "startNode": {"type": "string"}},
                "required": ["id"]
            }),
        ),
        tool(
            "get_trace",
            "The trace of the last run of a canvas scenario, finished or still going: nodePasses (how many times each block was entered), edgePasses (keys are blockId:output), outcomes (the output each block was left through last), moves (the last transitions) and stop with the block, the outcome and the reason the run ended on. trace is null when the scenario has not run since ClickIt started.",
            json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        ),
        tool(
            "stop_scenario",
            "Stop the current run immediately, the same as the Stop button. No confirmation.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "capture_screen",
            "Capture the virtual screen or a rectangle. The main window hides for the shot. The PNG is scaled so its longer side is at most 1600 pixels. Multiply a pixel of the PNG by scale and add originX/originY to get a screen pixel. For space client, pass the target window. Omit width and height, or send 0, to capture that window's whole client area. x and y are inside the window, and the same rectangle is also returned in client coordinates.",
            json!({
                "type": "object",
                "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "width": {"type": "integer"},
                    "height": {"type": "integer"},
                    "space": {"type": "string", "enum": ["screen", "client"]},
                    "target": {"type": "object"}
                }
            }),
        ),
        tool(
            "pixel_color",
            "Read one pixel as #RRGGBB. x and y follow the same space rules as capture_screen.",
            json!({
                "type": "object",
                "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "space": {"type": "string", "enum": ["screen", "client"]},
                    "target": {"type": "object"}
                },
                "required": ["x", "y"]
            }),
        ),
        tool(
            "save_template",
            "Capture a rectangle at full resolution and store it as a PNG template of the scenario. Returns the file name to put in an imageFound or imageGone step. The rectangle must be at least 4 pixels on each side.",
            json!({
                "type": "object",
                "properties": {
                    "scenarioId": {"type": "string"},
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "width": {"type": "integer"},
                    "height": {"type": "integer"},
                    "space": {"type": "string", "enum": ["screen", "client"]},
                    "target": {"type": "object"}
                },
                "required": ["scenarioId", "x", "y", "width", "height"]
            }),
        ),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({"name": name, "description": description, "parameters": parameters})
}

/// Runs one tool. `run_scenario` may block until the user answers.
pub fn call(core: &Core, method: &str, params: &Value) -> AppResult<Value> {
    match method {
        "list_scenarios" => Ok(serde_json::to_value(core.store.list_scenarios()).unwrap_or(Value::Null)),
        "get_scenario" => scenario_view(&core.store.load_scenario(&required_str(params, "id")?)?),
        "create_scenario" => {
            let format = params.get("format").and_then(Value::as_str).unwrap_or("list");
            create_scenario(core, &required_str(params, "name")?, format)
        }
        "save_scenario" => save_scenario(core, params, true),
        "validate_scenario" => {
            let scenario = scenario_from(params)?;
            Ok(json!({"issues": issues_of(core, &scenario)}))
        }
        "convert_scenario" => convert_scenario(core, params),
        "check_condition" => check_condition(core, params),
        "get_trace" => {
            let id = required_str(params, "id")?;
            core.store.load_scenario(&id)?;
            Ok(json!({"trace": core.engine.trace(&id)}))
        }
        "list_windows" => {
            let windows: Vec<Value> = target::top_windows()
                .into_iter()
                .map(|window| json!({"title": window.title, "process": window.process}))
                .collect();
            Ok(json!(windows))
        }
        "get_journal" => Ok(json!(journal_view(core, params))),
        "run_status" => run_status(core),
        "run_scenario" => {
            let start_node = params.get("startNode").and_then(Value::as_str).map(str::trim).filter(|id| !id.is_empty());
            run_scenario(core, &required_str(params, "id")?, start_node.map(str::to_string))
        }
        "stop_scenario" => {
            core.stop();
            Ok(json!({"stopped": true}))
        }
        "capture_screen" => capture_screen(core, params),
        "pixel_color" => pixel_color(core, params),
        "save_template" => save_template(core, params),
        _ => Err(AppError::new("notFound").with("message", method)),
    }
}

fn required_str(params: &Value, key: &str) -> AppResult<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::new("invalid"))
}

/// Reads the scenario a tool was given and brings it to the shape that is
/// stored: ids filled in, new blocks laid out, frames drawn around their blocks.
pub(crate) fn scenario_from(params: &Value) -> AppResult<Scenario> {
    let value = params.get("scenario").cloned().unwrap_or(Value::Null);
    let around = frames_around(&value);
    let mut scenario: Scenario = serde_json::from_value(value)?;
    scenario.fill_empty_ids();
    scenario.version = scenario.format_version();
    if let Some(graph) = scenario.graph.as_mut() {
        graph.place_missing();
        for (index, ids) in around {
            let Some((x, y, width, height)) = graph.frame_around(&ids) else {
                continue;
            };
            if let Some(note) = graph.notes.get_mut(index) {
                (note.x, note.y, note.width, note.height) = (x, y, width, height);
            }
        }
    }
    Ok(scenario)
}

/// Frames that name their blocks instead of a rectangle: index of the note and the ids.
fn frames_around(scenario: &Value) -> Vec<(usize, Vec<String>)> {
    let notes = scenario.get("graph").and_then(|graph| graph.get("notes")).and_then(Value::as_array);
    notes
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(index, note)| {
            let ids: Vec<String> =
                note.get("around")?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect();
            (!ids.is_empty()).then_some((index, ids))
        })
        .collect()
}

/// A scenario as the tools see it: the stored object and how it is stored.
fn scenario_view(scenario: &Scenario) -> AppResult<Value> {
    let mut value = serde_json::to_value(scenario)?;
    value["format"] = json!(scenario.format());
    Ok(value)
}

fn issues_of(core: &Core, scenario: &Scenario) -> Vec<Issue> {
    core.store.issues(scenario)
}

fn create_scenario(core: &Core, name: &str, format: &str) -> AppResult<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new("nameEmpty"));
    }
    let scenario = match format {
        "list" => Scenario::new(name),
        "canvas" => {
            let mut scenario = Scenario::new_canvas(name);
            if let Some(graph) = scenario.graph.as_mut() {
                graph.place_missing();
            }
            scenario
        }
        _ => return Err(AppError::new("invalid")),
    };
    core.store.save_scenario(&scenario)?;
    core.refresh_tray();
    announce(core, &scenario.id);
    scenario_view(&scenario)
}

/// Turns a list into a graph: in place, or as a new scenario when `copy` is set.
fn convert_scenario(core: &Core, params: &Value) -> AppResult<Value> {
    let id = required_str(params, "id")?;
    let copy = params.get("copy").and_then(Value::as_bool).unwrap_or(false);
    let name = if copy {
        let given = params.get("name").and_then(Value::as_str).map(str::trim).filter(|name| !name.is_empty());
        Some(match given {
            Some(name) => name.to_string(),
            None => {
                let source = core.store.load_scenario(&id)?;
                text(&core.lang(), "scenarios.canvasCopyName", &[("name", &source.name)])
            }
        })
    } else {
        None
    };
    let scenario = core.scenario_to_canvas(&id, name.as_deref())?;
    announce(core, &scenario.id);
    scenario_view(&scenario)
}

/// The condition of a step of a list, wherever the step is nested.
fn step_condition(steps: &[Step], id: &str) -> Option<Condition> {
    for step in steps {
        if step.id == id {
            return match &step.kind {
                StepKind::Wait { condition, .. }
                | StepKind::If { condition, .. }
                | StepKind::Loop { mode: LoopMode::While { condition }, .. } => Some(condition.clone()),
                _ => None,
            };
        }
        let nested = match &step.kind {
            StepKind::If { then_steps, else_steps, .. } => {
                step_condition(then_steps, id).or_else(|| step_condition(else_steps, id))
            }
            StepKind::Loop { steps, .. } => step_condition(steps, id),
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
    }
    None
}

/// "Check now" for a tool: the condition of a block or a step, or one that is not saved yet.
fn check_condition(core: &Core, params: &Value) -> AppResult<Value> {
    let scenario = core.store.load_scenario(&required_str(params, "id")?)?;
    let node_id = params.get("nodeId").and_then(Value::as_str).map(str::trim).filter(|id| !id.is_empty());
    let condition = match (node_id, params.get("condition").filter(|value| !value.is_null())) {
        (Some(node_id), _) => match &scenario.graph {
            Some(graph) => graph.node(node_id).and_then(|node| node.kind.condition().cloned()),
            None => step_condition(&scenario.steps, node_id),
        }
        .ok_or_else(|| AppError::new("notFound"))?,
        (None, Some(value)) => serde_json::from_value::<Condition>(value.clone())?,
        (None, None) => return Err(AppError::new("invalid")),
    };
    let result = core.check_condition(&scenario.id, scenario.mode, &scenario.target, &condition)?;
    core.journal.add("harnessCheck", &[("name", scenario.name.clone())]);
    Ok(serde_json::to_value(result)?)
}

/// The live status; for a graph scenario it also names the block it is on.
fn run_status(core: &Core) -> AppResult<Value> {
    let status = core.hub.get();
    let mut value = serde_json::to_value(&status)?;
    let block = status.scenario_id.as_deref().zip(status.step_id.as_deref()).and_then(|(scenario_id, step_id)| {
        let graph = core.store.load_scenario(scenario_id).ok()?.graph?;
        let index = graph.nodes.iter().position(|node| node.id == step_id)?;
        let node = serde_json::to_value(&graph.nodes[index]).ok()?;
        Some(json!({"id": step_id, "label": graph.label(index), "type": node.get("type")}))
    });
    if let Some(block) = block {
        value["block"] = block;
    }
    Ok(value)
}

/// Saves an existing scenario. When `reject_errors` is set, error issues abort the write.
pub fn save_scenario(core: &Core, params: &Value, reject_errors: bool) -> AppResult<Value> {
    let mut scenario = scenario_from(params)?;
    if core.scenario_in_use(&scenario.id) {
        return Err(AppError::new("busy"));
    }
    let stored = core.store.load_scenario(&scenario.id)?;
    scenario.revision = bumped_revision(scenario.revision, stored.revision)?;
    // A list becomes a graph, and a graph a list again, only through a conversion.
    let mixed = scenario.graph.is_some() && !scenario.steps.is_empty();
    if mixed || stored.graph.is_some() != scenario.graph.is_some() {
        return Err(AppError::new("formatMismatch"));
    }
    let issues = issues_of(core, &scenario);
    if reject_errors && has_errors(&issues) {
        return Ok(json!({"saved": false, "issues": issues}));
    }
    if reject_errors {
        // A command runs with the rights of the user, and a scenario is later
        // started by hand without reading it again. So a tool does not write a
        // command the user has not seen.
        let added = added_commands(&stored, &scenario);
        if !added.is_empty() {
            if !confirm(core, Ask::Save, &scenario.name, &added)? {
                return Err(AppError::new("commandsDeclined"));
            }
            // The answer took a while: the scenario may have been started or changed meanwhile.
            if core.scenario_in_use(&scenario.id) {
                return Err(AppError::new("busy"));
            }
            if core.store.load_scenario(&scenario.id)?.revision != stored.revision {
                return Err(AppError::new("stale"));
            }
            core.journal
                .add("harnessCommands", &[("name", scenario.name.clone()), ("count", added.len().to_string())]);
        }
    }
    core.store.save_scenario(&scenario)?;
    if stored.name != scenario.name {
        core.refresh_tray();
    }
    // The editor saves through this function too. Only a harness save must
    // make the open editor drop its draft; the editor's own save must not.
    if reject_errors {
        announce(core, &scenario.id);
    }
    let warnings: Vec<&Issue> = issues.iter().filter(|issue| !matches!(issue.severity, crate::model::validate::Severity::Error)).collect();
    let saved = if reject_errors { scenario_view(&scenario)? } else { serde_json::to_value(&scenario)? };
    Ok(json!({"saved": true, "scenario": saved, "warnings": warnings}))
}

fn announce(core: &Core, id: &str) {
    let _ = core.app().emit("harness:scenario", json!({"id": id}));
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JournalView {
    time: String,
    code: String,
    params: serde_json::Map<String, Value>,
    text: String,
}

fn journal_view(core: &Core, params: &Value) -> Vec<JournalView> {
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(JOURNAL_DEFAULT as u64) as usize;
    let limit = limit.clamp(1, JOURNAL_MAX);
    let lang = core.lang();
    let entries = core.journal.read();
    entries
        .iter()
        .rev()
        .take(limit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|entry| journal_line(&lang, entry))
        .collect()
}

fn journal_line(lang: &str, entry: &JournalEntry) -> JournalView {
    let params: Vec<(String, String)> = entry
        .params
        .iter()
        .map(|(key, value)| {
            let shown = if key == "source" {
                text(lang, &format!("journal.sources.{value}"), &[])
            } else {
                value.clone()
            };
            (key.clone(), shown)
        })
        .collect();
    let pairs: Vec<(&str, &str)> = params.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect();
    let text = text(lang, &format!("journal.codes.{}", entry.code), &pairs);
    let mut map = serde_json::Map::new();
    for (key, value) in params {
        map.insert(key, Value::String(value));
    }
    JournalView { time: entry.time.clone(), code: entry.code.clone(), params: map, text }
}

fn run_scenario(core: &Core, id: &str, start_node: Option<String>) -> AppResult<Value> {
    let scenario = core.store.load_scenario(id)?;
    if let Some(node) = &start_node {
        // A trial run from a block exists only for a graph, and the block has to be there.
        if scenario.graph.as_ref().and_then(|graph| graph.node(node)).is_none() {
            return Err(AppError::new("notFound"));
        }
    }
    let issues = issues_of(core, &scenario);
    if has_errors(&issues) {
        return Ok(json!({"outcome": "error", "issues": issues}));
    }
    if core.engine.is_running() || core.recorder.is_active() || core.is_updating() {
        return Err(AppError::new("busy"));
    }
    let target = RunTarget { kind: TargetKind::Scenario, id: id.to_string() };
    let settings = core.settings();
    // Commands of the scenario and of everything it runs. With them the start
    // waits for the user whatever the auto-run setting says, and the question
    // shows what is about to be started.
    let commands = shown_commands(&build_plan(&core.store, &target)?.commands());
    if !commands.is_empty() && !settings.allow_commands {
        // No question about a start that cannot happen. The journal gets the
        // same entry a start refused by the core leaves.
        let error = AppError::new("commandsDisabled");
        core.journal.add_with_reason("startRejected", &[("name", scenario.name.clone())], &error);
        return Err(error);
    }
    let auto_run = settings.harness_auto_run && commands.is_empty();
    let accepted = if auto_run { false } else { confirm(core, Ask::Run, &scenario.name, &commands)? };
    let allowed = run_outcome(auto_run, accepted) == "started";
    if !allowed {
        return Ok(json!({"outcome": "declined"}));
    }
    if !commands.is_empty() {
        // The answer was about these commands: a scenario that changed while
        // the question was open is not the one that was allowed.
        let now = shown_commands(&build_plan(&core.store, &target)?.commands());
        if now != commands {
            return Err(AppError::new("stale"));
        }
    }
    core.start_from(&target, Source::Harness, start_node, None)?;
    Ok(json!({"outcome": "started"}))
}

/// A command the way the confirm dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShownCommand {
    text: String,
    /// Empty when the step does not set the folder.
    work_dir: String,
}

/// The commands for the dialog: every different one once, in a stable order.
fn shown_commands(commands: &[&CommandSpec]) -> Vec<ShownCommand> {
    let mut shown: Vec<ShownCommand> = commands
        .iter()
        .map(|command| ShownCommand { text: command.describe(), work_dir: command.work_dir.trim().to_string() })
        .collect();
    shown.sort();
    shown.dedup();
    shown
}

/// Commands a save would bring into a scenario: the ones that were not in it,
/// and the ones that were switched off there and would now run. A command is
/// what it starts and where; how long it is waited for is not a new command.
fn added_commands(stored: &Scenario, incoming: &Scenario) -> Vec<ShownCommand> {
    let known = shown_commands(&stored.commands(false));
    let live = shown_commands(&stored.commands(true));
    let mut added = shown_commands(&incoming.commands(false));
    added.retain(|command| !known.contains(command));
    let woken = shown_commands(&incoming.commands(true)).into_iter().filter(|command| !live.contains(command));
    added.extend(woken);
    added.sort();
    added.dedup();
    added
}

/// What the confirm dialog asks about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    /// Start a scenario.
    Run,
    /// Write new commands into a scenario.
    Save,
}

impl Ask {
    fn as_str(self) -> &'static str {
        match self {
            Ask::Run => "run",
            Ask::Save => "save",
        }
    }
}

/// `started` when auto-run is on or the dialog was accepted. A timeout is a decline.
pub(crate) fn run_outcome(auto_run: bool, accepted: bool) -> &'static str {
    if auto_run || accepted {
        "started"
    } else {
        "declined"
    }
}

/// Shows the main window and waits for the answer. `false` is a decline or a timeout.
fn confirm(core: &Core, ask: Ask, name: &str, commands: &[ShownCommand]) -> AppResult<bool> {
    let (tx, rx) = mpsc::channel();
    let id = {
        let mut slot = core.confirm.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_some() {
            return Err(AppError::new("pending"));
        }
        let id = core.confirm_seq.fetch_add(1, Ordering::SeqCst) + 1;
        *slot = Some(ConfirmWait { id, reply: tx });
        id
    };
    window::show_main(core.app());
    let _ = core.app().emit(
        "harness:confirm",
        json!({"requestId": id, "kind": ask.as_str(), "name": name, "commands": commands}),
    );
    let answer = rx.recv_timeout(CONFIRM_WAIT).unwrap_or(false);
    let mut slot = core.confirm.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if slot.as_ref().is_some_and(|wait| wait.id == id) {
        *slot = None;
    }
    Ok(answer)
}

pub fn reply(core: &Core, request_id: u64, accepted: bool) {
    let mut slot = core.confirm.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(wait) = slot.take() {
        if wait.id == request_id {
            let _ = wait.reply.send(accepted);
        } else {
            *slot = Some(wait);
        }
    }
}

fn capture_screen(core: &Core, params: &Value) -> AppResult<Value> {
    ensure_idle_capture(core)?;
    let (rect, client) = rect_of(params, false)?;
    let frame = shoot(core, rect)?;
    let (png, scale) = frame.preview_png(MAX_PREVIEW_SIDE)?;
    core.journal.add(
        "harnessCapture",
        &[("width", rect.width.to_string()), ("height", rect.height.to_string())],
    );
    let mut result = json!({
        "originX": rect.x,
        "originY": rect.y,
        "width": rect.width,
        "height": rect.height,
        "scale": scale,
        "pngBase64": STANDARD.encode(png),
    });
    if let Some(client) = client {
        result["clientX"] = json!(client.x);
        result["clientY"] = json!(client.y);
        result["clientWidth"] = json!(client.width);
        result["clientHeight"] = json!(client.height);
    }
    Ok(result)
}

fn pixel_color(core: &Core, params: &Value) -> AppResult<Value> {
    ensure_idle_capture(core)?;
    let (rect, _) = rect_of(params, true)?;
    let frame = shoot(core, rect)?;
    let color = frame
        .rgb(0, 0)
        .map(screen::color_text)
        .ok_or_else(|| AppError::new("captureFailed"))?;
    core.journal.add("harnessCapture", &[("width", "1".to_string()), ("height", "1".to_string())]);
    Ok(json!({"color": color, "x": rect.x, "y": rect.y}))
}

fn save_template(core: &Core, params: &Value) -> AppResult<Value> {
    ensure_idle_capture(core)?;
    let scenario_id = required_str(params, "scenarioId")?;
    if !core.store.scenario_exists(&scenario_id) {
        return Err(AppError::new("notFound"));
    }
    let (rect, _) = rect_of(params, false)?;
    if rect.width < MIN_TEMPLATE_SIDE || rect.height < MIN_TEMPLATE_SIDE {
        return Err(AppError::new("regionTooSmall"));
    }
    let frame = shoot(core, rect)?;
    let file = format!("tpl-{}.png", crate::model::scenario::new_id("t"));
    frame.save_png(&core.store.template_path(&scenario_id, &file)?)?;
    core.journal.add("harnessTemplate", &[("file", file.clone()), ("scenario", scenario_id)]);
    Ok(json!({"file": file, "width": rect.width, "height": rect.height}))
}

fn ensure_idle_capture(core: &Core) -> AppResult<()> {
    if core.engine.is_running() || core.recorder.is_active() || core.is_updating() {
        Err(AppError::new("busy"))
    } else {
        Ok(())
    }
}

/// Screen rectangle to capture, and the same rectangle in client coordinates when asked.
fn rect_of(params: &Value, one_pixel: bool) -> AppResult<(Rect, Option<Rect>)> {
    let target = params
        .get("target")
        .and_then(|value| serde_json::from_value::<WindowTarget>(value.clone()).ok())
        .unwrap_or_default();
    let client_space = params.get("space").and_then(Value::as_str) == Some("client");
    if one_pixel {
        let (x, y) = point_of(params)?;
        let rect = place(x, y, 1, 1, client_space, &target)?;
        return Ok((rect, None));
    }
    let width = number(params, "width");
    let height = number(params, "height");
    if client_space && (width <= 0 || height <= 0) {
        let hwnd = target::find_target(&target).ok_or_else(|| {
            AppError::new("targetNotFound").with("target", target::describe_target(&target))
        })?;
        let client = target::client_rect(hwnd).ok_or_else(|| AppError::new("captureFailed"))?;
        let local = Rect { x: 0, y: 0, width: client.width, height: client.height };
        return Ok((client, Some(local)));
    }
    if width <= 0 || height <= 0 {
        return Ok((screen::virtual_screen(), None));
    }
    let rect = place(number(params, "x"), number(params, "y"), width, height, client_space, &target)?;
    let client = client_space.then_some(Rect { x: number(params, "x"), y: number(params, "y"), width, height });
    Ok((rect, client))
}

fn point_of(params: &Value) -> AppResult<(i32, i32)> {
    match (params.get("x").and_then(Value::as_i64), params.get("y").and_then(Value::as_i64)) {
        (Some(x), Some(y)) => Ok((x as i32, y as i32)),
        _ => Err(AppError::new("invalid")),
    }
}

fn number(params: &Value, key: &str) -> i32 {
    params.get(key).and_then(Value::as_i64).unwrap_or(0) as i32
}

fn place(x: i32, y: i32, width: i32, height: i32, client_space: bool, target: &WindowTarget) -> AppResult<Rect> {
    if !client_space {
        return Ok(Rect { x, y, width, height });
    }
    let hwnd = target::find_target(target).ok_or_else(|| {
        AppError::new("targetNotFound").with("target", target::describe_target(target))
    })?;
    let client = target::client_rect(hwnd).ok_or_else(|| AppError::new("captureFailed"))?;
    Ok(Rect { x: client.x.saturating_add(x), y: client.y.saturating_add(y), width, height })
}

fn shoot(core: &Core, rect: Rect) -> AppResult<screen::Frame> {
    let restore = window::main_visible(core.app());
    if restore {
        window::hide_main(core.app());
        thread::sleep(Duration::from_millis(200));
    }
    let frame = screen::capture(rect);
    if restore {
        window::show_main(core.app());
    }
    frame
}

/// Listens while the harness setting is on. A second connection wakes a blocked wait.
pub fn serve(core: Arc<Core>) {
    let _ = thread::Builder::new().name("clickit-harness".to_string()).spawn(move || loop {
        if core.is_quitting() {
            break;
        }
        if !core.settings().harness {
            thread::sleep(Duration::from_millis(400));
            continue;
        }
        // The client reads this file before it opens the pipe, so it has to exist first.
        let _ = core.store.harness_token();
        if let Ok(handle) = pipe::listen() {
            let _ = pipe::serve_client(&core, handle);
        } else {
            thread::sleep(Duration::from_millis(400));
        }
    });
}

/// Wakes a blocked `ConnectNamedPipe`, so turning the setting off takes effect.
pub fn kick() {
    let _ = std::fs::OpenOptions::new().read(true).write(true).open(PIPE_NAME);
}

mod pipe {
    use super::*;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, HANDLE};
    use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    /// The process default access list already limits the pipe to this user.
    /// An explicit "current user" descriptor is rejected on this Windows, and a
    /// failed listen must not spin.
    pub fn listen() -> AppResult<HANDLE> {
        let name: Vec<u16> = PIPE_NAME.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                1024 * 1024,
                1024 * 1024,
                0,
                None,
            )
        };
        if handle.is_invalid() {
            Err(AppError::new("io").with("message", "pipe"))
        } else {
            Ok(handle)
        }
    }

    pub fn serve_client(core: &Core, handle: HANDLE) -> AppResult<()> {
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        let connected = connected.is_ok()
            || connected.as_ref().err().is_some_and(|error| error.code() == ERROR_PIPE_CONNECTED.to_hresult());
        if !connected || !core.settings().harness || core.is_quitting() {
            close(handle);
            return Ok(());
        }
        let token = core.store.harness_token().unwrap_or_default();
        let ok = read_line(handle).is_some_and(|line| line == token);
        let mut session = None;
        if ok {
            while let Some(line) = read_line(handle) {
                if !core.settings().harness || core.is_quitting() {
                    break;
                }
                let response = match parse_request(&line) {
                    Ok(request) => {
                        let outcome = call(core, &request.method, &request.params);
                        crate::external::record(
                            core,
                            &mut session,
                            &request.client,
                            &request.method,
                            &request.params,
                            &outcome,
                        );
                        match outcome {
                            Ok(result) => json!({"id": request.id, "result": result}),
                            Err(error) => json!({"id": request.id, "error": error}),
                        }
                    }
                    Err(error) => json!({"id": 0, "error": error}),
                };
                let mut bytes = serde_json::to_vec(&response).unwrap_or_default();
                bytes.push(b'\n');
                if !write_all(handle, &bytes) {
                    break;
                }
            }
        }
        crate::external::close(core, session);
        close(handle);
        Ok(())
    }

    fn close(handle: HANDLE) {
        unsafe {
            let _ = DisconnectNamedPipe(handle);
            let _ = CloseHandle(handle);
        }
    }

    fn read_line(handle: HANDLE) -> Option<String> {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            let mut read = 0u32;
            let ok = unsafe { ReadFile(handle, Some(&mut byte), Some(&mut read), None) };
            if ok.is_err() || read == 0 {
                return None;
            }
            if byte[0] == b'\n' {
                break;
            }
            if byte[0] != b'\r' {
                buf.push(byte[0]);
            }
            if buf.len() > 16 * 1024 * 1024 {
                return None;
            }
        }
        String::from_utf8(buf).ok()
    }

    fn write_all(handle: HANDLE, bytes: &[u8]) -> bool {
        let mut sent = 0;
        while sent < bytes.len() {
            let mut wrote = 0u32;
            let ok = unsafe { WriteFile(handle, Some(&bytes[sent..]), Some(&mut wrote), None) };
            if ok.is_err() || wrote == 0 {
                return false;
            }
            sent += wrote as usize;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_grows_and_a_stale_copy_is_rejected() {
        assert_eq!(bumped_revision(0, 0).unwrap(), 1);
        assert_eq!(bumped_revision(4, 4).unwrap(), 5);
        assert_eq!(bumped_revision(3, 4).unwrap_err().code, "stale");
    }

    #[test]
    fn a_saved_graph_gets_ids_a_layout_and_frames_around_the_named_blocks() {
        let scenario = scenario_from(&json!({"scenario": {
            "id": "sc-1",
            "name": "Demo",
            "graph": {
                "nodes": [
                    {"id": "s", "type": "start", "x": 40.0, "y": 60.0},
                    {"id": "", "type": "pause", "duration": {"fromMs": 5, "toMs": 5}},
                    {"id": "w", "type": "text", "text": "hi"}
                ],
                "edges": [{"from": "s", "port": "next", "to": "w"}],
                "notes": [{"kind": "frame", "text": "Typing", "around": ["w"]}, {"kind": "note", "text": "Plain"}]
            }
        }}))
        .unwrap();
        assert_eq!(scenario.version, crate::model::graph::GRAPH_VERSION);
        let graph = scenario.graph.unwrap();
        assert!(graph.nodes.iter().all(|node| !node.id.is_empty() && node.x.is_some() && node.y.is_some()));
        // A block that came with a place keeps it.
        assert_eq!((graph.nodes[0].x, graph.nodes[0].y), (Some(40.0), Some(60.0)));
        let typed = &graph.nodes[2];
        let frame = &graph.notes[0];
        assert!(frame.x < typed.x.unwrap() && frame.y < typed.y.unwrap());
        assert!(frame.width > crate::model::graph::BLOCK_WIDTH && frame.height > crate::model::graph::BLOCK_HEIGHT);
        assert!(!frame.id.is_empty());
        assert_eq!((graph.notes[1].width, graph.notes[1].height), (0.0, 0.0));
    }

    /// The shortest graph a tool may send, the one the README shows: every field left out has a default.
    #[test]
    fn a_graph_with_only_the_required_fields_is_valid() {
        let scenario = scenario_from(&json!({"scenario": {
            "id": "sc-1",
            "name": "Demo",
            "graph": {
                "nodes": [
                    {"id": "s", "type": "start"},
                    {
                        "id": "w",
                        "type": "wait",
                        "title": "Wait for the button",
                        "condition": {"kind": "imageFound", "template": "tpl-button.png", "area": {"kind": "screen"}, "threshold": 0.9},
                        "pollMs": 200,
                        "timeoutMs": 10000
                    },
                    {
                        "id": "c",
                        "type": "click",
                        "title": "Press",
                        "button": "left",
                        "action": "click",
                        "position": {"kind": "absolute", "x": 640, "y": 360}
                    }
                ],
                "edges": [{"from": "s", "port": "next", "to": "w"}, {"from": "w", "port": "next", "to": "c"}],
                "notes": []
            }
        }}))
        .unwrap();
        let issues = crate::model::validate::validate_scenario(&scenario, &|_| true);
        let errors: Vec<&str> = issues
            .iter()
            .filter(|issue| issue.severity == crate::model::validate::Severity::Error)
            .map(|issue| issue.code.as_str())
            .collect();
        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn a_tool_sees_the_format_of_a_scenario() {
        let list = scenario_view(&Scenario::new("List")).unwrap();
        assert_eq!(list["format"], "list");
        let canvas = scenario_view(&Scenario::new_canvas("Canvas")).unwrap();
        assert_eq!(canvas["format"], "canvas");
        assert_eq!(canvas["graph"]["nodes"][0]["type"], "start");
        // What a tool reads it can send back: the extra field is ignored.
        assert!(scenario_from(&json!({"scenario": canvas})).unwrap().graph.is_some());
    }

    #[test]
    fn the_condition_of_a_nested_step_is_found_by_its_id() {
        let wait = Step::new(StepKind::Wait {
            condition: Condition::Process { name: "game.exe".into() },
            poll_ms: 200,
            timeout_ms: 0,
            on_timeout: crate::model::scenario::OnTimeout::Stop,
            on_found: crate::model::scenario::OnFound::None,
        });
        let wait_id = wait.id.clone();
        let outer = Step::new(StepKind::Loop {
            mode: LoopMode::Forever,
            pause_between: Default::default(),
            steps: vec![wait],
        });
        let outer_id = outer.id.clone();
        let steps = [outer];
        assert_eq!(step_condition(&steps, &wait_id), Some(Condition::Process { name: "game.exe".into() }));
        assert_eq!(step_condition(&steps, &outer_id), None);
        assert_eq!(step_condition(&steps, "nope"), None);
    }

    #[test]
    fn every_tool_has_a_name_a_description_and_an_object_schema() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
        for expected in ["get_scenario", "save_scenario", "convert_scenario", "check_condition", "get_trace", "run_scenario"] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        for tool in &tools {
            assert!(!tool["description"].as_str().unwrap_or("").is_empty());
            assert_eq!(tool["parameters"]["type"], "object");
        }
    }

    #[test]
    fn a_request_line_parses() {
        let request = parse_request(r#"{"id":7,"method":"pixel_color","params":{"x":1,"y":2}}"#).unwrap();
        assert_eq!(request.id, 7);
        assert_eq!(request.method, "pixel_color");
        assert_eq!(request.params["x"], 1);
    }

    #[test]
    fn a_timeout_declines_and_auto_run_starts() {
        assert_eq!(run_outcome(false, false), "declined");
        assert_eq!(run_outcome(false, true), "started");
        assert_eq!(run_outcome(true, false), "started");
    }

    fn with_commands(steps: Value) -> Scenario {
        scenario_from(&json!({"scenario": {"id": "sc-1", "name": "Demo", "steps": steps}})).unwrap()
    }

    fn script(text: &str) -> Value {
        json!({"type": "command", "command": {"run": {"kind": "script", "shell": "cmd", "text": text}}})
    }

    #[test]
    fn a_save_asks_about_the_commands_it_brings_and_about_the_ones_it_switches_on() {
        let texts = |added: Vec<ShownCommand>| added.into_iter().map(|command| command.text).collect::<Vec<_>>();
        let mut off = script("echo off");
        off["enabled"] = json!(false);
        let stored = with_commands(json!([script("echo kept"), off.clone()]));

        // Nothing new: the same commands, one of them waited for differently.
        let mut patient = script("echo kept");
        patient["command"]["timeoutMs"] = json!(5000);
        assert!(added_commands(&stored, &with_commands(json!([patient, off.clone()]))).is_empty());
        // Taking a command away asks nothing.
        assert!(added_commands(&stored, &with_commands(json!([]))).is_empty());

        // A new command is asked about even while its step is switched off.
        let mut asleep = script("echo new");
        asleep["enabled"] = json!(false);
        let added = added_commands(&stored, &with_commands(json!([script("echo kept"), off, asleep])));
        assert_eq!(texts(added), ["[cmd]\necho new"]);
        // So is one the user had switched off.
        let added = added_commands(&stored, &with_commands(json!([script("echo kept"), script("echo off")])));
        assert_eq!(texts(added), ["[cmd]\necho off"]);
        // The same text in another folder is another command.
        let mut moved = script("echo kept");
        moved["command"]["workDir"] = json!("C:\\Temp");
        let added = added_commands(&stored, &with_commands(json!([moved])));
        assert_eq!(added, [ShownCommand { text: "[cmd]\necho kept".to_string(), work_dir: "C:\\Temp".to_string() }]);
    }

    #[test]
    fn preview_scale_maps_back_to_the_frame() {
        let (width, _height, scale) = crate::screen::preview_size(3200, 1600, MAX_PREVIEW_SIDE);
        assert_eq!(width, 1600);
        let screen_x = 0.0 + 10.0 * scale;
        assert!((screen_x - 20.0).abs() < 0.01);
    }
}
