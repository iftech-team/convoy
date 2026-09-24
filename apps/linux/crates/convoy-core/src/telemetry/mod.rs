//! Claude's documented per-session hooks.
//!
//! Convoy writes a settings file that points every hook at its own binary in
//! `--hook` mode. The helper stores a state word and, optionally, validated
//! quota windows — never prompts, transcripts or tool arguments.

pub mod hook;

use crate::hash::sha256_hex;
use crate::workspace::model::Session;
use crate::Result;
use serde_json::{json, Map, Value};
use std::fs;
use std::io::Write;

use std::path::{Path, PathBuf};

/// The events Convoy subscribes to. Each maps to one agent state.
pub const EVENTS: [&str; 8] = [
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Notification",
    "Stop",
    "SessionEnd",
];

/// A telemetry file set is addressed by the hash of the session id, so nothing
/// derived from user input reaches the filesystem as a path component.
pub fn output_path(root: &Path, session_id: &str) -> PathBuf {
    root.join(sha256_hex(session_id))
}

/// `sanitize()` — reduces a hook payload to a state word and validated quota
/// windows. Everything else in the payload is discarded here, before it can be
/// written anywhere.
pub fn sanitize(input: &Value, now_ms: f64) -> Value {
    let event = input.get("hook_event_name").and_then(Value::as_str);
    let tool = input.get("tool_name").and_then(Value::as_str);
    let notification = input.get("notification_type").and_then(Value::as_str);
    let state = match event {
        Some("SessionStart") => Some("idle"),
        Some("UserPromptSubmit") => Some("working"),
        Some("PreToolUse") => Some(if tool == Some("AskUserQuestion") {
            "waiting"
        } else {
            "working"
        }),
        Some("PostToolUse") => Some("working"),
        Some("PermissionRequest") => Some("waiting"),
        Some("Notification") => Some(if notification == Some("idle_prompt") {
            "done"
        } else {
            "waiting"
        }),
        Some("Stop") => Some("done"),
        Some("SessionEnd") => Some("ended"),
        _ => None,
    };

    let limits = input.get("rate_limits");
    let mut windows: Vec<Value> = Vec::new();
    if let Some(limits) = limits {
        for name in ["five_hour", "seven_day"] {
            let Some(value) = limits.get(name) else {
                continue;
            };
            let Some(percent) = value.get("used_percentage").and_then(Value::as_f64) else {
                continue;
            };
            if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                continue;
            }
            let resets_at = match value.get("resets_at") {
                None | Some(Value::Null) => None,
                Some(raw) => match raw.as_f64() {
                    Some(seconds) if seconds.is_finite() && seconds > now_ms / 1000.0 => {
                        Some(seconds)
                    }
                    _ => continue,
                },
            };
            let mut window = Map::new();
            window.insert("name".into(), json!(name));
            window.insert("percent".into(), json!(percent));
            if let Some(resets_at) = resets_at {
                window.insert("resetsAt".into(), json!(resets_at));
            }
            windows.push(Value::Object(window));
        }
    }

    let mut result = Map::new();
    result.insert("at".into(), json!(now_ms));
    if let Some(state) = state {
        result.insert("state".into(), json!(state));
        result.insert("event".into(), json!(event));
    }
    if limits.is_some() {
        result.insert("windows".into(), Value::Array(windows));
    }
    Value::Object(result)
}

/// `configuration()` — writes the per-session settings file and clears stale
/// status, so old `Stop` data cannot mark a fresh launch complete.
pub fn configuration(
    root: &Path,
    session: &Session,
    executable: &Path,
    usage: bool,
) -> Result<PathBuf> {
    fs::create_dir_all(root)?;
    let output = output_path(root, &session.id);
    let output_text = output.to_string_lossy().into_owned();
    let executable_text = executable.to_string_lossy().into_owned();

    // Exec-form hooks take an argument vector, so nothing is shell-quoted and
    // a path with spaces needs no escaping.
    let command = json!({
        "type": "command",
        "command": executable_text,
        "args": ["--hook", output_text],
        "timeout": 5,
    });
    let mut hooks = Map::new();
    for event in EVENTS {
        hooks.insert(
            event.to_string(),
            json!([{ "matcher": "", "hooks": [command] }]),
        );
    }
    let mut document = Map::new();
    document.insert("hooks".into(), Value::Object(hooks));
    if usage {
        // The status line is a shell string rather than an argument vector.
        let quoted = format!(
            "{} --hook {}",
            crate::provider::launch::quote(&executable_text),
            crate::provider::launch::quote(&output_text)
        );
        document.insert(
            "statusLine".into(),
            json!({ "type": "command", "command": quoted }),
        );
    }

    let file = with_suffix(&output, ".settings.json");
    let mut handle = crate::platform::private_file_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&file)?;
    handle.write_all(serde_json::to_string(&Value::Object(document))?.as_bytes())?;
    drop(handle);
    for suffix in [".status", ".usage"] {
        let _ = fs::remove_file(with_suffix(&output, suffix));
    }
    Ok(file)
}

/// `read()` — a telemetry file, or nothing. Unreadable, oversized and
/// malformed files are all simply absent: telemetry never blocks the UI.
pub fn read(root: &Path, session_id: &str, suffix: &str) -> Option<Value> {
    let file = with_suffix(&output_path(root, session_id), suffix);
    let metadata = fs::metadata(&file).ok()?;
    if metadata.len() > 16_000 {
        return None;
    }
    serde_json::from_str(&fs::read_to_string(&file).ok()?).ok()
}

pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.to_path_buf().into_os_string();
    name.push(suffix);
    PathBuf::from(name)
}
