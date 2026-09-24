//! Coverage for rules exercised only indirectly elsewhere:
//! settings bounds, shortcut syntax and the referential checks in
//! `validate()`. These are the rules a hand-edited or third-party
//! `workspace.json` is most likely to break.

mod common;

use common::{fixture, new_session};
use convoy_core::workspace::model::Agent;
use convoy_core::workspace::validate::validate;
use convoy_core::workspace::{SettingsPatch, Workspace};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;

fn document(patch: Value) -> Value {
    let mut base = json!({
        "schemaVersion": 3,
        "projects": [{ "id": "p1", "title": "Project", "path": "/tmp/project" }],
        "sessions": []
    });
    let object = base.as_object_mut().unwrap();
    for (key, value) in patch.as_object().unwrap() {
        object.insert(key.clone(), value.clone());
    }
    base
}

fn rejects(patch: Value, message: &str) {
    let error = validate(&document(patch)).expect_err("expected a rejection");
    assert_eq!(error.to_string(), message);
}

#[test]
fn schema_version_and_collections_must_be_present() {
    for broken in [
        json!({ "schemaVersion": 4, "projects": [], "sessions": [] }),
        json!({ "schemaVersion": 3, "projects": {}, "sessions": [] }),
        json!({ "schemaVersion": 3, "projects": [], "sessions": null }),
        json!(null),
    ] {
        let error = validate(&broken).expect_err("expected a rejection");
        assert_eq!(
            error.to_string(),
            "Unsupported or damaged desktop workspace. The file has not been changed."
        );
    }
}

#[test]
fn project_paths_must_be_absolute_and_ids_unique() {
    rejects(
        json!({ "projects": [{ "id": "p1", "title": "T", "path": "relative" }] }),
        "Invalid project path.",
    );
    rejects(
        json!({ "projects": [
            { "id": "p1", "title": "A", "path": "/a" },
            { "id": "p1", "title": "B", "path": "/b" }
        ] }),
        "Invalid workspace records.",
    );
}

#[test]
fn sessions_must_reference_a_project_and_carry_a_well_formed_provider_id() {
    let session = |patch: Value| {
        let mut base = json!({
            "id": "s1", "projectID": "p1", "agent": "codex",
            "title": "S", "prompt": "", "providerID": "", "started": false
        });
        let object = base.as_object_mut().unwrap();
        for (key, value) in patch.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        json!({ "sessions": [base] })
    };

    rejects(
        session(json!({ "projectID": "missing" })),
        "Invalid session record.",
    );
    rejects(
        session(json!({ "agent": "gemini" })),
        "Invalid session record.",
    );
    rejects(
        session(json!({ "started": "yes" })),
        "Invalid session record.",
    );
    rejects(
        session(json!({ "providerID": "not-a-uuid" })),
        "Invalid provider session ID.",
    );
    rejects(
        session(json!({ "agent": "claude", "providerID": "" })),
        "Missing Claude session ID.",
    );
    rejects(session(json!({ "notes": 7 })), "Invalid session metadata.");
    rejects(session(json!({ "pinned": "yes" })), "Invalid session flag.");
    rejects(
        session(json!({ "workingDirectory": "relative" })),
        "Invalid worktree path.",
    );
    rejects(
        session(json!({ "reviewOf": "s1" })),
        "Invalid review relationship.",
    );

    // A valid Codex session with no provider identity yet is accepted.
    assert!(validate(&document(session(json!({})))).is_ok());
}

#[test]
fn shortcuts_must_use_the_known_actions_and_stay_unique() {
    let settings = |shortcuts: Value| {
        json!({ "settings": {
            "fontSize": 14, "scrollback": 10000, "theme": "dark",
            "defaultAgent": "claude", "shortcuts": shortcuts
        }})
    };

    assert!(validate(&document(settings(
        json!({ "palette": "mod+shift+p", "files": "mod+b" })
    )))
    .is_ok());

    for broken in [
        json!({ "unknownAction": "mod+p" }),
        json!({ "palette": "ctrl+p" }),
        json!({ "palette": "mod+P" }),
        json!({ "palette": "mod+shift+alt+p" }),
        json!({ "palette": "mod+p", "files": "mod+p" }),
        json!([]),
    ] {
        rejects(settings(broken), "Invalid or duplicate shortcut.");
    }
}

#[test]
fn settings_bounds_are_enforced_with_their_own_messages() {
    let settings = |patch: Value| {
        let mut base = json!({
            "fontSize": 14, "scrollback": 10000, "theme": "dark", "defaultAgent": "claude"
        });
        let object = base.as_object_mut().unwrap();
        for (key, value) in patch.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        json!({ "settings": base })
    };

    rejects(settings(json!({ "fontSize": 9 })), "Invalid settings.");
    rejects(settings(json!({ "fontSize": 14.5 })), "Invalid settings.");
    rejects(settings(json!({ "scrollback": 999 })), "Invalid settings.");
    rejects(settings(json!({ "theme": "sepia" })), "Invalid settings.");
    rejects(
        settings(json!({ "hibernateMinutes": 1441 })),
        "Invalid hibernation delay.",
    );
    rejects(
        settings(json!({ "keepAwake": "sometimes" })),
        "Invalid keep-awake setting.",
    );
    rejects(
        settings(json!({ "claudeUsage": "yes" })),
        "Invalid Claude usage setting.",
    );
}

#[test]
fn quick_commands_are_bounded_and_scoped_to_a_known_project() {
    let command = |patch: Value| {
        let mut base = json!({ "id": "c1", "title": "T", "text": "body", "submit": false });
        let object = base.as_object_mut().unwrap();
        for (key, value) in patch.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        json!({ "quickCommands": [base] })
    };

    assert!(validate(&document(command(json!({ "projectID": "p1" })))).is_ok());
    rejects(
        command(json!({ "projectID": "missing" })),
        "Invalid quick command.",
    );
    rejects(command(json!({ "title": "   " })), "Invalid quick command.");
    rejects(command(json!({ "submit": 1 })), "Invalid quick command.");
    rejects(
        command(json!({ "text": "x".repeat(32_001) })),
        "Invalid quick command.",
    );
    rejects(
        json!({ "quickCommands": "none" }),
        "Invalid quick commands.",
    );
}

#[test]
fn activity_is_capped_and_must_point_at_a_live_session() {
    let event = |patch: Value| {
        let mut base = json!({
            "id": "a1", "at": "2026-09-24T10:00:00.000Z", "kind": "exited",
            "sessionID": "s1", "title": "T", "detail": "D"
        });
        let object = base.as_object_mut().unwrap();
        for (key, value) in patch.as_object().unwrap() {
            object.insert(key.clone(), value.clone());
        }
        json!({
            "sessions": [{
                "id": "s1", "projectID": "p1", "agent": "codex",
                "title": "S", "prompt": "", "providerID": "", "started": false
            }],
            "activity": [base]
        })
    };

    assert!(validate(&document(event(json!({})))).is_ok());
    rejects(
        event(json!({ "kind": "pondering" })),
        "Invalid activity event.",
    );
    rejects(
        event(json!({ "at": "sometime" })),
        "Invalid activity event.",
    );
    rejects(
        event(json!({ "sessionID": "gone" })),
        "Invalid activity event.",
    );

    let overflowing: Vec<Value> = (0..201)
        .map(|index| {
            json!({
                "id": format!("a{index}"), "at": "2026-09-24T10:00:00.000Z",
                "kind": "exited", "sessionID": "s1", "title": "T", "detail": "D"
            })
        })
        .collect();
    rejects(
        json!({
            "sessions": [{
                "id": "s1", "projectID": "p1", "agent": "codex",
                "title": "S", "prompt": "", "providerID": "", "started": false
            }],
            "activity": overflowing
        }),
        "Invalid activity history.",
    );
}

#[test]
fn saved_shortcuts_survive_a_round_trip() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .add_session(new_session(&project, Agent::Codex, "Session"))
        .unwrap();

    let shortcuts: BTreeMap<String, String> = [
        ("palette".to_string(), "mod+shift+p".to_string()),
        ("newSession".to_string(), "mod+n".to_string()),
    ]
    .into_iter()
    .collect();
    workspace
        .save_settings(SettingsPatch {
            shortcuts: Some(shortcuts.clone()),
            ..SettingsPatch::default()
        })
        .unwrap();

    let reloaded = Workspace::load(&fixture.file).unwrap();
    assert_eq!(reloaded.settings().shortcuts, shortcuts);

    // And the file on disk still passes validation.
    let raw: Value = serde_json::from_str(&fs::read_to_string(&fixture.file).unwrap()).unwrap();
    assert!(validate(&raw).is_ok());
    assert_eq!(raw["schemaVersion"], 3);
}
