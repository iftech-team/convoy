//! Specifications, tasks and the queue.
//!
//! A specification is revised and approved; tasks reference the revision they
//! were written against; a task session is prepared from both. A clean exit
//! moves a task to review, never to done — accepting work stays a decision a
//! person makes.

use super::Workspace;
use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::planning::{markdown, SpecInput, TaskInput};
use convoy_core::queue::Completion;
#[cfg(test)]
use convoy_core::session::ExitCause;
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
    pub project_id: String,
    pub title: String,
    pub details: String,
    pub findings: String,
    pub agent: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub source: Option<convoy_core::integrations::IssueSource>,
    pub status: String,
    pub mode: String,
    pub auto_review: bool,
    pub spec_id: Option<String>,
    pub session_id: Option<String>,
    pub last_error: Option<String>,
    /// The pull request its session opened.
    pub pr_url: Option<String>,
    pub sync: Option<convoy_core::integrations::sync::SyncState>,
    pub qa: Option<convoy_core::qa::QaRun>,
    /// The session reviewing its work, when there is one.
    pub review_session_id: Option<String>,
    pub depends_on: Vec<String>,
    /// Tasks of any project its agent reads first.
    pub related: Vec<String>,
    pub profile_id: Option<String>,
    /// The titles of the tasks it still waits for.
    pub blocked_by: Vec<String>,
    /// Skills its session starts with, by id.
    pub skills: Vec<String>,
    /// The current round of fixes.
    pub round: u32,
    pub max_rounds: u32,
    pub review: Option<convoy_core::pipeline::ReviewRun>,
    pub build_summary: Option<String>,
    pub plan_first: bool,
    pub plan_auto_approve: Option<bool>,
    pub plan: Option<convoy_core::plan::PlanRun>,
    pub docs: Vec<convoy_core::task_docs::TaskDoc>,
    pub docs_run: Option<convoy_core::task_docs::DocsRun>,
    pub publish_run: Option<convoy_core::publish::PublishRun>,
    pub pr_title: Option<String>,
    pub pr_body: Option<String>,
}

fn task_view(state: &convoy_core::model::State, task: &convoy_core::model::Task) -> TaskView {
    TaskView {
        id: task.id.clone(),
        project_id: task.project_id.clone(),
        title: task.title.clone(),
        details: task.details.clone(),
        findings: task.findings.clone(),
        agent: task.agent.as_str().to_string(),
        model: task.model.clone(),
        effort: task.effort.clone(),
        source: task.source.clone(),
        status: task.status.as_str().to_string(),
        mode: mode_name(task.mode).to_string(),
        auto_review: task.auto_review,
        spec_id: task.spec_id.clone(),
        session_id: task.session_id.clone(),
        last_error: task.last_error.clone(),
        pr_url: task.pr_url.clone(),
        sync: task.sync.clone(),
        qa: task.qa.clone(),
        depends_on: task.depends_on.clone(),
        related: task.related.clone(),
        profile_id: task.profile_id.clone(),
        skills: task.skills.clone().unwrap_or_default(),
        round: task.round,
        max_rounds: state
            .projects
            .iter()
            .find(|project| project.id == task.project_id)
            .map(convoy_core::pipeline::max_rounds)
            .unwrap_or(convoy_core::pipeline::DEFAULT_MAX_FIX_ROUNDS),
        review: task.review.clone(),
        build_summary: task.build_summary.clone(),
        plan_first: task.plan_first,
        plan_auto_approve: task.plan_auto_approve,
        plan: task.plan.clone(),
        docs: task.docs.clone(),
        docs_run: task.docs_run.clone(),
        publish_run: task.publish_run.clone(),
        pr_title: task.pr_title.clone(),
        pr_body: task.pr_body.clone(),
        blocked_by: convoy_core::queue::blocked_by(state, task)
            .into_iter()
            .map(|other| other.title.clone())
            .collect(),
        review_session_id: task.session_id.as_ref().and_then(|builder| {
            state
                .sessions
                .iter()
                .rev()
                .find(|session| {
                    session.review_of.as_ref() == Some(builder) && !session.is_archived()
                })
                .map(|session| session.id.clone())
        }),
    }
}

#[derive(Serialize)]
pub struct PlanningView {
    pub specs: Vec<SpecView>,
    pub tasks: Vec<TaskView>,
    pub queued: usize,
}

fn mode_name(mode: PublishMode) -> &'static str {
    mode.as_str()
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
            .map(|task| task_view(state, task))
            .collect();
        let queued = tasks.iter().filter(|task| task.status == "queued").count();
        Ok(PlanningView {
            specs,
            tasks,
            queued,
        })
    })
}

/// Every project's tasks, for the list that spans them.
#[tauri::command]
pub fn tasks_all(workspace: State<'_, Workspace>) -> Result<Vec<TaskView>, String> {
    workspace.with(|workspace| {
        let state = workspace.state();
        Ok(state
            .tasks
            .iter()
            .map(|task| task_view(state, task))
            .collect())
    })
}

#[tauri::command]
pub fn task_dependencies(
    id: String,
    depends_on: Vec<String>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.set_task_dependencies(&id, depends_on).map(|_| ()))
}

#[tauri::command]
pub fn task_related(
    id: String,
    related: Vec<String>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.set_task_related(&id, related).map(|_| ()))
}

#[tauri::command]
pub fn task_account(
    id: String,
    profile_id: Option<String>,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.set_task_account(&id, profile_id).map(|_| ()))
}

#[tauri::command]
pub fn task_link_session(
    id: String,
    session_id: String,
    workspace: State<'_, Workspace>,
) -> Result<(), String> {
    workspace.act(|workspace| workspace.link_task_session(&id, &session_id).map(|_| ()))
}

#[tauri::command]
pub fn task_delete(id: String, workspace: State<'_, Workspace>) -> Result<(), String> {
    workspace.act(|workspace| workspace.delete_task(&id).map(|_| ()))
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
    /// Skills its session starts with. None keeps an existing task's; a new
    /// task then gets the automatic ones.
    #[serde(default)]
    pub skills: Option<Vec<String>>,
}

#[tauri::command]
/// Returns the task's id, so a new task can be run at once.
pub fn task_save(input: TaskFormInput, workspace: State<'_, Workspace>) -> Result<String, String> {
    let agent = Agent::parse(&input.agent).ok_or("Unknown agent.")?;
    let mode = PublishMode::parse(&input.mode)
        .ok_or_else(|| format!("Unknown publication mode: {}", input.mode))?;
    let existing = input.id.clone().filter(|id| !id.is_empty());
    let skills = match (&input.skills, &existing) {
        (None, Some(_)) => None,
        (asked, _) => Some(crate::skills::resolve(
            &workspace.storage,
            &input.project_id,
            None,
            asked.as_deref(),
        )?),
    };
    workspace.act(|workspace| {
        let state = workspace.save_task(TaskInput {
            skills,
            id: input.id,
            project_id: input.project_id,
            spec_id: input.spec_id,
            title: input.title,
            details: input.details,
            findings: input.findings,
            agent,
            mode,
            auto_review: input.auto_review,
        })?;
        Ok(existing.unwrap_or_else(|| {
            state
                .tasks
                .last()
                .map(|task| task.id.clone())
                .unwrap_or_default()
        }))
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
        "pr" => TaskStatus::Pr,
        "testing" => TaskStatus::Testing,
        "changes" => TaskStatus::Changes,
        "done" => TaskStatus::Done,
        "failed" => TaskStatus::Failed,
        "attention" => TaskStatus::Attention,
        other => return Err(format!("Invalid task status: {other}")),
    };
    workspace.act(|workspace| workspace.set_task_status(&id, status).map(|_| ()))
}

/// Writes the brief from the approved specification and the task, and returns
/// the session it belongs to. A task planned first gets its planner instead,
/// until its plan is approved.
#[tauri::command]
pub async fn task_prepare(
    id: String,
    profile_id: Option<String>,
    workspace: State<'_, Workspace>,
    terminals: State<'_, std::sync::Arc<crate::pty::Terminals>>,
    chats: State<'_, std::sync::Arc<crate::chat::Chats>>,
    repositories: State<'_, super::Repositories>,
) -> Result<String, String> {
    let session = prepare(&workspace, &terminals, &chats, &id, profile_id)?;
    // A new builder in a project that gives each task a worktree gets one,
    // set up, before it starts.
    let branch = workspace.act(|core| Ok(convoy_core::queue::wants_worktree(core, &id)))?;
    if let Some(branch) = branch {
        if let Err(error) = task_worktree(&workspace, &repositories, &session, &branch).await {
            let reason = format!("Its worktree could not be prepared. {error}");
            workspace
                .act(|core| convoy_core::pipeline::attention(core, &id, &reason).map(|_| ()))?;
            return Err(reason);
        }
    }
    Ok(session)
}

/// Creates the builder's worktree on `branch` (or the first free variant of
/// it), copies the shared paths and runs the setup commands. Turning on a
/// worktree per task in the project is the consent to run them unasked.
async fn task_worktree(
    workspace: &Workspace,
    repositories: &super::Repositories,
    session: &str,
    branch: &str,
) -> Result<(), String> {
    let (project, shared, command) = workspace.act(|core| {
        let project_id = core.session(session)?.project_id.clone();
        let project = core.project(&project_id)?.clone();
        let (shared, command) = convoy_core::worktree::setup_parts(core, &project);
        Ok((project, shared, command))
    })?;
    let root = workspace.storage.worktrees();
    let git = convoy_core::Git::default();
    let (directory, branch) = repositories.with(&project.path, || {
        // A task prepared again after an edit keeps its old branch; the new
        // one takes the next free name.
        let mut name = branch.to_string();
        for n in 2..10 {
            if !git.succeeds(
                &project.path,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{name}"),
                ],
            ) {
                break;
            }
            name = format!("{branch}-{n}");
        }
        git.create_worktree_from(&project.path, &root, &name, project.base_ref.as_deref())
            .map(|directory| (directory, name))
            .map_err(|error| error.to_string())
    })?;
    workspace
        .act(|core| convoy_core::worktree::record_create(core, session, &directory, &branch))?;
    tauri::async_runtime::spawn_blocking(move || {
        convoy_core::worktree::setup_within(
            &project.path,
            &directory,
            &shared,
            command.as_deref().filter(|value| !value.trim().is_empty()),
            &convoy_core::StdRunner,
            std::time::Duration::from_secs(15 * 60),
        )
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())
}

/// The builder's session, or the planner's for a task planned first.
fn prepare(
    workspace: &Workspace,
    terminals: &crate::pty::Terminals,
    chats: &crate::chat::Chats,
    id: &str,
    profile_id: Option<String>,
) -> Result<String, String> {
    let id = id.to_string();
    crate::pipeline::run(workspace, terminals, chats, |workspace, context| {
        let planned = workspace
            .state()
            .tasks
            .iter()
            .find(|task| task.id == id)
            .is_some_and(convoy_core::plan::needed);
        if planned {
            return convoy_core::plan::begin(workspace, context, &id);
        }
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
/// What a finished session means for the queue. Called after every exit, not
/// only while a queue is running: a single task with automatic review set is
/// still handed on when it finishes.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AfterExit {
    /// Not a task session, or its round was already handled.
    Nothing,
    /// The specification moved on while the task ran. The queue stops.
    Superseded,
    /// The round is done; the pipeline asks for these. The queue may pull
    /// the next task.
    Advance {
        actions: Vec<convoy_core::pipeline::Action>,
    },
}

#[tauri::command]
pub fn queue_after_exit(
    id: String,
    clean: bool,
    workspace: State<'_, Workspace>,
    terminals: State<'_, std::sync::Arc<crate::pty::Terminals>>,
    chats: State<'_, std::sync::Arc<crate::chat::Chats>>,
) -> Result<AfterExit, String> {
    let running = |session: &str| crate::chat::is_running(&terminals, &chats, session);
    after_exit(&workspace, &id, clean, &running)
}

/// The rule itself, apart from Tauri's state extractor so a test can reach it.
pub fn after_exit(
    workspace: &Workspace,
    id: &str,
    clean: bool,
    running: &dyn Fn(&str) -> bool,
) -> Result<AfterExit, String> {
    // A failure or a stop pauses the queue, and the caller does that; there is
    // nothing to hand on.
    if !clean {
        return Ok(AfterExit::Nothing);
    }
    crate::pipeline::run_with(workspace, running, |core, context| {
        core.session(id)?;
        match convoy_core::queue::on_clean_exit(core, context, id)? {
            Completion::Nothing => Ok(AfterExit::Nothing),
            Completion::Superseded => {
                convoy_core::queue::mark_superseded(core, id)?;
                Ok(AfterExit::Superseded)
            }
            Completion::Advance(actions) => Ok(AfterExit::Advance { actions }),
        }
    })
}

/// The tasks the queue starts now in a project: as many as its free slots,
/// none waiting for another that is not done.
#[tauri::command]
pub fn queue_next(
    project_id: String,
    workspace: State<'_, Workspace>,
    terminals: State<'_, std::sync::Arc<crate::pty::Terminals>>,
    chats: State<'_, std::sync::Arc<crate::chat::Chats>>,
) -> Result<Vec<String>, String> {
    let running = |session: &str| crate::chat::is_running(&terminals, &chats, session);
    workspace.with(|core| Ok(convoy_core::queue::next_tasks(core, &project_id, &running)))
}

#[tauri::command]
pub fn queue_summary(
    project_id: String,
    workspace: State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    workspace.with(|workspace| Ok(convoy_core::queue::summary(workspace, &project_id)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use convoy_core::model::{Agent, PublishMode, TaskStatus};
    use convoy_core::{Storage, Workspace as CoreWorkspace};

    /// A task marked for automatic review hands its work to the other agent
    /// when it finishes, and the review session it creates is the one the
    /// caller is told to start.
    #[test]
    fn a_finished_task_marked_for_review_is_handed_to_the_other_agent() {
        let root = std::env::temp_dir().join(format!("convoy-handoff-{}", std::process::id()));
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let storage = Storage::new(root.join("state"));

        let mut core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        core.add_project(&project).unwrap();
        let project_id = core.state().projects[0].id.clone();
        core.save_task(TaskInput {
            skills: None,
            id: None,
            project_id,
            spec_id: None,
            title: "Something to review".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::None,
            auto_review: true,
        })
        .unwrap();
        let task_id = core.state().tasks[0].id.clone();
        core.prepare_task(&task_id, None).unwrap();
        let builder = core.state().tasks[0].session_id.clone().unwrap();
        // The exit rules have already run by the time the queue is asked.
        convoy_core::session::finish_session(&mut core, &builder, ExitCause::Exited(0)).unwrap();
        assert_eq!(core.state().tasks[0].status, TaskStatus::Review);
        drop(core);

        let workspace = Workspace::at(storage.clone());
        let idle = |_: &str| false;
        let step = after_exit(&workspace, &builder, true, &idle).unwrap();
        let AfterExit::Advance { actions } = step else {
            panic!("no review was handed on");
        };
        let session = actions
            .iter()
            .find_map(|action| match action {
                convoy_core::pipeline::Action::Start { session, .. } => Some(session.clone()),
                _ => None,
            })
            .expect("a review session to start");

        let core = CoreWorkspace::load(storage.workspace_file()).unwrap();
        let review = core
            .state()
            .sessions
            .iter()
            .find(|item| item.id == session)
            .expect("the review session was not saved");
        assert_eq!(review.review_of.as_deref(), Some(builder.as_str()));
        assert_eq!(
            review.agent,
            Agent::Codex,
            "a review goes to the other agent"
        );

        // One review per round: a second exit must not spawn another.
        assert!(matches!(
            after_exit(&workspace, &builder, true, &idle).unwrap(),
            AfterExit::Nothing
        ));
        std::fs::remove_dir_all(&root).ok();
    }
}
