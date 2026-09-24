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
use std::os::unix::fs::OpenOptionsExt;
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
}

impl ProjectPatch {
    fn values(&self) -> [Option<&String>; 8] {
        [
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
            let mut handle = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temporary)?;
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
        ensure!(
            fs::metadata(directory)?.is_dir(),
            "Choose a folder."
        );
        let resolved = fs::canonicalize(directory)?;
        self.update(move |state| {
            if !state.projects.iter().any(|project| project.path == resolved) {
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
                    color: None,
                    icon: None,
                    setup_command: None,
                    shared_paths: None,
                    review_template: None,
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
            Ok(())
        })
    }

    pub fn remove_project(&mut self, id: &str) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
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
            unknown: current.unknown,
        };
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
