//! Port of `provider.cjs`: how a provider CLI is invoked and how its usage
//! quotas are read.

pub mod codex;
pub mod launch;
pub mod transcripts;

use crate::workspace::model::{Agent, Session};
use crate::{Platform, Result};
use launch::{agent_args, quote, LaunchSpec};
use serde_json::Value;
use std::collections::BTreeMap;

/// Environment keys forwarded explicitly into the login shell, because `-ilc`
/// re-reads the user's profile and may otherwise discard them.
const FORWARDED: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// `providerSpec()` — how the agent is actually invoked.
///
/// On Unix that is a login shell, so the agent sees the PATH and provider
/// configuration the user gets in a terminal, with the account bindings
/// restored because `-ilc` re-reads the profile. On Windows there is no such
/// shell: the executable is resolved directly and the argument vector is
/// passed literally, which is what keeps a prompt containing quotes, `&` or a
/// newline intact through `.cmd` shims that would otherwise mangle it.
pub fn provider_spec(agent: Agent, args: &[String], env: &BTreeMap<String, String>) -> LaunchSpec {
    provider_spec_for(agent, args, env, Platform::current())
        .unwrap_or_else(|_| unix_spec(agent, args, env))
}

/// As [`provider_spec`], for a stated platform, reporting a Windows agent that
/// cannot be found rather than hiding it.
pub fn provider_spec_for(
    agent: Agent,
    args: &[String],
    env: &BTreeMap<String, String>,
    platform: Platform,
) -> Result<LaunchSpec> {
    if platform.is_windows() {
        let resolved = launch::windows_executable(agent, env)?;
        let mut argv = resolved.prefix;
        argv.extend(args.iter().cloned());
        return Ok(LaunchSpec {
            file: resolved.file,
            args: argv,
        });
    }
    Ok(unix_spec(agent, args, env))
}

fn unix_spec(agent: Agent, args: &[String], env: &BTreeMap<String, String>) -> LaunchSpec {
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
    session_spec_for(session, settings_file, env, Platform::current())
        .unwrap_or_else(|_| unix_spec(session.agent, &session_args(session, settings_file), env))
}

/// The arguments a session launches with, minus the agent name.
fn session_args(session: &Session, settings_file: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = agent_args(session, session.started)
        .into_iter()
        .skip(1)
        .collect();
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
    args
}

/// As [`session_spec`], for a stated platform. Returns the reason when a
/// Windows agent cannot be found, so the user is told what to install.
pub fn session_spec_for(
    session: &Session,
    settings_file: Option<&str>,
    env: &BTreeMap<String, String>,
    platform: Platform,
) -> Result<LaunchSpec> {
    let args = session_args(session, settings_file);
    provider_spec_for(session.agent, &args, env, platform)
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
            let Some(value) = bucket.get(key) else {
                continue;
            };
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
