//! Sessions: creating them, editing them, running them, and handing work on.

use super::{directory_or_project, view_of, SessionView, Workspace};
use convoy_core::model::{ActivityKind, Agent};
use convoy_core::session::{mark_started, plan_launch};
use convoy_core::workspace::{NewSession, SessionPatch};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::pty::{Launch, Terminals};

#[tauri::command]
pub fn sessions_for(
    project_id: String,
    archived: bool,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<Vec<SessionView>, String> {
    let running = terminals.ids();
    workspace.with(|workspace| {
        let mut sessions: Vec<SessionView> = workspace
            .state()
            .sessions
            .iter()
            .filter(|session| {
                session.project_id == project_id && (archived || !session.is_archived())
            })
            .map(|session| view_of(session, running.contains(&session.id)))
            .collect();
        sessions.sort_by_key(|session| !session.pinned);
        Ok(sessions)
    })
}

#[tauri::command]
pub fn session_detail(
    id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<SessionView, String> {
    let running = terminals.running(&id);
    workspace.act(|workspace| Ok(view_of(workspace.session(&id)?, running)))
}

/// Creates a session. Its title, model name and review relationship are all
/// checked by the core, so the rules are the ones every build enforces.
#[tauri::command]
pub fn session_create(
    project_id: String,
    agent: String,
    title: String,
    prompt: String,
    model: String,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let agent = Agent::parse(&agent).ok_or("Unknown agent.")?;
    workspace.act(|workspace| {
        let mut input = NewSession::new(project_id, agent, title);
        input.prompt = prompt;
        input.model = model;
        let state = workspace.add_session(input)?;
        Ok(state
            .sessions
            .last()
            .map(|session| session.id.clone())
            .unwrap_or_default())
    })
}

#[derive(Deserialize, Default)]
pub struct SessionInput {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub provider_id: Option<String>,
}

/// Name, notes and provider identity. The identity is what makes an exact
/// resume possible, so it can only change while the session is stopped.
#[tauri::command]
pub fn session_edit(
    id: String,
    input: SessionInput,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let running = terminals.running(&id);
    workspace.act(|workspace| {
        workspace
            .edit_session(
                &id,
                SessionPatch {
                    title: input.title,
                    notes: input.notes,
                    provider_id: input.provider_id,
                    ..Default::default()
                },
                running,
            )
            .map(|_| ())
    })
}

#[tauri::command]
pub fn session_archive(
    id: String,
    archived: bool,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let running = terminals.running(&id);
    workspace.act(|workspace| {
        workspace
            .edit_session(
                &id,
                SessionPatch {
                    archived: Some(archived),
                    ..Default::default()
                },
                running,
            )
            .map(|_| ())
    })
}

#[tauri::command]
pub fn session_pin(
    id: String,
    pinned: bool,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let running = terminals.running(&id);
    workspace.act(|workspace| {
        workspace
            .edit_session(
                &id,
                SessionPatch {
                    pinned: Some(pinned),
                    ..Default::default()
                },
                running,
            )
            .map(|_| ())
    })
}

/// A fresh conversation that keeps the folder and the notes. The old one is
/// left intact rather than resumed into.
#[tauri::command]
pub fn session_recover(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    workspace.act(|workspace| {
        let state = workspace.recover_session(&id)?;
        Ok(state
            .sessions
            .last()
            .map(|session| session.id.clone())
            .unwrap_or_default())
    })
}

/// Starts or resumes a session. The folder, the account, the hook settings and
/// the argument vector all come from `convoy-core`.
#[tauri::command]
pub fn session_start(
    app: AppHandle,
    id: String,
    cols: u16,
    rows: u16,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let storage = workspace.storage.clone();
    let plan = workspace.act(|workspace| plan_launch(workspace, &storage, &id, &executable))?;

    let env: Vec<(String, String)> = plan.env.clone().into_iter().collect();
    terminals.start(
        &app,
        Launch {
            id: &id,
            program: &plan.spec.file,
            args: &plan.spec.args,
            cwd: &plan.directory,
            env: &env,
            cols,
            rows,
        },
    )?;

    // Recorded only once the process exists, and stopped if recording fails:
    // an agent the workspace does not know about would be invisible and
    // unstoppable.
    if let Err(error) = workspace.act(|workspace| mark_started(workspace, &plan)) {
        terminals.stop(&id);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub fn session_stop(id: String, terminals: State<'_, Arc<Terminals>>) {
    terminals.stop(&id);
}

#[tauri::command]
pub fn terminal_write(
    id: String,
    data: String,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    terminals.write(&id, &data)
}

#[tauri::command]
pub fn terminal_resize(
    id: String,
    cols: u16,
    rows: u16,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    terminals.resize(&id, cols, rows)
}

/// Text typed into a running agent: bracketed, control characters stripped,
/// and no Enter unless asked for. Convoy never submits on the user's behalf.
#[tauri::command]
pub fn terminal_paste(
    id: String,
    text: String,
    submit: bool,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    if !terminals.running(&id) {
        return Err("Start or resume the target session first.".into());
    }
    let payload = convoy_core::history::paste(&text, submit).map_err(|error| error.to_string())?;
    terminals.write(&id, &payload)
}

/// The bounded excerpt of what the agent printed. Not a transcript: whatever
/// the CLI put on screen, trimmed to the last 48,000 characters.
#[tauri::command]
pub fn session_output(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    let history = convoy_core::history::History::new(workspace.storage.history());
    workspace.act(|workspace| {
        workspace.session(&id)?;
        history.read(&id)
    })
}

/// The brief a reviewing agent would receive, for the user to edit first.
/// The terminal excerpt it carries is context, never instructions.
#[tauri::command]
pub fn review_brief(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    let history = convoy_core::history::History::new(workspace.storage.history());
    workspace.act(|workspace| {
        workspace.session(&id)?;
        let output = history.read(&id)?;
        convoy_core::review::brief(workspace, &id, &output)
    })
}

/// Hands the work to the other agent, in the same folder, told not to edit.
#[tauri::command]
pub fn review_create(
    id: String,
    prompt: String,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    workspace.act(|workspace| {
        let input = convoy_core::review::handoff(workspace, &id, prompt)?;
        let state = workspace.add_session(input)?;
        Ok(state
            .sessions
            .last()
            .map(|session| session.id.clone())
            .unwrap_or_default())
    })
}

/// Which session review feedback goes to, so the front end can insert it.
#[tauri::command]
pub fn review_builder(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    workspace.act(|workspace| convoy_core::review::builder_of(workspace, &id))
}

#[derive(serde::Serialize)]
pub struct GitStatusView {
    pub branch: String,
    pub changed_files: usize,
}

#[tauri::command]
pub fn git_status(id: String, workspace: State<'_, Workspace>) -> Result<GitStatusView, String> {
    let directory = directory_or_project(&workspace, &id)?;
    let status = convoy_core::Git::default()
        .status(&directory)
        .map_err(|error| error.to_string())?;
    Ok(GitStatusView {
        branch: status.branch,
        changed_files: status.changed_files,
    })
}

#[tauri::command]
pub fn activity_record(
    id: String,
    kind: String,
    detail: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let kind = match kind.as_str() {
        "started" => ActivityKind::Started,
        "resumed" => ActivityKind::Resumed,
        "exited" => ActivityKind::Exited,
        "done" => ActivityKind::Done,
        "waiting" => ActivityKind::Waiting,
        "hibernated" => ActivityKind::Hibernated,
        "worktree" => ActivityKind::Worktree,
        other => return Err(format!("Unknown activity: {other}")),
    };
    workspace.act(|workspace| workspace.record(kind, &id, &detail).map(|_| ()))
}

// ---------------------------------------------------------------------------
// Worktrees.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct WorktreePlanView {
    pub branch: String,
    pub shared_paths: Vec<String>,
    pub setup_command: Option<String>,
    pub directory: String,
}

/// Creates an isolated checkout on a new branch for a session that has not run
/// yet. A review shares the builder's folder, and a session already in
/// progress would have the ground moved under it.
#[tauri::command]
pub fn worktree_create(
    id: String,
    branch: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<WorktreePlanView, String> {
    let running: Vec<String> = terminals.ids();
    let root = workspace.storage.worktrees();
    let plan = workspace.act(|core| {
        let busy = |id: &str| running.iter().any(|other| other == id);
        convoy_core::worktree::plan_create(core, &id, &branch, &busy)
    })?;

    let directory = convoy_core::Git::default()
        .create_worktree(&plan.project_path, &root, &plan.branch)
        .map_err(|error| error.to_string())?;
    workspace
        .act(|core| convoy_core::worktree::record_create(core, &id, &directory, &plan.branch))?;

    Ok(WorktreePlanView {
        branch: plan.branch,
        shared_paths: plan.shared_paths,
        setup_command: plan.setup_command,
        directory: directory.to_string_lossy().into_owned(),
    })
}

/// Copies the project's shared files into a new worktree and runs its setup
/// command. Shown and confirmed by the front end first: the command runs with
/// the user's own permissions.
#[tauri::command]
pub fn worktree_setup(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    let (project_path, directory, shared, command) = workspace.act(|core| {
        let session = core.session(&id)?.clone();
        let project = core.project(&session.project_id)?.clone();
        let directory = session
            .working_directory
            .clone()
            .ok_or_else(|| convoy_core::ConvoyError::message("This session has no worktree."))?;
        let shared: Vec<String> = project
            .shared_paths
            .clone()
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        Ok((project.path, directory, shared, project.setup_command))
    })?;

    convoy_core::worktree::setup(
        &project_path,
        &directory,
        &shared,
        command.as_deref().filter(|value| !value.trim().is_empty()),
        &convoy_core::StdRunner,
    )
    .map_err(|error| error.to_string())
}

#[derive(serde::Serialize)]
pub struct WorktreeRemovalView {
    pub directory: String,
    pub linked: usize,
}

/// What removing a worktree would do, so the front end can say it before
/// asking. Only worktrees this app created, directly under its own root, may
/// be removed.
#[tauri::command]
pub fn worktree_plan_remove(
    id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<WorktreeRemovalView, String> {
    let running: Vec<String> = terminals.ids();
    let root = workspace.storage.worktrees();
    let plan = workspace.act(|core| {
        let busy = |id: &str| running.iter().any(|other| other == id);
        convoy_core::worktree::plan_remove(core, &id, &root, &busy)
    })?;
    Ok(WorktreeRemovalView {
        directory: plan.directory.to_string_lossy().into_owned(),
        linked: plan.linked.len(),
    })
}

/// Removes the checkout and archives the sessions that used it. Git runs
/// without `--force`, so uncommitted work is never discarded, and the branch
/// is kept.
#[tauri::command]
pub fn worktree_remove(
    id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let running: Vec<String> = terminals.ids();
    let root = workspace.storage.worktrees();
    let plan = workspace.act(|core| {
        let busy = |id: &str| running.iter().any(|other| other == id);
        convoy_core::worktree::plan_remove(core, &id, &root, &busy)
    })?;
    convoy_core::worktree::remove_checkout(&convoy_core::Git::default(), &plan)
        .map_err(|error| error.to_string())?;
    workspace.act(|core| convoy_core::worktree::record_remove(core, &plan.directory))
}

// ---------------------------------------------------------------------------
// Quick commands.
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct QuickCommandView {
    pub id: String,
    pub title: String,
    pub text: String,
    pub submit: bool,
    pub project_id: Option<String>,
}

#[tauri::command]
pub fn quick_commands_read(
    project_id: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<Vec<QuickCommandView>, String> {
    workspace.with(|workspace| {
        Ok(workspace
            .state()
            .quick_commands
            .iter()
            .filter(|command| {
                command
                    .project_id
                    .as_deref()
                    .is_none_or(|owner| Some(owner) == project_id.as_deref())
            })
            .map(|command| QuickCommandView {
                id: command.id.clone(),
                title: command.title.clone(),
                text: command.text.clone(),
                submit: command.submit,
                project_id: command.project_id.clone(),
            })
            .collect())
    })
}

#[tauri::command]
pub fn quick_command_save(
    id: String,
    title: String,
    text: String,
    submit: bool,
    project_id: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| {
        workspace
            .save_command(convoy_core::model::QuickCommand {
                id,
                title,
                text,
                submit,
                project_id: project_id.filter(|value| !value.is_empty()),
                unknown: Default::default(),
            })
            .map(|_| ())
    })
}

#[tauri::command]
pub fn quick_command_remove(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|workspace| workspace.remove_command(&id).map(|_| ()))
}

/// Sends a saved command into a running agent, scoped to its project.
#[tauri::command]
pub fn quick_command_send(
    session_id: String,
    command_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let (text, submit) =
        workspace.act(|core| convoy_core::review::quick_command(core, &session_id, &command_id))?;
    if !terminals.running(&session_id) {
        return Err("Start or resume the target session first.".into());
    }
    let payload = convoy_core::history::paste(&text, submit).map_err(|error| error.to_string())?;
    terminals.write(&session_id, &payload)
}
