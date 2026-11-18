//! The assistant inside the window. It calls the same tools as the local pipe
//! and talks to an OpenAI-compatible HTTP endpoint chosen in the settings.
//! Screenshots stay in memory for the open session; the transcript file keeps text only.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;

use crate::app::Core;
use crate::error::{AppError, AppResult};
use crate::harness::{self, MAX_TOOL_CALLS};

const COMPLETIONS: &str = "/chat/completions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEntry {
    pub id: u64,
    pub role: AgentRole,
    pub text: String,
    /// PNG bytes, base64, for the open session only. Never written to disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_png: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEntry {
    id: u64,
    role: AgentRole,
    text: String,
}

pub struct Session {
    entries: Mutex<Vec<AgentEntry>>,
    /// Messages in the shape the endpoint expects, for this process only.
    messages: Mutex<Vec<Value>>,
    busy: AtomicBool,
    next_id: AtomicU64,
}

impl Session {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
            busy: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn entries(&self) -> Vec<AgentEntry> {
        self.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// Text of the conversation saved so far, without pictures.
pub fn load_transcript(core: &Core) -> Vec<AgentEntry> {
    let Ok(text) = fs::read_to_string(core.store.agent_transcript_path()) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<StoredEntry>(line).ok())
        .map(|entry| AgentEntry { id: entry.id, role: entry.role, text: entry.text, image_png: None })
        .collect()
}

pub fn send(core: &Arc<Core>, text: String) -> AppResult<()> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::new("invalid"));
    }
    let settings = core.settings();
    if settings.agent_endpoint.trim().is_empty() || settings.agent_model.trim().is_empty() {
        return Err(AppError::new("agentUnconfigured"));
    }
    if core.agent.busy.swap(true, Ordering::SeqCst) {
        return Err(AppError::new("agentBusy"));
    }
    let _ = history(core);
    remember(core, AgentRole::User, text.clone(), None);
    let worker = Arc::clone(core);
    let spawned = thread::Builder::new().name("clickit-agent".to_string()).spawn(move || {
        if let Err(error) = turn(&worker, text) {
            remember(&worker, AgentRole::Assistant, crate::shell::text::error_text(&worker.lang(), &error), None);
        }
        worker.agent.busy.store(false, Ordering::SeqCst);
        let _ = worker.app().emit("agent:idle", json!({}));
    });
    if spawned.is_err() {
        core.agent.busy.store(false, Ordering::SeqCst);
        return Err(AppError::new("agentFailed").with("message", "thread"));
    }
    Ok(())
}

fn turn(core: &Core, text: String) -> AppResult<()> {
    {
        let mut messages = core.agent.messages.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if messages.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": "You build ClickIt scenarios by calling tools. Use capture_screen to see the desktop, save_template for images, and save_scenario for the steps. Coordinates in a preview are multiplied by scale and then added to originX and originY. Do not invent template file names."
            }));
            // Earlier turns survive a restart as text, without the pictures.
            let prior = core.agent.entries();
            let prior = prior.split_last().map(|(_last, rest)| rest).unwrap_or(&[]);
            for entry in prior {
                let role = match entry.role {
                    AgentRole::User => "user",
                    AgentRole::Assistant | AgentRole::Tool => "assistant",
                };
                messages.push(json!({"role": role, "content": entry.text}));
            }
        }
        messages.push(json!({"role": "user", "content": text}));
    }
    let mut calls = 0u32;
    loop {
        let reply = complete(core)?;
        let message = reply.get("choices").and_then(|choices| choices.get(0)).and_then(|choice| choice.get("message")).cloned().unwrap_or(Value::Null);
        let spoken = message_text(&message);
        if !spoken.is_empty() {
            remember(core, AgentRole::Assistant, spoken, None);
        }
        let tool_calls = message.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        if tool_calls.is_empty() {
            return Ok(());
        }
        {
            let mut messages = core.agent.messages.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            messages.push(message);
        }
        for call in tool_calls {
            calls += 1;
            if over_tool_budget(calls) {
                remember(core, AgentRole::Assistant, limit_text(core), None);
                return Ok(());
            }
            run_tool(core, &call)?;
        }
    }
}

pub(crate) fn over_tool_budget(calls: u32) -> bool {
    calls > MAX_TOOL_CALLS
}

fn limit_text(core: &Core) -> String {
    crate::shell::text::text(&core.lang(), "agent.limit", &[("count", &MAX_TOOL_CALLS.to_string())])
}

fn run_tool(core: &Core, call: &Value) -> AppResult<()> {
    let id = call.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let function = call.get("function").cloned().unwrap_or(Value::Null);
    let name = function.get("name").and_then(Value::as_str).unwrap_or("").to_string();
    let arguments = function.get("arguments").and_then(Value::as_str).unwrap_or("{}");
    let params: Value = serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
    let result = match harness::call(core, &name, &params) {
        Ok(result) => result,
        Err(error) => json!({"error": error}),
    };
    let image = result.get("pngBase64").and_then(Value::as_str).map(str::to_string);
    let mut shown = result.clone();
    if let Some(object) = shown.as_object_mut() {
        object.remove("pngBase64");
    }
    let summary = serde_json::to_string(&shown).unwrap_or_else(|_| "{}".to_string());
    remember(core, AgentRole::Tool, format!("{name}: {summary}"), image.clone());
    let content = match image {
        Some(png) => json!([
            {"type": "text", "text": summary},
            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{png}")}}
        ]),
        None => Value::String(summary),
    };
    let mut messages = core.agent.messages.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    messages.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
    Ok(())
}

fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn complete(core: &Core) -> AppResult<Value> {
    let settings = core.settings();
    let url = completions_url(&settings.agent_endpoint);
    let messages = core.agent.messages.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let body = json!({
        "model": settings.agent_model.trim(),
        "messages": messages,
        "tools": endpoint_tools(),
        "temperature": 0.2,
    });
    let mut request = ureq::post(&url).timeout(std::time::Duration::from_secs(120));
    let key = core.store.agent_key();
    if !key.is_empty() {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let response = request
        .send_json(body)
        .map_err(|error| AppError::new("agentFailed").with("message", error.to_string()))?;
    response.into_json().map_err(|error| AppError::new("agentFailed").with("message", error.to_string()))
}

fn endpoint_tools() -> Vec<Value> {
    harness::tool_definitions()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool["name"],
                    "description": tool["description"],
                    "parameters": tool["parameters"],
                }
            })
        })
        .collect()
}

pub fn completions_url(endpoint: &str) -> String {
    let endpoint = endpoint.trim().trim_end_matches('/');
    if endpoint.ends_with(COMPLETIONS) {
        endpoint.to_string()
    } else {
        format!("{endpoint}{COMPLETIONS}")
    }
}

fn remember(core: &Core, role: AgentRole, text: String, image_png: Option<String>) {
    let id = core.agent.next_id.fetch_add(1, Ordering::SeqCst);
    let entry = AgentEntry { id, role, text, image_png };
    core.agent.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(entry.clone());
    let stored = StoredEntry { id: entry.id, role: entry.role, text: entry.text.clone() };
    if let Ok(line) = serde_json::to_string(&stored) {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(core.store.agent_transcript_path()) {
            let _ = writeln!(file, "{line}");
        }
    }
    let wire = AgentEntry { image_png: entry.image_png.clone(), ..entry };
    let _ = core.app().emit("agent:entry", &wire);
}

/// Fills the in-memory transcript from disk once, when the window asks for it.
pub fn history(core: &Core) -> Vec<AgentEntry> {
    let mut entries = core.agent.entries.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if entries.is_empty() {
        *entries = load_transcript(core);
        let next = entries.last().map(|entry| entry.id.saturating_add(1)).unwrap_or(1);
        core.agent.next_id.store(next, Ordering::SeqCst);
    }
    entries.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_completions_path_is_added_once() {
        assert_eq!(
            completions_url("http://127.0.0.1:11434/v1"),
            "http://127.0.0.1:11434/v1/chat/completions"
        );
        assert_eq!(
            completions_url("http://127.0.0.1:1234/v1/chat/completions/"),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }

    #[test]
    fn the_tool_budget_stops_at_the_limit() {
        assert!(!over_tool_budget(MAX_TOOL_CALLS));
        assert!(over_tool_budget(MAX_TOOL_CALLS + 1));
    }
}
