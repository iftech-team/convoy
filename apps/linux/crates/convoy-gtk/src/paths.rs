//! Application identity and the storage locations it uses.

use convoy_core::Storage;

/// Deliberately different from the Electron preview's `com.iftech.convoy.desktop`:
/// a shared id would make the two builds fight over the same D-Bus name, and
/// one would hand its command line to the other instead of starting.
pub const APPLICATION_ID: &str = "com.iftech.convoy.linux";

pub fn storage() -> Storage {
    Storage::default()
}
