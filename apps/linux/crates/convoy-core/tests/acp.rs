//! Agents reached through ACP, in the roles Claude and Codex fill: building,
//! reviewing and testing a task.

mod common;

use common::fixture;
use convoy_core::model::{Agent, PublishMode};
use convoy_core::planning::TaskInput;
use convoy_core::qa;
use convoy_core::workspace::{ProjectPatch, Workspace};

fn task(workspace: &mut Workspace, project: &str, agent: Agent, acp: Option<&str>) -> String {
    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.into(),
            spec_id: None,
            title: "Add a health check".into(),
            details: "GET /health answers 200.".into(),
            findings: String::new(),
            agent,
            acp_agent: acp.map(str::to_string),
            mode: PublishMode::None,
            auto_review: false,
            skills: None,
        })
        .unwrap();
    let id = workspace.state().tasks.last().unwrap().id.clone();
    workspace.prepare_task(&id, None).unwrap();
    id
}

#[test]
fn an_acp_agent_reviews_and_tests_when_the_project_names_one() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                qa_agent: Some("acp:gemini".into()),
                review_agent: Some("acp:opencode".into()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    let settings = workspace.project(&project).unwrap();
    assert_eq!(settings.qa_agent, Some(Agent::Acp));
    assert_eq!(settings.qa_acp_agent.as_deref(), Some("gemini"));
    assert_eq!(settings.review_agent, Some(Agent::Acp));
    assert_eq!(settings.review_acp_agent.as_deref(), Some("opencode"));

    let id = task(&mut workspace, &project, Agent::Claude, None);
    let builder = workspace.state().tasks[0].session_id.clone().unwrap();

    let review = convoy_core::review::handoff(&workspace, &builder, "Review it.".into()).unwrap();
    assert_eq!(review.agent, Agent::Acp);
    assert_eq!(review.acp_agent.as_deref(), Some("opencode"));
    let builder_session = workspace.session(&builder).unwrap().clone();
    assert_eq!(convoy_core::review::reviewer_key(&workspace, &builder_session), "acp:opencode");
    workspace.add_session(review).unwrap();
    let session = workspace.state().sessions.last().unwrap().clone();
    assert_eq!(session.acp_agent.as_deref(), Some("opencode"));
    assert_eq!(session.view.as_deref(), Some("chat"), "its brief is its first turn");

    let tester = qa::handoff(&workspace, &id, Agent::Acp, "Test it.".into()).unwrap();
    assert_eq!(tester.acp_agent.as_deref(), Some("gemini"));
    workspace.add_session(tester).unwrap();

    // Everything written loads again.
    let again = Workspace::load(&fixture.file).unwrap();
    assert_eq!(again.state().sessions.len(), workspace.state().sessions.len());

    // Back to Claude: the ACP id goes with it.
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                review_agent: Some("claude".into()),
                qa_agent: Some(String::new()),
                ..ProjectPatch::default()
            },
        )
        .unwrap();
    let settings = workspace.project(&project).unwrap();
    assert_eq!(settings.review_agent, Some(Agent::Claude));
    assert_eq!(settings.review_acp_agent, None);
    assert_eq!(settings.qa_agent, None);
    assert_eq!(settings.qa_acp_agent, None);
}

#[test]
fn an_acp_builder_is_reviewed_by_claude_unless_the_project_says_otherwise() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    task(&mut workspace, &project, Agent::Acp, Some("opencode"));
    let builder = workspace.state().tasks[0].session_id.clone().unwrap();
    let session = workspace.session(&builder).unwrap().clone();
    assert_eq!(session.agent, Agent::Acp);
    assert_eq!(session.acp_agent.as_deref(), Some("opencode"));

    let review = convoy_core::review::handoff(&workspace, &builder, String::new()).unwrap();
    assert_eq!(review.agent, Agent::Claude);
    assert_eq!(review.acp_agent, None);
}

#[test]
fn an_unknown_acp_agent_is_refused() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let saved = workspace.save_task(TaskInput {
        id: None,
        project_id: project.clone(),
        spec_id: None,
        title: "x".into(),
        details: String::new(),
        findings: String::new(),
        agent: Agent::Acp,
        acp_agent: Some("nobody".into()),
        mode: PublishMode::None,
        auto_review: false,
        skills: None,
    });
    assert!(saved.is_err());
    assert!(workspace
        .edit_project(
            &project,
            ProjectPatch {
                qa_agent: Some("acp".into()),
                ..ProjectPatch::default()
            },
        )
        .is_err());
}
