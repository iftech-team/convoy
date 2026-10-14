//! Task queue rules: when the next task runs and how a finished one ends.
//!
//! Three properties hold throughout, and each one was a deliberate choice in
//! the original:
//!
//! * A clean exit moves a task to **review**, never to done. Whether the work
//!   is acceptable stays a human decision.
//! * A failure pauses the queue. Restarting is explicit; nothing resumes on
//!   its own after a crash or a stop.
//! * Publishing happens only for tasks configured for it, and a pull-request
//!   task gets its own worktree so it never pushes the shared checkout.

use crate::model::{Task, TaskStatus};
use crate::workspace::Workspace;
use crate::Result;

/// Whether a session currently has a process.
pub type Running<'a> = &'a dyn Fn(&str) -> bool;

/// What the queue should do next for a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// A task is already building; wait for it.
    Waiting,
    /// Nothing left to run — the queue stops.
    Empty,
    /// Prepare and start this task.
    Run {
        task_id: String,
        /// Set for a pull-request task with no worktree yet: it must get one
        /// before it runs, so publishing never touches the shared checkout.
        branch: Option<String>,
    },
}

/// The tasks this one waits for that are not done yet.
pub fn blocked_by<'a>(state: &'a crate::model::State, task: &Task) -> Vec<&'a Task> {
    task.depends_on
        .iter()
        .filter_map(|id| state.tasks.iter().find(|other| &other.id == id))
        .filter(|other| other.status != TaskStatus::Done)
        .collect()
}

/// The branch a queued pull-request task gets.
pub fn worktree_branch(task: &Task) -> String {
    format!("convoy/task-{}", &task.id[..task.id.len().min(8)])
}

pub fn next_step(workspace: &Workspace, project_id: &str, running: Running<'_>) -> Step {
    let state = workspace.state();
    let building = state.tasks.iter().any(|task| {
        task.status == TaskStatus::Building
            && task.session_id.as_deref().is_some_and(running)
            && task.project_id == project_id
    });
    if building {
        return Step::Waiting;
    }
    // A queued task waits while any task it depends on is not done.
    let Some(task) = state.tasks.iter().find(|task| {
        task.project_id == project_id
            && task.status == TaskStatus::Queued
            && blocked_by(state, task).is_empty()
    }) else {
        return Step::Empty;
    };
    Step::Run {
        task_id: task.id.clone(),
        branch: None,
    }
}

/// The tasks the queue starts now in a project, in order: queued, waiting
/// for nothing that is not done, as many as its free slots. A project runs
/// `maxParallel` at once only when each task has a worktree of its own;
/// tasks sharing one checkout run one at a time.
pub fn next_tasks(workspace: &Workspace, project_id: &str, running: Running<'_>) -> Vec<String> {
    let state = workspace.state();
    let Some(project) = state
        .projects
        .iter()
        .find(|project| project.id == project_id)
    else {
        return Vec::new();
    };
    let limit = if project.task_worktrees == Some(true) {
        project.max_parallel.unwrap_or(1).max(1) as usize
    } else {
        1
    };
    let busy = state
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
        .filter(|task| match task.status {
            TaskStatus::Building => task.session_id.as_deref().is_some_and(running),
            TaskStatus::Planning => true,
            _ => false,
        })
        .count();
    state
        .tasks
        .iter()
        .filter(|task| {
            task.project_id == project_id
                && task.status == TaskStatus::Queued
                && blocked_by(state, task).is_empty()
        })
        .take(limit.saturating_sub(busy))
        .map(|task| task.id.clone())
        .collect()
}

/// The branch a task's worktree gets: the prefix, then its issue key and
/// title as far as they spell in ASCII — `convoy/LOYAL-523-refresh-token`.
pub fn task_branch(workspace: &Workspace, task: &Task) -> String {
    let prefix = workspace
        .project(&task.project_id)
        .ok()
        .and_then(|project| project.branch_prefix.clone())
        .or_else(|| Some(workspace.settings().branch_prefix.clone()))
        .map(|prefix| prefix.trim().trim_matches('/').to_string())
        .filter(|prefix| !prefix.is_empty())
        .unwrap_or_else(|| "convoy".into());
    let key = task
        .source
        .as_ref()
        .map(|source| source.key.clone())
        .unwrap_or_default();
    let title = task
        .title
        .strip_prefix(&format!("{key}:"))
        .unwrap_or(&task.title);
    let mut slug = String::new();
    for word in title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        if slug.len() + word.len() > 40 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_ascii_lowercase());
    }
    let key: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let name = match (key.is_empty(), slug.is_empty()) {
        (false, false) => format!("{key}-{slug}"),
        (false, true) => key,
        (true, false) => slug,
        (true, true) => format!("task-{}", &task.id[..task.id.len().min(8)]),
    };
    format!("{prefix}/{name}")
}

/// The branch a task's builder gets a worktree on, when its project gives
/// each task one and the builder is new, with no folder of its own yet.
pub fn wants_worktree(workspace: &Workspace, task_id: &str) -> Option<String> {
    let state = workspace.state();
    let task = state.tasks.iter().find(|task| task.id == task_id)?;
    let project = state
        .projects
        .iter()
        .find(|project| project.id == task.project_id)?;
    if project.task_worktrees != Some(true) {
        return None;
    }
    let session = state
        .sessions
        .iter()
        .find(|session| Some(&session.id) == task.session_id.as_ref())?;
    (session.working_directory.is_none() && !session.started).then(|| task_branch(workspace, task))
}

/// Fills in the worktree branch once the task's session exists, which is only
/// after `prepare_task` has run.
pub fn needs_worktree(workspace: &Workspace, task_id: &str) -> Option<String> {
    let state = workspace.state();
    let task = state.tasks.iter().find(|task| task.id == task_id)?;
    if task.mode != crate::model::PublishMode::Pr {
        return None;
    }
    let session = state
        .sessions
        .iter()
        .find(|session| Some(&session.id) == task.session_id.as_ref())?;
    session
        .working_directory
        .is_none()
        .then(|| worktree_branch(task))
}

/// What should happen after a task session exits cleanly.
#[derive(Debug, Clone, PartialEq)]
pub enum Completion {
    /// Not a task session, or its round was already handled.
    Nothing,
    /// The specification moved on while the task ran: the work is against an
    /// old brief, so the task goes back to `changes` and the queue stops.
    Superseded,
    /// Its round is done; the pipeline moved it on. Carry on with the queue.
    Advance(Vec<crate::pipeline::Action>),
}

/// Decides what a clean exit means: the builder's round is done, as if it
/// had written its completion file. A round already handled by that file
/// is not handled again.
pub fn on_clean_exit(
    workspace: &mut Workspace,
    context: &crate::pipeline::Context<'_>,
    session_id: &str,
) -> Result<Completion> {
    let state = workspace.state();
    let Some(task) = state
        .tasks
        .iter()
        .find(|task| task.session_id.as_deref() == Some(session_id))
    else {
        return Ok(Completion::Nothing);
    };
    if !matches!(task.status, TaskStatus::Review | TaskStatus::Pr) || task.stage_run.is_none() {
        return Ok(Completion::Nothing);
    }
    let spec = task
        .spec_id
        .as_deref()
        .and_then(|spec_id| state.specs.iter().find(|spec| spec.id == spec_id));
    if let Some(spec) = spec {
        if !spec.approved() || task.spec_revision != Some(spec.revision) {
            return Ok(Completion::Superseded);
        }
    }
    let task_id = task.id.clone();
    // Whatever the builder wrote on its way out is read first.
    let report = task.stage_run.as_deref().and_then(|run| {
        match crate::stage::read(
            crate::stage::Stage::Build,
            &crate::stage::file(workspace, crate::stage::Stage::Build, run),
        ) {
            Some(crate::stage::StageResult::Build { summary, verified }) => {
                Some((summary, verified))
            }
            _ => None,
        }
    });
    let actions = crate::pipeline::build_done(workspace, context, &task_id, report)?;
    Ok(Completion::Advance(actions))
}

/// Marks a task as superseded and reports that the queue must stop.
pub fn mark_superseded(workspace: &mut Workspace, session_id: &str) -> Result<()> {
    let session_id = session_id.to_string();
    workspace
        .update(move |state| {
            if let Some(task) = state
                .tasks
                .iter_mut()
                .find(|task| task.session_id.as_deref() == Some(session_id.as_str()))
            {
                task.status = TaskStatus::Changes;
            }
            Ok(())
        })
        .map(|_| ())
}

/// The summary shown before a queue is started. Running agents costs money and
/// may publish, so the user sees exactly what will happen first.
pub fn summary(workspace: &Workspace, project_id: &str) -> Vec<String> {
    workspace
        .state()
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id && task.status == TaskStatus::Queued)
        .map(|task| {
            let mode = match task.mode {
                crate::model::PublishMode::None => "no publishing",
                crate::model::PublishMode::Pr => "pull request",
                crate::model::PublishMode::Push => "push",
                crate::model::PublishMode::Describe => "pull request description",
            };
            let waits = blocked_by(workspace.state(), task);
            format!(
                "{}: {mode}{}{}",
                task.title,
                if task.auto_review {
                    ", automatic review"
                } else {
                    ""
                },
                if waits.is_empty() {
                    String::new()
                } else {
                    format!(
                        ", waits for {}",
                        waits
                            .iter()
                            .map(|other| other.title.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
            )
        })
        .collect()
}
