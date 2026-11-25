//! The ACP agents the user can pick — Antigravity, opencode (and through it
//! DeepSeek and the rest), Gemini CLI, and any they add — with whether each
//! is installed. Claude and Codex are listed by the window itself.

use convoy_core::provider::acp_agents::{self, AcpAgentDef};
use serde::Serialize;
use std::collections::HashMap;
use tauri::State;

use super::Workspace;

#[derive(Serialize)]
pub struct AcpAgentView {
    /// `acp:<id>`, as sessions and tasks name it.
    pub key: String,
    #[serde(flatten)]
    pub definition: AcpAgentDef,
    pub builtin: bool,
    pub installed: bool,
    /// Whether it has a terminal UI of its own; otherwise chat only.
    pub terminal: bool,
}

/// Marks the line the probe prints for each command, so whatever a `.zshrc`
/// prints around it cannot be mistaken for an answer.
const MARK: &str = "__CONVOY_ACP__";

/// Which of `commands` the login shell finds, in one shell for all of them.
fn installed(commands: &[String]) -> HashMap<String, bool> {
    let mut found: HashMap<String, bool> = commands.iter().map(|c| (c.clone(), false)).collect();
    if commands.is_empty() {
        return found;
    }
    if cfg!(windows) {
        for command in commands {
            let ok = std::process::Command::new("where")
                .arg(command)
                .output()
                .is_ok_and(|output| output.status.success());
            found.insert(command.clone(), ok);
        }
        return found;
    }
    let script: Vec<String> = commands
        .iter()
        .enumerate()
        .map(|(index, command)| {
            format!(
                "command -v {} >/dev/null 2>&1 && printf '\\n{MARK}{index}\\n'",
                convoy_core::provider::launch::quote(command)
            )
        })
        .collect();
    let (shell, args) = convoy_core::provider::launch::shell_command(script.join("; "));
    if let Ok(output) = std::process::Command::new(shell).args(args).output() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(index) = line
                .find(MARK)
                .and_then(|at| line[at + MARK.len()..].trim().parse::<usize>().ok())
            {
                if let Some(command) = commands.get(index) {
                    found.insert(command.clone(), true);
                }
            }
        }
    }
    found
}

/// Every ACP agent, shipped and added, with whether it is installed.
/// Probing starts a login shell, so it runs off the main thread.
#[tauri::command]
pub async fn acp_agents_list(workspace: State<'_, Workspace>) -> Result<Vec<AcpAgentView>, String> {
    let agents = workspace.act(|core| Ok(acp_agents::all(core.settings())))?;
    let commands: Vec<String> = agents.iter().map(|agent| agent.executable().to_string()).collect();
    let found = tauri::async_runtime::spawn_blocking(move || installed(&commands))
        .await
        .map_err(|error| error.to_string())?;
    Ok(agents
        .into_iter()
        .map(|agent| AcpAgentView {
            key: agent.key(),
            installed: found.get(agent.executable()).copied().unwrap_or(false),
            terminal: agent.has_terminal(),
            builtin: agent.builtin,
            definition: agent,
        })
        .collect())
}

/// Replaces the user's own agents: the ones they added, and shipped ones
/// they changed. A shipped agent left as shipped is not stored.
#[tauri::command]
pub fn acp_agents_save(
    agents: Vec<AcpAgentDef>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let shipped = acp_agents::builtins();
    let custom: Vec<AcpAgentDef> = agents
        .into_iter()
        .map(|mut agent| {
            agent.builtin = false;
            agent.id = agent.id.trim().to_string();
            agent.name = agent.name.trim().to_string();
            agent.command = agent.command.trim().to_string();
            agent.tui_command = agent.tui_command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            agent.tui_prompt_flag = agent.tui_prompt_flag.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            agent.login_command = agent.login_command.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            agent
        })
        .filter(|agent| {
            !shipped.iter().any(|known| {
                let mut known = known.clone();
                known.builtin = false;
                known == *agent
            })
        })
        .collect();
    workspace.act(|core| {
        core.save_settings(convoy_core::workspace::SettingsPatch {
            acp_agents: Some(custom),
            ..Default::default()
        })
        .map(|_| ())
    })
}
