//! Project discovery, folder listing
//! and bounded previews.

pub mod paths;
pub mod preview;

use crate::patterns::PROJECT_FILE;
use crate::Result;
use std::path::{Path, PathBuf};

pub use preview::{file_content, preview, Content};

/// Directories that are almost always dependency or build output. Descending
/// into them turns discovery into a disk scan and finds nothing useful.
const EXCLUDED: [&str; 12] = [
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    "DerivedData",
    "Pods",
    "Carthage",
    "venv",
    "env",
    "__pycache__",
    "coverage",
];

/// Files that mark the root of a project. Finding one stops the walk: nested
/// packages inside a project are not separate projects.
const MARKERS: [&str; 18] = [
    ".git",
    "package.json",
    "Cargo.toml",
    "Package.swift",
    "composer.json",
    "pyproject.toml",
    "setup.py",
    "requirements.txt",
    "go.mod",
    "pubspec.yaml",
    "Gemfile",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "CMakeLists.txt",
    "mix.exs",
    "deno.json",
    "deno.jsonc",
];

fn is_excluded(name: &str) -> bool {
    EXCLUDED.contains(&name)
}

/// A folder found by [`discover_tree`]: `parent` is the index of the folder
/// it was found in, and `group` marks a folder that holds projects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: PathBuf,
    pub parent: Option<usize>,
    pub group: bool,
}

/// `ProjectDiscovery.scan()` from the macOS app: the chosen folder and every
/// folder between it and the projects inside, as a tree. A folder that holds
/// projects is a group; a project's own subfolders are not looked into.
/// `force_group` looks inside the chosen folder even when it is a project
/// itself, as "Import projects from this folder" does.
pub fn discover_tree(root: &Path, force_group: bool) -> Vec<Found> {
    fn visit(
        directory: &Path,
        parent: Option<usize>,
        depth: u32,
        root: bool,
        force_group: bool,
        count: &mut usize,
        found: &mut Vec<Found>,
    ) -> bool {
        *count += 1;
        if *count > 3000 || depth > 10 {
            return false;
        }
        let Ok(entries) = std::fs::read_dir(directory) else {
            return root;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
        let marked = entries.iter().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            MARKERS.contains(&name.as_ref()) || PROJECT_FILE.is_match(&name)
        });
        let at = found.len();
        found.push(Found {
            path: directory.to_path_buf(),
            parent,
            group: false,
        });
        if marked && !(root && force_group) {
            return true;
        }
        let mut any = false;
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `file_type` does not follow symlinks, so a linked directory is
            // skipped rather than walked into another tree.
            let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
            if is_directory && !name.starts_with('.') && !is_excluded(&name) {
                any |= visit(
                    &entry.path(),
                    Some(at),
                    depth + 1,
                    false,
                    force_group,
                    count,
                    found,
                );
            }
        }
        if any {
            found[at].group = true;
            return true;
        }
        if !root {
            // A folder with no project in it is not shown.
            found.truncate(at);
        }
        root
    }
    let mut found = Vec::new();
    let mut count = 0usize;
    visit(root, None, 0, true, force_group, &mut count, &mut found);
    found
}

/// `discover()` — the projects inside a chosen folder, or the folder itself
/// when it contains none.
pub fn discover(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut count = 0usize;
    visit(root, 0, &mut count, &mut found);
    if found.is_empty() {
        vec![root.to_path_buf()]
    } else {
        found
    }
}

/// The projects strictly inside `root`, looked for even when `root` is a
/// project itself — what "Refresh projects" adds. Unlike [`discover`] it never
/// returns `root`, and a folder with nothing in it gives nothing.
pub fn discover_within(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut count = 0usize;
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_directory && !name.starts_with('.') && !is_excluded(&name) {
            visit(&entry.path(), 1, &mut count, &mut found);
        }
    }
    found
}

fn visit(directory: &Path, depth: u32, count: &mut usize, found: &mut Vec<PathBuf>) {
    *count += 1;
    if *count > 3000 || depth > 10 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let entries: Vec<_> = entries.flatten().collect();
    let marked = entries.iter().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        MARKERS.contains(&name.as_ref()) || PROJECT_FILE.is_match(&name)
    });
    if marked {
        found.push(directory.to_path_buf());
        return;
    }
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        // `file_type` does not follow symlinks, so a linked directory is
        // skipped rather than walked into another tree.
        let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
        if is_directory && !name.starts_with('.') && !is_excluded(&name) {
            visit(&entry.path(), depth + 1, count, found);
        }
    }
}

/// `folderFiles()` — a sorted, bounded list of relative file paths, used when
/// the folder is not a Git repository.
pub fn folder_files(root: &Path) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let mut visited = 0usize;
    walk(root, "", 0, &mut visited, &mut result);
    result.sort();
    Ok(result)
}

fn walk(root: &Path, relative: &str, depth: u32, visited: &mut usize, result: &mut Vec<String>) {
    *visited += 1;
    if *visited > 3000 || depth > 10 || result.len() > 10_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root.join(relative)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if name.starts_with('.') || is_excluded(&name) {
            continue;
        }
        let joined = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(root, &joined, depth + 1, visited, result),
            Ok(kind) if kind.is_file() => result.push(joined),
            _ => {}
        }
    }
}
