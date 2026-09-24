//! Which operating system a decision is being made for.
//!
//! Passed as a value rather than read from `cfg!`, so the Windows branches can
//! be exercised from a Linux machine and from Linux CI. The Electron build did
//! the same thing with its `platform = process.platform` parameter, and it is
//! the only reason its Windows launch had tests at all.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Unix,
    Windows,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Unix
        }
    }

    pub fn is_windows(self) -> bool {
        self == Platform::Windows
    }

    /// The separator `PATH` uses.
    pub fn path_separator(self) -> char {
        match self {
            Platform::Unix => ':',
            Platform::Windows => ';',
        }
    }

    /// The separator between directories in a path.
    pub fn directory_separator(self) -> char {
        match self {
            Platform::Unix => '/',
            Platform::Windows => '\\',
        }
    }

    /// Joins path parts the way the target platform writes them. `PathBuf`
    /// cannot be used for this: it always produces the host's form, and half
    /// the point here is to produce the other one.
    pub fn join(self, parts: &[&str]) -> String {
        let separator = self.directory_separator();
        let mut joined = String::new();
        for part in parts {
            if part.is_empty() {
                continue;
            }
            if !joined.is_empty() && !joined.ends_with(separator) {
                joined.push(separator);
            }
            joined.push_str(part.trim_end_matches(separator));
        }
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_written_the_way_each_platform_writes_them() {
        assert_eq!(
            Platform::Unix.join(&["/usr", "bin", "claude"]),
            "/usr/bin/claude"
        );
        assert_eq!(
            Platform::Windows.join(&["C:\\Windows", "System32", "cmd.exe"]),
            "C:\\Windows\\System32\\cmd.exe"
        );
        // A trailing separator on a part does not double up.
        assert_eq!(Platform::Windows.join(&["C:\\", "tools"]), "C:\\tools");
        assert_eq!(Platform::Unix.join(&["", "bin"]), "bin");
    }
}
