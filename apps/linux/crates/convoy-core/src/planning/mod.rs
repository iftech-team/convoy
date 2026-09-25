//! Specifications, tasks, account profiles and the rules that connect them,
//! as methods on [`Workspace`].

pub mod markdown;

use crate::integrations::{self, Connection, Issue, IssueSource, TrackerAuth};
use crate::json::utf16_len;
use crate::patterns::MODEL;
use crate::workspace::model::{
    Agent, Profile, PublishMode, Session, Spec, State, Task, TaskStatus,
};
use crate::workspace::{find_spec, find_task_index, new_id, provider_id_for, Workspace};
use crate::{bail, ensure, Result};
pub use markdown::markdown;
use std::collections::BTreeMap;

/// A string within `max` UTF-16 units, optionally non-blank.
fn text(value: &str, max: usize, required: bool) -> Result<()> {
    ensure!(
        utf16_len(value) <= max && !(required && value.trim().is_empty()),
        "Invalid or missing text."
    );
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct SpecInput {
    pub id: Option<String>,
    pub project_id: String,
    pub title: String,
    pub problem: String,
    pub requirements: String,
    pub acceptance: String,
    pub constraints: String,
    pub plan: String,
}

#[derive(Debug, Clone)]
pub struct TaskInput {
    pub id: Option<String>,
    pub project_id: String,
    pub spec_id: Option<String>,
    pub title: String,
    pub details: String,
    pub findings: String,
    pub agent: Agent,
    pub mode: PublishMode,
    pub auto_review: bool,
}

/// Options for [`Workspace::import_issues`]. `overrides` replaces the agent
/// and model for single issues, keyed by issue key.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub agent: Agent,
    pub model: String,
    pub mode: PublishMode,
    pub auto_review: bool,
    pub overrides: BTreeMap<String, (Agent, String)>,
}

/// The origin of an imported task, as the agent reads it in its brief.
fn source_note(source: &IssueSource) -> String {
    let link = source
        .url
        .as_deref()
        .map(|url| format!(" — {url}"))
        .unwrap_or_default();
    let mut note = format!(
        "\n\nSource issue: {} {}{link}",
        source.tracker.label(),
        source.key
    );
    if source.via_mcp == Some(true) {
        note.push_str(&format!(
            "\nFetch the full {} issue (description, acceptance criteria, comments) with your {} MCP tools before changing anything, and treat it as the task definition. If those tools are not available, say so and stop.",
            source.key,
            source.tracker.label()
        ));
    }
    note
}

fn model_name(model: &str) -> Result<Option<String>> {
    let model = model.trim();
    ensure!(
        utf16_len(model) <= 100 && MODEL.is_match(model),
        "Invalid model name."
    );
    Ok((!model.is_empty()).then(|| model.to_string()))
}

const PR_NOTE: &str = "Commit your changes, push the current branch and create a pull request with gh pr create --fill. Report its URL. Do not merge.";
const PUSH_NOTE: &str = "Commit and push the current branch to its configured upstream. Never force push. Report the commit.";
const NO_PUBLISH_NOTE: &str = "Do not publish, push, or open a PR.";

impl Workspace {
    pub fn save_spec(&mut self, input: SpecInput) -> Result<&State> {
        text(&input.title, 200, true)?;
        for value in [
            &input.problem,
            &input.requirements,
            &input.acceptance,
            &input.constraints,
            &input.plan,
        ] {
            text(value, 16_000, false)?;
        }
        self.update(move |state| {
            let existing = input
                .id
                .as_ref()
                .map(|id| state.specs.iter().position(|spec| spec.id == *id));
            match existing {
                Some(None) => bail!("Specification not found."),
                Some(Some(index)) => {
                    let spec = &mut state.specs[index];
                    let unchanged = spec.title == input.title
                        && spec.problem == input.problem
                        && spec.requirements == input.requirements
                        && spec.acceptance == input.acceptance
                        && spec.constraints == input.constraints
                        && spec.plan == input.plan;
                    if unchanged {
                        return Ok(());
                    }
                    spec.title = input.title;
                    spec.problem = input.problem;
                    spec.requirements = input.requirements;
                    spec.acceptance = input.acceptance;
                    spec.constraints = input.constraints;
                    spec.plan = input.plan;
                    spec.revision += 1;
                    spec.approved_revision = None;
                    let spec_id = spec.id.clone();
                    // An accepted task is no longer accepted once its
                    // specification moves on.
                    for task in state.tasks.iter_mut().filter(|task| {
                        task.spec_id.as_deref() == Some(spec_id.as_str())
                            && task.status == TaskStatus::Done
                    }) {
                        task.status = TaskStatus::Changes;
                    }
                }
                None => state.specs.push(Spec {
                    id: new_id(),
                    project_id: input.project_id,
                    title: input.title,
                    problem: input.problem,
                    requirements: input.requirements,
                    acceptance: input.acceptance,
                    constraints: input.constraints,
                    plan: input.plan,
                    revision: 1,
                    approved_revision: None,
                    unknown: Default::default(),
                }),
            }
            Ok(())
        })
    }

    pub fn approve_spec(&mut self, id: &str, revision: u64) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            let spec = state.specs.iter_mut().find(|spec| spec.id == id);
            match spec {
                Some(spec) if spec.revision == revision => {
                    spec.approved_revision = Some(revision);
                    Ok(())
                }
                _ => bail!("The specification changed. Review the latest revision."),
            }
        })
    }

    pub fn save_task(&mut self, input: TaskInput) -> Result<&State> {
        text(&input.title, 200, true)?;
        text(&input.details, 16_000, false)?;
        text(&input.findings, 16_000, false)?;
        self.update(move |state| {
            let existing = input.id.as_ref().map(|id| find_task_index(state, id));
            match existing {
                Some(None) => bail!("Task not found."),
                Some(Some(index)) => {
                    let task = &mut state.tasks[index];
                    ensure!(
                        task.status != TaskStatus::Building,
                        "Stop the task session before editing the task."
                    );
                    // `findings` is deliberately absent: recording review
                    // findings must not send the task back to the queue.
                    let changed = task.title != input.title
                        || task.details != input.details
                        || task.agent != input.agent
                        || task.mode != input.mode
                        || task.auto_review != input.auto_review;
                    task.title = input.title;
                    task.details = input.details;
                    task.findings = input.findings;
                    if task.agent != input.agent {
                        task.model = None;
                    }
                    task.agent = input.agent;
                    task.mode = input.mode;
                    task.auto_review = input.auto_review;
                    if changed {
                        task.status = TaskStatus::Queued;
                        task.session_id = None;
                        task.spec_revision = None;
                    }
                }
                None => state.tasks.push(Task {
                    id: new_id(),
                    project_id: input.project_id,
                    title: input.title,
                    details: input.details,
                    findings: input.findings,
                    agent: input.agent,
                    status: TaskStatus::Queued,
                    mode: input.mode,
                    auto_review: input.auto_review,
                    spec_id: input.spec_id.filter(|id| !id.is_empty()),
                    session_id: None,
                    spec_revision: None,
                    last_error: None,
                    model: None,
                    source: None,
                    pr_url: None,
                    depends_on: Vec::new(),
                    unknown: Default::default(),
                }),
            }
            Ok(())
        })
    }

    pub fn set_task_status(&mut self, id: &str, status: TaskStatus) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            ensure!(
                matches!(
                    status,
                    TaskStatus::Queued
                        | TaskStatus::Review
                        | TaskStatus::Pr
                        | TaskStatus::Changes
                        | TaskStatus::Done
                        | TaskStatus::Failed
                ),
                "Invalid task status."
            );
            let Some(index) = find_task_index(state, &id) else {
                bail!("Invalid task status.")
            };
            ensure!(
                state.tasks[index].status != TaskStatus::Building,
                "Stop the task session before changing its status."
            );
            if status == TaskStatus::Done {
                if let Some(spec_id) = state.tasks[index].spec_id.clone() {
                    let approved = find_spec(state, &spec_id).is_some_and(Spec::approved);
                    ensure!(
                        approved,
                        "Approve the current specification before accepting this task."
                    );
                }
            }
            state.tasks[index].status = status;
            Ok(())
        })
    }

    /// Sets the tasks this one waits for. Refused when one is of another
    /// project, or when they would wait for each other in a circle.
    pub fn set_task_dependencies(&mut self, id: &str, depends_on: Vec<String>) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            let Some(index) = find_task_index(state, &id) else {
                bail!("Task unavailable.")
            };
            let mut unique = Vec::new();
            for other in depends_on {
                if !unique.contains(&other) {
                    unique.push(other);
                }
            }
            state.tasks[index].depends_on = unique;
            Ok(())
        })
    }

    /// Makes an existing session the one that builds a task — the macOS
    /// app's "Link existing session". The session must be of the task's
    /// project and not already building another task.
    pub fn link_task_session(&mut self, id: &str, session_id: &str) -> Result<&State> {
        let (id, session_id) = (id.to_string(), session_id.to_string());
        self.update(move |state| {
            let Some(index) = find_task_index(state, &id) else {
                bail!("Task unavailable.")
            };
            ensure!(
                state.tasks[index].status != TaskStatus::Building,
                "Stop the task session before linking another."
            );
            let project = state.tasks[index].project_id.clone();
            let Some(session) = state
                .sessions
                .iter()
                .find(|session| session.id == session_id)
            else {
                bail!("Session not found.")
            };
            ensure!(
                session.project_id == project,
                "Link a session of the task's own project."
            );
            ensure!(
                session.task_id.as_deref().is_none_or(|other| other == id),
                "That session already builds another task."
            );
            let previous = state.tasks[index].session_id.replace(session_id.clone());
            state.tasks[index].spec_revision = None;
            for session in state.sessions.iter_mut() {
                if Some(&session.id) == previous.as_ref() {
                    session.task_id = None;
                }
                if session.id == session_id {
                    session.task_id = Some(id.clone());
                }
            }
            Ok(())
        })
    }

    /// Removes a task. One whose agent is running must be stopped first; its
    /// session stays, no longer linked to anything.
    pub fn delete_task(&mut self, id: &str) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            let Some(index) = find_task_index(state, &id) else {
                bail!("Task unavailable.")
            };
            ensure!(
                state.tasks[index].status != TaskStatus::Building,
                "Stop the task session before deleting the task."
            );
            state.tasks.remove(index);
            for task in state.tasks.iter_mut() {
                task.depends_on.retain(|other| *other != id);
            }
            for session in state.sessions.iter_mut() {
                if session.task_id.as_deref() == Some(id.as_str()) {
                    session.task_id = None;
                }
            }
            Ok(())
        })
    }

    /// Creates (or reuses) the session that will build a task. Returns the
    /// state; the caller looks the session up through `task.session_id`.
    pub fn prepare_task(&mut self, id: &str, profile_id: Option<String>) -> Result<&State> {
        let id = id.to_string();
        self.update(move |state| {
            let Some(index) = find_task_index(state, &id) else {
                bail!("Task unavailable.")
            };
            ensure!(
                state.tasks[index].status != TaskStatus::Building,
                "Task unavailable."
            );
            let task = state.tasks[index].clone();
            let spec = task
                .spec_id
                .as_deref()
                .and_then(|spec_id| find_spec(state, spec_id))
                .cloned();
            if let Some(spec) = &spec {
                ensure!(
                    spec.approved(),
                    "Approve the specification before preparing a task session."
                );
            }
            let revision = spec.as_ref().map(|spec| spec.revision);
            if task.session_id.is_some() && task.spec_revision == revision {
                let archived = task
                    .session_id
                    .as_deref()
                    .and_then(|session_id| {
                        state.sessions.iter().find(|session| session.id == session_id)
                    })
                    .is_some_and(Session::is_archived);
                if !archived {
                    return Ok(());
                }
            }
            let publication = match task.mode {
                PublishMode::Pr => PR_NOTE,
                PublishMode::Push => PUSH_NOTE,
                PublishMode::None => NO_PUBLISH_NOTE,
            };
            let preamble = spec
                .as_ref()
                .map(|spec| markdown(spec, &state.tasks))
                .unwrap_or_default();
            let origin = task.source.as_ref().map(source_note).unwrap_or_default();
            let prompt = format!(
                "{preamble}\n## Current task\n{}\n\n{}{origin}\n\nVerify the changes and report your results. {publication}",
                task.title, task.details
            );
            ensure!(
                utf16_len(&prompt) <= 32_000,
                "The specification and task exceed the session brief limit of 32,000 characters. Shorten them first."
            );
            let session = Session {
                id: new_id(),
                project_id: task.project_id.clone(),
                agent: task.agent,
                title: task.title.clone(),
                prompt,
                provider_id: provider_id_for(task.agent),
                started: false,
                model: task.model.clone(),
                notes: None,
                branch: None,
                archived: None,
                pinned: None,
                working_directory: None,
                agent_home: None,
                owns_worktree: None,
                worktree_removed: None,
                review_of: None,
                task_id: Some(task.id.clone()),
                profile_id: profile_id.filter(|id| !id.is_empty()),
                unknown: Default::default(),
            };
            let session_id = session.id.clone();
            state.sessions.push(session);
            state.tasks[index].session_id = Some(session_id);
            state.tasks[index].spec_revision = revision;
            Ok(())
        })
    }

    /// Creates a queued task for every issue not already imported into the
    /// project from the same tracker. Returns (created, skipped).
    pub fn import_issues(
        &mut self,
        project_id: &str,
        connection: &Connection,
        issues: &[Issue],
        options: &ImportOptions,
    ) -> Result<(usize, usize)> {
        ensure!(
            self.state()
                .projects
                .iter()
                .any(|project| project.id == project_id),
            "Project not found."
        );
        let default_model = model_name(&options.model)?;
        let mut picks = BTreeMap::new();
        for (key, (agent, model)) in &options.overrides {
            picks.insert(key.clone(), (*agent, model_name(model)?));
        }
        let mut created = Vec::new();
        {
            let mut seen: std::collections::HashSet<String> = self
                .state()
                .tasks
                .iter()
                .filter(|task| task.project_id == project_id)
                .filter_map(|task| task.source.as_ref())
                .filter(|source| source.tracker == connection.kind)
                .map(|source| format!("{}\n{}", source.origin.as_deref().unwrap_or(""), source.key))
                .collect();
            for issue in issues {
                let origin = issue
                    .origin
                    .clone()
                    .unwrap_or_else(|| integrations::origin(issue, connection));
                if issue.key.trim().is_empty() || !seen.insert(format!("{origin}\n{}", issue.key)) {
                    continue;
                }
                let title = if issue.title.is_empty() || issue.title == issue.key {
                    issue.key.clone()
                } else {
                    format!("{}: {}", issue.key, issue.title)
                };
                let title: String = title.chars().take(200).collect();
                let mut details = issue.details.clone();
                if utf16_len(&details) > 12_000 {
                    details = details.chars().take(12_000).collect::<String>()
                        + "\n…(truncated; see the issue)";
                }
                let (agent, model) = picks
                    .get(&issue.key)
                    .cloned()
                    .unwrap_or((options.agent, default_model.clone()));
                created.push(Task {
                    id: new_id(),
                    project_id: project_id.to_string(),
                    title,
                    details,
                    findings: String::new(),
                    agent,
                    status: TaskStatus::Queued,
                    mode: options.mode,
                    auto_review: options.auto_review,
                    spec_id: None,
                    session_id: None,
                    spec_revision: None,
                    last_error: None,
                    model,
                    source: Some(IssueSource {
                        tracker: connection.kind,
                        key: issue.key.clone(),
                        origin: Some(origin),
                        url: issue.url.clone(),
                        via_mcp: (connection.auth == TrackerAuth::Mcp).then_some(true),
                    }),
                    pr_url: None,
                    depends_on: Vec::new(),
                    unknown: Default::default(),
                });
            }
        }
        let count = created.len();
        if count > 0 {
            self.update(move |state| {
                state.tasks.extend(created);
                Ok(())
            })?;
        }
        Ok((count, issues.len() - count))
    }

    /// Changes who builds a task and with which model. A prepared session keeps
    /// the old choice, so it is detached and the next run prepares a new one.
    pub fn set_task_agent(&mut self, id: &str, agent: Agent, model: &str) -> Result<&State> {
        let model = model_name(model)?;
        let id = id.to_string();
        self.update(move |state| {
            let Some(index) = find_task_index(state, &id) else {
                bail!("Task not found.")
            };
            let task = &mut state.tasks[index];
            ensure!(
                task.status != TaskStatus::Building,
                "Stop the task session before editing the task."
            );
            if task.agent != agent || task.model != model {
                task.agent = agent;
                task.model = model;
                task.session_id = None;
            }
            Ok(())
        })
    }

    pub fn add_profile(&mut self, label: &str, agent: Agent) -> Result<&State> {
        text(label, 100, true)?;
        let label = label.trim().to_string();
        self.update(move |state| {
            let taken = state.profiles.iter().any(|profile| {
                profile.agent == agent && profile.label.to_lowercase() == label.to_lowercase()
            });
            ensure!(!taken, "That account label already exists.");
            state.profiles.push(Profile {
                id: new_id(),
                label,
                agent,
                unknown: Default::default(),
            });
            Ok(())
        })
    }
}
