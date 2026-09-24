//! Port of `worktree-setup.cjs`: copying shared files into a fresh worktree
//! and running its optional setup command.
//!
//! Three rules matter here and each one is load-bearing:
//! the source must stay inside the project, every destination directory is
//! checked for symlinks before it is used, and an existing file is never
//! overwritten — a shared `.env` must not clobber a tracked file.

use crate::files::paths::relative;
use crate::process::{ProcessRunner, ProcessSpec};
use crate::{bail, ensure, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub fn setup(
    repo: &Path,
    destination: &Path,
    shared_paths: &[String],
    command: Option<&str>,
    runner: &dyn ProcessRunner,
) -> Result<()> {
    let root = fs::canonicalize(repo)?;
    let target = fs::canonicalize(destination)?;

    for name in shared_paths {
        relative(name)?;
        ensure!(
            !name.split(['/', '\\']).any(|part| part == ".git"),
            "Git metadata cannot be shared."
        );
        let source = fs::canonicalize(root.join(name))?;
        ensure!(
            source.starts_with(&root) && source != root,
            "Shared path points outside the project."
        );
        let output = target.join(name);
        let parent = output
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| target.clone());

        // Create each intermediate directory one level at a time, refusing to
        // step through a symlink someone left in the worktree.
        let mut checked = target.clone();
        let suffix = parent.strip_prefix(&target).unwrap_or(Path::new(""));
        for part in suffix.components() {
            checked = checked.join(part);
            match fs::symlink_metadata(&checked) {
                Ok(metadata) => {
                    ensure!(
                        metadata.is_dir() && !metadata.file_type().is_symlink(),
                        "Shared destination contains a symlink or non-directory."
                    );
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&checked)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        let resolved_parent = fs::canonicalize(&parent)?;
        ensure!(
            resolved_parent == target || resolved_parent.starts_with(&target),
            "Shared destination points outside the worktree."
        );
        copy_tree(&source, &output)?;
    }

    if let Some(command) = command.filter(|value| !value.trim().is_empty()) {
        // The command is the project's own configuration but it runs with the
        // user's permissions, so it is shown and confirmed before reaching
        // here. Each platform gets the shell it actually has.
        let spec = ProcessSpec::new(shell_program(), shell_arguments(command))
            .cwd(&target)
            .timeout(Duration::from_secs(120));
        let output = runner.run(&spec).map_err(|error| {
            if error.to_string() == "Command timed out." {
                crate::ConvoyError::message(
                    "Worktree setup exceeded 120 seconds. Worktree has been retained.",
                )
            } else {
                error
            }
        })?;
        ensure!(
            output.ok(),
            "Setup exited with code {}. Worktree has been retained.",
            output
                .status
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
    }
    Ok(())
}

fn shell_program() -> String {
    if crate::Platform::current().is_windows() {
        crate::telemetry::powershell()
    } else {
        "/bin/bash".to_string()
    }
}

fn shell_arguments(command: &str) -> Vec<String> {
    if crate::Platform::current().is_windows() {
        vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-Command".into(),
            command.to_string(),
        ]
    } else {
        vec!["-lc".into(), command.to_string()]
    }
}

/// Recursive copy that preserves symlinks as symlinks and refuses to replace
/// anything that already exists — `fs.cp` with `dereference: false`,
/// `force: false` and `errorOnExist: true`.
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        bail!("A shared path already exists in the worktree and was not replaced.")
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        crate::fs::copy_link(source, destination)?;
    } else if metadata.is_dir() {
        fs::create_dir(destination)?;
        let mut entries: Vec<PathBuf> = fs::read_dir(source)?
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        for entry in entries {
            let Some(name) = entry.file_name() else {
                continue;
            };
            copy_tree(&entry, &destination.join(name))?;
        }
    } else {
        fs::copy(source, destination)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Worktree lifecycle, ported from the `worktree:create` and `worktree:remove`
// handlers in main.cjs.
// ---------------------------------------------------------------------------

use crate::git::Git;
use crate::model::{ActivityKind, Session};
use crate::workspace::Workspace;

/// Whether a session currently has a process, or is in the middle of a
/// worktree operation. Held by the UI; the rules that consult it live here.
pub type Busy<'a> = &'a dyn Fn(&str) -> bool;

#[derive(Debug, Clone)]
pub struct CreatePlan {
    pub session_id: String,
    pub project_path: PathBuf,
    pub branch: String,
    /// Shared files and the setup command the user will be asked about.
    pub shared_paths: Vec<String>,
    pub setup_command: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemovePlan {
    pub project_path: PathBuf,
    pub directory: PathBuf,
    /// Every session pointing at this worktree; all of them get archived.
    pub linked: Vec<String>,
}

/// A worktree belongs to a fresh coding session. Reviews share the builder's
/// folder, and a session that has already run would change directory
/// underneath a live conversation.
pub fn plan_create(
    workspace: &Workspace,
    id: &str,
    branch: &str,
    busy: Busy<'_>,
) -> Result<CreatePlan> {
    let session = workspace.session(id)?.clone();
    ensure!(
        !busy(id)
            && !session.started
            && session.working_directory.is_none()
            && session.review_of.is_none()
            && !session.is_archived(),
        "Create a worktree on a new, stopped coding session."
    );
    let project = workspace.project(&session.project_id)?.clone();
    Ok(CreatePlan {
        session_id: id.to_string(),
        project_path: project.path,
        branch: branch.to_string(),
        shared_paths: project
            .shared_paths
            .unwrap_or_default()
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect(),
        setup_command: project
            .setup_command
            .filter(|value| !value.trim().is_empty()),
    })
}

/// Binds a created worktree to its session. If this fails the worktree and its
/// branch are left in place — losing a checkout because a write failed would
/// be worse than an orphan the user can see and remove.
pub fn record_create(
    workspace: &mut Workspace,
    id: &str,
    directory: &Path,
    branch: &str,
) -> Result<()> {
    let session_id = id.to_string();
    let target = directory.to_path_buf();
    let name = branch.to_string();
    let saved = workspace
        .update({
            let session_id = session_id.clone();
            let name = name.clone();
            move |state| {
                let Some(session) = state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                else {
                    bail!("Session not found.")
                };
                session.working_directory = Some(target);
                session.branch = Some(name);
                session.owns_worktree = Some(true);
                Ok(())
            }
        })
        .map(|_| ());
    if let Err(error) = saved {
        bail!(
            "Worktree created at {}, but its session could not be saved: {error}. \
             The worktree and branch have been preserved.",
            directory.display()
        )
    }
    workspace.record(
        ActivityKind::Worktree,
        &session_id,
        &format!("Created worktree on {name}"),
    )?;
    Ok(())
}

/// Only worktrees this app created, directly under its own root, may be
/// removed — never a checkout the user set up themselves.
pub fn plan_remove(
    workspace: &Workspace,
    id: &str,
    root: &Path,
    busy: Busy<'_>,
) -> Result<RemovePlan> {
    let session = workspace.session(id)?.clone();
    let directory = session.working_directory.clone();
    let owned = session.owns_worktree()
        && directory
            .as_ref()
            .is_some_and(|directory| directory.parent() == Some(root));
    ensure!(owned, "Only worktrees created by this app can be removed.");
    let directory = directory.expect("checked above");
    let linked: Vec<String> = workspace
        .state()
        .sessions
        .iter()
        .filter(|other| other.working_directory.as_deref() == Some(directory.as_path()))
        .map(|other| other.id.clone())
        .collect();
    ensure!(
        !linked.iter().any(|id| busy(id)),
        "Stop all sessions using this worktree first."
    );
    let project = workspace.project(&session.project_id)?.clone();
    Ok(RemovePlan {
        project_path: project.path,
        directory,
        linked,
    })
}

/// Removes the checkout and archives the sessions that used it. `git worktree
/// remove` runs without `--force`, so uncommitted work is never discarded, and
/// the branch is kept.
pub fn remove(workspace: &mut Workspace, git: &Git, plan: &RemovePlan) -> Result<()> {
    remove_checkout(git, plan)?;
    record_remove(workspace, &plan.directory)
}

/// The blocking half: Git removes the checkout, or refuses because it is dirty.
pub fn remove_checkout(git: &Git, plan: &RemovePlan) -> Result<()> {
    git.run(
        &plan.project_path,
        &["worktree", "remove", &plan.directory.to_string_lossy()],
    )
    .map(|_| ())
}

/// The bookkeeping half: every session that used the folder is archived,
/// because for them it is gone.
pub fn record_remove(workspace: &mut Workspace, directory: &Path) -> Result<()> {
    let directory = directory.to_path_buf();
    workspace
        .update(move |state| {
            for session in state
                .sessions
                .iter_mut()
                .filter(|session| session.working_directory.as_deref() == Some(directory.as_path()))
            {
                session.archived = Some(true);
                session.worktree_removed = Some(true);
            }
            Ok(())
        })
        .map(|_| ())
}

/// The folder a session works in.
pub fn directory_of(workspace: &Workspace, session: &Session) -> Result<PathBuf> {
    match &session.working_directory {
        Some(directory) => Ok(directory.clone()),
        None => Ok(workspace.project(&session.project_id)?.path.clone()),
    }
}
