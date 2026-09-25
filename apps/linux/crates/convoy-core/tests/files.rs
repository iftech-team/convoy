//! Ported from `test/files.test.cjs` — discovery, previews and every Git write.

mod common;

use common::fixture;
use convoy_core::files::{discover, preview};
use convoy_core::git::diff::digest;
use convoy_core::git::mutate::Action;
use convoy_core::git::{ReadRequest, Snapshot};
use convoy_core::process::StdRunner;
use convoy_core::worktree::setup;
use convoy_core::Git;
use std::fs;
use std::path::Path;

/// A repository with deterministic identity and line-ending handling.
fn repository(path: &Path, autocrlf: &str) -> Git {
    let git = Git::default();
    fs::create_dir_all(path).unwrap();
    git.run(path, &["init"]).unwrap();
    git.run(path, &["config", "core.autocrlf", autocrlf])
        .unwrap();
    git.run(path, &["config", "user.name", "Test"]).unwrap();
    git.run(path, &["config", "user.email", "test@example.invalid"])
        .unwrap();
    git.run(path, &["config", "commit.gpgSign", "false"])
        .unwrap();
    git
}

fn change<'a>(snapshot: &'a Snapshot, name: &str) -> &'a convoy_core::git::status::Change {
    snapshot
        .changes
        .iter()
        .find(|change| change.path == name)
        .unwrap_or_else(|| panic!("no status entry for {name}"))
}

#[test]
fn discovery_stops_at_project_boundaries_and_skips_dependencies() {
    let fixture = fixture();
    for name in ["group/app", "node_modules/ignored"] {
        let folder = fixture.path().join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("package.json"), "{}").unwrap();
    }
    assert_eq!(
        discover(fixture.path()),
        vec![fixture.path().join("group/app")]
    );
}

/// Symlinks are a Unix construct here; the Windows build copies instead.
#[cfg(unix)]
#[test]
fn preview_rejects_traversal_external_symlinks_binary_and_oversized_files() {
    let fixture = fixture();
    let root = fixture.path();
    fs::write(root.join("text"), "hello").unwrap();
    fs::write(root.join("binary"), [0u8, 1u8]).unwrap();
    fs::write(root.join("large"), vec![b'A'; 1024 * 1024 + 1]).unwrap();

    assert_eq!(preview(root, "text").unwrap(), "hello");
    assert!(preview(root, "../outside").is_err());
    assert!(preview(root, "binary").unwrap().contains("Binary"));
    assert!(preview(root, "large").unwrap().contains("exceeds"));

    std::os::unix::fs::symlink(std::env::temp_dir(), root.join("link")).unwrap();
    assert!(
        preview(root, "link").is_err(),
        "a symlink out of the folder must not be readable"
    );
}

#[test]
fn git_panel_handles_unborn_repositories_spaced_paths_stage_commit_and_diffs() {
    let fixture = fixture();
    let root = fixture.path();
    let git = repository(root, "false");
    let name = "space name.txt";
    fs::write(root.join(name), "one\n").unwrap();

    assert!(change(&git.snapshot(root).unwrap(), name).untracked);

    // Unstaging before the first commit must not need a HEAD.
    git.mutate(
        root,
        &Action::Stage {
            path: name.into(),
            original: None,
        },
    )
    .unwrap();
    git.mutate(
        root,
        &Action::Unstage {
            path: name.into(),
            original: None,
        },
    )
    .unwrap();
    assert!(change(&git.snapshot(root).unwrap(), name).untracked);

    git.mutate(root, &Action::StageAll).unwrap();
    git.mutate(
        root,
        &Action::Commit {
            message: "Initial commit".into(),
            amend: false,
        },
    )
    .unwrap();

    fs::write(root.join(name), "two\n").unwrap();
    let diff = git
        .read(root, &ReadRequest::Unstaged { path: name.into() })
        .unwrap();
    assert!(diff.contains("+two"), "{diff}");

    git.mutate(root, &Action::Discard { path: name.into() })
        .unwrap();
    assert_eq!(fs::read_to_string(root.join(name)).unwrap(), "one\n");
    assert_eq!(git.snapshot(root).unwrap().log.len(), 1);

    // A reset target is an object name, never a flag.
    assert!(git
        .mutate(
            root,
            &Action::ResetMixed {
                commit: "--hard".into()
            }
        )
        .is_err());
}

#[test]
fn rename_records_carry_their_original_path() {
    let parsed = convoy_core::git::status::parse("R  new name\0old name\0?? other\0");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].path, "new name");
    assert_eq!(parsed[0].original.as_deref(), Some("old name"));
    assert_eq!(parsed[1].path, "other");
    assert!(parsed[1].untracked);
}

#[test]
fn discard_hunk_rejects_stale_diffs_and_preserves_other_hunks() {
    let fixture = fixture();
    let root = fixture.path();
    let git = repository(root, "false");

    let before: String = (0..30).map(|index| format!("line {index}\n")).collect();
    fs::write(root.join("text"), &before).unwrap();
    git.mutate(root, &Action::StageAll).unwrap();
    git.mutate(
        root,
        &Action::Commit {
            message: "Initial".into(),
            amend: false,
        },
    )
    .unwrap();

    let edited = before
        .replace("line 1\n", "first edit\n")
        .replace("line 25\n", "second edit\n");
    fs::write(root.join("text"), &edited).unwrap();

    let diff = git
        .read(
            root,
            &ReadRequest::Unstaged {
                path: "text".into(),
            },
        )
        .unwrap();
    let stale = git.mutate(
        root,
        &Action::DiscardHunk {
            path: "text".into(),
            hunk: 0,
            hash: "stale".into(),
        },
    );
    assert!(stale.unwrap_err().to_string().contains("changed"));

    git.mutate(
        root,
        &Action::DiscardHunk {
            path: "text".into(),
            hunk: 0,
            hash: digest(&diff),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("text")).unwrap(),
        before.replace("line 25\n", "second edit\n"),
        "only the first hunk is reverted"
    );
}

#[test]
fn staging_a_path_treats_git_wildcard_characters_literally() {
    let fixture = fixture();
    let root = fixture.path();
    let git = repository(root, "false");
    fs::write(root.join("[ab].txt"), "literal").unwrap();
    fs::write(root.join("a.txt"), "other").unwrap();

    git.mutate(
        root,
        &Action::Stage {
            path: "[ab].txt".into(),
            original: None,
        },
    )
    .unwrap();

    let snapshot = git.snapshot(root).unwrap();
    assert_eq!(change(&snapshot, "[ab].txt").index, 'A');
    assert!(
        change(&snapshot, "a.txt").untracked,
        "the glob matched nothing else"
    );
}

/// Symlinks are a Unix construct here; the Windows build copies instead.
#[cfg(unix)]
#[test]
fn shared_file_setup_never_follows_destination_symlinks_or_overwrites_existing_files() {
    let fixture = fixture();
    let root = fixture.path();
    let (repo, target, outside) = (root.join("repo"), root.join("target"), root.join("outside"));
    for folder in [&repo, &target, &outside] {
        fs::create_dir(folder).unwrap();
    }
    fs::write(repo.join("settings"), "source").unwrap();
    fs::write(target.join("settings"), "keep").unwrap();

    let shared = vec!["settings".to_string()];
    assert!(setup(&repo, &target, &shared, None, &StdRunner).is_err());
    assert_eq!(
        fs::read_to_string(target.join("settings")).unwrap(),
        "keep",
        "a tracked file is never replaced"
    );

    fs::create_dir_all(repo.join("linked/nested")).unwrap();
    fs::write(repo.join("linked/nested/file"), "source").unwrap();
    std::os::unix::fs::symlink(&outside, target.join("linked")).unwrap();

    let shared = vec!["linked/nested/file".to_string()];
    assert!(setup(&repo, &target, &shared, None, &StdRunner).is_err());
    assert_eq!(
        fs::read_dir(&outside).unwrap().count(),
        0,
        "nothing was written through the symlink"
    );
}

#[test]
fn discard_and_hunk_discard_respect_git_crlf_checkout_settings() {
    let fixture = fixture();
    let root = fixture.path();
    let git = repository(root, "true");

    let before: String = (0..30).map(|index| format!("line {index}\r\n")).collect();
    fs::write(root.join("text.txt"), &before).unwrap();
    git.mutate(root, &Action::StageAll).unwrap();
    git.mutate(
        root,
        &Action::Commit {
            message: "CRLF fixture".into(),
            amend: false,
        },
    )
    .unwrap();

    fs::write(root.join("text.txt"), "changed\r\n").unwrap();
    git.mutate(
        root,
        &Action::Discard {
            path: "text.txt".into(),
        },
    )
    .unwrap();
    assert_eq!(fs::read_to_string(root.join("text.txt")).unwrap(), before);

    let edited = before
        .replace("line 1\r\n", "first edit\r\n")
        .replace("line 25\r\n", "second edit\r\n");
    fs::write(root.join("text.txt"), &edited).unwrap();
    let diff = git
        .read(
            root,
            &ReadRequest::Unstaged {
                path: "text.txt".into(),
            },
        )
        .unwrap();
    git.mutate(
        root,
        &Action::DiscardHunk {
            path: "text.txt".into(),
            hunk: 0,
            hash: digest(&diff),
        },
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("text.txt")).unwrap(),
        before.replace("line 25\r\n", "second edit\r\n")
    );
}

/// `refs/remotes/origin/HEAD` is a symbolic ref whose short name is the bare
/// remote. Offering "origin" as a branch offers to switch to something that is
/// not one.
#[test]
fn the_branch_list_leaves_out_the_remote_head() {
    let fixture = fixture();
    let root = fixture.path().join("repo");
    let git = repository(&root, "false");
    fs::write(root.join("a.txt"), "one").unwrap();
    git.run(&root, &["add", "-A"]).unwrap();
    git.run(&root, &["commit", "-m", "first"]).unwrap();
    git.run(&root, &["branch", "-M", "main"]).unwrap();

    // A remote that exists only as refs, which is what a clone leaves behind.
    git.run(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"])
        .unwrap();
    git.run(
        &root,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    )
    .unwrap();

    let snapshot = git.snapshot(&root).unwrap();
    assert!(
        snapshot.branches.contains(&"main".to_string())
            && snapshot.branches.contains(&"origin/main".to_string()),
        "real branches are missing from {:?}",
        snapshot.branches
    );
    assert!(
        !snapshot.branches.contains(&"origin".to_string()),
        "the remote head was listed as a branch: {:?}",
        snapshot.branches
    );
}

/// Ahead and behind are counted against the tracked branch, and unstaging
/// everything works whether or not anything is committed yet.
#[test]
fn upstream_counts_and_unstaging_everything() {
    let fixture = fixture();
    let origin = fixture.path().join("origin");
    let git = repository(&origin, "false");
    fs::write(origin.join("a.txt"), "a").unwrap();
    // Before the first commit, unstaging everything drops the index entries.
    git.mutate(&origin, &Action::StageAll).unwrap();
    git.mutate(&origin, &Action::UnstageAll).unwrap();
    assert!(git
        .snapshot(&origin)
        .unwrap()
        .changes
        .iter()
        .all(|change| change.untracked));
    git.mutate(&origin, &Action::StageAll).unwrap();
    git.mutate(
        &origin,
        &Action::Commit {
            message: "one".into(),
            amend: false,
        },
    )
    .unwrap();
    assert_eq!(
        git.snapshot(&origin).unwrap().upstream,
        None,
        "nothing is tracked yet"
    );

    let clone = fixture.path().join("clone");
    git.run(fixture.path(), &["clone", "-q", "origin", "clone"])
        .unwrap();
    let local = repository(&clone, "false");
    fs::write(clone.join("b.txt"), "b").unwrap();
    local.mutate(&clone, &Action::StageAll).unwrap();
    local
        .mutate(
            &clone,
            &Action::Commit {
                message: "two".into(),
                amend: false,
            },
        )
        .unwrap();
    fs::write(origin.join("c.txt"), "c").unwrap();
    git.mutate(&origin, &Action::StageAll).unwrap();
    git.mutate(
        &origin,
        &Action::Commit {
            message: "three".into(),
            amend: false,
        },
    )
    .unwrap();
    local.mutate(&clone, &Action::Fetch).unwrap();

    let upstream = local
        .snapshot(&clone)
        .unwrap()
        .upstream
        .expect("the clone tracks origin");
    assert_eq!((upstream.ahead, upstream.behind), (1, 1));
    assert!(upstream.name.starts_with("origin/"));

    // After a commit, unstaging everything leaves the change in the worktree.
    fs::write(clone.join("b.txt"), "changed").unwrap();
    local.mutate(&clone, &Action::StageAll).unwrap();
    local.mutate(&clone, &Action::UnstageAll).unwrap();
    let snapshot = local.snapshot(&clone).unwrap();
    let entry = change(&snapshot, "b.txt");
    assert_eq!((entry.index, entry.worktree), (' ', 'M'));
}

/// A branch can start from any base, not only from where HEAD is.
#[test]
fn a_branch_starts_from_the_chosen_base() {
    let fixture = fixture();
    let root = fixture.path();
    let git = repository(root, "false");
    fs::write(root.join("a.txt"), "one").unwrap();
    git.mutate(root, &Action::StageAll).unwrap();
    git.mutate(
        root,
        &Action::Commit {
            message: "one".into(),
            amend: false,
        },
    )
    .unwrap();
    git.run(root, &["tag", "v1"]).unwrap();
    fs::write(root.join("a.txt"), "two").unwrap();
    git.mutate(root, &Action::StageAll).unwrap();
    git.mutate(
        root,
        &Action::Commit {
            message: "two".into(),
            amend: false,
        },
    )
    .unwrap();

    git.mutate(
        root,
        &Action::BranchFrom {
            branch: "hotfix".into(),
            base: "v1".into(),
        },
    )
    .unwrap();
    assert_eq!(git.snapshot(root).unwrap().branch, "hotfix");
    assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "one");
    assert!(git
        .mutate(
            root,
            &Action::BranchFrom {
                branch: "other".into(),
                base: "--orphan".into()
            }
        )
        .is_err());
    assert!(git
        .mutate(
            root,
            &Action::BranchFrom {
                branch: "other".into(),
                base: "missing".into()
            }
        )
        .is_err());
}
