//! Importing what the native macOS app saved.

mod common;

use common::fixture;
use convoy_core::macos_import::{binding_here, import, preview, MacSources};
use convoy_core::model::{ActivityKind, Agent, KeepAwake, TaskStatus, Theme};
use convoy_core::Storage;
use serde_json::{json, Map, Value};
use std::path::Path;

/// A macOS support folder and Claude home with the shapes the Swift app writes.
fn swift_fixture(root: &Path, repo: &Path, child: &Path) -> MacSources {
    let support = root.join("support");
    let claude = root.join("claude");
    std::fs::create_dir_all(support.join("TerminalHistory")).unwrap();
    std::fs::create_dir_all(support.join("icons")).unwrap();
    std::fs::write(support.join("icons/gh-acme.png"), b"\x89PNG\r\n\x1a\nfake").unwrap();
    std::fs::write(
        support.join("TerminalHistory/11111111-1111-1111-1111-111111111111.txt"),
        "saved output",
    )
    .unwrap();

    // Claude keeps the conversation that ran; the other never started.
    let slug: String = repo
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    std::fs::create_dir_all(claude.join("projects").join(&slug)).unwrap();
    std::fs::write(
        claude
            .join("projects")
            .join(&slug)
            .join("aaaaaaaa-0000-4000-8000-000000000001.jsonl"),
        "{}\n",
    )
    .unwrap();

    let workspace = json!({
        "schemaVersion": 2,
        "projects": [
            {
                "id": "AAAAAAAA-0000-0000-0000-00000000000A", "name": "acme", "path": repo.to_string_lossy(),
                "group": true, "order": 0, "icon": format!("gh:{}", support.join("icons/gh-acme.png").to_string_lossy()),
                "color": "#4CAF83", "defaultAgent": "Codex", "sharedPaths": [".env", "node_modules"],
                "setupCommands": "pnpm install", "baseRef": "main",
                "specs": [{
                    "id": "5PEC0000-0000-0000-0000-000000000001", "title": "Checkout", "problem": "p", "requirements": "r",
                    "acceptance": "a", "constraints": "", "plan": "", "revision": 2, "approvedRevision": 2, "updatedAt": 700000000.0,
                    "tasks": [{ "id": "W0RK0000-0000-0000-0000-000000000001", "title": "Build it", "status": "Needs review",
                                "builder": "Claude Code", "reviewer": "Codex", "notes": "n", "findings": "f", "sessions": [] }]
                }],
                "sessions": [
                    { "id": "11111111-1111-1111-1111-111111111111", "agent": "Claude Code", "sessionID": "aaaaaaaa-0000-4000-8000-000000000001",
                      "title": "Builder", "notes": "", "createdAt": 780000000.0, "initialPrompt": "Fix it", "pinned": true },
                    { "id": "22222222-2222-2222-2222-222222222222", "agent": "Claude Code", "sessionID": "aaaaaaaa-0000-4000-8000-000000000002",
                      "title": "Never ran", "notes": "", "createdAt": 780000500.0 },
                    { "id": "33333333-3333-3333-3333-333333333333", "agent": "Codex", "sessionID": "",
                      "title": "Review: Builder", "notes": "", "createdAt": 780000100.0, "reviewOf": "11111111-1111-1111-1111-111111111111" },
                    { "id": "44444444-4444-4444-4444-444444444444", "agent": "Claude Code", "sessionID": "",
                      "title": "Login: Claude Code · work", "notes": "", "createdAt": 780000200.0 }
                ]
            },
            {
                "id": "BBBBBBBB-0000-0000-0000-00000000000B", "name": "api", "path": child.to_string_lossy(),
                "parentID": "AAAAAAAA-0000-0000-0000-00000000000A", "specs": []
            }
        ],
        "tasks": [
            { "id": "7A5C0000-0000-0000-0000-000000000001", "projectID": "AAAAAAAA-0000-0000-0000-00000000000A", "title": "ENG-1: Fix login",
              "details": "Steps", "spec": ".specdesk/specs/login.md", "mode": "pr", "agent": "Claude Code", "status": "pr",
              "createdAt": 780000000.0, "sessionID": "11111111-1111-1111-1111-111111111111", "prURL": "https://github.com/acme/pull/1",
              "autoReview": true, "model": "opus",
              "source": { "tracker": "Linear", "key": "ENG-1", "origin": "linear.app/acme", "url": "https://linear.app/acme/issue/ENG-1" } }
        ],
        "quickCommands": [
            { "id": "C0000000-0000-0000-0000-000000000001", "title": "Tests", "text": "npm test", "submit": true },
            { "id": "C0000000-0000-0000-0000-000000000002", "title": "Lint", "text": "npm run lint", "submit": false,
              "projectID": "BBBBBBBB-0000-0000-0000-00000000000B" }
        ]
    });
    let activity = json!([
        { "id": "E0000000-0000-0000-0000-000000000001", "date": 780000300.5, "kind": "waiting",
          "sessionID": "11111111-1111-1111-1111-111111111111", "sessionTitle": "Builder", "projectName": "acme", "detail": "Permission" },
        { "id": "E0000000-0000-0000-0000-000000000002", "date": 780000400.0, "kind": "slept",
          "sessionID": "11111111-1111-1111-1111-111111111111", "sessionTitle": "Builder", "projectName": "acme", "detail": "" },
        { "id": "E0000000-0000-0000-0000-000000000003", "date": 780000400.0, "kind": "done",
          "sessionID": "99999999-9999-9999-9999-999999999999", "sessionTitle": "Deleted", "projectName": "x", "detail": "" }
    ]);
    let mut defaults = Map::new();
    for (key, value) in [
        ("appearance", json!("dark")),
        ("defaultAgent", json!("Codex")),
        ("terminalFontSize", json!(14.0)),
        ("yoloCodex", json!(true)),
        ("notifyDone", json!(false)),
        ("wakeMode", json!("sessions")),
        ("gitPollSeconds", json!(30.0)),
        ("branchPrefix", json!("feature/")),
        (
            "keybindings",
            json!({ "session.stop": "cmd+.", "go.sidebar": "cmd+b", "tab.switch": "cmd+e", "session.edit": "" }),
        ),
        (
            "trackerConnections",
            json!(
                r#"[{"id":"D0000000-0000-0000-0000-000000000001","kind":"Linear","auth":"apiKey","name":"Acme Linear"}]"#
            ),
        ),
    ] {
        defaults.insert(key.to_string(), value);
    }
    MacSources {
        support,
        workspace,
        activity: Some(activity),
        defaults,
        claude_home: claude,
    }
}

#[test]
fn everything_the_macos_app_saved_comes_across() {
    let fixture = fixture();
    let repo = fixture.path().join("acme");
    let child = repo.join("api");
    std::fs::create_dir_all(&child).unwrap();
    let sources = swift_fixture(fixture.path(), &repo, &child);
    let storage = Storage::new(fixture.path().join("store"));
    let mut workspace = convoy_core::Workspace::load(storage.workspace_file()).unwrap();

    let dry = preview(&workspace, &storage, &sources, true);
    assert_eq!((dry.projects, dry.sessions), (2, 3));
    assert!(
        workspace.state().projects.is_empty(),
        "a preview changes nothing"
    );

    let report = import(&mut workspace, &storage, &sources, true).unwrap();
    assert_eq!(
        (
            report.projects,
            report.sessions,
            report.specs,
            report.tasks,
            report.quick_commands,
            report.activity,
            report.connections
        ),
        (2, 3, 1, 2, 2, 2, 1)
    );

    // Saved and reloaded: the result passes this client's own validation.
    let saved = convoy_core::Workspace::load(storage.workspace_file()).unwrap();
    let state = saved.state();

    let acme = state.projects.iter().find(|p| p.title == "acme").unwrap();
    assert_eq!(acme.default_agent, Some(Agent::Codex));
    assert_eq!(acme.shared_paths.as_deref(), Some(".env\nnode_modules"));
    assert_eq!(acme.setup_command.as_deref(), Some("pnpm install"));
    assert_eq!(
        acme.task_mode.as_deref(),
        Some("pr"),
        "the macOS default is written out"
    );
    let icon = acme.icon.clone().unwrap();
    assert!(
        icon.starts_with("gh:") && Path::new(&icon[3..]).starts_with(storage.root().join("icons")),
        "{icon}"
    );
    assert!(Path::new(&icon[3..]).is_file(), "the image is copied");
    let api = state.projects.iter().find(|p| p.title == "api").unwrap();
    assert_eq!(
        api.group.as_deref(),
        Some("acme"),
        "a child is filed under its parent"
    );

    let builder = state
        .sessions
        .iter()
        .find(|s| s.title == "Builder")
        .unwrap();
    assert!(builder.started, "Claude's transcript proves it ran");
    assert_eq!(
        (builder.agent, builder.prompt.as_str(), builder.pinned),
        (Agent::Claude, "Fix it", Some(true))
    );
    assert_eq!(
        builder.task_id.as_deref(),
        Some("7A5C0000-0000-0000-0000-000000000001")
    );
    let never = state
        .sessions
        .iter()
        .find(|s| s.title == "Never ran")
        .unwrap();
    assert!(!never.started, "no transcript: it starts fresh");
    let review = state
        .sessions
        .iter()
        .find(|s| s.title == "Review: Builder")
        .unwrap();
    assert_eq!(review.review_of.as_deref(), Some(builder.id.as_str()));
    assert!(
        state
            .sessions
            .iter()
            .all(|s| !s.title.starts_with("Login:")),
        "login terminals are not conversations"
    );
    assert!(storage
        .history()
        .join(format!(
            "{}.txt",
            convoy_core::hash::sha256_hex(&builder.id)
        ))
        .is_file());

    let task = state
        .tasks
        .iter()
        .find(|t| t.title == "ENG-1: Fix login")
        .unwrap();
    assert_eq!(
        task.status,
        TaskStatus::Review,
        "a PR in progress is here for review"
    );
    assert!(task.details.contains(".specdesk/specs/login.md") && task.details.contains("pull/1"));
    assert_eq!(task.source.as_ref().unwrap().key, "ENG-1");
    let work = state.tasks.iter().find(|t| t.title == "Build it").unwrap();
    assert_eq!(
        (work.status, work.spec_id.as_deref()),
        (
            TaskStatus::Review,
            Some("5PEC0000-0000-0000-0000-000000000001")
        )
    );
    assert!(state.specs[0].approved());

    let lint = state
        .quick_commands
        .iter()
        .find(|c| c.title == "Lint")
        .unwrap();
    assert_eq!(lint.project_id.as_deref(), Some(api.id.as_str()));

    assert_eq!(
        state.activity.len(),
        2,
        "events of sessions that no longer exist are dropped"
    );
    assert_eq!(
        state.activity[0].kind,
        ActivityKind::Hibernated,
        "newest first; slept is hibernated here"
    );
    assert!(
        state.activity[1].at.starts_with("2025-"),
        "{}",
        state.activity[1].at
    );

    let settings = &state.settings;
    assert_eq!(
        (settings.theme, settings.default_agent, settings.font_size),
        (Theme::Dark, Agent::Codex, 14)
    );
    assert!(settings.yolo_codex && !settings.notify_done && settings.notifications);
    assert_eq!(
        (settings.keep_awake, settings.git_poll_seconds),
        (KeepAwake::Sessions, 30)
    );
    assert_eq!(settings.branch_prefix, "feature");
    assert_eq!(
        settings.shortcuts.get("stop").map(String::as_str),
        Some("mod+.")
    );
    assert_eq!(
        settings.shortcuts.get("sidebar").map(String::as_str),
        Some("mod+b")
    );
    assert_eq!(
        settings.shortcuts.len(),
        2,
        "no counterpart for tab.switch; unbound is dropped"
    );

    let integrations =
        convoy_core::integrations::Integrations::load(storage.integrations()).unwrap();
    assert_eq!(integrations.connections()[0].name, "Acme Linear");
    assert!(integrations.connections()[0].secret.is_empty());

    // Running it again changes nothing.
    let again = import(&mut workspace, &storage, &sources, true).unwrap();
    assert_eq!(
        (
            again.projects,
            again.projects_existing,
            again.sessions,
            again.tasks,
            again.connections
        ),
        (0, 2, 0, 0, 0)
    );
}

#[test]
fn a_project_already_here_keeps_its_id_and_gains_the_sessions() {
    let fixture = fixture();
    let repo = fixture.path().join("acme");
    let child = repo.join("api");
    std::fs::create_dir_all(&child).unwrap();
    let sources = swift_fixture(fixture.path(), &repo, &child);
    let storage = Storage::new(fixture.path().join("store"));
    let mut workspace = convoy_core::Workspace::load(storage.workspace_file()).unwrap();
    workspace.add_project(&repo).unwrap();
    let existing = workspace.state().projects[0].id.clone();

    let report = import(&mut workspace, &storage, &sources, false).unwrap();
    assert!(report.projects_existing >= 1);
    assert!(!report.settings, "settings are optional");
    assert!(
        workspace
            .state()
            .sessions
            .iter()
            .filter(|s| s.project_id == existing)
            .count()
            >= 3
    );
}

#[test]
fn macos_shortcuts_translate_or_are_left_out() {
    assert_eq!(binding_here("cmd+shift+r").as_deref(), Some("mod+shift+r"));
    assert_eq!(binding_here("cmd+opt+p").as_deref(), Some("mod+alt+p"));
    assert_eq!(binding_here("ctrl+tab").as_deref(), Some("ctrl+tab"));
    assert_eq!(binding_here("cmd+ctrl+r").as_deref(), Some("mod+ctrl+r"));
    assert_eq!(
        binding_here("ctrl+c"),
        None,
        "Ctrl with a letter belongs to the terminal"
    );
    assert_eq!(binding_here("cmd+return"), None);
    let _: Value = json!(null);
}
