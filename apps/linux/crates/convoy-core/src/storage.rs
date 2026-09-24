//! Where Convoy keeps its state on Linux.
//!
//! The directory name and layout are shared with the Electron preview so both
//! builds read the same `workspace.json`: the port can be installed alongside
//! the old build and used on real data from day one. Renaming this directory
//! would silently strand every existing project, so it stays as it is until
//! the Electron build is retired on Linux.
//!
//! **Do not run both builds against the same file at once** — each writes the
//! whole document, so the last writer wins.

use std::path::{Path, PathBuf};

pub const STORAGE_NAME: &str = "Convoy Desktop Preview";

/// `$XDG_CONFIG_HOME` when it is set to an absolute path, otherwise
/// `~/.config`. This is what both `g_get_user_config_dir()` and Electron's
/// `app.getPath('appData')` resolve to on Linux.
pub fn config_root() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home().join(".config"))
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[derive(Debug, Clone)]
pub struct Storage {
    root: PathBuf,
}

impl Default for Storage {
    fn default() -> Self {
        Storage::new(config_root().join(STORAGE_NAME))
    }
}

impl Storage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Storage { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn workspace_file(&self) -> PathBuf {
        self.root.join("workspace.json")
    }

    /// Bounded plain-text excerpts of terminal output, one file per session.
    pub fn history(&self) -> PathBuf {
        self.root.join("TerminalHistory")
    }

    /// Hook settings and the state each agent reports back.
    pub fn telemetry(&self) -> PathBuf {
        self.root.join("telemetry")
    }

    /// Isolated `CLAUDE_CONFIG_DIR` / `CODEX_HOME` trees, one per profile.
    pub fn accounts(&self) -> PathBuf {
        self.root.join("accounts")
    }

    /// Worktrees Convoy created itself, and may therefore remove.
    pub fn worktrees(&self) -> PathBuf {
        self.root.join("worktrees")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_electron_preview() {
        let storage = Storage::new("/home/example/.config/Convoy Desktop Preview");
        assert_eq!(
            storage.workspace_file(),
            Path::new("/home/example/.config/Convoy Desktop Preview/workspace.json")
        );
        assert!(storage.history().ends_with("TerminalHistory"));
        assert!(storage.accounts().ends_with("accounts"));
    }

    #[test]
    fn config_root_follows_the_xdg_variable_only_when_absolute() {
        let root = config_root();
        assert!(root.is_absolute(), "{root:?}");
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(value) if Path::new(&value).is_absolute() => {
                assert_eq!(root, PathBuf::from(value))
            }
            _ => assert!(root.ends_with(".config")),
        }
    }
}
