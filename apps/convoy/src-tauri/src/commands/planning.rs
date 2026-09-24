//! Specifications, tasks and the queue.
//!
//! A specification is revised and approved; tasks reference the revision they
//! were written against; a task session is prepared from both. A clean exit
//! moves a task to review, never to done — accepting work stays a decision a
//! person makes.

use super::Workspace;
use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::planning::{markdown, SpecInput, TaskInput};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Serialize)]
pub struct SpecView {
    pub id: String,
    pub title: String,
    pub problem: String,
    pub requirements: String,
    pub acceptance: String,
    pub constraints: String,
    pub plan: String,
    pub revision: u64,
    pub approved: bool,
}

#[derive(Serialize)]
pub struct TaskView {
    pub id: String,
    pub title: String,
    pub details: String,
    pub findings: String,
    pub agent: String,
    pub status: String,
    pub mode: String,
    pub auto_review: bool,
    pub spec_id: Option<String>,
    pub session_id: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Serialize)]
pub struct PlanningView {
    pub specs: Vec<SpecView>,
    pub tasks: Vec<TaskView>,
    pub queued: usize,
}

fn mode_name(mode: PublishMode) -> &'static str {
    match mode {
        PublishMode::None => "none",
        PublishMode::Pr => "pr",
        PublishMode::Push => "push",
    }
}

#[tauri::command]
pub fn planning_read(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<PlanningView, String> {
    workspace.with(|workspace| {
        let state = workspace.state();
        let specs = state
            .specs
            .iter()
            .filter(|spec| spec.project_id == project_id)
            .map(|spec| SpecView {
                id: spec.id.clone(),
                title: spec.title.clone(),
                problem: spec.problem.clone(),
                requirements: spec.requirements.clone(),
                acceptance: spec.acceptance.clone(),
                constraints: spec.constraints.clone(),
                plan: spec.plan.clone(),
                revision: spec.revision,
                approved: spec.approved(),
            })
            .collect();
        let tasks: Vec<TaskView> = state
            .tasks
            .iter()
            .filter(|task| task.project_id == project_id)
            .map(|task| TaskView {
                id: task.id.clone(),
                title: task.title.clone(),
                details: task.details.clone(),
                findings: task.findings.clone(),
                agent: task.agent.as_str().to_string(),
                status: task.status.as_str().to_string(),
                mode: mode_name(task.mode).to_string(),
                auto_review: task.auto_review,
                spec_id: task.spec_id.clone(),
                session_id: task.session_id.clone(),
                last_error: task.last_error.clone(),
            })
            .collect();
        let queued = tasks.iter().filter(|task| task.status == "queued").count();
        Ok(PlanningView {
            specs,
            tasks,
            queued,
        })
    })
}

#[derive(Deserialize)]
pub struct SpecFormInput {
    pub id: Option<String>,
    pub project_id: String,
    pub title: String,
    pub problem: String,
    pub requirements: String,
    pub acceptance: String,
    pub constraints: String,
    pub plan: String,
}

#[tauri::command]
pub fn spec_save(input: SpecFormInput, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|workspace| {
        workspace
            .save_spec(SpecInput {
                id: input.id,
                project_id: input.project_id,
                title: input.title,
                problem: input.problem,
                requirements: input.requirements,
                acceptance: input.acceptance,
                constraints: input.constraints,
                plan: input.plan,
            })
            .map(|_| ())
    })
}

#[tauri::command]
pub fn spec_approve(
    id: String,
    revision: u64,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.approve_spec(&id, revision).map(|_| ()))
}

/// The Markdown a specification exports as, including its tasks.
#[tauri::command]
pub fn spec_markdown(id: String, workspace: State<'_, Workspace>) -> Result<String, String> {
    workspace.with(|workspace| {
        let state = workspace.state();
        let spec = state
            .specs
            .iter()
            .find(|spec| spec.id == id)
            .ok_or("Specification not found.")?;
        Ok(markdown(spec, &state.tasks))
    })
}

#[derive(Deserialize)]
pub struct TaskFormInput {
    pub id: Option<String>,
    pub project_id: String,
    pub spec_id: Option<String>,
    pub title: String,
    pub details: String,
    pub findings: String,
    pub agent: String,
    pub mode: String,
    pub auto_review: bool,
}

#[tauri::command]
pub fn task_save(input: TaskFormInput, workspace: State<'_, Workspace>) -> Result<(), String> {
    let agent = Agent::parse(&input.agent).ok_or("Unknown agent.")?;
    let mode = match input.mode.as_str() {
        "none" => PublishMode::None,
        "pr" => PublishMode::Pr,
        "push" => PublishMode::Push,
        other => return Err(format!("Unknown publication mode: {other}")),
    };
    workspace.act(|workspace| {
        workspace
            .save_task(TaskInput {
                id: input.id,
                project_id: input.project_id,
                spec_id: input.spec_id,
                title: input.title,
                details: input.details,
                findings: input.findings,
                agent,
                mode,
                auto_review: input.auto_review,
            })
            .map(|_| ())
    })
}

#[tauri::command]
pub fn task_status(
    id: String,
    status: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    let status = match status.as_str() {
        "queued" => TaskStatus::Queued,
        "review" => TaskStatus::Review,
        "changes" => TaskStatus::Changes,
        "done" => TaskStatus::Done,
        other => return Err(format!("Invalid task status: {other}")),
    };
    workspace.act(|workspace| workspace.set_task_status(&id, status).map(|_| ()))
}

/// Writes the brief from the approved specification and the task, and returns
/// the session it belongs to.
#[tauri::command]
pub fn task_prepare(
    id: String,
    profile_id: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<String, String> {
    workspace.act(|workspace| {
        let state = workspace.prepare_task(&id, profile_id)?;
        Ok(state
            .tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.session_id.clone())
            .unwrap_or_default())
    })
}

/// What the queue would run, in order, with what each task will publish. Shown
/// before it starts: running agents costs money and can publish.
#[tauri::command]
pub fn queue_summary(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    workspace.with(|workspace| Ok(convoy_core::queue::summary(workspace, &project_id)))
}
