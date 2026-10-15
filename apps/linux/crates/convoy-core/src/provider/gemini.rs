//! Gemini CLI's sessions on disk, so a Convoy session resumes its own
//! conversation exactly.
//!
//! Gemini names a session itself and takes no id to start with, but
//! `--resume <uuid>` resumes one. Each is a JSON file under the project's
//! folder in its home: `tmp/<name>/chats/session-*.json`, where the name is
//! the one `projects.json` gives the working folder, or, in older versions,
//! the SHA-256 of its path. The file says its `sessionId` and `startTime`.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// The folders a working folder's chats may be in, newest layout first.
pub fn chat_folders(home: &Path, directory: &Path) -> Vec<PathBuf> {
    let tmp = home.join("tmp");
    let path = directory.to_string_lossy().into_owned();
    let mut folders = Vec::new();
    if let Some(name) = std::fs::read_to_string(home.join("projects.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|json| json["projects"][path.as_str()].as_str().map(str::to_string))
    {
        folders.push(tmp.join(name).join("chats"));
    }
    folders.push(tmp.join(crate::hash::sha256_hex(&path)).join("chats"));
    folders
}

/// The session Gemini started in `directory` at or after `since` (an
/// ISO 8601 time), the earliest such, skipping ids `taken` by other Convoy
/// sessions. `None` until Gemini has written it.
pub fn session_since(
    home: &Path,
    directory: &Path,
    since: &str,
    taken: &[String],
) -> Option<String> {
    let mut found: Vec<(String, String)> = Vec::new();
    for folder in chat_folders(home, directory) {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("session-") && name.ends_with(".json")) {
                continue;
            }
            let Some(json) = std::fs::read_to_string(entry.path())
                .ok()
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            else {
                continue;
            };
            let (Some(id), Some(start)) = (json["sessionId"].as_str(), json["startTime"].as_str())
            else {
                continue;
            };
            // A few seconds of slack: Convoy notes the launch as the
            // process starts, Gemini its session a moment later or earlier.
            let after = match (
                crate::time::parse_iso8601(start),
                crate::time::parse_iso8601(since),
            ) {
                (Some(start), Some(since)) => start + 5 >= since,
                _ => start >= since,
            };
            if after && !taken.iter().any(|other| other == id) {
                found.push((start.to_string(), id.to_string()));
            }
        }
    }
    found.sort();
    found.into_iter().next().map(|(_, id)| id)
}
