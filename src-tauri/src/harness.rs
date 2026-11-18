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
use crate::engine::Source;
use crate::error::{AppError, AppResult};
use crate::model::scenario::{Scenario, WindowTarget, SCENARIO_VERSION};
use crate::model::schedule::{RunTarget, TargetKind};
use crate::model::validate::{has_errors, validate_scenario, Issue};
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
            "List saved scenarios: id, name, mode (screen or window) and step count.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "get_scenario",
            "Read one scenario, including its steps and revision.",
            json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        ),
        tool(
            "create_scenario",
            "Create an empty scenario. The core assigns the id.",
            json!({"type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}),
        ),
        tool(
            "save_scenario",
            "Replace a scenario. Send the object from get_scenario with your edits, including revision. Empty step ids are filled in. Errors are returned and nothing is written. Steps use type click, move, drag, scroll, key, text, pause, wait, if, loop, breakLoop or stop. Coordinates are absolute for screen mode and client for window mode. Image steps refer to a template file name from save_template.",
            json!({
                "type": "object",
                "properties": {"scenario": {"type": "object"}},
                "required": ["scenario"]
            }),
        ),
        tool(
            "validate_scenario",
            "Check a scenario without writing it. Returns the issues.",
            json!({"type": "object", "properties": {"scenario": {"type": "object"}}, "required": ["scenario"]}),
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
            "What a run or a recording is doing right now.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "run_scenario",
            "Start a scenario by id. Does not wait until it finishes: a scenario may run until stopped. Returns outcome started or declined. Unless auto-run is enabled, ClickIt asks the user first.",
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
        "get_scenario" => {
            let scenario = core.store.load_scenario(&required_str(params, "id")?)?;
            Ok(serde_json::to_value(scenario)?)
        }
        "create_scenario" => create_scenario(core, &required_str(params, "name")?),
        "save_scenario" => save_scenario(core, params, true),
        "validate_scenario" => {
            let mut scenario = scenario_from(params)?;
            scenario.fill_empty_ids();
            Ok(json!({"issues": issues_of(core, &scenario)}))
        }
        "list_windows" => {
            let windows: Vec<Value> = target::top_windows()
                .into_iter()
                .map(|window| json!({"title": window.title, "process": window.process}))
                .collect();
            Ok(json!(windows))
        }
        "get_journal" => Ok(json!(journal_view(core, params))),
        "run_status" => Ok(serde_json::to_value(core.hub.get())?),
        "run_scenario" => run_scenario(core, &required_str(params, "id")?),
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

fn scenario_from(params: &Value) -> AppResult<Scenario> {
    let value = params.get("scenario").cloned().unwrap_or(Value::Null);
    Ok(serde_json::from_value(value)?)
}

fn issues_of(core: &Core, scenario: &Scenario) -> Vec<Issue> {
    validate_scenario(scenario, &|file| core.store.template_exists(&scenario.id, file))
}

fn create_scenario(core: &Core, name: &str) -> AppResult<Value> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::new("nameEmpty"));
    }
    let scenario = Scenario::new(name);
    core.store.save_scenario(&scenario)?;
    core.refresh_tray();
    announce(core, &scenario.id);
    Ok(serde_json::to_value(scenario)?)
}

/// Saves an existing scenario. When `reject_errors` is set, error issues abort the write.
pub fn save_scenario(core: &Core, params: &Value, reject_errors: bool) -> AppResult<Value> {
    let mut scenario = scenario_from(params)?;
    scenario.fill_empty_ids();
    scenario.version = SCENARIO_VERSION;
    if core.scenario_in_use(&scenario.id) {
        return Err(AppError::new("busy"));
    }
    let stored = core.store.load_scenario(&scenario.id)?;
    scenario.revision = bumped_revision(scenario.revision, stored.revision)?;
    let issues = issues_of(core, &scenario);
    if reject_errors && has_errors(&issues) {
        return Ok(json!({"saved": false, "issues": issues}));
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
    Ok(json!({"saved": true, "scenario": scenario, "warnings": warnings}))
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

fn run_scenario(core: &Core, id: &str) -> AppResult<Value> {
    let scenario = core.store.load_scenario(id)?;
    let issues = issues_of(core, &scenario);
    if has_errors(&issues) {
        return Ok(json!({"outcome": "error", "issues": issues}));
    }
    if core.engine.is_running() || core.recorder.is_active() || core.is_updating() {
        return Err(AppError::new("busy"));
    }
    let auto_run = core.settings().harness_auto_run;
    let accepted = if auto_run { false } else { confirm(core, &scenario.name)? };
    let allowed = run_outcome(auto_run, accepted) == "started";
    if !allowed {
        return Ok(json!({"outcome": "declined"}));
    }
    core.start(&RunTarget { kind: TargetKind::Scenario, id: id.to_string() }, Source::Harness)?;
    Ok(json!({"outcome": "started"}))
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
fn confirm(core: &Core, name: &str) -> AppResult<bool> {
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
    let _ = core.app().emit("harness:confirm", json!({"requestId": id, "name": name}));
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

    #[test]
    fn preview_scale_maps_back_to_the_frame() {
        let (width, _height, scale) = crate::screen::preview_size(3200, 1600, MAX_PREVIEW_SIDE);
        assert_eq!(width, 1600);
        let screen_x = 0.0 + 10.0 * scale;
        assert!((screen_x - 20.0).abs() < 0.01);
    }
}
