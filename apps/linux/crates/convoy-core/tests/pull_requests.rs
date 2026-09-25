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
