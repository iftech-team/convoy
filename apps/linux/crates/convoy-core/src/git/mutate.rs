//! Port of `mutate()` — every write Convoy performs against a repository.
//!
//! Destructive actions are narrow on purpose: discard restores one path, hunk
//! discard applies one reversed patch and refuses when the diff has moved on,
//! and reset accepts only an object name, never a flag.

use super::diff::{digest, hunks};
use super::{Git, ReadRequest};
use crate::files::paths::{relative, revision};
use crate::{bail, ensure, Result};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Stage {
        path: String,
        original: Option<String>,
    },
    Unstage {
        path: String,
        original: Option<String>,
    },
    StageAll,
    Discard {
        path: String,
    },
    DiscardHunk {
        path: String,
        hunk: usize,
        hash: String,
    },
    Commit {
        message: String,
        amend: bool,
    },
    Fetch,
    Pull,
    Push,
    Switch {
        branch: String,
    },
    Branch {
        branch: String,
    },
    Revert {
        commit: String,
    },
    ResetSoft {
        commit: String,
    },
    ResetMixed {
        commit: String,
    },
}

impl Git {
    pub fn mutate(&self, root: &Path, action: &Action) -> Result<String> {
        match action {
            Action::Stage { path, original } => {
                let mut args = vec!["add", "--"];
                let paths = pathspec(path, original)?;
                args.extend(paths.iter().copied());
                self.run(root, &args)
            }
            Action::Unstage { path, original } => {
                let unborn = !self.succeeds(root, &["rev-parse", "--verify", "HEAD"]);
                let mut args = if unborn {
                    vec!["rm", "--cached", "--"]
                } else {
                    vec!["restore", "--staged", "--"]
                };
                let paths = pathspec(path, original)?;
                args.extend(paths.iter().copied());
                self.run(root, &args)
            }
            Action::StageAll => self.run(root, &["add", "--all"]),
            Action::Discard { path } => {
                self.run(root, &["restore", "--worktree", "--", relative(path)?])
            }
            Action::DiscardHunk { path, hunk, hash } => self.discard_hunk(root, path, *hunk, hash),
            Action::Commit { message, amend } => {
                ensure!(
                    !message.trim().is_empty() && crate::json::utf16_len(message) <= 10_000,
                    "Enter a commit message."
                );
                let mut args = vec!["commit"];
                if *amend {
                    args.push("--amend");
                }
                args.push("-m");
                args.push(message);
                self.run(root, &args)
            }
            Action::Fetch => self.run(root, &["fetch", "--all", "--prune"]),
            Action::Pull => self.run(root, &["pull", "--ff-only"]),
            Action::Push => self.run(root, &["push"]),
            Action::Switch { branch } | Action::Branch { branch } => {
                ensure!(!branch.starts_with('-'), "Invalid branch.");
                self.run(root, &["check-ref-format", "--branch", branch])?;
                let create = matches!(action, Action::Branch { .. });
                let mut args = vec!["switch"];
                if create {
                    args.push("-c");
                }
                args.push(branch);
                self.run(root, &args)
            }
            Action::Revert { commit } => {
                self.run(root, &["revert", "--no-edit", revision(commit)?])
            }
            Action::ResetSoft { commit } => self.run(root, &["reset", "--soft", revision(commit)?]),
            Action::ResetMixed { commit } => {
                self.run(root, &["reset", "--mixed", revision(commit)?])
            }
        }
    }

    fn discard_hunk(&self, root: &Path, path: &str, hunk: usize, hash: &str) -> Result<String> {
        let text = self.read(
            root,
            &ReadRequest::Unstaged {
                path: path.to_string(),
            },
        )?;
        ensure!(
            digest(&text) == hash,
            "The diff changed. Refresh before discarding a hunk."
        );
        let parsed = hunks(&text);
        let structural = text.lines().any(|line| {
            line.starts_with("new file")
                || line.starts_with("deleted file")
                || line.starts_with("rename from")
                || line.starts_with("rename to")
        });
        let Some(patch) = parsed.patches.get(hunk) else {
            bail!("This hunk cannot be discarded separately.")
        };
        ensure!(!structural, "This hunk cannot be discarded separately.");
        let directory = tempfile::Builder::new().prefix("convoy-patch-").tempdir()?;
        let file = directory.path().join("patch.diff");
        std::fs::write(&file, format!("{}{patch}", parsed.header))?;
        let name = file.to_string_lossy().into_owned();
        self.run(root, &["apply", "--check", "--reverse", "--", &name])?;
        self.run(root, &["apply", "--reverse", "--", &name])
    }
}

fn pathspec<'a>(path: &'a str, original: &'a Option<String>) -> Result<Vec<&'a str>> {
    let mut paths = vec![relative(path)?];
    if let Some(original) = original.as_deref().filter(|value| !value.is_empty()) {
        paths.push(relative(original)?);
    }
    Ok(paths)
}
