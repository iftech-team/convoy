//! "Plan first": a task whose plan is written and approved before anything
//! is built.
//!
//! A planner — the task's own agent — reads the code and writes the plan to
//! a file Convoy names ([`crate::stage`]), changing nothing in the
//! repository. Claude's native plan mode is deliberately not used: in it the
//! agent cannot write that file, and the plan would have to be dug out of
//! the conversation.
//!
//! A person approves the plan, or asks for changes, which go back to the
//! same planner; a project or task may approve plans by itself. The approved
//! plan goes into the builder's brief, and into the reviewer's, QA's and the
//! documentation writer's, so the work is checked against it.

use crate::model::{Role, Task, TaskStatus};
use crate::pipeline::{self, Action, Context};
use crate::stage::{self, Stage};
use crate::workspace::{new_id, NewSession, Workspace};
use crate::{bail, ensure, json::head, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStatus {
    /// The planner is writing it.
    Drafting,
    /// Written; waits for a person.
    Waiting,
    Approved,
    /// The task changed after approval, so the plan has to be redone.
    ChangesRequested,
}

/// A comment a person left on a draft.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanComment {
    pub at: String,
    pub text: String,
}

/// A task's plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRun {
    /// The current draft's run: its file is named after it.
    pub id: String,
    pub status: PlanStatus,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<PlanComment>,
    /// The planner's session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved_at: Option<String>,
    /// When the current draft was asked for: a plan Claude presented
    /// before it belongs to an earlier draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_at: Option<String>,
}

/// How a Claude planner ends: in plan mode it cannot write files, so it
/// presents the plan, which Convoy reads from its conversation.
const PRESENT: &str = "## When you are done\nThis session is in plan mode: it can read but not change anything. When the plan is ready, present the whole plan with ExitPlanMode. Convoy takes it from there, and a person approves it or asks for changes there; do not wait for an answer here.";

/// Whether the planner is Claude, which plans in its own plan mode.
fn native(agent: crate::model::Agent) -> bool {
    agent == crate::model::Agent::Claude
}

/// The latest plan a Claude planner presented since `since`, read from its
/// conversation: the `plan` of its last `ExitPlanMode` call.
pub fn presented(workspace: &Workspace, planner: &str, since: &str) -> Option<String> {
    let storage = stage::storage(workspace);
    let source = crate::chat::history::source(workspace, &storage, planner).ok()??;
    if !native(source.agent) {
        return None;
    }
    let file =
        crate::chat::history::claude_file(&source.home, &source.directory, &source.provider_id)?;
    let text = std::fs::read_to_string(file).ok()?;
    let mut latest = None;
    for line in text
        .lines()
        .filter(|line| line.contains("\"ExitPlanMode\""))
    {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record["timestamp"].as_str().is_some_and(|at| at < since) {
            continue;
        }
        for part in record["message"]["content"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if part["type"] == "tool_use" && part["name"] == "ExitPlanMode" {
                if let Some(plan) = part["input"]["plan"]
                    .as_str()
                    .filter(|plan| !plan.trim().is_empty())
                {
                    latest = Some(plan.trim().to_string());
                }
            }
        }
    }
    latest
}

impl PlanRun {
    pub fn approved(&self) -> bool {
        self.status == PlanStatus::Approved
    }
}

/// Whether running the task means planning it first.
pub fn needed(task: &Task) -> bool {
    task.plan_first && !task.plan.as_ref().is_some_and(PlanRun::approved)
}

/// Whether this task's plans are approved without a person.
pub fn auto_approve(workspace: &Workspace, task: &Task) -> bool {
    task.plan_auto_approve
        .or_else(|| {
            workspace
                .project(&task.project_id)
                .ok()
                .and_then(|project| project.plan_auto_approve)
        })
        .unwrap_or(false)
}

fn find(workspace: &Workspace, task_id: &str) -> Result<Task> {
    match workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == task_id)
    {
        Some(task) => Ok(task.clone()),
        None => bail!("Task not found."),
    }
}

/// The planner's brief: the task as the builder would get it, its role's
/// instructions, and the rule to change nothing.
fn brief(workspace: &Workspace, task: &Task, file: &std::path::Path) -> Result<String> {
    let closing = if native(task.agent) {
        PRESENT.to_string()
    } else {
        stage::closing(Stage::Plan, file)
    };
    let state = workspace.state();
    let project = workspace.project(&task.project_id)?;
    let instructions = crate::roles::brief(workspace.settings(), project, Role::Plan);
    let spec = task
        .spec_id
        .as_deref()
        .and_then(|id| state.specs.iter().find(|spec| spec.id == id))
        .map(|spec| crate::planning::markdown(spec, &state.tasks))
        .unwrap_or_default();
    let origin = task
        .source
        .as_ref()
        .map(crate::planning::source_note)
        .unwrap_or_default();
    let related = crate::planning::related_context(state, task);
    let files = crate::planning::attached_files(task);
    let (repositories, _) = crate::planning::other_repositories(state, task);
    let comments = task
        .plan
        .as_ref()
        .map(|plan| {
            plan.comments
                .iter()
                .map(|comment| format!("- {}", comment.text.trim()))
                .collect::<Vec<_>>()
        })
        .filter(|list| !list.is_empty())
        .map(|list| format!("\n\n## Comments on the earlier draft\n{}", list.join("\n")))
        .unwrap_or_default();
    let prompt = format!(
        "{instructions}\n\nDo not change anything in the repository, and do not commit: only read, and write the plan.\n\n{spec}\n## Current task\n{}\n\n{}{origin}{repositories}{files}{related}{comments}\n\n{closing}",
        task.title,
        task.details,
    );
    Ok(crate::stage::fit(&prompt))
}

/// Starts a planner for the task. Returns its session, to be started.
pub fn begin(workspace: &mut Workspace, context: &Context<'_>, task_id: &str) -> Result<String> {
    let task = find(workspace, task_id)?;
    ensure!(
        !matches!(task.status, TaskStatus::Building | TaskStatus::Planning),
        "The task is already running."
    );
    ensure!(
        task.status != TaskStatus::PlanReview,
        "Approve the plan or ask for changes first."
    );
    let run = new_id();
    let file = stage::file(workspace, Stage::Plan, &run);
    std::fs::create_dir_all(stage::dir(workspace))?;
    let mut input = NewSession::new(
        task.project_id.clone(),
        task.agent,
        head(&format!("Plan: {}", task.title), 200).to_string(),
    );
    input.prompt = brief(workspace, &task, &file)?;
    input.model = task.model.clone().unwrap_or_default();
    input.effort = task.effort.clone().unwrap_or_default();
    input.profile_id = task.profile_id.clone();
    input.skills = context.skills(&task.project_id, task.agent, Role::Plan);
    input.add_dirs = crate::planning::other_repositories(workspace.state(), &task).1;
    // Claude plans in its own plan mode, which cannot change anything.
    if native(task.agent) {
        input.permission_mode = Some("plan".into());
    }
    let session = workspace
        .add_session(input)?
        .sessions
        .last()
        .map(|session| session.id.clone())
        .unwrap_or_default();
    let planner = session.clone();
    pipeline::change(workspace, task_id, move |task| {
        let comments = task
            .plan
            .take()
            .map(|plan| plan.comments)
            .unwrap_or_default();
        task.plan = Some(PlanRun {
            id: run,
            status: PlanStatus::Drafting,
            text: String::new(),
            comments,
            session_id: Some(planner),
            approved_at: None,
            asked_at: Some(crate::time::now_iso8601()),
        });
        task.status = TaskStatus::Planning;
        task.last_error = None;
    })?;
    Ok(session)
}

/// The planner wrote its plan: it waits for a person, or is approved at
/// once when this task approves its plans by itself.
pub fn written(workspace: &mut Workspace, task_id: &str, text: &str) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let automatic = auto_approve(workspace, &task);
    let text = head(text, 32_000).to_string();
    pipeline::change(workspace, task_id, move |task| {
        if let Some(plan) = task.plan.as_mut() {
            plan.text = text;
            plan.status = PlanStatus::Waiting;
        }
        task.status = TaskStatus::PlanReview;
    })?;
    if automatic {
        approve(workspace, task_id)?;
        return Ok(vec![
            Action::Notice {
                task: task_id.to_string(),
                event: "plan_approved",
                detail: String::new(),
            },
            Action::Run {
                task: task_id.to_string(),
            },
        ]);
    }
    Ok(vec![Action::Notice {
        task: task_id.to_string(),
        event: "plan_ready",
        detail: String::new(),
    }])
}

/// A person approves the plan: the task is queued, to be built with it.
pub fn approve(workspace: &mut Workspace, task_id: &str) -> Result<()> {
    let task = find(workspace, task_id)?;
    ensure!(
        task.plan
            .as_ref()
            .is_some_and(|plan| plan.status == PlanStatus::Waiting && !plan.text.trim().is_empty()),
        "There is no plan waiting for approval."
    );
    pipeline::change(workspace, task_id, |task| {
        if let Some(plan) = task.plan.as_mut() {
            plan.status = PlanStatus::Approved;
            plan.approved_at = Some(crate::time::now_iso8601());
        }
        task.status = TaskStatus::Queued;
        // The builder is prepared afresh, with the plan in its brief.
        task.session_id = None;
        task.stage_run = None;
    })
}

/// A person asks for changes: the comment goes to the planner, which
/// rewrites the plan.
pub fn request_changes(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    comment: &str,
) -> Result<Vec<Action>> {
    let comment = comment.trim().to_string();
    ensure!(!comment.is_empty(), "Say what should change in the plan.");
    let task = find(workspace, task_id)?;
    let Some(plan) = task.plan.clone() else {
        bail!("This task has no plan yet.")
    };
    ensure!(
        plan.status == PlanStatus::Waiting,
        "Only a plan waiting for approval can be sent back."
    );
    let note = PlanComment {
        at: crate::time::now_iso8601(),
        text: head(&comment, 8_000).to_string(),
    };
    pipeline::change(workspace, task_id, move |task| {
        if let Some(plan) = task.plan.as_mut() {
            plan.comments.push(note);
        }
    })?;
    let planner = plan
        .session_id
        .as_deref()
        .and_then(|id| workspace.session(id).ok())
        .filter(|session| !session.is_archived())
        .map(|session| session.id.clone());
    let Some(planner) = planner else {
        // The planner is gone: a new one starts, with the comments.
        pipeline::change(workspace, task_id, |task| task.status = TaskStatus::Queued)?;
        let session = begin(workspace, context, task_id)?;
        return Ok(vec![Action::Start {
            task: task_id.to_string(),
            session,
        }]);
    };
    let run = new_id();
    let file = stage::file(workspace, Stage::Plan, &run);
    let claude = workspace
        .session(&planner)
        .is_ok_and(|session| native(session.agent));
    let text = format!(
        "Revise the plan. The reviewer of your plan says:\n\n{comment}\n\n{}",
        if claude {
            "Present the whole revised plan with ExitPlanMode again, not only what changed."
                .to_string()
        } else {
            format!(
                "Write the whole revised plan again, not only what changed.\n\n{}",
                stage::closing(Stage::Plan, &file)
            )
        }
    );
    let asked = crate::time::now_iso8601();
    pipeline::change(workspace, task_id, move |task| {
        if let Some(plan) = task.plan.as_mut() {
            plan.id = run;
            plan.status = PlanStatus::Drafting;
            plan.asked_at = Some(asked);
        }
        task.status = TaskStatus::Planning;
    })?;
    if (context.running)(&planner) {
        Ok(vec![Action::Send {
            task: task_id.to_string(),
            session: planner,
            text,
        }])
    } else {
        crate::qa::queue_feedback(workspace, &planner, &text)?;
        Ok(vec![Action::Start {
            task: task_id.to_string(),
            session: planner,
        }])
    }
}
