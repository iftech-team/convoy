//! What the front end may ask for.
//!
//! Every command is a thin call into `convoy-core`. Nothing is decided here:
//! the web view shows state and passes input back, and the rules it obeys are
//! the same ones the GTK and Electron builds obey, from the same crate.

pub mod docs;
pub mod files;
pub mod icons;
pub mod integrations;
pub mod limits;
pub mod macos;
pub mod planning;
pub mod project;
pub mod session;

use convoy_core::model::Session;
use convoy_core::{Storage, Workspace as CoreWorkspace};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
        Workspace::at(Storage::default())
    }

    /// The same thing pointed somewhere else, which is how a test gets one
    /// without writing into the workspace the user is actually using.
    pub fn at(storage: Storage) -> Self {
        Workspace {
            storage,
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

/// One git mutation per repository at a time.
///
/// Tauri runs commands concurrently, so two clicks a moment apart can reach
/// `git` together. Two writers in one checkout is how an index lock is hit, or
/// worse, how a stage and a discard interleave. Reads are left alone: they are
/// short, and `git` handles concurrent readers itself.
#[derive(Default)]
pub struct Repositories {
    inner: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl Repositories {
    /// Runs `action` with this repository to itself. Per directory rather than
    /// one lock for everything, because a push can take seconds and another
    /// project has no reason to wait for it.
    pub fn with<T>(&self, directory: &Path, action: impl FnOnce() -> T) -> T {
        let lock = {
            let Ok(mut repositories) = self.inner.lock() else {
                return action();
            };
            Arc::clone(
                repositories
                    .entry(directory.to_path_buf())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let _held = lock.lock();
        action()
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
    pub color: Option<String>,
    /// What new sessions and tasks here start with, when the project says.
    pub default_agent: Option<String>,
    pub branch_prefix: Option<String>,
    pub task_mode: Option<String>,
    pub auto_run_tasks: bool,
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
    /// Every project's sessions that are not archived, for the sidebar, the
    /// pinned list and the palette, which all span projects.
    pub sessions: Vec<SessionView>,
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
                    color: project.color.clone(),
                    default_agent: project
                        .default_agent
                        .map(|agent| agent.as_str().to_string()),
                    branch_prefix: project.branch_prefix.clone(),
                    task_mode: project.task_mode.clone(),
                    auto_run_tasks: project.auto_run_tasks == Some(true),
                    sessions: own.iter().filter(|session| !session.is_archived()).count(),
                    running: own
                        .iter()
                        .filter(|session| running.contains(&session.id))
                        .count(),
                }
            })
            .collect();
        let sessions = state
            .sessions
            .iter()
            .filter(|session| !session.is_archived())
            .map(|session| view_of(session, running.contains(&session.id)))
            .collect();
        Ok(WorkspaceView {
            projects,
            sessions,
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
    pub worktree_by_default: bool,
    pub branch_prefix: String,
    pub sort_projects: bool,
    pub yolo_claude: bool,
    pub yolo_codex: bool,
    pub review_template: String,
    pub notify_waiting: bool,
    pub notify_done: bool,
    pub notify_when_focused: bool,
    pub notification_sound: String,
    pub auto_trust: bool,
    pub status_hooks: bool,
    pub git_status: bool,
    pub git_poll_seconds: i64,
    pub compact_sidebar: bool,
    pub worktree_setup: String,
    pub worktree_shared: String,
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
            worktree_by_default: settings.worktree_by_default,
            branch_prefix: settings.branch_prefix.clone(),
            sort_projects: settings.sort_projects,
            yolo_claude: settings.yolo_claude,
            yolo_codex: settings.yolo_codex,
            review_template: settings.review_template.clone(),
            notify_waiting: settings.notify_waiting,
            notify_done: settings.notify_done,
            notify_when_focused: settings.notify_when_focused,
            notification_sound: settings.notification_sound.clone(),
            auto_trust: settings.auto_trust,
            status_hooks: settings.status_hooks,
            git_status: settings.git_status,
            git_poll_seconds: settings.git_poll_seconds,
            compact_sidebar: settings.compact_sidebar,
            worktree_setup: settings.worktree_setup.clone(),
            worktree_shared: settings.worktree_shared.clone(),
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
    // Newer settings are optional, so a page that predates them still saves.
    #[serde(default)]
    pub worktree_by_default: Option<bool>,
    #[serde(default)]
    pub branch_prefix: Option<String>,
    #[serde(default)]
    pub sort_projects: Option<bool>,
    #[serde(default)]
    pub yolo_claude: Option<bool>,
    #[serde(default)]
    pub yolo_codex: Option<bool>,
    #[serde(default)]
    pub review_template: Option<String>,
    #[serde(default)]
    pub notify_waiting: Option<bool>,
    #[serde(default)]
    pub notify_done: Option<bool>,
    #[serde(default)]
    pub notify_when_focused: Option<bool>,
    #[serde(default)]
    pub notification_sound: Option<String>,
    #[serde(default)]
    pub auto_trust: Option<bool>,
    #[serde(default)]
    pub status_hooks: Option<bool>,
    #[serde(default)]
    pub git_status: Option<bool>,
    #[serde(default)]
    pub git_poll_seconds: Option<i64>,
    #[serde(default)]
    pub compact_sidebar: Option<bool>,
    #[serde(default)]
    pub worktree_setup: Option<String>,
    #[serde(default)]
    pub worktree_shared: Option<String>,
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
                worktree_by_default: input.worktree_by_default,
                branch_prefix: input.branch_prefix,
                sort_projects: input.sort_projects,
                yolo_claude: input.yolo_claude,
                yolo_codex: input.yolo_codex,
                review_template: input.review_template,
                notify_waiting: input.notify_waiting,
                notify_done: input.notify_done,
                notify_when_focused: input.notify_when_focused,
                notification_sound: input.notification_sound,
                auto_trust: input.auto_trust,
                status_hooks: input.status_hooks,
                git_status: input.git_status,
                git_poll_seconds: input.git_poll_seconds,
                compact_sidebar: input.compact_sidebar,
                worktree_setup: input.worktree_setup,
                worktree_shared: input.worktree_shared,
            })
            .map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::Repositories;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Two writers in one checkout is how an index lock is hit, or worse, how
    /// a stage and a discard interleave. One repository lets one through at a
    /// time; a different one does not wait.
    #[test]
    fn one_git_mutation_per_repository_at_a_time() {
        let repositories = Arc::new(Repositories::default());
        let inside = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));

        let same: Vec<_> = (0..8)
            .map(|_| {
                let repositories = Arc::clone(&repositories);
                let inside = Arc::clone(&inside);
                let most = Arc::clone(&most);
                std::thread::spawn(move || {
                    repositories.with(std::path::Path::new("/one"), || {
                        let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        inside.fetch_sub(1, Ordering::SeqCst);
                    })
                })
            })
            .collect();
        for thread in same {
            thread.join().unwrap();
        }
        assert_eq!(
            most.load(Ordering::SeqCst),
            1,
            "two mutations ran in one repository at once"
        );

        // A second repository is not held up by the first.
        let held = Arc::clone(&repositories);
        let blocker = std::thread::spawn(move || {
            held.with(std::path::Path::new("/one"), || {
                std::thread::sleep(std::time::Duration::from_millis(300));
            })
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let started = std::time::Instant::now();
        repositories.with(std::path::Path::new("/two"), || {});
        assert!(
            started.elapsed() < std::time::Duration::from_millis(200),
            "a different repository waited for the first"
        );
        blocker.join().unwrap();
    }
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
        app.manage(Workspace::at(storage.clone()));
        project::project_open(directory.to_string_lossy().into_owned(), app.state()).unwrap();
        project::project_open(directory.to_string_lossy().into_owned(), app.state()).unwrap();
        let project = app
            .state::<Workspace>()
            .with(|w| Ok(w.state().projects[0].id.clone()))
            .unwrap();
        let builder = session::session_create(
            project,
            "claude".into(),
            "Fix login".into(),
            "Handle expired tokens".into(),
            "sonnet".into(),
            app.state(),
        )
        .unwrap();
        convoy_core::history::History::new(storage.history())
            .save(&builder, "Tests pass")
            .unwrap();
        let brief = session::review_brief(builder.clone(), app.state()).unwrap();
        let review = session::review_create(builder.clone(), brief, app.state()).unwrap();
        let reloaded = CoreWorkspace::load(storage.workspace_file()).unwrap();
        assert_eq!(reloaded.state().projects.len(), 1);
        assert_eq!(
            reloaded.session(&builder).unwrap().model.as_deref(),
            Some("sonnet")
        );
        assert_eq!(
            reloaded.session(&review).unwrap().agent,
            convoy_core::model::Agent::Codex
        );
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
