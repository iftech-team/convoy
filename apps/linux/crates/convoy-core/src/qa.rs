//! QA: a task in testing is checked by the project's own commands, then
//! tested end to end by a QA agent, which reports a verdict.
//!
//! A run is planned by [`begin`] (the task moves to `Testing`), its checks
//! are run by the caller off the main thread with [`run_checks`] and
//! recorded with [`record_checks`], and a QA session from [`handoff`] tests
//! what the checks cannot. The agent writes its verdict to a file Convoy
//! names ([`verdict_file`]), which works the same in a terminal or a chat,
//! for Claude Code or Codex; [`on_exit`] reads it when the session ends.
//!
//! A failure sends the task back to `Changes` with the findings, for the
//! builder. A pass leaves it in testing: marking it done stays a person's
//! decision.

use crate::json::{head, tail};
use crate::model::{Agent, Task, TaskStatus};
use crate::process::{ProcessRunner, ProcessSpec};
use crate::workspace::{new_id, NewSession, Workspace};
use crate::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const DEFAULT_TIMEOUT_MINUTES: u32 = 15;
/// How much of a check's output is kept: the end, where the failure is.
const OUTPUT_TAIL: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaStatus {
    /// Running the project's checks.
    Checking,
    /// The QA agent is testing.
    Testing,
    Passed,
    Failed,
    /// The QA agent ended without writing a verdict.
    NoVerdict,
}

/// One of the project's checks, as it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub command: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    pub seconds: u64,
    /// The last few thousand characters it printed.
    #[serde(default)]
    pub output: String,
}

/// A task's latest QA run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QaRun {
    pub id: String,
    pub status: QaStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Check>,
    /// The QA agent's session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// What the QA agent found, pass or fail.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub findings: String,
    pub started: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<String>,
}

/// What a run needs once it has begun.
#[derive(Debug, Clone)]
pub struct Plan {
    pub task_id: String,
    pub run_id: String,
    pub directory: PathBuf,
    pub commands: Vec<String>,
    pub timeout: Duration,
    pub agent: Option<Agent>,
}

/// The commands in a project's test field: one per line, blank lines and
/// `#` comments skipped.
pub fn commands(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Whether a project has anything to test with.
pub fn configured(project: &crate::model::Project) -> bool {
    project.qa_agent.is_some()
        || !commands(project.test_commands.as_deref().unwrap_or("")).is_empty()
}

/// Starts a QA run: the task moves to testing and gets a fresh run record.
/// Checks run where its builder worked, the worktree when it had one.
pub fn begin(workspace: &mut Workspace, task_id: &str) -> Result<Plan> {
    let task = find(workspace, task_id)?.clone();
    ensure!(
        task.status != TaskStatus::Building,
        "Stop the task's session before testing it."
    );
    let project = workspace.project(&task.project_id)?.clone();
    ensure!(
        configured(&project),
        "Set up test commands or a QA agent in the project's settings first."
    );
    let commands = commands(project.test_commands.as_deref().unwrap_or(""));
    let directory = match task.session_id.as_deref() {
        Some(session) if workspace.session(session).is_ok() => {
            crate::session::directory_for(workspace, session)?
        }
        _ => project.path.clone(),
    };
    let run_id = new_id();
    let run = QaRun {
        id: run_id.clone(),
        status: if commands.is_empty() {
            QaStatus::Testing
        } else {
            QaStatus::Checking
        },
        checks: Vec::new(),
        session_id: None,
        findings: String::new(),
        started: crate::time::now_iso8601(),
        finished: None,
    };
    let id = task_id.to_string();
    workspace.update(move |state| {
        let task = task_mut(state, &id)?;
        task.status = TaskStatus::Testing;
        task.qa = Some(run);
        Ok(())
    })?;
    Ok(Plan {
        task_id: task_id.to_string(),
        run_id,
        directory,
        commands,
        timeout: Duration::from_secs(
            u64::from(project.qa_timeout.unwrap_or(DEFAULT_TIMEOUT_MINUTES)) * 60,
        ),
        agent: project.qa_agent,
    })
}

/// Runs the checks in order, stopping at the first that fails, and reports
/// each as it ends.
pub fn run_checks(
    plan: &Plan,
    runner: &dyn ProcessRunner,
    mut each: impl FnMut(&Check),
) -> Vec<Check> {
    let mut checks = Vec::new();
    for command in &plan.commands {
        let spec = ProcessSpec::new(
            crate::worktree::shell_program(),
            crate::worktree::shell_arguments(command),
        )
        .cwd(&plan.directory)
        .timeout(plan.timeout)
        .max_output(256 * 1024);
        let started = Instant::now();
        let check = match runner.run(&spec) {
            Ok(output) => Check {
                command: command.clone(),
                ok: output.ok(),
                code: output.status,
                seconds: started.elapsed().as_secs(),
                output: tail(&format!("{}{}", output.stdout, output.stderr), OUTPUT_TAIL)
                    .to_string(),
            },
            Err(error) => Check {
                command: command.clone(),
                ok: false,
                code: None,
                seconds: started.elapsed().as_secs(),
                output: if error.to_string() == "Command timed out." {
                    format!("Stopped after {} minutes.", plan.timeout.as_secs() / 60)
                } else {
                    error.to_string()
                },
            },
        };
        let failed = !check.ok;
        each(&check);
        checks.push(check);
        if failed {
            break;
        }
    }
    checks
}

/// What comes after the checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// They passed; the QA agent tests next.
    Agent(Agent),
    /// They passed and there is no agent: the run passed.
    Passed,
    /// One failed: the task went back to changes.
    Failed(Outcome),
}

/// How a run ended, for the builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub task_id: String,
    /// `None` when the agent left no verdict.
    pub passed: Option<bool>,
    /// The builder session, which gets the findings.
    pub builder: Option<String>,
    pub findings: String,
}

impl Outcome {
    /// What the builder is sent when QA failed.
    pub fn feedback(&self) -> Option<String> {
        (self.passed == Some(false)).then(|| {
            format!(
                "QA found problems with this task. Fix them, verify the fix, and report what you changed.\n\n{}",
                self.findings
            )
        })
    }
}

/// Records the checks of run `run_id`, and moves the task on: back to
/// changes when one failed, on to the agent or passed otherwise.
pub fn record_checks(
    workspace: &mut Workspace,
    task_id: &str,
    run_id: &str,
    checks: Vec<Check>,
) -> Result<Next> {
    let task = find(workspace, task_id)?.clone();
    ensure!(
        task.qa.as_ref().is_some_and(|run| run.id == run_id),
        "A newer QA run replaced this one."
    );
    let agent = workspace.project(&task.project_id)?.qa_agent;
    let failure = checks.iter().find(|check| !check.ok).map(|check| {
        format!(
            "The check `{}` failed{}.\n\n```\n{}\n```",
            check.command,
            check
                .code
                .map(|code| format!(" with exit code {code}"))
                .unwrap_or_default(),
            check.output.trim()
        )
    });
    let id = task_id.to_string();
    let next = match (&failure, agent) {
        (Some(findings), _) => Next::Failed(Outcome {
            task_id: id.clone(),
            passed: Some(false),
            builder: task.session_id.clone(),
            findings: findings.clone(),
        }),
        (None, Some(agent)) => Next::Agent(agent),
        (None, None) => Next::Passed,
    };
    workspace.update(move |state| {
        let task = task_mut(state, &id)?;
        let run = task.qa.as_mut().expect("checked above");
        run.checks = checks;
        match &failure {
            Some(findings) => {
                run.status = QaStatus::Failed;
                run.findings = findings.clone();
                run.finished = Some(crate::time::now_iso8601());
                task.status = TaskStatus::Changes;
                task.findings = head(findings, 16_000).to_string();
            }
            None if agent.is_some() => run.status = QaStatus::Testing,
            None => {
                run.status = QaStatus::Passed;
                run.finished = Some(crate::time::now_iso8601());
            }
        }
        Ok(())
    })?;
    Ok(next)
}

/// Keeps `feedback` for the builder session to receive when it is next
/// resumed: the way QA's findings reach a builder that is not running.
pub fn queue_feedback(workspace: &mut Workspace, builder: &str, feedback: &str) -> Result<()> {
    let (builder, feedback) = (builder.to_string(), feedback.to_string());
    workspace.update(move |state| {
        if let Some(session) = state
            .sessions
            .iter_mut()
            .find(|session| session.id == builder)
        {
            session.resume_prompt = Some(head(&feedback, 16_000).to_string());
        }
        Ok(())
    })?;
    Ok(())
}

/// Where the QA agent of run `run_id` writes its verdict.
pub fn verdict_file(qa_dir: &Path, run_id: &str) -> PathBuf {
    qa_dir.join(format!("{run_id}.json"))
}

/// The QA agent's brief: the task, what it must satisfy, what the checks
/// already showed, and where the verdict goes.
pub fn brief(workspace: &Workspace, task_id: &str, verdict: &Path) -> Result<String> {
    let task = find(workspace, task_id)?;
    let state = workspace.state();
    let spec = task
        .spec_id
        .as_deref()
        .and_then(|id| state.specs.iter().find(|spec| spec.id == id))
        .map(|spec| crate::planning::markdown(spec, &[]))
        .unwrap_or_default();
    let checks = task
        .qa
        .as_ref()
        .map(|run| {
            run.checks
                .iter()
                .map(|check| format!("- `{}` passed in {} s", check.command, check.seconds))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|checks| !checks.is_empty())
        .map(|checks| format!("\n## Checks that already passed\n{checks}\n"))
        .unwrap_or_default();
    let pr = task
        .pr_url
        .as_deref()
        .map(|url| format!("\nPull request: {url}\n"))
        .unwrap_or_default();
    let prompt = format!(
        "You are QA for this task. Test the change end to end against the task and its acceptance criteria: run the project's tests, and if it has a user interface or an API, use it the way a user would. Look for regressions near the change too.\n\
         Do not edit, commit or push anything.\n\n\
         ## Task\n{}\n\n{}\n{pr}{checks}{}\n\
         ## Verdict\n\
         When you are done, write your verdict as JSON to {}:\n\
         {{\"verdict\": \"pass\"}} when everything works, or\n\
         {{\"verdict\": \"fail\", \"findings\": \"1. What fails, how to reproduce it, what you expected.\\n2. …\"}}\n\
         Findings must be concrete and reproducible. Then state the verdict in your last message.",
        task.title,
        task.details,
        if spec.is_empty() {
            String::new()
        } else {
            format!("\n## Specification\n{spec}\n")
        },
        verdict.display(),
    );
    Ok(head(&prompt, 32_000).to_string())
}

/// The QA session: the project's QA agent, in the builder's folder and
/// branch, tied to the task by `qa_of` rather than as its builder, so its
/// exit never moves the task the way a builder's does.
pub fn handoff(
    workspace: &Workspace,
    task_id: &str,
    agent: Agent,
    prompt: String,
) -> Result<NewSession> {
    let task = find(workspace, task_id)?;
    let title = head(&format!("QA: {}", task.title), 200).to_string();
    let mut input = NewSession::new(task.project_id.clone(), agent, title);
    input.prompt = prompt;
    input.qa_of = Some(task_id.to_string());
    input.directory_of = task
        .session_id
        .clone()
        .filter(|session| workspace.session(session).is_ok());
    Ok(input)
}

/// Ties run `run_id` to its QA session.
pub fn record_session(
    workspace: &mut Workspace,
    task_id: &str,
    run_id: &str,
    session_id: &str,
) -> Result<()> {
    let (id, run_id, session_id) = (
        task_id.to_string(),
        run_id.to_string(),
        session_id.to_string(),
    );
    workspace.update(move |state| {
        let task = task_mut(state, &id)?;
        match task.qa.as_mut() {
            Some(run) if run.id == run_id => {
                run.session_id = Some(session_id);
                run.status = QaStatus::Testing;
            }
            _ => bail!("A newer QA run replaced this one."),
        }
        Ok(())
    })?;
    Ok(())
}

#[derive(Deserialize)]
struct Verdict {
    verdict: String,
    #[serde(default)]
    findings: String,
}

/// When a QA session ends: reads its verdict and applies it. `None` for any
/// other session, or one whose run was replaced since.
pub fn on_exit(
    workspace: &mut Workspace,
    qa_dir: &Path,
    session_id: &str,
) -> Result<Option<Outcome>> {
    let Some(task_id) = workspace.session(session_id)?.qa_of.clone() else {
        return Ok(None);
    };
    let Ok(task) = find(workspace, &task_id).cloned() else {
        return Ok(None);
    };
    let Some(run) = task
        .qa
        .clone()
        .filter(|run| run.session_id.as_deref() == Some(session_id))
    else {
        return Ok(None);
    };
    if matches!(run.status, QaStatus::Passed | QaStatus::Failed) {
        return Ok(None);
    }
    let verdict = std::fs::read_to_string(verdict_file(qa_dir, &run.id))
        .ok()
        .and_then(|text| serde_json::from_str::<Verdict>(&text).ok());
    let passed = verdict
        .as_ref()
        .map(|verdict| verdict.verdict.trim().eq_ignore_ascii_case("pass"));
    let findings = verdict
        .map(|verdict| verdict.findings.trim().to_string())
        .unwrap_or_default();
    let outcome = Outcome {
        task_id: task_id.clone(),
        passed,
        builder: task.session_id.clone(),
        findings: if passed == Some(false) && findings.is_empty() {
            "The QA agent failed the task without saying why; see its session.".into()
        } else {
            findings
        },
    };
    let applied = outcome.clone();
    workspace.update(move |state| {
        let task = task_mut(state, &task_id)?;
        let Some(run) = task.qa.as_mut() else {
            return Ok(());
        };
        run.finished = Some(crate::time::now_iso8601());
        run.findings = applied.findings.clone();
        run.status = match applied.passed {
            Some(true) => QaStatus::Passed,
            Some(false) => QaStatus::Failed,
            None => QaStatus::NoVerdict,
        };
        if applied.passed == Some(false) {
            task.status = TaskStatus::Changes;
            task.findings = head(&applied.findings, 16_000).to_string();
        }
        Ok(())
    })?;
    Ok(Some(outcome))
}

/// The commands a project probably tests with, read from its manifests.
pub fn suggest_commands(directory: &Path) -> Vec<String> {
    let has = |name: &str| directory.join(name).exists();
    let mut found = Vec::new();
    if let Ok(text) = std::fs::read_to_string(directory.join("package.json")) {
        let script = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|json| json["scripts"]["test"].as_str().map(str::to_string));
        if script.is_some_and(|script| !script.contains("no test specified")) {
            let runner = if has("pnpm-lock.yaml") {
                "pnpm"
            } else if has("yarn.lock") {
                "yarn"
            } else if has("bun.lockb") || has("bun.lock") {
                "bun"
            } else {
                "npm"
            };
            found.push(format!("{runner} test"));
        }
    }
    if has("Cargo.toml") {
        found.push("cargo test".into());
    }
    if has("artisan") {
        found.push("php artisan test".into());
    } else if has("composer.json") && has("phpunit.xml") {
        found.push("vendor/bin/phpunit".into());
    }
    if has("pubspec.yaml") {
        found.push("flutter test".into());
    }
    if has("go.mod") {
        found.push("go test ./...".into());
    }
    if has("pyproject.toml") || has("pytest.ini") || has("setup.cfg") {
        found.push("pytest".into());
    }
    found
}

fn find<'a>(workspace: &'a Workspace, task_id: &str) -> Result<&'a Task> {
    match workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == task_id)
    {
        Some(task) => Ok(task),
        None => bail!("Task not found."),
    }
}

fn task_mut<'a>(state: &'a mut crate::model::State, task_id: &str) -> Result<&'a mut Task> {
    match state.tasks.iter_mut().find(|task| task.id == task_id) {
        Some(task) => Ok(task),
        None => bail!("Task not found."),
    }
}
