//! Listing the provider's own saved conversations
//! for a folder so an existing session can be imported.
//!
//! Only the head of each `.jsonl` is read — enough for the session metadata and
//! the first user message, and bounded regardless of how long the file grew —
//! plus, for Claude, its tail, where the conversation's name is kept.

use crate::patterns::is_uuid;
use crate::workspace::model::Agent;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const HEAD_BYTES: u64 = 96_000;
/// Claude appends a conversation's name again as it goes, so the latest one is
/// always near the end.
const TAIL_BYTES: u64 = 64_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub agent: Agent,
    pub provider_id: String,
    pub title: String,
    /// Modification time in milliseconds since the epoch, newest first.
    pub at: f64,
}

/// The records in the head of a `.jsonl`; lines cut off or unreadable are skipped.
pub(crate) fn head(file: &Path) -> Vec<Value> {
    let Ok(handle) = fs::File::open(file) else {
        return Vec::new();
    };
    let mut raw = Vec::new();
    if handle.take(HEAD_BYTES).read_to_end(&mut raw).is_err() {
        return Vec::new();
    }
    String::from_utf8_lossy(&raw)
        .split('\n')
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The name a conversation was given, as `claude --resume` and Codex show it:
/// one set with `/rename` first, then the one the agent wrote itself. `None`
/// when it has none, and the first message stands in.
pub(crate) fn named(
    agent: Agent,
    file: &Path,
    id: &str,
    codex: &HashMap<String, String>,
) -> Option<String> {
    let name = match agent {
        Agent::Claude => claude_name(file)?,
        Agent::Codex => codex.get(&id.to_lowercase())?.clone(),
        Agent::Gemini => return None,
    };
    let folded = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!folded.is_empty()).then(|| crate::json::head(&folded, 100).to_string())
}

/// The last `custom-title`, else the last `ai-title`, else the last `summary`
/// in the tail of a Claude transcript.
fn claude_name(file: &Path) -> Option<String> {
    let mut handle = fs::File::open(file).ok()?;
    let length = handle.metadata().ok()?.len();
    handle
        .seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut raw = Vec::new();
    handle.read_to_end(&mut raw).ok()?;
    let mut found: [Option<String>; 3] = Default::default();
    for line in String::from_utf8_lossy(&raw).split('\n') {
        let (slot, key) = if line.starts_with(r#"{"type":"custom-title""#) {
            (0, "customTitle")
        } else if line.starts_with(r#"{"type":"ai-title""#) {
            (1, "aiTitle")
        } else if line.starts_with(r#"{"type":"summary""#) {
            (2, "summary")
        } else {
            continue;
        };
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(text) = record
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
        {
            found[slot] = Some(text.to_string());
        }
    }
    found.into_iter().flatten().next()
}

/// Codex's thread names by id, from `session_index.jsonl` in its home; a
/// thread renamed later appears again, and the last line wins.
pub(crate) fn codex_names(home: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(text) = fs::read_to_string(home.join("session_index.jsonl")) else {
        return names;
    };
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = record.get("id").and_then(Value::as_str);
        let name = record.get("thread_name").and_then(Value::as_str);
        if let (Some(id), Some(name)) = (id, name) {
            names.insert(id.to_lowercase(), name.to_string());
        }
    }
    names
}

fn walk(folder: &Path, depth: u32, max_depth: u32, visited: &mut usize, files: &mut Vec<PathBuf>) {
    *visited += 1;
    if *visited > 3000 || depth > max_depth || files.len() > 3000 {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&path, depth + 1, max_depth, visited, files),
            Ok(kind)
                if kind.is_file() && path.extension().is_some_and(|value| value == "jsonl") =>
            {
                files.push(path);
            }
            _ => {}
        }
    }
}

pub fn scan(agent: Agent, home: &Path, directory: &Path) -> Vec<Transcript> {
    // Gemini's past conversations are not imported yet.
    if agent == Agent::Gemini {
        return Vec::new();
    }
    let root = home.join(match agent {
        Agent::Claude => "projects",
        Agent::Codex | Agent::Gemini => "sessions",
    });
    let mut files = Vec::new();
    let mut visited = 0usize;
    match agent {
        // Claude stores each project under a slug of its absolute path, so the
        // folder is addressed directly and only its own files are read.
        Agent::Claude => {
            let slug: String = directory
                .to_string_lossy()
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        '-'
                    }
                })
                .collect();
            walk(&root.join(slug), 5, 5, &mut visited, &mut files);
        }
        Agent::Codex | Agent::Gemini => walk(&root, 0, 5, &mut visited, &mut files),
    }

    let codex = match agent {
        Agent::Codex => codex_names(home),
        Agent::Claude | Agent::Gemini => HashMap::new(),
    };
    let mut result = Vec::new();
    for file in files {
        let records = head(&file);
        let meta = records
            .iter()
            .find(|record| record.get("type").and_then(Value::as_str) == Some("session_meta"))
            .and_then(|record| record.get("payload"));
        let id = match agent {
            Agent::Claude => file
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Agent::Codex | Agent::Gemini => meta
                .and_then(|meta| meta.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        };
        if !is_uuid(&id) {
            continue;
        }
        if agent == Agent::Codex {
            let Some(meta) = meta else { continue };
            let cwd = meta.get("cwd").and_then(Value::as_str).unwrap_or_default();
            let same = fs::canonicalize(cwd)
                .ok()
                .zip(fs::canonicalize(directory).ok())
                .map(|(a, b)| a == b)
                .unwrap_or_else(|| Path::new(cwd) == directory);
            let subagent = meta
                .get("source")
                .and_then(|source| source.get("subagent"))
                .is_some_and(crate::json::truthy);
            let nested = meta
                .get("parent_thread_id")
                .is_some_and(crate::json::truthy);
            if !same || subagent || nested {
                continue;
            }
        }
        let title = named(agent, &file, &id, &codex).unwrap_or_else(|| title(agent, &records, &id));
        let at = fs::metadata(&file)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|value| value.as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        result.push(Transcript {
            agent,
            provider_id: id,
            title,
            at,
        });
    }
    result.sort_by(|a, b| b.at.total_cmp(&a.at));
    result.truncate(100);
    result
}

/// The conversation's first user message, collapsed to one line, or the agent
/// and the start of the id when there is none.
pub(crate) fn title(agent: Agent, records: &[Value], id: &str) -> String {
    let contents = records.iter().filter_map(|record| match agent {
        Agent::Claude => (record.get("type").and_then(Value::as_str) == Some("user"))
            .then(|| record.get("message"))
            .flatten()
            .and_then(|message| message.get("message").or_else(|| message.get("content"))),
        Agent::Codex => record
            .get("payload")
            .filter(|payload| {
                payload.get("type").and_then(Value::as_str) == Some("user_message")
                    || payload.get("role").and_then(Value::as_str) == Some("user")
            })
            .and_then(|payload| payload.get("message").or_else(|| payload.get("content"))),
        Agent::Gemini => None,
    });
    // The first message a person wrote. The agents also file their own
    // wrappers as user messages — `<local-command-caveat>`, `<command-name>`,
    // `<environment_context>` — and those make a title nobody recognises.
    let first = contents
        .map(|content| match content {
            Value::String(text) => text.clone(),
            Value::Array(parts) => parts
                .iter()
                .map(|part| part.get("text").and_then(Value::as_str).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        })
        .map(|raw| without_markup(&raw))
        .find(|text| !text.is_empty());
    let raw = first.unwrap_or_else(|| format!("{} {}", agent.as_str(), &id[..8]));
    crate::json::head(&raw, 100).to_string()
}

/// A message with the agents' `<tag>…</tag>` blocks removed and its
/// whitespace folded; empty when nothing a person wrote is left, or when what
/// is left still starts with markup.
pub(crate) fn without_markup(raw: &str) -> String {
    let mut text = raw.to_string();
    while let Some(open) = text.find('<') {
        let Some(close) = text[open..].find('>').map(|at| open + at) else {
            break;
        };
        let tag = &text[open + 1..close];
        if tag.is_empty()
            || !tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            break;
        }
        let end = format!("</{tag}>");
        let Some(after) = text[close..].find(&end).map(|at| close + at + end.len()) else {
            break;
        };
        text.replace_range(open..after, " ");
    }
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if folded.starts_with('<') {
        String::new()
    } else {
        folded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "0199aaaa-0000-4000-8000-000000000001";

    fn claude(texts: &[&str]) -> Vec<Value> {
        texts
            .iter()
            .map(|text| json!({ "type": "user", "message": { "role": "user", "content": text } }))
            .collect()
    }

    #[test]
    fn a_title_skips_the_agents_own_wrappers() {
        let records = claude(&[
            "<local-command-caveat>Caveat: The messages below were generated by the user while running local commands.</local-command-caveat>",
            "<command-name>/clear</command-name>\n<command-message>clear</command-message>",
            "Fix the   login\nredirect",
        ]);
        assert_eq!(title(Agent::Claude, &records, ID), "Fix the login redirect");
    }

    #[test]
    fn markup_around_what_a_person_wrote_is_removed() {
        let records =
            claude(&["<system-reminder>be brief</system-reminder> Add a filter for deposits"]);
        assert_eq!(
            title(Agent::Claude, &records, ID),
            "Add a filter for deposits"
        );
        // A comparison in prose is not markup.
        let records = claude(&["Make a < b hold"]);
        assert_eq!(title(Agent::Claude, &records, ID), "Make a < b hold");
    }

    #[test]
    fn codex_context_is_not_a_title_and_nothing_left_falls_back_to_the_id() {
        let records = vec![
            json!({ "type": "response_item", "payload": { "type": "message", "role": "user", "content": [{ "type": "input_text", "text": "<environment_context>\n<cwd>/home</cwd>\n</environment_context>" }] } }),
            json!({ "type": "event_msg", "payload": { "type": "user_message", "message": "Write the release notes" } }),
        ];
        assert_eq!(title(Agent::Codex, &records, ID), "Write the release notes");
        assert_eq!(
            title(
                Agent::Claude,
                &claude(&["<command-name>/usage</command-name>"]),
                ID
            ),
            "claude 0199aaaa"
        );
    }

    #[test]
    fn a_claude_name_beats_the_first_message() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("{ID}.jsonl"));
        let none = HashMap::new();
        let mut lines = vec![
            r#"{"type":"user","message":{"role":"user","content":"'/var/folders/x/IMAGE.jpg' what is this"}}"#.to_string(),
        ];
        fs::write(&file, lines.join("\n")).unwrap();
        assert_eq!(named(Agent::Claude, &file, ID, &none), None);
        // The agent's own title, then a later one, then the one set with /rename.
        lines.push(r#"{"type":"ai-title","aiTitle":"Old title","sessionId":"x"}"#.into());
        lines.push(
            r#"{"type":"ai-title","aiTitle":"Upload  screenshot\nreview","sessionId":"x"}"#.into(),
        );
        fs::write(&file, lines.join("\n")).unwrap();
        assert_eq!(
            named(Agent::Claude, &file, ID, &none).as_deref(),
            Some("Upload screenshot review")
        );
        lines.push(
            r#"{"type":"custom-title","customTitle":"Репозиторий convoy","sessionId":"x"}"#.into(),
        );
        lines.push(r#"{"type":"ai-title","aiTitle":"Later title","sessionId":"x"}"#.into());
        fs::write(&file, lines.join("\n")).unwrap();
        assert_eq!(
            named(Agent::Claude, &file, ID, &none).as_deref(),
            Some("Репозиторий convoy")
        );
    }

    #[test]
    fn a_codex_name_is_the_last_one_in_its_index() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("session_index.jsonl"),
            format!(
                "{{\"id\":\"{ID}\",\"thread_name\":\"Hi Codex!\"}}\n{{\"id\":\"{}\",\"thread_name\":\"Respond to greeting\"}}\n",
                ID.to_uppercase()
            ),
        )
        .unwrap();
        let names = codex_names(dir.path());
        assert_eq!(
            named(Agent::Codex, Path::new("unused"), ID, &names).as_deref(),
            Some("Respond to greeting")
        );
        assert_eq!(
            named(
                Agent::Codex,
                Path::new("unused"),
                "0199aaaa-0000-4000-8000-000000000002",
                &names
            ),
            None
        );
    }
}
