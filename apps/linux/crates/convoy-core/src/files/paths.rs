//! Path validation shared by every repository operation.
//!
//! These checks are not a renderer sandbox — there is no renderer any more.
//! They defend against a crafted `workspace.json`, a symlink planted inside
//! someone else's repository, and paths that Git would read as options.

use crate::{bail, Result};
use std::path::{Component, Path, PathBuf};

/// `relative()` — a repository-relative path with no traversal, no NUL and no
/// absolute prefix. Returned unchanged so callers keep the original spelling,
/// which Git needs for `--literal-pathspecs`.
pub fn relative(value: &str) -> Result<&str> {
    let traversal = value
        .split(['/', '\\'])
        .any(|segment| segment == "..");
    if value.is_empty() || value.contains('\0') || Path::new(value).is_absolute() || traversal {
        bail!("Invalid repository path.")
    }
    Ok(value)
}

/// Resolves `name` inside `root` and proves the result is still inside it,
/// after following every symlink on both sides.
pub fn resolve_inside(root: &Path, name: &str) -> Result<PathBuf> {
    relative(name)?;
    let base = std::fs::canonicalize(root)?;
    let resolved = std::fs::canonicalize(base.join(name))?;
    if !resolved.starts_with(&base) || resolved == base {
        bail!("File points outside this folder.")
    }
    Ok(resolved)
}

/// `revision()` — an object name, never a flag such as `--hard`.
pub fn revision(value: &str) -> Result<&str> {
    if !crate::patterns::REVISION.is_match(value) {
        bail!("Invalid commit.")
    }
    Ok(value)
}

/// True when `path` has no `..`, `.` or root components left to interpret.
pub fn is_plain_relative(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
}
