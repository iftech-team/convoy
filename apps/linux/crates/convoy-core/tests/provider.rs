//! Ported from `test/provider.test.cjs` — telemetry, usage quotas and the
//! provider's own saved conversations.

mod common;

use common::fixture;
use convoy_core::provider::rate_windows;
use convoy_core::provider::transcripts::scan;
use convoy_core::telemetry::{self, hook};
use convoy_core::workspace::model::{Agent, Session};
use serde_json::json;
use std::fs;

#[test]
fn telemetry_stores_only_status_and_validated_quota_windows() {
    let input = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "AskUserQuestion",
        "prompt": "secret",
        "transcript": "secret",
        "rate_limits": {
            "five_hour": { "used_percentage": 42, "resets_at": 500 },
            "seven_day": { "used_percentage": 101 }
        }
    });
    let result = telemetry::sanitize(&input, 100_000.0);
    assert_eq!(result["state"], "waiting");
    assert_eq!(
        result["windows"].as_array().unwrap().len(),
        1,
        "101% is not a quota"
    );

    let serialised = serde_json::to_string(&result).unwrap();
    assert!(!serialised.contains("secret"), "{serialised}");

    assert_eq!(
        telemetry::sanitize(&json!({ "hook_event_name": "Stop" }), 0.0)["state"],
        "done"
    );
}

#[test]
fn expired_and_missing_quotas_are_unavailable_never_zero() {
    // A window that has already reset carries no usable information.
    let expired = json!({ "rateLimits": { "primary": { "usedPercent": 12, "resetsAt": 5 } } });
    assert_eq!(rate_windows(&expired, 10.0), Vec::new());

    let open = json!({ "rateLimitsByLimitId": { "codex": { "primary": { "usedPercent": 12 } } } });
    let windows = rate_windows(&open, 0.0);
    assert_eq!(windows[0].percent, 12.0);
    assert_eq!(windows[0].name, "codex primary");
}

#[test]
fn hook_configuration_clears_stale_status_and_stores_only_a_state_word() {
    let fixture = fixture();
    let root = fixture.path().join("telemetry");
    let mut session = Session::new("project", Agent::Claude, "Session");
    session.id = "session-id".into();

    let output = telemetry::output_path(&root, &session.id);
    fs::create_dir_all(&root).unwrap();
    let stale = telemetry::with_suffix(&output, ".status");
    fs::write(&stale, r#"{"state":"done"}"#).unwrap();

    let settings = telemetry::configuration(
        &root,
        &session,
        std::path::Path::new("/usr/bin/convoy"),
        true,
    )
    .unwrap();
    assert!(
        !stale.exists(),
        "old Stop data must not mark a fresh launch complete"
    );

    let document: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(document["hooks"].as_object().unwrap().len(), 8);
    let command = &document["hooks"]["Stop"][0]["hooks"][0];
    assert_eq!(command["command"], "/usr/bin/convoy");
    assert_eq!(command["args"][0], "--hook");
    assert!(document["statusLine"]["command"]
        .as_str()
        .unwrap()
        .contains("--hook"));

    // The helper writes only the state word, never the payload it was given.
    let payload = json!({
        "hook_event_name": "Notification",
        "notification_type": "idle_prompt",
        "prompt": "secret"
    })
    .to_string();
    let printed = hook::run(&output, &mut payload.as_bytes());
    assert_eq!(printed, None, "a status event prints nothing");

    let stored = telemetry::read(&root, &session.id, ".status").expect("status file");
    assert_eq!(stored["state"], "done");
    assert!(!serde_json::to_string(&stored).unwrap().contains("secret"));
}

#[test]
fn hook_usage_mode_prints_a_status_line() {
    let fixture = fixture();
    let output = fixture.path().join("usage");
    let payload = json!({
        "rate_limits": { "five_hour": { "used_percentage": 42.4 } }
    })
    .to_string();
    let printed = hook::run(&output, &mut payload.as_bytes()).expect("status line");
    assert_eq!(printed, "five_hour: 42%");
}

#[test]
fn provider_history_matches_folders_and_excludes_codex_subagents() {
    let fixture = fixture();
    let root = fixture.path();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    let directory = root.join("sessions").join("2026");
    fs::create_dir_all(&directory).unwrap();

    let id = "11111111-1111-4111-8111-111111111111";
    let payload = json!({ "id": id, "cwd": project });
    fs::write(
        directory.join("rollout-a.jsonl"),
        format!(
            "{}\n{}",
            json!({ "type": "session_meta", "payload": payload }),
            json!({ "type": "event_msg", "payload": { "type": "user_message", "message": "Fix parser" } })
        ),
    )
    .unwrap();

    let mut nested = payload.clone();
    nested["parent_thread_id"] = json!("parent");
    fs::write(
        directory.join("rollout-b.jsonl"),
        json!({ "type": "session_meta", "payload": nested }).to_string(),
    )
    .unwrap();

    let found = scan(Agent::Codex, root, &project);
    assert_eq!(
        found.len(),
        1,
        "a subagent rollout is not a session to import"
    );
    assert_eq!(found[0].title, "Fix parser");
    assert_eq!(found[0].provider_id, id);

    assert!(
        scan(Agent::Codex, root, &root.join("other")).is_empty(),
        "conversations from another folder are not offered"
    );
}
