//! Application identity and the storage locations it uses.

use convoy_core::Storage;

/// Deliberately its own id: a shared one would make two installed builds
/// fight over the same D-Bus name, and one would hand its command line to the
/// other instead of starting.
pub const APPLICATION_ID: &str = "com.iftech.convoy.linux";

pub fn storage() -> Storage {
    Storage::default()
}
