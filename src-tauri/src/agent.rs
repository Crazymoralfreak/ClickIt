//! The assistant inside the window. It calls the same tools as the local pipe
//! and talks to an OpenAI-compatible HTTP endpoint chosen in the settings.
//!
//! The conversation is split into chats. Each chat is a file `chats\<id>.jsonl`:
//! a header line with the title and the creation time, then the lines of the
//! conversation. Screenshots stay in memory for the open session; the files keep text only.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::Emitter;

use crate::app::Core;
use crate::error::{AppError, AppResult};
use crate::harness::{self, MAX_TOOL_CALLS};
use crate::model::scenario::new_id;
use crate::storage::valid_id;

const COMPLETIONS: &str = "/chat/completions";
const CHAT_EXTENSION: &str = "jsonl";
/// A chat is named after the start of its first message.
const TITLE_CHARS: usize = 40;
const MAX_TITLE_CHARS: usize = 80;

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

/// One tab of the Assistant page. An empty `title` is shown as "New chat".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatInfo {
    pub id: String,
    pub title: String,
    /// Local `YYYY-MM-DDTHH:MM:SS`.
    pub created: String,
    pub entry_count: usize,
    /// The model is answering in this chat right now.
    pub busy: bool,
}

/// Payload of the `agent:entry` event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EntryEvent {
    chat_id: String,
    entry: AgentEntry,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEntry {
    id: u64,
    role: AgentRole,
    text: String,
}

/// The first line of a chat file.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredHeader {
    title: String,
    created: String,
}

struct Chat {
    id: String,
    title: String,
    created: String,
    entries: Vec<AgentEntry>,
    /// Messages in the shape the endpoint expects, for this process only.
    messages: Vec<Value>,
}

impl Chat {
    fn empty(now: &str) -> Self {
        Self {
            id: new_id("ch"),
            title: String::new(),
            created: now.to_string(),
            entries: Vec::new(),
            messages: Vec::new(),
        }
    }

    fn file_text(&self) -> AppResult<String> {
        let header = StoredHeader { title: self.title.clone(), created: self.created.clone() };
        let mut body = serde_json::to_string(&header)?;
        body.push('\n');
        for entry in &self.entries {
            let stored = StoredEntry { id: entry.id, role: entry.role, text: entry.text.clone() };
            body.push_str(&serde_json::to_string(&stored)?);
            body.push('\n');
        }
        Ok(body)
    }
}

/// What a new line changed.
struct Pushed {
    entry: AgentEntry,
    /// Set when the line gave the chat its title.
    retitled: Option<ChatInfo>,
}

/// The chats of this copy of ClickIt. Works on a folder, so it can be tested without the window.
pub struct Book {
    loaded: bool,
    chats: Vec<Chat>,
    /// The chat the model is answering in. One answer at a time: the model drives the desktop.
    busy: Option<String>,
    next_entry: u64,
}

fn chat_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.{CHAT_EXTENSION}"))
}

/// Writes through a temporary file so that a crash never leaves half a chat.
fn write_chat(dir: &Path, chat: &Chat) -> AppResult<()> {
    fs::create_dir_all(dir)?;
    let path = chat_path(dir, &chat.id);
    let temp = path.with_extension("tmp");
    fs::write(&temp, chat.file_text()?)?;
    fs::rename(&temp, &path)?;
    Ok(())
}

fn parse_entries<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<AgentEntry> {
    lines
        .filter_map(|line| serde_json::from_str::<StoredEntry>(line).ok())
        .map(|entry| AgentEntry { id: entry.id, role: entry.role, text: entry.text, image_png: None })
        .collect()
}

fn read_chat(path: &Path, id: &str) -> Option<Chat> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines().peekable();
    let header = lines.peek().and_then(|line| serde_json::from_str::<StoredHeader>(line).ok());
    if header.is_some() {
        lines.next();
    }
    let entries = parse_entries(lines);
    let (title, created) = match header {
        Some(header) => (clean_title(&header.title), header.created),
        None => (first_title(&entries), String::new()),
    };
    Some(Chat { id: id.to_string(), title, created, entries, messages: Vec::new() })
}

/// One line, without control characters, not longer than a tab can show.
fn clean_title(raw: &str) -> String {
    let flat: String = raw.chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_TITLE_CHARS).collect()
}

/// The start of a message as a chat title.
pub(crate) fn title_from(text: &str) -> String {
    let clean = clean_title(text.lines().find(|line| !line.trim().is_empty()).unwrap_or(""));
    if clean.chars().count() <= TITLE_CHARS {
        return clean;
    }
    let short: String = clean.chars().take(TITLE_CHARS).collect();
    format!("{}…", short.trim_end())
}

fn first_title(entries: &[AgentEntry]) -> String {
    entries.iter().find(|entry| entry.role == AgentRole::User).map(|entry| title_from(&entry.text)).unwrap_or_default()
}

impl Book {
    pub fn new() -> Self {
        Self { loaded: false, chats: Vec::new(), busy: None, next_entry: 1 }
    }

    /// Reads the chats once. The single conversation of an older version, `legacy`, becomes the first chat.
    fn load(&mut self, dir: &Path, legacy: &Path, now: &str) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_none_or(|extension| extension != CHAT_EXTENSION) {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|stem| stem.to_str()).filter(|id| valid_id(id)) else {
                    continue;
                };
                if let Some(chat) = read_chat(&path, id) {
                    self.chats.push(chat);
                }
            }
        }
        self.chats.sort_by(|a, b| (&a.created, &a.id).cmp(&(&b.created, &b.id)));
        self.adopt_legacy(dir, legacy, now);
        let last = self.chats.iter().flat_map(|chat| &chat.entries).map(|entry| entry.id).max().unwrap_or(0);
        self.next_entry = last.saturating_add(1);
        self.keep_one(now);
    }

    fn adopt_legacy(&mut self, dir: &Path, legacy: &Path, now: &str) {
        let Ok(text) = fs::read_to_string(legacy) else {
            return;
        };
        let entries = parse_entries(text.lines());
        if entries.is_empty() {
            let _ = fs::remove_file(legacy);
            return;
        }
        let mut chat = Chat::empty(now);
        chat.title = first_title(&entries);
        chat.entries = entries;
        // The old file goes away only after the chat is safely on the disk.
        if write_chat(dir, &chat).is_ok() {
            let _ = fs::remove_file(legacy);
        }
        self.chats.insert(0, chat);
    }

    /// The page always has a chat to type into.
    fn keep_one(&mut self, now: &str) {
        if self.chats.is_empty() {
            self.chats.push(Chat::empty(now));
        }
    }

    fn info(&self, chat: &Chat) -> ChatInfo {
        ChatInfo {
            id: chat.id.clone(),
            title: chat.title.clone(),
            created: chat.created.clone(),
            entry_count: chat.entries.len(),
            busy: self.busy.as_deref() == Some(chat.id.as_str()),
        }
    }

    fn infos(&self) -> Vec<ChatInfo> {
        self.chats.iter().map(|chat| self.info(chat)).collect()
    }

    fn find(&self, id: &str) -> AppResult<&Chat> {
        self.chats.iter().find(|chat| chat.id == id).ok_or_else(|| AppError::new("chatMissing"))
    }

    fn find_mut(&mut self, id: &str) -> AppResult<&mut Chat> {
        self.chats.iter_mut().find(|chat| chat.id == id).ok_or_else(|| AppError::new("chatMissing"))
    }

    /// A new chat at the end. A chat that is still empty is handed out again instead of a second one.
    fn create(&mut self, now: &str) -> ChatInfo {
        if let Some(chat) = self.chats.iter().find(|chat| chat.entries.is_empty()) {
            return self.info(chat);
        }
        self.chats.push(Chat::empty(now));
        let chat = self.chats.last().expect("a chat was just added");
        self.info(chat)
    }

    /// Removes the chat and its file. The chat the model is answering in stays.
    fn delete(&mut self, dir: &Path, id: &str, now: &str) -> AppResult<()> {
        if self.busy.as_deref() == Some(id) {
            return Err(AppError::new("chatBusy"));
        }
        let Some(index) = self.chats.iter().position(|chat| chat.id == id) else {
            return Ok(());
        };
        let path = chat_path(dir, id);
        if path.exists() {
            fs::remove_file(path)?;
        }
        self.chats.remove(index);
        self.keep_one(now);
        Ok(())
    }

    /// Removes every chat. Refused while the model is answering.
    fn clear(&mut self, dir: &Path, now: &str) -> AppResult<()> {
        if self.busy.is_some() {
            return Err(AppError::new("chatBusy"));
        }
        for chat in &self.chats {
            let path = chat_path(dir, &chat.id);
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        self.chats.clear();
        self.keep_one(now);
        Ok(())
    }

    fn rename(&mut self, dir: &Path, id: &str, title: &str) -> AppResult<ChatInfo> {
        let title = clean_title(title);
        if title.is_empty() {
            return Err(AppError::new("nameEmpty"));
        }
        let chat = self.find_mut(id)?;
        let previous = std::mem::replace(&mut chat.title, title);
        // A chat without lines is not on the disk yet; its title is written with the first line.
        if !chat.entries.is_empty() {
            if let Err(error) = write_chat(dir, chat) {
                chat.title = previous;
                return Err(error);
            }
        }
        let chat = self.find(id)?;
        Ok(self.info(chat))
    }

    fn entries(&self, id: &str) -> Vec<AgentEntry> {
        self.find(id).map(|chat| chat.entries.clone()).unwrap_or_default()
    }

    /// Adds a line to the chat and to its file. The first message names an untitled chat.
    fn push(&mut self, dir: &Path, id: &str, role: AgentRole, text: String, image_png: Option<String>) -> Option<Pushed> {
        let entry_id = self.next_entry;
        self.next_entry = self.next_entry.saturating_add(1);
        let entry = AgentEntry { id: entry_id, role, text, image_png };
        let chat = self.chats.iter_mut().find(|chat| chat.id == id)?;
        let first = chat.entries.is_empty();
        let named = chat.title.is_empty() && role == AgentRole::User;
        if named {
            chat.title = title_from(&entry.text);
        }
        chat.entries.push(entry.clone());
        if first || named {
            let _ = write_chat(dir, chat);
        } else {
            let stored = StoredEntry { id: entry.id, role: entry.role, text: entry.text.clone() };
            if let Ok(line) = serde_json::to_string(&stored) {
                if let Ok(mut file) = OpenOptions::new().append(true).open(chat_path(dir, id)) {
                    let _ = writeln!(file, "{line}");
                }
            }
        }
        let retitled = if named { self.find(id).ok().map(|chat| self.info(chat)) } else { None };
        Some(Pushed { entry, retitled })
    }
}

impl Default for Book {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Session {
    book: Mutex<Book>,
}

impl Session {
    pub fn new() -> Self {
        Self { book: Mutex::new(Book::new()) }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

fn now_text() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// The chats, read from the disk on first use.
fn book(core: &Core) -> MutexGuard<'_, Book> {
    let mut book = core.agent.book.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    book.load(&core.store.chats_dir(), &core.store.agent_transcript_path(), &now_text());
    book
}

pub fn chats(core: &Core) -> Vec<ChatInfo> {
    book(core).infos()
}

pub fn chat_create(core: &Core) -> ChatInfo {
    book(core).create(&now_text())
}

/// Deletes one chat and returns what is left; an empty new chat when it was the last one.
pub fn chat_delete(core: &Core, id: &str) -> AppResult<Vec<ChatInfo>> {
    let mut book = book(core);
    book.delete(&core.store.chats_dir(), id, &now_text())?;
    Ok(book.infos())
}

pub fn chat_rename(core: &Core, id: &str, title: &str) -> AppResult<ChatInfo> {
    book(core).rename(&core.store.chats_dir(), id, title)
}

/// Removes every chat and its file. Refused while the model is answering.
pub fn clear_chats(core: &Core) -> AppResult<()> {
    book(core).clear(&core.store.chats_dir(), &now_text())
}

pub fn is_busy(core: &Core) -> bool {
    book(core).busy.is_some()
}

pub fn history(core: &Core, chat_id: &str) -> Vec<AgentEntry> {
    book(core).entries(chat_id)
}

pub fn send(core: &Arc<Core>, chat_id: String, text: String) -> AppResult<()> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(AppError::new("invalid"));
    }
    let settings = core.settings();
    if settings.agent_endpoint.trim().is_empty() || settings.agent_model.trim().is_empty() {
        return Err(AppError::new("agentUnconfigured"));
    }
    {
        let mut book = book(core);
        book.find(&chat_id)?;
        if book.busy.is_some() {
            return Err(AppError::new("agentBusy"));
        }
        book.busy = Some(chat_id.clone());
    }
    remember(core, &chat_id, AgentRole::User, text.clone(), None);
    let worker = Arc::clone(core);
    let chat = chat_id.clone();
    let spawned = thread::Builder::new().name("clickit-agent".to_string()).spawn(move || {
        if let Err(error) = turn(&worker, &chat, text) {
            let shown = crate::shell::text::error_text(&worker.lang(), &error);
            remember(&worker, &chat, AgentRole::Assistant, shown, None);
        }
        book(&worker).busy = None;
        let _ = worker.app().emit("agent:idle", json!({"chatId": chat}));
    });
    if spawned.is_err() {
        book(core).busy = None;
        return Err(AppError::new("agentFailed").with("message", "thread"));
    }
    Ok(())
}

/// What the model is told before the first message of a chat.
const SYSTEM_PROMPT: &str = "You build ClickIt scenarios by calling tools. Use capture_screen to see the desktop, save_template for images, and save_scenario to write the scenario. Coordinates in a preview are multiplied by scale and then added to originX and originY. Do not invent template file names.\n\nA scenario is stored either as a list of steps or as a canvas graph of blocks; get_scenario tells which in format. Edit a scenario in the format it is stored in. Create a new one with format canvas unless the user asks for a list. convert_scenario turns a list into a graph.\n\nIn a graph every output of a block has at most one link and a block may have any number of incoming links. An output without a link ends the pass. Loops are links back to an earlier block; a counter block sends the pass into body the set number of times and then into done. A branch block leaves through yes or no, a wait block through next or timeout. An intercept block has no input and watches its condition during the whole run. A call block runs one pass of another scenario. A command block starts a program or a script and leaves through next or error; add one only when the user asks for it: ClickIt shows the command to the user and waits for an answer before it is saved and before it is run. Leave x and y out of new blocks, they are laid out for you. Give every block a short title and group related blocks into frames by adding a note of kind frame with around set to the ids of its blocks.\n\nAfter saving, use check_condition to see whether a wait or a branch finds its image right now, and get_trace after a run to see which way the pass went and where it stopped.";

fn turn(core: &Core, chat_id: &str, text: String) -> AppResult<()> {
    {
        let mut book = book(core);
        let chat = book.find_mut(chat_id)?;
        if chat.messages.is_empty() {
            chat.messages.push(json!({
                "role": "system",
                "content": SYSTEM_PROMPT
            }));
            // Earlier turns survive a restart as text, without the pictures.
            let prior = chat.entries.split_last().map(|(_last, rest)| rest).unwrap_or(&[]);
            let prior: Vec<Value> = prior
                .iter()
                .map(|entry| {
                    let role = match entry.role {
                        AgentRole::User => "user",
                        AgentRole::Assistant | AgentRole::Tool => "assistant",
                    };
                    json!({"role": role, "content": entry.text})
                })
                .collect();
            chat.messages.extend(prior);
        }
        chat.messages.push(json!({"role": "user", "content": text}));
    }
    let mut calls = 0u32;
    loop {
        let reply = complete(core, chat_id)?;
        let message = reply.get("choices").and_then(|choices| choices.get(0)).and_then(|choice| choice.get("message")).cloned().unwrap_or(Value::Null);
        let spoken = message_text(&message);
        if !spoken.is_empty() {
            remember(core, chat_id, AgentRole::Assistant, spoken, None);
        }
        let tool_calls = message.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        if tool_calls.is_empty() {
            return Ok(());
        }
        book(core).find_mut(chat_id)?.messages.push(message);
        for call in tool_calls {
            calls += 1;
            if over_tool_budget(calls) {
                remember(core, chat_id, AgentRole::Assistant, limit_text(core), None);
                return Ok(());
            }
            run_tool(core, chat_id, &call)?;
        }
    }
}

pub(crate) fn over_tool_budget(calls: u32) -> bool {
    calls > MAX_TOOL_CALLS
}

fn limit_text(core: &Core) -> String {
    crate::shell::text::text(&core.lang(), "agent.limit", &[("count", &MAX_TOOL_CALLS.to_string())])
}

fn run_tool(core: &Core, chat_id: &str, call: &Value) -> AppResult<()> {
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
    remember(core, chat_id, AgentRole::Tool, format!("{name}: {summary}"), image.clone());
    let content = match image {
        Some(png) => json!([
            {"type": "text", "text": summary},
            {"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{png}")}}
        ]),
        None => Value::String(summary),
    };
    book(core).find_mut(chat_id)?.messages.push(json!({"role": "tool", "tool_call_id": id, "content": content}));
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

fn complete(core: &Core, chat_id: &str) -> AppResult<Value> {
    let settings = core.settings();
    let url = completions_url(&settings.agent_endpoint);
    let messages = book(core).find(chat_id)?.messages.clone();
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

/// Adds a line to the chat, to its file and to the window.
fn remember(core: &Core, chat_id: &str, role: AgentRole, text: String, image_png: Option<String>) {
    let pushed = book(core).push(&core.store.chats_dir(), chat_id, role, text, image_png);
    let Some(pushed) = pushed else {
        return;
    };
    if let Some(info) = &pushed.retitled {
        let _ = core.app().emit("agent:chat", info);
    }
    let _ = core.app().emit("agent:entry", &EntryEvent { chat_id: chat_id.to_string(), entry: pushed.entry });
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-11-18T19:00:00";

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clickit-chats-{}", new_id("t")));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn loaded(root: &Path) -> Book {
        let mut book = Book::new();
        book.load(&root.join("chats"), &root.join("agent.jsonl"), NOW);
        book
    }

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

    #[test]
    fn a_long_first_message_is_cut_for_the_title() {
        assert_eq!(title_from("  Собери сценарий\nдля арены"), "Собери сценарий");
        let long = "a".repeat(60);
        let title = title_from(&long);
        assert_eq!(title.chars().count(), TITLE_CHARS + 1);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn the_old_conversation_becomes_the_first_chat() {
        let root = temp_dir();
        fs::write(
            root.join("agent.jsonl"),
            "{\"id\":1,\"role\":\"user\",\"text\":\"Собери арену\"}\n{\"id\":2,\"role\":\"assistant\",\"text\":\"Готово\"}\n",
        )
        .unwrap();
        let book = loaded(&root);
        let chats = book.infos();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].title, "Собери арену");
        assert_eq!(chats[0].entry_count, 2);
        assert!(!root.join("agent.jsonl").exists());
        assert!(chat_path(&root.join("chats"), &chats[0].id).is_file());

        // A second start reads the same chat from its own file.
        let again = loaded(&root);
        assert_eq!(again.infos(), chats);
        assert_eq!(again.next_entry, 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_empty_chat_is_not_written_until_its_first_message() {
        let root = temp_dir();
        let dir = root.join("chats");
        let mut book = loaded(&root);
        let chat = book.infos().remove(0);
        assert_eq!(chat.entry_count, 0);
        assert!(!chat_path(&dir, &chat.id).exists());
        // Asking for a new chat while one is still empty gives the same one.
        assert_eq!(book.create(NOW).id, chat.id);

        let pushed = book.push(&dir, &chat.id, AgentRole::User, "Привет".to_string(), None).unwrap();
        assert_eq!(pushed.retitled.map(|info| info.title).as_deref(), Some("Привет"));
        assert!(chat_path(&dir, &chat.id).is_file());
        book.push(&dir, &chat.id, AgentRole::Assistant, "Здравствуйте".to_string(), None).unwrap();

        let second = book.create(NOW);
        assert_ne!(second.id, chat.id);
        assert_eq!(book.infos().len(), 2);

        let again = loaded(&root);
        assert_eq!(again.infos().len(), 1);
        assert_eq!(again.entries(&chat.id).len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_renamed_chat_keeps_its_name_after_a_restart() {
        let root = temp_dir();
        let dir = root.join("chats");
        let mut book = loaded(&root);
        let chat = book.infos().remove(0);
        book.push(&dir, &chat.id, AgentRole::User, "Первое сообщение".to_string(), None).unwrap();
        assert_eq!(book.rename(&dir, &chat.id, "  Арена \n").unwrap().title, "Арена");
        assert_eq!(book.rename(&dir, &chat.id, "   ").unwrap_err().code, "nameEmpty");
        // The next message does not rename the chat again.
        let pushed = book.push(&dir, &chat.id, AgentRole::User, "Второе".to_string(), None).unwrap();
        assert!(pushed.retitled.is_none());

        let again = loaded(&root);
        assert_eq!(again.infos()[0].title, "Арена");
        assert_eq!(again.entries(&chat.id).len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_the_last_chat_leaves_an_empty_one() {
        let root = temp_dir();
        let dir = root.join("chats");
        let mut book = loaded(&root);
        let chat = book.infos().remove(0);
        book.push(&dir, &chat.id, AgentRole::User, "Привет".to_string(), None).unwrap();
        book.delete(&dir, &chat.id, NOW).unwrap();
        assert!(!chat_path(&dir, &chat.id).exists());
        let left = book.infos();
        assert_eq!(left.len(), 1);
        assert_ne!(left[0].id, chat.id);
        assert_eq!(left[0].entry_count, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_chat_with_an_answer_in_progress_cannot_be_deleted() {
        let root = temp_dir();
        let dir = root.join("chats");
        let mut book = loaded(&root);
        let chat = book.infos().remove(0);
        book.push(&dir, &chat.id, AgentRole::User, "Привет".to_string(), None).unwrap();
        book.busy = Some(chat.id.clone());
        assert!(book.infos()[0].busy);
        assert_eq!(book.delete(&dir, &chat.id, NOW).unwrap_err().code, "chatBusy");
        assert_eq!(book.clear(&dir, NOW).unwrap_err().code, "chatBusy");
        assert!(chat_path(&dir, &chat.id).is_file());

        book.busy = None;
        book.clear(&dir, NOW).unwrap();
        assert!(!chat_path(&dir, &chat.id).exists());
        assert_eq!(book.infos().len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
