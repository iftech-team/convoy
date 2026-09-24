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

use crate::Platform;
use std::path::{Path, PathBuf};

pub const STORAGE_NAME: &str = "Convoy Desktop Preview";

/// Where per-user application data lives, by platform.
///
/// `$XDG_CONFIG_HOME` or `~/.config` on Unix; `%APPDATA%` on Windows. These
/// are exactly what Electron's `app.getPath('appData')` resolves to, which is
/// what lets the three builds share one file.
pub fn config_root() -> PathBuf {
    config_root_for(Platform::current())
}

pub fn config_root_for(platform: Platform) -> PathBuf {
    if platform.is_windows() {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| rooted(path, platform))
            .unwrap_or_else(|| home(platform).join("AppData").join("Roaming"));
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| rooted(path, platform))
        .unwrap_or_else(|| home(platform).join(".config"))
}

/// Whether a path is rooted **for the named platform**, rather than for
/// whichever one this is running on. `Path::is_absolute` answers the host's
/// question: `C:\\Users` is not absolute on Linux and `/home/me` is not
/// absolute on Windows, so using it here would make each platform's rules
/// testable only from that platform — the thing passing a platform exists to
/// avoid.
/// Rooted on either platform. A workspace file is shared between builds, and a
/// project path written on Windows is `C:\\Users\\…`, which Linux does not call
/// absolute; judging it by the host's rules alone would make each build reject
/// the other's file outright.
pub fn rooted_anywhere(path: &Path) -> bool {
    rooted(path, Platform::Unix) || rooted(path, Platform::Windows)
}

fn rooted(path: &Path, platform: Platform) -> bool {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    if platform.is_windows() {
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/');
        // A UNC share is rooted too.
        drive || text.starts_with("\\\\")
    } else {
        text.starts_with('/')
    }
}

pub fn home(platform: Platform) -> PathBuf {
    let name = if platform.is_windows() {
        "USERPROFILE"
    } else {
        "HOME"
    };
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
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
        let root = config_root_for(Platform::Unix);
        assert!(root.to_string_lossy().starts_with('/'), "{root:?}");
        match std::env::var_os("XDG_CONFIG_HOME") {
            Some(value) if rooted(Path::new(&value), Platform::Unix) => {
                assert_eq!(root, PathBuf::from(value))
            }
            _ => assert!(root.ends_with(".config")),
        }
    }

    /// Each platform's idea of a rooted path, judged from either of them.
    #[test]
    fn rootedness_is_decided_by_the_named_platform_not_the_host() {
        assert!(rooted(Path::new("/home/me"), Platform::Unix));
        assert!(!rooted(Path::new("home/me"), Platform::Unix));
        assert!(!rooted(Path::new("C:\\Users\\me"), Platform::Unix));

        assert!(rooted(Path::new("C:\\Users\\me"), Platform::Windows));
        assert!(rooted(Path::new("D:/Users/me"), Platform::Windows));
        assert!(rooted(Path::new("\\\\server\\share"), Platform::Windows));
        assert!(!rooted(Path::new("/home/me"), Platform::Windows));
        assert!(!rooted(Path::new("C:relative"), Platform::Windows));

        // A workspace file travels between builds; both spellings are a path.
        assert!(rooted_anywhere(Path::new("/home/me/project")));
        assert!(rooted_anywhere(Path::new("C:\\Users\\me\\project")));
        assert!(!rooted_anywhere(Path::new("project")));
    }
}
