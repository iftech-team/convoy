//! Phase 3, scale: one QA at a time in a shared environment, the tracker
//! read on a schedule, issues imported whole, and tasks over several
//! repositories.

mod common;

use common::fixture;
use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::pipeline::{self, Action, Context};
use convoy_core::planning::TaskInput;
use convoy_core::qa::{self, QaStatus};
use convoy_core::workspace::{ProjectPatch, Workspace};

fn idle(_: &str) -> bool {
    false
}

fn context() -> Context<'static> {
    Context {
        skills: None,
        running: &idle,
    }
}

fn add_task(workspace: &mut Workspace, project: &str, title: &str) -> String {
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.to_string(),
            spec_id: None,
            title: title.into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::None,
            auto_review: false,
            skills: None,
        })
        .unwrap();
    workspace.state().tasks.last().unwrap().id.clone()
}

fn task(workspace: &Workspace, id: &str) -> convoy_core::model::Task {
    workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.id == id)
        .unwrap()
        .clone()
}

#[test]
fn a_shared_environment_tests_one_task_at_a_time() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                test_commands: Some("php artisan test".into()),
                qa_agent: Some("codex".into()),
                qa_environment: Some("eskhata-local".into()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    let first = add_task(&mut workspace, &project, "First");
    let second = add_task(&mut workspace, &project, "Second");
    for id in [&first, &second] {
        workspace.prepare_task(id, None).unwrap();
    }
    workspace
        .update(|state| {
            for task in state.tasks.iter_mut() {
                task.status = TaskStatus::Review;
            }
            Ok(())
        })
        .unwrap();

    // The first runs its checks; the second waits for the environment.
    qa::begin(&mut workspace, &first).unwrap();
    assert!(qa::environment_busy(&workspace, &second));
    assert!(
        !qa::environment_busy(&workspace, &first),
        "a run never waits for itself"
    );
    qa::enqueue(&mut workspace, &second).unwrap();
    assert_eq!(
        task(&workspace, &second).qa.unwrap().status,
        QaStatus::Queued
    );
    assert!(pipeline::poll(&mut workspace, &context())
        .unwrap()
        .is_empty());

    // The first's checks pass and its agent tests: still busy.
    let run = task(&workspace, &first).qa.unwrap().id;
    let next = qa::record_checks(
        &mut workspace,
        &first,
        &run,
        vec![qa::Check {
            command: "php artisan test".into(),
            ok: true,
            code: Some(0),
            seconds: 9,
            output: String::new(),
        }],
    )
    .unwrap();
    pipeline::after_checks(&mut workspace, &context(), next, &first).unwrap();
    assert_eq!(
        task(&workspace, &first).qa.unwrap().status,
        QaStatus::Testing
    );
    assert!(pipeline::poll(&mut workspace, &context())
        .unwrap()
        .is_empty());

    // It passes: the second's checks start, holding the environment.
    let agent = task(&workspace, &first).qa.unwrap().session_id.unwrap();
    let verdict = convoy_core::stage::file(
        &workspace,
        convoy_core::stage::Stage::Qa,
        &task(&workspace, &first).qa.unwrap().id,
    );
    std::fs::create_dir_all(verdict.parent().unwrap()).unwrap();
    std::fs::write(&verdict, r#"{"verdict": "pass"}"#).unwrap();
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    assert!(actions.contains(&Action::Test {
        task: second.clone()
    }));
    assert_eq!(
        task(&workspace, &second).qa.unwrap().status,
        QaStatus::Checking
    );
    assert!(pipeline::on_exit(&mut workspace, &context(), &agent)
        .unwrap()
        .is_empty());

    // Another project with no shared environment never waits.
    let other = fixture.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    workspace.add_project(&other).unwrap();
    let elsewhere = workspace.state().projects.last().unwrap().id.clone();
    let free = add_task(&mut workspace, &elsewhere, "Elsewhere");
    assert!(!qa::environment_busy(&workspace, &free));
}

#[test]
fn an_agent_waits_for_the_environment_after_its_review() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                qa_agent: Some("codex".into()),
                qa_environment: Some("db".into()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    let busy = add_task(&mut workspace, &project, "Busy");
    let waiting = add_task(&mut workspace, &project, "Waiting");
    workspace.prepare_task(&waiting, None).unwrap();
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Review;
            state.tasks[1].status = TaskStatus::Review;
            Ok(())
        })
        .unwrap();
    qa::begin(&mut workspace, &busy).unwrap();

    let actions = pipeline::handle(
        &mut workspace,
        &context(),
        &waiting,
        pipeline::Event::ChecksPassed,
    )
    .unwrap();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Notice {
            event: "qa_queued",
            ..
        }
    )));
    assert_eq!(
        task(&workspace, &waiting).qa.unwrap().status,
        QaStatus::Queued
    );
}

#[test]
fn intake_searches_on_its_schedule_and_remembers_how_it_went() {
    use convoy_core::integrations::{Intake, ProjectLink, ProjectLinks};
    let mut intake = Intake {
        enabled: true,
        minutes: Some(1),
        ..Intake::default()
    };
    assert_eq!(intake.interval(), 5 * 60, "five minutes at the least");
    assert!(intake.due(1_000), "never searched: due now");
    intake.last_run = Some(1_000);
    assert!(!intake.due(1_000 + 299));
    assert!(intake.due(1_000 + 300));
    intake.enabled = false;
    assert!(!intake.due(10_000));

    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("integration-links.json");
    let mut links = ProjectLinks::load(&file).unwrap();
    links
        .set(
            "p1",
            Some(ProjectLink {
                connection_id: "jira".into(),
                workspace: None,
                project: None,
                sync: None,
                intake: Some(Intake {
                    enabled: true,
                    query: "status = \"To Do\"".into(),
                    run: true,
                    ..Intake::default()
                }),
            }),
        )
        .unwrap();
    links.record_intake("p1", 42, "Imported 3.").unwrap();
    // Unlinked projects are left alone.
    links.record_intake("p2", 42, "nothing").unwrap();
    let reread = ProjectLinks::load(&file).unwrap();
    let kept = reread.get("p1").unwrap().intake.clone().unwrap();
    assert_eq!(kept.last_run, Some(42));
    assert_eq!(kept.last_result.as_deref(), Some("Imported 3."));
    assert!(kept.run);
    assert_eq!(reread.all().len(), 1);
}

#[test]
fn attachments_are_downloaded_only_from_the_tracker_and_reach_the_brief() {
    use convoy_core::integrations::{
        download_request, linear_uploads, safe_file_name, Connection, IssueAttachment, TrackerAuth,
        TrackerKind,
    };
    let jira = Connection {
        id: "j".into(),
        kind: TrackerKind::Jira,
        auth: TrackerAuth::ApiKey,
        name: "Jira".into(),
        site: Some("https://jira.eskhata.com".into()),
        username: Some("me".into()),
        secret: "token".into(),
    };
    let own = download_request(
        &jira,
        "https://jira.eskhata.com/secure/attachment/1/screen.png",
    )
    .unwrap();
    assert!(own.header("Authorization").unwrap().starts_with("Basic "));
    for elsewhere in [
        "https://evil.example/jira.eskhata.com/a.png",
        "https://jira.eskhata.com.evil.example/a.png",
        "http://jira.eskhata.com/a.png",
    ] {
        assert!(download_request(&jira, elsewhere).is_err(), "{elsewhere}");
    }
    let linear = Connection {
        kind: TrackerKind::Linear,
        site: None,
        username: None,
        ..jira.clone()
    };
    assert!(download_request(&linear, "https://uploads.linear.app/a/b/shot.png").is_ok());
    assert!(download_request(&linear, "https://linear.app/a").is_err());

    let file = |name: &str, mime: Option<&str>, size: Option<u64>| IssueAttachment {
        name: name.into(),
        url: String::new(),
        mime: mime.map(str::to_string),
        size,
    };
    assert!(file("screen.png", Some("image/png"), Some(1_000)).downloadable());
    assert!(file("spec.pdf", None, None).downloadable());
    assert!(!file("build.zip", Some("application/zip"), Some(10)).downloadable());
    assert!(!file("huge.png", Some("image/png"), Some(50 * 1024 * 1024)).downloadable());

    let uploads = linear_uploads(
        "See ![shot](https://uploads.linear.app/ws/abc/shot.png) and https://uploads.linear.app/ws/abc/shot.png again",
    );
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0].name, "shot.png");
    assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
    assert_eq!(safe_file_name(".env"), "env");

    // The builder is told where they are.
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let id = add_task(&mut workspace, &project, "With a screenshot");
    workspace
        .set_task_attachments(
            &id,
            vec![convoy_core::model::TaskAttachment {
                name: "screen.png".into(),
                path: "/data/attachments/t/screen.png".into(),
            }],
        )
        .unwrap();
    workspace.prepare_task(&id, None).unwrap();
    let builder = task(&workspace, &id).session_id.unwrap();
    assert!(
        workspace
            .session(&builder)
            .unwrap()
            .prompt
            .contains("## Attached files\n")
            && workspace
                .session(&builder)
                .unwrap()
                .prompt
                .contains("- screen.png: /data/attachments/t/screen.png")
    );
}
