//! Git: status, diffs, history, branches and every write.
//!
//! Every invocation sanitises the environment first: an agent running inside a
//! worktree exports `GIT_DIR` and friends, and inheriting those would silently
//! redirect Convoy's own commands at the wrong repository.

pub mod diff;
pub mod mutate;
pub mod status;

use crate::process::{ProcessRunner, ProcessSpec, StdRunner};
use crate::{ensure, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Repository overrides that must never be inherited from a parent agent.
const STRIPPED: [&str; 6] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
];

#[derive(Clone)]
pub struct Git {
    runner: Arc<dyn ProcessRunner>,
}

impl Default for Git {
    fn default() -> Self {
        Git::new(Arc::new(StdRunner))
    }
}

impl Git {
    pub fn new(runner: Arc<dyn ProcessRunner>) -> Self {
        Git { runner }
    }

    fn environment() -> BTreeMap<String, String> {
        let mut env: BTreeMap<String, String> = std::env::vars()
            .filter(|(key, _)| !STRIPPED.contains(&key.as_str()))
            .collect();
        env.insert("GIT_OPTIONAL_LOCKS".into(), "0".into());
        env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
        env
    }

    /// `git()` — stdout on success, the trimmed stderr as the error otherwise.
    /// `--literal-pathspecs` is always present so a file called `[ab].txt` is
    /// staged as itself rather than as a glob.
    pub fn run(&self, directory: &Path, args: &[&str]) -> Result<String> {
        let mut argv: Vec<String> = vec![
            "--literal-pathspecs".into(),
            "-C".into(),
            directory.to_string_lossy().into_owned(),
        ];
        argv.extend(args.iter().map(|arg| arg.to_string()));
        let spec = ProcessSpec::new("git", argv)
            .env(Self::environment())
            .timeout(Duration::from_secs(60))
            .max_output(2 * 1024 * 1024);
        let output = self.runner.run(&spec)?;
        if output.ok() {
            Ok(output.stdout)
        } else {
            Err(output.failure())
        }
    }

    /// `git(...).then(() => true, () => false)` — for probes where failure is
    /// an expected answer rather than an error.
    pub fn succeeds(&self, directory: &Path, args: &[&str]) -> bool {
        self.run(directory, args).is_ok()
    }

    pub fn is_repository(&self, directory: &Path) -> bool {
        self.succeeds(directory, &["rev-parse", "--is-inside-work-tree"])
    }

    /// `createWorktree()` — a new branch in its own directory, leaving the
    /// original checkout on its current branch.
    pub fn create_worktree(&self, repo: &Path, root: &Path, branch: &str) -> Result<PathBuf> {
        self.create_worktree_from(repo, root, branch, None)
    }

    /// As [`Git::create_worktree`], starting from `base` (a branch, tag or
    /// commit) instead of HEAD when one is given.
    pub fn create_worktree_from(
        &self,
        repo: &Path,
        root: &Path,
        branch: &str,
        base: Option<&str>,
    ) -> Result<PathBuf> {
        let base = base
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("HEAD");
        ensure!(!base.starts_with('-'), "Invalid base ref.");
        ensure!(
            !branch.is_empty() && branch.len() <= 150 && !branch.starts_with('-'),
            "Enter a valid new branch name."
        );
        self.run(repo, &["check-ref-format", "--branch", branch])?;
        self.run(
            repo,
            &["rev-parse", "--verify", &format!("{base}^{{commit}}")],
        )
        .map_err(|_| {
            crate::ConvoyError::message(format!(
                "The base ref {base} does not exist in this repository."
            ))
        })?;
        std::fs::create_dir_all(root)?;
        let directory = root.join(Uuid::new_v4().to_string());
        self.run(
            repo,
            &[
                "worktree",
                "add",
                "--no-track",
                "-b",
                branch,
                &directory.to_string_lossy(),
                base,
            ],
        )?;
        Ok(directory)
    }

    /// `gitStatus()` — branch name and the number of changed entries, with
    /// rename records counted once.
    pub fn status(&self, directory: &Path) -> Result<ShortStatus> {
        let branch = self.run(directory, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let changes = self.run(directory, &["status", "--porcelain=v1", "-z"])?;
        Ok(ShortStatus {
            branch: branch.trim().to_string(),
            changed_files: status::count(&changes),
        })
    }

    /// `snapshot()` — everything the Files & Changes panel shows in one pass.
    /// A folder that is not a repository still lists its files.
    pub fn snapshot(&self, root: &Path) -> Result<Snapshot> {
        if !self.is_repository(root) {
            return Ok(Snapshot {
                branch: "Folder".into(),
                changes: Vec::new(),
                files: crate::files::folder_files(root)?,
                log: Vec::new(),
                branches: Vec::new(),
                upstream: None,
            });
        }
        let raw = self.run(root, &["status", "--porcelain=v1", "-z"])?;
        let branch = match self.run(root, &["symbolic-ref", "--short", "-q", "HEAD"]) {
            Ok(branch) => branch,
            Err(_) => self.run(root, &["rev-parse", "--short", "HEAD"])?,
        };
        let listed = self.run(
            root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
        )?;
        let log = self
            .run(
                root,
                &["log", "-100", "--format=%H%x00%h%x00%s%x00%an%x00%aI"],
            )
            .unwrap_or_default();
        // `refs/remotes/origin/HEAD` is a symbolic ref, and its short name is
        // the bare remote — "origin". Listing it as a branch invites switching
        // to something that is not one, so the symref field is asked for and
        // anything that has one is dropped.
        let branches = self.run(
            root,
            &[
                "for-each-ref",
                "--format=%(symref)%00%(refname:short)",
                "refs/heads",
                "refs/remotes",
            ],
        )?;

        let upstream = self.upstream(root);

        let mut files: Vec<String> = listed
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect();
        files.sort();
        files.dedup();

        Ok(Snapshot {
            branch: branch.trim().to_string(),
            changes: status::parse(&raw),
            files,
            log: log
                .trim()
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| {
                    let mut parts = line.split('\0');
                    Commit {
                        id: parts.next().unwrap_or_default().to_string(),
                        short: parts.next().unwrap_or_default().to_string(),
                        subject: parts.next().unwrap_or_default().to_string(),
                        author: parts.next().unwrap_or_default().to_string(),
                        date: parts.next().unwrap_or_default().to_string(),
                    }
                })
                .collect(),
            branches: branches
                .trim()
                .lines()
                .filter_map(|line| line.split_once('\0'))
                .filter(|(symref, _)| symref.is_empty())
                .map(|(_, name)| name.to_string())
                .filter(|name| !name.is_empty())
                .collect(),
            upstream,
        })
    }

    /// The tracked branch and the counts either side of it.
    fn upstream(&self, root: &Path) -> Option<Upstream> {
        let name = self
            .run(
                root,
                &[
                    "rev-parse",
                    "--abbrev-ref",
                    "--symbolic-full-name",
                    "@{upstream}",
                ],
            )
            .ok()?
            .trim()
            .to_string();
        let counts = self
            .run(
                root,
                &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
            )
            .ok()?;
        let mut parts = counts
            .split_whitespace()
            .map(|part| part.parse::<u32>().ok());
        let behind = parts.next()??;
        let ahead = parts.next()??;
        Some(Upstream {
            name,
            ahead,
            behind,
        })
    }

    /// `read()` — the text shown in the preview pane for one selection.
    pub fn read(&self, root: &Path, request: &ReadRequest) -> Result<String> {
        match request {
            ReadRequest::File { path } | ReadRequest::Untracked { path } => {
                crate::files::preview(root, path)
            }
            ReadRequest::Commit { commit } => self.run(
                root,
                &[
                    "show",
                    "--format=fuller",
                    "--no-ext-diff",
                    "--no-textconv",
                    crate::files::paths::revision(commit)?,
                    "--",
                ],
            ),
            ReadRequest::Staged { path } | ReadRequest::Unstaged { path } => {
                let name = crate::files::paths::relative(path)?;
                let staged = matches!(request, ReadRequest::Staged { .. });
                let mut args = vec!["diff", "--no-ext-diff", "--no-textconv"];
                if staged {
                    args.push("--cached");
                }
                args.push("--");
                args.push(name);
                self.run(root, &args)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadRequest {
    File { path: String },
    Untracked { path: String },
    Staged { path: String },
    Unstaged { path: String },
    Commit { commit: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortStatus {
    pub branch: String,
    pub changed_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub date: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub branch: String,
    pub changes: Vec<status::Change>,
    pub files: Vec<String>,
    pub log: Vec<Commit>,
    pub branches: Vec<String>,
    /// The branch it tracks, and how far each side has moved: commits here
    /// not pushed, and commits there not pulled. `None` without an upstream.
    pub upstream: Option<Upstream>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    pub name: String,
    pub ahead: u32,
    pub behind: u32,
}
