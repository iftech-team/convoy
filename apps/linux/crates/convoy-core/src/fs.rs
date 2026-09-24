//! File creation that keeps the same promise on both platforms.
//!
//! Convoy writes credentials-adjacent things: hook settings, quota readings,
//! saved terminal output. On Unix those are created 0600. Windows has no mode
//! bits; a file under the user's own `%APPDATA%` already inherits an ACL that
//! excludes other users, which is the same promise by a different mechanism.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

/// Creates or truncates a file only the owner can read.
pub fn create_private(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Reproduces a symlink, or on Windows copies what it points at.
///
/// Windows needs either developer mode or an elevated process to create one,
/// and a shared file that silently failed to appear would be worse than one
/// that is a copy. The Electron build copied on both platforms for the same
/// reason.
pub fn copy_link(source: &Path, destination: &Path) -> io::Result<()> {
    let target = std::fs::read_link(source)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, destination)
    }
    #[cfg(windows)]
    {
        let resolved = if target.is_absolute() {
            target
        } else {
            source
                .parent()
                .map(|parent| parent.join(&target))
                .unwrap_or(target)
        };
        if resolved.is_dir() {
            std::fs::create_dir_all(destination)
        } else {
            std::fs::copy(&resolved, destination).map(|_| ())
        }
    }
}
