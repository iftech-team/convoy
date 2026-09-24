//! Ported from `test/planning.test.cjs` — specifications, tasks and accounts.

mod common;

use common::{fixture, new_session, Fixture};
use convoy_core::accounts::account_environment;
use convoy_core::planning::{markdown, SpecInput, TaskInput};
use convoy_core::provider::launch::{launch_spec, Bindings};
use convoy_core::workspace::model::{ActivityKind, Agent, PublishMode, Spec, Task, TaskStatus};
use convoy_core::workspace::Workspace;
use std::collections::BTreeMap;

struct Planned {
    fixture: Fixture,
    workspace: Workspace,
    project: String,
    spec: String,
    task: String,
}

fn planned() -> Planned {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_spec(SpecInput {
            project_id: project.clone(),
            title: "Login".into(),
            acceptance: "Keyboard navigation works".into(),
            ..SpecInput::default()
        })
        .unwrap();
    let spec = workspace.state().specs[0].id.clone();
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.clone(),
            spec_id: Some(spec.clone()),
            title: "Keyboard support".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Codex,
            mode: PublishMode::None,
            auto_review: false,
        })
        .unwrap();
    let task = workspace.state().tasks[0].id.clone();
    Planned {
        fixture,
        workspace,
        project,
        spec,
        task,
    }
}

/// The equivalent of `{ ...spec }` — resaving a specification as it stands.
fn spec_input(spec: &Spec) -> SpecInput {
    SpecInput {
        id: Some(spec.id.clone()),
        project_id: spec.project_id.clone(),
        title: spec.title.clone(),
        problem: spec.problem.clone(),
        requirements: spec.requirements.clone(),
        acceptance: spec.acceptance.clone(),
        constraints: spec.constraints.clone(),
        plan: spec.plan.clone(),
    }
}

fn task_input(task: &Task) -> TaskInput {
    TaskInput {
        id: Some(task.id.clone()),
        project_id: task.project_id.clone(),
        spec_id: task.spec_id.clone(),
        title: task.title.clone(),
        details: task.details.clone(),
        findings: task.findings.clone(),
        agent: task.agent,
        mode: task.mode,
        auto_review: task.auto_review,
    }
}

#[test]
fn spec_approval_gates_task_preparation_and_edits_invalidate_completed_work() {
    let mut planned = planned();
    let workspace = &mut planned.workspace;

    let error = workspace.prepare_task(&planned.task, None).unwrap_err();
    assert!(error.to_string().contains("Approve"), "{error}");
    let error = workspace.approve_spec(&planned.spec, 99).unwrap_err();
    assert!(error.to_string().contains("changed"), "{error}");

    workspace.approve_spec(&planned.spec, 1).unwrap();
    workspace.prepare_task(&planned.task, None).unwrap();
    let original = workspace.state().tasks[0].session_id.clone();

    // Repeated preparation does not fork a fresh conversation.
    workspace.prepare_task(&planned.task, None).unwrap();
    assert_eq!(workspace.state().tasks[0].session_id, original);

    workspace
        .set_task_status(&planned.task, TaskStatus::Done)
        .unwrap();

    let mut edited = spec_input(&workspace.state().specs[0]);
    edited.acceptance = "Keyboard and screen reader navigation work".into();
    workspace.save_spec(edited).unwrap();

    assert_eq!(workspace.state().specs[0].revision, 2);
    assert_eq!(workspace.state().specs[0].approved_revision, None);
    assert_eq!(workspace.state().tasks[0].status, TaskStatus::Changes);

    let error = workspace
        .set_task_status(&planned.task, TaskStatus::Done)
        .unwrap_err();
    assert!(error.to_string().contains("Approve"), "{error}");

    workspace.approve_spec(&planned.spec, 2).unwrap();
    workspace.prepare_task(&planned.task, None).unwrap();
    assert_ne!(workspace.state().tasks[0].session_id, original);
    assert_eq!(workspace.state().sessions.len(), 2);
    assert!(workspace.state().sessions[1]
        .prompt
        .contains("screen reader"));
}

#[test]
fn unchanged_specs_retain_approval_and_task_edits_preserve_previous_session_history() {
    let mut planned = planned();
    let workspace = &mut planned.workspace;
    workspace.approve_spec(&planned.spec, 1).unwrap();

    let unchanged = spec_input(&workspace.state().specs[0]);
    workspace.save_spec(unchanged).unwrap();
    assert_eq!(workspace.state().specs[0].approved_revision, Some(1));

    workspace.prepare_task(&planned.task, None).unwrap();
    let old = workspace.state().tasks[0].session_id.clone().unwrap();

    let mut edited = task_input(&workspace.state().tasks[0]);
    edited.details = "Support Tab and Escape".into();
    workspace.save_task(edited).unwrap();

    assert_eq!(workspace.state().tasks[0].session_id, None);
    assert!(workspace.session(&old).is_ok(), "the old session is kept");

    workspace.prepare_task(&planned.task, None).unwrap();
    assert_ne!(workspace.state().tasks[0].session_id, Some(old));
}

#[test]
fn running_tasks_reject_edits_and_relaunch_marks_them_interrupted_without_restarting() {
    let mut planned = planned();
    {
        let workspace = &mut planned.workspace;
        workspace.approve_spec(&planned.spec, 1).unwrap();
        workspace.prepare_task(&planned.task, None).unwrap();
        workspace
            .update(|state| {
                state.tasks[0].status = TaskStatus::Building;
                Ok(())
            })
            .unwrap();

        let mut edited = task_input(&workspace.state().tasks[0]);
        edited.title = "Changed".into();
        let error = workspace.save_task(edited).unwrap_err();
        assert!(error.to_string().contains("Stop"), "{error}");

        let error = workspace
            .set_task_status(&planned.task, TaskStatus::Done)
            .unwrap_err();
        assert!(error.to_string().contains("Stop"), "{error}");
    }

    let restored = Workspace::load(&planned.fixture.file).unwrap();
    assert_eq!(restored.state().tasks[0].status, TaskStatus::Failed);
    assert!(restored.state().tasks[0]
        .last_error
        .as_deref()
        .unwrap()
        .contains("closed"));
    assert_eq!(
        restored.state().tasks[0].session_id,
        planned.workspace.state().tasks[0].session_id,
        "the saved session is preserved, not restarted"
    );
}

#[test]
fn markdown_export_includes_acceptance_revision_tasks_and_findings() {
    let mut planned = planned();
    let workspace = &mut planned.workspace;
    workspace.approve_spec(&planned.spec, 1).unwrap();

    let mut edited = task_input(&workspace.state().tasks[0]);
    edited.findings = "Checked with keyboard only.".into();
    workspace.save_task(edited).unwrap();

    let output = markdown(&workspace.state().specs[0], &workspace.state().tasks);
    assert!(output.contains("Revision: 1 · Approved"), "{output}");
    assert!(output.contains("Keyboard navigation works"));
    assert!(output.contains("Checked with keyboard only"));
}

#[test]
fn account_profiles_use_separate_homes_and_reject_provider_mismatches() {
    let mut planned = planned();
    let directory = planned.fixture.path().to_path_buf();
    let workspace = &mut planned.workspace;
    workspace
        .add_profile("Work / ../ account", Agent::Codex)
        .unwrap();
    let profile = workspace.state().profiles[0].id.clone();

    let mut wrong = new_session(&planned.project, Agent::Claude, "Wrong account");
    wrong.profile_id = Some(profile.clone());
    let error = workspace.add_session(wrong).unwrap_err();
    assert!(error.to_string().contains("account"), "{error}");

    let mut right = new_session(&planned.project, Agent::Codex, "Work");
    right.profile_id = Some(profile);
    workspace.add_session(right).unwrap();
    let session = workspace.state().sessions.last().unwrap().clone();

    let source: BTreeMap<String, String> = [
        ("OPENAI_API_KEY".to_string(), "not-forwarded".to_string()),
        ("PATH".to_string(), "/bin".to_string()),
    ]
    .into_iter()
    .collect();
    let bound = account_environment(&session, &workspace.state().profiles, &directory, &source)
        .expect("account environment");

    // The home is named by a hash, so a label containing `../` cannot escape.
    assert_eq!(bound.home.parent().unwrap(), directory);
    assert_eq!(
        bound.env.get("CODEX_HOME").unwrap(),
        &bound.home.to_string_lossy().to_string()
    );
    assert_eq!(bound.env.get("OPENAI_API_KEY"), None);

    // A session that already ran keeps its recorded home even if the root moves.
    let mut resumed = session.clone();
    resumed.agent_home = Some(bound.home.clone());
    let moved = directory.join("changed");
    let other: BTreeMap<String, String> = [("CODEX_HOME".to_string(), "/other".to_string())]
        .into_iter()
        .collect();
    let resumed_account =
        account_environment(&resumed, &workspace.state().profiles, &moved, &other).unwrap();
    assert_eq!(resumed_account.home, bound.home);

    let error = account_environment(&session, &[], &directory, &source).unwrap_err();
    assert!(error.to_string().contains("missing"), "{error}");

    let bindings = Bindings {
        codex_home: Some(bound.home.to_string_lossy().into_owned()),
        ..Bindings::default()
    };
    let launch = launch_spec(&resumed, true, &bindings);
    assert!(launch.args[1].starts_with("exec env "));
    assert!(launch.args[1].contains(&format!("CODEX_HOME={}", bound.home.display())));
}

#[test]
fn activity_stays_bounded_and_persisted() {
    let mut planned = planned();
    let workspace = &mut planned.workspace;
    workspace
        .add_session(new_session(&planned.project, Agent::Codex, "Activity"))
        .unwrap();
    let session = workspace.state().sessions[0].id.clone();
    for index in 0..205 {
        workspace
            .record(ActivityKind::Exited, &session, &format!("Exit {index}"))
            .unwrap();
    }

    let restored = Workspace::load(&planned.fixture.file).unwrap();
    assert_eq!(restored.state().activity.len(), 200);
    assert_eq!(restored.state().activity[0].detail, "Exit 204");
}

#[test]
fn publication_and_review_choices_are_explicit_and_changing_them_invalidates_the_old_brief() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.clone(),
            spec_id: None,
            title: "Publish safely".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::Pr,
            auto_review: true,
        })
        .unwrap();
    let task = workspace.state().tasks[0].clone();

    workspace.prepare_task(&task.id, None).unwrap();
    assert!(workspace.state().sessions[0]
        .prompt
        .contains("pull request"));

    let mut edited = task_input(&task);
    edited.mode = PublishMode::None;
    workspace.save_task(edited).unwrap();
    assert_eq!(workspace.state().tasks[0].session_id, None);

    workspace.prepare_task(&task.id, None).unwrap();
    assert!(workspace
        .state()
        .sessions
        .last()
        .unwrap()
        .prompt
        .contains("Do not publish"));
}
