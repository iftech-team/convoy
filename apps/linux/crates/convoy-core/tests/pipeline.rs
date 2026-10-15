//! The task pipeline: completion files, roles, the order of review and QA,
//! rounds of fixes, plans first, and the document at the end.

mod common;

use common::fixture;
use convoy_core::model::{Agent, PublishMode, Role, TaskStatus};
use convoy_core::pipeline::{self, Action, Context, Event, ReviewStatus, Step};
use convoy_core::plan::{self, PlanStatus};
use convoy_core::planning::TaskInput;
use convoy_core::qa::{self, QaStatus};
use convoy_core::skills::{SkillInput, Store};
use convoy_core::stage::{self, Stage, StageResult};
use convoy_core::task_docs::{self, DocsStatus};
use convoy_core::workspace::{ProjectPatch, SettingsPatch, Workspace};
use std::collections::BTreeMap;

fn idle(_: &str) -> bool {
    false
}

fn busy(_: &str) -> bool {
    true
}

fn context() -> Context<'static> {
    Context {
        skills: None,
        running: &idle,
    }
}

/// A project, a task with automatic review, and its prepared builder.
fn setup(patch: ProjectPatch) -> (common::Fixture, Workspace, String, String) {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace.edit_project(&project, patch).unwrap();
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.clone(),
            spec_id: None,
            title: "Refresh the token on login".into(),
            details: "Users stay signed in after the token expires.".into(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::None,
            auto_review: true,
            skills: None,
        })
        .unwrap();
    let task = workspace.state().tasks[0].id.clone();
    (fixture, workspace, project, task)
}

fn task(workspace: &Workspace) -> convoy_core::model::Task {
    workspace.state().tasks[0].clone()
}

fn started(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .filter_map(|action| match action {
            Action::Start { session, .. } => Some(session.clone()),
            _ => None,
        })
        .collect()
}

fn write(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn stage_files_are_read_only_once_they_are_whole() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("run.verdict.json");
    assert_eq!(stage::read(Stage::Review, &path), None, "missing");
    write(&path, "");
    assert_eq!(stage::read(Stage::Review, &path), None, "empty");
    write(&path, "{\"verdict\": \"pa");
    assert_eq!(stage::read(Stage::Review, &path), None, "half written");
    write(&path, "{\"verdict\": \"maybe\"}");
    assert_eq!(stage::read(Stage::Review, &path), None, "no verdict");
    write(
        &path,
        "{\"verdict\": \"FAIL\", \"findings\": \" 1. It leaks. \"}",
    );
    assert_eq!(
        stage::read(Stage::Review, &path),
        Some(StageResult::Verdict {
            passed: false,
            findings: "1. It leaks.".into()
        })
    );

    let done = folder.path().join("run.done.json");
    write(
        &done,
        "{\"summary\": \"Refreshed it\", \"verified\": \"cargo test\"}",
    );
    assert_eq!(
        stage::read(Stage::Build, &done),
        Some(StageResult::Build {
            summary: "Refreshed it".into(),
            verified: "cargo test".into()
        })
    );

    let plan = folder.path().join("run.plan.md");
    write(&plan, "\n# Plan\n1. Do it\n");
    assert_eq!(
        stage::read(Stage::Plan, &plan),
        Some(StageResult::Plan("# Plan\n1. Do it".into()))
    );

    let docs = folder.path().join("run.docs.json");
    write(&docs, "{\"files\": [\"/tmp/a.pdf\"]}");
    assert_eq!(
        stage::read(Stage::Docs, &docs),
        Some(StageResult::Docs(vec!["/tmp/a.pdf".into()]))
    );
}

#[test]
fn a_builder_brief_ends_with_where_to_report() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch::default());
    workspace.prepare_task(&id, None).unwrap();
    let run = task(&workspace).stage_run.expect("a run to report on");
    let builder = task(&workspace).session_id.unwrap();
    let prompt = workspace.session(&builder).unwrap().prompt.clone();
    let file = stage::file(&workspace, Stage::Build, &run);
    assert!(prompt.contains(&file.display().to_string()));
    assert!(prompt.contains("never commit it"));
    // Outside the repository, beside the workspace file.
    assert!(file.starts_with(workspace.file().parent().unwrap().join("stages")));
}

#[test]
fn advance_follows_the_order_of_the_stages() {
    let (_fixture, workspace, project, _) = setup(ProjectPatch::default());
    let mut project = workspace.project(&project).unwrap().clone();
    let mut task = task(&workspace);

    // Nothing automatic but the review.
    assert_eq!(
        pipeline::advance(&task, &project, &Event::BuildDone),
        Step::Review
    );
    task.auto_review = false;
    assert_eq!(
        pipeline::advance(&task, &project, &Event::BuildDone),
        Step::Wait
    );

    // QA set to start by itself: checks first.
    project.qa_auto = Some(true);
    project.test_commands = Some("npm test".into());
    assert_eq!(
        pipeline::advance(&task, &project, &Event::BuildDone),
        Step::Test
    );

    // After the checks: the review, then the QA agent.
    task.auto_review = true;
    assert_eq!(
        pipeline::advance(&task, &project, &Event::ChecksPassed),
        Step::Review
    );
    task.auto_review = false;
    assert_eq!(
        pipeline::advance(&task, &project, &Event::ChecksPassed),
        Step::Ready
    );
    project.qa_agent = Some(Agent::Codex);
    assert_eq!(
        pipeline::advance(&task, &project, &Event::ChecksPassed),
        Step::QaAgent(Agent::Codex)
    );

    // A passing review goes on to the QA agent only when QA waits for it.
    assert_eq!(
        pipeline::advance(&task, &project, &Event::Review(Some(true), String::new())),
        Step::Ready
    );
    task.qa = Some(qa::QaRun {
        id: "run".into(),
        status: QaStatus::Waiting,
        checks: Vec::new(),
        session_id: None,
        findings: String::new(),
        started: String::new(),
        finished: None,
    });
    assert_eq!(
        pipeline::advance(&task, &project, &Event::Review(Some(true), String::new())),
        Step::QaAgent(Agent::Codex)
    );
    assert_eq!(
        pipeline::advance(&task, &project, &Event::Qa(Some(true), String::new())),
        Step::Ready
    );

    // No verdict is never a silent pass.
    assert!(matches!(
        pipeline::advance(&task, &project, &Event::Review(None, String::new())),
        Step::Attention(_)
    ));
    assert!(matches!(
        pipeline::advance(&task, &project, &Event::Qa(None, String::new())),
        Step::Attention(_)
    ));
}

#[test]
fn failures_go_back_to_the_builder_until_the_rounds_run_out() {
    let (_fixture, workspace, project, _) = setup(ProjectPatch::default());
    let mut project = workspace.project(&project).unwrap().clone();
    let mut task = task(&workspace);
    let fail = Event::Review(Some(false), "1. It leaks.".into());

    for round in 0..pipeline::DEFAULT_MAX_FIX_ROUNDS {
        task.round = round;
        assert_eq!(
            pipeline::advance(&task, &project, &fail),
            Step::Fix {
                from: "The review",
                findings: "1. It leaks.".into()
            }
        );
    }
    task.round = pipeline::DEFAULT_MAX_FIX_ROUNDS;
    let Step::Attention(reason) = pipeline::advance(&task, &project, &fail) else {
        panic!("out of rounds");
    };
    assert!(reason.contains("1. It leaks."));
    assert!(matches!(
        pipeline::advance(&task, &project, &Event::ChecksFailed("boom".into())),
        Step::Attention(_)
    ));

    // The project sets its own limit.
    project.max_fix_rounds = Some(5);
    assert!(matches!(
        pipeline::advance(&task, &project, &fail),
        Step::Fix { .. }
    ));
    project.max_fix_rounds = Some(0);
    task.round = 0;
    assert!(matches!(
        pipeline::advance(&task, &project, &fail),
        Step::Attention(_)
    ));
}

#[test]
fn a_review_fail_fixes_and_a_pass_finishes_by_files_alone() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch::default());
    workspace.prepare_task(&id, None).unwrap();
    let builder = task(&workspace).session_id.unwrap();
    let run = task(&workspace).stage_run.unwrap();

    // Nothing is there yet.
    assert!(pipeline::poll(&mut workspace, &context())
        .unwrap()
        .is_empty());

    // The builder reports, and keeps running: it is put to sleep, and a
    // review starts.
    write(
        &stage::file(&workspace, Stage::Build, &run),
        "{\"summary\": \"Refreshed the token\", \"verified\": \"ran the tests\"}",
    );
    let running = Context {
        skills: None,
        running: &busy,
    };
    let actions = pipeline::poll(&mut workspace, &running).unwrap();
    assert!(actions.contains(&Action::Hibernate {
        session: builder.clone()
    }));
    let reviewer = started(&actions).pop().expect("a reviewer");
    assert_eq!(workspace.session(&reviewer).unwrap().agent, Agent::Codex);
    let review = task(&workspace).review.unwrap();
    assert_eq!(review.status, ReviewStatus::Reviewing);
    assert_eq!(task(&workspace).stage_run, None, "the round is handled");
    assert!(task(&workspace)
        .build_summary
        .unwrap()
        .contains("Refreshed the token"));
    let brief = workspace.session(&reviewer).unwrap().prompt.clone();
    assert!(brief.contains("Refreshed the token"));
    assert!(brief.contains(
        &stage::file(&workspace, Stage::Review, &review.id)
            .display()
            .to_string()
    ));
    // Polling again does nothing more.
    assert!(pipeline::poll(&mut workspace, &context())
        .unwrap()
        .is_empty());

    // The reviewer fails it: the findings wait for the builder, which is
    // resumed with them, in round 1.
    write(
        &stage::file(&workspace, Stage::Review, &review.id),
        "{\"verdict\": \"fail\", \"findings\": \"1. The old token is kept.\"}",
    );
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    assert_eq!(started(&actions), std::slice::from_ref(&builder));
    let after = task(&workspace);
    assert_eq!(after.round, 1);
    assert_eq!(after.status, TaskStatus::Changes);
    assert_eq!(after.review.unwrap().status, ReviewStatus::Failed);
    let resume = workspace
        .session(&builder)
        .unwrap()
        .resume_prompt
        .clone()
        .unwrap();
    assert!(resume.contains("1. The old token is kept."));
    let next_run = after.stage_run.unwrap();
    assert!(resume.contains(
        &stage::file(&workspace, Stage::Build, &next_run)
            .display()
            .to_string()
    ));

    // The fixed round is reviewed again, and passes.
    write(
        &stage::file(&workspace, Stage::Build, &next_run),
        "{\"summary\": \"Fixed\"}",
    );
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    let second = started(&actions).pop().expect("a second review");
    assert_ne!(second, reviewer);
    let review = task(&workspace).review.unwrap();
    assert_eq!(review.round, 1);
    assert!(workspace
        .session(&second)
        .unwrap()
        .prompt
        .contains("Your previous findings"));
    write(
        &stage::file(&workspace, Stage::Review, &review.id),
        "{\"verdict\": \"pass\"}",
    );
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    assert!(actions
        .iter()
        .any(|action| matches!(action, Action::Notice { event: "ready", .. })));
    assert_eq!(
        task(&workspace).status,
        TaskStatus::Review,
        "a person marks it done"
    );
}

#[test]
fn a_running_builder_is_sent_its_findings_directly() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch::default());
    workspace.prepare_task(&id, None).unwrap();
    let builder = task(&workspace).session_id.unwrap();
    let running = Context {
        skills: None,
        running: &busy,
    };
    let actions = pipeline::handle(
        &mut workspace,
        &running,
        &id,
        Event::ChecksFailed("The check failed.".into()),
    )
    .unwrap();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Send { session, text, .. } if *session == builder && text.contains("The check failed.")
    )));
    assert_eq!(task(&workspace).status, TaskStatus::Building);
}

#[test]
fn rounds_run_out_into_attention() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch {
        max_fix_rounds: Some(1),
        ..ProjectPatch::default()
    });
    workspace.prepare_task(&id, None).unwrap();
    let fail = || Event::Qa(Some(false), "Still broken.".into());
    pipeline::handle(&mut workspace, &context(), &id, fail()).unwrap();
    assert_eq!(task(&workspace).round, 1);
    let actions = pipeline::handle(&mut workspace, &context(), &id, fail()).unwrap();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Notice {
            event: "attention",
            ..
        }
    )));
    let stuck = task(&workspace);
    assert_eq!(stuck.status, TaskStatus::Attention);
    assert!(stuck.last_error.unwrap().contains("Still broken."));
}

#[test]
fn checks_then_review_then_the_qa_agent() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch {
        test_commands: Some("npm test".into()),
        qa_agent: Some("codex".into()),
        qa_auto: Some(true),
        ..ProjectPatch::default()
    });
    workspace.prepare_task(&id, None).unwrap();
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Review;
            Ok(())
        })
        .unwrap();
    let plan = qa::begin(&mut workspace, &id).unwrap();
    let next = qa::record_checks(
        &mut workspace,
        &id,
        &plan.run_id,
        vec![qa::Check {
            command: "npm test".into(),
            ok: true,
            code: Some(0),
            seconds: 1,
            output: String::new(),
        }],
    )
    .unwrap();
    // The review comes before the QA agent, which waits for it.
    let actions = pipeline::after_checks(&mut workspace, &context(), next, &id).unwrap();
    assert_eq!(started(&actions).len(), 1);
    assert_eq!(task(&workspace).qa.unwrap().status, QaStatus::Waiting);
    assert_eq!(task(&workspace).status, TaskStatus::Testing);

    let review = task(&workspace).review.unwrap();
    write(
        &stage::file(&workspace, Stage::Review, &review.id),
        "{\"verdict\": \"pass\"}",
    );
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    let tester = started(&actions).pop().expect("the QA agent");
    let run = task(&workspace).qa.unwrap();
    assert_eq!(run.status, QaStatus::Testing);
    assert_eq!(run.session_id.as_deref(), Some(tester.as_str()));

    // QA reports by file, while its session is still open.
    write(
        &stage::file(&workspace, Stage::Qa, &run.id),
        "{\"verdict\": \"pass\"}",
    );
    pipeline::poll(&mut workspace, &context()).unwrap();
    assert_eq!(task(&workspace).qa.unwrap().status, QaStatus::Passed);
    assert_eq!(task(&workspace).status, TaskStatus::Testing);
    // Its exit afterwards changes nothing.
    assert!(pipeline::on_exit(&mut workspace, &context(), &tester)
        .unwrap()
        .is_empty());
}

#[test]
fn a_reviewer_that_leaves_no_verdict_needs_attention() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch::default());
    workspace.prepare_task(&id, None).unwrap();
    let actions = pipeline::build_done(&mut workspace, &context(), &id, None).unwrap();
    let reviewer = started(&actions).pop().unwrap();
    pipeline::on_exit(&mut workspace, &context(), &reviewer).unwrap();
    assert_eq!(
        task(&workspace).review.unwrap().status,
        ReviewStatus::NoVerdict
    );
    assert_eq!(task(&workspace).status, TaskStatus::Attention);

    // A person can skip the review and go on.
    pipeline::skip_review(&mut workspace, &context(), &id).unwrap();
    assert_eq!(
        task(&workspace).review.unwrap().status,
        ReviewStatus::Skipped
    );
    assert_eq!(task(&workspace).status, TaskStatus::Review);
}

#[test]
fn role_instructions_come_from_the_project_then_settings_then_built_in() {
    let (_fixture, mut workspace, project, _) = setup(ProjectPatch::default());
    let brief = |workspace: &Workspace, role| {
        convoy_core::roles::brief(
            workspace.settings(),
            workspace.project(&project).unwrap(),
            role,
        )
    };
    assert_eq!(
        brief(&workspace, Role::Review),
        convoy_core::review::DEFAULT_TEMPLATE
    );
    assert_eq!(brief(&workspace, Role::Build), "");

    // The older review template still counts, globally and per project.
    workspace
        .save_settings(SettingsPatch {
            review_template: Some("Global review".into()),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(brief(&workspace, Role::Review), "Global review");
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                review_template: Some("Project review".into()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    assert_eq!(brief(&workspace, Role::Review), "Project review");

    // The role fields win over it.
    workspace
        .save_settings(SettingsPatch {
            role_instructions: Some(BTreeMap::from([(Role::Qa, "Global QA".to_string())])),
            ..SettingsPatch::default()
        })
        .unwrap();
    assert_eq!(brief(&workspace, Role::Qa), "Global QA");
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                role_instructions: Some(BTreeMap::from([
                    (Role::Qa, "Use the eskhata-qa skill".to_string()),
                    (Role::Review, "Strict review".to_string()),
                    (Role::Docs, "  ".to_string()),
                ])),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    assert_eq!(brief(&workspace, Role::Qa), "Use the eskhata-qa skill");
    assert_eq!(brief(&workspace, Role::Review), "Strict review");
    assert_eq!(
        brief(&workspace, Role::Docs),
        convoy_core::roles::DOCS,
        "blank is unset"
    );
}

#[test]
fn skills_reach_the_roles_they_are_for() {
    let (fixture, mut workspace, project, id) = setup(ProjectPatch::default());
    let mut store = Store::load(fixture.path().join("skills.json")).unwrap();
    let mut save = |name: &str, roles: Option<Vec<Role>>, auto: bool| {
        store
            .save(
                SkillInput {
                    id: None,
                    name: name.into(),
                    description: String::new(),
                    body: format!("{name} body"),
                    project_id: Some(project.clone()),
                    agents: "both".into(),
                    auto,
                    roles,
                    projects: None,
                },
                &workspace,
                0,
            )
            .unwrap()
    };
    let builder = save("builder", None, true);
    let qa_too = save("eskhata-qa", Some(vec![Role::Qa, Role::Build]), true);
    let reviewer_skill = save("reviewer", Some(vec![Role::Review]), true);
    save("manual", Some(vec![Role::Review]), false);

    assert_eq!(
        store.defaults(&project, None),
        [builder.clone(), qa_too.clone()]
    );
    assert_eq!(
        store.for_role(&project, Agent::Claude, Role::Qa),
        std::slice::from_ref(&qa_too)
    );
    assert_eq!(
        store.for_role(&project, Agent::Codex, Role::Review),
        std::slice::from_ref(&reviewer_skill)
    );
    assert!(store
        .for_role(&project, Agent::Claude, Role::Docs)
        .is_empty());

    // The reviewer's session starts with the review skills.
    workspace.prepare_task(&id, None).unwrap();
    let with_skills = Context {
        skills: Some(&store),
        running: &idle,
    };
    let actions = pipeline::build_done(&mut workspace, &with_skills, &id, None).unwrap();
    let reviewer = started(&actions).pop().unwrap();
    assert_eq!(
        workspace.session(&reviewer).unwrap().skill_ids(),
        [reviewer_skill]
    );
}

#[test]
fn a_planned_task_is_built_only_after_its_plan_is_approved() {
    let (_fixture, mut workspace, project, id) = setup(ProjectPatch {
        plan_first_default: Some(true),
        ..ProjectPatch::default()
    });
    // New tasks of the project are planned first.
    assert!(task(&workspace).plan_first);
    assert!(plan::needed(&task(&workspace)));
    assert!(workspace
        .prepare_task(&id, None)
        .unwrap_err()
        .to_string()
        .contains("approve its plan"));

    let planner = plan::begin(&mut workspace, &context(), &id).unwrap();
    assert_eq!(task(&workspace).status, TaskStatus::Planning);
    let brief = workspace.session(&planner).unwrap().prompt.clone();
    assert!(brief.contains("Do not change anything in the repository"));
    let draft = task(&workspace).plan.unwrap();
    // Claude plans in its own plan mode and presents the plan.
    assert!(brief.contains("present the whole plan with ExitPlanMode"));
    assert_eq!(
        workspace
            .session(&planner)
            .unwrap()
            .permission_mode
            .as_deref(),
        Some("plan")
    );

    // The plan arrives and waits for a person.
    write(
        &stage::file(&workspace, Stage::Plan, &draft.id),
        "1. Read the token.\n",
    );
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Notice {
            event: "plan_ready",
            ..
        }
    )));
    assert_eq!(task(&workspace).status, TaskStatus::PlanReview);

    // Changes go back to the same planner, in a new draft.
    let actions =
        plan::request_changes(&mut workspace, &context(), &id, "Cover logout too.").unwrap();
    assert_eq!(started(&actions), std::slice::from_ref(&planner));
    let redraft = task(&workspace).plan.unwrap();
    assert_ne!(redraft.id, draft.id);
    assert_eq!(redraft.status, PlanStatus::Drafting);
    assert!(workspace
        .session(&planner)
        .unwrap()
        .resume_prompt
        .clone()
        .unwrap()
        .contains("Cover logout too."));
    write(
        &stage::file(&workspace, Stage::Plan, &redraft.id),
        "1. Read the token.\n2. Cover logout.\n",
    );
    pipeline::poll(&mut workspace, &context()).unwrap();

    // Approved, it is built with the plan in its brief.
    plan::approve(&mut workspace, &id).unwrap();
    assert_eq!(task(&workspace).status, TaskStatus::Queued);
    workspace.prepare_task(&id, None).unwrap();
    let builder = task(&workspace).session_id.unwrap();
    assert!(workspace
        .session(&builder)
        .unwrap()
        .prompt
        .contains("## Approved plan\nFollow this plan.\n\n1. Read the token.\n2. Cover logout."));

    // Editing the task sends the plan back.
    let mut edited = TaskInput {
        id: Some(id.clone()),
        project_id: project,
        spec_id: None,
        title: "Refresh the token on login and logout".into(),
        details: task(&workspace).details,
        findings: String::new(),
        agent: Agent::Claude,
        mode: PublishMode::None,
        auto_review: true,
        skills: None,
    };
    edited.findings.clear();
    workspace.save_task(edited).unwrap();
    assert_eq!(
        task(&workspace).plan.unwrap().status,
        PlanStatus::ChangesRequested
    );
    assert!(plan::needed(&task(&workspace)));
}

#[test]
fn plans_approve_themselves_where_asked() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch {
        plan_auto_approve: Some(true),
        ..ProjectPatch::default()
    });
    workspace.set_task_plan(&id, true, None).unwrap();
    plan::begin(&mut workspace, &context(), &id).unwrap();
    let draft = task(&workspace).plan.unwrap();
    write(
        &stage::file(&workspace, Stage::Plan, &draft.id),
        "The plan.",
    );
    let actions = pipeline::poll(&mut workspace, &context()).unwrap();
    assert!(actions.contains(&Action::Run { task: id.clone() }));
    assert!(task(&workspace).plan.unwrap().approved());

    // The task can say otherwise.
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch {
        plan_auto_approve: Some(true),
        ..ProjectPatch::default()
    });
    workspace.set_task_plan(&id, true, Some(false)).unwrap();
    plan::begin(&mut workspace, &context(), &id).unwrap();
    let draft = task(&workspace).plan.unwrap();
    write(
        &stage::file(&workspace, Stage::Plan, &draft.id),
        "The plan.",
    );
    pipeline::poll(&mut workspace, &context()).unwrap();
    assert_eq!(task(&workspace).status, TaskStatus::PlanReview);
}

#[test]
fn the_document_is_kept_only_from_inside_the_documents_folder() {
    let docs = tempfile::tempdir().unwrap();
    let (_fixture, mut workspace, project, id) = setup(ProjectPatch {
        docs_dir: Some(docs.path().display().to_string()),
        docs_auto: Some(true),
        ..ProjectPatch::default()
    });
    workspace.prepare_task(&id, None).unwrap();

    // A pass with nothing else to run: ready, and the writer starts.
    let actions = pipeline::handle(
        &mut workspace,
        &context(),
        &id,
        Event::Review(Some(true), String::new()),
    )
    .unwrap();
    let writer = started(&actions).pop().expect("a documentation writer");
    let brief = workspace.session(&writer).unwrap().prompt.clone();
    assert!(brief.contains(&docs.path().display().to_string()));
    let run = task(&workspace).docs_run.unwrap();
    assert_eq!(run.status, DocsStatus::Writing);

    // A file outside the folder is refused; one inside is kept.
    let outside = tempfile::NamedTempFile::new().unwrap();
    let pdf = docs.path().join("LOYAL-523.pdf");
    std::fs::write(&pdf, "%PDF").unwrap();
    let sneaky = docs
        .path()
        .join("..")
        .join(outside.path().file_name().unwrap());
    write(
        &stage::file(&workspace, Stage::Docs, &run.id),
        &serde_json::json!({ "files": [outside.path(), sneaky, pdf] }).to_string(),
    );
    pipeline::poll(&mut workspace, &context()).unwrap();
    let done = task(&workspace);
    assert_eq!(done.docs.len(), 1);
    assert_eq!(done.docs[0].kind, task_docs::DocKind::Pdf);
    assert_eq!(done.docs_run.unwrap().status, DocsStatus::Written);

    // The page lists it under its task, with a file put there by hand.
    std::fs::write(docs.path().join("notes.md"), "# Notes").unwrap();
    let listed = task_docs::list(&workspace, &project).unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .any(|doc| doc.task_id.as_deref() == Some(id.as_str()) && doc.title == "LOYAL-523"));
    assert!(listed
        .iter()
        .any(|doc| doc.task_id.is_none() && doc.title == "notes"));
    assert!(task_docs::inside(docs.path(), &docs.path().join("../etc")).is_err());
}

#[test]
fn a_writer_without_a_document_leaves_the_task_needing_attention() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch::default());
    workspace.prepare_task(&id, None).unwrap();
    let actions = task_docs::begin(&mut workspace, &context(), &id).unwrap();
    let writer = started(&actions).pop().unwrap();
    pipeline::on_exit(&mut workspace, &context(), &writer).unwrap();
    let stuck = task(&workspace);
    assert_eq!(stuck.status, TaskStatus::Attention);
    assert!(stuck.last_error.unwrap().starts_with("No document."));
    assert_eq!(stuck.docs_run.unwrap().status, DocsStatus::Failed);
}

#[test]
fn a_finished_task_can_be_reviewed_on_request() {
    let (_fixture, mut workspace, _, id) = setup(ProjectPatch::default());
    workspace.prepare_task(&id, None).unwrap();
    // Not while it builds.
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Building;
            Ok(())
        })
        .unwrap();
    assert!(pipeline::review_now(&mut workspace, &context(), &id).is_err());
    // A builder briefed before the pipeline: its hook moved it to review,
    // and it never writes a completion file.
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Review;
            Ok(())
        })
        .unwrap();
    let actions = pipeline::review_now(&mut workspace, &context(), &id).unwrap();
    assert_eq!(started(&actions).len(), 1);
    assert_eq!(
        task(&workspace).review.unwrap().status,
        ReviewStatus::Reviewing
    );
    assert_eq!(task(&workspace).stage_run, None);
    assert!(
        pipeline::review_now(&mut workspace, &context(), &id).is_err(),
        "once at a time"
    );
}

#[test]
fn a_claude_planner_s_plan_is_read_from_what_it_presented() {
    let (fixture, mut workspace, _, id) = setup(ProjectPatch {
        plan_first_default: Some(true),
        ..ProjectPatch::default()
    });
    let planner = plan::begin(&mut workspace, &context(), &id).unwrap();
    let home = fixture.path().join("claude-home");
    let session = workspace.session(&planner).unwrap().clone();
    let (planner_id, provider) = (planner.clone(), session.provider_id.clone());
    let at = home.clone();
    workspace
        .update(move |state| {
            let session = state
                .sessions
                .iter_mut()
                .find(|s| s.id == planner_id)
                .unwrap();
            session.agent_home = Some(at);
            Ok(())
        })
        .unwrap();
    let directory = workspace.project(&session.project_id).unwrap().path.clone();
    let folder = home
        .join("projects")
        .join(convoy_core::provider::sweep::slug(&directory));
    std::fs::create_dir_all(&folder).unwrap();
    let presented = |at: &str, plan: &str| {
        serde_json::json!({
            "type": "assistant",
            "timestamp": at,
            "message": { "content": [ { "type": "tool_use", "name": "ExitPlanMode", "input": { "plan": plan } } ] }
        })
        .to_string()
    };
    std::fs::write(
        folder.join(format!("{provider}.jsonl")),
        [
            presented("2000-01-01T00:00:00.000Z", "An old plan."),
            r#"{"type":"user","message":{"content":"ok"}}"#.to_string(),
            presented("2999-01-01T00:00:00.000Z", "# Plan\n1. Read the token."),
        ]
        .join("\n"),
    )
    .unwrap();
    pipeline::poll(&mut workspace, &context()).unwrap();
    let plan = task(&workspace).plan.unwrap();
    assert_eq!(
        plan.text, "# Plan\n1. Read the token.",
        "the latest since it was asked"
    );
    assert_eq!(task(&workspace).status, TaskStatus::PlanReview);
}
