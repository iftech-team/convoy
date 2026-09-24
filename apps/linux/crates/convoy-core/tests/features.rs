//! Ported from `test/features.test.cjs` — migration, review handoff, settings,
//! saved output and real worktrees.

mod common;

use common::{fixture, new_session, same_state};
use convoy_core::history::{paste, review_brief, History};
use convoy_core::workspace::model::{Agent, QuickCommand, Settings, Theme};
use convoy_core::workspace::{SessionPatch, SettingsPatch, Workspace};
use convoy_core::Git;
use serde_json::json;
use std::fs;
use std::path::Path;

fn seeded() -> (common::Fixture, Workspace, String) {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    let mut session = new_session(&project, Agent::Claude, "Builder");
    session.prompt = "Fix bug".into();
    workspace.add_session(session).unwrap();
    let builder = workspace.state().sessions[0].id.clone();
    (fixture, workspace, builder)
}

#[test]
fn schema_1_migrates_without_losing_provider_ids_or_unknown_data() {
    let (fixture, workspace, builder) = seeded();
    let provider_id = workspace.session(&builder).unwrap().provider_id.clone();

    // A document written by an older preview: no settings, no quick commands,
    // and a field this build has never heard of.
    let mut legacy = serde_json::to_value(workspace.state()).unwrap();
    let object = legacy.as_object_mut().unwrap();
    object.insert("schemaVersion".into(), json!(1));
    object.insert("customField".into(), json!({ "preserved": true }));
    object.remove("settings");
    object.remove("quickCommands");
    fs::write(&fixture.file, serde_json::to_string(&legacy).unwrap()).unwrap();

    let mut migrated = Workspace::load(&fixture.file).unwrap();
    assert_eq!(migrated.state().schema_version, 3);
    assert_eq!(
        serde_json::to_value(migrated.settings()).unwrap(),
        serde_json::to_value(Settings::default()).unwrap()
    );
    assert_eq!(migrated.session(&builder).unwrap().provider_id, provider_id);

    // The unknown field must survive a write, or the Electron build would lose
    // data the moment this one saves.
    migrated
        .edit_session(
            &builder,
            SessionPatch {
                notes: Some("Updated".into()),
                ..SessionPatch::default()
            },
            false,
        )
        .unwrap();
    let reloaded = Workspace::load(&fixture.file).unwrap();
    assert_eq!(
        reloaded.state().unknown.get("customField"),
        Some(&json!({ "preserved": true }))
    );
}

#[test]
fn review_handoff_preserves_folder_branch_source_link_and_uses_a_new_provider_identity() {
    let (fixture, mut workspace, builder) = seeded();
    let checkout = fixture.path().join("checkout");
    workspace
        .update(|state| {
            state.sessions[0].working_directory = Some(checkout.clone());
            state.sessions[0].branch = Some("task/fix".into());
            state.sessions[0].owns_worktree = Some(true);
            Ok(())
        })
        .unwrap();

    let source = workspace.session(&builder).unwrap().clone();
    let mut review = new_session(&source.project_id, Agent::Codex, "Review");
    review.prompt = review_brief(&source, "\x1b[32mDone\x1b[0m");
    review.review_of = Some(builder.clone());
    workspace.add_session(review).unwrap();

    let stored = Workspace::load(&fixture.file).unwrap();
    let review = stored.state().sessions[1].clone();
    assert_eq!(review.review_of.as_deref(), Some(builder.as_str()));
    assert_eq!(review.working_directory.as_deref(), Some(checkout.as_path()));
    assert_eq!(review.branch.as_deref(), Some("task/fix"));
    assert_eq!(review.owns_worktree, None, "a review never owns the worktree");
    assert_eq!(review.provider_id, "", "Codex gets its identity from the CLI");
    assert!(review.prompt.contains("Do not edit files"));
    assert!(!review.prompt.contains('\x1b'), "escape sequences are stripped");
}

#[test]
fn running_sessions_cannot_be_archived_or_rebound_but_notes_and_pinning_can_change() {
    let (_fixture, mut workspace, builder) = seeded();
    let provider_id = workspace.session(&builder).unwrap().provider_id.clone();

    let archived = SessionPatch {
        archived: Some(true),
        ..SessionPatch::default()
    };
    let error = workspace.edit_session(&builder, archived, true).unwrap_err();
    assert!(error.to_string().contains("Stop"), "{error}");

    let rebound = SessionPatch {
        provider_id: Some(provider_id.clone()),
        ..SessionPatch::default()
    };
    let error = workspace.edit_session(&builder, rebound, true).unwrap_err();
    assert!(error.to_string().contains("Stop"), "{error}");

    // A running session can still be renamed, pinned and annotated. The
    // working directory is not in `SessionPatch` at all, so the Electron test
    // for an unknown key has no counterpart here.
    workspace
        .edit_session(
            &builder,
            SessionPatch {
                title: Some("Updated".into()),
                notes: Some("Important".into()),
                pinned: Some(true),
                ..SessionPatch::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(workspace.session(&builder).unwrap().pinned, Some(true));

    for archived in [true, false] {
        workspace
            .edit_session(
                &builder,
                SessionPatch {
                    archived: Some(archived),
                    ..SessionPatch::default()
                },
                false,
            )
            .unwrap();
    }
    assert_eq!(workspace.session(&builder).unwrap().provider_id, provider_id);
}

#[test]
fn settings_and_scoped_commands_persist_and_invalid_changes_are_atomic() {
    let (fixture, mut workspace, builder) = seeded();
    let project = workspace.session(&builder).unwrap().project_id.clone();

    workspace
        .save_settings(SettingsPatch {
            theme: Some(Theme::Light),
            font_size: Some(18),
            ..SettingsPatch::default()
        })
        .unwrap();
    workspace
        .save_command(QuickCommand {
            id: String::new(),
            title: "Check".into(),
            text: "Run the test suite".into(),
            submit: false,
            project_id: Some(project.clone()),
            unknown: Default::default(),
        })
        .unwrap();

    let saved = Workspace::load(&fixture.file).unwrap();
    assert_eq!(saved.settings().font_size, 18);
    assert_eq!(saved.state().quick_commands[0].project_id.as_deref(), Some(project.as_str()));

    assert!(workspace
        .save_settings(SettingsPatch {
            scrollback: Some(999_999),
            ..SettingsPatch::default()
        })
        .is_err());
    assert!(workspace
        .save_command(QuickCommand {
            id: String::new(),
            title: "Oops".into(),
            text: "text".into(),
            submit: false,
            project_id: Some("missing".into()),
            unknown: Default::default(),
        })
        .is_err());

    let after = Workspace::load(&fixture.file).unwrap();
    assert!(same_state(after.state(), saved.state()));
}

#[test]
fn snapshots_remain_bounded_plain_text_and_ids_cannot_escape_the_history_directory() {
    let fixture = fixture();
    let history = History::new(fixture.path().join("history"));
    let id = "../outside";

    let mut text = "x".repeat(60_000);
    text.push_str("\x1b[31mred\x1b[0m");
    history.save(id, &text).unwrap();

    let stored = history.read(id).unwrap();
    assert_eq!(stored.chars().count(), 48_000);
    assert!(stored.ends_with("red"), "colour codes are removed, the text stays");
    assert!(
        !fixture.path().join("outside").exists(),
        "the id is hashed, so it cannot address a path"
    );
}

#[test]
fn feedback_paste_strips_control_sequences_and_does_not_press_enter() {
    let text = "hello\x1b[201~\r\x03\x00\nworld";
    assert_eq!(paste(text, false).unwrap(), "\x1b[200~hello\nworld\x1b[201~");
    assert_eq!(paste("test", true).unwrap(), "\x1b[200~test\x1b[201~\r");
}

#[test]
fn real_worktrees_isolate_changes_reject_invalid_branches_and_refuse_dirty_removal() {
    let fixture = fixture();
    let git = Git::default();
    let repo = fixture.path().join("repo");
    fs::create_dir(&repo).unwrap();
    git.run(&repo, &["init"]).unwrap();
    git.run(
        &repo,
        &[
            "-c", "user.name=Convoy Test",
            "-c", "user.email=test@example.invalid",
            "-c", "commit.gpgSign=false",
            "commit", "--allow-empty", "-m", "initial",
        ],
    )
    .unwrap();
    let original = git
        .run(&repo, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let root = fixture.path().join("worktrees");

    assert!(git.create_worktree(&repo, &root, "--detach").is_err());
    assert!(git.create_worktree(&repo, &root, "bad branch").is_err());

    let checkout = git.create_worktree(&repo, &root, "convoy/test").unwrap();
    assert_eq!(git.status(&checkout).unwrap().branch, "convoy/test");
    assert_eq!(
        git.run(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap().trim(),
        original,
        "the original checkout stays on its branch"
    );

    let untracked = checkout.join("untracked.txt");
    fs::write(&untracked, "do not delete").unwrap();
    assert_eq!(git.status(&checkout).unwrap().changed_files, 1);

    // Removal is clean-only: an uncommitted file is never discarded silently.
    let path = checkout.to_string_lossy().into_owned();
    assert!(git.run(&repo, &["worktree", "remove", &path]).is_err());
    assert_eq!(fs::read_to_string(&untracked).unwrap(), "do not delete");

    fs::remove_file(&untracked).unwrap();
    git.run(&repo, &["worktree", "remove", &path]).unwrap();
    assert!(!Path::new(&path).exists());
    assert!(
        git.run(&repo, &["branch", "--list", "convoy/test"]).unwrap().contains("convoy/test"),
        "the branch is retained after the worktree is removed"
    );
}
