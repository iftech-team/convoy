//! Gemini CLI as a third agent: how it is launched and resumed, how it is
//! given instructions, trusted folders, and the session id read from its
//! files.

mod common;

use common::fixture;
use convoy_core::model::Agent;
use convoy_core::provider::launch::agent_args;
use convoy_core::provider::{add_dir_flags, gemini, models, permission_flags, session_spec_with};
use convoy_core::workspace::model::{Session, Settings};
use std::collections::BTreeMap;

fn session(provider_id: &str, prompt: &str) -> Session {
    let mut session = Session::new("project", Agent::Gemini, "Session");
    session.provider_id = provider_id.into();
    session.prompt = prompt.into();
    session
}

#[test]
fn gemini_starts_with_its_message_and_resumes_its_own_session() {
    assert_eq!(
        agent_args(&session("", "Fix login"), false),
        ["gemini", "-i", "Fix login"]
    );
    assert_eq!(agent_args(&session("", ""), false), ["gemini"]);
    // Resumed by id once known, the most recent before; never the brief again.
    assert_eq!(
        agent_args(
            &session("8a749a7c-1cff-4b83-ad3a-bd13a993fa73", "Fix login"),
            true
        ),
        ["gemini", "--resume", "8a749a7c-1cff-4b83-ad3a-bd13a993fa73"]
    );
    assert_eq!(
        agent_args(&session("", "Fix login"), true),
        ["gemini", "--resume", "latest"]
    );
    // What waits for a resume goes with it.
    let mut waiting = session("abc", "Fix login");
    waiting.resume_prompt = Some("QA found problems.".into());
    assert_eq!(
        agent_args(&waiting, true),
        ["gemini", "--resume", "abc", "-i", "QA found problems."]
    );

    // Its flags go before the message.
    let mut chosen = session("", "Fix login");
    chosen.model = Some("gemini-2.5-pro".into());
    let settings = Settings {
        yolo_gemini: true,
        ..Settings::default()
    };
    let extra = [
        permission_flags(Agent::Gemini, &settings),
        add_dir_flags(Agent::Gemini, Some(&["/w/api".into()])),
    ]
    .concat();
    let spec = session_spec_with(&chosen, None, &BTreeMap::new(), &extra);
    let line = spec.args.join(" ");
    assert!(
        line.contains("'gemini' '--model' 'gemini-2.5-pro' '--yolo' '--include-directories' '/w/api' '-i' 'Fix login'")
            || line.contains("gemini --model gemini-2.5-pro --yolo --include-directories /w/api -i 'Fix login'"),
        "{line}"
    );
    assert!(permission_flags(Agent::Gemini, &Settings::default()).is_empty());
}

#[test]
fn gemini_is_reviewed_by_claude_and_has_its_models() {
    assert_eq!(Agent::Gemini.reviewer(), Agent::Claude);
    assert_eq!(Agent::Claude.reviewer(), Agent::Codex);
    assert_eq!(Agent::Codex.reviewer(), Agent::Claude);
    assert!(!Agent::Gemini.has_chat());
    assert_eq!(Agent::parse("gemini"), Some(Agent::Gemini));
    let list = models::list(Agent::Gemini, std::path::Path::new("/nowhere"), None);
    assert!(list.iter().any(|model| model.id == "gemini-2.5-pro"));
    assert!(list.iter().all(|model| model.efforts.is_empty()));
    assert!(!convoy_core::provider::valid_effort(Agent::Gemini, "high"));
}

#[test]
fn a_folder_is_trusted_for_gemini_unless_refused() {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("trustedFolders.json");
    std::fs::write(&file, r#"{"/Users/me": "DO_NOT_TRUST"}"#).unwrap();
    assert!(convoy_core::trust::approve_gemini(&file, "/Users/me/app").unwrap());
    assert!(
        !convoy_core::trust::approve_gemini(&file, "/Users/me/app").unwrap(),
        "once"
    );
    assert!(
        !convoy_core::trust::approve_gemini(&file, "/Users/me").unwrap(),
        "a refusal stays"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["/Users/me/app"], "TRUST_FOLDER");
    assert_eq!(saved["/Users/me"], "DO_NOT_TRUST");
}

#[test]
fn the_session_id_is_read_from_geminis_files() {
    let home = tempfile::tempdir().unwrap();
    let write = |folder: &str, file: &str, id: &str, start: &str| {
        let dir = home.path().join("tmp").join(folder).join("chats");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(file),
            format!(r#"{{"sessionId": "{id}", "startTime": "{start}", "messages": []}}"#),
        )
        .unwrap();
    };
    // The current layout: a name per folder in projects.json.
    std::fs::write(
        home.path().join("projects.json"),
        r#"{"projects": {"/w/app": "app"}}"#,
    )
    .unwrap();
    write("app", "session-old.json", "old", "2026-10-01T10:00:00.000Z");
    write(
        "app",
        "session-mine.json",
        "mine",
        "2026-10-15T10:00:02.000Z",
    );
    write(
        "app",
        "session-later.json",
        "later",
        "2026-10-15T11:00:00.000Z",
    );
    let found = gemini::session_since(
        home.path(),
        std::path::Path::new("/w/app"),
        "2026-10-15T10:00:00.000Z",
        &[],
    );
    assert_eq!(
        found.as_deref(),
        Some("mine"),
        "the first one since the launch"
    );
    // One another Convoy session already has is passed by.
    let found = gemini::session_since(
        home.path(),
        std::path::Path::new("/w/app"),
        "2026-10-15T10:00:00.000Z",
        &["mine".into()],
    );
    assert_eq!(found.as_deref(), Some("later"));

    // The older layout: the folder's path hashed.
    let hashed = convoy_core::hash::sha256_hex("/w/old");
    write(
        &hashed,
        "session-x.json",
        "hashed",
        "2026-10-15T10:00:01.000Z",
    );
    assert_eq!(
        gemini::session_since(
            home.path(),
            std::path::Path::new("/w/old"),
            "2026-10-15T10:00:03.000Z",
            &[]
        )
        .as_deref(),
        Some("hashed"),
        "a few seconds' slack"
    );
}

#[test]
fn a_gemini_session_takes_the_id_once_and_its_instructions_open_the_first_message() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let mut input =
        convoy_core::workspace::NewSession::new(project.clone(), Agent::Gemini, "Gemini");
    input.prompt = "Refresh the token.".into();
    workspace.add_session(input).unwrap();
    let id = workspace.state().sessions[0].id.clone();
    assert!(workspace.state().sessions[0].provider_id.is_empty());
    assert!(convoy_core::session::record_provider_id(
        &mut workspace,
        &id,
        "8a749a7c-1cff-4b83-ad3a-bd13a993fa73"
    )
    .unwrap());
    assert!(!convoy_core::session::record_provider_id(
        &mut workspace,
        &id,
        "9b749a7c-1cff-4b83-ad3a-bd13a993fa74"
    )
    .unwrap());

    // Its skills open its first message, ahead of what it was asked.
    let storage = convoy_core::Storage::new(fixture.path().join("data"));
    let mut store = convoy_core::skills::Store::load(storage.skills()).unwrap();
    let skill = store
        .save(
            convoy_core::skills::SkillInput {
                id: None,
                name: "Tests first".into(),
                description: String::new(),
                body: "Write the failing test first.".into(),
                project_id: None,
                agents: "both".into(),
                auto: false,
                roles: None,
                projects: None,
            },
            &workspace,
            0,
        )
        .unwrap();
    let mut fresh = convoy_core::workspace::NewSession::new(project, Agent::Gemini, "Fresh");
    fresh.prompt = "Build it.".into();
    fresh.skills = vec![skill];
    workspace.add_session(fresh).unwrap();
    let fresh = workspace.state().sessions.last().unwrap().id.clone();
    let plan = convoy_core::session::plan_launch(
        &workspace,
        &storage,
        &fresh,
        std::path::Path::new("/bin/convoy"),
    )
    .unwrap();
    let line = plan.spec.args.join(" ");
    let skill_at = line
        .find("Write the failing test first.")
        .expect("the skill is sent");
    let brief_at = line.find("Build it.").expect("the brief is sent");
    assert!(skill_at < brief_at, "{line}");
    assert!(!line.contains("--append-system-prompt"));

    // A chat cannot be made of it.
    assert!(convoy_core::session::plan_chat_launch(
        &workspace,
        &storage,
        &fresh,
        std::path::Path::new("/bin/convoy")
    )
    .is_err());
}
