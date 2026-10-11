//! Sessions: creating them, editing them, running them, and handing work on.

use super::{directory_or_project, view_of, Repositories, SessionView, Workspace};
use convoy_core::model::Agent;
use convoy_core::session::{mark_started, plan_launch_with};
use convoy_core::workspace::{NewSession, SessionPatch};
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::chat::{is_running, running_ids, Chats};
use crate::pty::{Launch, Terminals};

#[tauri::command]
pub fn sessions_for(
    project_id: String,
    archived: bool,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<Vec<SessionView>, String> {
    let running = running_ids(&terminals, &chats);
    workspace.with(|workspace| {
        let state = workspace.state();
        let mut sessions: Vec<SessionView> = state
            .sessions
            .iter()
            .filter(|session| {
                session.project_id == project_id && (archived || !session.is_archived())
            })
            .map(|session| view_of(session, running.contains(&session.id), &state.settings))
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
    chats: State<'_, Arc<Chats>>,
) -> Result<SessionView, String> {
    let running = is_running(&terminals, &chats, &id);
    workspace.act(|workspace| {
        Ok(view_of(
            workspace.session(&id)?,
            running,
            workspace.settings(),
        ))
    })
}

/// A conversation picked as background for a new session: a session, or a
/// saved conversation no session holds.
#[derive(Deserialize)]
pub struct KnowledgeItem {
    pub session_id: Option<String>,
    pub agent: Option<Agent>,
    pub provider_id: Option<String>,
    pub title: Option<String>,
}

/// Creates a session. Its title, model name and review relationship are all
/// checked by the core, so the rules are the ones every build enforces.
/// Conversations picked as background are written to one file the first
/// message points at; reading them happens off the main thread.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn session_create(
    project_id: String,
    agent: String,
    title: String,
    prompt: String,
    model: String,
    effort: Option<String>,
    profile_id: Option<String>,
    notes: Option<String>,
    view: Option<String>,
    knowledge: Option<Vec<KnowledgeItem>>,
    manager: Option<bool>,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let agent = Agent::parse(&agent).ok_or("Unknown agent.")?;
    let storage = workspace.storage.clone();
    let knowledge = knowledge.unwrap_or_default();

    // What to read, resolved against the workspace for sessions.
    let (sources, homes) = if knowledge.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        workspace.act(|core| {
            let mut sources: Vec<(String, Agent, String)> = Vec::new();
            for item in &knowledge {
                match &item.session_id {
                    Some(id) => {
                        let session = core.session(id)?;
                        sources.push((
                            session.title.clone(),
                            session.agent,
                            session.provider_id.clone(),
                        ));
                    }
                    None => {
                        if let (Some(agent), Some(provider_id)) = (item.agent, &item.provider_id) {
                            let title = item.title.clone().unwrap_or_else(|| provider_id.clone());
                            sources.push((title, agent, provider_id.clone()));
                        }
                    }
                }
            }
            Ok((sources, agent_homes(core, &storage)?))
        })?
    };
    let digest = if sources.is_empty() {
        None
    } else {
        let digest = tauri::async_runtime::spawn_blocking(move || {
            let sources: Vec<_> = sources
                .into_iter()
                .filter_map(|(title, agent, provider_id)| {
                    transcript_file(&homes, agent, &provider_id)
                        .map(|file| convoy_core::provider::knowledge::Source { title, agent, file })
                })
                .collect();
            (!sources.is_empty()).then(|| convoy_core::provider::knowledge::digest(&sources))
        })
        .await
        .map_err(|error| error.to_string())?;
        Some(digest.ok_or("The picked conversations could not be found on this computer.")?)
    };

    workspace.act(|workspace| {
        let mut input = NewSession::new(project_id, agent, title);
        input.prompt = prompt;
        input.model = model;
        input.effort = effort.unwrap_or_default();
        // The account it signs in with; none means the system login.
        input.profile_id = profile_id.filter(|id| !id.is_empty());
        let state = workspace.add_session(input)?;
        let id = state
            .sessions
            .last()
            .map(|session| session.id.clone())
            .unwrap_or_default();
        if let Some(digest) = digest {
            let folder = storage.knowledge();
            let file = folder.join(format!("{id}.md"));
            let written =
                std::fs::create_dir_all(&folder).and_then(|_| std::fs::write(&file, digest));
            if let Err(error) = written {
                workspace.remove_sessions(std::slice::from_ref(&id))?;
                return Err(convoy_core::ConvoyError::message(format!(
                    "Could not write the earlier conversations: {error}"
                )));
            }
            let note = convoy_core::provider::knowledge::note(&file);
            workspace.update(|state| {
                if let Some(session) = state.sessions.iter_mut().find(|session| session.id == id) {
                    session.prompt = if session.prompt.trim().is_empty() {
                        format!("{note} Wait for the person's request.")
                    } else {
                        format!("{note}\n\n{}", session.prompt)
                    };
                }
                Ok(())
            })?;
        }
        if let Some(notes) = notes.filter(|notes| !notes.trim().is_empty()) {
            workspace.edit_session(
                &id,
                SessionPatch {
                    notes: Some(notes),
                    ..Default::default()
                },
                false,
            )?;
        }
        // Chosen in the sheet; none follows the settings default.
        if let Some(view) = view.filter(|view| !view.is_empty()) {
            workspace.set_session_view(&id, &view)?;
        }
        // A manager can start helper sessions from its shell.
        if manager == Some(true) {
            workspace.set_session_manager(&id, true)?;
        }
        Ok(id)
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
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let running = is_running(&terminals, &chats, &id);
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
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let running = is_running(&terminals, &chats, &id);
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
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let running = is_running(&terminals, &chats, &id);
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
#[allow(clippy::too_many_arguments)]
pub fn session_start(
    app: AppHandle,
    id: String,
    cols: u16,
    rows: u16,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
    helpers: State<'_, crate::helpers::Helpers>,
) -> Result<(), String> {
    start_terminal(
        &app,
        &id,
        cols,
        rows,
        &workspace,
        &terminals,
        &chats,
        Some(&helpers),
    )
}

/// Starts a session's agent in a terminal: the window's start, and an
/// automation's when it runs as one.
#[allow(clippy::too_many_arguments)]
pub fn start_terminal<R: tauri::Runtime>(
    app: &AppHandle<R>,
    id: &str,
    cols: u16,
    rows: u16,
    workspace: &Workspace,
    terminals: &Arc<Terminals>,
    chats: &Chats,
    helpers: Option<&crate::helpers::Helpers>,
) -> Result<(), String> {
    // One process per session: the chat's goes before the terminal's starts.
    if chats.running(id) {
        return Err("This session is running in the chat. Stop it first.".into());
    }
    let revoke = || {
        if let Some(helpers) = helpers {
            helpers.revoke(id);
        }
    };
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let storage = workspace.storage.clone();
    let manager = helpers.and_then(|helpers| helpers.launch_for(workspace, id));
    let plan = workspace
        .act(|workspace| plan_launch_with(workspace, &storage, id, &executable, manager.as_ref()))
        .inspect_err(|_| revoke())?;

    let env: Vec<(String, String)> = plan.env.clone().into_iter().collect();
    crate::pty::start(
        terminals,
        app,
        Launch {
            id,
            program: &plan.spec.file,
            args: &plan.spec.args,
            cwd: &plan.directory,
            env: &env,
            cols,
            rows,
        },
    )
    .inspect_err(|_| revoke())?;

    // Recorded only once the process exists, and stopped if recording fails:
    // an agent the workspace does not know about would be invisible and
    // unstoppable.
    if let Err(error) = workspace.act(|workspace| mark_started(workspace, &plan)) {
        terminals.stop(id);
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub fn session_stop(
    id: String,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) {
    terminals.stop(&id);
    chats.stop(&id);
}

/// Stopping an agent that has been idle since it said it was finished. The
/// task behind it goes to review, not to failed: nothing went wrong.
#[tauri::command]
pub fn session_hibernate(
    id: String,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) {
    terminals.hibernate(&id);
    chats.hibernate(&id);
}

/// Shows the session as `chat` or `terminal` from now on. A running agent is
/// left alone: the window stops it and starts it in the other mode itself.
#[tauri::command]
pub fn session_view(
    id: String,
    view: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.set_session_view(&id, &view).map(|_| ()))
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

/// Saves what a session's terminal shows, as the page renders it, so a review
/// brief, the saved output and a restart all have it.
#[tauri::command]
pub fn session_snapshot(
    id: String,
    text: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let history = convoy_core::history::History::new(workspace.storage.history());
    workspace.act(|workspace| {
        workspace.session(&id)?;
        history.save(&id, &text)
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

/// Hands the work to a reviewer in the same folder, told not to edit. The
/// other agent unless one is picked; the model and account are the
/// reviewer's own.
#[tauri::command]
pub fn review_create(
    id: String,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    profile_id: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    let agent = match agent.as_deref() {
        Some(name) => Some(Agent::parse(name).ok_or("Unknown agent.")?),
        None => None,
    };
    workspace.act(|workspace| {
        let mut input = convoy_core::review::handoff(workspace, &id, prompt)?;
        if let Some(agent) = agent {
            input.agent = agent;
        }
        input.model = model.unwrap_or_default();
        input.effort = effort.unwrap_or_default();
        input.profile_id = profile_id.filter(|id| !id.is_empty());
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
    base: Option<String>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
    repositories: State<'_, Repositories>,
) -> Result<WorktreePlanView, String> {
    let running: Vec<String> = running_ids(&terminals, &chats);
    let root = workspace.storage.worktrees();
    let mut plan = workspace.act(|core| {
        let busy = |id: &str| running.iter().any(|other| other == id);
        convoy_core::worktree::plan_create(core, &id, &branch, &busy)
    })?;
    // A ref chosen in the New Session sheet wins over the project's default.
    if let Some(base) = base
        .map(|base| base.trim().to_string())
        .filter(|base| !base.is_empty())
    {
        plan.base_ref = Some(base);
    }

    let directory = repositories.with(&plan.project_path, || {
        convoy_core::Git::default()
            .create_worktree_from(
                &plan.project_path,
                &root,
                &plan.branch,
                plan.base_ref.as_deref(),
            )
            .map_err(|error| error.to_string())
    })?;
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
        // Settings' global setup first, then the project's own.
        let (shared, command) = convoy_core::worktree::setup_parts(core, &project);
        Ok((project.path, directory, shared, command))
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
    chats: State<'_, Arc<Chats>>,
) -> Result<WorktreeRemovalView, String> {
    let running: Vec<String> = running_ids(&terminals, &chats);
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
    chats: State<'_, Arc<Chats>>,
    repositories: State<'_, Repositories>,
) -> Result<(), String> {
    let running: Vec<String> = running_ids(&terminals, &chats);
    let root = workspace.storage.worktrees();
    let plan = workspace.act(|core| {
        let busy = |id: &str| running.iter().any(|other| other == id);
        convoy_core::worktree::plan_remove(core, &id, &root, &busy)
    })?;
    repositories.with(&plan.project_path, || {
        convoy_core::worktree::remove_checkout(&convoy_core::Git::default(), &plan)
            .map_err(|error| error.to_string())
    })?;
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
    chats: State<'_, Arc<Chats>>,
) -> Result<(), String> {
    let (text, submit) =
        workspace.act(|core| convoy_core::review::quick_command(core, &session_id, &command_id))?;
    // A chat has no half-typed line: a command that should not submit can
    // only go to a terminal.
    if chats.running(&session_id) {
        if !submit {
            return Err("Open the terminal view to insert text without sending it.".into());
        }
        return chats.send(&session_id, &text, &[]);
    }
    if !terminals.running(&session_id) {
        return Err("Start or resume the target session first.".into());
    }
    let payload = convoy_core::history::paste(&text, submit).map_err(|error| error.to_string())?;
    terminals.write(&session_id, &payload)
}

/// Branches, then origin's branches, then tags — each newest first — for the
/// "Create from" picker. A folder that is not a repository has none.
#[tauri::command]
pub fn git_refs(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    let path = workspace.act(|core| Ok(core.project(&project_id)?.path.clone()))?;
    let git = convoy_core::Git::default();
    let list = |args: &[&str]| -> Vec<String> {
        git.run(&path, args)
            .map(|out| {
                out.lines()
                    .map(str::to_string)
                    .filter(|line| !line.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut refs = list(&[
        "for-each-ref",
        "--sort=-committerdate",
        "--format=%(refname:short)",
        "refs/heads",
    ]);
    refs.extend(
        list(&[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/remotes/origin",
        ])
        .into_iter()
        .filter(|name| name != "origin/HEAD" && name != "origin"),
    );
    refs.extend(
        list(&[
            "for-each-ref",
            "--sort=-creatordate",
            "--format=%(refname:short)",
            "refs/tags",
        ])
        .into_iter()
        .map(|name| format!("tag:{name}")),
    );
    Ok(refs)
}

/// The terminal id a profile's login runs under. It is not a session: nothing
/// is recorded in the workspace, and its exit is not a session's exit.
pub fn login_terminal(profile_id: &str) -> String {
    format!("login:{profile_id}")
}

/// Runs the provider's own login inside a profile's account folder, in a
/// terminal the page shows — `claude /login` or `codex login`, as the macOS
/// app does. Returns the terminal's id.
#[tauri::command]
pub fn account_login(
    app: AppHandle,
    profile_id: String,
    cols: u16,
    rows: u16,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<String, String> {
    let storage = workspace.storage.clone();
    let (agent, account) = workspace.act(|core| {
        let profile = core
            .state()
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .cloned()
            .ok_or_else(|| convoy_core::ConvoyError::message("This account no longer exists."))?;
        let mut probe = convoy_core::model::Session::new(String::new(), profile.agent, "login");
        probe.profile_id = Some(profile.id.clone());
        let account = convoy_core::accounts::account_environment(
            &probe,
            &core.state().profiles,
            &storage.accounts(),
            &convoy_core::provider::launch::current_environment(),
        )?;
        std::fs::create_dir_all(&account.home)?;
        Ok((profile.agent, account))
    })?;

    let login: &[&str] = match agent {
        Agent::Claude => &["/login"],
        Agent::Codex => &["login"],
    };
    let (program, args) = if cfg!(windows) {
        let resolved = convoy_core::provider::launch::windows_executable(agent, &account.env)
            .map_err(|error| error.to_string())?;
        let mut args = resolved.prefix;
        args.extend(login.iter().map(|arg| arg.to_string()));
        (resolved.file, args)
    } else {
        convoy_core::provider::launch::shell_command(format!(
            "exec {} {}",
            agent.as_str(),
            login.join(" ")
        ))
    };

    let id = login_terminal(&profile_id);
    if terminals.running(&id) {
        return Ok(id);
    }
    let env: Vec<(String, String)> = account.env.clone().into_iter().collect();
    crate::pty::start(
        &terminals,
        &app,
        Launch {
            id: &id,
            program: &program,
            args: &args,
            cwd: &account.home,
            env: &env,
            cols,
            rows,
        },
    )?;
    Ok(id)
}

/// One row to delete: a session, or a saved conversation no session holds.
#[derive(Deserialize)]
pub struct DeleteItem {
    pub session_id: Option<String>,
    pub agent: Option<Agent>,
    pub provider_id: Option<String>,
}

#[derive(serde::Serialize)]
pub struct DeleteReport {
    pub deleted: usize,
    /// Sessions left alone because they are running.
    pub running: usize,
    /// Rows whose conversation could not be moved to the Trash; kept.
    pub failed: usize,
}

/// Deletes sessions and saved conversations: each conversation's files go to
/// the Trash from every home it could be in (the system login and every
/// account), then its session, if any, is forgotten. A running session is
/// skipped, and one whose files could not be trashed is kept, so a failure
/// never leaves a session pointing at nothing.
#[tauri::command]
pub async fn sessions_delete(
    items: Vec<DeleteItem>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<DeleteReport, String> {
    let storage = workspace.storage.clone();
    let mut running = 0;
    // (session id, agent, provider id), resolved against the workspace so a
    // session's conversation is the one Convoy recorded, not what was sent.
    let (targets, homes) = workspace.act(|core| {
        let mut targets: Vec<(Option<String>, Agent, String)> = Vec::new();
        for item in &items {
            match &item.session_id {
                Some(id) => {
                    if is_running(&terminals, &chats, id) {
                        running += 1;
                        continue;
                    }
                    let session = core.session(id)?;
                    targets.push((Some(id.clone()), session.agent, session.provider_id.clone()));
                }
                None => {
                    if let (Some(agent), Some(provider_id)) = (item.agent, &item.provider_id) {
                        targets.push((None, agent, provider_id.clone()));
                    }
                }
            }
        }
        let homes = agent_homes(core, &storage)?;
        Ok((targets, homes))
    })?;

    let (removed, deleted, failed) = tauri::async_runtime::spawn_blocking(move || {
        let (mut removed, mut deleted, mut failed) = (Vec::new(), 0, 0);
        for (session_id, agent, provider_id) in targets {
            let files: Vec<_> = homes
                .iter()
                .filter(|(kind, _)| *kind == agent)
                .flat_map(|(_, home)| {
                    convoy_core::provider::sweep::conversation_files(agent, home, &provider_id)
                })
                .collect();
            if !files.is_empty() && trash_all(&files).is_err() {
                failed += 1;
                continue;
            }
            deleted += 1;
            removed.extend(session_id);
        }
        (removed, deleted, failed)
    })
    .await
    .map_err(|error| error.to_string())?;

    if !removed.is_empty() {
        workspace.act(|core| core.remove_sessions(&removed).map(|_| ()))?;
        let history = convoy_core::history::History::new(storage.history());
        for id in &removed {
            history.remove(id);
        }
    }
    Ok(DeleteReport {
        deleted,
        running,
        failed,
    })
}

/// Takes sessions out of Convoy and nothing else: their conversations stay
/// where the agent saved them and show again under "Not in Convoy yet". A
/// running session is skipped. Returns how many were removed and how many
/// were running.
#[tauri::command]
pub fn sessions_remove(
    ids: Vec<String>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    chats: State<'_, Arc<Chats>>,
) -> Result<(usize, usize), String> {
    let (idle, running): (Vec<String>, Vec<String>) = ids
        .into_iter()
        .partition(|id| !is_running(&terminals, &chats, id));
    let removed = workspace.act(|core| {
        let known: Vec<String> = idle
            .into_iter()
            .filter(|id| core.session(id).is_ok())
            .collect();
        if !known.is_empty() {
            core.remove_sessions(&known)?;
        }
        Ok(known)
    })?;
    let history = convoy_core::history::History::new(workspace.storage.history());
    for id in &removed {
        history.remove(id);
    }
    Ok((removed.len(), running.len()))
}

/// To the Trash, recoverable. On macOS through the file manager rather than
/// Finder, which would ask to let Convoy control it.
fn trash_all(files: &[std::path::PathBuf]) -> Result<(), trash::Error> {
    #[allow(unused_mut)]
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete_all(files)
}

/// Every home a conversation can be saved under: each agent's system login
/// and every account's, once each.
fn agent_homes(
    core: &convoy_core::Workspace,
    storage: &convoy_core::Storage,
) -> convoy_core::Result<Vec<(Agent, std::path::PathBuf)>> {
    let state = core.state();
    let environment = convoy_core::provider::launch::current_environment();
    let mut homes: Vec<(Agent, std::path::PathBuf)> = Vec::new();
    for agent in [Agent::Claude, Agent::Codex] {
        let mut probe = convoy_core::model::Session::new(String::new(), agent, "homes");
        let profiles = std::iter::once(None).chain(
            state
                .profiles
                .iter()
                .filter(|p| p.agent == agent)
                .map(|p| Some(p.id.clone())),
        );
        for profile in profiles {
            probe.profile_id = profile;
            let account = convoy_core::accounts::account_environment(
                &probe,
                &state.profiles,
                &storage.accounts(),
                &environment,
            )?;
            if !homes
                .iter()
                .any(|(kind, home)| *kind == agent && *home == account.home)
            {
                homes.push((agent, account.home));
            }
        }
    }
    Ok(homes)
}

/// The transcript to read for a conversation: the largest copy, since a
/// conversation copied for a worktree keeps growing only where it resumes.
fn transcript_file(
    homes: &[(Agent, std::path::PathBuf)],
    agent: Agent,
    provider_id: &str,
) -> Option<std::path::PathBuf> {
    homes
        .iter()
        .filter(|(kind, _)| *kind == agent)
        .flat_map(|(_, home)| {
            convoy_core::provider::sweep::conversation_files(agent, home, provider_id)
        })
        .filter(|file| file.extension().is_some_and(|ext| ext == "jsonl"))
        .max_by_key(|file| std::fs::metadata(file).map(|meta| meta.len()).unwrap_or(0))
}
