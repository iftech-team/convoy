//! Port of `provider.cjs`: how a provider CLI is invoked and how its usage
//! quotas are read.

pub mod codex;
pub mod knowledge;
pub mod launch;
pub mod mcp;
pub mod models;
pub mod sweep;
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
    let (file, args) = launch::shell_command(format!(
        "exec env {} {}",
        bindings.join(" "),
        command.join(" ")
    ));
    LaunchSpec { file, args }
}

/// `sessionSpec()` — the launch used when the installed CLI supports model and
/// settings flags. Flags are inserted *before* the `--` separator so the first
/// prompt stays a literal positional argument.
pub fn session_spec(
    session: &Session,
    settings_file: Option<&str>,
    env: &BTreeMap<String, String>,
) -> LaunchSpec {
    session_spec_with(session, settings_file, env, &[])
}

/// As [`session_spec`], with extra flags placed beside `--model` — the
/// permission flags a user opted into.
pub fn session_spec_with(
    session: &Session,
    settings_file: Option<&str>,
    env: &BTreeMap<String, String>,
    extra: &[String],
) -> LaunchSpec {
    let args = session_args(session, settings_file, extra);
    provider_spec_for(session.agent, &args, env, Platform::current())
        .unwrap_or_else(|_| unix_spec(session.agent, &args, env))
}

/// The flags that skip an agent's permission prompts, when the user chose
/// that for this agent. Only ever added at launch, never stored.
pub fn permission_flags(agent: Agent, settings: &crate::model::Settings) -> Vec<String> {
    match agent {
        Agent::Claude if settings.yolo_claude => vec!["--dangerously-skip-permissions".into()],
        Agent::Codex if settings.yolo_codex => {
            vec!["--dangerously-bypass-approvals-and-sandbox".into()]
        }
        _ => Vec::new(),
    }
}

/// Every effort level either agent takes; what a model offers is a subset
/// (see [`models`]).
pub const EFFORTS: &[&str] = &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"];

/// What `claude --effort` and its `effortLevel` setting take.
pub const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Whether `agent` can be asked for `effort`. Claude Code takes a fixed set;
/// Codex lists its own per model and adds new ones, so any lowercase word is
/// passed on — it only ever travels as one argument or one JSON string.
pub fn valid_effort(agent: Agent, effort: &str) -> bool {
    match agent {
        Agent::Claude => CLAUDE_EFFORTS.contains(&effort),
        Agent::Codex => {
            !effort.is_empty()
                && effort.len() <= 32
                && effort
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
        }
    }
}

/// The flags that set how hard the agent thinks: `--effort` for Claude Code,
/// the `model_reasoning_effort` setting for Codex. Nothing for an empty or
/// unknown level, so the agent keeps its own default.
pub fn effort_flags(agent: Agent, effort: Option<&str>) -> Vec<String> {
    let Some(effort) = effort.filter(|effort| valid_effort(agent, effort)) else {
        return Vec::new();
    };
    match agent {
        Agent::Claude => vec!["--effort".into(), effort.into()],
        Agent::Codex => vec!["-c".into(), format!("model_reasoning_effort={effort}")],
    }
}

/// The arguments a session launches with, minus the agent name.
fn session_args(session: &Session, settings_file: Option<&str>, extra: &[String]) -> Vec<String> {
    let mut args: Vec<String> = agent_args(session, session.started)
        .into_iter()
        .skip(1)
        .collect();
    let mut flags: Vec<String> = Vec::new();
    if let Some(model) = session.model.as_deref().filter(|model| !model.is_empty()) {
        flags.push("--model".into());
        flags.push(model.to_string());
    }
    flags.extend(effort_flags(session.agent, session.effort.as_deref()));
    if let (Agent::Claude, Some(file)) = (session.agent, settings_file.filter(|f| !f.is_empty())) {
        flags.push("--settings".into());
        flags.push(file.to_string());
    }
    flags.extend(extra.iter().cloned());
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
    let args = session_args(session, settings_file, &[]);
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

#[cfg(test)]
mod effort_tests {
    use super::*;

    /// Effort goes beside `--model`, in each agent's own form; an unknown
    /// level is never passed on.
    #[test]
    fn effort_reaches_the_command_line() {
        let mut session = Session::new("p", Agent::Claude, "t");
        session.started = true;
        session.model = Some("fable".into());
        session.effort = Some("xhigh".into());
        let args = session_args(&session, None, &[]);
        let at = args
            .iter()
            .position(|arg| arg == "--effort")
            .expect("--effort");
        assert_eq!(args[at + 1], "xhigh");

        session.agent = Agent::Codex;
        session.effort = Some("high".into());
        let args = session_args(&session, None, &[]);
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-c", "model_reasoning_effort=high"]));

        session.effort = Some("rm -rf".into());
        assert!(effort_flags(Agent::Claude, session.effort.as_deref()).is_empty());
        assert!(effort_flags(Agent::Codex, session.effort.as_deref()).is_empty());
        assert!(effort_flags(Agent::Claude, Some("ultra")).is_empty());
        assert_eq!(
            effort_flags(Agent::Codex, Some("ultra")),
            ["-c", "model_reasoning_effort=ultra"]
        );
        assert!(!valid_effort(Agent::Codex, ""));
        assert!(!valid_effort(Agent::Codex, "High"));
        assert!(valid_effort(Agent::Codex, "extra_high"));
    }
}
