//! The regular expressions every build validates input with, kept in one
//! place so they all accept exactly the same values.

use regex::Regex;
use std::sync::LazyLock;

/// `/^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i`
pub static UUID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$").unwrap());

/// A binding, in the shapes the macOS app uses:
///
/// * `mod` (Command on macOS, Control elsewhere), optionally with `ctrl`,
///   `alt` and `shift`, then a letter, digit, punctuation, arrow or Tab;
/// * `ctrl` alone, like ⌃Tab or ⌃1 — but never with a letter, so ⌃C, ⌃D
///   and the rest always reach the terminal.
pub static SHORTCUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(mod\+(ctrl\+)?(alt\+)?(shift\+)?([a-z0-9,./;\[\]\\]|left|right|up|down|tab)",
        r"|ctrl\+(alt\+)?(shift\+)?([0-9,./;\[\]\\]|left|right|up|down|tab))$",
    ))
    .unwrap()
});

/// A branch prefix: empty, or path-like segments Git accepts.
pub static BRANCH_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*)?$").unwrap());

/// `/^[a-zA-Z0-9._:/-]*$/`
pub static MODEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9._:/-]*$").unwrap());

/// `/^[0-9a-f]{40,64}$/`
pub static REVISION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{40,64}$").unwrap());

/// `/\.(xcodeproj|sln|csproj)$/`
pub static PROJECT_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.(xcodeproj|sln|csproj)$").unwrap());

pub fn is_uuid(value: &str) -> bool {
    UUID.is_match(value)
}

/// `codex resume <uuid>`, as Codex prints it when it exits.
pub static CODEX_RESUME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"codex resume ([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})")
        .unwrap()
});

/// A GitHub pull request or GitLab merge request link.
pub static PULL_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"https://(github\.com|gitlab\.com)/[^\s'"`)]+/(pull|merge_requests)/\d+"#).unwrap()
});
