//! A conversation read back from the agent's own transcript, so a chat
//! opened later — or after the same session ran in a terminal — shows what
//! was said. Nothing here writes: the files belong to the agents.
//!
//! Claude keeps `<config>/projects/<slug of the folder>/<id>.jsonl`; Codex a
//! `rollout-…-<thread id>.jsonl` under `<home>/sessions`. Only the end of a
//! long file is read, which is the part a chat shows first anyway.

use super::claude;
use super::codex;
use super::event::{
    clip_head, clip_tail, one_line, ChatEvent, ToolKind, DETAIL_LIMIT, OUTPUT_LIMIT,
};
use crate::accounts::account_environment;
use crate::provider::launch::current_environment;
use crate::provider::sweep;
use crate::workspace::model::Agent;
use crate::workspace::Workspace;
use crate::{Result, Storage};
use serde_json::Value;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How much of the end of a transcript is read.
const READ_LIMIT: u64 = 16 * 1024 * 1024;
/// How many events a history returns at most; the newest are kept.
const EVENT_LIMIT: usize = 4_000;

/// Where a session's conversation is kept. Taken from the workspace, so the
/// slow part — reading the file — can happen without holding it.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub agent: Agent,
    pub home: PathBuf,
    pub directory: PathBuf,
    pub provider_id: String,
}

/// The session's transcript location; `None` before the agent has an id.
pub fn source(workspace: &Workspace, storage: &Storage, id: &str) -> Result<Option<Source>> {
    let session = workspace.session(id)?.clone();
    if session.provider_id.is_empty() {
        return Ok(None);
    }
    let directory = crate::session::directory_for(workspace, id)?;
    let account = account_environment(
        &session,
        &workspace.state().profiles,
        &storage.accounts(),
        &current_environment(),
    )?;
    Ok(Some(Source {
        agent: session.agent,
        home: account.home,
        directory,
        provider_id: session.provider_id,
    }))
}

/// The conversation as chat events, oldest first. Empty when the agent has
/// not written one yet.
pub fn read(source: &Source) -> Vec<ChatEvent> {
    let events = match source.agent {
        Agent::Claude => claude_file(&source.home, &source.directory, &source.provider_id)
            .map(|file| claude_events(&file)),
        Agent::Codex => {
            codex_file(&source.home, &source.provider_id).map(|file| codex_events(&file))
        }
    };
    events.unwrap_or_default()
}

/// [`source`] and [`read`] in one step.
pub fn load(workspace: &Workspace, storage: &Storage, id: &str) -> Result<Vec<ChatEvent>> {
    Ok(source(workspace, storage, id)?
        .map(|source| read(&source))
        .unwrap_or_default())
}

/// Claude's transcript for `provider_id`: under the folder's slug, or the
/// resolved folder's (a symlinked `/tmp` is stored as `/private/tmp`), or
/// failing both wherever it is.
pub fn claude_file(home: &Path, directory: &Path, provider_id: &str) -> Option<PathBuf> {
    let projects = home.join("projects");
    let name = format!("{provider_id}.jsonl");
    let mut folders = vec![sweep::slug(directory)];
    if let Ok(resolved) = fs::canonicalize(directory) {
        folders.push(sweep::slug(&resolved));
    }
    for folder in folders {
        let file = projects.join(folder).join(&name);
        if file.is_file() {
            return Some(file);
        }
    }
    fs::read_dir(&projects)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(&name))
        .find(|file| file.is_file())
}

/// Codex's rollout for `thread`.
pub fn codex_file(home: &Path, thread: &str) -> Option<PathBuf> {
    let mut files = Vec::new();
    sweep::rollouts(&home.join("sessions"), 0, &mut files);
    let suffix = format!("-{thread}.jsonl");
    files.into_iter().find(|file| {
        file.file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with(&suffix))
    })
}

/// The records at the end of a `.jsonl`, at most [`READ_LIMIT`] of it. A
/// record the cut went through is skipped.
fn records(file: &Path) -> Vec<Value> {
    let Ok(mut handle) = fs::File::open(file) else {
        return Vec::new();
    };
    let length = handle.metadata().map(|meta| meta.len()).unwrap_or(0);
    let cut = length > READ_LIMIT;
    if cut && handle.seek(SeekFrom::Start(length - READ_LIMIT)).is_err() {
        return Vec::new();
    }
    let mut raw = Vec::new();
    if handle.take(READ_LIMIT).read_to_end(&mut raw).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.split('\n');
    if cut {
        lines.next();
    }
    lines
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn newest(mut events: Vec<ChatEvent>) -> Vec<ChatEvent> {
    if events.len() > EVENT_LIMIT {
        events.drain(..events.len() - EVENT_LIMIT);
    }
    events
}

/// Text a person typed, or nothing for the wrappers the agents file as user
/// messages (`<command-name>`, `<local-command-caveat>`, context blocks).
fn typed(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty() && !trimmed.starts_with('<')).then(|| trimmed.to_string())
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

pub fn claude_events(file: &Path) -> Vec<ChatEvent> {
    let mut events = Vec::new();
    let mut blocks: std::collections::HashMap<String, usize> = Default::default();
    for record in records(file) {
        // A subagent's own exchange, and context Claude adds itself.
        let flag = |key: &str| record.get(key).and_then(Value::as_bool).unwrap_or(false);
        if flag("isSidechain") || flag("isMeta") {
            continue;
        }
        let cwd = record.get("cwd").and_then(Value::as_str);
        let uuid = string(&record, "uuid").to_string();
        let Some(message) = record.get("message") else {
            continue;
        };
        match string(&record, "type") {
            "user" => match message.get("content") {
                Some(Value::String(text)) => {
                    if let Some(text) = typed(text) {
                        events.push(ChatEvent::User { id: uuid, text });
                    }
                }
                Some(Value::Array(parts)) => {
                    let text = parts
                        .iter()
                        .filter(|part| string(part, "type") == "text")
                        .map(|part| string(part, "text"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Some(text) = typed(&text) {
                        events.push(ChatEvent::User {
                            id: uuid.clone(),
                            text,
                        });
                    }
                    events.extend(parts.iter().filter_map(claude::tool_result));
                }
                _ => {}
            },
            "assistant" => {
                let id = string(message, "id").to_string();
                let Some(parts) = message.get("content").and_then(Value::as_array) else {
                    continue;
                };
                for part in parts {
                    // Claude files one record per content block. Counting the
                    // blocks of each message gives the index the live stream
                    // used, so a block read back has the id it was shown with
                    // and the window does not show it twice.
                    let index = blocks.entry(id.clone()).or_insert(0);
                    let block = format!("{id}:{index}");
                    *index += 1;
                    match string(part, "type") {
                        "text" => events.push(ChatEvent::TextDone {
                            id: block,
                            text: string(part, "text").to_string(),
                        }),
                        "thinking" if !string(part, "thinking").is_empty() => {
                            events.push(ChatEvent::Thinking {
                                id: block,
                                delta: string(part, "thinking").to_string(),
                            })
                        }
                        "tool_use" => claude::tool_events(part, cwd, &mut events),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    newest(events)
}

pub fn codex_events(file: &Path) -> Vec<ChatEvent> {
    let records = records(file);
    // Newer rollouts record each finished item whole; older ones only the
    // model's raw exchange, read as a fallback.
    let items = records.iter().any(|record| {
        record.pointer("/payload/type").and_then(Value::as_str) == Some("item_completed")
    });
    let cwd = records
        .iter()
        .find(|record| string(record, "type") == "session_meta")
        .and_then(|record| record.pointer("/payload/cwd"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut events = Vec::new();
    for record in &records {
        let Some(payload) = record.get("payload") else {
            continue;
        };
        match (string(record, "type"), string(payload, "type")) {
            ("event_msg", "item_completed") if items => {
                if let Some(item) = payload.get("item") {
                    codex_item(item, cwd.as_deref(), &mut events);
                }
            }
            ("event_msg", "user_message") if !items => {
                if let Some(text) = typed(string(payload, "message")) {
                    events.push(ChatEvent::User {
                        id: format!("user-{}", events.len()),
                        text,
                    });
                }
            }
            ("event_msg", "agent_message") if !items => events.push(ChatEvent::TextDone {
                id: format!("agent-{}", events.len()),
                text: string(payload, "message").to_string(),
            }),
            ("response_item", "function_call" | "custom_tool_call") if !items => {
                let name = string(payload, "name");
                let detail = payload
                    .get("arguments")
                    .or_else(|| payload.get("input"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                events.push(ChatEvent::Tool {
                    id: string(payload, "call_id").to_string(),
                    kind: ToolKind::Other,
                    name: name.to_string(),
                    title: name.to_string(),
                    detail: clip_head(detail, DETAIL_LIMIT),
                });
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") if !items => {
                let output = match payload.get("output") {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .map(|part| string(part, "text"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                    _ => String::new(),
                };
                events.push(ChatEvent::ToolResult {
                    id: string(payload, "call_id").to_string(),
                    ok: true,
                    output: clip_tail(&output, OUTPUT_LIMIT),
                });
            }
            _ => {}
        }
    }
    newest(events)
}

/// One finished item as the rollout stores it.
fn codex_item(item: &Value, cwd: Option<&str>, events: &mut Vec<ChatEvent>) {
    let id = string(item, "id").to_string();
    let texts = |key: &str| {
        item.get(key)
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    };
    match string(item, "type") {
        "UserMessage" => {
            if let Some(text) = typed(&texts("content")) {
                events.push(ChatEvent::User { id, text });
            }
        }
        "AgentMessage" => events.push(ChatEvent::TextDone {
            id,
            text: texts("content"),
        }),
        "Reasoning" => {
            let summary = item
                .get("summary_text")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if !summary.is_empty() {
                events.push(ChatEvent::Thinking { id, delta: summary });
            }
        }
        "CommandExecution" => {
            let parsed: Vec<&str> = item
                .get("parsed_cmd")
                .and_then(Value::as_array)
                .map(|parts| parts.iter().map(|part| string(part, "cmd")).collect())
                .unwrap_or_default();
            let command = if parsed.is_empty() {
                match item.get("command") {
                    Some(Value::Array(parts)) => parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" "),
                    Some(Value::String(text)) => text.clone(),
                    _ => String::new(),
                }
            } else {
                parsed.join(" && ")
            };
            events.push(ChatEvent::Tool {
                id: id.clone(),
                kind: ToolKind::Command,
                name: "command".into(),
                title: format!("Run `{}`", one_line(&command, 80)),
                detail: clip_head(&command, DETAIL_LIMIT),
            });
            let exit = item
                .get("exit_code")
                .and_then(|code| code.as_i64().or_else(|| code.as_str()?.parse().ok()));
            events.push(ChatEvent::ToolResult {
                id,
                ok: string(item, "status") == "completed" && exit.is_none_or(|code| code == 0),
                output: clip_tail(string(item, "aggregated_output"), OUTPUT_LIMIT),
            });
        }
        "FileChange" => {
            let changes = codex::changes(item, cwd);
            events.push(ChatEvent::Tool {
                id: id.clone(),
                kind: ToolKind::Edit,
                name: "fileChange".into(),
                title: match changes.as_slice() {
                    [(path, _)] => format!("Edit {path}"),
                    _ => format!("Edit {} files", changes.len()),
                },
                detail: String::new(),
            });
            for (path, diff) in changes {
                events.push(ChatEvent::FileChange {
                    id: id.clone(),
                    path,
                    diff,
                });
            }
        }
        "Extension" if string(item, "kind") == "web.search" => {
            events.push(ChatEvent::Tool {
                id,
                kind: ToolKind::Web,
                name: "webSearch".into(),
                title: format!("Search the web for {}", one_line(string(item, "query"), 80)),
                detail: clip_head(string(item, "query"), DETAIL_LIMIT),
            });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn write(file: &Path, records: &[Value]) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut handle = fs::File::create(file).unwrap();
        for record in records {
            writeln!(handle, "{record}").unwrap();
        }
    }

    #[test]
    fn a_claude_transcript_reads_back_as_the_chat_it_was() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("work");
        fs::create_dir_all(&folder).unwrap();
        let file = root
            .path()
            .join("home/projects")
            .join(sweep::slug(&folder))
            .join("abc.jsonl");
        let cwd = folder.to_string_lossy().into_owned();
        write(
            &file,
            &[
                json!({ "type": "user", "uuid": "u0", "isMeta": true, "message": { "role": "user", "content": "<local-command-caveat>x</local-command-caveat>" } }),
                json!({ "type": "user", "uuid": "u1", "cwd": cwd, "message": { "role": "user", "content": "Create hello.txt" } }),
                json!({ "type": "assistant", "uuid": "a1", "cwd": cwd, "message": { "id": "m1", "role": "assistant", "content": [
                    { "type": "tool_use", "id": "t1", "name": "Write", "input": { "file_path": format!("{cwd}/hello.txt"), "content": "hi\n" } }
                ] } }),
                json!({ "type": "user", "uuid": "u2", "cwd": cwd, "message": { "role": "user", "content": [
                    { "type": "tool_result", "tool_use_id": "t1", "content": "File created" }
                ] } }),
                json!({ "type": "assistant", "uuid": "a2", "isSidechain": true, "message": { "id": "m9", "content": [{ "type": "text", "text": "subagent" }] } }),
                json!({ "type": "assistant", "uuid": "a3", "cwd": cwd, "message": { "id": "m2", "role": "assistant", "content": [{ "type": "thinking", "thinking": "" }] } }),
                json!({ "type": "assistant", "uuid": "a4", "cwd": cwd, "message": { "id": "m2", "role": "assistant", "content": [{ "type": "text", "text": "done" }] } }),
            ],
        );
        let found = claude_file(&root.path().join("home"), &folder, "abc").unwrap();
        assert_eq!(found, file);
        let events = claude_events(&found);
        assert_eq!(
            events,
            [
                ChatEvent::User {
                    id: "u1".into(),
                    text: "Create hello.txt".into()
                },
                ChatEvent::Tool {
                    id: "t1".into(),
                    kind: ToolKind::Edit,
                    name: "Write".into(),
                    title: "Write hello.txt".into(),
                    detail: serde_json::to_string_pretty(
                        &json!({ "file_path": format!("{cwd}/hello.txt"), "content": "hi\n" })
                    )
                    .unwrap()
                },
                ChatEvent::FileChange {
                    id: "t1".into(),
                    path: "hello.txt".into(),
                    diff: "--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1,1 @@\n+hi\n".into()
                },
                ChatEvent::ToolResult {
                    id: "t1".into(),
                    ok: true,
                    output: "File created".into()
                },
                ChatEvent::TextDone {
                    // The stream's block index: the second block of m2.
                    id: "m2:1".into(),
                    text: "done".into()
                },
            ]
        );
        // Found by id when the folder moved.
        assert_eq!(
            claude_file(&root.path().join("home"), Path::new("/elsewhere"), "abc"),
            Some(file)
        );
        assert_eq!(
            claude_file(&root.path().join("home"), &folder, "missing"),
            None
        );
    }

    #[test]
    fn a_codex_rollout_reads_back_from_its_finished_items() {
        let root = tempfile::tempdir().unwrap();
        let file = root
            .path()
            .join("sessions/2026/10/11/rollout-2026-10-11T16-34-16-thread-1.jsonl");
        write(
            &file,
            &[
                json!({ "type": "session_meta", "payload": { "id": "thread-1", "cwd": "/work" } }),
                json!({ "type": "event_msg", "payload": { "type": "item_completed", "item": { "type": "UserMessage", "id": "u", "content": [{ "type": "text", "text": "Run the tests" }] } } }),
                json!({ "type": "response_item", "payload": { "type": "function_call", "name": "shell", "arguments": "{}", "call_id": "c" } }),
                json!({ "type": "event_msg", "payload": { "type": "item_completed", "item": { "type": "CommandExecution", "id": "e", "command": ["/bin/zsh", "-lc", "npm test"], "parsed_cmd": [{ "type": "unknown", "cmd": "npm test" }], "status": "completed", "aggregated_output": "ok\n", "exit_code": "0" } } }),
                json!({ "type": "event_msg", "payload": { "type": "item_completed", "item": { "type": "FileChange", "id": "f", "changes": { "/work/a.txt": { "type": "update", "unified_diff": "@@ -1 +1 @@\n-a\n+b\n" } } } } }),
                json!({ "type": "event_msg", "payload": { "type": "item_completed", "item": { "type": "AgentMessage", "id": "m", "content": [{ "type": "Text", "text": "All green." }] } } }),
            ],
        );
        let found = codex_file(root.path(), "thread-1").unwrap();
        let events = codex_events(&found);
        assert_eq!(events.len(), 6, "{events:#?}");
        assert_eq!(
            events[0],
            ChatEvent::User {
                id: "u".into(),
                text: "Run the tests".into()
            }
        );
        assert!(matches!(&events[1], ChatEvent::Tool { title, .. } if title == "Run `npm test`"));
        assert_eq!(
            events[2],
            ChatEvent::ToolResult {
                id: "e".into(),
                ok: true,
                output: "ok\n".into()
            }
        );
        assert_eq!(
            events[4],
            ChatEvent::FileChange {
                id: "f".into(),
                path: "a.txt".into(),
                diff: "--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-a\n+b\n".into()
            }
        );
        assert_eq!(
            events[5],
            ChatEvent::TextDone {
                id: "m".into(),
                text: "All green.".into()
            }
        );
        assert_eq!(codex_file(root.path(), "thread-2"), None);
    }

    #[test]
    fn an_older_codex_rollout_falls_back_to_its_messages() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("sessions/rollout-x-thread-1.jsonl");
        write(
            &file,
            &[
                json!({ "type": "event_msg", "payload": { "type": "user_message", "message": "Hello" } }),
                json!({ "type": "response_item", "payload": { "type": "function_call", "name": "shell", "arguments": "{\"command\":[\"ls\"]}", "call_id": "c" } }),
                json!({ "type": "response_item", "payload": { "type": "function_call_output", "call_id": "c", "output": "a.txt" } }),
                json!({ "type": "event_msg", "payload": { "type": "agent_message", "message": "Hi" } }),
            ],
        );
        let events = codex_events(&file);
        assert_eq!(events.len(), 4);
        assert!(matches!(&events[0], ChatEvent::User { text, .. } if text == "Hello"));
        assert!(matches!(&events[2], ChatEvent::ToolResult { output, .. } if output == "a.txt"));
        assert!(matches!(&events[3], ChatEvent::TextDone { text, .. } if text == "Hi"));
    }
}
