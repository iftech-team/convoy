//! Port of `launch.cjs`, Linux half only.
//!
//! The Windows PowerShell branch is deliberately absent: this build targets
//! Linux, and the Electron preview remains the Windows implementation.

use crate::workspace::model::{Agent, Session};
use std::collections::BTreeMap;

/// A program and its argument vector, ready for `vte::Terminal::spawn_async`
/// or [`crate::process::ProcessSpec`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub file: String,
    pub args: Vec<String>,
}

/// Environment values bound to a specific account profile.
#[derive(Debug, Clone, Default)]
pub struct Bindings {
    pub claude_config_dir: Option<String>,
    pub codex_home: Option<String>,
}

impl Bindings {
    pub fn pairs(&self) -> Vec<String> {
        let mut pairs = Vec::new();
        if let Some(value) = self.claude_config_dir.as_ref().filter(|v| !v.is_empty()) {
            pairs.push(format!("CLAUDE_CONFIG_DIR={value}"));
        }
        if let Some(value) = self.codex_home.as_ref().filter(|v| !v.is_empty()) {
            pairs.push(format!("CODEX_HOME={value}"));
        }
        pairs
    }
}

/// POSIX single-quoting: everything inside is literal, and an embedded quote
/// is closed, escaped and reopened. Prompts therefore reach the agent exactly
/// as typed, including `$(…)`, backticks and newlines.
pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// `agentArgs()` — the provider command line, including exact conversation
/// resume. A first prompt is passed only on first launch; resume never
/// replays it.
pub fn agent_args(session: &Session, resume: bool) -> Vec<String> {
    let mut args: Vec<String> = match session.agent {
        Agent::Claude => {
            let flag = if resume { "--resume" } else { "--session-id" };
            vec!["claude".into(), flag.into(), session.provider_id.clone()]
        }
        Agent::Codex => {
            let mut args: Vec<String> = vec!["codex".into()];
            if resume {
                args.push("resume".into());
                if !session.provider_id.is_empty() {
                    args.push(session.provider_id.clone());
                }
            }
            args.push("--no-alt-screen".into());
            args
        }
    };
    if !resume && !session.prompt.is_empty() {
        args.push("--".into());
        args.push(session.prompt.clone());
    }
    args
}

/// `launchSpec()` — a login shell is used so the agent sees the same PATH and
/// provider configuration the user gets in a terminal. `$SHELL` is not used:
/// it may be fish, which cannot execute POSIX syntax.
pub fn launch_spec(session: &Session, resume: bool, bindings: &Bindings) -> LaunchSpec {
    let args = agent_args(session, resume);
    let bound = bindings.pairs();
    let prefix = if bound.is_empty() {
        String::new()
    } else {
        format!(
            "env {} ",
            bound
                .iter()
                .map(|pair| quote(pair))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    let command = args
        .iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    LaunchSpec {
        file: "/bin/bash".into(),
        args: vec!["-ilc".into(), format!("exec {prefix}{command}")],
    }
}

/// `agentEnvironment()` — removes the markers that identify *this* process's
/// own conversation, so a nested agent does not inherit its parent's session,
/// while leaving login configuration intact.
pub fn agent_environment(source: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = source
        .iter()
        .filter(|(key, _)| {
            !((key.starts_with("CLAUDE") && key.as_str() != "CLAUDE_CONFIG_DIR")
                || key.as_str() == "CODEX_THREAD_ID")
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    env.insert("CLAUDE_CODE_FORCE_SESSION_PERSISTENCE".into(), "1".into());
    env
}

/// The current process environment as a map, the usual `source` argument.
pub fn current_environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}
