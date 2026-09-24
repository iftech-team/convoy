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

/// Where an untracked file to be trashed actually lives.
///
/// The parent directory is resolved rather than the file itself: following the
/// file's own symlink would trash whatever it points at, which may be outside
/// the repository entirely.
pub fn trash_target(root: &Path, name: &str) -> Result<PathBuf> {
    let name = relative(name)?;
    let base = std::fs::canonicalize(root)?;
    let candidate = base.join(name);
    let parent = candidate
        .parent()
        .map(std::fs::canonicalize)
        .transpose()?
        .unwrap_or_else(|| base.clone());
    if parent != base && !parent.starts_with(&base) {
        bail!("Path points outside repository.")
    }
    Ok(parent.join(candidate.file_name().unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traversal_and_absolute_paths_are_refused() {
        assert!(relative("src/main.rs").is_ok());
        assert!(relative("../outside").is_err());
        assert!(relative("/etc/passwd").is_err());
        assert!(relative("a/../../b").is_err());
        assert!(relative("").is_err());
        assert!(relative("with\0nul").is_err());
        // A dash-prefixed name is a path, not an option, once `--` precedes it.
        assert!(relative("--hard").is_ok());
    }

    #[test]
    fn a_commit_is_an_object_name_never_a_flag() {
        assert!(revision(&"a".repeat(40)).is_ok());
        assert!(revision("--hard").is_err());
        assert!(revision("HEAD~1").is_err());
    }

    #[test]
    fn trash_resolves_the_parent_not_the_file() {
        let directory = tempfile::Builder::new().prefix("convoy-trash-").tempdir().unwrap();
        let root = directory.path().join("repo");
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("sub/file"), "x").unwrap();
        assert!(trash_target(&root, "sub/file").is_ok());

        // A symlinked directory leading out of the repository is refused.
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        std::fs::write(outside.join("victim"), "x").unwrap();
        assert!(trash_target(&root, "link/victim").is_err());
    }
}
