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
use crate::helpers::{self, ManagerLaunch};
use crate::provider::launch::LaunchSpec;
use crate::provider::{permission_flags, session_spec_with};
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
    /// What the agent is told beyond its prompt, as one value: a manager's
    /// instructions about its helpers, then the session's skills. Already in
    /// `spec` for a terminal; a chat passes them its own way.
    pub instructions: Option<String>,
    /// The flags that hand the agent Convoy's MCP servers (see
    /// [`crate::provider::mcp`]). Already in `spec` for a terminal.
    pub mcp_flags: Vec<String>,
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
    plan_launch_with(workspace, storage, id, executable, None)
}

/// As [`plan_launch`]. A manager session launched with `manager` gets the
/// variables and instructions its helpers need; every other launch has those
/// variables removed, so nothing inherits a manager's secret.
pub fn plan_launch_with(
    workspace: &Workspace,
    storage: &Storage,
    id: &str,
    executable: &Path,
    manager: Option<&ManagerLaunch>,
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

    let mut account = prepare_account(workspace, storage, &session)?;
    for key in helpers::ENV_KEYS {
        account.env.remove(key);
    }
    let manager = manager.filter(|_| session.is_manager() && session.parent_id.is_none());
    let managing = manager.map(|launch| {
        account.env.extend(launch.env(&session.id));
        helpers::instructions(crate::Platform::current())
    });
    // One value: Claude takes one appended system prompt and Codex one set
    // of developer instructions; a second flag would replace the first.
    let instructions =
        crate::skills::combine(&[managing, crate::skills::for_session(storage, &session)?]);
    // Tracker connections that come with an MCP server. A broken
    // integrations file must not stop sessions from starting.
    let connections = crate::integrations::Integrations::load(storage.integrations())
        .map(|integrations| integrations.connections().to_vec())
        .unwrap_or_default();
    // The project's link, so a Peshbar server works in the linked workspace.
    let link = crate::integrations::ProjectLinks::load(storage.integration_links())
        .ok()
        .and_then(|links| links.get(&session.project_id).cloned());
    let mcp_flags = crate::provider::mcp::flags(
        session.agent,
        &crate::provider::mcp::servers(&connections, link.as_ref()),
        &storage.telemetry(),
        &session.id,
        &mut account.env,
    )?;
    let settings = workspace.settings();
    let settings_file = match session.agent {
        Agent::Claude if settings.status_hooks || settings.claude_usage => {
            Some(telemetry::configuration_with(
                &storage.telemetry(),
                &session,
                executable,
                settings.claude_usage,
                settings.status_hooks,
            )?)
        }
        _ => None,
    };
    // Choosing the folder in Convoy is the consent; without this every new
    // worktree would stop on the agent's "trust this folder?" prompt.
    if settings.auto_trust {
        crate::trust::approve(session.agent, &directory, &account.env);
        for dir in session.add_dirs.as_deref().unwrap_or_default() {
            crate::trust::approve(session.agent, dir, &account.env);
        }
    }
    let spec = session_spec_with(
        &session,
        settings_file
            .as_ref()
            .map(|file| file.to_string_lossy())
            .as_deref(),
        &account.env,
        &[
            permission_flags(session.agent, workspace.settings()),
            crate::provider::add_dir_flags(session.add_dirs.as_deref()),
            crate::provider::knowledge::flags(session.agent, &storage.knowledge(), &session.id),
            mcp_flags.clone(),
            instructions
                .as_deref()
                .map(|text| helpers::terminal_flags(session.agent, text))
                .unwrap_or_default(),
        ]
        .concat(),
    );

    Ok(LaunchPlan {
        session_id: id.to_string(),
        directory,
        spec,
        env: account.env,
        account_home: account.home,
        settings_file,
        resume: session.started,
        instructions,
        mcp_flags,
    })
}

/// A chat launch: the terminal launch's checks and preparation, with the
/// agent on JSON pipes instead of a screen.
#[derive(Debug)]
pub struct ChatPlan {
    /// As [`plan_launch`] made it, with `spec` replaced by the chat command.
    pub launch: LaunchPlan,
    /// The protocol for this agent, set up to start or resume the
    /// conversation the session records.
    pub driver: crate::chat::Driver,
    /// A first launch's prompt, sent as the first user turn. A terminal passes
    /// it on the command line; a chat has no such argument.
    pub first_prompt: Option<String>,
}

/// Everything [`plan_launch`] checks and prepares — account, hook settings,
/// trusted folder, resume — for a chat. Claude gets the stream-json flags and
/// the same conversation, model, settings and permission flags; Codex its
/// app server, with the model and permission choice sent when the thread
/// opens. Wrapped in the login shell, or resolved on Windows, exactly as a
/// terminal launch is.
pub fn plan_chat_launch(
    workspace: &Workspace,
    storage: &Storage,
    id: &str,
    executable: &Path,
) -> Result<ChatPlan> {
    plan_chat_launch_with(workspace, storage, id, executable, None)
}

/// As [`plan_chat_launch`], with a manager's launch as [`plan_launch_with`]
/// takes it: Claude gets the instructions as an appended system prompt, Codex
/// as the thread's developer instructions.
pub fn plan_chat_launch_with(
    workspace: &Workspace,
    storage: &Storage,
    id: &str,
    executable: &Path,
    manager: Option<&ManagerLaunch>,
) -> Result<ChatPlan> {
    use crate::chat::{claude, codex, Driver};

    let mut launch = plan_launch_with(workspace, storage, id, executable, manager)?;
    let session = workspace.session(id)?;
    let settings = workspace.settings();
    let model = session.model.as_deref().filter(|model| !model.is_empty());
    let effort = session
        .effort
        .as_deref()
        .filter(|effort| crate::provider::valid_effort(session.agent, effort));
    let (args, driver) = match session.agent {
        Agent::Claude => (
            claude::args(
                &session.provider_id,
                launch.resume,
                model,
                launch
                    .settings_file
                    .as_ref()
                    .map(|file| file.to_string_lossy())
                    .as_deref(),
                &[
                    permission_flags(Agent::Claude, settings),
                    crate::provider::add_dir_flags(session.add_dirs.as_deref()),
                    crate::provider::effort_flags(Agent::Claude, session.effort.as_deref()),
                    crate::provider::knowledge::flags(
                        Agent::Claude,
                        &storage.knowledge(),
                        &session.id,
                    ),
                    launch.mcp_flags.clone(),
                    launch
                        .instructions
                        .as_deref()
                        .map(|text| helpers::terminal_flags(Agent::Claude, text))
                        .unwrap_or_default(),
                ]
                .concat(),
            ),
            Driver::Claude(
                claude::ClaudeStream::new(session.provider_id.clone()).with_choice(model, effort),
            ),
        ),
        // `-c` is a top-level Codex option, so it goes before `app-server`.
        Agent::Codex => (
            [launch.mcp_flags.clone(), codex::args()].concat(),
            Driver::Codex(codex::CodexClient::new(codex::Options {
                cwd: launch.directory.to_string_lossy().into_owned(),
                model: model.map(str::to_string),
                effort: effort.map(str::to_string),
                // A Codex session that never printed its thread id cannot be
                // resumed exactly, so it starts a new thread.
                resume: (launch.resume && !session.provider_id.is_empty())
                    .then(|| session.provider_id.clone()),
                yolo: settings.yolo_codex,
                developer_instructions: launch.instructions.clone(),
            })),
        ),
    };
    launch.spec = crate::provider::provider_spec_for(
        session.agent,
        &args,
        &launch.env,
        crate::Platform::current(),
    )?;
    let first_prompt = if launch.resume {
        session
            .resume_prompt
            .clone()
            .filter(|prompt| !prompt.is_empty())
    } else {
        (!session.prompt.is_empty()).then(|| session.prompt.clone())
    };
    Ok(ChatPlan {
        launch,
        driver,
        first_prompt,
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
        // Handed to the agent by this launch.
        session.resume_prompt = None;
        let task_id = session.task_id.clone();
        if let Some(task) =
            task_id.and_then(|task_id| state.tasks.iter_mut().find(|task| task.id == task_id))
        {
            task.status = TaskStatus::Building;
            task.last_error = None;
            // Every round of building reports on a run of its own.
            if task.stage_run.is_none() {
                task.stage_run = Some(crate::workspace::new_id());
            }
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
            // A task the pipeline already moved on — its round reported by
            // file, then the builder put to sleep — stays where it is.
            if !matches!(
                task.status,
                TaskStatus::Building | TaskStatus::Queued | TaskStatus::Changes
            ) {
                return Ok(());
            }
            let superseded = task
                .spec_id
                .as_deref()
                .and_then(|spec_id| state.specs.iter().find(|spec| spec.id == spec_id))
                .is_some_and(|spec| task.spec_revision != Some(spec.revision));

            let status = if superseded {
                TaskStatus::Changes
            } else {
                // A task whose session opened a pull request awaits that
                // pull request rather than review here, as on macOS.
                let finished = if task.pr_url.is_some() {
                    TaskStatus::Pr
                } else {
                    TaskStatus::Review
                };
                match cause {
                    ExitCause::Hibernated => finished,
                    _ if cause.failed() => TaskStatus::Failed,
                    _ => finished,
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

/// `codex resume <id>`, which Codex prints as it exits.
pub fn codex_resume_id(text: &str) -> Option<String> {
    crate::patterns::CODEX_RESUME
        .captures(text)
        .map(|found| found[1].to_string())
}

/// The first pull or merge request link in `text`.
pub fn pull_request_url(text: &str) -> Option<String> {
    crate::patterns::PULL_REQUEST
        .find(text)
        .map(|found| found.as_str().to_string())
}

/// Records the conversation id Codex printed, so the session resumes it
/// exactly. Only a Codex session that has none yet takes it. Returns whether
/// anything changed.
pub fn record_provider_id(workspace: &mut Workspace, session_id: &str, id: &str) -> Result<bool> {
    let wanted = workspace.state().sessions.iter().any(|session| {
        session.id == session_id
            && session.agent == crate::model::Agent::Codex
            && session.provider_id.is_empty()
    });
    if !wanted {
        return Ok(false);
    }
    let (session_id, id) = (session_id.to_string(), id.to_string());
    workspace.update(move |state| {
        if let Some(session) = state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.provider_id = id;
        }
        Ok(())
    })?;
    Ok(true)
}

/// Records a pull request the agent printed on the task its session runs.
/// A task that is building or awaiting review now awaits the pull request.
/// Returns whether anything changed.
pub fn record_pull_request(workspace: &mut Workspace, session_id: &str, url: &str) -> Result<bool> {
    let known = workspace.state().tasks.iter().any(|task| {
        task.session_id.as_deref() == Some(session_id) && task.pr_url.as_deref() != Some(url)
    });
    if !known {
        return Ok(false);
    }
    let (session, link) = (session_id.to_string(), url.to_string());
    workspace.update(move |state| {
        if let Some(task) = state
            .tasks
            .iter_mut()
            .find(|task| task.session_id.as_deref() == Some(session.as_str()))
        {
            task.pr_url = Some(link);
            if task.status == TaskStatus::Review {
                task.status = TaskStatus::Pr;
            }
        }
        Ok(())
    })?;
    workspace.record(
        ActivityKind::Done,
        session_id,
        &format!("Pull request: {url}"),
    )?;
    Ok(true)
}
