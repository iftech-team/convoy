//! Pre-approves a session's folder for its agent, so a session opened by
//! Convoy never stops on "Do you trust the files in this folder?". Port of
//! the macOS app's `FolderTrust`. Worktrees are new paths every time and
//! would otherwise ask on every task.
//!
//! Both edits are additive and idempotent, and an unreadable file is left
//! alone rather than replaced.

use crate::model::Agent;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Approves `directory` for `agent`, in the provider home `env` points at
/// (`CLAUDE_CONFIG_DIR` / `CODEX_HOME`), or the default one. Failures are
/// ignored: the worst outcome is the agent asking, as it would anyway.
pub fn approve(agent: Agent, directory: &Path, env: &BTreeMap<String, String>) {
    let home = crate::storage::home(crate::Platform::current());
    let folder = directory.to_string_lossy();
    let _ = match agent {
        Agent::Claude => {
            let file = env
                .get("CLAUDE_CONFIG_DIR")
                .filter(|value| !value.is_empty())
                .map(|dir| PathBuf::from(dir).join(".claude.json"))
                .unwrap_or_else(|| home.join(".claude.json"));
            approve_claude(&file, &folder)
        }
        Agent::Codex => {
            let dir = env
                .get("CODEX_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            approve_codex(&dir.join("config.toml"), &folder)
        }
    };
}

/// Claude keeps per-project state in `.claude.json`:
/// `projects["<path>"].hasTrustDialogAccepted = true`.
pub fn approve_claude(file: &Path, directory: &str) -> std::io::Result<bool> {
    let mut root: Map<String, Value> = match std::fs::read_to_string(file) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(Value::Object(map)) => map,
            // Something Convoy does not understand: leave it alone.
            _ => return Ok(false),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(error) => return Err(error),
    };
    let projects = root
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(projects) = projects.as_object_mut() else {
        return Ok(false);
    };
    let entry = projects
        .entry(directory.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(entry) = entry.as_object_mut() else {
        return Ok(false);
    };
    if entry.get("hasTrustDialogAccepted") == Some(&Value::Bool(true)) {
        return Ok(false);
    }
    entry.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
    entry
        .entry("allowedTools")
        .or_insert_with(|| Value::Array(Vec::new()));
    write_atomically(file, &serde_json::to_string_pretty(&Value::Object(root))?)?;
    Ok(true)
}

/// Codex keeps trust in `config.toml` as
/// `[projects."<path>"]` / `trust_level = "trusted"`.
pub fn approve_codex(file: &Path, directory: &str) -> std::io::Result<bool> {
    let mut text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    // TOML basic strings escape backslashes and quotes, which a Windows path has.
    let key = directory.replace('\\', "\\\\").replace('"', "\\\"");
    let header = format!("[projects.\"{key}\"]");
    if text.contains(&header) {
        return Ok(false);
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!("\n{header}\ntrust_level = \"trusted\"\n"));
    write_atomically(file, &text)?;
    Ok(true)
}

/// Writes through a private temporary file and a rename, so a crash never
/// leaves a provider's configuration half written.
fn write_atomically(file: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = file.with_extension(format!("convoy-{}.tmp", std::process::id()));
    let mut handle = crate::fs::create_private(&temporary)?;
    handle.write_all(contents.as_bytes())?;
    handle.sync_all()?;
    drop(handle);
    std::fs::rename(&temporary, file)
}
