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

// ---------------------------------------------------------------------------
// Windows.
// ---------------------------------------------------------------------------

/// A resolved Windows executable: the program, and anything that must precede
/// the agent's own arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub file: String,
    pub prefix: Vec<String>,
}

/// The npm package each agent publishes, used when no native executable is on
/// PATH.
fn npm_entry(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "@anthropic-ai/claude-code/cli.js",
        Agent::Codex => "@openai/codex/bin/codex.js",
    }
}

/// `windowsExecutable()` — finds the agent without going through a shell.
///
/// Windows has no login shell to ask, and `.cmd` shims mangle an argument
/// vector: a prompt containing quotes, `&` or a newline does not survive one.
/// So the native `.exe` is preferred, and failing that the npm package's entry
/// script is run by `node` directly. Either way the arguments are passed
/// literally.
pub fn windows_executable(
    agent: Agent,
    env: &std::collections::BTreeMap<String, String>,
) -> crate::Result<Resolved> {
    // Windows environment names are case-insensitive, and `Path` is the usual
    // spelling rather than `PATH`.
    let path = env
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("path"))
        .map(|(_, value)| value.clone())
        .unwrap_or_default();

    let mut node: Option<String> = None;
    for directory in path.split(';').filter(|part| !part.is_empty()) {
        let directory = std::path::Path::new(directory);

        let executable = directory.join(format!("{}.exe", agent.as_str()));
        if executable.is_file() {
            return Ok(Resolved {
                file: executable.to_string_lossy().into_owned(),
                prefix: Vec::new(),
            });
        }

        // The package may sit under this directory's `node_modules`, or under
        // its parent's — npm puts shims in `<prefix>` and packages one level up.
        for root in [directory.join("node_modules"), parent_of(directory)] {
            let script = root.join(npm_entry(agent));
            if script.is_file() {
                if node.is_none() {
                    node = find_node(&path);
                }
                if let Some(node) = &node {
                    return Ok(Resolved {
                        file: node.clone(),
                        prefix: vec![script.to_string_lossy().into_owned()],
                    });
                }
            }
        }
    }

    crate::bail!(
        "{} was not found. Install the native CLI or its standard npm package and restart Convoy.",
        agent.as_str()
    )
}

fn parent_of(directory: &std::path::Path) -> std::path::PathBuf {
    directory
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| directory.to_path_buf())
}

/// The npm fallback needs a Node to run the entry script with. Electron used
/// its own binary in node mode; this build has no bundled Node, so the one on
/// PATH is used.
fn find_node(path: &str) -> Option<String> {
    for directory in path.split(';').filter(|part| !part.is_empty()) {
        for name in ["node.exe", "node"] {
            let candidate = std::path::Path::new(directory).join(name);
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}
