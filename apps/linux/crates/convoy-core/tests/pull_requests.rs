//! What the agent prints that Convoy acts on: Codex's resume id and a pull
//! request link.

use convoy_core::model::{Agent, PublishMode, TaskStatus};
use convoy_core::planning::TaskInput;
use convoy_core::session::{
    codex_resume_id, finish_session, pull_request_url, record_provider_id, record_pull_request,
    ExitCause,
};
use convoy_core::workspace::NewSession;
use convoy_core::Workspace;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("app");
    std::fs::create_dir_all(&project).unwrap();
    let mut workspace = Workspace::load(dir.path().join("workspace.json")).unwrap();
    workspace.add_project(&project).unwrap();
    (dir, workspace)
}

#[test]
fn codex_resume_ids_and_pull_request_links_are_found_in_output() {
    let out = "\x1b[2mTo continue this session, run codex resume 0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b\x1b[0m";
    assert_eq!(
        codex_resume_id(&convoy_core::history::plain(out)).as_deref(),
        Some("0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b")
    );
    assert_eq!(codex_resume_id("codex resume --last"), None);
    assert_eq!(
        pull_request_url("Opened https://github.com/acme/app/pull/42 for review.").as_deref(),
        Some("https://github.com/acme/app/pull/42")
    );
    assert_eq!(
        pull_request_url("see (https://gitlab.com/acme/app/-/merge_requests/7)").as_deref(),
        Some("https://gitlab.com/acme/app/-/merge_requests/7")
    );
    assert_eq!(
        pull_request_url("https://github.com/acme/app/issues/3"),
        None
    );
}

#[test]
fn a_codex_session_without_an_id_takes_the_printed_one_once() {
    let (_dir, mut workspace) = workspace();
    let project = workspace.state().projects[0].id.clone();
    workspace
        .add_session(NewSession::new(project.clone(), Agent::Codex, "Codex work"))
        .unwrap();
    let id = workspace.state().sessions[0].id.clone();
    let first = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
    assert!(record_provider_id(&mut workspace, &id, first).unwrap());
    assert!(
        !record_provider_id(&mut workspace, &id, "0199a1b2-0000-7e5f-8a9b-0c1d2e3f4a5b").unwrap()
    );
    assert_eq!(workspace.session(&id).unwrap().provider_id, first);
}

#[test]
fn a_task_whose_session_opens_a_pull_request_awaits_it() {
    let (_dir, mut workspace) = workspace();
    let project = workspace.state().projects[0].id.clone();
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.clone(),
            spec_id: None,
            title: "Ship it".into(),
            details: "Open a pull request.".into(),
            findings: String::new(),
            agent: Agent::Claude,
            mode: PublishMode::Pr,
            auto_review: false,
        })
        .unwrap();
    let task = workspace.state().tasks[0].id.clone();
    workspace.prepare_task(&task, None).unwrap();
    let session = workspace.state().tasks[0].session_id.clone().unwrap();

    assert!(record_pull_request(
        &mut workspace,
        &session,
        "https://github.com/acme/app/pull/42"
    )
    .unwrap());
    assert!(!record_pull_request(
        &mut workspace,
        &session,
        "https://github.com/acme/app/pull/42"
    )
    .unwrap());
    finish_session(&mut workspace, &session, ExitCause::Exited(0)).unwrap();
    let task = &workspace.state().tasks[0];
    assert_eq!(task.status, TaskStatus::Pr);
    assert_eq!(
        task.pr_url.as_deref(),
        Some("https://github.com/acme/app/pull/42")
    );
}

#[test]
fn a_task_can_be_deleted_unless_its_agent_runs() {
    let (_dir, mut workspace) = workspace();
    let project = workspace.state().projects[0].id.clone();
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project,
            spec_id: None,
            title: "Throwaway".into(),
            details: String::new(),
            findings: String::new(),
            agent: Agent::Codex,
            mode: PublishMode::None,
            auto_review: false,
        })
        .unwrap();
    let task = workspace.state().tasks[0].id.clone();
    workspace.prepare_task(&task, None).unwrap();
    let session = workspace.state().tasks[0].session_id.clone().unwrap();
    workspace.delete_task(&task).unwrap();
    assert!(workspace.state().tasks.is_empty());
    assert_eq!(
        workspace.session(&session).unwrap().task_id,
        None,
        "the session is kept, unlinked"
    );
}

#[test]
fn projects_keep_the_order_they_are_moved_into() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::load(dir.path().join("workspace.json")).unwrap();
    for name in ["a", "b", "c"] {
        let folder = dir.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        workspace.add_project(&folder).unwrap();
    }
    let titles = |workspace: &Workspace| -> Vec<String> {
        workspace
            .state()
            .projects
            .iter()
            .map(|project| project.title.clone())
            .collect()
    };
    let c = workspace.state().projects[2].id.clone();
    workspace.move_project(&c, 0).unwrap();
    assert_eq!(titles(&workspace), ["c", "a", "b"]);
    workspace.move_project(&c, 9).unwrap();
    assert_eq!(titles(&workspace), ["a", "b", "c"]);
    let reloaded = Workspace::load(dir.path().join("workspace.json")).unwrap();
    assert_eq!(titles(&reloaded), ["a", "b", "c"], "the order is saved");
}

fn task(workspace: &mut Workspace, project: &str, title: &str) -> String {
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
        })
        .unwrap();
    workspace.state().tasks.last().unwrap().id.clone()
}

/// The queue passes a task by while one it waits for is not done; a circle
/// and a task of another project are refused.
#[test]
fn a_blocked_task_waits_and_circles_are_refused() {
    let (dir, mut workspace) = workspace();
    let project = workspace.state().projects[0].id.clone();
    let schema = task(&mut workspace, &project, "Schema");
    let api = task(&mut workspace, &project, "API");
    // "API" is first in the queue once it no longer waits.
    workspace
        .set_task_dependencies(&schema, vec![api.clone()])
        .unwrap();
    let next = |workspace: &Workspace| match convoy_core::queue::next_step(
        workspace,
        &project,
        &|_: &str| false,
    ) {
        convoy_core::queue::Step::Run { task_id, .. } => task_id,
        _ => String::new(),
    };
    assert_eq!(next(&workspace), api, "Schema waits for API");
    assert!(
        workspace
            .set_task_dependencies(&api, vec![schema.clone()])
            .is_err(),
        "a circle"
    );
    assert!(
        workspace
            .set_task_dependencies(&api, vec![api.clone()])
            .is_err(),
        "itself"
    );

    let elsewhere = dir.path().join("other");
    std::fs::create_dir_all(&elsewhere).unwrap();
    workspace.add_project(&elsewhere).unwrap();
    let other = workspace.state().projects[1].id.clone();
    let foreign = task(&mut workspace, &other, "Foreign");
    assert!(
        workspace
            .set_task_dependencies(&schema, vec![foreign])
            .is_err(),
        "another project"
    );

    workspace.delete_task(&api).unwrap();
    assert!(workspace
        .state()
        .tasks
        .iter()
        .find(|t| t.id == schema)
        .unwrap()
        .depends_on
        .is_empty());
}

/// "Link existing session": the session builds the task from then on, and
/// the one it replaces is let go.
#[test]
fn an_existing_session_can_be_linked_to_a_task() {
    let (_dir, mut workspace) = workspace();
    let project = workspace.state().projects[0].id.clone();
    let spec_task = task(&mut workspace, &project, "Login flow");
    workspace
        .add_session(NewSession::new(
            project.clone(),
            Agent::Codex,
            "Earlier work",
        ))
        .unwrap();
    let earlier = workspace.state().sessions.last().unwrap().id.clone();
    workspace.link_task_session(&spec_task, &earlier).unwrap();
    assert_eq!(
        workspace.state().tasks[0].session_id.as_deref(),
        Some(earlier.as_str())
    );
    assert_eq!(
        workspace.session(&earlier).unwrap().task_id.as_deref(),
        Some(spec_task.as_str())
    );

    workspace
        .add_session(NewSession::new(project.clone(), Agent::Codex, "Later work"))
        .unwrap();
    let later = workspace.state().sessions.last().unwrap().id.clone();
    workspace.link_task_session(&spec_task, &later).unwrap();
    assert_eq!(
        workspace.session(&earlier).unwrap().task_id,
        None,
        "the old one is let go"
    );

    let second = task(&mut workspace, &project, "Another");
    assert!(
        workspace.link_task_session(&second, &later).is_err(),
        "one task per session"
    );
}
