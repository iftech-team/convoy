//! QA: the project's checks, the QA agent's brief and session, its verdict,
//! and what each outcome does to the task.

mod common;

use common::fixture;
use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::pipeline::{Action, Context};
use convoy_core::planning::TaskInput;
use convoy_core::process::{Output, ProcessRunner, ProcessSpec};
use convoy_core::qa::{self, Check, Next, QaStatus};
use convoy_core::queue::{self, Completion};
use convoy_core::workspace::{ProjectPatch, Workspace};
use convoy_core::Result;

/// Answers a check by its command: "fail" exits 1, "slow" times out,
/// anything else passes.
struct Runner;

impl ProcessRunner for Runner {
    fn run(&self, spec: &ProcessSpec) -> Result<Output> {
        let command = spec.args.last().cloned().unwrap_or_default();
        if command.contains("slow") {
            return Err(convoy_core::ConvoyError::message("Command timed out."));
        }
        let failing = command.contains("fail");
        Ok(Output {
            status: Some(if failing { 1 } else { 0 }),
            stdout: format!("ran {command}\n"),
            stderr: if failing {
                "1 test failed: login keeps the old token\n".into()
            } else {
                String::new()
            },
        })
    }
}

/// A project with QA set up, a task in it, and a prepared builder session.
fn setup(commands: &str, agent: Option<&str>) -> (common::Fixture, Workspace, String, String) {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                test_commands: Some(commands.into()),
                qa_agent: Some(agent.unwrap_or("").into()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.clone(),
            spec_id: None,
            title: "Refresh the token on login".into(),
            details: "Users stay signed in after the token expires.".into(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::Pr,
            auto_review: false,
            skills: None,
        })
        .unwrap();
    let task = workspace.state().tasks[0].id.clone();
    workspace.prepare_task(&task, None).unwrap();
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Pr;
            Ok(())
        })
        .unwrap();
    (fixture, workspace, project, task)
}

fn task(workspace: &Workspace) -> convoy_core::model::Task {
    workspace.state().tasks[0].clone()
}

#[test]
fn test_commands_are_read_line_by_line_and_suggested_from_manifests() {
    assert_eq!(
        qa::commands("npm test\n\n# slow ones\ncargo test --all\n"),
        ["npm test", "cargo test --all"]
    );

    let folder = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| std::fs::write(folder.path().join(name), text).unwrap();
    write("package.json", r#"{"scripts":{"test":"vitest run"}}"#);
    write("pnpm-lock.yaml", "");
    write("Cargo.toml", "[package]");
    write("artisan", "");
    write("go.mod", "module x");
    assert_eq!(
        qa::suggest_commands(folder.path()),
        [
            "pnpm test",
            "cargo test",
            "php artisan test",
            "go test ./..."
        ]
    );
    // npm's placeholder script is no test at all.
    let empty = tempfile::tempdir().unwrap();
    std::fs::write(
        empty.path().join("package.json"),
        r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#,
    )
    .unwrap();
    assert!(qa::suggest_commands(empty.path()).is_empty());
}

#[test]
fn a_run_moves_the_task_to_testing_only_when_qa_is_set_up() {
    let (_fixture, mut bare, _, id) = setup("", None);
    let error = qa::begin(&mut bare, &id).unwrap_err();
    assert!(error
        .to_string()
        .contains("Set up test commands or a QA agent"));

    let (_fixture, mut workspace, _, id) = setup("npm test", None);
    let plan = qa::begin(&mut workspace, &id).unwrap();
    assert_eq!(plan.commands, ["npm test"]);
    assert_eq!(plan.timeout.as_secs(), 15 * 60);
    let task = task(&workspace);
    assert_eq!(task.status, TaskStatus::Testing);
    assert_eq!(task.qa.unwrap().status, QaStatus::Checking);
}

#[test]
fn a_failing_check_stops_the_run_and_sends_the_task_back_with_its_output() {
    let (_fixture, mut workspace, _, id) =
        setup("npm run lint\nnpm run fail\nnpm run e2e", Some("codex"));
    let plan = qa::begin(&mut workspace, &id).unwrap();
    let mut seen = Vec::new();
    let checks = qa::run_checks(&plan, &Runner, |check: &Check| {
        seen.push(check.command.clone())
    });
    assert_eq!(
        seen,
        ["npm run lint", "npm run fail"],
        "the run stops at the first failure"
    );
    assert!(checks[0].ok && !checks[1].ok);

    let next = qa::record_checks(&mut workspace, &id, &plan.run_id, checks).unwrap();
    let Next::Failed(outcome) = next else {
        panic!("expected a failure, got {next:?}")
    };
    let task = task(&workspace);
    assert_eq!(outcome.builder, task.session_id);
    assert_eq!(task.status, TaskStatus::Changes);
    assert!(task
        .findings
        .contains("`npm run fail` failed with exit code 1"));
    assert!(task.findings.contains("login keeps the old token"));
    assert!(outcome.feedback().unwrap().starts_with("QA found problems"));
    assert_eq!(task.qa.unwrap().status, QaStatus::Failed);

    // A builder that is not running gets the findings when it is resumed,
    // once.
    let builder = outcome.builder.clone().unwrap();
    qa::queue_feedback(&mut workspace, &builder, &outcome.feedback().unwrap()).unwrap();
    let session = workspace.session(&builder).unwrap().clone();
    let resumed = convoy_core::provider::launch::agent_args(&session, true);
    assert_eq!(resumed[resumed.len() - 2], "--");
    assert!(resumed.last().unwrap().starts_with("QA found problems"));
    let fresh = convoy_core::provider::launch::agent_args(&session, false);
    assert!(
        !fresh.iter().any(|arg| arg.starts_with("QA found problems")),
        "a first launch sends its brief, not this"
    );
}

#[test]
fn a_check_that_runs_too_long_fails_with_the_limit() {
    let (_fixture, mut workspace, _, id) = setup("npm run slow", None);
    let plan = qa::begin(&mut workspace, &id).unwrap();
    let checks = qa::run_checks(&plan, &Runner, |_| {});
    assert_eq!(checks[0].output, "Stopped after 15 minutes.");
    assert!(!checks[0].ok);
}

#[test]
fn passing_checks_hand_over_to_the_agent_or_pass_the_run() {
    let (_fixture, mut workspace, _, id) = setup("npm test", None);
    let plan = qa::begin(&mut workspace, &id).unwrap();
    let checks = qa::run_checks(&plan, &Runner, |_| {});
    assert_eq!(
        qa::record_checks(&mut workspace, &id, &plan.run_id, checks).unwrap(),
        Next::Passed
    );
    let after = task(&workspace);
    assert_eq!(
        after.status,
        TaskStatus::Testing,
        "a pass leaves done to a person"
    );
    assert_eq!(after.qa.unwrap().status, QaStatus::Passed);

    let (_fixture, mut workspace, _, id) = setup("npm test", Some("codex"));
    let plan = qa::begin(&mut workspace, &id).unwrap();
    let checks = qa::run_checks(&plan, &Runner, |_| {});
    assert_eq!(
        qa::record_checks(&mut workspace, &id, &plan.run_id, checks).unwrap(),
        Next::Agent(Agent::Codex)
    );
    assert_eq!(task(&workspace).qa.unwrap().status, QaStatus::Testing);
}

#[test]
fn the_qa_agent_tests_in_the_builders_folder_and_its_verdict_decides() {
    let (fixture, mut workspace, _, id) = setup("", Some("codex"));
    let verdicts = fixture.path().join("qa");
    std::fs::create_dir_all(&verdicts).unwrap();
    let builder = task(&workspace).session_id.unwrap();

    let run = |workspace: &mut Workspace, verdict: Option<&str>| {
        let plan = qa::begin(workspace, &id).unwrap();
        assert!(plan.commands.is_empty());
        let file = qa::verdict_file(&verdicts, &plan.run_id);
        let brief = qa::brief(workspace, &id, &file).unwrap();
        assert!(brief.contains("Users stay signed in after the token expires."));
        assert!(brief.contains(&file.display().to_string()));
        assert!(brief.contains("Do not edit, commit or push anything."));
        let handoff = qa::handoff(workspace, &id, Agent::Codex, brief).unwrap();
        assert_eq!(handoff.qa_of.as_deref(), Some(id.as_str()));
        assert_eq!(handoff.directory_of.as_deref(), Some(builder.as_str()));
        workspace.add_session(handoff).unwrap();
        let session = workspace.state().sessions.last().unwrap().clone();
        assert_eq!(session.review_of, None, "QA is not a review of the builder");
        assert_eq!(
            session.task_id, None,
            "its exit must not move the task like a builder's"
        );
        qa::record_session(workspace, &id, &plan.run_id, &session.id).unwrap();
        if let Some(verdict) = verdict {
            std::fs::write(&file, verdict).unwrap();
        }
        qa::on_exit(workspace, &verdicts, &session.id)
            .unwrap()
            .unwrap()
    };

    let passed = run(&mut workspace, Some(r#"{"verdict":"pass"}"#));
    assert_eq!(passed.passed, Some(true));
    assert_eq!(passed.feedback(), None);
    assert_eq!(task(&workspace).status, TaskStatus::Testing);
    assert_eq!(task(&workspace).qa.unwrap().status, QaStatus::Passed);

    let failed = run(
        &mut workspace,
        Some(r#"{"verdict":"fail","findings":"1. Logging out keeps the session."}"#),
    );
    assert_eq!(failed.passed, Some(false));
    assert_eq!(failed.builder.as_deref(), Some(builder.as_str()));
    assert!(failed
        .feedback()
        .unwrap()
        .contains("1. Logging out keeps the session."));
    assert_eq!(task(&workspace).status, TaskStatus::Changes);
    assert_eq!(
        task(&workspace).findings,
        "1. Logging out keeps the session."
    );

    let silent = run(&mut workspace, None);
    assert_eq!(silent.passed, None);
    assert_eq!(task(&workspace).qa.unwrap().status, QaStatus::NoVerdict);

    // Any other session's exit is none of QA's business.
    assert_eq!(
        qa::on_exit(&mut workspace, &verdicts, &builder).unwrap(),
        None
    );
}

#[test]
fn a_finished_builder_starts_qa_only_where_the_project_asks_for_it() {
    let (_fixture, mut workspace, project, id) = setup("npm test", None);
    let builder = task(&workspace).session_id.unwrap();
    let none = |_: &str| false;
    let context = Context {
        skills: None,
        running: &none,
    };
    // Nothing automatic: the round ends, and the task waits for a person.
    assert_eq!(
        queue::on_clean_exit(&mut workspace, &context, &builder).unwrap(),
        Completion::Advance(vec![Action::Built {
            task: id.clone(),
            project: project.clone()
        }])
    );

    workspace
        .edit_project(
            &project,
            ProjectPatch {
                qa_auto: Some(true),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    // A new round: QA tests it.
    workspace
        .update(|state| {
            state.tasks[0].stage_run = Some("round-2".into());
            Ok(())
        })
        .unwrap();
    assert_eq!(
        queue::on_clean_exit(&mut workspace, &context, &builder).unwrap(),
        Completion::Advance(vec![
            Action::Built {
                task: id.clone(),
                project: project.clone()
            },
            Action::Test { task: id.clone() }
        ])
    );
}
