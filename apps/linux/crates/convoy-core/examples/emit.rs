//! Writes a fully populated `workspace.json`, for seeding a client under test. Usage: `cargo run -p convoy-core --example emit -- <path>`

use convoy_core::planning::{SpecInput, TaskInput};
use convoy_core::workspace::model::{ActivityKind, Agent, PublishMode, QuickCommand, Theme};
use convoy_core::workspace::{NewSession, SettingsPatch, Workspace};

fn main() {
    let file = std::env::args().nth(1).expect("output path");
    let project_path = std::env::args().nth(2).expect("project path");
    let mut workspace = Workspace::load(&file).expect("load");

    workspace
        .add_project(std::path::Path::new(&project_path))
        .expect("project");
    let project = workspace.state().projects[0].id.clone();

    let mut builder = NewSession::new(&project, Agent::Claude, "Builder");
    builder.prompt = "Fix the parser · Привет · 🙂".into();
    builder.model = "sonnet".into();
    workspace.add_session(builder).expect("session");
    let builder_id = workspace.state().sessions[0].id.clone();

    let mut review = NewSession::new(&project, Agent::Codex, "Review");
    review.review_of = Some(builder_id.clone());
    workspace.add_session(review).expect("review");

    workspace
        .save_spec(SpecInput {
            project_id: project.clone(),
            title: "Login".into(),
            problem: "Sign-in is unreachable by keyboard".into(),
            acceptance: "Keyboard navigation works".into(),
            ..SpecInput::default()
        })
        .expect("spec");
    let spec = workspace.state().specs[0].id.clone();
    workspace.approve_spec(&spec, 1).expect("approve");

    workspace
        .save_task(TaskInput {
            id: None,
            project_id: project.clone(),
            spec_id: Some(spec),
            title: "Keyboard support".into(),
            details: "Tab and Escape".into(),
            findings: String::new(),
            agent: Agent::Codex,
            mode: PublishMode::Pr,
            auto_review: true,
        })
        .expect("task");
    let task = workspace.state().tasks[0].id.clone();
    workspace.prepare_task(&task, None).expect("prepare");

    workspace
        .add_profile("Work account", Agent::Codex)
        .expect("profile");
    workspace
        .save_command(QuickCommand {
            id: String::new(),
            title: "Check".into(),
            text: "Run the test suite".into(),
            submit: false,
            project_id: Some(project),
            unknown: Default::default(),
        })
        .expect("command");
    workspace
        .save_settings(SettingsPatch {
            theme: Some(Theme::Light),
            font_size: Some(18),
            shortcuts: Some(
                [("palette".to_string(), "mod+shift+p".to_string())]
                    .into_iter()
                    .collect(),
            ),
            ..SettingsPatch::default()
        })
        .expect("settings");
    workspace
        .record(ActivityKind::Started, &builder_id, "Launched")
        .expect("activity");

    println!("{file}");
}
