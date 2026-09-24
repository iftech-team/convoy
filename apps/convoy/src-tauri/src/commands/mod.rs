//! What the front end may ask for.
//!
//! Every command is a thin call into `convoy-core`. Nothing is decided here:
//! the web view shows state and passes input back, and the rules it obeys are
//! the same ones the GTK and Electron builds obey, from the same crate.

pub mod files;
pub mod integrations;
pub mod planning;
pub mod project;
pub mod session;

use convoy_core::model::Session;
use convoy_core::{Storage, Workspace as CoreWorkspace};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::State;

use crate::pty::Terminals;

/// The workspace, opened once and kept. A damaged or future file is reported
/// rather than replaced, so the user is told which file to look at.
pub struct Workspace {
    pub storage: Storage,
    inner: Mutex<Option<CoreWorkspace>>,
}

impl Workspace {
    pub fn new() -> Self {
        Workspace {
            storage: Storage::default(),
            inner: Mutex::new(None),
        }
    }

    pub fn with<T>(
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

    /// Runs a core call that returns `Result`, turning its message into the
    /// one the user sees. The wording is already reviewed; it is not rephrased.
    pub fn act<T>(
        &self,
        action: impl FnOnce(&mut CoreWorkspace) -> convoy_core::Result<T>,
    ) -> Result<T, String> {
        self.with(|workspace| action(workspace).map_err(|error| error.to_string()))
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace::new()
    }
}

/// The folder a session works in, or a project's own checkout when the id
/// names a project rather than a session.
pub fn directory_or_project(workspace: &Workspace, id: &str) -> Result<std::path::PathBuf, String> {
    workspace.act(|core| {
        if core.session(id).is_ok() {
            return convoy_core::session::directory_for(core, id);
        }
        Ok(core.project(id)?.path.clone())
    })
}

#[derive(Serialize)]
pub struct ProjectView {
    pub id: String,
    pub title: String,
    pub path: String,
    pub group: Option<String>,
    pub icon: Option<String>,
    pub sessions: usize,
    pub running: usize,
}

#[derive(Serialize)]
pub struct SessionView {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub agent: String,
    pub provider_id: String,
    pub model: String,
    pub notes: String,
    pub started: bool,
    pub running: bool,
    pub archived: bool,
    pub pinned: bool,
    pub branch: Option<String>,
    pub working_directory: Option<String>,
    pub owns_worktree: bool,
    pub worktree_removed: bool,
    pub review_of: Option<String>,
    pub task_id: Option<String>,
}

#[derive(Serialize)]
pub struct WorkspaceView {
    pub projects: Vec<ProjectView>,
    pub running: Vec<String>,
    pub storage: String,
}

pub fn view_of(session: &Session, running: bool) -> SessionView {
    SessionView {
        id: session.id.clone(),
        project_id: session.project_id.clone(),
        title: session.title.clone(),
        agent: session.agent.as_str().to_string(),
        provider_id: session.provider_id.clone(),
        model: session.model.clone().unwrap_or_default(),
        notes: session.notes.clone().unwrap_or_default(),
        started: session.started,
        running,
        archived: session.is_archived(),
        pinned: session.is_pinned(),
        branch: session.branch.clone(),
        working_directory: session
            .working_directory
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        owns_worktree: session.owns_worktree(),
        worktree_removed: session.worktree_removed(),
        review_of: session.review_of.clone(),
        task_id: session.task_id.clone(),
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

/// The settings the front end needs to draw itself.
#[derive(Serialize)]
pub struct SettingsView {
    pub theme: String,
    pub default_agent: String,
    pub font_size: i64,
    pub scrollback: i64,
    pub claude_usage: bool,
    pub notifications: bool,
    pub keep_awake: String,
    pub hibernate_minutes: i64,
    pub shortcuts: std::collections::BTreeMap<String, String>,
    pub storage: String,
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
            notifications: settings.notifications,
            keep_awake: match settings.keep_awake {
                convoy_core::model::KeepAwake::Off => "off",
                convoy_core::model::KeepAwake::Always => "always",
                convoy_core::model::KeepAwake::Sessions => "sessions",
            }
            .to_string(),
            hibernate_minutes: settings.hibernate_minutes,
            shortcuts: settings.shortcuts.clone(),
            storage: storage.clone(),
        })
    })
}

#[derive(serde::Deserialize)]
pub struct SettingsInput {
    pub theme: String,
    pub default_agent: String,
    pub font_size: i64,
    pub scrollback: i64,
    pub claude_usage: bool,
    pub notifications: bool,
    pub keep_awake: String,
    pub hibernate_minutes: i64,
    pub shortcuts: std::collections::BTreeMap<String, String>,
}

#[tauri::command]
pub fn settings_save(input: SettingsInput, workspace: State<'_, Workspace>) -> Result<(), String> {
    use convoy_core::model::{Agent, KeepAwake, Theme};
    let theme = match input.theme.as_str() {
        "dark" => Theme::Dark,
        "light" => Theme::Light,
        "system" => Theme::System,
        other => return Err(format!("Unknown theme: {other}")),
    };
    let keep_awake = match input.keep_awake.as_str() {
        "off" => KeepAwake::Off,
        "always" => KeepAwake::Always,
        "sessions" => KeepAwake::Sessions,
        other => return Err(format!("Unknown keep-awake mode: {other}")),
    };
    let default_agent = Agent::parse(&input.default_agent).ok_or("Unknown agent.")?;
    workspace.act(|workspace| {
        workspace
            .save_settings(convoy_core::workspace::SettingsPatch {
                theme: Some(theme),
                default_agent: Some(default_agent),
                font_size: Some(input.font_size),
                scrollback: Some(input.scrollback),
                claude_usage: Some(input.claude_usage),
                notifications: Some(input.notifications),
                keep_awake: Some(keep_awake),
                hibernate_minutes: Some(input.hibernate_minutes),
                shortcuts: Some(input.shortcuts),
            })
            .map(|_| ())
    })
}
