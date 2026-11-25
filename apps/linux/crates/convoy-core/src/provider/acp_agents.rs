//! The agents Convoy reaches through the Agent Client Protocol: a few known
//! ones, and whatever the user adds in Settings. An agent here is only data —
//! the command that speaks ACP on stdio, and optionally the command for its
//! own terminal UI — so adding one needs no code.
//!
//! A session records the agent as [`Agent::Acp`] plus the id here; the window
//! names it `acp:<id>`.

use crate::workspace::model::{Agent, Settings};
use crate::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How the window names an ACP agent: `acp:<id>`.
pub const KEY_PREFIX: &str = "acp:";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpAgentDef {
    /// Lowercase letters, digits and dashes.
    pub id: String,
    pub name: String,
    /// The program that speaks ACP, found through the login shell's PATH.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra variables, such as an API key the agent reads.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// The agent's own terminal UI, for a terminal session; none makes it
    /// chat-only.
    #[serde(rename = "tuiCommand", default, skip_serializing_if = "Option::is_none")]
    pub tui_command: Option<String>,
    #[serde(rename = "tuiArgs", default, skip_serializing_if = "Vec::is_empty")]
    pub tui_args: Vec<String>,
    /// The flag its terminal UI takes a first prompt with; none drops the
    /// prompt there.
    #[serde(rename = "tuiPromptFlag", default, skip_serializing_if = "Option::is_none")]
    pub tui_prompt_flag: Option<String>,
    /// What to run to sign in, in a terminal.
    #[serde(rename = "loginCommand", default, skip_serializing_if = "Option::is_none")]
    pub login_command: Option<String>,
    #[serde(rename = "installUrl", default, skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    /// Shipped with Convoy, as opposed to added by the user.
    #[serde(default, skip_serializing)]
    pub builtin: bool,
}

impl AcpAgentDef {
    /// `acp:<id>`, the window's name for it.
    pub fn key(&self) -> String {
        format!("{KEY_PREFIX}{}", self.id)
    }

    /// The program checked for to say whether it is installed.
    pub fn executable(&self) -> &str {
        &self.command
    }

    pub fn has_terminal(&self) -> bool {
        self.tui_command.as_deref().is_some_and(|c| !c.trim().is_empty())
    }
}

fn builtin(
    id: &str,
    name: &str,
    command: &str,
    args: &[&str],
    tui: Option<(&str, Option<&str>)>,
    login: Option<&str>,
    install: &str,
) -> AcpAgentDef {
    AcpAgentDef {
        id: id.into(),
        name: name.into(),
        command: command.into(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        env: BTreeMap::new(),
        tui_command: tui.map(|(command, _)| command.to_string()),
        tui_args: Vec::new(),
        tui_prompt_flag: tui.and_then(|(_, flag)| flag.map(str::to_string)),
        login_command: login.map(str::to_string),
        install_url: Some(install.into()),
        builtin: true,
    }
}

/// The agents Convoy knows without being told.
pub fn builtins() -> Vec<AcpAgentDef> {
    vec![
        builtin(
            "antigravity",
            "Antigravity",
            "agy",
            &["acp"],
            Some(("agy", Some("--prompt-interactive"))),
            Some("agy login"),
            "https://antigravity.google/docs/cli",
        ),
        // opencode reaches DeepSeek, Kimi, Qwen, OpenRouter and the rest:
        // the provider and key are chosen in opencode itself.
        builtin(
            "opencode",
            "opencode",
            "opencode",
            &["acp"],
            Some(("opencode", Some("--prompt"))),
            Some("opencode auth login"),
            "https://opencode.ai",
        ),
        builtin(
            "gemini",
            "Gemini CLI",
            "gemini",
            &["--acp"],
            Some(("gemini", Some("--prompt-interactive"))),
            None,
            "https://github.com/google-gemini/gemini-cli",
        ),
    ]
}

/// Every agent the user can pick: the shipped ones, with the user's own
/// added — and replacing a shipped one that has the same id.
pub fn all(settings: &Settings) -> Vec<AcpAgentDef> {
    let mut agents = builtins();
    for custom in &settings.acp_agents {
        let mut custom = custom.clone();
        custom.builtin = false;
        match agents.iter_mut().find(|known| known.id == custom.id) {
            Some(known) => *known = custom,
            None => agents.push(custom),
        }
    }
    agents
}

/// The agent with this id, or the reason it cannot be used.
pub fn find(settings: &Settings, id: Option<&str>) -> Result<AcpAgentDef> {
    let Some(id) = id.filter(|id| !id.is_empty()) else {
        bail!("This session names no ACP agent.")
    };
    match all(settings).into_iter().find(|agent| agent.id == id) {
        Some(agent) => Ok(agent),
        None => bail!("The agent \u{201c}{id}\u{201d} is no longer set up. Add it again in Settings."),
    }
}

/// The agent and ACP id a window key names: `claude`, `codex` or
/// `acp:<id>`.
pub fn parse_key(value: &str) -> Option<(Agent, Option<String>)> {
    if let Some(id) = value.strip_prefix(KEY_PREFIX) {
        return valid_id(id).then(|| (Agent::Acp, Some(id.to_string())));
    }
    Agent::parse(value).map(|agent| (agent, None))
}

/// The window key for an agent and its ACP id.
pub fn key(agent: Agent, acp: Option<&str>) -> String {
    match (agent, acp) {
        (Agent::Acp, Some(id)) => format!("{KEY_PREFIX}{id}"),
        (agent, _) => agent.as_str().to_string(),
    }
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn valid_env_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Checks an agent the user typed in, with the wording Settings shows.
pub fn validate(agent: &AcpAgentDef) -> Result<()> {
    ensure!(
        valid_id(&agent.id),
        "The id may only use lowercase letters, digits and dashes."
    );
    ensure!(
        !agent.name.trim().is_empty() && agent.name.chars().count() <= 60,
        "Give the agent a name of up to 60 characters."
    );
    ensure!(
        !agent.command.trim().is_empty() && agent.command.len() <= 300,
        "Give the command that starts the agent."
    );
    ensure!(
        agent.args.len() <= 40 && agent.tui_args.len() <= 40,
        "Too many arguments."
    );
    ensure!(
        agent.env.keys().all(|name| valid_env_name(name)),
        "Environment names may only use letters, digits and underscores."
    );
    ensure!(agent.env.len() <= 40, "Too many environment variables.");
    Ok(())
}

/// The terminal command for a session: the agent's own UI, with the first
/// prompt when its UI takes one.
pub fn terminal_command(agent: &AcpAgentDef, prompt: Option<&str>) -> Result<Vec<String>> {
    let Some(command) = agent.tui_command.as_deref().filter(|c| !c.trim().is_empty()) else {
        bail!("{} has no terminal of its own. Open it as a chat.", agent.name)
    };
    let mut argv = vec![command.to_string()];
    argv.extend(agent.tui_args.iter().cloned());
    if let (Some(flag), Some(prompt)) = (
        agent.tui_prompt_flag.as_deref().filter(|f| !f.is_empty()),
        prompt.filter(|p| !p.trim().is_empty()),
    ) {
        argv.push(flag.to_string());
        argv.push(prompt.to_string());
    }
    Ok(argv)
}

/// The chat command: the program that speaks ACP.
pub fn chat_command(agent: &AcpAgentDef) -> Vec<String> {
    std::iter::once(agent.command.clone())
        .chain(agent.args.iter().cloned())
        .collect()
}

/// A command line run through the user's login shell, as every agent is,
/// with the agent's own variables set.
pub fn shell_spec(
    argv: &[String],
    agent_env: &BTreeMap<String, String>,
) -> crate::provider::launch::LaunchSpec {
    use crate::provider::launch::{quote, shell_command, LaunchSpec};
    let bindings: Vec<String> = agent_env
        .iter()
        .filter(|(name, _)| valid_env_name(name))
        .map(|(name, value)| quote(&format!("{name}={value}")))
        .collect();
    let command: Vec<String> = argv.iter().map(|arg| quote(arg)).collect();
    let (file, args) = shell_command(format!("exec env {} {}", bindings.join(" "), command.join(" ")));
    LaunchSpec { file, args }
}

/// Convoy's MCP servers in ACP's shape: HTTP servers with their headers.
pub fn mcp_servers(servers: &[crate::provider::mcp::McpServer]) -> Vec<serde_json::Value> {
    servers
        .iter()
        .map(|server| {
            let mut headers = vec![serde_json::json!({ "name": "Authorization", "value": format!("Bearer {}", server.token) })];
            if let Some(workspace) = &server.workspace {
                headers.push(serde_json::json!({
                    "name": crate::integrations::scopes::WORKSPACE_HEADER,
                    "value": workspace
                }));
            }
            serde_json::json!({ "type": "http", "name": server.name, "url": server.url, "headers": headers })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_name_the_agent_and_its_id() {
        assert_eq!(parse_key("claude"), Some((Agent::Claude, None)));
        assert_eq!(parse_key("acp:opencode"), Some((Agent::Acp, Some("opencode".into()))));
        assert_eq!(parse_key("acp"), None, "an ACP agent needs its id");
        assert_eq!(parse_key("acp:Bad Id"), None);
        assert_eq!(key(Agent::Acp, Some("agy")), "acp:agy");
        assert_eq!(key(Agent::Codex, None), "codex");
    }

    #[test]
    fn a_users_agent_replaces_a_shipped_one_with_its_id() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        settings.acp_agents.push(AcpAgentDef {
            id: "opencode".into(),
            name: "My opencode".into(),
            command: "/opt/opencode".into(),
            args: vec!["acp".into()],
            env: BTreeMap::new(),
            tui_command: None,
            tui_args: Vec::new(),
            tui_prompt_flag: None,
            login_command: None,
            install_url: None,
            builtin: true,
        });
        let agents = all(&settings);
        assert_eq!(agents.len(), builtins().len());
        let opencode = find(&settings, Some("opencode")).unwrap();
        assert_eq!(opencode.name, "My opencode");
        assert!(!opencode.builtin);
        assert!(find(&settings, Some("missing")).is_err());
    }

    #[test]
    fn commands_carry_the_prompt_only_where_the_ui_takes_one() {
        let agents = builtins();
        let opencode = agents.iter().find(|a| a.id == "opencode").unwrap();
        assert_eq!(
            terminal_command(opencode, Some("hi")).unwrap(),
            vec!["opencode", "--prompt", "hi"]
        );
        let agy = agents.iter().find(|a| a.id == "antigravity").unwrap();
        assert_eq!(
            terminal_command(agy, Some("hi")).unwrap(),
            vec!["agy", "--prompt-interactive", "hi"]
        );
        assert_eq!(terminal_command(agy, None).unwrap(), vec!["agy"]);
        assert_eq!(chat_command(agy), vec!["agy", "acp"]);
        let mut chat_only = agy.clone();
        chat_only.tui_command = None;
        assert!(terminal_command(&chat_only, None).is_err());
    }

    #[test]
    fn typed_in_agents_are_checked() {
        let mut agent = builtins().remove(0);
        assert!(validate(&agent).is_ok());
        agent.id = "Has Space".into();
        assert!(validate(&agent).is_err());
        agent.id = "ok".into();
        agent.env.insert("1BAD".into(), "x".into());
        assert!(validate(&agent).is_err());
    }
}
