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
        let spec = ProcessSpec::new(
            "/bin/bash",
            vec!["-lc".into(), command.to_string()],
        )
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

/// Recursive copy that preserves symlinks as symlinks and refuses to replace
/// anything that already exists — `fs.cp` with `dereference: false`,
/// `force: false` and `errorOnExist: true`.
fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        bail!("A shared path already exists in the worktree and was not replaced.")
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        let link = fs::read_link(source)?;
        std::os::unix::fs::symlink(link, destination)?;
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
