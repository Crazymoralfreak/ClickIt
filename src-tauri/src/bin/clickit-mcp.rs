//! MCP server for a harness. It has no model of its own:
//! it forwards tool calls to the running ClickIt over a local named pipe.
//! ClickIt must be running with harness connections turned on.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt};
use serde_json::{json, Value};

const PIPE_NAME: &str = r"\\.\pipe\clickit-harness";
const OFF: &str = "ClickIt is not running, or harness connections are turned off in its settings.";

struct ClickItMcp {
    pipe: Mutex<Option<Pipe>>,
}

struct Pipe {
    file: File,
    next_id: u64,
}

impl ServerHandler for ClickItMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        let tools = clickit_lib::harness::tool_definitions()
            .into_iter()
            .filter_map(|tool| {
                let name = tool.get("name")?.as_str()?.to_string();
                let description = tool.get("description").and_then(Value::as_str).unwrap_or("").to_string();
                let parameters = tool.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"}));
                Some(Tool::new(name, description, rmcp::model::object(parameters)))
            })
            .collect();
        Ok(ListToolsResult { tools, ..Default::default() })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
        let params = request.arguments.clone().map(Value::Object).unwrap_or_else(|| json!({}));
        match forward(&self.pipe, &request.name, &params, &client_label(&context)) {
            Ok(result) => Ok(CallToolResult::success(vec![ContentBlock::text(result.to_string())]).into()),
            Err(message) => Ok(CallToolResult::error(vec![ContentBlock::text(message)]).into()),
        }
    }
}

fn forward(slot: &Mutex<Option<Pipe>>, method: &str, params: &Value, client: &str) -> Result<Value, String> {
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.is_none() {
        *guard = Some(connect()?);
    }
    let pipe = guard.as_mut().ok_or_else(|| OFF.to_string())?;
    match exchange(pipe, method, params, client) {
        Ok(value) => Ok(value),
        Err(message) => {
            *guard = None;
            Err(message)
        }
    }
}

fn connect() -> Result<Pipe, String> {
    let token = token().ok_or_else(|| OFF.to_string())?;
    let mut file = File::options()
        .read(true)
        .write(true)
        .open(PIPE_NAME)
        .map_err(|_| OFF.to_string())?;
    writeln!(file, "{token}").map_err(|_| OFF.to_string())?;
    file.flush().map_err(|_| OFF.to_string())?;
    Ok(Pipe { file, next_id: 1 })
}

fn exchange(pipe: &mut Pipe, method: &str, params: &Value, client: &str) -> Result<Value, String> {
    let id = pipe.next_id;
    pipe.next_id += 1;
    let line = serde_json::to_string(&json!({"id": id, "method": method, "params": params, "client": client}))
        .map_err(|error| error.to_string())?;
    writeln!(pipe.file, "{line}").map_err(|_| OFF.to_string())?;
    pipe.file.flush().map_err(|_| OFF.to_string())?;
    let mut reader = BufReader::new(&pipe.file);
    let mut response = String::new();
    reader.read_line(&mut response).map_err(|_| OFF.to_string())?;
    if response.is_empty() {
        return Err(OFF.to_string());
    }
    let value: Value = serde_json::from_str(&response).map_err(|error| error.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(error.to_string());
    }
    value.get("result").cloned().ok_or_else(|| "ClickIt returned an empty response.".to_string())
}

fn client_label(context: &RequestContext<RoleServer>) -> String {
    context
        .peer
        .peer_info()
        .map(|info| {
            info.client_info
                .title
                .clone()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| info.client_info.name.clone())
        })
        .unwrap_or_default()
}

fn token() -> Option<String> {
    let root = std::env::var_os("APPDATA")?;
    let path = PathBuf::from(root).join("ClickIt").join("harness.token");
    let text = fs::read_to_string(path).ok()?;
    let token = text.trim();
    if token.len() >= 32 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(token.to_string())
    } else {
        None
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = ClickItMcp { pipe: Mutex::new(None) }.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
