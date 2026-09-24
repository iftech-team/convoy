//! Port of `planning.cjs`: specifications, tasks, account profiles and the
//! rules that connect them. Mounted on [`Workspace`] the same way the JavaScript
//! object was mixed into `Workspace.prototype`.

pub mod markdown;

use crate::json::utf16_len;
use crate::workspace::model::{
    Agent, Profile, PublishMode, Session, Spec, State, Task, TaskStatus,
};
use crate::workspace::{find_spec, find_task_index, new_id, provider_id_for, Workspace};
use crate::{bail, ensure, Result};
pub use markdown::markdown;

/// `text()` from planning.cjs, for values already known to be strings.
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
                        | TaskStatus::Changes
                        | TaskStatus::Done
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
            let prompt = format!(
                "{preamble}\n## Current task\n{}\n\n{}\n\nVerify the changes and report your results. {publication}",
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
                model: None,
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
