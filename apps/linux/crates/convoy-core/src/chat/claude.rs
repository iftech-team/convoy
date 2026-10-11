//! Claude Code in stream-json mode.
//!
//! The CLI is started with JSON on both pipes. Each user turn is one line in;
//! out comes one line per event — partial-message deltas, whole messages,
//! permission questions as control requests, and a `result` closing the turn.
//! The conversation is the same `.jsonl` a terminal session writes, so
//! `--resume` works from either side.

use super::event::{
    added_diff, clip_head, clip_tail, one_line, replacement_diff, short_path, ApprovalKind,
    ChatEvent, Decision, Image, ToolKind, Usage, DETAIL_LIMIT, OUTPUT_LIMIT,
};
use crate::{bail, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

/// The flags that put Claude Code on JSON pipes with permission questions
/// sent to us rather than drawn on a screen.
pub const CHAT_FLAGS: &[&str] = &[
    "--input-format",
    "stream-json",
    "--output-format",
    "stream-json",
    "--verbose",
    "--include-partial-messages",
    "--permission-prompt-tool",
    "stdio",
];

/// What a denied tool call tells the model. Short, because the model reads
/// it and decides what to do next.
pub const DENIED: &str = "The user declined this.";

/// The argument vector after `claude`: the chat flags, the exact
/// conversation, then the same model, settings and permission flags a
/// terminal launch passes.
pub fn args(
    provider_id: &str,
    resume: bool,
    model: Option<&str>,
    settings_file: Option<&str>,
    extra: &[String],
) -> Vec<String> {
    let mut args: Vec<String> = CHAT_FLAGS.iter().map(|flag| flag.to_string()).collect();
    args.push(if resume { "--resume" } else { "--session-id" }.into());
    args.push(provider_id.to_string());
    if let Some(model) = model.filter(|model| !model.is_empty()) {
        args.push("--model".into());
        args.push(model.to_string());
    }
    if let Some(file) = settings_file.filter(|file| !file.is_empty()) {
        args.push("--settings".into());
        args.push(file.to_string());
    }
    args.extend(extra.iter().cloned());
    args
}

/// The first line sent. No hooks are registered over the pipe: Convoy's own
/// hooks already reach the agent through `--settings`.
pub fn initialize() -> String {
    json!({
        "type": "control_request",
        "request_id": "init",
        "request": { "subtype": "initialize", "hooks": {} }
    })
    .to_string()
}

/// A user turn. Plain text goes as a string, as the CLI itself sends it;
/// with images it becomes content blocks, images first.
pub fn user(session_id: &str, text: &str, images: &[Image]) -> String {
    let content = if images.is_empty() {
        Value::String(text.to_string())
    } else {
        let mut blocks: Vec<Value> = images
            .iter()
            .map(|image| {
                json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": image.media_type, "data": image.data }
                })
            })
            .collect();
        if !text.is_empty() {
            blocks.push(json!({ "type": "text", "text": text }));
        }
        Value::Array(blocks)
    };
    json!({
        "type": "user",
        "message": { "role": "user", "content": content },
        "parent_tool_use_id": null,
        "session_id": session_id
    })
    .to_string()
}

/// Stops the current turn; the process stays.
pub fn interrupt(request_id: &str) -> String {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": { "subtype": "interrupt" }
    })
    .to_string()
}

/// The answer to a `can_use_tool` question. Allowing echoes the input back
/// unchanged; "always" also accepts the rules Claude suggested, which is
/// exactly what its own "don't ask again" choice does.
pub fn answer(
    request_id: &str,
    decision: Decision,
    input: &Value,
    suggestions: Option<&Value>,
) -> String {
    let response = match decision {
        Decision::Deny => json!({ "behavior": "deny", "message": DENIED }),
        Decision::Allow => json!({ "behavior": "allow", "updatedInput": input }),
        Decision::Always => {
            let mut response = json!({ "behavior": "allow", "updatedInput": input });
            if let Some(suggestions) = suggestions.filter(|value| !is_empty(value)) {
                response["updatedPermissions"] = suggestions.clone();
            }
            response
        }
    };
    json!({
        "type": "control_response",
        "response": { "subtype": "success", "request_id": request_id, "response": response }
    })
    .to_string()
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

/// A question waiting for the user, with what answering it needs.
#[derive(Debug, Clone)]
struct Pending {
    input: Value,
    suggestions: Option<Value>,
}

/// A content block being streamed.
#[derive(Debug, Clone)]
struct Block {
    id: String,
    thinking: bool,
    text: String,
    done: bool,
}

/// Claude's output, turned into [`ChatEvent`]s. Stateful because a message
/// arrives in pieces: deltas first, then the whole message, and the window
/// must be told both belong to the same bubble.
#[derive(Debug, Default)]
pub struct ClaudeStream {
    session_id: String,
    cwd: Option<String>,
    ready: bool,
    in_turn: bool,
    announced: bool,
    message: Option<String>,
    blocks: BTreeMap<u64, Block>,
    unstreamed: u64,
    pending: HashMap<String, Pending>,
    outbox: Vec<String>,
    requests: u64,
}

impl ClaudeStream {
    pub fn new(session_id: impl Into<String>) -> Self {
        ClaudeStream {
            session_id: session_id.into(),
            ..Default::default()
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Whether Claude has answered `initialize`.
    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn in_turn(&self) -> bool {
        self.in_turn
    }

    /// Lines to write back that reading produced: refusals of requests
    /// Convoy does not handle, so Claude is never left waiting on one.
    pub fn take_outgoing(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    /// A user turn, and `turn_started` if the agent was idle — at once,
    /// rather than seconds later when the first token arrives.
    pub fn send(&mut self, text: &str, images: &[Image]) -> (String, Vec<ChatEvent>) {
        let mut events = Vec::new();
        if !self.in_turn {
            self.in_turn = true;
            events.push(ChatEvent::TurnStarted);
        }
        (user(&self.session_id, text, images), events)
    }

    pub fn interrupt(&mut self) -> String {
        self.requests += 1;
        interrupt(&format!("interrupt-{}", self.requests))
    }

    /// Answers a pending question. Returns the line and `approval_resolved`.
    pub fn answer(&mut self, request: &str, decision: Decision) -> Result<(String, ChatEvent)> {
        let Some(pending) = self.pending.remove(request) else {
            bail!("This request was already answered.")
        };
        Ok((
            answer(
                request,
                decision,
                &pending.input,
                pending.suggestions.as_ref(),
            ),
            ChatEvent::ApprovalResolved {
                request: request.to_string(),
            },
        ))
    }

    /// One line of Claude's output.
    pub fn feed(&mut self, line: &str) -> Vec<ChatEvent> {
        let Ok(message) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        match message.get("type").and_then(Value::as_str) {
            Some("system") => self.system(&message, &mut events),
            Some("stream_event") if top_level(&message) => self.stream(&message, &mut events),
            Some("assistant") if top_level(&message) => self.assistant(&message, &mut events),
            Some("user") if top_level(&message) => self.tool_results(&message, &mut events),
            Some("control_request") => self.control_request(&message, &mut events),
            Some("control_response") => self.control_response(&message, &mut events),
            Some("control_cancel_request") => {
                if let Some(id) = message.get("request_id").and_then(Value::as_str) {
                    if self.pending.remove(id).is_some() {
                        events.push(ChatEvent::ApprovalResolved {
                            request: id.to_string(),
                        });
                    }
                }
            }
            Some("result") => self.result(&message, &mut events),
            _ => {}
        }
        events
    }

    fn system(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        if message.get("subtype").and_then(Value::as_str) != Some("init") {
            return;
        }
        if let Some(cwd) = message.get("cwd").and_then(Value::as_str) {
            self.cwd = Some(cwd.to_string());
        }
        if let Some(id) = message.get("session_id").and_then(Value::as_str) {
            if !self.announced {
                self.announced = true;
                self.session_id = id.to_string();
                events.push(ChatEvent::Session {
                    provider_id: id.to_string(),
                });
            }
        }
    }

    fn begin(&mut self, events: &mut Vec<ChatEvent>) {
        if !self.in_turn {
            self.in_turn = true;
            events.push(ChatEvent::TurnStarted);
        }
    }

    fn stream(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        let Some(event) = message.get("event") else {
            return;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                self.begin(events);
                self.message = event
                    .pointer("/message/id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.blocks.clear();
            }
            Some("content_block_start") => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let kind = event
                    .pointer("/content_block/type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if kind == "text" || kind == "thinking" {
                    let message = self.message.clone().unwrap_or_default();
                    self.blocks.insert(
                        index,
                        Block {
                            id: format!("{message}:{index}"),
                            thinking: kind == "thinking",
                            text: String::new(),
                            done: false,
                        },
                    );
                }
            }
            Some("content_block_delta") => {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0);
                let Some(block) = self.blocks.get_mut(&index) else {
                    return;
                };
                let delta = event.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        block.text.push_str(text);
                        events.push(ChatEvent::Text {
                            id: block.id.clone(),
                            delta: text.to_string(),
                        });
                    }
                    Some("thinking_delta") => {
                        let text = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        block.text.push_str(text);
                        events.push(ChatEvent::Thinking {
                            id: block.id.clone(),
                            delta: text.to_string(),
                        });
                    }
                    _ => {}
                }
            }
            // A message whose whole form never came still ends with its
            // text in place.
            Some("message_stop") => {
                for block in self.blocks.values_mut() {
                    if !block.thinking && !block.done {
                        block.done = true;
                        events.push(ChatEvent::TextDone {
                            id: block.id.clone(),
                            text: block.text.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// A whole assistant message. With partial messages on, Claude sends one
    /// per content block, after that block's deltas; its text replaces what
    /// the deltas built, under the same id.
    fn assistant(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        self.begin(events);
        let id = message
            .pointer("/message/id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let streamed = self.message.as_deref() == Some(id.as_str());
        let Some(content) = message
            .pointer("/message/content")
            .and_then(Value::as_array)
        else {
            return;
        };
        for item in content {
            match item.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = item.get("text").and_then(Value::as_str).unwrap_or("");
                    let block = streamed
                        .then(|| {
                            self.blocks
                                .values_mut()
                                .find(|block| !block.thinking && !block.done)
                        })
                        .flatten();
                    let block_id = match block {
                        Some(block) => {
                            block.done = true;
                            block.id.clone()
                        }
                        None => {
                            self.unstreamed += 1;
                            format!("{id}:whole-{}", self.unstreamed)
                        }
                    };
                    events.push(ChatEvent::TextDone {
                        id: block_id,
                        text: text.to_string(),
                    });
                }
                Some("thinking") => {
                    let seen = streamed
                        && self
                            .blocks
                            .values()
                            .any(|block| block.thinking && !block.text.is_empty());
                    let text = item.get("thinking").and_then(Value::as_str).unwrap_or("");
                    if !seen && !text.is_empty() {
                        self.unstreamed += 1;
                        events.push(ChatEvent::Thinking {
                            id: format!("{id}:thinking-{}", self.unstreamed),
                            delta: text.to_string(),
                        });
                    }
                }
                Some("tool_use") => {
                    tool_events(item, self.cwd.as_deref(), events);
                }
                _ => {}
            }
        }
    }

    fn tool_results(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        let Some(content) = message
            .pointer("/message/content")
            .and_then(Value::as_array)
        else {
            return;
        };
        for item in content {
            if let Some(event) = tool_result(item) {
                events.push(event);
            }
        }
    }

    fn control_request(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        let request_id = message
            .get("request_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let request = message.get("request").unwrap_or(&Value::Null);
        if request.get("subtype").and_then(Value::as_str) != Some("can_use_tool") {
            // Hooks and MCP over the pipe are not registered, so nothing else
            // should arrive; if it does, Claude is told rather than left hanging.
            self.outbox.push(
                json!({
                    "type": "control_response",
                    "response": {
                        "subtype": "error",
                        "request_id": request_id,
                        "error": "Not supported by Convoy."
                    }
                })
                .to_string(),
            );
            return;
        }
        let name = request
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or("tool");
        let input = request.get("input").cloned().unwrap_or(Value::Null);
        let suggestions = request.get("permission_suggestions").cloned();
        let (kind, title, detail) = describe(name, &input, self.cwd.as_deref());
        let always = suggestions.as_ref().is_some_and(|value| !is_empty(value));
        self.pending
            .insert(request_id.clone(), Pending { input, suggestions });
        events.push(ChatEvent::Approval {
            request: request_id,
            kind: match kind {
                ToolKind::Command => ApprovalKind::Command,
                ToolKind::Edit => ApprovalKind::Edit,
                _ => ApprovalKind::Tool,
            },
            title,
            detail,
            always,
        });
    }

    fn control_response(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        let response = message.get("response").unwrap_or(&Value::Null);
        if response.get("request_id").and_then(Value::as_str) != Some("init") {
            return;
        }
        if response.get("subtype").and_then(Value::as_str) == Some("error") {
            events.push(ChatEvent::Error {
                message: response
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Claude Code could not start the conversation.")
                    .to_string(),
            });
        } else {
            self.ready = true;
        }
    }

    fn result(&mut self, message: &Value, events: &mut Vec<ChatEvent>) {
        // A question the turn no longer waits for is closed with it.
        for request in self.pending.drain().map(|(id, _)| id) {
            events.push(ChatEvent::ApprovalResolved { request });
        }
        self.in_turn = false;
        let usage = message.get("usage").unwrap_or(&Value::Null);
        let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
        let failed = message
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let error = failed.then(|| {
            let text = message
                .get("result")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty());
            match (text, message.get("subtype").and_then(Value::as_str)) {
                (Some(text), _) => text.to_string(),
                // What an interrupt ends a turn with.
                (None, Some("error_during_execution")) => "Interrupted.".to_string(),
                (None, Some("error_max_turns")) => "Stopped at the turn limit.".to_string(),
                _ => "The turn failed.".to_string(),
            }
        });
        events.push(ChatEvent::TurnEnded {
            usage: Usage {
                input: count("input_tokens"),
                output: count("output_tokens"),
                cache_read: count("cache_read_input_tokens"),
                cache_write: count("cache_creation_input_tokens"),
            },
            error,
        });
    }
}

/// A subagent's messages carry the tool call that started it; only the
/// conversation itself is shown, with the subagent as one tool card.
fn top_level(message: &Value) -> bool {
    message.get("parent_tool_use_id").is_none_or(Value::is_null)
}

/// `tool`, and `file_change` for a tool that edits, from a `tool_use` block.
pub(crate) fn tool_events(item: &Value, cwd: Option<&str>, events: &mut Vec<ChatEvent>) {
    let id = item
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = item.get("name").and_then(Value::as_str).unwrap_or("tool");
    let input = item.get("input").unwrap_or(&Value::Null);
    let (kind, title, detail) = describe(name, input, cwd);
    events.push(ChatEvent::Tool {
        id: id.clone(),
        kind,
        name: name.to_string(),
        title,
        detail,
    });
    events.extend(file_changes(&id, name, input, cwd));
}

/// `tool_result` from a result block in a user message.
pub(crate) fn tool_result(item: &Value) -> Option<ChatEvent> {
    if item.get("type").and_then(Value::as_str) != Some("tool_result") {
        return None;
    }
    let output = match item.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    Some(ChatEvent::ToolResult {
        id: item
            .get("tool_use_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        ok: !item
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        output: clip_tail(&output, OUTPUT_LIMIT),
    })
}

/// The card for one of Claude's tools: its kind, a short title, and the full
/// command or input.
pub(crate) fn describe(name: &str, input: &Value, cwd: Option<&str>) -> (ToolKind, String, String) {
    let field = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or("");
    let path = || short_path(field("file_path"), cwd);
    let json = || {
        clip_head(
            &serde_json::to_string_pretty(input).unwrap_or_default(),
            DETAIL_LIMIT,
        )
    };
    match name {
        "Bash" => (
            ToolKind::Command,
            format!("Run `{}`", one_line(field("command"), 80)),
            clip_head(field("command"), DETAIL_LIMIT),
        ),
        "Edit" | "MultiEdit" => (ToolKind::Edit, format!("Edit {}", path()), json()),
        "Write" => (ToolKind::Edit, format!("Write {}", path()), json()),
        "NotebookEdit" => (
            ToolKind::Edit,
            format!("Edit {}", short_path(field("notebook_path"), cwd)),
            json(),
        ),
        "Read" => (ToolKind::Read, format!("Read {}", path()), json()),
        "Glob" | "Grep" => (
            ToolKind::Search,
            format!("Search {}", one_line(field("pattern"), 80)),
            json(),
        ),
        "WebFetch" => (
            ToolKind::Web,
            format!("Fetch {}", one_line(field("url"), 80)),
            json(),
        ),
        "WebSearch" => (
            ToolKind::Web,
            format!("Search the web for {}", one_line(field("query"), 80)),
            json(),
        ),
        "Task" | "Agent" => (
            ToolKind::Task,
            one_line(
                Some(field("description"))
                    .filter(|text| !text.is_empty())
                    .unwrap_or(name),
                80,
            ),
            json(),
        ),
        _ => (ToolKind::Other, name.to_string(), json()),
    }
}

/// The diffs an editing tool's input describes. Claude sends the snippet it
/// replaces, not the file, so an edit is shown as that snippet changing and
/// a write as every line added.
pub(crate) fn file_changes(
    id: &str,
    name: &str,
    input: &Value,
    cwd: Option<&str>,
) -> Vec<ChatEvent> {
    let field = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let path = short_path(&field(input, "file_path"), cwd);
    if path.is_empty() {
        return Vec::new();
    }
    let diff = match name {
        "Edit" => replacement_diff(
            &path,
            &field(input, "old_string"),
            &field(input, "new_string"),
        ),
        "MultiEdit" => input
            .get("edits")
            .and_then(Value::as_array)
            .map(|edits| {
                edits
                    .iter()
                    .map(|edit| {
                        replacement_diff(
                            &path,
                            &field(edit, "old_string"),
                            &field(edit, "new_string"),
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default(),
        "Write" => added_diff(&path, &field(input, "content")),
        _ => return Vec::new(),
    };
    vec![ChatEvent::FileChange {
        id: id.to_string(),
        path,
        diff,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::fixtures::{inbound, outbound};

    /// Replays a recording, answering its question as the recording did.
    fn replay(name: &str, decision: Decision) -> Vec<ChatEvent> {
        let mut stream = ClaudeStream::new("ignored");
        let mut events = Vec::new();
        for line in inbound("claude", name) {
            for event in stream.feed(&line) {
                let request = match &event {
                    ChatEvent::Approval { request, .. } => Some(request.clone()),
                    _ => None,
                };
                events.push(event);
                if let Some(request) = request {
                    events.push(stream.answer(&request, decision).unwrap().1);
                }
            }
            assert!(stream.take_outgoing().is_empty());
        }
        events
    }

    /// The shape of the sequence, without the text that varies.
    fn outline(events: &[ChatEvent]) -> Vec<&'static str> {
        let mut outline: Vec<&'static str> = Vec::new();
        for event in events {
            let name = match event {
                ChatEvent::Session { .. } => "session",
                ChatEvent::User { .. } => "user",
                ChatEvent::TurnStarted => "turn_started",
                ChatEvent::Text { .. } => "text",
                ChatEvent::TextDone { .. } => "text_done",
                ChatEvent::Thinking { .. } => "thinking",
                ChatEvent::Tool { .. } => "tool",
                ChatEvent::ToolResult { .. } => "tool_result",
                ChatEvent::FileChange { .. } => "file_change",
                ChatEvent::Approval { .. } => "approval",
                ChatEvent::ApprovalResolved { .. } => "approval_resolved",
                ChatEvent::TurnEnded { .. } => "turn_ended",
                ChatEvent::Error { .. } => "error",
            };
            // Runs of deltas are one step.
            if !(name == "text" && outline.last() == Some(&"text")) {
                outline.push(name);
            }
        }
        outline
    }

    #[test]
    fn an_allowed_write_runs_and_the_turn_ends_with_usage() {
        let events = replay("approval-allow", Decision::Allow);
        assert_eq!(
            outline(&events),
            [
                "session",
                "turn_started",
                "tool",
                "file_change",
                "approval",
                "approval_resolved",
                "tool_result",
                "text",
                "text_done",
                "turn_ended"
            ]
        );
        assert!(events.contains(&ChatEvent::Session {
            provider_id: "4312e440-964b-4587-84a4-4f34131ee8e1".into()
        }));
        let tool = events
            .iter()
            .find(|event| matches!(event, ChatEvent::Tool { .. }))
            .unwrap();
        let ChatEvent::Tool {
            id, kind, title, ..
        } = tool
        else {
            unreachable!()
        };
        assert_eq!(id, "toolu_01RwxeG9m5v6LFabQx7THQaw");
        assert_eq!(*kind, ToolKind::Edit);
        assert_eq!(
            title, "Write hello.txt",
            "paths are shown inside the folder"
        );
        assert!(events.contains(&ChatEvent::FileChange {
            id: "toolu_01RwxeG9m5v6LFabQx7THQaw".into(),
            path: "hello.txt".into(),
            diff: "--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1,1 @@\n+hi\n".into()
        }));
        assert!(events.contains(&ChatEvent::Approval {
            request: "d98d7c0d-c2a9-491c-98b9-04227e51788b".into(),
            kind: ApprovalKind::Edit,
            title: "Write hello.txt".into(),
            detail: "{\n  \"file_path\": \"/work/hello.txt\",\n  \"content\": \"hi\\n\"\n}".into(),
            always: true,
        }));
        assert!(matches!(
            events
                .iter()
                .find(|e| matches!(e, ChatEvent::ToolResult { .. })),
            Some(ChatEvent::ToolResult { ok: true, .. })
        ));
        // Deltas and the finished message share one id.
        let ids: Vec<&String> = events
            .iter()
            .filter_map(|event| match event {
                ChatEvent::Text { id, .. } | ChatEvent::TextDone { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        assert!(ids.iter().all(|id| *id == ids[0]));
        assert!(events.contains(&ChatEvent::TextDone {
            id: "msg_011CfvNtpJJ1N3CdgbRR9Jxh:0".into(),
            text: "done".into()
        }));
        assert_eq!(
            events.last().unwrap(),
            &ChatEvent::TurnEnded {
                usage: Usage {
                    input: 4,
                    output: 147,
                    cache_read: 41240,
                    cache_write: 13723
                },
                error: None
            }
        );
    }

    #[test]
    fn a_denied_write_fails_and_the_agent_says_it_was_blocked() {
        let events = replay("approval-deny", Decision::Deny);
        let result = events
            .iter()
            .find(|event| matches!(event, ChatEvent::ToolResult { .. }))
            .unwrap();
        assert_eq!(
            result,
            &ChatEvent::ToolResult {
                id: "toolu_016WMzafxBA7mqkRBGbjdek1".into(),
                ok: false,
                output: DENIED.into()
            }
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, ChatEvent::TextDone { text, .. } if text == "blocked")));
        assert!(matches!(
            events.last(),
            Some(ChatEvent::TurnEnded { error: None, .. })
        ));
    }

    /// What we send is what the recording sent: the same initialize, the same
    /// user line, and the same answers to the same questions.
    #[test]
    fn encoders_match_the_recorded_traffic() {
        for (name, decision) in [
            ("approval-allow", Decision::Allow),
            ("approval-deny", Decision::Deny),
        ] {
            let sent = outbound("claude", name);
            let session = sent[1]["session_id"].as_str().unwrap().to_string();
            let mut stream = ClaudeStream::new(session.clone());
            assert_eq!(
                serde_json::from_str::<Value>(&initialize()).unwrap(),
                sent[0]
            );
            let text = sent[1]["message"]["content"].as_str().unwrap();
            let (line, events) = stream.send(text, &[]);
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), sent[1]);
            assert_eq!(events, [ChatEvent::TurnStarted]);

            let mut request = None;
            for line in inbound("claude", name) {
                for event in stream.feed(&line) {
                    if let ChatEvent::Approval { request: id, .. } = event {
                        request = Some(id);
                    }
                }
                if let Some(id) = request.take() {
                    let (answer, resolved) = stream.answer(&id, decision).unwrap();
                    assert_eq!(serde_json::from_str::<Value>(&answer).unwrap(), sent[2]);
                    assert_eq!(
                        resolved,
                        ChatEvent::ApprovalResolved {
                            request: id.clone()
                        }
                    );
                    assert!(
                        stream.answer(&id, decision).is_err(),
                        "a question is answered once"
                    );
                }
            }
        }
    }

    #[test]
    fn always_accepts_the_suggested_rules_and_images_go_first() {
        let line = answer(
            "r",
            Decision::Always,
            &json!({ "command": "ls" }),
            Some(&json!([{ "type": "setMode", "mode": "acceptEdits", "destination": "session" }])),
        );
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            value["response"]["response"]["updatedPermissions"][0]["mode"],
            "acceptEdits"
        );
        assert_eq!(value["response"]["response"]["behavior"], "allow");

        let line = user(
            "s",
            "what is this?",
            &[Image {
                media_type: "image/png".into(),
                data: "AAAA".into(),
            }],
        );
        let value: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["message"]["content"][0]["type"], "image");
        assert_eq!(
            value["message"]["content"][0]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(value["message"]["content"][1]["text"], "what is this?");

        let value: Value = serde_json::from_str(&interrupt("x")).unwrap();
        assert_eq!(value["request"]["subtype"], "interrupt");
    }

    #[test]
    fn the_argument_vector_resumes_exactly_and_keeps_terminal_flags() {
        let args = args(
            "id-1",
            true,
            Some("sonnet"),
            Some("/s.json"),
            &["--dangerously-skip-permissions".to_string()],
        );
        assert_eq!(&args[..CHAT_FLAGS.len()], CHAT_FLAGS);
        assert_eq!(
            &args[CHAT_FLAGS.len()..],
            [
                "--resume",
                "id-1",
                "--model",
                "sonnet",
                "--settings",
                "/s.json",
                "--dangerously-skip-permissions"
            ]
        );
        assert_eq!(
            super::args("id-2", false, None, None, &[])[CHAT_FLAGS.len()..],
            ["--session-id", "id-2"]
        );
    }

    /// A question still open when the turn ends is closed with it.
    #[test]
    fn an_unanswered_question_is_resolved_when_the_turn_ends() {
        let mut stream = ClaudeStream::new("s");
        let mut events = Vec::new();
        for line in inbound("claude", "approval-allow") {
            events.extend(stream.feed(&line));
        }
        let at = events
            .iter()
            .position(|event| matches!(event, ChatEvent::ApprovalResolved { .. }))
            .unwrap();
        assert!(matches!(events[at + 1], ChatEvent::TurnEnded { .. }));
    }

    #[test]
    fn unsupported_requests_are_refused_and_unknown_lines_ignored() {
        let mut stream = ClaudeStream::new("s");
        assert!(stream.feed("zsh: no job control").is_empty());
        assert!(stream
            .feed(r#"{"type":"control_request","request_id":"h1","request":{"subtype":"hook_callback"}}"#)
            .is_empty());
        let out = stream.take_outgoing();
        assert_eq!(out.len(), 1);
        assert!(out[0].contains("\"subtype\":\"error\""));
        // An init failure is shown.
        let events = stream.feed(r#"{"type":"control_response","response":{"subtype":"error","request_id":"init","error":"bad"}}"#);
        assert_eq!(
            events,
            [ChatEvent::Error {
                message: "bad".into()
            }]
        );
    }
}
