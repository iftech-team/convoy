//! Codex through `codex app-server`, the JSON-RPC protocol its own editors
//! use.
//!
//! The order is fixed: `initialize`, the `initialized` notification, then a
//! thread is started or resumed, and only then can a turn start. A message
//! typed before the thread exists waits here. Approvals arrive as requests
//! from the server, with an `id` of their own, and are answered by that id.
//! As in [`crate::provider::codex::handle_line`], the protocol is kept apart
//! from the I/O so it can be replayed in a test.

use super::event::{
    added_diff, clip_head, clip_tail, one_line, short_path, ApprovalKind, ChatEvent, ChatModel,
    Decision, Image, ToolKind, Usage, DETAIL_LIMIT, OUTPUT_LIMIT,
};
use crate::{bail, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

/// After `codex`: the app server on stdio.
pub fn args() -> Vec<String> {
    vec!["app-server".into(), "--listen".into(), "stdio://".into()]
}

// Fixed ids for the handshake; turns and interrupts count up from THREAD.
// The same numbers the recorded traffic used, so it replays as recorded.
const INITIALIZE: i64 = 1;
/// `model/list`, and each further page of it.
const MODEL_LIST: i64 = 2;
const THREAD: i64 = 11;

/// Asks for the models this account may use, from `cursor` on.
fn model_list(cursor: Option<&str>) -> String {
    let params = match cursor {
        Some(cursor) => json!({ "cursor": cursor }),
        None => json!({}),
    };
    json!({ "id": MODEL_LIST, "method": "model/list", "params": params }).to_string()
}

/// The models in a `model/list` result, in its order. Hidden ones and
/// anything malformed are left out.
pub fn models(result: &Value) -> Vec<ChatModel> {
    let level = |value: &Value| {
        value
            .as_str()
            .filter(|level| crate::provider::valid_effort(crate::model::Agent::Codex, level))
            .map(str::to_string)
    };
    result
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|entry| entry.get("hidden").and_then(Value::as_bool) != Some(true))
        .filter_map(|entry| {
            let id = entry
                .get("id")
                .or_else(|| entry.get("model"))
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && crate::patterns::MODEL.is_match(id))?;
            let name = entry
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            let efforts: Vec<String> = entry
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| option.get("reasoningEffort").and_then(level))
                .collect();
            let default_effort = entry
                .get("defaultReasoningEffort")
                .and_then(level)
                .filter(|level| efforts.contains(level));
            Some(ChatModel {
                id: id.to_string(),
                name: name.to_string(),
                efforts,
                default_effort,
                is_default: entry.get("isDefault").and_then(Value::as_bool) == Some(true),
            })
        })
        .collect()
}

/// How the thread is opened.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub cwd: String,
    pub model: Option<String>,
    /// The reasoning effort each turn asks for; none keeps the default.
    pub effort: Option<String>,
    /// The thread to resume; none starts a new one.
    pub resume: Option<String>,
    /// The user chose to skip approvals and the sandbox for Codex, as
    /// `--dangerously-bypass-approvals-and-sandbox` does in a terminal.
    pub yolo: bool,
    /// Instructions on top of Codex's own, sent as `developerInstructions`
    /// when the thread opens: a manager session's, for one.
    pub developer_instructions: Option<String>,
}

/// A question waiting for the user, with what answering it needs.
#[derive(Debug, Clone)]
struct Pending {
    /// The server's own id, number or string, echoed back exactly.
    id: Value,
    available: Vec<Value>,
    amendment: Option<Value>,
}

/// The client side of one app-server conversation.
#[derive(Debug)]
pub struct CodexClient {
    options: Options,
    next_id: i64,
    ready: bool,
    thread: Option<String>,
    turn: Option<String>,
    in_turn: bool,
    /// Turns typed before the thread existed.
    queued: Vec<Value>,
    pending: HashMap<String, Pending>,
    usage: Usage,
    outbox: Vec<String>,
    /// What `model/list` returned so far, pages joined.
    models: Vec<ChatModel>,
    /// Set once the user switches mid-conversation: from then on each turn
    /// says which model and effort, the default's spelled out, since a
    /// turn's override lasts until another replaces it.
    model_changed: bool,
    effort_changed: bool,
}

impl CodexClient {
    pub fn new(options: Options) -> Self {
        CodexClient {
            options,
            next_id: THREAD,
            ready: false,
            thread: None,
            turn: None,
            in_turn: false,
            queued: Vec::new(),
            pending: HashMap::new(),
            usage: Usage::default(),
            outbox: Vec::new(),
            models: Vec::new(),
            model_changed: false,
            effort_changed: false,
        }
    }

    /// Switches the model from the next turn on; none is Codex's default.
    pub fn set_model(&mut self, model: Option<&str>) {
        self.options.model = model.filter(|m| !m.is_empty()).map(str::to_string);
        self.model_changed = true;
    }

    /// Switches the reasoning effort from the next turn on; none is the
    /// model's default.
    pub fn set_effort(&mut self, effort: Option<&str>) {
        self.options.effort = effort.filter(|e| !e.is_empty()).map(str::to_string);
        self.effort_changed = true;
    }

    /// The model a turn runs on: the one chosen, else the server's default.
    fn current_model(&self) -> Option<&ChatModel> {
        match self.options.model.as_deref() {
            Some(id) => self.models.iter().find(|model| model.id == id),
            None => self.models.iter().find(|model| model.is_default),
        }
    }

    /// The first line to send.
    pub fn start(&mut self) -> Vec<String> {
        vec![json!({
            "id": INITIALIZE,
            "method": "initialize",
            "params": { "clientInfo": { "name": "convoy", "version": env!("CARGO_PKG_VERSION") } }
        })
        .to_string()]
    }

    /// The thread id, once the server has given one. It is the id
    /// `codex resume <id>` takes.
    pub fn thread_id(&self) -> Option<&str> {
        self.thread.as_deref()
    }

    /// Whether the thread is open and turns can start.
    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn in_turn(&self) -> bool {
        self.in_turn
    }

    /// Lines reading produced: the next step of the handshake, queued turns,
    /// and refusals of requests Convoy does not handle.
    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    fn request(&mut self, method: &str, params: Value) -> String {
        self.next_id += 1;
        json!({ "id": self.next_id, "method": method, "params": params }).to_string()
    }

    /// Overrides that apply to a new and a resumed thread alike.
    fn overrides(&self, params: &mut Value) {
        if let Some(model) = self.options.model.as_deref().filter(|m| !m.is_empty()) {
            params["model"] = json!(model);
        }
        if self.options.yolo {
            params["approvalPolicy"] = json!("never");
            params["sandbox"] = json!("danger-full-access");
        } else {
            params["approvalPolicy"] = json!("on-request");
        }
        if let Some(text) = self
            .options
            .developer_instructions
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            params["developerInstructions"] = json!(text);
        }
    }

    fn thread_request(&self) -> String {
        let (method, mut params) = match &self.options.resume {
            Some(thread) => ("thread/resume", json!({ "threadId": thread })),
            None => ("thread/start", json!({ "cwd": self.options.cwd })),
        };
        self.overrides(&mut params);
        json!({ "id": THREAD, "method": method, "params": params }).to_string()
    }

    fn turn_request(&mut self, input: Value) -> String {
        let thread = self.thread.clone().unwrap_or_default();
        let mut params = json!({ "threadId": thread, "input": input });
        if self.model_changed {
            let model = self
                .options
                .model
                .clone()
                .or_else(|| self.current_model().map(|model| model.id.clone()));
            if let Some(model) = model {
                params["model"] = json!(model);
            }
        }
        let effort = match self.options.effort.as_deref().filter(|e| !e.is_empty()) {
            Some(effort) => Some(effort.to_string()),
            // Back to the default after a switch: the model's own level.
            None if self.effort_changed || self.model_changed => self
                .current_model()
                .and_then(|model| model.default_effort.clone()),
            None => None,
        };
        if let Some(effort) = effort {
            params["effort"] = json!(effort);
        }
        self.request("turn/start", params)
    }

    /// A user turn. Sent at once when the thread is open, otherwise when it
    /// opens. `turn_started` goes out now so the window shows work at once.
    pub fn send(&mut self, text: &str, images: &[Image]) -> (Vec<String>, Vec<ChatEvent>) {
        let mut input: Vec<Value> = images
            .iter()
            .map(|image| {
                json!({ "type": "image", "url": format!("data:{};base64,{}", image.media_type, image.data) })
            })
            .collect();
        if !text.is_empty() || input.is_empty() {
            input.push(json!({ "type": "text", "text": text }));
        }
        let mut events = Vec::new();
        if !self.in_turn {
            self.in_turn = true;
            events.push(ChatEvent::TurnStarted);
        }
        if self.ready {
            (vec![self.turn_request(Value::Array(input))], events)
        } else {
            self.queued.push(Value::Array(input));
            (Vec::new(), events)
        }
    }

    /// Stops the running turn. Nothing to send when none runs.
    pub fn interrupt(&mut self) -> Option<String> {
        let thread = self.thread.clone()?;
        let turn = self.turn.clone()?;
        Some(self.request(
            "turn/interrupt",
            json!({ "threadId": thread, "turnId": turn }),
        ))
    }

    /// Answers a pending approval. "Always" takes the broadest session-wide
    /// choice the server offered, and plain acceptance when it offered none.
    pub fn answer(&mut self, request: &str, decision: Decision) -> Result<(String, ChatEvent)> {
        let Some(pending) = self.pending.remove(request) else {
            bail!("This request was already answered.")
        };
        let offered = |name: &str| {
            pending.available.is_empty()
                || pending
                    .available
                    .iter()
                    .any(|choice| choice.as_str() == Some(name))
        };
        let decision = match decision {
            Decision::Deny => json!("decline"),
            Decision::Allow => json!("accept"),
            Decision::Always if offered("acceptForSession") => json!("acceptForSession"),
            Decision::Always => match &pending.amendment {
                Some(amendment) => {
                    json!({ "acceptWithExecpolicyAmendment": { "execpolicy_amendment": amendment } })
                }
                None => json!("accept"),
            },
        };
        Ok((
            json!({ "id": pending.id, "result": { "decision": decision } }).to_string(),
            ChatEvent::ApprovalResolved {
                request: request.to_string(),
            },
        ))
    }

    /// One line from the server.
    pub fn feed(&mut self, line: &str) -> Vec<ChatEvent> {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let method = message.get("method").and_then(Value::as_str);
        match (message.get("id"), method) {
            (Some(id), Some(method)) => self.server_request(id, method, &message, &mut events),
            (Some(id), None) => self.response(id, &message, &mut events),
            (None, Some(method)) => self.notification(method, &message, &mut events),
            (None, None) => {}
        }
        events
    }

    fn response(&mut self, id: &Value, message: &Value, events: &mut Vec<ChatEvent>) {
        if id.as_i64() == Some(MODEL_LIST) {
            // A server without the method keeps the window's own list.
            if let Some(result) = message.get("result") {
                self.model_page(result, events);
            }
            return;
        }
        if let Some(error) = message.get("error") {
            let text = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Codex refused the request.");
            events.push(ChatEvent::Error {
                message: match id.as_i64() {
                    Some(THREAD) if self.options.resume.is_some() => {
                        format!("Codex could not resume this conversation: {text}")
                    }
                    _ => text.to_string(),
                },
            });
            if id.as_i64() != Some(INITIALIZE) && id.as_i64() != Some(THREAD) && self.in_turn {
                // A turn that never started will not end on its own.
                self.in_turn = false;
                events.push(ChatEvent::TurnEnded {
                    usage: Usage::default(),
                    error: Some(text.to_string()),
                });
            }
            return;
        }
        let result = message.get("result").unwrap_or(&Value::Null);
        match id.as_i64() {
            Some(INITIALIZE) => {
                self.outbox
                    .push(json!({ "method": "initialized" }).to_string());
                self.outbox.push(self.thread_request());
                self.outbox.push(model_list(None));
            }
            Some(THREAD) => {
                let Some(thread) = result.pointer("/thread/id").and_then(Value::as_str) else {
                    return;
                };
                self.thread = Some(thread.to_string());
                self.ready = true;
                events.push(ChatEvent::Session {
                    provider_id: thread.to_string(),
                });
                for input in std::mem::take(&mut self.queued) {
                    let line = self.turn_request(input);
                    self.outbox.push(line);
                }
            }
            _ => {
                if let Some(turn) = result.pointer("/turn/id").and_then(Value::as_str) {
                    self.turn = Some(turn.to_string());
                }
            }
        }
    }

    /// One page of `model/list`. The list is reported once it is whole.
    fn model_page(&mut self, result: &Value, events: &mut Vec<ChatEvent>) {
        for model in models(result) {
            if !self.models.iter().any(|known| known.id == model.id) {
                self.models.push(model);
            }
        }
        if let Some(cursor) = result
            .get("nextCursor")
            .and_then(Value::as_str)
            .filter(|cursor| !cursor.is_empty())
        {
            self.outbox.push(model_list(Some(cursor)));
            return;
        }
        if !self.models.is_empty() {
            events.push(ChatEvent::Models {
                models: self.models.clone(),
                model: self.options.model.clone(),
                effort: self.options.effort.clone(),
            });
        }
    }

    fn server_request(
        &mut self,
        id: &Value,
        method: &str,
        message: &Value,
        events: &mut Vec<ChatEvent>,
    ) {
        let params = message.get("params").unwrap_or(&Value::Null);
        let request = match id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let available = params
            .get("availableDecisions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let reason = params.get("reason").and_then(Value::as_str).unwrap_or("");
        let (kind, title, detail) = match method {
            "item/commandExecution/requestApproval" => {
                let command = command_text(params);
                let detail = if reason.is_empty() {
                    command.clone()
                } else {
                    format!("{command}\n\n{reason}")
                };
                (
                    ApprovalKind::Command,
                    format!("Run `{}`", one_line(&command, 80)),
                    clip_head(&detail, DETAIL_LIMIT),
                )
            }
            "item/fileChange/requestApproval" => (
                ApprovalKind::Edit,
                "Edit files".to_string(),
                clip_head(reason, DETAIL_LIMIT),
            ),
            _ => {
                // Anything else — a question for the user, a token refresh —
                // is refused, so the server does not wait for ever.
                self.outbox.push(
                    json!({
                        "id": id,
                        "error": { "code": -32601, "message": "Not supported by Convoy." }
                    })
                    .to_string(),
                );
                return;
            }
        };
        let amendment = params.get("proposedExecpolicyAmendment").cloned();
        let always = available.is_empty()
            || available.iter().any(|choice| {
                choice.as_str() == Some("acceptForSession")
                    || choice.get("acceptWithExecpolicyAmendment").is_some()
            });
        self.pending.insert(
            request.clone(),
            Pending {
                id: id.clone(),
                available,
                amendment,
            },
        );
        events.push(ChatEvent::Approval {
            request,
            kind,
            title,
            detail,
            always,
        });
    }

    fn notification(&mut self, method: &str, message: &Value, events: &mut Vec<ChatEvent>) {
        let params = message.get("params").unwrap_or(&Value::Null);
        let text = |key: &str| {
            params
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        match method {
            "turn/started" => {
                if let Some(turn) = params.pointer("/turn/id").and_then(Value::as_str) {
                    self.turn = Some(turn.to_string());
                }
                self.usage = Usage::default();
                if !self.in_turn {
                    self.in_turn = true;
                    events.push(ChatEvent::TurnStarted);
                }
            }
            "item/agentMessage/delta" => events.push(ChatEvent::Text {
                id: text("itemId"),
                delta: text("delta"),
            }),
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                events.push(ChatEvent::Thinking {
                    id: text("itemId"),
                    delta: text("delta"),
                })
            }
            "item/started" => {
                if let Some(item) = params.get("item") {
                    item_started(item, Some(&self.options.cwd), events);
                }
            }
            "item/completed" => {
                if let Some(item) = params.get("item") {
                    item_completed(item, Some(&self.options.cwd), events);
                }
            }
            "serverRequest/resolved" => {
                let request = match params.get("requestId") {
                    Some(Value::String(text)) => text.clone(),
                    Some(other) => other.to_string(),
                    None => return,
                };
                if self.pending.remove(&request).is_some() {
                    events.push(ChatEvent::ApprovalResolved { request });
                }
            }
            "thread/tokenUsage/updated" => {
                // `last` is one model call; a turn is the sum of its calls.
                let last = params.pointer("/tokenUsage/last").unwrap_or(&Value::Null);
                let count = |key: &str| last.get(key).and_then(Value::as_u64).unwrap_or(0);
                let cached = count("cachedInputTokens");
                self.usage.input += count("inputTokens").saturating_sub(cached);
                self.usage.output += count("outputTokens");
                self.usage.cache_read += cached;
                self.usage.cache_write += count("cacheWriteInputTokens");
            }
            "turn/completed" => {
                for request in self.pending.drain().map(|(id, _)| id) {
                    events.push(ChatEvent::ApprovalResolved { request });
                }
                let turn = params.get("turn").unwrap_or(&Value::Null);
                let error = turn
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| match turn.get("status").and_then(Value::as_str) {
                        Some("failed") => Some("The turn failed.".to_string()),
                        Some("interrupted") => Some("Interrupted.".to_string()),
                        _ => None,
                    });
                self.in_turn = false;
                self.turn = None;
                events.push(ChatEvent::TurnEnded {
                    usage: std::mem::take(&mut self.usage),
                    error,
                });
            }
            "error" => {
                // Retried errors are only progress; the turn reports a final one.
                let retrying = params
                    .get("willRetry")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if let Some(message) = params.pointer("/error/message").and_then(Value::as_str) {
                    if !retrying {
                        events.push(ChatEvent::Error {
                            message: message.to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// The command as the agent meant it: the parsed form without the
/// `/bin/zsh -lc` wrapper when there is one.
fn command_text(item: &Value) -> String {
    let parsed: Vec<&str> = item
        .get("commandActions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| action.get("command").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    if !parsed.is_empty() {
        return parsed.join(" && ");
    }
    match item.get("command") {
        Some(Value::String(command)) => command.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

/// `read` or `search` when every part of a command only looks, else `command`.
fn command_kind(item: &Value) -> ToolKind {
    let kinds: Vec<&str> = item
        .get("commandActions")
        .and_then(Value::as_array)
        .map(|actions| {
            actions
                .iter()
                .filter_map(|action| action.get("type").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    match kinds.as_slice() {
        [] => ToolKind::Command,
        kinds if kinds.iter().all(|kind| *kind == "read") => ToolKind::Read,
        kinds
            if kinds
                .iter()
                .all(|kind| *kind == "search" || *kind == "listFiles") =>
        {
            ToolKind::Search
        }
        _ => ToolKind::Command,
    }
}

/// The paths a file change touches, with a diff for each. The live protocol
/// sends a list; older builds and the rollout file a map keyed by path.
pub(crate) fn changes(item: &Value, cwd: Option<&str>) -> Vec<(String, String)> {
    let one = |path: &str, change: &Value| {
        let path = &short_path(path, cwd);
        let header = path.trim_start_matches('/');
        let kind = change
            .pointer("/kind/type")
            .or_else(|| change.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("update");
        let diff = change
            .get("diff")
            .or_else(|| change.get("unified_diff"))
            .and_then(Value::as_str);
        let diff = match (kind, diff) {
            // An added file's "diff" is its content.
            ("add", Some(text)) if !text.starts_with("@@") && !text.starts_with("---") => {
                added_diff(path, text)
            }
            ("add", None) => added_diff(
                path,
                change.get("content").and_then(Value::as_str).unwrap_or(""),
            ),
            ("delete", None) => format!("--- a/{header}\n+++ /dev/null\n"),
            (_, Some(text)) if text.starts_with("@@") => {
                format!("--- a/{header}\n+++ b/{header}\n{text}")
            }
            (_, Some(text)) => text.to_string(),
            (_, None) => String::new(),
        };
        (path.to_string(), diff)
    };
    match item.get("changes") {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|change| {
                let path = change.get("path").and_then(Value::as_str)?;
                Some(one(path, change))
            })
            .collect(),
        Some(Value::Object(map)) => map.iter().map(|(path, change)| one(path, change)).collect(),
        _ => Vec::new(),
    }
}

fn item_started(item: &Value, cwd: Option<&str>, events: &mut Vec<ChatEvent>) {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match item.get("type").and_then(Value::as_str) {
        Some("commandExecution") => {
            let command = command_text(item);
            events.push(ChatEvent::Tool {
                id,
                kind: command_kind(item),
                name: "command".into(),
                title: format!("Run `{}`", one_line(&command, 80)),
                detail: clip_head(&command, DETAIL_LIMIT),
            });
        }
        Some("fileChange") => {
            let paths: Vec<String> = changes(item, cwd)
                .into_iter()
                .map(|(path, _)| path)
                .collect();
            events.push(ChatEvent::Tool {
                id,
                kind: ToolKind::Edit,
                name: "fileChange".into(),
                title: match paths.as_slice() {
                    [one] => format!("Edit {one}"),
                    _ => format!("Edit {} files", paths.len()),
                },
                detail: clip_head(&paths.join("\n"), DETAIL_LIMIT),
            });
        }
        Some("mcpToolCall") => {
            let tool = item.get("tool").and_then(Value::as_str).unwrap_or("tool");
            let server = item.get("server").and_then(Value::as_str).unwrap_or("");
            events.push(ChatEvent::Tool {
                id,
                kind: ToolKind::Other,
                name: tool.to_string(),
                title: if server.is_empty() {
                    tool.to_string()
                } else {
                    format!("{server}: {tool}")
                },
                detail: clip_head(
                    &serde_json::to_string_pretty(item.get("arguments").unwrap_or(&Value::Null))
                        .unwrap_or_default(),
                    DETAIL_LIMIT,
                ),
            });
        }
        Some("webSearch") => {
            let query = item.get("query").and_then(Value::as_str).unwrap_or("");
            events.push(ChatEvent::Tool {
                id,
                kind: ToolKind::Web,
                name: "webSearch".into(),
                title: format!("Search the web for {}", one_line(query, 80)),
                detail: clip_head(query, DETAIL_LIMIT),
            });
        }
        _ => {}
    }
}

fn item_completed(item: &Value, cwd: Option<&str>, events: &mut Vec<ChatEvent>) {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let status = item.get("status").and_then(Value::as_str);
    match item.get("type").and_then(Value::as_str) {
        Some("agentMessage") => events.push(ChatEvent::TextDone {
            id,
            text: item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        }),
        Some("commandExecution") => {
            let exit = item.get("exitCode").and_then(Value::as_i64);
            let output = item
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let output = match status {
                Some("declined") if output.is_empty() => "Declined.".to_string(),
                _ => output.to_string(),
            };
            events.push(ChatEvent::ToolResult {
                id,
                ok: status == Some("completed") && exit.is_none_or(|code| code == 0),
                output: clip_tail(&output, OUTPUT_LIMIT),
            });
        }
        Some("fileChange") => {
            for (path, diff) in changes(item, cwd) {
                events.push(ChatEvent::FileChange {
                    id: id.clone(),
                    path,
                    diff,
                });
            }
            events.push(ChatEvent::ToolResult {
                id,
                ok: status == Some("completed"),
                output: match status {
                    Some("declined") => "Declined.".into(),
                    _ => String::new(),
                },
            });
        }
        Some("mcpToolCall") | Some("webSearch") => {
            let output = item
                .get("error")
                .and_then(|error| error.get("message").or(Some(error)))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    item.get("result")
                        .map(|result| serde_json::to_string_pretty(result).unwrap_or_default())
                })
                .unwrap_or_default();
            events.push(ChatEvent::ToolResult {
                id,
                ok: status.is_none_or(|status| status == "completed"),
                output: clip_tail(&output, OUTPUT_LIMIT),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::fixtures::{inbound, outbound};

    fn client() -> CodexClient {
        CodexClient::new(Options {
            cwd: "/work".into(),
            ..Default::default()
        })
    }

    /// Replays a recording, answering each approval the way the recording
    /// did, and returns every event and every line sent.
    fn replay(name: &str, decision: Decision) -> (Vec<ChatEvent>, Vec<Value>) {
        let mut codex = client();
        let mut sent: Vec<String> = codex.start();
        let mut events = Vec::new();
        let recorded = outbound("codex", name);
        let prompt = recorded
            .iter()
            .find(|message| message["method"] == "turn/start")
            .unwrap()["params"]["input"][0]["text"]
            .as_str()
            .unwrap()
            .to_string();
        // Typed at once, before the thread exists: it waits.
        let (lines, started) = codex.send(&prompt, &[]);
        assert!(lines.is_empty());
        events.extend(started);
        for line in inbound("codex", name) {
            let produced = codex.feed(&line);
            sent.extend(codex.take_outgoing());
            for event in &produced {
                if let ChatEvent::Approval { request, .. } = event {
                    let (answer, resolved) = codex.answer(request, decision).unwrap();
                    sent.push(answer);
                    events.push(resolved);
                }
            }
            events.extend(produced);
        }
        let sent = sent
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        (events, sent)
    }

    fn methods(sent: &[Value]) -> Vec<String> {
        sent.iter()
            .map(|message| {
                message["method"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("answer {}", message["result"]["decision"]))
            })
            .collect()
    }

    #[test]
    fn the_handshake_and_answers_follow_the_recording() {
        let (events, sent) = replay("approval-allow", Decision::Allow);
        assert_eq!(
            methods(&sent),
            [
                "initialize",
                "initialized",
                "thread/start",
                "model/list",
                "turn/start",
                "answer \"accept\"",
                "answer \"accept\"",
                "answer \"accept\""
            ]
        );
        let recorded = outbound("codex", "approval-allow");
        assert_eq!(sent[2]["params"]["cwd"], recorded[2]["params"]["cwd"]);
        assert_eq!(sent[4]["params"], recorded[3]["params"]);
        // Answers go back under the server's own ids.
        let answers: Vec<&Value> = sent.iter().filter(|m| m.get("result").is_some()).collect();
        assert_eq!(
            answers.iter().map(|m| m["id"].clone()).collect::<Vec<_>>(),
            [json!(0), json!(1), json!(2)]
        );
        assert_eq!(answers[0], &recorded[4]);

        assert_eq!(
            events[0],
            ChatEvent::TurnStarted,
            "shown before the server says so"
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, ChatEvent::TurnStarted))
                .count(),
            1
        );
        assert!(events.contains(&ChatEvent::Session {
            provider_id: "01a12abd-ecc2-7de3-87f6-70db6a6aae51".into()
        }));
        assert!(
            !events.iter().any(|e| matches!(e, ChatEvent::User { .. })),
            "the echo comes from the sender, not the server"
        );
        let writing = events
            .iter()
            .find(|e| matches!(e, ChatEvent::Approval { title, .. } if title.contains("codex.txt")))
            .unwrap();
        let ChatEvent::Approval {
            request,
            kind,
            title,
            detail,
            always,
        } = writing
        else {
            unreachable!()
        };
        assert_eq!(request, "1");
        assert_eq!(*kind, ApprovalKind::Command);
        assert_eq!(title, "Run `printf 'hi\\n' > codex.txt`");
        assert!(detail.contains("May I create the requested codex.txt file"));
        assert!(*always);
        assert!(events.contains(&ChatEvent::ApprovalResolved {
            request: "1".into()
        }));
        assert!(events.contains(&ChatEvent::ToolResult {
            id: "exec-172238b3-332b-4126-97ad-8e22529eec1f".into(),
            ok: true,
            output: String::new()
        }));
        assert!(events.contains(&ChatEvent::TextDone {
            id: "msg_0828d49afb230af8016acb744b527887d281ab82ca95ad7995".into(),
            text: "done".into()
        }));
        assert!(events.contains(&ChatEvent::Text {
            id: "msg_0828d49afb230af8016acb744b527887d281ab82ca95ad7995".into(),
            delta: "done".into()
        }));
        // The sum of the turn's four model calls.
        assert_eq!(
            events.last().unwrap(),
            &ChatEvent::TurnEnded {
                usage: Usage {
                    input: (18990 - 12288) + (19927 - 18816) + (20079 - 12288) + (20167 - 19840),
                    output: 83 + 95 + 30 + 5,
                    cache_read: 12288 + 18816 + 12288 + 19840,
                    cache_write: 0
                },
                error: None
            }
        );
    }

    #[test]
    fn a_declined_command_fails_and_the_agent_says_it_was_blocked() {
        let (events, sent) = replay("approval-deny", Decision::Deny);
        let recorded = outbound("codex", "approval-deny");
        let answer = sent.iter().find(|m| m.get("result").is_some()).unwrap();
        assert_eq!(answer, &recorded[4]);
        assert!(events.contains(&ChatEvent::ToolResult {
            id: "exec-5763548d-f6c2-484a-82de-62834f2db3a2".into(),
            ok: false,
            output: "Declined.".into()
        }));
        assert!(events
            .iter()
            .any(|e| matches!(e, ChatEvent::TextDone { text, .. } if text == "blocked")));
        assert!(matches!(
            events.last(),
            Some(ChatEvent::TurnEnded { error: None, .. })
        ));
    }

    #[test]
    fn always_uses_the_amendment_the_server_offered() {
        let mut codex = client();
        let events = codex.feed(
            &json!({
                "id": 7, "method": "item/commandExecution/requestApproval",
                "params": {
                    "command": "/bin/zsh -lc 'npm test'",
                    "commandActions": [{ "type": "unknown", "command": "npm test" }],
                    "proposedExecpolicyAmendment": ["npm", "test"],
                    "availableDecisions": ["accept", { "acceptWithExecpolicyAmendment": { "execpolicy_amendment": ["npm", "test"] } }, "cancel"]
                }
            })
            .to_string(),
        );
        assert!(matches!(
            &events[0],
            ChatEvent::Approval { always: true, .. }
        ));
        let (line, _) = codex.answer("7", Decision::Always).unwrap();
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            value["result"]["decision"]["acceptWithExecpolicyAmendment"]["execpolicy_amendment"],
            json!(["npm", "test"])
        );

        codex.feed(
            &json!({ "id": 8, "method": "item/fileChange/requestApproval", "params": { "availableDecisions": ["accept", "acceptForSession", "decline"] } })
                .to_string(),
        );
        let (line, _) = codex.answer("8", Decision::Always).unwrap();
        assert!(line.contains("\"acceptForSession\""));
        assert!(codex.answer("8", Decision::Always).is_err());
    }

    #[test]
    fn resume_interrupt_and_unsupported_requests() {
        let mut codex = CodexClient::new(Options {
            cwd: "/work".into(),
            resume: Some("thread-1".into()),
            model: Some("gpt-5".into()),
            effort: Some("high".into()),
            yolo: true,
            developer_instructions: None,
        });
        codex.start();
        codex.feed(r#"{"id":1,"result":{}}"#);
        let out = codex.take_outgoing();
        let thread: Value = serde_json::from_str(&out[1]).unwrap();
        assert_eq!(thread["method"], "thread/resume");
        assert_eq!(thread["params"]["threadId"], "thread-1");
        assert_eq!(thread["params"]["model"], "gpt-5");
        assert_eq!(thread["params"]["approvalPolicy"], "never");

        assert!(codex.interrupt().is_none(), "nothing runs yet");
        codex.feed(r#"{"id":11,"result":{"thread":{"id":"thread-1"}}}"#);
        assert_eq!(codex.thread_id(), Some("thread-1"));
        let (lines, _) = codex.send("go", &[]);
        let turn: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(
            turn["params"]["effort"], "high",
            "each turn asks for the effort"
        );
        codex.feed(
            &json!({ "id": turn["id"], "result": { "turn": { "id": "turn-9" } } }).to_string(),
        );
        let interrupt: Value = serde_json::from_str(&codex.interrupt().unwrap()).unwrap();
        assert_eq!(interrupt["method"], "turn/interrupt");
        assert_eq!(interrupt["params"]["turnId"], "turn-9");

        let events = codex.feed(r#"{"id":"q1","method":"item/tool/requestUserInput","params":{}}"#);
        assert!(events.is_empty());
        let refusal: Value = serde_json::from_str(&codex.take_outgoing()[0]).unwrap();
        assert_eq!(refusal["id"], "q1");
        assert!(refusal.get("error").is_some());
    }

    #[test]
    fn file_changes_become_diffs() {
        let item = json!({ "type": "fileChange", "id": "f", "status": "completed", "changes": [
            { "path": "a.txt", "kind": { "type": "add" }, "diff": "hi\n" },
            { "path": "/work/b.txt", "kind": { "type": "update", "move_path": null }, "diff": "@@ -1 +1 @@\n-a\n+b\n" }
        ]});
        let mut events = Vec::new();
        item_completed(&item, Some("/work"), &mut events);
        assert_eq!(
            events,
            [
                ChatEvent::FileChange {
                    id: "f".into(),
                    path: "a.txt".into(),
                    diff: "--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1,1 @@\n+hi\n".into()
                },
                ChatEvent::FileChange {
                    id: "f".into(),
                    path: "b.txt".into(),
                    diff: "--- a/b.txt\n+++ b/b.txt\n@@ -1 +1 @@\n-a\n+b\n".into()
                },
                ChatEvent::ToolResult {
                    id: "f".into(),
                    ok: true,
                    output: String::new()
                }
            ]
        );
    }

    /// `model/list` as Codex 0.162 answers it (trimmed).
    const MODEL_LIST_RESULT: &str = r#"{"id":2,"result":{"data":[
        {"id":"gpt-6.1-sol","model":"gpt-6.1-sol","displayName":"GPT-6.1-Sol","description":"","hidden":false,"isDefault":true,"defaultReasoningEffort":"low",
         "supportedReasoningEfforts":[{"reasoningEffort":"low","description":""},{"reasoningEffort":"high","description":""},{"reasoningEffort":"ultra","description":""}]},
        {"id":"gpt-6-luna","model":"gpt-6-luna","displayName":"GPT-6-Luna","description":"","hidden":false,"isDefault":false,"defaultReasoningEffort":"medium",
         "supportedReasoningEfforts":[{"reasoningEffort":"medium","description":""},{"reasoningEffort":"Bad Level","description":""}]},
        {"id":"secret","model":"secret","displayName":"Hidden","description":"","hidden":true,"isDefault":false,"defaultReasoningEffort":"low","supportedReasoningEfforts":[]}
    ],"nextCursor":null}}"#;

    #[test]
    fn the_models_are_listed_after_the_handshake() {
        let mut codex = CodexClient::new(Options {
            cwd: "/work".into(),
            model: Some("gpt-6-luna".into()),
            effort: Some("medium".into()),
            ..Default::default()
        });
        codex.start();
        codex.feed(r#"{"id":1,"result":{}}"#);
        let out: Vec<Value> = codex
            .take_outgoing()
            .iter()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            out[2],
            json!({ "id": 2, "method": "model/list", "params": {} })
        );

        let events = codex.feed(&MODEL_LIST_RESULT.replace('\n', ""));
        let [ChatEvent::Models {
            models,
            model,
            effort,
        }] = events.as_slice()
        else {
            panic!("{events:?}")
        };
        assert_eq!(model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(effort.as_deref(), Some("medium"));
        assert_eq!(
            models,
            &[
                ChatModel {
                    id: "gpt-6.1-sol".into(),
                    name: "GPT-6.1-Sol".into(),
                    efforts: vec!["low".into(), "high".into(), "ultra".into()],
                    default_effort: Some("low".into()),
                    is_default: true,
                },
                ChatModel {
                    id: "gpt-6-luna".into(),
                    name: "GPT-6-Luna".into(),
                    efforts: vec!["medium".into()],
                    default_effort: Some("medium".into()),
                    is_default: false,
                },
            ]
        );

        // A further page is asked for; the list is reported once whole.
        let mut codex = client();
        let events = codex.feed(r#"{"id":2,"result":{"data":[],"nextCursor":"c2"}}"#);
        assert!(events.is_empty());
        let next: Value = serde_json::from_str(&codex.take_outgoing()[0]).unwrap();
        assert_eq!(next["params"], json!({ "cursor": "c2" }));
        // An old server without the method: nothing shown, nothing ended.
        assert!(codex
            .feed(r#"{"id":2,"error":{"code":-32601,"message":"unknown method"}}"#)
            .is_empty());
    }

    #[test]
    fn a_switch_applies_from_the_next_turn() {
        let mut codex = client();
        codex.start();
        codex.feed(r#"{"id":1,"result":{}}"#);
        codex.feed(&MODEL_LIST_RESULT.replace('\n', ""));
        codex.feed(r#"{"id":11,"result":{"thread":{"id":"t"}}}"#);
        codex.take_outgoing();
        let turn = |codex: &mut CodexClient| -> Value {
            let (lines, _) = codex.send("go", &[]);
            codex.feed(
                r#"{"method":"turn/completed","params":{"turn":{"id":"x","status":"completed"}}}"#,
            );
            serde_json::from_str(&lines[0]).unwrap()
        };
        let first = turn(&mut codex);
        assert!(
            first["params"].get("model").is_none(),
            "nothing chosen, nothing sent"
        );
        assert!(first["params"].get("effort").is_none());

        codex.set_model(Some("gpt-6-luna"));
        codex.set_effort(Some("medium"));
        let second = turn(&mut codex);
        assert_eq!(second["params"]["model"], "gpt-6-luna");
        assert_eq!(second["params"]["effort"], "medium");

        // Back to the defaults: spelled out, since the override lasts.
        codex.set_model(None);
        codex.set_effort(None);
        let third = turn(&mut codex);
        assert_eq!(third["params"]["model"], "gpt-6.1-sol");
        assert_eq!(third["params"]["effort"], "low");
    }
}
