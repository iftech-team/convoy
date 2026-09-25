//! Review handoff.
//!
//! A review is an ordinary session with three differences: it uses the other
//! provider, it shares the builder's folder and branch, and it is told not to
//! edit anything. Findings come back as text the user inserts into the
//! builder — Convoy never submits on their behalf.

use crate::history::review_brief;
use crate::model::Agent;
use crate::workspace::{NewSession, Workspace};
use crate::{bail, ensure, json::head, Result};

pub const DEFAULT_TEMPLATE: &str =
    "Review correctness, regressions, tests and security. Report findings without editing code.";

/// Builds the brief for reviewing `id`, using the project's own template when
/// it has one.
pub fn brief(workspace: &Workspace, id: &str, output: &str) -> Result<String> {
    let session = workspace.session(id)?;
    let project = workspace.project(&session.project_id)?;
    let global = workspace.settings().review_template.as_str();
    let template = project
        .review_template
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(Some(global).filter(|value| !value.trim().is_empty()))
        .unwrap_or(DEFAULT_TEMPLATE);
    let brief = review_brief(session, output);
    Ok(head(&format!("{template}\n\n{brief}"), 32_000).to_string())
}

/// The session a review handoff creates: the other agent, the same project,
/// linked back to the builder.
pub fn handoff(workspace: &Workspace, id: &str, prompt: String) -> Result<NewSession> {
    let session = workspace.session(id)?.clone();
    let agent = match session.agent {
        Agent::Claude => Agent::Codex,
        Agent::Codex => Agent::Claude,
    };
    let title = head(&format!("Review: {}", session.title), 200).to_string();
    let mut input = NewSession::new(session.project_id.clone(), agent, title);
    input.prompt = prompt;
    input.review_of = Some(session.id);
    Ok(input)
}

/// Which session review feedback goes to. Text is inserted into the builder's
/// terminal without an Enter, so the user decides whether to send it.
pub fn builder_of(workspace: &Workspace, review_id: &str) -> Result<String> {
    let review = workspace.session(review_id)?;
    let Some(builder) = review.review_of.clone() else {
        bail!("This session is not linked to a builder.")
    };
    Ok(builder)
}

/// The quick command to send, checked against the session's project.
pub fn quick_command(
    workspace: &Workspace,
    session_id: &str,
    command_id: &str,
) -> Result<(String, bool)> {
    let session = workspace.session(session_id)?;
    let command = workspace.state().quick_commands.iter().find(|command| {
        command.id == command_id
            && command
                .project_id
                .as_deref()
                .is_none_or(|project| project == session.project_id)
    });
    let Some(command) = command else {
        bail!("Quick command not found for this project.")
    };
    ensure!(
        !command.text.trim().is_empty(),
        "Quick command not found for this project."
    );
    Ok((command.text.clone(), command.submit))
}
