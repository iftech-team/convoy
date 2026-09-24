//! Listing the provider's own saved conversations
//! for a folder so an existing session can be imported.
//!
//! Only the head of each `.jsonl` is read — enough for the session metadata and
//! the first user message, and bounded regardless of how long the file grew.

use crate::patterns::is_uuid;
use crate::workspace::model::Agent;
use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const HEAD_BYTES: u64 = 96_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    pub agent: Agent,
    pub provider_id: String,
    pub title: String,
    /// Modification time in milliseconds since the epoch, newest first.
    pub at: f64,
}

fn head(file: &Path) -> Vec<Value> {
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
    let root = home.join(match agent {
        Agent::Claude => "projects",
        Agent::Codex => "sessions",
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
        Agent::Codex => walk(&root, 0, 5, &mut visited, &mut files),
    }

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
            Agent::Codex => meta
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
        let content = match agent {
            Agent::Claude => records
                .iter()
                .find(|record| record.get("type").and_then(Value::as_str) == Some("user"))
                .and_then(|record| record.get("message"))
                .and_then(|message| message.get("message").or_else(|| message.get("content")))
                .cloned(),
            Agent::Codex => records
                .iter()
                .filter_map(|record| record.get("payload"))
                .find(|payload| {
                    payload.get("type").and_then(Value::as_str) == Some("user_message")
                        || payload.get("role").and_then(Value::as_str) == Some("user")
                })
                .and_then(|payload| {
                    payload
                        .get("message")
                        .or_else(|| payload.get("content"))
                        .cloned()
                }),
        };
        let raw = match content {
            Some(Value::String(text)) => text,
            Some(Value::Array(parts)) => parts
                .iter()
                .map(|part| part.get("text").and_then(Value::as_str).unwrap_or_default())
                .collect::<Vec<_>>()
                .join(" "),
            _ => format!("{} {}", agent.as_str(), &id[..8]),
        };
        let title: String =
            crate::json::head(&raw.split_whitespace().collect::<Vec<_>>().join(" "), 100)
                .to_string();
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
