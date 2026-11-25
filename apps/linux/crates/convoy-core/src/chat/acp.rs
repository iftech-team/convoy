//! Any agent that speaks the Agent Client Protocol (ACP): Antigravity
//! (`agy acp`), opencode (`opencode acp`, which reaches DeepSeek and the
//! rest), Gemini CLI and whatever else the user adds.
//!
//! ACP is JSON-RPC 2.0, one message per line on stdio. The order is fixed:
//! `initialize`, then `session/new` (or `session/load` to reopen one), then a
//! `session/prompt` per user turn; the turn ends when that prompt's response
//! arrives. While it runs the agent streams `session/update` notifications
//! and may ask `session/request_permission`. As with [`super::codex`], the
//! protocol is kept apart from the I/O so it can be replayed in a test.
//!
//! Convoy offers the agent no file system and no terminal of its own, so the
//! agent uses its own tools; requests for them are refused.

use super::event::{
    clip_head, clip_tail, one_line, replacement_diff, added_diff, short_path, ApprovalKind,
    ChatEvent, ChatModel, Decision, Image, ToolKind, Usage, DETAIL_LIMIT, OUTPUT_LIMIT,
};
use crate::{bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

/// The protocol version Convoy speaks.
pub const PROTOCOL_VERSION: i64 = 1;

const INITIALIZE: i64 = 1;
const SESSION: i64 = 2;
/// Prompts, model switches and the rest count up from here.
const FIRST_REQUEST: i64 = 10;

/// How the session is opened.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The agent's name as the user knows it, for messages.
    pub name: String,
    pub cwd: String,
    /// The model to switch to once the session is open; none keeps the
    /// agent's default.
    pub model: Option<String>,
    /// The session to reopen; none starts a new one.
    pub resume: Option<String>,
    /// What to tell the user to run when the agent wants a sign-in.
    pub login_command: Option<String>,
    /// MCP servers, already in ACP's shape (`{name, command, args, env}`).
    pub mcp_servers: Vec<Value>,
    /// Instructions on top of the agent's own. ACP has no system prompt, so
    /// they go ahead of the first user turn.
    pub instructions: Option<String>,
}

/// How the window picks the model: ACP has two ways, the older `models`
/// field and the newer config options.
#[derive(Debug, Clone, PartialEq)]
enum ModelSwitch {
    None,
    SetModel,
    ConfigOption(String),
}

/// A permission question waiting for the user.
#[derive(Debug, Clone)]
struct Pending {
    id: Value,
    options: Vec<(String, String)>,
}

/// What the last streamed chunk belonged to, so consecutive chunks join one
/// message and anything in between starts a new one.
#[derive(Debug, Clone, PartialEq)]
enum Run {
    None,
    Text(String),
    Thinking(String),
}

/// A tool call as last reported, since updates carry only what changed.
#[derive(Debug, Clone, Default)]
struct Tool {
    kind: Option<String>,
    title: String,
    detail: String,
}

/// The client side of one ACP conversation.
#[derive(Debug)]
pub struct AcpClient {
    options: Options,
    next_id: i64,
    ready: bool,
    can_load: bool,
    takes_images: bool,
    /// Whether the agent takes MCP servers over HTTP, the only kind Convoy
    /// hands out.
    takes_http_mcp: bool,
    session: Option<String>,
    /// While `session/load` runs the agent replays the old conversation;
    /// Convoy keeps its own record of it, so the replay is not shown again.
    loading: bool,
    /// The id of the `session/prompt` that is running.
    prompt: Option<i64>,
    in_turn: bool,
    /// Turns typed before the session existed or while one ran.
    queued: Vec<Value>,
    pending: HashMap<String, Pending>,
    tools: HashMap<String, Tool>,
    run: Run,
    outbox: Vec<String>,
    models: Vec<ChatModel>,
    switch: ModelSwitch,
    instructions_sent: bool,
}

impl AcpClient {
    pub fn new(options: Options) -> Self {
        AcpClient {
            options,
            next_id: FIRST_REQUEST,
            ready: false,
            can_load: false,
            takes_images: false,
            takes_http_mcp: false,
            session: None,
            loading: false,
            prompt: None,
            in_turn: false,
            queued: Vec::new(),
            pending: HashMap::new(),
            tools: HashMap::new(),
            run: Run::None,
            outbox: Vec::new(),
            models: Vec::new(),
            switch: ModelSwitch::None,
            instructions_sent: false,
        }
    }

    /// The first line to send.
    pub fn start(&mut self) -> Vec<String> {
        vec![json!({
            "jsonrpc": "2.0",
            "id": INITIALIZE,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false
                },
                "clientInfo": { "name": "convoy", "version": env!("CARGO_PKG_VERSION") }
            }
        })
        .to_string()]
    }

    /// The session id once the agent has given one.
    pub fn session_id(&self) -> Option<&str> {
        self.session.as_deref()
    }

    /// Whether the session is open and turns can start.
    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn in_turn(&self) -> bool {
        self.in_turn
    }

    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    fn request(&mut self, method: &str, params: Value) -> (i64, String) {
        self.next_id += 1;
        let id = self.next_id;
        (
            id,
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string(),
        )
    }

    fn session_request(&mut self) -> String {
        let servers: Vec<&Value> = self
            .options
            .mcp_servers
            .iter()
            .filter(|server| {
                server.get("type").and_then(Value::as_str) != Some("http") || self.takes_http_mcp
            })
            .collect();
        let mut params = json!({ "cwd": self.options.cwd, "mcpServers": servers });
        let method = match self.options.resume.as_deref().filter(|_| self.can_load) {
            Some(session) => {
                params["sessionId"] = json!(session);
                self.loading = true;
                "session/load"
            }
            None => "session/new",
        };
        json!({ "jsonrpc": "2.0", "id": SESSION, "method": method, "params": params }).to_string()
    }

    /// The prompt blocks of one user turn.
    fn blocks(&mut self, text: &str, images: &[Image]) -> (Value, Vec<ChatEvent>) {
        let mut blocks = Vec::new();
        let mut events = Vec::new();
        if !self.instructions_sent {
            self.instructions_sent = true;
            if let Some(instructions) = self.options.instructions.as_deref().filter(|t| !t.is_empty())
            {
                blocks.push(json!({ "type": "text", "text": instructions }));
            }
        }
        if !images.is_empty() {
            if self.takes_images {
                for image in images {
                    blocks.push(json!({ "type": "image", "mimeType": image.media_type, "data": image.data }));
                }
            } else {
                events.push(ChatEvent::Error {
                    message: format!("{} does not take images; they were left out.", self.name()),
                });
            }
        }
        if !text.is_empty() || blocks.is_empty() {
            blocks.push(json!({ "type": "text", "text": text }));
        }
        (Value::Array(blocks), events)
    }

    fn prompt_request(&mut self, blocks: Value) -> String {
        let session = self.session.clone().unwrap_or_default();
        let (id, line) = self.request("session/prompt", json!({ "sessionId": session, "prompt": blocks }));
        self.prompt = Some(id);
        self.run = Run::None;
        line
    }

    fn name(&self) -> &str {
        if self.options.name.is_empty() {
            "The agent"
        } else {
            &self.options.name
        }
    }

    /// A user turn. ACP runs one prompt at a time, so a turn typed while
    /// another runs waits for it.
    pub fn send(&mut self, text: &str, images: &[Image]) -> (Vec<String>, Vec<ChatEvent>) {
        let (blocks, mut events) = self.blocks(text, images);
        if !self.in_turn {
            self.in_turn = true;
            events.push(ChatEvent::TurnStarted);
        }
        if self.ready && self.prompt.is_none() {
            (vec![self.prompt_request(blocks)], events)
        } else {
            self.queued.push(blocks);
            (Vec::new(), events)
        }
    }

    /// Stops the running turn: `session/cancel`, and every open question
    /// answered as cancelled, as the protocol asks.
    pub fn interrupt(&mut self) -> Vec<String> {
        let Some(session) = self.session.clone() else {
            return Vec::new();
        };
        if self.prompt.is_none() {
            return Vec::new();
        }
        self.queued.clear();
        let mut lines = vec![json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session }
        })
        .to_string()];
        for (_, pending) in self.pending.drain() {
            lines.push(
                json!({ "jsonrpc": "2.0", "id": pending.id, "result": { "outcome": { "outcome": "cancelled" } } })
                    .to_string(),
            );
        }
        lines
    }

    /// Answers a permission question with the option that fits best.
    pub fn answer(&mut self, request: &str, decision: Decision) -> Result<(String, ChatEvent)> {
        let Some(pending) = self.pending.remove(request) else {
            bail!("This request was already answered.")
        };
        let wanted: &[&str] = match decision {
            Decision::Allow => &["allow_once", "allow_always"],
            Decision::Always => &["allow_always", "allow_once"],
            Decision::Deny => &["reject_once", "reject_always"],
        };
        let chosen = wanted.iter().find_map(|kind| {
            pending
                .options
                .iter()
                .find(|(_, option_kind)| option_kind == kind)
                .map(|(id, _)| id.clone())
        });
        let outcome = match chosen {
            Some(option) => json!({ "outcome": "selected", "optionId": option }),
            None => json!({ "outcome": "cancelled" }),
        };
        Ok((
            json!({ "jsonrpc": "2.0", "id": pending.id, "result": { "outcome": outcome } }).to_string(),
            ChatEvent::ApprovalResolved {
                request: request.to_string(),
            },
        ))
    }

    /// Switches the model; none leaves the current one, since ACP has no way
    /// back to "the default".
    pub fn set_model(&mut self, model: Option<&str>) -> Vec<String> {
        let Some(model) = model.filter(|m| !m.is_empty()) else {
            return Vec::new();
        };
        self.options.model = Some(model.to_string());
        if !self.ready {
            return Vec::new();
        }
        self.model_request(model).into_iter().collect()
    }

    fn model_request(&mut self, model: &str) -> Option<String> {
        let session = self.session.clone()?;
        match self.switch.clone() {
            ModelSwitch::None => None,
            ModelSwitch::SetModel => Some(
                self.request("session/set_model", json!({ "sessionId": session, "modelId": model }))
                    .1,
            ),
            ModelSwitch::ConfigOption(config) => Some(
                self.request(
                    "session/set_config_option",
                    json!({ "sessionId": session, "configId": config, "value": model }),
                )
                .1,
            ),
        }
    }

    /// One line from the agent.
    pub fn feed(&mut self, line: &str) -> Vec<ChatEvent> {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let method = message.get("method").and_then(Value::as_str);
        match (message.get("id"), method) {
            (Some(id), Some(method)) => self.agent_request(id, method, &message, &mut events),
            (Some(id), None) => self.response(id, &message, &mut events),
            (None, Some("session/update")) => {
                if let Some(update) = message.pointer("/params/update") {
                    self.update(update, &mut events);
                }
            }
            _ => {}
        }
        events
    }

    fn response(&mut self, id: &Value, message: &Value, events: &mut Vec<ChatEvent>) {
        let id = id.as_i64();
        if id == Some(SESSION) {
            self.loading = false;
        }
        if let Some(error) = message.get("error") {
            let text = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the agent refused the request")
                .to_string();
            let code = error.get("code").and_then(Value::as_i64);
            let message = match id {
                Some(SESSION) if code == Some(-32000) || text.to_lowercase().contains("auth") => {
                    // The agent's own words come first: "sign in" is the
                    // usual reason, but not the only one.
                    match &self.options.login_command {
                        Some(login) => format!(
                            "{} could not start: {text} If it needs a sign-in, run `{login}` (Settings → Agents → Sign in…), then try again.",
                            self.name()
                        ),
                        None => format!("{} could not start: {text}", self.name()),
                    }
                }
                Some(SESSION) if self.options.resume.is_some() && self.can_load => {
                    format!("{} could not reopen this conversation: {text}", self.name())
                }
                _ => text.clone(),
            };
            events.push(ChatEvent::Error { message });
            if id.is_some() && id == self.prompt {
                self.prompt = None;
                self.end_turn(Usage::default(), Some(text), events);
            } else if id == Some(SESSION) && self.in_turn {
                self.queued.clear();
                self.in_turn = false;
                events.push(ChatEvent::TurnEnded { usage: Usage::default(), error: None });
            }
            return;
        }
        let result = message.get("result").unwrap_or(&Value::Null);
        match id {
            Some(INITIALIZE) => {
                let capabilities = result.get("agentCapabilities").unwrap_or(&Value::Null);
                self.can_load = capabilities.get("loadSession").and_then(Value::as_bool) == Some(true);
                self.takes_images = capabilities
                    .pointer("/promptCapabilities/image")
                    .and_then(Value::as_bool)
                    == Some(true);
                self.takes_http_mcp = capabilities
                    .pointer("/mcpCapabilities/http")
                    .and_then(Value::as_bool)
                    == Some(true);
                if self.options.resume.is_some() && !self.can_load {
                    events.push(ChatEvent::Error {
                        message: format!(
                            "{} cannot reopen old conversations, so this is a new one.",
                            self.name()
                        ),
                    });
                }
                let line = self.session_request();
                self.outbox.push(line);
            }
            Some(SESSION) => {
                let session = result
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| self.options.resume.clone().filter(|_| self.can_load));
                let Some(session) = session else {
                    events.push(ChatEvent::Error {
                        message: format!("{} opened no session.", self.name()),
                    });
                    return;
                };
                self.session = Some(session.clone());
                self.ready = true;
                events.push(ChatEvent::Session { provider_id: session });
                self.read_models(result, events);
                if let Some(model) = self.options.model.clone() {
                    let current = self.models.iter().any(|m| m.is_default && m.id == model);
                    if !current {
                        if let Some(line) = self.model_request(&model) {
                            self.outbox.push(line);
                        }
                    }
                }
                self.next_queued();
            }
            Some(_) if id == self.prompt => {
                self.prompt = None;
                let reason = result.get("stopReason").and_then(Value::as_str).unwrap_or("end_turn");
                let error = match reason {
                    "end_turn" | "cancelled" => None,
                    "refusal" => Some(format!("{} refused to continue.", self.name())),
                    "max_tokens" => Some("The model reached its output limit.".to_string()),
                    "max_turn_requests" => Some("The turn reached its step limit.".to_string()),
                    other => Some(format!("The turn stopped: {other}.")),
                };
                let usage = usage(result.get("usage").unwrap_or(&Value::Null));
                if reason == "cancelled" {
                    self.queued.clear();
                }
                if self.queued.is_empty() {
                    self.end_turn(usage, error, events);
                } else {
                    if let Some(error) = error {
                        events.push(ChatEvent::Error { message: error });
                    }
                    self.next_queued();
                }
            }
            _ => {}
        }
    }

    fn end_turn(&mut self, usage: Usage, error: Option<String>, events: &mut Vec<ChatEvent>) {
        self.in_turn = false;
        self.run = Run::None;
        self.tools.clear();
        events.push(ChatEvent::TurnEnded { usage, error });
    }

    /// Sends the next waiting turn, if any and if nothing runs.
    fn next_queued(&mut self) {
        if self.prompt.is_some() || self.queued.is_empty() {
            return;
        }
        let blocks = self.queued.remove(0);
        let line = self.prompt_request(blocks);
        self.outbox.push(line);
    }

    /// The models a `session/new` or `session/load` result offers.
    fn read_models(&mut self, result: &Value, events: &mut Vec<ChatEvent>) {
        let mut current = None;
        if let Some(models) = result.get("models") {
            current = models.get("currentModelId").and_then(Value::as_str).map(str::to_string);
            self.models = models
                .get("availableModels")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let id = entry.get("modelId").and_then(Value::as_str)?;
                    let name = entry.get("name").and_then(Value::as_str).unwrap_or(id);
                    Some(chat_model(id, name, current.as_deref() == Some(id)))
                })
                .collect();
            if !self.models.is_empty() {
                self.switch = ModelSwitch::SetModel;
            }
        }
        if self.models.is_empty() {
            let option = result
                .get("configOptions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|option| option.get("category").and_then(Value::as_str) == Some("model"));
            if let Some(option) = option {
                current = option.get("currentValue").and_then(Value::as_str).map(str::to_string);
                self.models = option
                    .get("options")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| {
                        let id = entry.get("value").and_then(Value::as_str)?;
                        let name = entry.get("name").and_then(Value::as_str).unwrap_or(id);
                        Some(chat_model(id, name, current.as_deref() == Some(id)))
                    })
                    .collect();
                if let (false, Some(config)) =
                    (self.models.is_empty(), option.get("id").and_then(Value::as_str))
                {
                    self.switch = ModelSwitch::ConfigOption(config.to_string());
                }
            }
        }
        if !self.models.is_empty() {
            events.push(ChatEvent::Models {
                models: self.models.clone(),
                model: self.options.model.clone().or(current),
                effort: None,
            });
        }
    }

    fn update(&mut self, update: &Value, events: &mut Vec<ChatEvent>) {
        if self.loading {
            return;
        }
        let kind = update.get("sessionUpdate").and_then(Value::as_str).unwrap_or("");
        match kind {
            "agent_message_chunk" | "agent_thought_chunk" => {
                let Some(text) = content_text(update.get("content")) else {
                    return;
                };
                let thinking = kind == "agent_thought_chunk";
                let given = update.get("messageId").and_then(Value::as_str).map(str::to_string);
                let id = match (&self.run, thinking, given) {
                    (_, _, Some(id)) => id,
                    (Run::Text(id), false, None) | (Run::Thinking(id), true, None) => id.clone(),
                    _ => crate::workspace::new_id(),
                };
                if thinking {
                    self.run = Run::Thinking(id.clone());
                    events.push(ChatEvent::Thinking { id, delta: text });
                } else {
                    self.run = Run::Text(id.clone());
                    events.push(ChatEvent::Text { id, delta: text });
                }
            }
            // The user's own words: the window shows them as typed.
            "user_message_chunk" => {}
            "tool_call" | "tool_call_update" => {
                self.run = Run::None;
                self.tool(update, kind == "tool_call", events);
            }
            "plan" => {
                self.run = Run::None;
                let entries: Vec<String> = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|entry| {
                        let text = entry.get("content").and_then(Value::as_str)?;
                        let mark = match entry.get("status").and_then(Value::as_str) {
                            Some("completed") => "[x]",
                            Some("in_progress") => "[~]",
                            _ => "[ ]",
                        };
                        Some(format!("{mark} {text}"))
                    })
                    .collect();
                if !entries.is_empty() {
                    let id = "plan".to_string();
                    events.push(ChatEvent::Tool {
                        id: id.clone(),
                        kind: ToolKind::Task,
                        name: "plan".into(),
                        title: "Plan".into(),
                        detail: clip_head(&entries.join("\n"), DETAIL_LIMIT),
                    });
                }
            }
            _ => {}
        }
    }

    fn tool(&mut self, update: &Value, new: bool, events: &mut Vec<ChatEvent>) {
        let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
            return;
        };
        let cwd = Some(self.options.cwd.as_str());
        let known = self.tools.entry(id.to_string()).or_default();
        let mut changed = new;
        if let Some(kind) = update.get("kind").and_then(Value::as_str) {
            changed |= known.kind.as_deref() != Some(kind);
            known.kind = Some(kind.to_string());
        }
        if let Some(title) = update.get("title").and_then(Value::as_str) {
            changed |= known.title != title;
            known.title = one_line(title, 120);
        }
        let detail = tool_detail(update, cwd);
        if !detail.is_empty() && detail != known.detail {
            known.detail = detail;
            changed = true;
        }
        if changed {
            let kind = known.kind.as_deref().unwrap_or("other");
            events.push(ChatEvent::Tool {
                id: id.to_string(),
                kind: tool_kind(kind),
                name: kind.to_string(),
                title: if known.title.is_empty() { kind.to_string() } else { known.title.clone() },
                detail: known.detail.clone(),
            });
        }
        for (path, diff) in diffs(update, cwd) {
            events.push(ChatEvent::FileChange { id: id.to_string(), path, diff });
        }
        let status = update.get("status").and_then(Value::as_str);
        if matches!(status, Some("completed") | Some("failed")) {
            events.push(ChatEvent::ToolResult {
                id: id.to_string(),
                ok: status == Some("completed"),
                output: clip_tail(&tool_output(update), OUTPUT_LIMIT),
            });
        }
    }

    fn agent_request(&mut self, id: &Value, method: &str, message: &Value, events: &mut Vec<ChatEvent>) {
        let params = message.get("params").unwrap_or(&Value::Null);
        if method != "session/request_permission" {
            // The file system and terminal Convoy did not offer, and
            // anything newer: refused, so the agent does not wait for ever.
            self.outbox.push(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": "Not supported by Convoy." }
                })
                .to_string(),
            );
            return;
        }
        let request = match id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let options: Vec<(String, String)> = params
            .get("options")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|option| {
                Some((
                    option.get("optionId")?.as_str()?.to_string(),
                    option.get("kind")?.as_str()?.to_string(),
                ))
            })
            .collect();
        let call = params.get("toolCall").unwrap_or(&Value::Null);
        let tool_id = call.get("toolCallId").and_then(Value::as_str).unwrap_or("");
        let known = self.tools.get(tool_id).cloned().unwrap_or_default();
        let kind_name = call
            .get("kind")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or(known.kind)
            .unwrap_or_default();
        let title = call
            .get("title")
            .and_then(Value::as_str)
            .map(|title| one_line(title, 120))
            .filter(|title| !title.is_empty())
            .unwrap_or(if known.title.is_empty() { "Use a tool".to_string() } else { known.title });
        let detail = tool_detail(call, Some(self.options.cwd.as_str()));
        let detail = if detail.is_empty() { known.detail } else { detail };
        let always = options.iter().any(|(_, kind)| kind == "allow_always");
        self.pending.insert(request.clone(), Pending { id: id.clone(), options });
        events.push(ChatEvent::Approval {
            request,
            kind: match kind_name.as_str() {
                "execute" => ApprovalKind::Command,
                "edit" | "delete" | "move" => ApprovalKind::Edit,
                _ => ApprovalKind::Tool,
            },
            title,
            detail,
            always,
        });
    }
}

fn chat_model(id: &str, name: &str, current: bool) -> ChatModel {
    ChatModel {
        id: id.to_string(),
        name: name.to_string(),
        efforts: Vec::new(),
        default_effort: None,
        is_default: current,
    }
}

/// ACP's tool kinds, as the window groups them.
fn tool_kind(kind: &str) -> ToolKind {
    match kind {
        "execute" => ToolKind::Command,
        "edit" | "delete" | "move" => ToolKind::Edit,
        "read" => ToolKind::Read,
        "search" => ToolKind::Search,
        "fetch" => ToolKind::Web,
        _ => ToolKind::Other,
    }
}

/// The text of a content block, when it is text.
fn content_text(content: Option<&Value>) -> Option<String> {
    let content = content?;
    match content.get("type").and_then(Value::as_str) {
        Some("text") => content.get("text").and_then(Value::as_str).map(str::to_string),
        Some("resource_link") => content.get("uri").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// What a tool card shows under its title: the command, or the files.
fn tool_detail(call: &Value, cwd: Option<&str>) -> String {
    if let Some(input) = call.get("rawInput") {
        let command = match input.get("command") {
            Some(Value::String(command)) => Some(command.clone()),
            Some(Value::Array(parts)) => Some(
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            _ => None,
        };
        if let Some(command) = command.filter(|c| !c.is_empty()) {
            return clip_head(&command, DETAIL_LIMIT);
        }
    }
    let paths: Vec<String> = call
        .get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| location.get("path").and_then(Value::as_str))
        .map(|path| short_path(path, cwd))
        .collect();
    if !paths.is_empty() {
        return clip_head(&paths.join("\n"), DETAIL_LIMIT);
    }
    match call.get("rawInput") {
        Some(Value::Null) | None => String::new(),
        Some(Value::String(text)) => clip_head(text, DETAIL_LIMIT),
        Some(input) => clip_head(&input.to_string(), DETAIL_LIMIT),
    }
}

/// The diffs a tool call carries, as unified diffs.
fn diffs(call: &Value, cwd: Option<&str>) -> Vec<(String, String)> {
    call.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("diff"))
        .filter_map(|item| {
            let path = short_path(item.get("path")?.as_str()?, cwd);
            let new = item.get("newText").and_then(Value::as_str).unwrap_or("");
            let diff = match item.get("oldText").and_then(Value::as_str) {
                Some(old) => replacement_diff(&path, old, new),
                None => added_diff(&path, new),
            };
            Some((path, diff))
        })
        .collect()
}

/// What a finished tool printed: its text content, else its raw output.
fn tool_output(call: &Value) -> String {
    let text: Vec<String> = call
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("content"))
        .filter_map(|item| content_text(item.get("content")))
        .collect();
    if !text.is_empty() {
        return text.join("\n");
    }
    match call.get("rawOutput") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(output)) => output
            .get("output")
            .or_else(|| output.get("stdout"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| Value::Object(output.clone()).to_string()),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// Token counts from a prompt result, when the agent reports them.
/// `inputTokens` there includes cache reads, which [`Usage`] keeps apart.
fn usage(value: &Value) -> Usage {
    let count = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
    let cache_read = count("cachedReadTokens");
    Usage {
        input: count("inputTokens").saturating_sub(cache_read),
        output: count("outputTokens") + count("thoughtTokens"),
        cache_read,
        cache_write: count("cachedWriteTokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> AcpClient {
        AcpClient::new(Options {
            name: "opencode".into(),
            cwd: "/work".into(),
            login_command: Some("opencode auth login".into()),
            ..Options::default()
        })
    }

    fn parse(line: &str) -> Value {
        serde_json::from_str(line).unwrap()
    }

    /// Runs the handshake up to an open session.
    fn opened(client: &mut AcpClient, session_result: Value) -> Vec<ChatEvent> {
        client.start();
        client.feed(
            &json!({ "jsonrpc": "2.0", "id": 1, "result": {
                "protocolVersion": 1,
                "agentCapabilities": { "loadSession": true, "promptCapabilities": { "image": true } }
            }})
            .to_string(),
        );
        let out = client.take_outgoing();
        assert_eq!(parse(&out[0])["method"], "session/new");
        assert_eq!(parse(&out[0])["params"]["cwd"], "/work");
        client.feed(&json!({ "jsonrpc": "2.0", "id": 2, "result": session_result }).to_string())
    }

    #[test]
    fn handshake_opens_a_session_and_lists_models() {
        let mut acp = client();
        let first = parse(&acp.start()[0]);
        assert_eq!(first["method"], "initialize");
        assert_eq!(first["params"]["protocolVersion"], 1);
        assert_eq!(first["params"]["clientCapabilities"]["terminal"], false);
        let events = opened(
            &mut acp,
            json!({ "sessionId": "s1", "models": {
                "currentModelId": "deepseek/deepseek-chat",
                "availableModels": [
                    { "modelId": "deepseek/deepseek-chat", "name": "DeepSeek V3" },
                    { "modelId": "deepseek/deepseek-reasoner", "name": "DeepSeek R1" }
                ]
            }}),
        );
        assert_eq!(events[0], ChatEvent::Session { provider_id: "s1".into() });
        let ChatEvent::Models { models, model, .. } = &events[1] else {
            panic!("{events:?}")
        };
        assert_eq!(models.len(), 2);
        assert!(models[0].is_default);
        assert_eq!(model.as_deref(), Some("deepseek/deepseek-chat"));
        assert!(acp.ready());

        let switch = acp.set_model(Some("deepseek/deepseek-reasoner"));
        let switch = parse(&switch[0]);
        assert_eq!(switch["method"], "session/set_model");
        assert_eq!(switch["params"]["modelId"], "deepseek/deepseek-reasoner");
    }

    #[test]
    fn a_turn_streams_text_tools_and_ends_with_usage() {
        let mut acp = client();
        opened(&mut acp, json!({ "sessionId": "s1" }));
        let (lines, events) = acp.send("fix it", &[]);
        assert_eq!(events, vec![ChatEvent::TurnStarted]);
        let prompt = parse(&lines[0]);
        assert_eq!(prompt["method"], "session/prompt");
        assert_eq!(prompt["params"]["prompt"][0]["text"], "fix it");
        let prompt_id = prompt["id"].as_i64().unwrap();

        let update = |update: Value| {
            json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": update } })
                .to_string()
        };
        let a = acp.feed(&update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Hel" } })));
        let b = acp.feed(&update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "lo" } })));
        let (ChatEvent::Text { id: first, .. }, ChatEvent::Text { id: second, delta }) = (&a[0], &b[0]) else {
            panic!()
        };
        assert_eq!(first, second, "chunks join one message");
        assert_eq!(delta, "lo");

        let tool = acp.feed(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Run tests", "kind": "execute",
            "status": "pending", "rawInput": { "command": ["cargo", "test"] }
        })));
        assert_eq!(
            tool[0],
            ChatEvent::Tool { id: "t1".into(), kind: ToolKind::Command, name: "execute".into(), title: "Run tests".into(), detail: "cargo test".into() }
        );
        let done = acp.feed(&update(json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
            "content": [{ "type": "content", "content": { "type": "text", "text": "ok" } }]
        })));
        assert_eq!(done, vec![ChatEvent::ToolResult { id: "t1".into(), ok: true, output: "ok".into() }]);

        let edit = acp.feed(&update(json!({
            "sessionUpdate": "tool_call", "toolCallId": "t2", "title": "Edit", "kind": "edit", "status": "completed",
            "content": [{ "type": "diff", "path": "/work/a.txt", "oldText": "a\n", "newText": "b\n" }]
        })));
        assert!(edit.iter().any(|e| matches!(e, ChatEvent::FileChange { path, .. } if path == "a.txt")));

        let after = acp.feed(&update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "Done" } })));
        let ChatEvent::Text { id: third, .. } = &after[0] else { panic!() };
        assert_ne!(third, first, "a tool in between starts a new message");

        let end = acp.feed(
            &json!({ "jsonrpc": "2.0", "id": prompt_id, "result": { "stopReason": "end_turn",
                "usage": { "inputTokens": 100, "outputTokens": 20, "cachedReadTokens": 60 } } })
            .to_string(),
        );
        assert_eq!(
            end,
            vec![ChatEvent::TurnEnded { usage: Usage { input: 40, output: 20, cache_read: 60, cache_write: 0 }, error: None }]
        );
        assert!(!acp.in_turn());
    }

    #[test]
    fn permission_questions_are_answered_with_the_offered_option() {
        let mut acp = client();
        opened(&mut acp, json!({ "sessionId": "s1" }));
        acp.send("go", &[]);
        let events = acp.feed(
            &json!({ "jsonrpc": "2.0", "id": 7, "method": "session/request_permission", "params": {
                "sessionId": "s1",
                "toolCall": { "toolCallId": "t1", "title": "rm -rf build", "kind": "execute" },
                "options": [
                    { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "always", "name": "Always", "kind": "allow_always" },
                    { "optionId": "no", "name": "Reject", "kind": "reject_once" }
                ]
            }})
            .to_string(),
        );
        let ChatEvent::Approval { request, kind, always, .. } = &events[0] else { panic!() };
        assert_eq!(*kind, ApprovalKind::Command);
        assert!(*always);
        let (line, resolved) = acp.answer(request, Decision::Always).unwrap();
        let line = parse(&line);
        assert_eq!(line["id"], 7);
        assert_eq!(line["result"]["outcome"]["optionId"], "always");
        assert!(matches!(resolved, ChatEvent::ApprovalResolved { .. }));
        assert!(acp.answer(request, Decision::Deny).is_err());
    }

    #[test]
    fn interrupt_cancels_the_turn_and_open_questions() {
        let mut acp = client();
        opened(&mut acp, json!({ "sessionId": "s1" }));
        assert!(acp.interrupt().is_empty(), "nothing runs yet");
        let (lines, _) = acp.send("go", &[]);
        let prompt_id = parse(&lines[0])["id"].as_i64().unwrap();
        acp.feed(
            &json!({ "jsonrpc": "2.0", "id": "p", "method": "session/request_permission", "params": {
                "sessionId": "s1", "toolCall": { "toolCallId": "t1" },
                "options": [{ "optionId": "yes", "kind": "allow_once" }]
            }})
            .to_string(),
        );
        let lines = acp.interrupt();
        assert_eq!(parse(&lines[0])["method"], "session/cancel");
        assert_eq!(parse(&lines[1])["result"]["outcome"]["outcome"], "cancelled");
        let end = acp.feed(&json!({ "jsonrpc": "2.0", "id": prompt_id, "result": { "stopReason": "cancelled" } }).to_string());
        assert!(matches!(end[0], ChatEvent::TurnEnded { error: None, .. }));
    }

    #[test]
    fn turns_wait_for_the_session_and_for_each_other() {
        let mut acp = client();
        acp.start();
        let (lines, events) = acp.send("early", &[]);
        assert!(lines.is_empty());
        assert_eq!(events, vec![ChatEvent::TurnStarted]);
        acp.feed(&json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": {} } }).to_string());
        acp.take_outgoing();
        acp.feed(&json!({ "jsonrpc": "2.0", "id": 2, "result": { "sessionId": "s1" } }).to_string());
        let out = acp.take_outgoing();
        let first = parse(&out[0]);
        assert_eq!(first["params"]["prompt"][0]["text"], "early");

        let (lines, events) = acp.send("second", &[]);
        assert!(lines.is_empty(), "one prompt at a time");
        assert!(events.is_empty(), "the turn already shows as running");
        let end = acp.feed(&json!({ "jsonrpc": "2.0", "id": first["id"], "result": { "stopReason": "end_turn" } }).to_string());
        assert!(end.is_empty(), "the next turn follows at once");
        let out = acp.take_outgoing();
        assert_eq!(parse(&out[0])["params"]["prompt"][0]["text"], "second");
    }

    #[test]
    fn resume_loads_the_session_without_showing_the_replay_again() {
        let mut acp = AcpClient::new(Options { cwd: "/work".into(), resume: Some("old".into()), ..Options::default() });
        acp.start();
        acp.feed(&json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": { "loadSession": true } } }).to_string());
        let load = parse(&acp.take_outgoing()[0]);
        assert_eq!(load["method"], "session/load");
        assert_eq!(load["params"]["sessionId"], "old");
        let update = |update: Value| {
            json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "old", "update": update } }).to_string()
        };
        assert!(acp.feed(&update(json!({ "sessionUpdate": "user_message_chunk", "content": { "type": "text", "text": "hi" } }))).is_empty());
        assert!(acp.feed(&update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "hello" } }))).is_empty());
        let events = acp.feed(&json!({ "jsonrpc": "2.0", "id": 2, "result": null }).to_string());
        assert_eq!(events[0], ChatEvent::Session { provider_id: "old".into() });
        let live = acp.feed(&update(json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "new" } })));
        assert!(matches!(&live[0], ChatEvent::Text { delta, .. } if delta == "new"));
    }

    #[test]
    fn resume_without_load_support_starts_fresh_and_says_so() {
        let mut acp = AcpClient::new(Options { name: "Agy".into(), resume: Some("old".into()), ..Options::default() });
        acp.start();
        let events = acp.feed(&json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": {} } }).to_string());
        assert!(matches!(&events[0], ChatEvent::Error { message } if message.contains("new one")));
        assert_eq!(parse(&acp.take_outgoing()[0])["method"], "session/new");
    }

    #[test]
    fn sign_in_errors_name_the_login_command_and_unknown_requests_are_refused() {
        let mut acp = client();
        acp.start();
        acp.feed(&json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": {} } }).to_string());
        acp.take_outgoing();
        let events = acp.feed(&json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -32000, "message": "Authentication required" } }).to_string());
        assert!(matches!(&events[0], ChatEvent::Error { message } if message.contains("opencode auth login")));

        let events = acp.feed(&json!({ "jsonrpc": "2.0", "id": 9, "method": "fs/read_text_file", "params": {} }).to_string());
        assert!(events.is_empty());
        assert_eq!(parse(&acp.take_outgoing()[0])["error"]["code"], -32601);
    }

    #[test]
    fn models_from_config_options_switch_through_them() {
        let mut acp = client();
        let events = opened(
            &mut acp,
            json!({ "sessionId": "s1", "configOptions": [{
                "id": "model", "category": "model", "type": "select", "currentValue": "a",
                "options": [{ "value": "a", "name": "A" }, { "value": "b", "name": "B" }]
            }]}),
        );
        assert!(matches!(&events[1], ChatEvent::Models { models, .. } if models.len() == 2));
        let line = parse(&acp.set_model(Some("b"))[0]);
        assert_eq!(line["method"], "session/set_config_option");
        assert_eq!(line["params"]["configId"], "model");
        assert_eq!(line["params"]["value"], "b");
    }

    #[test]
    fn instructions_lead_the_first_turn_only() {
        let mut acp = AcpClient::new(Options { instructions: Some("be brief".into()), ..Options::default() });
        opened_plain(&mut acp);
        let (lines, _) = acp.send("one", &[]);
        let prompt = parse(&lines[0]);
        assert_eq!(prompt["params"]["prompt"][0]["text"], "be brief");
        assert_eq!(prompt["params"]["prompt"][1]["text"], "one");
        acp.feed(&json!({ "jsonrpc": "2.0", "id": prompt["id"], "result": { "stopReason": "end_turn" } }).to_string());
        let (lines, _) = acp.send("two", &[]);
        assert_eq!(parse(&lines[0])["params"]["prompt"].as_array().unwrap().len(), 1);
    }

    fn opened_plain(acp: &mut AcpClient) {
        acp.start();
        acp.feed(&json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": {} } }).to_string());
        acp.take_outgoing();
        acp.feed(&json!({ "jsonrpc": "2.0", "id": 2, "result": { "sessionId": "s" } }).to_string());
    }
}
