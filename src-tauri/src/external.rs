//! Tool calls that arrived through the local pipe, shown on the Assistant page.
//! The words of the harness chat stay in that program: the pipe only carries
//! tool calls. Pictures stay in memory; the file keeps text.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;

use crate::app::Core;
use crate::error::{AppError, AppResult};
use crate::shell::text::{error_text, text};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DialogRole {
    Call,
    Result,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogEntry {
    pub id: u64,
    pub role: DialogRole,
    pub text: String,
    /// PNG bytes, base64, for this process only. Never written to disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_png: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dialog {
    pub id: u64,
    pub client: String,
    pub open: bool,
    pub entries: Vec<DialogEntry>,
}

/// One step pushed to the window. `entry` is absent when the connection only changes state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DialogEvent {
    pub id: u64,
    pub client: String,
    pub open: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<DialogEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredLine {
    session: u64,
    client: String,
    id: u64,
    role: DialogRole,
    text: String,
}

struct Inner {
    ready: bool,
    next_session: u64,
    next_entry: u64,
    dialogs: Vec<Dialog>,
}

pub struct Log {
    inner: Mutex<Inner>,
}

impl Log {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner { ready: false, next_session: 1, next_entry: 1, dialogs: Vec::new() }),
        }
    }
}

impl Default for Log {
    fn default() -> Self {
        Self::new()
    }
}

/// Remembers one pipe call and its result. `session` is the open connection.
pub fn record(
    core: &Core,
    session: &mut Option<u64>,
    client: &str,
    method: &str,
    params: &Value,
    outcome: &AppResult<Value>,
) {
    let noted = note(&core.lang(), method, params, outcome);
    let mut events = Vec::new();
    {
        let mut inner = lock(core);
        ensure_loaded(core, &mut inner);
        let session_id = match *session {
            Some(id) => id,
            None => {
                let id = inner.next_session;
                inner.next_session = inner.next_session.saturating_add(1);
                let client = client_name(client);
                inner.dialogs.push(Dialog { id, client, open: true, entries: Vec::new() });
                *session = Some(id);
                id
            }
        };
        if let Some(event) = push(&mut inner, core, session_id, DialogRole::Call, noted.call, None) {
            events.push(event);
        }
        if let Some(event) = push(&mut inner, core, session_id, DialogRole::Result, noted.result, noted.image_png) {
            events.push(event);
        }
    }
    emit_all(core, events);
}

/// Marks the pipe connection closed and adds a closing line.
pub fn close(core: &Core, session: Option<u64>) {
    let Some(session_id) = session else {
        return;
    };
    let closed = text(&core.lang(), "agent.externalLines.closed", &[]);
    let event = {
        let mut inner = lock(core);
        ensure_loaded(core, &mut inner);
        let Some(dialog) = inner.dialogs.iter_mut().find(|dialog| dialog.id == session_id) else {
            return;
        };
        if !dialog.open {
            return;
        }
        dialog.open = false;
        push(&mut inner, core, session_id, DialogRole::Result, closed, None)
    };
    if let Some(event) = event {
        emit_all(core, vec![event]);
    }
}

pub fn list(core: &Core) -> Vec<Dialog> {
    let mut inner = lock(core);
    ensure_loaded(core, &mut inner);
    inner.dialogs.clone()
}

/// Drops a closed connection and its lines. An open connection stays: the pipe is still writing to it.
pub fn forget(core: &Core, session_id: u64) -> AppResult<()> {
    let mut inner = lock(core);
    ensure_loaded(core, &mut inner);
    let Some(removed) = take_closed(&mut inner.dialogs, session_id)? else {
        return Ok(());
    };
    if let Err(error) = write_dialogs(&core.store.harness_dialog_path(), &inner.dialogs) {
        inner.dialogs.insert(removed.0, removed.1);
        return Err(error);
    }
    Ok(())
}

/// Removes a closed dialog. `None` when that id is already gone. An open dialog is left in place.
fn take_closed(dialogs: &mut Vec<Dialog>, session_id: u64) -> AppResult<Option<(usize, Dialog)>> {
    let Some(index) = dialogs.iter().position(|dialog| dialog.id == session_id) else {
        return Ok(None);
    };
    if dialogs[index].open {
        return Err(AppError::new("dialogOpen"));
    }
    Ok(Some((index, dialogs.remove(index))))
}

fn journal_text(dialogs: &[Dialog]) -> AppResult<String> {
    let mut body = String::new();
    for dialog in dialogs {
        for entry in &dialog.entries {
            let stored = StoredLine {
                session: dialog.id,
                client: dialog.client.clone(),
                id: entry.id,
                role: entry.role,
                text: entry.text.clone(),
            };
            body.push_str(&serde_json::to_string(&stored)?);
            body.push('\n');
        }
    }
    Ok(body)
}

fn write_dialogs(path: &std::path::Path, dialogs: &[Dialog]) -> AppResult<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, journal_text(dialogs)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    Ok(())
}

struct Noted {
    call: String,
    result: String,
    image_png: Option<String>,
}

fn note(lang: &str, method: &str, params: &Value, outcome: &AppResult<Value>) -> Noted {
    let call = call_line(method, params);
    match outcome {
        Ok(result) => {
            let image_png = result.get("pngBase64").and_then(Value::as_str).map(str::to_string);
            Noted { call, result: result_line(lang, method, result), image_png }
        }
        Err(error) => Noted { call, result: error_text(lang, error), image_png: None },
    }
}

fn call_line(method: &str, params: &Value) -> String {
    let detail = match method {
        "get_scenario" | "run_scenario" => field(params, "id"),
        "create_scenario" => field(params, "name"),
        "save_scenario" | "validate_scenario" => scenario_label(params),
        "capture_screen" | "save_template" => rect_label(params),
        "pixel_color" => point_label(params),
        "get_journal" => params.get("limit").and_then(Value::as_u64).map(|limit| limit.to_string()).unwrap_or_default(),
        _ => String::new(),
    };
    if detail.is_empty() {
        method.to_string()
    } else {
        format!("{method} · {detail}")
    }
}

fn result_line(lang: &str, method: &str, result: &Value) -> String {
    match method {
        "list_scenarios" => line(lang, "scenarios", &[("count", &len_text(result))]),
        "get_scenario" | "create_scenario" => {
            line(lang, if method == "create_scenario" { "created" } else { "scenario" }, &[("name", &name_of(result))])
        }
        "save_scenario" if result.get("saved").and_then(Value::as_bool) == Some(false) => line(lang, "rejected", &[]),
        "save_scenario" => {
            let scenario = result.get("scenario").cloned().unwrap_or(Value::Null);
            line(lang, "saved", &[("name", &name_of(&scenario))])
        }
        "validate_scenario" => line(lang, "issues", &[("count", &len_text(result.get("issues").unwrap_or(&Value::Null)))]),
        "list_windows" => line(lang, "windows", &[("count", &len_text(result))]),
        "get_journal" => line(lang, "journal", &[("count", &len_text(result))]),
        "run_status" => status_line(lang, result),
        "run_scenario" => match result.get("outcome").and_then(Value::as_str).unwrap_or("") {
            "started" => line(lang, "started", &[]),
            "declined" => line(lang, "declined", &[]),
            _ => line(lang, "runError", &[]),
        },
        "stop_scenario" => line(lang, "stopped", &[]),
        "capture_screen" => line(lang, "capture", &[("width", &num(result, "width")), ("height", &num(result, "height"))]),
        "pixel_color" => line(lang, "color", &[("color", &field(result, "color")), ("x", &num(result, "x")), ("y", &num(result, "y"))]),
        "save_template" => line(
            lang,
            "template",
            &[("file", &field(result, "file")), ("width", &num(result, "width")), ("height", &num(result, "height"))],
        ),
        _ => line(lang, "done", &[]),
    }
}

fn status_line(lang: &str, result: &Value) -> String {
    let state = result.get("state").and_then(Value::as_str).unwrap_or("idle");
    let key = match state {
        "idle" => "status.idle",
        "countdown" => "status.countdown",
        "running" => "status.running",
        "waiting" => "status.waiting",
        "pausedWindow" => "status.pausedWindow",
        "pausedProcess" => "status.pausedProcess",
        "recording" => "status.recording",
        _ => "agent.externalLines.done",
    };
    let seconds = result.get("countdownLeft").and_then(Value::as_u64).unwrap_or(0).to_string();
    let process = result.get("blockedProcess").and_then(Value::as_str).unwrap_or("");
    text(lang, key, &[("seconds", seconds.as_str()), ("process", process)])
}

fn line(lang: &str, name: &str, params: &[(&str, &str)]) -> String {
    text(lang, &format!("agent.externalLines.{name}"), params)
}

fn push(inner: &mut Inner, core: &Core, session_id: u64, role: DialogRole, text_line: String, image_png: Option<String>) -> Option<DialogEvent> {
    let id = inner.next_entry;
    inner.next_entry = inner.next_entry.saturating_add(1);
    let entry = DialogEntry { id, role, text: text_line, image_png };
    let dialog = inner.dialogs.iter_mut().find(|dialog| dialog.id == session_id)?;
    let stored = StoredLine {
        session: dialog.id,
        client: dialog.client.clone(),
        id: entry.id,
        role: entry.role,
        text: entry.text.clone(),
    };
    if let Ok(line) = serde_json::to_string(&stored) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(core.store.harness_dialog_path()) {
            let _ = writeln!(file, "{line}");
        }
    }
    let event = DialogEvent {
        id: dialog.id,
        client: dialog.client.clone(),
        open: dialog.open,
        entry: Some(entry.clone()),
    };
    dialog.entries.push(entry);
    Some(event)
}

fn emit_all(core: &Core, events: Vec<DialogEvent>) {
    for event in events {
        let _ = core.app().emit("harness:dialog", &event);
    }
}

fn ensure_loaded(core: &Core, inner: &mut Inner) {
    if inner.ready {
        return;
    }
    inner.ready = true;
    let Ok(text) = fs::read_to_string(core.store.harness_dialog_path()) else {
        return;
    };
    for line in text.lines() {
        let Ok(stored) = serde_json::from_str::<StoredLine>(line) else {
            continue;
        };
        if inner.dialogs.last().is_none_or(|dialog| dialog.id != stored.session) {
            inner.dialogs.push(Dialog { id: stored.session, client: stored.client.clone(), open: false, entries: Vec::new() });
        }
        if let Some(dialog) = inner.dialogs.last_mut() {
            dialog.entries.push(DialogEntry {
                id: stored.id,
                role: stored.role,
                text: stored.text,
                image_png: None,
            });
        }
        inner.next_session = inner.next_session.max(stored.session.saturating_add(1));
        inner.next_entry = inner.next_entry.max(stored.id.saturating_add(1));
    }
}

fn lock(core: &Core) -> std::sync::MutexGuard<'_, Inner> {
    core.external.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn client_name(raw: &str) -> String {
    let name: String = raw.chars().filter(|char| !char.is_control()).take(80).collect();
    let name = name.trim();
    if name.is_empty() {
        "MCP".to_string()
    } else {
        name.to_string()
    }
}

fn field(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

fn num(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_i64).unwrap_or(0).to_string()
}

fn len_text(value: &Value) -> String {
    value.as_array().map(|items| items.len()).unwrap_or(0).to_string()
}

fn name_of(value: &Value) -> String {
    let name = field(value, "name");
    if name.is_empty() {
        field(value, "id")
    } else {
        name
    }
}

fn scenario_label(params: &Value) -> String {
    let scenario = params.get("scenario").cloned().unwrap_or_else(|| json!({}));
    name_of(&scenario)
}

fn rect_label(params: &Value) -> String {
    let width = params.get("width").and_then(Value::as_i64);
    let height = params.get("height").and_then(Value::as_i64);
    match (width, height) {
        (Some(width), Some(height)) => format!("{},{} {width}×{height}", num(params, "x"), num(params, "y")),
        _ => String::new(),
    }
}

/// Text to paste into a harness MCP config. `servers` replaces an empty `"mcpServers": {}`.
/// `entry` is only the clickit record, for a list that already has other servers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpSnippet {
    pub servers: String,
    pub entry: String,
    pub found: bool,
}

pub fn mcp_snippet(mcp_path: &std::path::Path, found: bool) -> McpSnippet {
    let command = Value::String(mcp_path.display().to_string()).to_string();
    let servers = format!("\"mcpServers\": {{\n  \"clickit\": {{\n    \"command\": {command}\n  }}\n}}");
    let entry = format!("\"clickit\": {{\n  \"command\": {command}\n}}");
    McpSnippet { servers, entry, found }
}

/// Path of `clickit-mcp.exe` next to the running ClickIt.
pub fn running_mcp_snippet() -> McpSnippet {
    let path = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("clickit-mcp.exe"))
        .unwrap_or_else(|| std::path::PathBuf::from("clickit-mcp.exe"));
    let found = path.is_file();
    mcp_snippet(&path, found)
}

fn point_label(params: &Value) -> String {
    if params.get("x").is_none() && params.get("y").is_none() {
        String::new()
    } else {
        format!("{},{}", num(params, "x"), num(params, "y"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn a_capture_line_keeps_the_picture_out_of_the_text() {
        let result = json!({"width": 20, "height": 10, "pngBase64": "AAAA"});
        let noted = note("en", "capture_screen", &json!({"x": 1, "y": 2, "width": 20, "height": 10}), &Ok(result));
        assert_eq!(noted.call, "capture_screen · 1,2 20×10");
        assert_eq!(noted.result, "Screenshot 20×10");
        assert_eq!(noted.image_png.as_deref(), Some("AAAA"));
        assert!(!noted.result.contains("AAAA"));
    }

    #[test]
    fn a_failed_call_uses_the_error_text() {
        let noted = note("en", "capture_screen", &json!({}), &Err(AppError::new("busy")));
        assert_eq!(noted.result, "A run or a recording is in progress. Stop it first.");
        assert!(noted.image_png.is_none());
    }

    #[test]
    fn an_empty_client_name_is_mcp() {
        assert_eq!(client_name("  "), "MCP");
        assert_eq!(client_name("Client\n"), "Client");
    }

    fn sample(id: u64, open: bool, text: &str) -> Dialog {
        Dialog {
            id,
            client: "cursor-vscode".to_string(),
            open,
            entries: vec![DialogEntry { id, role: DialogRole::Call, text: text.to_string(), image_png: None }],
        }
    }

    #[test]
    fn a_closed_dialog_drops_out_of_the_file() {
        let dir = std::env::temp_dir().join(format!("clickit-dialogs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("harness.jsonl");
        let mut dialogs = vec![sample(1, false, "one"), sample(2, false, "two")];
        let removed = take_closed(&mut dialogs, 1).unwrap();
        assert!(removed.is_some());
        write_dialogs(&path, &dialogs).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"session\":1"));
        assert!(text.contains("two"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_open_dialog_stays() {
        let mut dialogs = vec![sample(1, true, "live")];
        let error = take_closed(&mut dialogs, 1).unwrap_err();
        assert_eq!(error.code, "dialogOpen");
        assert_eq!(dialogs.len(), 1);
    }

    #[test]
    fn the_mcp_block_uses_the_path_of_this_copy() {
        let snippet = mcp_snippet(std::path::Path::new(r"C:\ClickIt\clickit-mcp.exe"), true);
        assert!(snippet.found);
        assert_eq!(
            snippet.servers,
            "\"mcpServers\": {\n  \"clickit\": {\n    \"command\": \"C:\\\\ClickIt\\\\clickit-mcp.exe\"\n  }\n}"
        );
        assert_eq!(snippet.entry, "\"clickit\": {\n  \"command\": \"C:\\\\ClickIt\\\\clickit-mcp.exe\"\n}");
    }
}
