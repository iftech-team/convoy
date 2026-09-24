//! The session lifecycle — everything a start does before the PTY exists,
//! and everything an exit does after it is gone.
//!
//! The spawn itself belongs to the UI, because VTE owns the pseudo-terminal.
//! Splitting it this way keeps every rule here, where it can be tested, and
//! leaves the front end with three calls: plan, confirm, finish.
//!
//! The order matters. `vte::Pty::spawn_async` is asynchronous where
//! `pty.spawn` was not, so the process can exist before the workspace has been
//! updated. [`mark_started`] is therefore called from the spawn callback, and
//! if it fails the caller must kill the process group: otherwise an agent runs
//! against a workspace that does not know about it.

use crate::accounts::{account_environment, Account};
use crate::provider::launch::LaunchSpec;
use crate::provider::session_spec;
use crate::telemetry;
use crate::workspace::model::{ActivityKind, Agent, Session, TaskStatus};
use crate::workspace::Workspace;
use crate::{bail, ensure, ConvoyError, Result, Storage};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Everything needed to start the agent, and nothing that requires a terminal.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub session_id: String,
    pub directory: PathBuf,
    pub spec: LaunchSpec,
    pub env: BTreeMap<String, String>,
    pub account_home: PathBuf,
    pub settings_file: Option<PathBuf>,
    /// True when the agent is resuming an existing conversation rather than
    /// starting one. Decides the activity record and whether the first prompt
    /// is passed at all.
    pub resume: bool,
}

/// Why a session's process is no longer running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCause {
    /// The user pressed Stop.
    Stopped,
    /// Claude was hibernated after an explicit done event and an idle period.
    Hibernated,
    Exited(i32),
}

impl ExitCause {
    fn failed(self) -> bool {
        matches!(self, ExitCause::Stopped) || matches!(self, ExitCause::Exited(code) if code != 0)
    }
}

/// The folder an agent runs in: its worktree if it has one, otherwise the
/// project itself.
pub fn directory_for(workspace: &Workspace, id: &str) -> Result<PathBuf> {
    let session = workspace.session(id)?;
    match &session.working_directory {
        Some(directory) => Ok(directory.clone()),
        None => Ok(workspace.project(&session.project_id)?.path.clone()),
    }
}

/// Everything a start checks and prepares before spawning. Returns an error
/// with fixed, user-facing wording whenever the session cannot run.
///
/// `executable` is the path hooks will invoke — this binary, which re-enters
/// in `--hook` mode.
pub fn plan_launch(
    workspace: &Workspace,
    storage: &Storage,
    id: &str,
    executable: &Path,
) -> Result<LaunchPlan> {
    let session = workspace.session(id)?.clone();
    ensure!(
        !session.is_archived() && !session.worktree_removed(),
        "This session is archived or its worktree was removed."
    );
    let directory = directory_for(workspace, id)?;
    ensure!(
        std::fs::metadata(&directory).is_ok_and(|meta| meta.is_dir()),
        "Session folder is unavailable."
    );

    // A task session must still match the task it was prepared for, and the
    // specification behind it must still be the approved one.
    if let Some(task_id) = session.task_id.as_deref() {
        let task = workspace
            .state()
            .tasks
            .iter()
            .find(|task| task.id == task_id)
            .ok_or_else(|| ConvoyError::message(OUTDATED_BRIEF))?;
        let spec = task.spec_id.as_deref().and_then(|spec_id| {
            workspace
                .state()
                .specs
                .iter()
                .find(|spec| spec.id == spec_id)
        });
        let stale = task.session_id.as_deref() != Some(id)
            || spec
                .is_some_and(|spec| !spec.approved() || task.spec_revision != Some(spec.revision));
        ensure!(!stale, "{OUTDATED_BRIEF}");
    }

    let account = prepare_account(workspace, storage, &session)?;
    let settings_file = match session.agent {
        Agent::Claude => Some(telemetry::configuration(
            &storage.telemetry(),
            &session,
            executable,
            workspace.settings().claude_usage,
        )?),
        Agent::Codex => None,
    };
    let spec = session_spec(
        &session,
        settings_file
            .as_ref()
            .map(|file| file.to_string_lossy())
            .as_deref(),
        &account.env,
    );

    Ok(LaunchPlan {
        session_id: id.to_string(),
        directory,
        spec,
        env: account.env,
        account_home: account.home,
        settings_file,
        resume: session.started,
    })
}

const OUTDATED_BRIEF: &str =
    "This task brief is outdated or unapproved. Approve the spec and prepare a new task brief.";

/// Resolves the account home and creates it for profile-bound sessions.
pub fn prepare_account(
    workspace: &Workspace,
    storage: &Storage,
    session: &Session,
) -> Result<Account> {
    let account = account_environment(
        session,
        &workspace.state().profiles,
        &storage.accounts(),
        &crate::provider::launch::current_environment(),
    )?;
    if session.profile_id.is_some() {
        // Resuming into a home that has vanished would silently sign the
        // session out, so it is refused instead of recreated.
        ensure!(
            !session.started || account.home.exists(),
            "This session\u{2019}s account folder is missing. Restore it before resuming."
        );
        std::fs::create_dir_all(&account.home)?;
    }
    Ok(account)
}

/// Records that the agent is running. Called from the spawn callback; if it
/// fails, the caller kills the process group it has just created.
pub fn mark_started(workspace: &mut Workspace, plan: &LaunchPlan) -> Result<()> {
    let id = plan.session_id.clone();
    let home = plan.account_home.clone();
    workspace.update(move |state| {
        let Some(session) = state.sessions.iter_mut().find(|session| session.id == id) else {
            bail!("Session not found.")
        };
        session.started = true;
        session.agent_home = Some(home);
        let task_id = session.task_id.clone();
        if let Some(task) =
            task_id.and_then(|task_id| state.tasks.iter_mut().find(|task| task.id == task_id))
        {
            task.status = TaskStatus::Building;
            task.last_error = None;
        }
        Ok(())
    })?;
    let kind = if plan.resume {
        ActivityKind::Resumed
    } else {
        ActivityKind::Started
    };
    workspace.record(kind, &plan.session_id, "Agent process launched")?;
    Ok(())
}

/// Applies the exit rules. A clean exit moves a task to **review**, never to
/// done: whether the work is acceptable stays an explicit decision.
pub fn finish_session(workspace: &mut Workspace, id: &str, cause: ExitCause) -> Result<()> {
    let session_id = id.to_string();
    workspace.update({
        let session_id = session_id.clone();
        move |state| {
            let Some(index) = state
                .tasks
                .iter()
                .position(|task| task.session_id.as_deref() == Some(session_id.as_str()))
            else {
                return Ok(());
            };
            let task = &state.tasks[index];
            let superseded = task
                .spec_id
                .as_deref()
                .and_then(|spec_id| state.specs.iter().find(|spec| spec.id == spec_id))
                .is_some_and(|spec| task.spec_revision != Some(spec.revision));

            let status = if superseded {
                TaskStatus::Changes
            } else {
                match cause {
                    ExitCause::Hibernated => TaskStatus::Review,
                    _ if cause.failed() => TaskStatus::Failed,
                    _ => TaskStatus::Review,
                }
            };
            let task = &mut state.tasks[index];
            task.status = status;
            if cause != ExitCause::Hibernated && cause.failed() {
                task.last_error = Some(match cause {
                    ExitCause::Stopped => "Session stopped by user.".to_string(),
                    ExitCause::Exited(code) => format!("Agent exited with code {code}."),
                    ExitCause::Hibernated => unreachable!(),
                });
            }
            Ok(())
        }
    })?;

    let detail = match cause {
        ExitCause::Stopped => "Stopped by user".to_string(),
        ExitCause::Hibernated => "Hibernated after an idle period".to_string(),
        ExitCause::Exited(code) => {
            format!("Process exited with code {code}; task completion is unverified")
        }
    };
    workspace.record(ActivityKind::Exited, &session_id, &detail)?;
    Ok(())
}

/// Whether a clean exit should hand the task on to review and pull the next
/// queued one. Mirrors the condition guarding `finishTask()`.
pub fn completed_cleanly(cause: ExitCause) -> bool {
    cause == ExitCause::Exited(0)
}
