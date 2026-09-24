//! Regular expressions ported verbatim from the Electron sources so the two
//! builds accept exactly the same values.

use regex::Regex;
use std::sync::LazyLock;

/// `/^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i`
pub static UUID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$").unwrap());

/// `/^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$/`
pub static SHORTCUT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^mod\+(alt\+)?(shift\+)?[a-z0-9,]+$").unwrap());

/// `/^[a-zA-Z0-9._:/-]*$/`
pub static MODEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._:/-]*$").unwrap());

/// `/^[0-9a-f]{40,64}$/`
pub static REVISION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{40,64}$").unwrap());

/// `/\.(xcodeproj|sln|csproj)$/`
pub static PROJECT_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.(xcodeproj|sln|csproj)$").unwrap());

pub fn is_uuid(value: &str) -> bool {
    UUID.is_match(value)
}
