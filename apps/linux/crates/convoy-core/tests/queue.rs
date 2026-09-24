//! The task queue: what runs next, and what a clean exit means.
//!
//! They decide when an agent starts and
//! whether work is published, so they are worth stating plainly.

mod common;

use common::fixture;
use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::planning::{SpecInput, TaskInput};
use convoy_core::queue::{self, Completion, Step};
use convoy_core::workspace::Workspace;

fn task(project: &str, title: &str, mode: PublishMode, auto_review: bool) -> TaskInput {
    TaskInput {
        id: None,
        project_id: project.to_string(),
        spec_id: None,
        title: title.to_string(),
        details: "Do the thing".into(),
        findings: String::new(),
        agent: Agent::Claude,
        mode,
        auto_review,
    }
}

#[test]
fn the_queue_runs_one_task_at_a_time_and_stops_when_empty() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let idle = |_: &str| false;

    assert_eq!(queue::next_step(&workspace, &project, &idle), Step::Empty);

    workspace
        .save_task(task(&project, "First", PublishMode::None, false))
        .unwrap();
    workspace
        .save_task(task(&project, "Second", PublishMode::None, false))
        .unwrap();
    let first = workspace.state().tasks[0].id.clone();

    match queue::next_step(&workspace, &project, &idle) {
        Step::Run { task_id, .. } => assert_eq!(task_id, first, "the first queued task runs first"),
        other => panic!("unexpected step: {other:?}"),
    }

    // While one task is building with a live session, nothing else starts.
    workspace.prepare_task(&first, None).unwrap();
    let session = workspace.state().tasks[0].session_id.clone().unwrap();
    workspace
        .update(|state| {
            state.tasks[0].status = TaskStatus::Building;
            Ok(())
        })
        .unwrap();
    let running = |id: &str| id == session;
    assert_eq!(
        queue::next_step(&workspace, &project, &running),
        Step::Waiting
    );

    // A task marked building whose session died does not block the queue: a
    // restart never silently resumes, but it must not deadlock either.
    assert!(matches!(
        queue::next_step(&workspace, &project, &idle),
        Step::Run { .. }
    ));
}

#[test]
fn a_pull_request_task_gets_its_own_worktree_before_it_runs() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();

    workspace
        .save_task(task(&project, "Publish", PublishMode::Pr, false))
        .unwrap();
    let id = workspace.state().tasks[0].id.clone();
    assert_eq!(
        queue::needs_worktree(&workspace, &id),
        None,
        "there is no session to place yet"
    );

    workspace.prepare_task(&id, None).unwrap();
    let branch = queue::needs_worktree(&workspace, &id).expect("a branch is required");
    assert!(branch.starts_with("convoy/task-"));
    assert_eq!(branch.len(), "convoy/task-".len() + 8);

    // A task that publishes nothing shares the project checkout.
    workspace
        .save_task(task(&project, "Quietly", PublishMode::None, false))
        .unwrap();
    let quiet = workspace.state().tasks[1].id.clone();
    workspace.prepare_task(&quiet, None).unwrap();
    assert_eq!(queue::needs_worktree(&workspace, &quiet), None);
}

#[test]
fn a_clean_exit_asks_for_review_and_never_marks_work_done() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_task(task(&project, "Build it", PublishMode::None, true))
        .unwrap();
    let id = workspace.state().tasks[0].id.clone();
    workspace.prepare_task(&id, None).unwrap();
    let session = workspace.state().tasks[0].session_id.clone().unwrap();

    // Until the task reaches review there is nothing to hand on.
    assert_eq!(
        queue::on_clean_exit(&workspace, &session, "output").unwrap(),
        Completion::Nothing
    );

    convoy_core::session::finish_session(
        &mut workspace,
        &session,
        convoy_core::session::ExitCause::Exited(0),
    )
    .unwrap();
    assert_eq!(
        workspace.state().tasks[0].status,
        TaskStatus::Review,
        "a clean exit means review, not done"
    );

    let completion = queue::on_clean_exit(&workspace, &session, "\x1b[32mbuilt\x1b[0m").unwrap();
    let Completion::Review(handoff) = completion else {
        panic!("expected a review handoff, got {completion:?}");
    };
    assert_eq!(handoff.agent, Agent::Codex, "the other agent reviews");
    assert!(handoff.prompt.contains("built"));
    assert!(!handoff.prompt.contains('\x1b'));

    // Once a review exists, a second exit does not create another.
    workspace.add_session(*handoff).unwrap();
    assert_eq!(
        queue::on_clean_exit(&workspace, &session, "output").unwrap(),
        Completion::Continue
    );
}

#[test]
fn work_against_a_superseded_specification_goes_back_for_changes() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_spec(SpecInput {
            project_id: project.clone(),
            title: "Login".into(),
            acceptance: "Works".into(),
            ..SpecInput::default()
        })
        .unwrap();
    let spec = workspace.state().specs[0].id.clone();
    workspace.approve_spec(&spec, 1).unwrap();

    let mut input = task(&project, "Implement", PublishMode::None, true);
    input.spec_id = Some(spec.clone());
    workspace.save_task(input).unwrap();
    let id = workspace.state().tasks[0].id.clone();
    workspace.prepare_task(&id, None).unwrap();
    let session = workspace.state().tasks[0].session_id.clone().unwrap();
    convoy_core::session::finish_session(
        &mut workspace,
        &session,
        convoy_core::session::ExitCause::Exited(0),
    )
    .unwrap();

    // The specification moves while the agent works.
    let mut edited = SpecInput {
        id: Some(spec.clone()),
        project_id: project.clone(),
        title: "Login".into(),
        acceptance: "Works with a keyboard too".into(),
        ..SpecInput::default()
    };
    edited.problem = String::new();
    workspace.save_spec(edited).unwrap();

    assert_eq!(
        queue::on_clean_exit(&workspace, &session, "output").unwrap(),
        Completion::Superseded
    );
    queue::mark_superseded(&mut workspace, &session).unwrap();
    assert_eq!(workspace.state().tasks[0].status, TaskStatus::Changes);

    let reloaded = Workspace::load(&fixture.file).unwrap();
    assert_eq!(reloaded.state().tasks[0].status, TaskStatus::Changes);
}

#[test]
fn the_queue_summary_says_what_each_task_will_do() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_task(task(&project, "Quiet", PublishMode::None, false))
        .unwrap();
    workspace
        .save_task(task(&project, "Publish", PublishMode::Pr, true))
        .unwrap();

    let summary = queue::summary(&workspace, &project);
    assert_eq!(summary.len(), 2);
    assert_eq!(summary[0], "Quiet: no publishing");
    assert_eq!(summary[1], "Publish: pull request, automatic review");
}
