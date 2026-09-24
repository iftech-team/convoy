//! Port of `provider.cjs`: how a provider CLI is invoked and how its usage
//! quotas are read.

pub mod codex;
pub mod launch;
pub mod transcripts;

use crate::workspace::model::{Agent, Session};
use launch::{agent_args, quote, LaunchSpec};
use serde_json::Value;
use std::collections::BTreeMap;

/// Environment keys forwarded explicitly into the login shell, because `-ilc`
/// re-reads the user's profile and may otherwise discard them.
const FORWARDED: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// `providerSpec()` — run `<agent> <args…>` through a login shell with the
/// account bindings restored.
pub fn provider_spec(agent: Agent, args: &[String], env: &BTreeMap<String, String>) -> LaunchSpec {
    let bindings: Vec<String> = FORWARDED
        .iter()
        .filter_map(|key| env.get(*key).map(|value| quote(&format!("{key}={value}"))))
        .collect();
    let command: Vec<String> = std::iter::once(agent.as_str().to_string())
        .chain(args.iter().cloned())
        .map(|arg| quote(&arg))
        .collect();
    LaunchSpec {
        file: "/bin/bash".into(),
        args: vec![
            "-ilc".into(),
            format!("exec env {} {}", bindings.join(" "), command.join(" ")),
        ],
    }
}

/// `sessionSpec()` — the launch used when the installed CLI supports model and
/// settings flags. Flags are inserted *before* the `--` separator so the first
/// prompt stays a literal positional argument.
pub fn session_spec(
    session: &Session,
    settings_file: Option<&str>,
    env: &BTreeMap<String, String>,
) -> LaunchSpec {
    let mut args: Vec<String> = agent_args(session, session.started).into_iter().skip(1).collect();
    let mut flags: Vec<String> = Vec::new();
    if let Some(model) = session.model.as_deref().filter(|model| !model.is_empty()) {
        flags.push("--model".into());
        flags.push(model.to_string());
    }
    if let (Agent::Claude, Some(file)) = (session.agent, settings_file.filter(|f| !f.is_empty())) {
        flags.push("--settings".into());
        flags.push(file.to_string());
    }
    let separator = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    args.splice(separator..separator, flags);
    provider_spec(session.agent, &args, env)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RateWindow {
    pub name: String,
    pub percent: f64,
    pub minutes: Option<f64>,
    pub resets_at: Option<f64>,
}

/// `rateWindows()` — accepts only well-formed, unexpired quota windows.
/// A missing or expired quota is reported as unavailable, never as zero usage.
pub fn rate_windows(result: &Value, now: f64) -> Vec<RateWindow> {
    let mut buckets: Vec<(String, &Value)> = Vec::new();
    if let Some(map) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        buckets.extend(map.iter().map(|(name, bucket)| (name.clone(), bucket)));
    } else if let Some(bucket) = result.get("rateLimits") {
        buckets.push(("codex".to_string(), bucket));
    }
    let mut windows = Vec::new();
    for (name, bucket) in buckets {
        for key in ["primary", "secondary"] {
            let Some(value) = bucket.get(key) else { continue };
            let Some(percent) = value.get("usedPercent").and_then(Value::as_f64) else {
                continue;
            };
            if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
                continue;
            }
            let resets_at = match value.get("resetsAt") {
                None | Some(Value::Null) => None,
                Some(raw) => match raw.as_f64() {
                    Some(seconds) if seconds.is_finite() && seconds > now => Some(seconds),
                    _ => continue,
                },
            };
            windows.push(RateWindow {
                name: format!("{name} {key}"),
                percent,
                minutes: value.get("windowDurationMins").and_then(Value::as_f64),
                resets_at,
            });
        }
    }
    windows
}

/// Seconds since the epoch, the default `now` for [`rate_windows`].
pub fn now_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or(0.0)
}
