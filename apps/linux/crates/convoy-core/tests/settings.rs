//! Settings shared with the macOS app: shortcuts, permission flags, the
//! global review template and the new preferences' validation.

mod common;

use common::{fixture, new_session};
use convoy_core::model::{Agent, Session, Settings, SHORTCUT_ACTIONS};
use convoy_core::provider::{permission_flags, session_spec_with};
use convoy_core::shortcuts::{from_accelerator, to_accelerator, DEFAULTS, DESCRIPTIONS};
use convoy_core::workspace::SettingsPatch;
use std::collections::{BTreeMap, HashSet};

#[test]
fn every_action_has_a_description_and_a_distinct_valid_default() {
    let mut seen = HashSet::new();
    let mut seen_there = HashSet::new();
    for action in SHORTCUT_ACTIONS {
        assert!(
            DESCRIPTIONS.iter().any(|(name, _)| name == action),
            "{action} has no description"
        );
        let (_, binding) = DEFAULTS
            .iter()
            .find(|(name, _)| name == action)
            .expect("default");
        assert!(
            to_accelerator(binding).is_some(),
            "{action}: {binding} is not a valid binding"
        );
        assert!(seen.insert(*binding), "{binding} is bound twice");
        // On Linux and Windows `mod` is Control, so `ctrl+x` and `mod+x`
        // are one key there: defaults must differ in that form too.
        let there = binding
            .replacen("mod+ctrl+", "mod+", 1)
            .replacen("ctrl+", "mod+", 1);
        assert!(
            seen_there.insert(there.clone()),
            "{binding} collides with another default as {there} on Linux and Windows"
        );
    }
}

#[test]
fn punctuation_and_arrows_round_trip_through_gtk_names() {
    for stored in [
        "mod+.",
        "mod+/",
        "mod+shift+]",
        "mod+shift+[",
        "mod+\\",
        "mod+alt+left",
        "mod+;",
    ] {
        let accelerator = to_accelerator(stored).expect(stored);
        assert_eq!(
            from_accelerator(&accelerator).as_deref(),
            Some(stored),
            "{accelerator}"
        );
    }
    assert_eq!(to_accelerator("mod+.").as_deref(), Some("<Primary>period"));
    assert_eq!(
        to_accelerator("mod+alt+left").as_deref(),
        Some("<Primary><Alt>Left")
    );
    assert!(
        to_accelerator("mod+space").is_none(),
        "only the listed keys are allowed"
    );
}

#[test]
fn yolo_flags_are_opt_in_per_agent_and_precede_the_prompt() {
    let mut settings = Settings::default();
    assert!(permission_flags(Agent::Claude, &settings).is_empty());
    settings.yolo_claude = true;
    assert_eq!(
        permission_flags(Agent::Claude, &settings),
        ["--dangerously-skip-permissions"]
    );
    assert!(permission_flags(Agent::Codex, &settings).is_empty());
    settings.yolo_codex = true;
    assert_eq!(
        permission_flags(Agent::Codex, &settings),
        ["--dangerously-bypass-approvals-and-sandbox"]
    );

    let mut session = Session::new("project", Agent::Claude, "Session");
    session.provider_id = "id".into();
    session.prompt = "Fix it".into();
    let spec = session_spec_with(
        &session,
        None,
        &BTreeMap::new(),
        &permission_flags(Agent::Claude, &settings),
    );
    let command = spec.args.join(" ");
    let flag = command
        .find("--dangerously-skip-permissions")
        .expect("flag present");
    let prompt = command.find("Fix it").expect("prompt present");
    assert!(
        flag < prompt,
        "the flag goes before the literal prompt: {command}"
    );
}

#[test]
fn new_preferences_are_validated_and_saved() {
    let fixture = fixture();
    let (mut workspace, _) = fixture.with_project();
    workspace
        .save_settings(SettingsPatch {
            worktree_by_default: Some(true),
            branch_prefix: Some(" feature/ ".into()),
            notify_done: Some(false),
            yolo_codex: Some(true),
            ..Default::default()
        })
        .unwrap();
    let saved = convoy_core::Workspace::load(&fixture.file)
        .unwrap()
        .settings()
        .clone();
    assert!(saved.worktree_by_default && saved.yolo_codex && !saved.notify_done);
    assert!(saved.notify_waiting, "defaults to on");
    assert_eq!(
        saved.branch_prefix, "feature",
        "trimmed of spaces and slashes"
    );

    let bad = workspace.save_settings(SettingsPatch {
        branch_prefix: Some("has space".into()),
        ..Default::default()
    });
    assert!(bad.unwrap_err().to_string().contains("branch prefix"));
}

#[test]
fn a_global_review_template_applies_unless_the_project_sets_one() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .add_session(new_session(&project, Agent::Claude, "Builder"))
        .unwrap();
    let id = workspace.state().sessions[0].id.clone();

    workspace
        .save_settings(SettingsPatch {
            review_template: Some("Check the migrations first.".into()),
            ..Default::default()
        })
        .unwrap();
    let brief = convoy_core::review::brief(&workspace, &id, "output").unwrap();
    assert!(brief.starts_with("Check the migrations first."), "{brief}");
}

#[test]
fn control_bindings_follow_the_macos_shapes_but_spare_terminal_keys() {
    use convoy_core::patterns::SHORTCUT;
    for good in [
        "ctrl+tab",
        "ctrl+shift+tab",
        "ctrl+1",
        "mod+ctrl+r",
        "mod+tab",
        "ctrl+alt+left",
    ] {
        assert!(SHORTCUT.is_match(good), "{good} should be allowed");
    }
    for bad in ["ctrl+c", "ctrl+shift+d", "tab", "alt+1", "ctrl", "mod+"] {
        assert!(!SHORTCUT.is_match(bad), "{bad} should be refused");
    }
    assert_eq!(
        to_accelerator("ctrl+shift+tab").as_deref(),
        Some("<Control><Shift>Tab")
    );
    assert_eq!(
        to_accelerator("mod+ctrl+r").as_deref(),
        Some("<Primary><Control>r")
    );
}

#[test]
fn folder_trust_is_added_once_and_never_clobbers_what_it_cannot_read() {
    use convoy_core::trust::{approve_claude, approve_codex};
    let fixture = fixture();
    let claude = fixture.path().join(".claude.json");
    std::fs::write(
        &claude,
        r#"{"theme":"dark","projects":{"/other":{"allowedTools":["Bash"]}}}"#,
    )
    .unwrap();
    assert!(approve_claude(&claude, "/work/app").unwrap());
    assert!(
        !approve_claude(&claude, "/work/app").unwrap(),
        "already trusted"
    );
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&claude).unwrap()).unwrap();
    assert_eq!(saved["theme"], "dark", "the rest of the file is kept");
    assert_eq!(saved["projects"]["/other"]["allowedTools"][0], "Bash");
    assert_eq!(
        saved["projects"]["/work/app"]["hasTrustDialogAccepted"],
        true
    );

    let broken = fixture.path().join("broken.json");
    std::fs::write(&broken, "{ not json").unwrap();
    assert!(!approve_claude(&broken, "/work/app").unwrap());
    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{ not json");

    let codex = fixture.path().join("codex/config.toml");
    assert!(approve_codex(&codex, "/work/app").unwrap());
    assert!(!approve_codex(&codex, "/work/app").unwrap());
    let text = std::fs::read_to_string(&codex).unwrap();
    assert_eq!(text.matches("[projects.\"/work/app\"]").count(), 1);
    assert!(text.contains("trust_level = \"trusted\""));
    assert!(approve_codex(&codex, r"C:\Users\me\app").unwrap());
    assert!(std::fs::read_to_string(&codex)
        .unwrap()
        .contains(r#"[projects."C:\\Users\\me\\app"]"#));
}

#[test]
fn hooks_can_be_turned_off_without_losing_the_usage_line() {
    let fixture = fixture();
    let session = Session::new("project", Agent::Claude, "Session");
    let file = convoy_core::telemetry::configuration_with(
        fixture.path(),
        &session,
        std::path::Path::new("/usr/bin/convoy"),
        true,
        false,
    )
    .unwrap();
    let document: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
    assert!(document.get("hooks").is_none());
    assert!(document.get("statusLine").is_some());
}

#[test]
fn global_worktree_setup_runs_before_the_projects_own() {
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .save_settings(SettingsPatch {
            worktree_setup: Some("pnpm install".into()),
            worktree_shared: Some(".env\nnode_modules\n".into()),
            ..Default::default()
        })
        .unwrap();
    let mut owned = workspace.project(&project).unwrap().clone();
    owned.setup_command = Some("make seed".into());
    owned.shared_paths = Some(".env\n.local".into());
    let (shared, command) = convoy_core::worktree::setup_parts(&workspace, &owned);
    assert_eq!(shared, [".env", "node_modules", ".local"]);
    assert_eq!(command.as_deref(), Some("pnpm install\nmake seed"));
}

#[test]
fn project_settings_follow_the_macos_fields_and_rules() {
    use convoy_core::workspace::ProjectPatch;
    let fixture = fixture();
    let (mut workspace, project) = fixture.with_project();
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                color: Some("#E5A54B".into()),
                icon: Some("sf:cpu".into()),
                default_agent: Some("codex".into()),
                base_ref: Some("main".into()),
                branch_prefix: Some("feature/".into()),
                task_mode: Some("pr".into()),
                auto_run_tasks: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
    let saved = convoy_core::Workspace::load(&fixture.file)
        .unwrap()
        .project(&project)
        .unwrap()
        .clone();
    assert_eq!(saved.color.as_deref(), Some("#E5A54B"));
    assert_eq!(saved.default_agent, Some(Agent::Codex));
    assert_eq!(saved.branch_prefix.as_deref(), Some("feature"));
    assert_eq!(
        (saved.task_mode.as_deref(), saved.auto_run_tasks),
        (Some("pr"), Some(true))
    );

    // Empty values fall back to the global settings.
    workspace
        .edit_project(
            &project,
            ProjectPatch {
                default_agent: Some(String::new()),
                base_ref: Some(" ".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let cleared = workspace.project(&project).unwrap();
    assert_eq!(
        (cleared.default_agent, cleared.base_ref.as_deref()),
        (None, None)
    );

    for bad in [
        ProjectPatch {
            color: Some("orange".into()),
            ..Default::default()
        },
        ProjectPatch {
            default_agent: Some("gpt".into()),
            ..Default::default()
        },
        ProjectPatch {
            base_ref: Some("--upload-pack=x".into()),
            ..Default::default()
        },
        ProjectPatch {
            task_mode: Some("merge".into()),
            ..Default::default()
        },
    ] {
        assert!(workspace.edit_project(&project, bad).is_err());
    }
}

#[cfg(unix)]
#[test]
fn a_worktree_starts_from_the_projects_base_ref() {
    use std::process::Command;
    let fixture = fixture();
    let repo = fixture.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args([
                "-c",
                "user.email=t@e",
                "-c",
                "user.name=t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        String::from_utf8_lossy(&status.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "one").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "one"]);
    git(&["branch", "release"]);
    std::fs::write(repo.join("a.txt"), "two").unwrap();
    git(&["commit", "-qam", "two"]);
    let release = git(&["rev-parse", "release"]);

    let directory = convoy_core::Git::default()
        .create_worktree_from(
            &repo,
            &fixture.path().join("trees"),
            "feature/x",
            Some("release"),
        )
        .unwrap();
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&directory)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), release);
    assert_eq!(
        std::fs::read_to_string(directory.join("a.txt")).unwrap(),
        "one"
    );

    let missing = convoy_core::Git::default()
        .create_worktree_from(
            &repo,
            &fixture.path().join("trees"),
            "feature/y",
            Some("nope"),
        )
        .unwrap_err();
    assert!(missing.to_string().contains("does not exist"), "{missing}");
}

#[test]
fn docs_list_read_and_write_only_markdown_inside_the_project() {
    use convoy_core::docs;
    let fixture = fixture();
    let root = fixture.path().join("repo");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("README.md"), "# Hello").unwrap();
    std::fs::write(root.join("docs/b.md"), "b").unwrap();
    std::fs::write(root.join("docs/a.md"), "a").unwrap();
    std::fs::write(root.join("docs/notes.txt"), "x").unwrap();
    std::fs::write(fixture.path().join("secret.md"), "no").unwrap();

    let spec = docs::create_spec(&root, "Checkout retries for failed cards").unwrap();
    assert_eq!(spec, ".specdesk/specs/checkout-retries-for-failed.md");
    assert!(docs::read(&root, &spec)
        .unwrap()
        .starts_with("# Checkout retries for failed cards\n\n## Problem"));
    docs::write(&root, ".specdesk/PROJECT.md", "# Project").unwrap();
    assert_eq!(
        docs::files(&root),
        [
            ".specdesk/PROJECT.md",
            ".specdesk/specs/checkout-retries-for-failed.md",
            "README.md",
            "docs/a.md",
            "docs/b.md"
        ]
    );
    // A second create keeps what is there.
    docs::write(&root, &spec, "edited").unwrap();
    docs::create_spec(&root, "Checkout retries for failed cards").unwrap();
    assert_eq!(docs::read(&root, &spec).unwrap(), "edited");

    for bad in [
        "../secret.md",
        "/etc/passwd.md",
        "docs/notes.txt",
        "",
        "docs/../../secret.md",
    ] {
        assert!(docs::read(&root, bad).is_err(), "{bad} must be refused");
        assert!(
            docs::write(&root, bad, "x").is_err(),
            "{bad} must be refused"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(fixture.path().join("secret.md"), root.join("docs/link.md"))
            .unwrap();
        assert!(
            docs::read(&root, "docs/link.md").is_err(),
            "a symlink out of the project is refused"
        );
    }
}

/// An action can be left unbound on purpose, and still only one action may
/// have any one key.
#[test]
fn a_shortcut_can_be_unbound_but_not_shared() {
    let dir = tempfile::tempdir().unwrap();
    let mut workspace = convoy_core::Workspace::load(dir.path().join("workspace.json")).unwrap();
    let save = |workspace: &mut convoy_core::Workspace, pairs: &[(&str, &str)]| {
        workspace
            .save_settings(convoy_core::workspace::SettingsPatch {
                shortcuts: Some(
                    pairs
                        .iter()
                        .map(|(a, b)| (a.to_string(), b.to_string()))
                        .collect(),
                ),
                ..Default::default()
            })
            .map(|_| ())
    };
    save(&mut workspace, &[("palette", ""), ("search", "")]).unwrap();
    let resolved = convoy_core::shortcuts::resolve(&workspace.settings().shortcuts);
    let palette = resolved
        .iter()
        .find(|(name, ..)| *name == "palette")
        .unwrap();
    assert_eq!(
        (palette.1.as_str(), palette.2.as_str()),
        ("", ""),
        "unbound stays unbound"
    );
    assert!(save(&mut workspace, &[("palette", "mod+k"), ("search", "mod+k")]).is_err());
}
