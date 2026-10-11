//! Port of `workspace.cjs`: load, validate, mutate and persist `workspace.json`.
//!
//! Every mutation goes through [`Workspace::update`], which clones the current
//! state, applies the change, validates the result and only then replaces the
//! file and the in-memory copy. A rejected change therefore leaves both disk
//! and memory untouched — an invariant the ported tests check directly.

pub mod migrate;
pub mod model;
pub mod validate;

use crate::json::{head, utf16_len};
use crate::patterns::MODEL;
use crate::time::now_iso8601;
use crate::{bail, ensure, ConvoyError, Result};
use model::{
    ActivityEvent, ActivityKind, Agent, KeepAwake, Project, QuickCommand, Session, Settings, Spec,
    State, Theme,
};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use validate::{validate, validate_settings, DAMAGED};

pub const RECOVERED_TASK_ERROR: &str =
    "App closed while the session was running. Resume its saved session explicitly.";

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// Fields `editProject` is allowed to touch.
#[derive(Debug, Clone, Default)]
pub struct ProjectPatch {
    pub title: Option<String>,
    pub group: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub path: Option<String>,
    pub setup_command: Option<String>,
    pub shared_paths: Option<String>,
    pub review_template: Option<String>,
    /// `Some("")` clears it back to the global default.
    pub default_agent: Option<String>,
    pub base_ref: Option<String>,
    pub branch_prefix: Option<String>,
    pub task_mode: Option<String>,
    pub auto_run_tasks: Option<bool>,
}

impl ProjectPatch {
    fn values(&self) -> [Option<&String>; 12] {
        [
            self.default_agent.as_ref(),
            self.base_ref.as_ref(),
            self.branch_prefix.as_ref(),
            self.task_mode.as_ref(),
            self.title.as_ref(),
            self.group.as_ref(),
            self.color.as_ref(),
            self.icon.as_ref(),
            self.path.as_ref(),
            self.setup_command.as_ref(),
            self.shared_paths.as_ref(),
            self.review_template.as_ref(),
        ]
    }
}

/// Fields `editSession` is allowed to touch.
#[derive(Debug, Clone, Default)]
pub struct SessionPatch {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub provider_id: Option<String>,
    pub archived: Option<bool>,
    pub pinned: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    pub project_id: String,
    pub agent: Agent,
    pub title: String,
    pub prompt: String,
    pub review_of: Option<String>,
    pub profile_id: Option<String>,
    pub model: String,
    /// A level [`crate::provider::valid_effort`] accepts for the agent, or
    /// empty for its default.
    pub effort: String,
}

impl NewSession {
    pub fn new(project_id: impl Into<String>, agent: Agent, title: impl Into<String>) -> Self {
        NewSession {
            project_id: project_id.into(),
            agent,
            title: title.into(),
            prompt: String::new(),
            review_of: None,
            profile_id: None,
            model: String::new(),
            effort: String::new(),
        }
    }
}

/// Partial settings update; absent fields keep their current value, matching
/// `settings[key] ?? this.state.settings[key] ?? defaults[key]`.
#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    pub font_size: Option<i64>,
    pub scrollback: Option<i64>,
    pub theme: Option<Theme>,
    pub default_agent: Option<Agent>,
    pub notifications: Option<bool>,
    pub keep_awake: Option<KeepAwake>,
    pub hibernate_minutes: Option<i64>,
    pub shortcuts: Option<std::collections::BTreeMap<String, String>>,
    pub claude_usage: Option<bool>,
    pub worktree_by_default: Option<bool>,
    pub branch_prefix: Option<String>,
    pub sort_projects: Option<bool>,
    pub yolo_claude: Option<bool>,
    pub yolo_codex: Option<bool>,
    pub review_template: Option<String>,
    pub notify_waiting: Option<bool>,
    pub notify_done: Option<bool>,
    pub notify_when_focused: Option<bool>,
    pub notification_sound: Option<String>,
    pub auto_trust: Option<bool>,
    pub status_hooks: Option<bool>,
    pub git_status: Option<bool>,
    pub git_poll_seconds: Option<i64>,
    pub compact_sidebar: Option<bool>,
    pub worktree_setup: Option<String>,
    pub worktree_shared: Option<String>,
    /// `chat` or `terminal`.
    pub default_view: Option<String>,
}

pub struct Workspace {
    file: PathBuf,
    state: State,
}

impl Workspace {
    pub fn load(file: impl Into<PathBuf>) -> Result<Workspace> {
        let file = file.into();
        let mut state = if file.exists() {
            let text = fs::read_to_string(&file)?;
            let value: Value = serde_json::from_str(&text)?;
            validate(&value)?;
            // Validation accepted the document; anything the typed model still
            // rejects is a shape no Convoy build has ever written.
            serde_json::from_value::<State>(value).map_err(|_| ConvoyError::message(DAMAGED))?
        } else {
            State::default()
        };
        migrate::migrate(&mut state);
        Ok(Workspace { file, state })
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn settings(&self) -> &Settings {
        &self.state.settings
    }

    pub fn update<F>(&mut self, change: F) -> Result<&State>
    where
        F: FnOnce(&mut State) -> Result<()>,
    {
        let mut next = self.state.clone();
        change(&mut next)?;
        let value = serde_json::to_value(&next)?;
        validate(&value)?;
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut temporary = self.file.clone().into_os_string();
        temporary.push(format!(".{}.tmp", Uuid::new_v4()));
        let temporary = PathBuf::from(temporary);
        let outcome = (|| -> Result<()> {
            let body = serde_json::to_string_pretty(&value)?;
            let mut handle = crate::fs::create_private(&temporary)?;
            std::io::Write::write_all(&mut handle, body.as_bytes())?;
            handle.sync_all()?;
            drop(handle);
            fs::rename(&temporary, &self.file)?;
            Ok(())
        })();
        let _ = fs::remove_file(&temporary);
        outcome?;
        self.state = next;
        Ok(&self.state)
    }

    // ---- projects ------------------------------------------------------

    pub fn project(&self, id: &str) -> Result<&Project> {
        self.state
            .projects
            .iter()
            .find(|project| project.id == id)
            .ok_or_else(|| ConvoyError::message("Project not found."))
    }

    pub fn add_project(&mut self, directory: &Path) -> Result<&State> {
        ensure!(fs::metadata(directory)?.is_dir(), "Choose a folder.");
        let resolved = fs::canonicalize(directory)?;
        self.update(move |state| {
            if !state
                .projects
                .iter()
                .any(|project| project.path == resolved)
            {
                let title = resolved
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| resolved.to_string_lossy().into_owned());
                state.projects.push(Project {
                    id: new_id(),
                    title,
                    path: resolved,
                    group: None,
                    parent_id: None,
                    is_group: None,
                    color: None,
                    icon: None,
                    setup_command: None,
                    shared_paths: None,
                    review_template: None,
                    default_agent: None,
                    base_ref: None,
                    branch_prefix: None,
                    task_mode: None,
                    auto_run_tasks: None,
                    unknown: Default::default(),
                });
            }
            Ok(())
        })
    }

    pub fn edit_project(&mut self, id: &str, patch: ProjectPatch) -> Result<&State> {
        ensure!(
            patch
                .values()
                .iter()
                .flatten()
                .all(|value| utf16_len(value) <= 4096),
            "Invalid project settings."
        );
        if let Some(title) = &patch.title {
            ensure!(!title.trim().is_empty(), "Enter a project name.");
        }
        if let Some(color) = patch.color.as_deref().filter(|value| !value.is_empty()) {
            ensure!(
                color.len() == 7
                    && color.starts_with('#')
                    && color[1..].chars().all(|c| c.is_ascii_hexdigit()),
                "Enter a colour as #RRGGBB."
            );
        }
        let default_agent = match patch.default_agent.as_deref() {
            None => None,
            Some("") => Some(None),
            Some(value) => Some(Some(
                Agent::parse(value).ok_or_else(|| ConvoyError::message("Unknown agent."))?,
            )),
        };
        if let Some(prefix) = patch.branch_prefix.as_deref() {
            ensure!(
                crate::patterns::BRANCH_PREFIX.is_match(prefix.trim().trim_matches('/')),
                "Invalid branch prefix. Use letters, digits, dots, dashes and slashes."
            );
        }
        if let Some(base) = patch
            .base_ref
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            ensure!(
                !base.starts_with('-')
                    && base.len() <= 150
                    && base
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._/-~^@{}".contains(c)),
                "Invalid base ref."
            );
        }
        if let Some(mode) = patch.task_mode.as_deref() {
            ensure!(
                ["pr", "push", "none", ""].contains(&mode),
                "Unknown task mode."
            );
        }
        if let Some(path) = patch.path.as_deref().filter(|value| !value.is_empty()) {
            let path = Path::new(path);
            ensure!(
                path.is_absolute() && fs::metadata(path).is_ok_and(|meta| meta.is_dir()),
                "Invalid folder."
            );
        }
        let id = id.to_string();
        self.update(move |state| {
            let Some(project) = state.projects.iter_mut().find(|project| project.id == id) else {
                bail!("Project not found.")
            };
            if let Some(value) = patch.title {
                project.title = value;
            }
            if let Some(value) = patch.group {
                project.group = Some(value);
            }
            if let Some(value) = patch.color {
                project.color = Some(value);
            }
            if let Some(value) = patch.icon {
                project.icon = Some(value);
            }
            if let Some(value) = patch.path {
                project.path = PathBuf::from(value);
            }
            if let Some(value) = patch.setup_command {
                project.setup_command = Some(value);
            }
            if let Some(value) = patch.shared_paths {
                project.shared_paths = Some(value);
            }
            if let Some(value) = patch.review_template {
                project.review_template = Some(value);
            }
            if let Some(value) = default_agent {
                project.default_agent = value;
            }
            // An empty value falls back to the global setting.
            let optional = |value: String| {
                let value = value.trim().trim_matches('/').to_string();
                (!value.is_empty()).then_some(value)
            };
            if let Some(value) = patch.base_ref {
                project.base_ref = optional(value);
            }
            if let Some(value) = patch.branch_prefix {
                project.branch_prefix = optional(value);
            }
            if let Some(value) = patch.task_mode {
                project.task_mode = optional(value);
            }
            if let Some(value) = patch.auto_run_tasks {
                project.auto_run_tasks = Some(value);
            }
            Ok(())
        })
    }

    /// Moves a project to `index` in the list, which is the sidebar's order
    /// unless projects are sorted by name.
    pub fn move_project(&mut self, id: &str, index: usize) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            let Some(from) = state.projects.iter().position(|project| project.id == id) else {
                bail!("Project not found.")
            };
            let project = state.projects.remove(from);
            let to = index.min(state.projects.len());
            state.projects.insert(to, project);
            Ok(())
        })
    }

    /// Forgets every project at once, in one save: their sessions, specs,
    /// tasks and activity, and the quick commands that belonged to one.
    /// Global quick commands and settings stay. Nothing on disk is touched.
    pub fn remove_all_projects(&mut self) -> Result<&State> {
        self.update(|state| {
            state.projects.clear();
            state.sessions.clear();
            state.specs.clear();
            state.tasks.clear();
            state
                .quick_commands
                .retain(|command| command.project_id.is_none());
            state.activity.clear();
            Ok(())
        })
    }

    /// Adds what [`crate::files::discover_tree`] found, as the macOS app does:
    /// a folder already here keeps its id, sessions and specs, and takes its
    /// place in the tree; opening a child on its own never detaches it from
    /// its parent. Gives how many projects are new.
    pub fn import_tree(&mut self, found: &[crate::files::Found]) -> Result<usize> {
        let resolved: Vec<PathBuf> = found
            .iter()
            .map(|item| fs::canonicalize(&item.path).unwrap_or_else(|_| item.path.clone()))
            .collect();
        let found = found.to_vec();
        let mut added = 0;
        self.update(|state| {
            let mut ids: Vec<String> = Vec::with_capacity(found.len());
            for (item, path) in found.iter().zip(&resolved) {
                let parent = item.parent.and_then(|index| ids.get(index).cloned());
                if let Some(project) = state
                    .projects
                    .iter_mut()
                    .find(|project| &project.path == path)
                {
                    if parent.is_some() && parent.as_deref() != Some(project.id.as_str()) {
                        project.parent_id = parent;
                    }
                    if item.group {
                        project.is_group = Some(true);
                    }
                    ids.push(project.id.clone());
                    continue;
                }
                let title = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                let id = new_id();
                state.projects.push(Project {
                    id: id.clone(),
                    title,
                    path: path.clone(),
                    group: None,
                    parent_id: parent,
                    is_group: item.group.then_some(true),
                    color: None,
                    icon: None,
                    setup_command: None,
                    shared_paths: None,
                    review_template: None,
                    default_agent: None,
                    base_ref: None,
                    branch_prefix: None,
                    task_mode: None,
                    auto_run_tasks: None,
                    unknown: Default::default(),
                });
                ids.push(id);
                added += 1;
            }
            Ok(())
        })?;
        Ok(added)
    }

    /// Forgets a project. As on macOS, removing a group keeps its children:
    /// they move up to the group's own parent.
    pub fn remove_project(&mut self, id: &str) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            let parent = state
                .projects
                .iter()
                .find(|project| project.id == id)
                .and_then(|project| project.parent_id.clone());
            for project in state.projects.iter_mut() {
                if project.parent_id.as_deref() == Some(id.as_str()) {
                    project.parent_id = parent.clone();
                }
            }
            let removed: Vec<String> = state
                .sessions
                .iter()
                .filter(|session| session.project_id == id)
                .map(|session| session.id.clone())
                .collect();
            state.projects.retain(|project| project.id != id);
            state.sessions.retain(|session| session.project_id != id);
            state.specs.retain(|spec| spec.project_id != id);
            state.tasks.retain(|task| task.project_id != id);
            state
                .quick_commands
                .retain(|command| command.project_id.as_deref() != Some(id.as_str()));
            state
                .activity
                .retain(|event| !removed.contains(&event.session_id));
            Ok(())
        })
    }

    // ---- sessions ------------------------------------------------------

    pub fn session(&self, id: &str) -> Result<&Session> {
        find_session(&self.state, id)
    }

    pub fn add_session(&mut self, input: NewSession) -> Result<&State> {
        ensure!(
            !input.title.trim().is_empty()
                && utf16_len(&input.title) <= 200
                && utf16_len(&input.prompt) <= 32_000,
            "Invalid session details."
        );
        ensure!(
            utf16_len(&input.model) <= 100 && MODEL.is_match(&input.model),
            "Invalid model name."
        );
        ensure!(
            input.effort.is_empty() || crate::provider::valid_effort(input.agent, &input.effort),
            "Invalid effort."
        );
        self.update(move |state| {
            let source = match &input.review_of {
                Some(id) => Some(find_session(state, id)?.clone()),
                None => None,
            };
            if let Some(source) = &source {
                ensure!(
                    source.project_id == input.project_id,
                    "Reviews must use the builder project."
                );
            }
            state.sessions.push(Session {
                id: new_id(),
                project_id: input.project_id,
                agent: input.agent,
                model: Some(input.model),
                effort: Some(input.effort).filter(|effort| !effort.is_empty()),
                title: input.title.trim().to_string(),
                prompt: input.prompt,
                provider_id: provider_id_for(input.agent),
                started: false,
                notes: Some(String::new()),
                branch: source.as_ref().and_then(|source| source.branch.clone()),
                archived: Some(false),
                pinned: Some(false),
                working_directory: source
                    .as_ref()
                    .and_then(|source| source.working_directory.clone()),
                agent_home: None,
                owns_worktree: None,
                worktree_removed: None,
                review_of: source.as_ref().map(|_| input.review_of.clone().unwrap()),
                task_id: None,
                profile_id: input.profile_id,
                view: None,
                unknown: Default::default(),
            });
            Ok(())
        })
    }

    pub fn edit_session(&mut self, id: &str, patch: SessionPatch, running: bool) -> Result<&State> {
        if let Some(title) = &patch.title {
            ensure!(
                !title.trim().is_empty() && utf16_len(title) <= 200,
                "Enter a session name."
            );
        }
        if let Some(notes) = &patch.notes {
            ensure!(utf16_len(notes) <= 32_000, "Notes are too long.");
        }
        ensure!(
            !(running && (patch.archived == Some(true) || patch.provider_id.is_some())),
            "Stop the session before archiving or changing its provider ID."
        );
        self.session(id)?;
        let id = id.to_string();
        self.update(move |state| {
            let session = find_session_mut(state, &id)?;
            if let Some(value) = patch.title {
                session.title = value;
            }
            if let Some(value) = patch.notes {
                session.notes = Some(value);
            }
            if let Some(value) = patch.provider_id {
                session.provider_id = value;
            }
            if let Some(value) = patch.archived {
                session.archived = Some(value);
            }
            if let Some(value) = patch.pinned {
                session.pinned = Some(value);
            }
            Ok(())
        })
    }

    /// Shows the session as `chat` or `terminal` from now on. Either view
    /// drives the same conversation, so this is allowed while it runs; the
    /// window restarts the agent in the other mode itself.
    pub fn set_session_view(&mut self, id: &str, view: &str) -> Result<&State> {
        ensure!(model::VIEWS.contains(&view), "Unknown session view.");
        self.session(id)?;
        let (id, view) = (id.to_string(), view.to_string());
        self.update(move |state| {
            find_session_mut(state, &id)?.view = Some(view);
            Ok(())
        })
    }

    /// The model the session asks for from its next turn or launch; none is
    /// the agent's default. Allowed while it runs: a chat switches at once.
    pub fn set_session_model(&mut self, id: &str, model: Option<&str>) -> Result<&State> {
        let model = model.filter(|model| !model.is_empty());
        if let Some(model) = model {
            ensure!(
                utf16_len(model) <= 100 && MODEL.is_match(model),
                "Invalid model name."
            );
        }
        self.session(id)?;
        let (id, model) = (id.to_string(), model.map(str::to_string));
        self.update(move |state| {
            find_session_mut(state, &id)?.model = model;
            Ok(())
        })
    }

    /// How hard the session's agent thinks, as [`Workspace::set_session_model`]
    /// sets its model. Checked against what that agent takes.
    pub fn set_session_effort(&mut self, id: &str, effort: Option<&str>) -> Result<&State> {
        let effort = effort.filter(|effort| !effort.is_empty());
        let agent = self.session(id)?.agent;
        if let Some(effort) = effort {
            ensure!(
                crate::provider::valid_effort(agent, effort),
                "Invalid effort."
            );
        }
        let (id, effort) = (id.to_string(), effort.map(str::to_string));
        self.update(move |state| {
            find_session_mut(state, &id)?.effort = effort;
            Ok(())
        })
    }

    pub fn recover_session(&mut self, id: &str) -> Result<&State> {
        let old = self.session(id)?.clone();
        ensure!(
            !old.worktree_removed(),
            "Restore the worktree before creating a recovery session."
        );
        self.update(move |state| {
            let mut fresh = old.clone();
            fresh.id = new_id();
            fresh.title = format!("{} · recovery", head(&old.title, 180));
            fresh.started = false;
            fresh.provider_id = provider_id_for(old.agent);
            fresh.archived = Some(false);
            fresh.owns_worktree = Some(false);
            fresh.prompt = String::new();
            fresh.task_id = None;
            state.sessions.push(fresh);
            Ok(())
        })
    }

    /// Forgets sessions. A task they were building goes back to having no
    /// session, and their activity goes with them. Their saved conversations
    /// are the caller's to remove.
    pub fn remove_sessions(&mut self, ids: &[String]) -> Result<&State> {
        let ids = ids.to_vec();
        self.update(move |state| {
            state.sessions.retain(|session| !ids.contains(&session.id));
            for task in state.tasks.iter_mut() {
                if task.session_id.as_ref().is_some_and(|id| ids.contains(id)) {
                    task.session_id = None;
                }
            }
            state
                .activity
                .retain(|event| !ids.contains(&event.session_id));
            Ok(())
        })
    }

    // ---- settings and quick commands -----------------------------------

    pub fn save_settings(&mut self, patch: SettingsPatch) -> Result<&State> {
        let current = self.state.settings.clone();
        let next = Settings {
            font_size: patch.font_size.unwrap_or(current.font_size),
            scrollback: patch.scrollback.unwrap_or(current.scrollback),
            theme: patch.theme.unwrap_or(current.theme),
            default_agent: patch.default_agent.unwrap_or(current.default_agent),
            notifications: patch.notifications.unwrap_or(current.notifications),
            keep_awake: patch.keep_awake.unwrap_or(current.keep_awake),
            hibernate_minutes: patch.hibernate_minutes.unwrap_or(current.hibernate_minutes),
            shortcuts: patch.shortcuts.unwrap_or(current.shortcuts),
            claude_usage: patch.claude_usage.unwrap_or(current.claude_usage),
            worktree_by_default: patch
                .worktree_by_default
                .unwrap_or(current.worktree_by_default),
            branch_prefix: patch
                .branch_prefix
                .map(|value| value.trim().trim_matches('/').to_string())
                .unwrap_or(current.branch_prefix),
            sort_projects: patch.sort_projects.unwrap_or(current.sort_projects),
            yolo_claude: patch.yolo_claude.unwrap_or(current.yolo_claude),
            yolo_codex: patch.yolo_codex.unwrap_or(current.yolo_codex),
            review_template: patch.review_template.unwrap_or(current.review_template),
            notify_waiting: patch.notify_waiting.unwrap_or(current.notify_waiting),
            notify_done: patch.notify_done.unwrap_or(current.notify_done),
            notify_when_focused: patch
                .notify_when_focused
                .unwrap_or(current.notify_when_focused),
            notification_sound: patch
                .notification_sound
                .unwrap_or(current.notification_sound),
            auto_trust: patch.auto_trust.unwrap_or(current.auto_trust),
            status_hooks: patch.status_hooks.unwrap_or(current.status_hooks),
            git_status: patch.git_status.unwrap_or(current.git_status),
            git_poll_seconds: patch.git_poll_seconds.unwrap_or(current.git_poll_seconds),
            compact_sidebar: patch.compact_sidebar.unwrap_or(current.compact_sidebar),
            worktree_setup: patch.worktree_setup.unwrap_or(current.worktree_setup),
            worktree_shared: patch.worktree_shared.unwrap_or(current.worktree_shared),
            default_view: patch.default_view.unwrap_or(current.default_view),
            unknown: current.unknown,
        };
        ensure!(
            model::VIEWS.contains(&next.default_view.as_str()),
            "Invalid settings."
        );
        validate_settings(&serde_json::to_value(&next)?)?;
        self.update(move |state| {
            state.settings = next;
            Ok(())
        })
    }

    pub fn save_command(&mut self, command: QuickCommand) -> Result<&State> {
        self.update(move |state| {
            let mut value = command;
            if value.id.is_empty() {
                value.id = new_id();
            }
            value.project_id = value.project_id.filter(|id| !id.is_empty());
            match state
                .quick_commands
                .iter_mut()
                .find(|existing| existing.id == value.id)
            {
                Some(existing) => *existing = value,
                None => state.quick_commands.push(value),
            }
            Ok(())
        })
    }

    pub fn remove_command(&mut self, id: &str) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            state.quick_commands.retain(|command| command.id != id);
            Ok(())
        })
    }

    // ---- activity ------------------------------------------------------

    /// Empties the activity feed, as its Clear button does.
    pub fn clear_activity(&mut self) -> Result<&State> {
        self.update(|state| {
            state.activity.clear();
            Ok(())
        })
    }

    pub fn record(&mut self, kind: ActivityKind, session_id: &str, detail: &str) -> Result<&State> {
        let session = self.session(session_id)?;
        let event = ActivityEvent {
            id: new_id(),
            at: now_iso8601(),
            kind,
            session_id: session.id.clone(),
            title: session.title.clone(),
            detail: detail.to_string(),
            unknown: Default::default(),
        };
        self.update(move |state| {
            state.activity.insert(0, event);
            state.activity.truncate(200);
            Ok(())
        })
    }
}

pub(crate) fn provider_id_for(agent: Agent) -> String {
    match agent {
        Agent::Claude => new_id(),
        Agent::Codex => String::new(),
    }
}

pub(crate) fn find_session<'a>(state: &'a State, id: &str) -> Result<&'a Session> {
    state
        .sessions
        .iter()
        .find(|session| session.id == id)
        .ok_or_else(|| ConvoyError::message("Session not found."))
}

pub(crate) fn find_session_mut<'a>(state: &'a mut State, id: &str) -> Result<&'a mut Session> {
    state
        .sessions
        .iter_mut()
        .find(|session| session.id == id)
        .ok_or_else(|| ConvoyError::message("Session not found."))
}

pub(crate) fn find_task_index(state: &State, id: &str) -> Option<usize> {
    state.tasks.iter().position(|task| task.id == id)
}

pub(crate) fn find_spec<'a>(state: &'a State, id: &str) -> Option<&'a Spec> {
    state.specs.iter().find(|spec| spec.id == id)
}

/// Re-exported so callers do not need to reach into the submodule.
pub use model::{Agent as AgentKind, TaskStatus as Status};
