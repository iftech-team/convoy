//! What the front end may ask for.
//!
//! Every command is a thin call into `convoy-core`. Nothing decides anything
//! here: the web view shows state and passes input back, exactly as the GTK
//! window did.

use convoy_core::model::{Agent, Session};
use convoy_core::{Storage, Workspace as CoreWorkspace};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use crate::pty::Terminals;

pub struct Workspace {
    storage: Storage,
    inner: Mutex<Option<CoreWorkspace>>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace {
            storage: Storage::default(),
            inner: Mutex::new(None),
        }
    }

    /// Opens the workspace on first use and keeps it. A damaged or future file
    /// is reported rather than replaced, so the error reaches the user intact.
    fn with<T>(
        &self,
        action: impl FnOnce(&mut CoreWorkspace) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self.inner.lock().map_err(|_| "Workspace unavailable.")?;
        if guard.is_none() {
            let loaded = CoreWorkspace::load(self.storage.workspace_file())
                .map_err(|error| error.to_string())?;
            *guard = Some(loaded);
        }
        action(guard.as_mut().expect("just loaded"))
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace::new()
    }
}

/// One row of the project list.
#[derive(Serialize)]
pub struct ProjectView {
    id: String,
    title: String,
    path: String,
    group: Option<String>,
    icon: Option<String>,
    sessions: usize,
    running: usize,
}

/// One row of the session list, shaped the way the list actually reads it.
#[derive(Serialize)]
pub struct SessionView {
    id: String,
    title: String,
    agent: String,
    provider_id: String,
    started: bool,
    running: bool,
    archived: bool,
    pinned: bool,
    notes: String,
    branch: Option<String>,
    review_of: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceView {
    projects: Vec<ProjectView>,
    running: Vec<String>,
    storage: String,
}

fn view_of(session: &Session, running: bool) -> SessionView {
    SessionView {
        id: session.id.clone(),
        title: session.title.clone(),
        agent: session.agent.as_str().to_string(),
        provider_id: session.provider_id.clone(),
        started: session.started,
        running,
        archived: session.is_archived(),
        pinned: session.is_pinned(),
        notes: session.notes.clone().unwrap_or_default(),
        branch: session.branch.clone(),
        review_of: session.review_of.clone(),
    }
}

#[tauri::command]
pub fn workspace_read(
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<WorkspaceView, String> {
    let running = terminals.ids();
    let storage = workspace.storage.root().to_string_lossy().into_owned();
    workspace.with(|workspace| {
        let state = workspace.state();
        let projects = state
            .projects
            .iter()
            .map(|project| {
                let own: Vec<&Session> = state
                    .sessions
                    .iter()
                    .filter(|session| session.project_id == project.id)
                    .collect();
                ProjectView {
                    id: project.id.clone(),
                    title: project.title.clone(),
                    path: project.path.to_string_lossy().into_owned(),
                    group: project.group.clone(),
                    icon: project.icon.clone(),
                    sessions: own.iter().filter(|session| !session.is_archived()).count(),
                    running: own
                        .iter()
                        .filter(|session| running.contains(&session.id))
                        .count(),
                }
            })
            .collect();
        Ok(WorkspaceView {
            projects,
            running: running.clone(),
            storage: storage.clone(),
        })
    })
}

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
        // Pinned first, as everywhere else.
        sessions.sort_by_key(|session| !session.pinned);
        Ok(sessions)
    })
}

/// Starts or resumes a session. The launch itself is planned by the core: the
/// folder, the account, the hook settings and the argument vector all come
/// from the same rules the GTK build used.
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
    let plan = workspace.with(|workspace| {
        convoy_core::session::plan_launch(workspace, &Storage::default(), &id, &executable)
            .map_err(|error| error.to_string())
    })?;

    let env: Vec<(String, String)> = plan.env.clone().into_iter().collect();
    terminals.start(
        &app,
        &id,
        &plan.spec.file,
        &plan.spec.args,
        &plan.directory,
        &env,
        cols,
        rows,
    )?;

    // Recorded only once the process exists, and the process is stopped if
    // recording fails: an agent the workspace does not know about would be
    // invisible and unstoppable.
    let recorded = workspace.with(|workspace| {
        convoy_core::session::mark_started(workspace, &plan).map_err(|error| error.to_string())
    });
    if let Err(error) = recorded {
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

/// Kept for the agents a session may use; the front end sends the word, not
/// the enum.
#[allow(dead_code)]
fn agent_of(value: &str) -> Option<Agent> {
    Agent::parse(value)
}
