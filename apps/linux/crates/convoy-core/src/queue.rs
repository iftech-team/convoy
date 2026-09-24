//! Task queue rules, ported from `runNext()` and `finishTask()` in main.cjs.
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
use crate::review;
use crate::workspace::{NewSession, Workspace};
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
    let Some(task) = state
        .tasks
        .iter()
        .find(|task| task.project_id == project_id && task.status == TaskStatus::Queued)
    else {
        return Step::Empty;
    };
    Step::Run {
        task_id: task.id.clone(),
        branch: None,
    }
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
    /// Not a task session, or the task is not awaiting review.
    Nothing,
    /// The specification moved on while the task ran: the work is against an
    /// old brief, so the task goes back to `changes` and the queue stops.
    Superseded,
    /// Hand the work to the other agent.
    Review(Box<NewSession>),
    /// Accepted into review; carry on with the queue.
    Continue,
}

/// Decides what a clean exit means. `output` is the terminal excerpt that
/// would go into a review brief; it is context, never instructions.
pub fn on_clean_exit(workspace: &Workspace, session_id: &str, output: &str) -> Result<Completion> {
    let state = workspace.state();
    let Some(task) = state
        .tasks
        .iter()
        .find(|task| task.session_id.as_deref() == Some(session_id))
    else {
        return Ok(Completion::Nothing);
    };
    if task.status != TaskStatus::Review {
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
    if !task.auto_review {
        return Ok(Completion::Continue);
    }
    // One review per builder: a second exit must not spawn another.
    let reviewed = state
        .sessions
        .iter()
        .any(|session| session.review_of.as_deref() == Some(session_id));
    if reviewed {
        return Ok(Completion::Continue);
    }
    let brief = review::brief(workspace, session_id, output)?;
    Ok(Completion::Review(Box::new(review::handoff(
        workspace,
        session_id,
        brief,
    )?)))
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
            };
            format!(
                "{}: {mode}{}",
                task.title,
                if task.auto_review {
                    ", automatic review"
                } else {
                    ""
                }
            )
        })
        .collect()
}
