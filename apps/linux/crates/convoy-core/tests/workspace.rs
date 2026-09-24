//! Ported from `test/core.test.cjs` — storage guarantees.

mod common;

use common::{fixture, is_root, new_session, same_state};
use convoy_core::patterns::is_uuid;
use convoy_core::workspace::model::Agent;
use convoy_core::workspace::Workspace;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn projects_and_sessions_survive_relaunch_and_duplicate_folders_are_ignored() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace.add_project(fixture.path()).unwrap();
    assert_eq!(workspace.state().projects.len(), 1);

    let mut session = new_session(&project, Agent::Claude, "Build it");
    session.prompt = "Hello".into();
    workspace.add_session(session).unwrap();

    let reloaded = Workspace::load(&fixture.file).unwrap();
    assert!(same_state(reloaded.state(), workspace.state()));
    assert!(is_uuid(&workspace.state().sessions[0].provider_id));
}

#[test]
fn corrupt_and_future_workspace_files_are_never_overwritten() {
    let fixture = fixture();
    for value in [
        "broken",
        r#"{"schemaVersion":999,"projects":[],"sessions":[]}"#,
        r#"{"schemaVersion":1,"projects":[null],"sessions":[]}"#,
    ] {
        fs::write(&fixture.file, value).unwrap();
        assert!(
            Workspace::load(&fixture.file).is_err(),
            "accepted a damaged file: {value}"
        );
        assert_eq!(fs::read_to_string(&fixture.file).unwrap(), value);
    }
}

#[test]
fn invalid_mutations_leave_both_memory_and_disk_untouched() {
    let fixture = fixture();
    let (mut workspace, _) = fixture.with_project();
    let before = fs::read_to_string(&fixture.file).unwrap();

    let error = workspace
        .add_session(new_session("missing", Agent::Codex, "Oops"))
        .unwrap_err();
    assert_eq!(error.to_string(), "Invalid session record.");
    assert_eq!(fs::read_to_string(&fixture.file).unwrap(), before);
    assert_eq!(workspace.state().sessions.len(), 0);
}

/// Made read-only through the Unix permission bits; Windows has no one-line
/// equivalent, and the rule under test is the same on both.
#[cfg(unix)]
#[test]
fn failed_writes_do_not_commit_mutations_in_memory() {
    if is_root() {
        return;
    }
    let fixture = fixture();
    let folder = fixture.path().join("state");
    fs::create_dir(&folder).unwrap();
    let file = folder.join("workspace.json");
    let mut workspace = Workspace::load(&file).unwrap();
    workspace.add_project(fixture.path()).unwrap();
    let project = workspace.state().projects[0].id.clone();

    // A read-only directory cannot receive the temporary file.
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o500)).unwrap();
    let result = workspace.add_session(new_session(&project, Agent::Codex, "Oops"));
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();

    assert!(result.is_err());
    assert_eq!(workspace.state().sessions.len(), 0);
    assert_eq!(
        Workspace::load(&file).unwrap().state().sessions.len(),
        0,
        "a rejected write must not reach the file"
    );
}

#[test]
fn a_recovery_session_is_fresh_but_keeps_its_folder() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .add_session(new_session(&project, Agent::Claude, "Builder"))
        .unwrap();
    let original = workspace.state().sessions[0].clone();

    workspace.recover_session(&original.id).unwrap();
    let fresh = workspace.state().sessions[1].clone();
    assert!(fresh.title.ends_with(" · recovery"));
    assert!(!fresh.started);
    assert_ne!(fresh.provider_id, original.provider_id);
    assert!(is_uuid(&fresh.provider_id));
    assert_eq!(fresh.prompt, "");
    assert_eq!(fresh.owns_worktree, Some(false));
}
