//! The `--hook` mode of the Convoy binary.
//!
//! Claude runs this for every subscribed event. It must be fast, silent and
//! incapable of blocking the agent: any malformed or unavailable input is
//! ignored rather than reported.

use super::{sanitize, with_suffix};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};

use std::path::Path;
use uuid::Uuid;

const MAX_INPUT: usize = 2 * 1024 * 1024;

/// Reads a hook payload from stdin and stores the sanitised result next to
/// `output`. Returns the status-line text, if any, which the caller prints.
pub fn run(output: &Path, input: &mut impl Read) -> Option<String> {
    let mut raw = Vec::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = input.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if raw.len() > MAX_INPUT {
            return None;
        }
    }
    let parsed: Value = serde_json::from_slice(&raw).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis() as f64;
    let value = sanitize(&parsed, now);
    let has_state = value.get("state").is_some();
    let windows = value.get("windows").and_then(Value::as_array).cloned();
    if !has_state && windows.is_none() {
        return None;
    }
    let suffix = if has_state { ".status" } else { ".usage" };
    let file = with_suffix(output, suffix);
    let temporary = with_suffix(&file, &format!(".{}", Uuid::new_v4()));
    let body = serde_json::to_string(&value).ok()?;
    let written = (|| -> std::io::Result<()> {
        let mut handle = crate::platform::private_file_options()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        handle.write_all(body.as_bytes())?;
        drop(handle);
        fs::rename(&temporary, &file)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
        return None;
    }
    if suffix == ".usage" {
        return Some(
            windows?
                .iter()
                .map(|window| {
                    format!(
                        "{}: {}%",
                        window
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                        window
                            .get("percent")
                            .and_then(Value::as_f64)
                            .unwrap_or_default()
                            .round()
                    )
                })
                .collect::<Vec<_>>()
                .join(" · "),
        );
    }
    None
}
