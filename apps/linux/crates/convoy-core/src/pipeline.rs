//! The order a task's stages run in, and what each outcome does next.
//!
//! ```text
//! build done ─► checks ─► review ─► QA agent ─► ready (a person marks it done)
//!        fail ─┴────────┴─────────┴─► findings to the builder, round += 1
//!                                     out of rounds ─► needs attention
//! ```
//!
//! [`advance`] is the whole rule, as a pure function of the task, its
//! project and what just happened. [`handle`] applies its answer to the
//! workspace and returns what the window has to do — start a session, send
//! one text, run QA — since only the window can start agents.
//!
//! Stages report back by file ([`crate::stage`]); [`poll`] reads them, and
//! [`on_exit`] is the fallback when a session ends without one. Every run is
//! named, and a run already acted on is never acted on again.

use crate::model::{Agent, Project, Role, Task, TaskStatus};
use crate::qa::{self, QaStatus};
use crate::stage::{self, Stage, StageResult};
use crate::workspace::{new_id, Workspace};
use crate::{bail, json::head, Result};
use serde::{Deserialize, Serialize};

/// Rounds of fixes a task gets before it needs attention.
pub const DEFAULT_MAX_FIX_ROUNDS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    Reviewing,
    Passed,
    Failed,
    /// The reviewer ended without writing a verdict.
    NoVerdict,
    /// A person chose to go on without it.
    Skipped,
}

/// A task's latest review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: String,
    pub status: ReviewStatus,
    /// The fix round it reviewed.
    #[serde(default)]
    pub round: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub findings: String,
    pub started: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<String>,
}

/// What just happened to a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Its builder finished a round.
    BuildDone,
    /// The project's checks passed.
    ChecksPassed,
    ChecksFailed(String),
    /// The reviewer's verdict; `None` when it left none.
    Review(Option<bool>, String),
    /// The QA agent's verdict; `None` when it left none.
    Qa(Option<bool>, String),
}

/// What comes next, as [`advance`] decides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Run QA: the checks first.
    Test,
    Review,
    QaAgent(Agent),
    /// Back to the builder with findings, for another round.
    Fix {
        from: &'static str,
        findings: String,
    },
    Attention(String),
    /// Everything automatic has passed; a person takes it from here.
    Ready,
    /// Nothing is automatic in this project: the task waits for a person.
    Wait,
}

/// What the window has to do for the pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// Start this session, or resume it: its brief, or what it was sent,
    /// is waiting for it.
    Start { task: String, session: String },
    /// Send text to this running session, as a message.
    Send {
        task: String,
        session: String,
        text: String,
    },
    /// Run QA for the task, starting with its checks.
    Test { task: String },
    /// Run the task: its plan is approved.
    Run { task: String },
    /// Push the task's branch and open its request, off the window's thread.
    Publish { task: String },
    /// This session finished its stage: put it to sleep. It can be resumed.
    Hibernate { session: String },
    /// The builder finished a round: a running queue may start the next task.
    Built { task: String, project: String },
    /// Something to tell the person, by name: `ready`, `attention`,
    /// `fixing`, `plan_ready`, `plan_approved`, `docs_written`, `published`,
    /// `qa_queued`.
    Notice {
        task: String,
        event: &'static str,
        detail: String,
    },
}

/// Whether a session currently has a process.
pub type Running<'a> = &'a dyn Fn(&str) -> bool;

/// What the pipeline needs beyond the workspace.
pub struct Context<'a> {
    /// The skills library, for each role's skills; none attaches none.
    pub skills: Option<&'a crate::skills::Store>,
    pub running: Running<'a>,
}

impl Context<'_> {
    /// The automatic skills of this role, for this agent in this project.
    pub fn skills(&self, project_id: &str, agent: Agent, role: Role) -> Vec<String> {
        self.skills
            .map(|store| store.for_role(project_id, agent, role))
            .unwrap_or_default()
    }
}

pub fn max_rounds(project: &Project) -> u32 {
    project.max_fix_rounds.unwrap_or(DEFAULT_MAX_FIX_ROUNDS)
}

/// Whether the task still needs a review in its current round.
fn review_due(task: &Task) -> bool {
    task.auto_review
        && !task.review.as_ref().is_some_and(|run| {
            run.round == task.round
                && matches!(run.status, ReviewStatus::Passed | ReviewStatus::Skipped)
        })
}

/// The rule: what follows `event` for this task in this project.
pub fn advance(task: &Task, project: &Project, event: &Event) -> Step {
    let fix = |from: &'static str, findings: &str| {
        if task.round >= max_rounds(project) {
            Step::Attention(format!(
                "{from} still fails after {} rounds of fixes.\n\n{findings}",
                task.round
            ))
        } else {
            Step::Fix {
                from,
                findings: findings.to_string(),
            }
        }
    };
    let qa_waits = task
        .qa
        .as_ref()
        .is_some_and(|run| run.status == QaStatus::Waiting);
    match event {
        Event::BuildDone => {
            if project.qa_auto == Some(true) && qa::configured(project) {
                Step::Test
            } else if review_due(task) {
                Step::Review
            } else {
                Step::Wait
            }
        }
        Event::ChecksPassed => {
            if review_due(task) {
                Step::Review
            } else if let Some(agent) = project.qa_agent {
                Step::QaAgent(agent)
            } else {
                Step::Ready
            }
        }
        Event::ChecksFailed(findings) => fix("The checks", findings),
        Event::Review(Some(true), _) => match project.qa_agent {
            Some(agent) if qa_waits => Step::QaAgent(agent),
            _ => Step::Ready,
        },
        Event::Review(Some(false), findings) => fix("The review", findings),
        Event::Review(None, _) => Step::Attention("The reviewer ended without a verdict.".into()),
        Event::Qa(Some(true), _) => Step::Ready,
        Event::Qa(Some(false), findings) => fix("QA", findings),
        Event::Qa(None, _) => Step::Attention("The QA agent ended without a verdict.".into()),
    }
}

/// What reviewers, QA and the documentation writer read besides the task:
/// the approved plan, and what the builder said it did.
pub fn task_context(task: &Task) -> String {
    let mut context = String::new();
    if let Some(plan) = task.plan.as_ref().filter(|plan| plan.approved()) {
        context.push_str(&format!(
            "\n## Approved plan\nCheck the work against this plan too.\n\n{}\n",
            plan.text.trim()
        ));
    }
    context.push_str(&crate::planning::attached_files(task));
    if let Some(summary) = task
        .build_summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        context.push_str(&format!(
            "\n## What the builder reports\n{}\n",
            summary.trim()
        ));
    }
    context
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

/// Changes one task.
pub(crate) fn change(
    workspace: &mut Workspace,
    task_id: &str,
    edit: impl FnOnce(&mut Task),
) -> Result<()> {
    let id = task_id.to_string();
    workspace.update(move |state| {
        let Some(task) = state.tasks.iter_mut().find(|task| task.id == id) else {
            bail!("Task not found.")
        };
        edit(task);
        Ok(())
    })?;
    Ok(())
}

/// A session that still exists and is not archived.
fn live_session(workspace: &Workspace, id: Option<&str>) -> Option<String> {
    let id = id?;
    workspace
        .session(id)
        .ok()
        .filter(|session| !session.is_archived())
        .map(|session| session.id.clone())
}

/// Decides what follows `event` and does it.
pub fn handle(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    event: Event,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let project = workspace.project(&task.project_id)?.clone();
    let step = advance(&task, &project, &event);
    apply(workspace, context, &task, step)
}

fn apply(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task: &Task,
    step: Step,
) -> Result<Vec<Action>> {
    let id = task.id.clone();
    match step {
        Step::Wait => Ok(Vec::new()),
        Step::Test => Ok(vec![Action::Test { task: id }]),
        Step::Review => start_review(workspace, context, &id),
        Step::QaAgent(_) if qa::environment_busy(workspace, &id) => {
            qa::enqueue(workspace, &id)?;
            Ok(vec![Action::Notice {
                task: id,
                event: "qa_queued",
                detail: String::new(),
            }])
        }
        Step::QaAgent(agent) => {
            let skills = context.skills(&task.project_id, agent, Role::Qa);
            let session =
                qa::start_agent(workspace, &stage::qa_dir(workspace), &skills, &id, agent)?;
            Ok(vec![Action::Start { task: id, session }])
        }
        Step::Fix { from, findings } => fix(workspace, context, &id, from, &findings),
        Step::Attention(reason) => attention(workspace, &id, &reason),
        Step::Ready => ready(workspace, context, &id),
    }
}

/// The task is stuck: a person has to look at it.
pub fn attention(workspace: &mut Workspace, task_id: &str, reason: &str) -> Result<Vec<Action>> {
    let detail = head(reason, 16_000).to_string();
    let kept = detail.clone();
    change(workspace, task_id, move |task| {
        task.status = TaskStatus::Attention;
        task.last_error = Some(kept);
    })?;
    Ok(vec![Action::Notice {
        task: task_id.to_string(),
        event: "attention",
        detail,
    }])
}

/// Everything automatic passed. The task waits for a person in testing when
/// QA ran, in review otherwise; the document is written next when the
/// project wants it.
fn ready(workspace: &mut Workspace, context: &Context<'_>, task_id: &str) -> Result<Vec<Action>> {
    change(workspace, task_id, |task| {
        let tested = task
            .qa
            .as_ref()
            .is_some_and(|run| run.status == QaStatus::Passed);
        task.status = if tested {
            TaskStatus::Testing
        } else if task.pr_url.is_some() {
            TaskStatus::Pr
        } else {
            TaskStatus::Review
        };
    })?;
    let mut actions = vec![Action::Notice {
        task: task_id.to_string(),
        event: "ready",
        detail: String::new(),
    }];
    let task = find(workspace, task_id)?;
    // Published first, so the document can link the request.
    if crate::publish::due(workspace, &task) {
        actions.extend(crate::publish::begin(workspace, context, task_id)?);
    } else if workspace.project(&task.project_id)?.docs_auto == Some(true) {
        actions.extend(crate::task_docs::begin(workspace, context, task_id)?);
    }
    Ok(actions)
}

/// What a builder is told when its work comes back.
fn fix_message(from: &str, round: u32, max: u32, findings: &str, done: &std::path::Path) -> String {
    format!(
        "{from} found problems with this task (fix round {round} of {max}). Fix them, verify the fix, and report what you changed.\n\n{}\n\n{}",
        findings.trim(),
        stage::closing(Stage::Build, done)
    )
}

/// Sends the findings to the builder for another round: typed into it when
/// it runs, otherwise waiting for it as it is resumed.
fn fix(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    from: &'static str,
    findings: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let Some(builder) = live_session(workspace, task.session_id.as_deref()) else {
        return attention(
            workspace,
            task_id,
            &format!("{from} found problems, and the task has no builder session to fix them.\n\n{findings}"),
        );
    };
    let max = max_rounds(workspace.project(&task.project_id)?);
    let round = task.round + 1;
    let run = new_id();
    let done = stage::file(workspace, Stage::Build, &run);
    let text = fix_message(from, round, max, findings, &done);
    let running = (context.running)(&builder);
    let findings = head(findings, 16_000).to_string();
    change(workspace, task_id, move |task| {
        // A QA run that waited for this review, or for its environment, is
        // over: the next round is tested afresh.
        if task
            .qa
            .as_ref()
            .is_some_and(|run| matches!(run.status, QaStatus::Waiting | QaStatus::Queued))
        {
            task.qa = None;
        }
        task.round = round;
        task.stage_run = Some(run);
        task.findings = findings;
        task.last_error = None;
        task.status = if running {
            TaskStatus::Building
        } else {
            TaskStatus::Changes
        };
    })?;
    let mut actions = vec![Action::Notice {
        task: task_id.to_string(),
        event: "fixing",
        detail: from.to_string(),
    }];
    if running {
        actions.push(Action::Send {
            task: task_id.to_string(),
            session: builder,
            text,
        });
    } else {
        qa::queue_feedback(workspace, &builder, &text)?;
        actions.push(Action::Start {
            task: task_id.to_string(),
            session: builder,
        });
    }
    Ok(actions)
}

/// Starts the review of the builder's work in its current round.
fn start_review(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let Some(builder) = live_session(workspace, task.session_id.as_deref()) else {
        return attention(workspace, task_id, "There is no builder session to review.");
    };
    let run = new_id();
    let verdict = stage::file(workspace, Stage::Review, &run);
    std::fs::create_dir_all(stage::dir(workspace))?;
    let output = crate::history::History::new(stage::storage(workspace).history())
        .read(&builder)
        .unwrap_or_default();
    let brief = crate::review::task_brief(workspace, &builder, &output, &task, &verdict)?;
    let mut input = crate::review::handoff(workspace, &builder, brief)?;
    input.skills = context.skills(&task.project_id, input.agent, Role::Review);
    let session = workspace
        .add_session(input)?
        .sessions
        .last()
        .map(|session| session.id.clone())
        .unwrap_or_default();
    let record = ReviewRun {
        id: run,
        status: ReviewStatus::Reviewing,
        round: task.round,
        session_id: Some(session.clone()),
        findings: String::new(),
        started: crate::time::now_iso8601(),
        finished: None,
    };
    change(workspace, task_id, move |task| {
        task.review = Some(record);
        if let Some(run) = task.qa.as_mut() {
            // The checks passed and the QA agent has not started: it waits
            // for this review.
            if run.status == QaStatus::Testing && run.session_id.is_none() {
                run.status = QaStatus::Waiting;
            }
        }
        if task.status != TaskStatus::Testing {
            task.status = if task.pr_url.is_some() {
                TaskStatus::Pr
            } else {
                TaskStatus::Review
            };
        }
    })?;
    Ok(vec![Action::Start {
        task: task_id.to_string(),
        session,
    }])
}

/// The builder finished a round — by its completion file, or by exiting
/// cleanly. Records what it said and moves the task on. Nothing happens for
/// a round already handled.
pub fn build_done(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    report: Option<(String, String)>,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    if task.stage_run.is_none() {
        return Ok(Vec::new());
    }
    change(workspace, task_id, move |task| {
        task.stage_run = None;
        if let Some((summary, verified)) = report {
            let mut text = summary;
            if !verified.is_empty() {
                text.push_str(&format!("\n\nVerified: {verified}"));
            }
            task.build_summary = Some(head(&text, 8_000).to_string());
        }
        if matches!(task.status, TaskStatus::Building | TaskStatus::Changes) {
            task.status = if task.pr_url.is_some() {
                TaskStatus::Pr
            } else {
                TaskStatus::Review
            };
        }
    })?;
    let mut actions = vec![Action::Built {
        task: task_id.to_string(),
        project: task.project_id.clone(),
    }];
    actions.extend(handle(workspace, context, task_id, Event::BuildDone)?);
    Ok(actions)
}

/// The QA checks ended: on to the review or the QA agent, or back to the
/// builder.
pub fn after_checks(
    workspace: &mut Workspace,
    context: &Context<'_>,
    next: qa::Next,
    task_id: &str,
) -> Result<Vec<Action>> {
    let event = match next {
        qa::Next::Failed(outcome) => Event::ChecksFailed(outcome.findings),
        qa::Next::Agent(_) | qa::Next::Passed => Event::ChecksPassed,
    };
    handle(workspace, context, task_id, event)
}

/// Records a reviewer's verdict and moves the task on. `None` when it left
/// none.
pub fn review_verdict(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    verdict: Option<(bool, String)>,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    if !task
        .review
        .as_ref()
        .is_some_and(|run| run.status == ReviewStatus::Reviewing)
    {
        return Ok(Vec::new());
    }
    let (passed, findings) = match verdict {
        Some((passed, findings)) => (Some(passed), findings),
        None => (None, String::new()),
    };
    let findings = if passed == Some(false) && findings.is_empty() {
        "The reviewer failed the work without saying why; see its session.".to_string()
    } else {
        findings
    };
    let kept = findings.clone();
    change(workspace, task_id, move |task| {
        if let Some(run) = task.review.as_mut() {
            run.status = match passed {
                Some(true) => ReviewStatus::Passed,
                Some(false) => ReviewStatus::Failed,
                None => ReviewStatus::NoVerdict,
            };
            run.findings = head(&kept, 16_000).to_string();
            run.finished = Some(crate::time::now_iso8601());
        }
    })?;
    handle(workspace, context, task_id, Event::Review(passed, findings))
}

/// A person asks for the review now: the builder's round counts as done,
/// whether or not it wrote its completion file. For a builder briefed
/// before the pipeline, which was never told to write one.
pub fn review_now(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    crate::ensure!(
        !matches!(
            task.status,
            TaskStatus::Building
                | TaskStatus::Planning
                | TaskStatus::PlanReview
                | TaskStatus::Queued
        ),
        "Review a task once its builder has finished."
    );
    // A review under way is replaced: one that went quiet, or wrote the
    // wrong file, is put to sleep and a fresh reviewer starts.
    let stale = task
        .review
        .as_ref()
        .filter(|run| run.status == ReviewStatus::Reviewing)
        .and_then(|run| run.session_id.clone());
    change(workspace, task_id, |task| {
        task.stage_run = None;
        task.last_error = None;
    })?;
    let mut actions: Vec<Action> = hibernate(context, stale.as_ref());
    actions.extend(start_review(workspace, context, task_id)?);
    Ok(actions)
}

/// A person goes on without the review.
pub fn skip_review(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let round = task.round;
    let reviewer = task.review.as_ref().and_then(|run| run.session_id.clone());
    change(workspace, task_id, move |task| {
        let run = task.review.get_or_insert_with(|| ReviewRun {
            id: new_id(),
            status: ReviewStatus::Skipped,
            round,
            session_id: None,
            findings: String::new(),
            started: crate::time::now_iso8601(),
            finished: None,
        });
        run.status = ReviewStatus::Skipped;
        run.round = round;
        run.finished = Some(crate::time::now_iso8601());
        if task.status == TaskStatus::Attention {
            task.last_error = None;
        }
    })?;
    let mut actions: Vec<Action> = reviewer
        .filter(|id| (context.running)(id))
        .map(|session| Action::Hibernate { session })
        .into_iter()
        .collect();
    actions.extend(handle(
        workspace,
        context,
        task_id,
        Event::Review(Some(true), String::new()),
    )?);
    Ok(actions)
}

/// A QA agent's verdict, and what follows it.
fn qa_verdict(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task: &Task,
    verdict: Option<(bool, String)>,
) -> Result<Vec<Action>> {
    let (passed, findings) = match verdict {
        Some((passed, findings)) => (Some(passed), findings),
        None => (None, String::new()),
    };
    let outcome = qa::conclude(workspace, task, passed, findings)?;
    handle(
        workspace,
        context,
        &task.id,
        Event::Qa(outcome.passed, outcome.findings),
    )
}

fn hibernate(context: &Context<'_>, session: Option<&String>) -> Vec<Action> {
    session
        .filter(|id| (context.running)(id))
        .map(|id| Action::Hibernate {
            session: id.clone(),
        })
        .into_iter()
        .collect()
}

/// Starts the QA runs waiting for an environment that is free now, oldest
/// first, one per environment.
fn release_environments(workspace: &mut Workspace, context: &Context<'_>) -> Result<Vec<Action>> {
    let mut waiting: Vec<Task> = workspace
        .state()
        .tasks
        .iter()
        .filter(|task| {
            task.qa
                .as_ref()
                .is_some_and(|run| run.status == QaStatus::Queued)
        })
        .cloned()
        .collect();
    waiting.sort_by(|a, b| {
        let started = |task: &Task| task.qa.as_ref().map(|run| run.started.clone());
        started(a).cmp(&started(b))
    });
    let mut actions = Vec::new();
    for task in waiting {
        if qa::environment_busy(workspace, &task.id) {
            continue;
        }
        let run = task.qa.clone().expect("filtered above");
        let agent = workspace.project(&task.project_id)?.qa_agent;
        match agent {
            Some(agent) if qa::checks_passed(&run) => {
                change(workspace, &task.id, |task| {
                    if let Some(run) = task.qa.as_mut() {
                        run.status = QaStatus::Testing;
                    }
                })?;
                let skills = context.skills(&task.project_id, agent, Role::Qa);
                let session = qa::start_agent(
                    workspace,
                    &stage::qa_dir(workspace),
                    &skills,
                    &task.id,
                    agent,
                )?;
                actions.push(Action::Start {
                    task: task.id.clone(),
                    session,
                });
            }
            _ => {
                // Holds the environment until the window runs the checks.
                change(workspace, &task.id, |task| {
                    if let Some(run) = task.qa.as_mut() {
                        run.status = QaStatus::Checking;
                    }
                })?;
                actions.push(Action::Test {
                    task: task.id.clone(),
                });
            }
        }
    }
    Ok(actions)
}

/// Looks for the completion files of every stage under way and acts on
/// those that have appeared. Cheap enough to call every few seconds: one
/// file check per active stage.
pub fn poll(workspace: &mut Workspace, context: &Context<'_>) -> Result<Vec<Action>> {
    let tasks = workspace.state().tasks.clone();
    let mut actions = Vec::new();
    for task in &tasks {
        if let Some(run) = &task.stage_run {
            if let Some(StageResult::Build { summary, verified }) =
                stage::read(Stage::Build, &stage::file(workspace, Stage::Build, run))
            {
                actions.extend(hibernate(context, task.session_id.as_ref()));
                actions.extend(build_done(
                    workspace,
                    context,
                    &task.id,
                    Some((summary, verified)),
                )?);
                continue;
            }
        }
        if let Some(run) = task
            .review
            .as_ref()
            .filter(|run| run.status == ReviewStatus::Reviewing)
        {
            if let Some(StageResult::Verdict { passed, findings }) = stage::read(
                Stage::Review,
                &stage::file(workspace, Stage::Review, &run.id),
            ) {
                actions.extend(hibernate(context, run.session_id.as_ref()));
                actions.extend(review_verdict(
                    workspace,
                    context,
                    &task.id,
                    Some((passed, findings)),
                )?);
                continue;
            }
        }
        if let Some(run) = task
            .qa
            .as_ref()
            .filter(|run| run.status == QaStatus::Testing && run.session_id.is_some())
        {
            if let Some(StageResult::Verdict { passed, findings }) =
                stage::read(Stage::Qa, &stage::file(workspace, Stage::Qa, &run.id))
            {
                actions.extend(hibernate(context, run.session_id.as_ref()));
                actions.extend(qa_verdict(
                    workspace,
                    context,
                    task,
                    Some((passed, findings)),
                )?);
                continue;
            }
        }
        if let Some(run) = task
            .plan
            .as_ref()
            .filter(|run| run.status == crate::plan::PlanStatus::Drafting)
        {
            // A Claude planner presents its plan in plan mode; the others
            // write it to their file.
            let presented = run.session_id.as_deref().and_then(|planner| {
                crate::plan::presented(workspace, planner, run.asked_at.as_deref().unwrap_or(""))
            });
            if let Some(StageResult::Plan(text)) = presented
                .map(StageResult::Plan)
                .or_else(|| stage::read(Stage::Plan, &stage::file(workspace, Stage::Plan, &run.id)))
            {
                actions.extend(hibernate(context, run.session_id.as_ref()));
                actions.extend(crate::plan::written(workspace, &task.id, &text)?);
                continue;
            }
        }
        if let Some(run) = task
            .publish_run
            .as_ref()
            .filter(|run| run.status == crate::publish::PublishStatus::Writing)
        {
            if let Some(StageResult::Pr { title, body }) =
                stage::read(Stage::Pr, &stage::file(workspace, Stage::Pr, &run.id))
            {
                actions.extend(hibernate(context, run.session_id.as_ref()));
                actions.extend(crate::publish::written(workspace, &task.id, &title, &body)?);
                continue;
            }
        }
        if let Some(run) = task
            .docs_run
            .as_ref()
            .filter(|run| run.status == crate::task_docs::DocsStatus::Writing)
        {
            if let Some(StageResult::Docs(files)) =
                stage::read(Stage::Docs, &stage::file(workspace, Stage::Docs, &run.id))
            {
                actions.extend(hibernate(context, run.session_id.as_ref()));
                actions.extend(crate::task_docs::written(workspace, &task.id, &files)?);
            }
        }
    }
    // Runs that ended above may have freed an environment.
    actions.extend(release_environments(workspace, context)?);
    Ok(actions)
}

/// A reviewer, QA agent, planner or documentation writer ended. Its file is
/// read one last time; a stage still open without one needs attention. The
/// builder's exit goes through [`crate::queue::on_clean_exit`].
pub fn on_exit(
    workspace: &mut Workspace,
    context: &Context<'_>,
    session_id: &str,
) -> Result<Vec<Action>> {
    // Whatever is already there is acted on as if polled.
    let mut actions = poll(workspace, context)?;
    let tasks = workspace.state().tasks.clone();
    let mine = |id: &Option<String>| id.as_deref() == Some(session_id);
    for task in &tasks {
        if task
            .review
            .as_ref()
            .is_some_and(|run| run.status == ReviewStatus::Reviewing && mine(&run.session_id))
        {
            actions.extend(review_verdict(workspace, context, &task.id, None)?);
        } else if task
            .qa
            .as_ref()
            .is_some_and(|run| run.status == QaStatus::Testing && mine(&run.session_id))
        {
            actions.extend(qa_verdict(workspace, context, task, None)?);
        } else if task.plan.as_ref().is_some_and(|run| {
            run.status == crate::plan::PlanStatus::Drafting && mine(&run.session_id)
        }) {
            actions.extend(attention(
                workspace,
                &task.id,
                "The planner ended without writing a plan.",
            )?);
        } else if task.publish_run.as_ref().is_some_and(|run| {
            run.status == crate::publish::PublishStatus::Writing && mine(&run.session_id)
        }) {
            actions.extend(crate::publish::failed(
                workspace,
                &task.id,
                "The pull request's author ended without writing it.",
            )?);
        } else if task.docs_run.as_ref().is_some_and(|run| {
            run.status == crate::task_docs::DocsStatus::Writing && mine(&run.session_id)
        }) {
            actions.extend(crate::task_docs::failed(
                workspace,
                &task.id,
                "The documentation writer ended without a document.",
            )?);
        }
    }
    Ok(actions)
}
