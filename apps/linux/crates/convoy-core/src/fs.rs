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

/// Reproduces a symlink without dereferencing it. Windows requires Developer
/// Mode or permission to create links; report that error rather than silently
/// producing an empty directory or copying the contents of an external target.
pub fn copy_link(source: &Path, destination: &Path) -> io::Result<()> {
    let target = std::fs::read_link(source)?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, destination)
    }
    #[cfg(windows)]
    {
        if source.is_dir() {
            std::os::windows::fs::symlink_dir(target, destination)
        } else {
            std::os::windows::fs::symlink_file(target, destination)
        }
    }
}
