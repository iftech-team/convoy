//! Publishing by Convoy: once review and QA pass, rather than by the builder
//! as it finishes.
//!
//! A project that asks for it tells its builders to commit and stop there.
//! When the task is ready, an author — the task's agent, with the project's
//! instructions for the role, which is where a team's description template
//! goes — writes the pull request's title and description to a file
//! ([`crate::stage`]). Then Convoy pushes the branch and opens the pull or
//! merge request with `gh` or `glab`, on GitHub, GitLab or their self-hosted
//! servers. A task set only to describe keeps the description for a person
//! to open the request with; a task set to push is pushed.
//!
//! The commands run off the window's thread ([`run`]); what they print is
//! read for the link. Any failure leaves the task needing attention, with
//! the reason, and the branch as it was.

use crate::model::{PublishMode, Role, Task, TaskStatus};
use crate::pipeline::{self, Action, Context};
use crate::process::{ProcessRunner, ProcessSpec};
use crate::stage::{self, Stage};
use crate::workspace::{new_id, NewSession, Workspace};
use crate::{bail, ensure, json::head, Git, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishStatus {
    /// The author is writing the description.
    Writing,
    /// Convoy is pushing and opening the request.
    Publishing,
    Published,
    /// The description is written, for a person to open the request with.
    Described,
    Failed,
}

/// A task's latest publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRun {
    pub id: String,
    pub status: PublishStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub started: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// What the builder is told about publishing when Convoy does it.
pub const BUILDER_NOTE: &str = "Commit your work on the current branch with clear messages. Do not push and do not open a pull request: Convoy publishes it after review and QA.";

/// Whether Convoy publishes this task, and has not yet.
pub fn due(workspace: &Workspace, task: &Task) -> bool {
    task.mode != PublishMode::None
        && workspace
            .project(&task.project_id)
            .is_ok_and(|project| project.convoy_publishes == Some(true))
        && !task.publish_run.as_ref().is_some_and(|run| {
            matches!(
                run.status,
                PublishStatus::Writing
                    | PublishStatus::Publishing
                    | PublishStatus::Published
                    | PublishStatus::Described
            )
        })
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

/// The author's brief: everything the description is written from.
fn brief(workspace: &Workspace, task: &Task, file: &std::path::Path) -> Result<String> {
    let project = workspace.project(&task.project_id)?;
    let instructions = crate::roles::brief(workspace.settings(), project, Role::Pr);
    let issue = task
        .source
        .as_ref()
        .map(|source| {
            format!(
                "\nIssue: {} {}{}\n",
                source.tracker.label(),
                source.key,
                source
                    .url
                    .as_deref()
                    .map(|url| format!(" — {url}"))
                    .unwrap_or_default()
            )
        })
        .unwrap_or_default();
    let mut verdicts = String::new();
    if let Some(review) = &task.review {
        verdicts.push_str(&format!("\nReview: {:?}", review.status));
    }
    if let Some(qa) = &task.qa {
        verdicts.push_str(&format!("\nQA: {:?}", qa.status));
        for check in &qa.checks {
            verdicts.push_str(&format!(
                "\n- `{}` {}",
                check.command,
                if check.ok { "passed" } else { "failed" }
            ));
        }
    }
    let prompt = format!(
        "{instructions}\n\nRead the branch's commits and diff against its base with git to see what changed. Do not change, commit or push anything.\n\n## Task\n{}\n\n{}\n{issue}{}\n## Verification{verdicts}\n\n{}",
        task.title,
        task.details,
        pipeline::task_context(task),
        stage::closing(Stage::Pr, file)
    );
    Ok(head(&prompt, 32_000).to_string())
}

/// Starts publishing a task: the author for a request or a description, the
/// push itself otherwise.
pub fn begin(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    ensure!(
        task.mode != PublishMode::None,
        "This task does not publish."
    );
    let run = new_id();
    if task.mode == PublishMode::Push {
        let record = PublishRun {
            id: run,
            status: PublishStatus::Publishing,
            session_id: None,
            started: crate::time::now_iso8601(),
            finished: None,
            error: String::new(),
        };
        pipeline::change(workspace, task_id, move |task| {
            task.publish_run = Some(record)
        })?;
        return Ok(vec![Action::Publish {
            task: task_id.to_string(),
        }]);
    }
    std::fs::create_dir_all(stage::dir(workspace))?;
    let file = stage::file(workspace, Stage::Pr, &run);
    let mut input = NewSession::new(
        task.project_id.clone(),
        task.agent,
        head(&format!("PR: {}", task.title), 200).to_string(),
    );
    input.prompt = brief(workspace, &task, &file)?;
    input.model = task.model.clone().unwrap_or_default();
    input.effort = task.effort.clone().unwrap_or_default();
    input.profile_id = task.profile_id.clone();
    input.directory_of = task
        .session_id
        .clone()
        .filter(|id| workspace.session(id).is_ok());
    input.skills = context.skills(&task.project_id, task.agent, Role::Pr);
    let session = workspace
        .add_session(input)?
        .sessions
        .last()
        .map(|session| session.id.clone())
        .unwrap_or_default();
    let record = PublishRun {
        id: run,
        status: PublishStatus::Writing,
        session_id: Some(session.clone()),
        started: crate::time::now_iso8601(),
        finished: None,
        error: String::new(),
    };
    pipeline::change(workspace, task_id, move |task| {
        task.publish_run = Some(record)
    })?;
    Ok(vec![Action::Start {
        task: task_id.to_string(),
        session,
    }])
}

/// The author wrote the title and description: a request is opened next, a
/// description is kept for a person.
pub fn written(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    title: &str,
    body: &str,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    let (title, body) = (
        head(title.trim(), 300).to_string(),
        head(body.trim(), 60_000).to_string(),
    );
    if title.is_empty() {
        return failed(
            workspace,
            task_id,
            "The author wrote no title for the pull request.",
        );
    }
    let describe = task.mode == PublishMode::Describe;
    pipeline::change(workspace, task_id, move |task| {
        task.pr_title = Some(title);
        task.pr_body = Some(body);
        if let Some(run) = task.publish_run.as_mut() {
            if describe {
                run.status = PublishStatus::Described;
                run.finished = Some(crate::time::now_iso8601());
            } else {
                run.status = PublishStatus::Publishing;
            }
        }
    })?;
    if !describe {
        return Ok(vec![Action::Publish {
            task: task_id.to_string(),
        }]);
    }
    let mut actions = vec![Action::Notice {
        task: task_id.to_string(),
        event: "described",
        detail: String::new(),
    }];
    actions.extend(after(workspace, context, task_id)?);
    Ok(actions)
}

/// What publishing runs, read while the workspace is held so the commands
/// can run without it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub task_id: String,
    pub run_id: String,
    pub mode: PublishMode,
    pub directory: PathBuf,
    /// `gh` or `glab`, when one is set; otherwise it is read from the remote.
    pub cli: Option<String>,
    /// The project's base, without its remote, as the request's target.
    pub base: Option<String>,
    pub title: String,
    pub body: String,
    /// Where the body is written for `gh`, which reads it from a file.
    pub body_file: PathBuf,
    /// The task's other repositories, each published the same way.
    pub extras: Vec<Target>,
}

/// Another repository to publish in: where the task's work is, and how its
/// project opens requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub title: String,
    pub directory: PathBuf,
    pub cli: Option<String>,
    pub base: Option<String>,
}

/// What publishing opened: this project's request, then the others'.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Published {
    pub url: Option<String>,
    pub extra: Vec<String>,
}

fn base_of(project: &crate::model::Project) -> Option<String> {
    project
        .base_ref
        .as_deref()
        .map(|base| base.strip_prefix("origin/").unwrap_or(base).to_string())
}

pub fn plan(workspace: &Workspace, task_id: &str) -> Result<Plan> {
    let task = find(workspace, task_id)?;
    let project = workspace.project(&task.project_id)?;
    let Some(run) = task.publish_run.clone() else {
        bail!("This task is not being published.")
    };
    ensure!(
        run.status == PublishStatus::Publishing,
        "This task is not being published."
    );
    // The other repositories are published from the worktrees made for
    // the task, never from a shared checkout.
    let mut extras = Vec::new();
    for id in &task.extra_projects {
        let other = workspace.project(id)?;
        let Some(checkout) = task
            .extra_checkouts
            .iter()
            .find(|checkout| &checkout.project_id == id)
        else {
            bail!(
                "{} has no worktree for this task, so it cannot be published. Turn on a worktree per task in its project.",
                other.title
            )
        };
        extras.push(Target {
            title: other.title.clone(),
            directory: checkout.directory.clone(),
            cli: other.pr_cli.clone().filter(|cli| !cli.is_empty()),
            base: base_of(other),
        });
    }
    let directory = match task
        .session_id
        .as_deref()
        .and_then(|id| workspace.session(id).ok())
    {
        Some(session) => crate::worktree::directory_of(workspace, session)?,
        None => project.path.clone(),
    };
    Ok(Plan {
        task_id: task.id.clone(),
        run_id: run.id.clone(),
        mode: task.mode,
        directory,
        cli: project.pr_cli.clone().filter(|cli| !cli.is_empty()),
        base: base_of(project),
        title: task.pr_title.clone().unwrap_or_else(|| task.title.clone()),
        body: task.pr_body.clone().unwrap_or_default(),
        body_file: stage::dir(workspace).join(format!("{}.pr-body.md", run.id)),
        extras,
    })
}

/// Which command line tool opens requests on this remote: `glab` for a
/// GitLab host, `gh` otherwise (GitHub, and GitHub Enterprise via its host).
pub fn cli_for(remote: &str) -> &'static str {
    if remote.to_ascii_lowercase().contains("gitlab") {
        "glab"
    } else {
        "gh"
    }
}

/// A tool from the user's login shell, which is where `gh` and `glab` are
/// on their PATH; the arguments are quoted, never interpreted.
fn tool(directory: &std::path::Path, args: &[String]) -> ProcessSpec {
    let spec = if crate::Platform::current().is_windows() {
        ProcessSpec::new(args[0].clone(), args[1..].to_vec())
    } else {
        let command = args
            .iter()
            .map(|arg| crate::provider::launch::quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        let (program, arguments) = crate::provider::launch::shell_command(command);
        ProcessSpec::new(program, arguments)
    };
    spec.cwd(directory).timeout(Duration::from_secs(180))
}

/// Pushes the branch and opens the request. Returns the request's link, or
/// none for a push. Runs the commands; holds nothing.
pub fn run(plan: &Plan, runner: Arc<dyn ProcessRunner>) -> Result<Published> {
    let main = Target {
        title: String::new(),
        directory: plan.directory.clone(),
        cli: plan.cli.clone(),
        base: plan.base.clone(),
    };
    let url = publish_in(plan, &main, &runner)?;
    let mut extra = Vec::new();
    for target in &plan.extras {
        let opened = publish_in(plan, target, &runner)
            .map_err(|error| crate::ConvoyError::message(format!("{}: {error}", target.title)))?;
        extra.extend(opened);
    }
    Ok(Published { url, extra })
}

/// Pushes one repository's branch and opens its request.
fn publish_in(
    plan: &Plan,
    target: &Target,
    runner: &Arc<dyn ProcessRunner>,
) -> Result<Option<String>> {
    let git = Git::new(runner.clone());
    let directory = &target.directory;
    let dirty = git.run(directory, &["status", "--porcelain"])?;
    ensure!(
        dirty.trim().is_empty(),
        "The builder left uncommitted changes. Commit or discard them, then publish again."
    );
    let branch = git
        .run(directory, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    ensure!(
        !branch.is_empty() && branch != "HEAD",
        "The task's checkout is not on a branch."
    );
    if plan.mode == PublishMode::Pr {
        let base = target.base.as_deref().unwrap_or("");
        ensure!(
            branch != base && !["main", "master", "develop"].contains(&branch.as_str()),
            "A pull request needs a branch of its own, and this task is on {branch}. Turn on a worktree per task in the project's settings."
        );
    }
    git.run(directory, &["push", "-u", "origin", &branch])?;
    if plan.mode != PublishMode::Pr {
        return Ok(None);
    }
    let cli = match &target.cli {
        Some(cli) => cli.clone(),
        None => {
            let remote = git.run(directory, &["remote", "get-url", "origin"])?;
            cli_for(&remote).to_string()
        }
    };
    let mut args: Vec<String> = Vec::new();
    match cli.as_str() {
        "glab" => {
            args.extend(["glab", "mr", "create", "--yes", "--source-branch"].map(String::from));
            args.push(branch.clone());
            args.extend(["--title".into(), plan.title.clone()]);
            args.extend(["--description".into(), plan.body.clone()]);
            if let Some(base) = &target.base {
                args.extend(["--target-branch".into(), base.clone()]);
            }
        }
        "gh" => {
            if let Some(parent) = plan.body_file.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&plan.body_file, &plan.body)?;
            args.extend(["gh", "pr", "create", "--head"].map(String::from));
            args.push(branch.clone());
            args.extend(["--title".into(), plan.title.clone()]);
            args.extend([
                "--body-file".into(),
                plan.body_file.to_string_lossy().into_owned(),
            ]);
            if let Some(base) = &target.base {
                args.extend(["--base".into(), base.clone()]);
            }
        }
        other => bail!("Unknown pull request tool: {other}. Use gh or glab."),
    }
    let output = runner.run(&tool(directory, &args))?;
    let printed = format!("{}\n{}", output.stdout, output.stderr);
    ensure!(
        output.ok(),
        "{cli} could not open the request: {}",
        head(printed.trim(), 2_000)
    );
    match crate::session::pull_request_url(&printed) {
        Some(url) => Ok(Some(url)),
        None => bail!(
            "{cli} printed no link to the request: {}",
            head(printed.trim(), 2_000)
        ),
    }
}

/// Records how publishing went, and moves on to the document.
pub fn finish(
    workspace: &mut Workspace,
    context: &Context<'_>,
    task_id: &str,
    run_id: &str,
    outcome: std::result::Result<Published, String>,
) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    if !task
        .publish_run
        .as_ref()
        .is_some_and(|run| run.id == run_id && run.status == PublishStatus::Publishing)
    {
        return Ok(Vec::new());
    }
    let published = match outcome {
        Ok(published) => published,
        Err(error) => return failed(workspace, task_id, &error),
    };
    let url = published.url.clone();
    pipeline::change(workspace, task_id, move |task| {
        if let Some(run) = task.publish_run.as_mut() {
            run.status = PublishStatus::Published;
            run.finished = Some(crate::time::now_iso8601());
        }
        task.extra_pr_urls = published.extra;
        if let Some(url) = published.url {
            task.pr_url = Some(url);
            task.status = TaskStatus::Pr;
        }
    })?;
    let mut actions = vec![Action::Notice {
        task: task_id.to_string(),
        event: "published",
        detail: url.unwrap_or_default(),
    }];
    actions.extend(after(workspace, context, task_id)?);
    Ok(actions)
}

/// Publishing failed: the task needs attention; its code and branch stay.
pub fn failed(workspace: &mut Workspace, task_id: &str, reason: &str) -> Result<Vec<Action>> {
    let reason = head(reason, 4_000).to_string();
    let kept = reason.clone();
    pipeline::change(workspace, task_id, move |task| {
        if let Some(run) = task.publish_run.as_mut() {
            run.status = PublishStatus::Failed;
            run.error = kept;
            run.finished = Some(crate::time::now_iso8601());
        }
    })?;
    pipeline::attention(workspace, task_id, &format!("Not published. {reason}"))
}

/// After publishing: the document, when the project writes one by itself.
fn after(workspace: &mut Workspace, context: &Context<'_>, task_id: &str) -> Result<Vec<Action>> {
    let task = find(workspace, task_id)?;
    if workspace.project(&task.project_id)?.docs_auto == Some(true) {
        return crate::task_docs::begin(workspace, context, task_id);
    }
    Ok(Vec::new())
}
