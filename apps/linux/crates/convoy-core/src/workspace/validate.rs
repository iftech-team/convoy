//! Full port of `validate()`, `validateSettings()` and `validatePlanning()`.
//!
//! Validation runs against the raw `serde_json::Value`, not the typed model,
//! for two reasons: a file on disk can contain anything, and every rejection
//! message must match the Electron build word for word — these strings are
//! shown to the user and several are asserted by the ported tests.

use crate::json::{
    array, boolean, integer, nonempty, safe_integer, string, truthy, truthy_opt, utf16_len,
};
use crate::patterns::{SHORTCUT, UUID};
use crate::{bail, ensure, Result};
use serde_json::Value;
use std::collections::HashSet;
use std::path::Path;

pub const DAMAGED: &str =
    "Unsupported or damaged desktop workspace. The file has not been changed.";

const SPEC_FIELDS: [&str; 6] = [
    "title",
    "problem",
    "requirements",
    "acceptance",
    "constraints",
    "plan",
];
const TASK_STATUSES: [&str; 6] = ["queued", "building", "review", "changes", "done", "failed"];
const AGENTS: [&str; 2] = ["claude", "codex"];
const ACTIVITY_KINDS: [&str; 7] = [
    "started",
    "resumed",
    "exited",
    "done",
    "waiting",
    "hibernated",
    "worktree",
];

/// `text()` from planning.cjs: a present string within `max` UTF-16 units,
/// optionally non-blank.
fn text(value: Option<&Value>, max: usize, required: bool) -> Result<()> {
    let Some(value) = string(value) else {
        bail!("Invalid or missing text.")
    };
    if utf16_len(value) > max || (required && value.trim().is_empty()) {
        bail!("Invalid or missing text.")
    }
    Ok(())
}

fn field<'a>(item: &'a Value, key: &str) -> Option<&'a Value> {
    item.get(key)
}

fn is_one_of(value: Option<&Value>, allowed: &[&str]) -> bool {
    string(value).is_some_and(|text| allowed.contains(&text))
}

fn absolute(value: Option<&Value>) -> bool {
    string(value).is_some_and(|text| Path::new(text).is_absolute())
}

pub fn validate(state: &Value) -> Result<()> {
    let schema = state.get("schemaVersion").and_then(Value::as_f64);
    let supported = schema.is_some_and(|value| value == 1.0 || value == 2.0 || value == 3.0);
    let (Some(projects), Some(sessions)) =
        (array(state.get("projects")), array(state.get("sessions")))
    else {
        bail!("{DAMAGED}")
    };
    ensure!(truthy(state) && supported, "{DAMAGED}");

    let mut ids: HashSet<&str> = HashSet::new();
    for item in projects.iter().chain(sessions.iter()) {
        let invalid = !truthy(item)
            || string(field(item, "title")).is_none()
            || match string(field(item, "id")) {
                Some(id) => !ids.insert(id),
                None => true,
            };
        ensure!(!invalid, "Invalid workspace records.");
    }

    let project_ids: HashSet<&str> = projects
        .iter()
        .filter_map(|project| string(field(project, "id")))
        .collect();

    for project in projects {
        ensure!(absolute(field(project, "path")), "Invalid project path.");
    }

    for session in sessions {
        let belongs =
            string(field(session, "projectID")).is_some_and(|id| project_ids.contains(id));
        ensure!(
            is_one_of(field(session, "agent"), &AGENTS)
                && string(field(session, "providerID")).is_some()
                && string(field(session, "prompt")).is_some()
                && boolean(field(session, "started")).is_some()
                && belongs,
            "Invalid session record."
        );
        if let Some(provider) = nonempty(field(session, "providerID")) {
            ensure!(UUID.is_match(provider), "Invalid provider session ID.");
        }
        if string(field(session, "agent")) == Some("claude") {
            ensure!(
                truthy_opt(field(session, "providerID")),
                "Missing Claude session ID."
            );
        }
        for key in ["notes", "branch"] {
            if let Some(value) = field(session, key) {
                ensure!(value.is_string(), "Invalid session metadata.");
            }
        }
        for key in ["archived", "pinned"] {
            if let Some(value) = field(session, key) {
                ensure!(value.is_boolean(), "Invalid session flag.");
            }
        }
        if field(session, "workingDirectory").is_some() {
            ensure!(
                absolute(field(session, "workingDirectory")),
                "Invalid worktree path."
            );
        }
        if field(session, "agentHome").is_some() {
            ensure!(
                absolute(field(session, "agentHome")),
                "Invalid account path."
            );
        }
        if let Some(review_of) = nonempty(field(session, "reviewOf")) {
            let id = string(field(session, "id"));
            let project = string(field(session, "projectID"));
            let linked = sessions.iter().any(|other| {
                string(field(other, "id")) == Some(review_of)
                    && string(field(other, "id")) != id
                    && string(field(other, "projectID")) == project
            });
            ensure!(linked, "Invalid review relationship.");
        }
    }

    if truthy_opt(state.get("settings")) {
        validate_settings(&state["settings"])?;
    }

    if let Some(commands) = state.get("quickCommands") {
        let Some(commands) = commands.as_array() else {
            bail!("Invalid quick commands.")
        };
        let mut command_ids: HashSet<&str> = HashSet::new();
        for command in commands {
            let title = string(field(command, "title"));
            let body = string(field(command, "text"));
            let invalid = !truthy(command)
                || match string(field(command, "id")) {
                    Some(id) => !command_ids.insert(id),
                    None => true,
                }
                || !title.is_some_and(|value| !value.trim().is_empty() && utf16_len(value) <= 200)
                || !body
                    .is_some_and(|value| !value.trim().is_empty() && utf16_len(value) <= 32_000)
                || boolean(field(command, "submit")).is_none()
                || nonempty(field(command, "projectID"))
                    .is_some_and(|id| !project_ids.contains(id));
            ensure!(!invalid, "Invalid quick command.");
        }
    }

    validate_planning(state)
}

pub fn validate_settings(settings: &Value) -> Result<()> {
    let font_size = integer(settings.get("fontSize"));
    let scrollback = integer(settings.get("scrollback"));
    ensure!(
        font_size.is_some_and(|value| (10.0..=24.0).contains(&value))
            && scrollback.is_some_and(|value| (1000.0..=50_000.0).contains(&value))
            && is_one_of(settings.get("theme"), &["dark", "light", "system"])
            && is_one_of(settings.get("defaultAgent"), &AGENTS),
        "Invalid settings."
    );
    if let Some(value) = settings.get("claudeUsage") {
        ensure!(value.is_boolean(), "Invalid Claude usage setting.");
    }
    if let Some(shortcuts) = settings.get("shortcuts") {
        let valid = shortcuts.as_object().is_some_and(|map| {
            let allowed = crate::workspace::model::SHORTCUT_ACTIONS;
            let mut seen: HashSet<&str> = HashSet::new();
            map.iter().all(|(key, value)| {
                allowed.contains(&key.as_str())
                    && value
                        .as_str()
                        .is_some_and(|accel| SHORTCUT.is_match(accel) && seen.insert(accel))
            })
        });
        ensure!(valid, "Invalid or duplicate shortcut.");
    }
    if settings.get("keepAwake").is_some() {
        ensure!(
            is_one_of(settings.get("keepAwake"), &["off", "always", "sessions"]),
            "Invalid keep-awake setting."
        );
    }
    if settings.get("hibernateMinutes").is_some() {
        ensure!(
            integer(settings.get("hibernateMinutes"))
                .is_some_and(|value| (0.0..=1440.0).contains(&value)),
            "Invalid hibernation delay."
        );
    }
    if let Some(value) = settings.get("notifications") {
        ensure!(value.is_boolean(), "Invalid notification setting.");
    }
    Ok(())
}

pub fn validate_planning(state: &Value) -> Result<()> {
    // Deliberately a second, independent id space: planning.cjs does not share
    // the set built for projects and sessions, so a spec may reuse a project id.
    let mut ids: HashSet<&str> = HashSet::new();
    for collection in ["specs", "tasks", "profiles", "activity"] {
        let Some(value) = state.get(collection) else {
            continue;
        };
        let Some(items) = value.as_array() else {
            bail!("Invalid {collection}.")
        };
        for item in items {
            let invalid = !truthy(item)
                || match string(field(item, "id")) {
                    Some(id) => !ids.insert(id),
                    None => true,
                };
            ensure!(!invalid, "Invalid {collection} record.");
        }
    }

    let empty: Vec<Value> = Vec::new();
    let projects = array(state.get("projects")).unwrap_or(&empty);
    let sessions = array(state.get("sessions")).unwrap_or(&empty);
    let specs = array(state.get("specs")).unwrap_or(&empty);
    let tasks = array(state.get("tasks")).unwrap_or(&empty);
    let profiles = array(state.get("profiles")).unwrap_or(&empty);
    let activity = array(state.get("activity")).unwrap_or(&empty);
    let has_specs = state.get("specs").is_some();
    let has_tasks = state.get("tasks").is_some();
    let has_profiles = state.get("profiles").is_some();
    let project_ids: HashSet<&str> = projects
        .iter()
        .filter_map(|project| string(field(project, "id")))
        .collect();

    for spec in specs {
        for key in SPEC_FIELDS {
            let max = if key == "title" { 200 } else { 16_000 };
            text(field(spec, key), max, key == "title")?;
        }
        let revision = safe_integer(field(spec, "revision"));
        let approved = field(spec, "approvedRevision");
        let belongs = string(field(spec, "projectID")).is_some_and(|id| project_ids.contains(id));
        ensure!(
            belongs
                && revision.is_some_and(|value| value >= 1.0)
                && match approved {
                    None => true,
                    Some(value) => value.as_f64() == revision,
                },
            "Invalid specification revision."
        );
    }

    for task in tasks {
        text(field(task, "title"), 200, true)?;
        text(field(task, "details"), 16_000, false)?;
        text(field(task, "findings"), 16_000, false)?;
        let belongs = string(field(task, "projectID")).is_some_and(|id| project_ids.contains(id));
        let spec_ok = match nonempty(field(task, "specID")) {
            None => true,
            Some(spec_id) => {
                has_specs
                    && specs.iter().any(|spec| {
                        string(field(spec, "id")) == Some(spec_id)
                            && string(field(spec, "projectID")) == string(field(task, "projectID"))
                    })
            }
        };
        let session_ok = match nonempty(field(task, "sessionID")) {
            None => true,
            Some(session_id) => sessions.iter().any(|session| {
                string(field(session, "id")) == Some(session_id)
                    && string(field(session, "taskID")) == string(field(task, "id"))
            }),
        };
        ensure!(
            is_one_of(field(task, "status"), &TASK_STATUSES)
                && is_one_of(field(task, "agent"), &AGENTS)
                && belongs
                && spec_ok
                && session_ok,
            "Invalid task."
        );
        if field(task, "specRevision").is_some() {
            ensure!(
                safe_integer(field(task, "specRevision")).is_some_and(|value| value >= 1.0),
                "Invalid task revision."
            );
        }
    }

    for profile in profiles {
        text(field(profile, "label"), 100, true)?;
        ensure!(
            is_one_of(field(profile, "agent"), &AGENTS),
            "Invalid account provider."
        );
    }

    for session in sessions {
        if let Some(profile_id) = nonempty(field(session, "profileID")) {
            let linked = has_profiles
                && profiles.iter().any(|profile| {
                    string(field(profile, "id")) == Some(profile_id)
                        && string(field(profile, "agent")) == string(field(session, "agent"))
                });
            ensure!(linked, "Invalid session account.");
        }
        if let Some(task_id) = nonempty(field(session, "taskID")) {
            let linked = has_tasks
                && tasks.iter().any(|task| {
                    string(field(task, "id")) == Some(task_id)
                        && string(field(task, "projectID")) == string(field(session, "projectID"))
                });
            ensure!(linked, "Invalid task session.");
        }
    }

    ensure!(activity.len() <= 200, "Invalid activity history.");
    for item in activity {
        text(field(item, "title"), 200, false)?;
        text(field(item, "detail"), 2_000, false)?;
        let known_session = sessions
            .iter()
            .any(|session| string(field(session, "id")) == string(field(item, "sessionID")));
        ensure!(
            is_one_of(field(item, "kind"), &ACTIVITY_KINDS)
                && string(field(item, "at"))
                    .is_some_and(|at| crate::time::parse_iso8601(at).is_some())
                && known_session,
            "Invalid activity event."
        );
    }

    Ok(())
}
