//! What the front end may ask for.
//!
//! Every command is a thin call into `convoy-core`. Nothing decides anything
//! here: the web view shows state and passes input back, exactly as the GTK
//! window did.

use convoy_core::model::{Agent, Session};
use convoy_core::{Storage, Workspace as CoreWorkspace};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use crate::pty::Terminals;

pub struct Workspace {
    pub(crate) storage: Storage,
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
    pub(crate) fn with<T>(
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
        crate::pty::Launch {
            id: &id,
            program: &plan.spec.file,
            args: &plan.spec.args,
            cwd: &plan.directory,
            env: &env,
            cols,
            rows,
        },
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

/// Creates a session. Validation — the title length, the model name, the
/// review relationship — all happens in the core, so the rules are the same
/// ones every other build enforces.
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
    workspace.with(|workspace| {
        let mut input = convoy_core::workspace::NewSession::new(project_id, agent, title);
        input.prompt = prompt;
        input.model = model;
        let state = workspace.add_session(input).map_err(|e| e.to_string())?;
        Ok(state
            .sessions
            .last()
            .map(|session| session.id.clone())
            .unwrap_or_default())
    })
}

/// Archiving is how a session gets out of the way. It is reversible, and it
/// deletes nothing.
#[tauri::command]
pub fn session_archive(
    id: String,
    archived: bool,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<(), String> {
    let running = terminals.running(&id);
    workspace.with(|workspace| {
        workspace
            .edit_session(
                &id,
                convoy_core::workspace::SessionPatch {
                    archived: Some(archived),
                    ..Default::default()
                },
                running,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
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
    workspace.with(|workspace| {
        workspace
            .edit_session(
                &id,
                convoy_core::workspace::SessionPatch {
                    pinned: Some(pinned),
                    ..Default::default()
                },
                running,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
}

/// The settings the front end needs to draw itself.
#[derive(Serialize)]
pub struct SettingsView {
    theme: String,
    default_agent: String,
    font_size: i64,
    scrollback: i64,
    claude_usage: bool,
    storage: String,
}

/// What the settings dialog sends back. One struct rather than eight loose
/// arguments, and every value is re-checked by the core before it is stored.
#[derive(Deserialize)]
pub struct SettingsInput {
    pub theme: String,
    pub default_agent: String,
    pub font_size: i64,
    pub scrollback: i64,
    pub claude_usage: bool,
}

#[tauri::command]
pub fn settings_save(input: SettingsInput, workspace: State<'_, Workspace>) -> Result<(), String> {
    let theme = match input.theme.as_str() {
        "dark" => convoy_core::model::Theme::Dark,
        "light" => convoy_core::model::Theme::Light,
        "system" => convoy_core::model::Theme::System,
        other => return Err(format!("Unknown theme: {other}")),
    };
    let default_agent = Agent::parse(&input.default_agent).ok_or("Unknown agent.")?;
    workspace.with(|workspace| {
        workspace
            .save_settings(convoy_core::workspace::SettingsPatch {
                theme: Some(theme),
                default_agent: Some(default_agent),
                font_size: Some(input.font_size),
                scrollback: Some(input.scrollback),
                claude_usage: Some(input.claude_usage),
                ..Default::default()
            })
            .map(|_| ())
            // "Invalid settings." and the rest arrive unchanged: the wording
            // is already reviewed and the user is the one who reads it.
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub fn settings_read(workspace: State<'_, Workspace>) -> Result<SettingsView, String> {
    let storage = workspace.storage.root().to_string_lossy().into_owned();
    workspace.with(|workspace| {
        let settings = workspace.settings();
        Ok(SettingsView {
            theme: match settings.theme {
                convoy_core::model::Theme::Dark => "dark",
                convoy_core::model::Theme::Light => "light",
                convoy_core::model::Theme::System => "system",
            }
            .to_string(),
            default_agent: settings.default_agent.as_str().to_string(),
            font_size: settings.font_size,
            scrollback: settings.scrollback,
            claude_usage: settings.claude_usage,
            storage: storage.clone(),
        })
    })
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

/// Preserve the existing schema and reuse an already imported folder.
#[tauri::command]
pub fn project_add(path: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    let directory = std::fs::canonicalize(path.trim()).map_err(|e| e.to_string())?;
    workspace.with(|workspace| {
        let state = workspace
            .add_project(&directory)
            .map_err(|e| e.to_string())?;
        state
            .projects
            .iter()
            .find(|p| p.path == directory)
            .map(|p| p.id.clone())
            .ok_or_else(|| "Could not open project.".into())
    })
}

#[tauri::command]
pub fn review_create(
    id: String,
    output: String,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    workspace.with(|workspace| {
        let prompt =
            convoy_core::review::brief(workspace, &id, &output).map_err(|e| e.to_string())?;
        let input =
            convoy_core::review::handoff(workspace, &id, prompt).map_err(|e| e.to_string())?;
        let state = workspace.add_session(input).map_err(|e| e.to_string())?;
        state
            .sessions
            .last()
            .map(|s| s.id.clone())
            .ok_or_else(|| "Could not create review.".into())
    })
}

/// Paste reviewed text into the builder without submitting it.
#[tauri::command]
pub fn review_feedback(
    id: String,
    text: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
) -> Result<String, String> {
    let builder = workspace.with(|workspace| {
        convoy_core::review::builder_of(workspace, &id).map_err(|e| e.to_string())
    })?;
    let text = convoy_core::history::paste(&text, false).map_err(|e| e.to_string())?;
    terminals.write(&builder, &text)?;
    Ok(builder)
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    use tauri::Manager;

    #[test]
    fn folders_and_reviews_survive_reopening_the_existing_workspace() {
        let directory =
            std::env::temp_dir().join(format!("convoy-flow-{}", convoy_core::workspace::new_id()));
        std::fs::create_dir_all(&directory).unwrap();
        let storage = Storage::new(directory.join("state"));
        let app = tauri::test::mock_app();
        app.manage(Workspace {
            storage: storage.clone(),
            inner: Mutex::new(None),
        });
        let project = project_add(directory.to_string_lossy().into_owned(), app.state()).unwrap();
        let again = project_add(directory.to_string_lossy().into_owned(), app.state()).unwrap();
        assert_eq!(project, again);
        let builder = session_create(
            project,
            "claude".into(),
            "Fix login".into(),
            "Handle expired tokens".into(),
            "sonnet".into(),
            app.state(),
        )
        .unwrap();
        let review = review_create(builder.clone(), "Tests pass".into(), app.state()).unwrap();
        let reloaded = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(reloaded.state().projects.len(), 1);
        assert_eq!(
            reloaded.session(&builder).unwrap().model.as_deref(),
            Some("sonnet")
        );
        assert_eq!(reloaded.session(&review).unwrap().agent, Agent::Codex);
        assert_eq!(
            convoy_core::review::builder_of(&reloaded, &review).unwrap(),
            builder
        );
        assert!(reloaded
            .session(&review)
            .unwrap()
            .prompt
            .contains("Tests pass"));
        assert_eq!(
            convoy_core::session::directory_for(&reloaded, &review).unwrap(),
            convoy_core::session::directory_for(&reloaded, &builder).unwrap()
        );
        drop(app);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
