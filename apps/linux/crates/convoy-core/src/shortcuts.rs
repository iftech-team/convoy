//! Keyboard shortcuts.
//!
//! The workspace stores them in the Electron form — `mod+shift+p` — because
//! both builds read the same file. GTK wants `<Primary><Shift>p`, so the two
//! notations are converted here rather than in the UI, and the stored form
//! never changes.

use crate::model::SHORTCUT_ACTIONS;
use crate::patterns::SHORTCUT;
use std::collections::BTreeMap;

/// What each action does, for the shortcut editor and the command palette.
pub const DESCRIPTIONS: [(&str, &str); 7] = [
    ("palette", "Command palette"),
    ("newSession", "New session"),
    ("files", "Files & Changes"),
    ("next", "Next session"),
    ("previous", "Previous session"),
    ("settings", "Settings"),
    ("search", "Search projects and sessions"),
];

/// Defaults used when the workspace has no shortcut for an action.
pub const DEFAULTS: [(&str, &str); 7] = [
    ("palette", "mod+k"),
    ("newSession", "mod+n"),
    ("files", "mod+b"),
    ("next", "mod+alt+n"),
    ("previous", "mod+alt+p"),
    ("settings", "mod+,"),
    ("search", "mod+f"),
];

/// `mod+shift+p` → `<Primary><Shift>p`. Returns nothing for anything the
/// stored format does not allow, so a hand-edited file cannot bind nonsense.
pub fn to_accelerator(value: &str) -> Option<String> {
    if !SHORTCUT.is_match(value) {
        return None;
    }
    let mut parts = value.split('+').peekable();
    // The pattern guarantees a leading `mod`.
    parts.next()?;
    let mut modifiers = String::from("<Primary>");
    if parts.peek() == Some(&"alt") {
        parts.next();
        modifiers.push_str("<Alt>");
    }
    if parts.peek() == Some(&"shift") {
        parts.next();
        modifiers.push_str("<Shift>");
    }
    let key: Vec<&str> = parts.collect();
    let key = key.join("+");
    if key.is_empty() {
        return None;
    }
    Some(format!("{modifiers}{}", key_name(&key)))
}

/// `<Primary><Shift>p` → `mod+shift+p`, for the editor's key capture.
pub fn from_accelerator(value: &str) -> Option<String> {
    let mut rest = value;
    let mut modifiers = Vec::new();
    let primary = ["<Primary>", "<Control>", "<Ctrl>", "<Meta>"];
    let mut has_primary = false;
    while let Some(start) = rest.strip_prefix('<') {
        let end = start.find('>')? + 1;
        let token = &rest[..end + 1];
        rest = &rest[end + 1..];
        if primary.contains(&token) {
            has_primary = true;
        } else if token.eq_ignore_ascii_case("<alt>") {
            modifiers.push("alt");
        } else if token.eq_ignore_ascii_case("<shift>") {
            modifiers.push("shift");
        } else {
            return None;
        }
    }
    if !has_primary || rest.is_empty() {
        return None;
    }
    let key = stored_key(rest);
    modifiers.sort_by_key(|name| if *name == "alt" { 0 } else { 1 });
    let result = format!("mod+{}{key}", modifiers.iter().map(|name| format!("{name}+")).collect::<String>());
    SHORTCUT.is_match(&result).then_some(result)
}

/// GTK names punctuation keys; the stored format spells them out.
fn key_name(key: &str) -> String {
    match key {
        "," => "comma".to_string(),
        other => other.to_string(),
    }
}

fn stored_key(key: &str) -> String {
    match key.to_lowercase().as_str() {
        "comma" => ",".to_string(),
        other => other.to_string(),
    }
}

/// The accelerator for every action, saved values first and defaults behind.
pub fn resolve(saved: &BTreeMap<String, String>) -> Vec<(&'static str, String, String)> {
    SHORTCUT_ACTIONS
        .iter()
        .map(|action| {
            let stored = saved
                .get(*action)
                .cloned()
                .or_else(|| {
                    DEFAULTS
                        .iter()
                        .find(|(name, _)| name == action)
                        .map(|(_, value)| value.to_string())
                })
                .unwrap_or_default();
            let accelerator = to_accelerator(&stored).unwrap_or_default();
            (*action, stored, accelerator)
        })
        .collect()
}

pub fn description(action: &str) -> &'static str {
    DESCRIPTIONS
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, text)| *text)
        .unwrap_or("Action")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_notations_round_trip() {
        for stored in ["mod+k", "mod+shift+p", "mod+alt+n", "mod+alt+shift+k", "mod+,"] {
            let accelerator = to_accelerator(stored).expect(stored);
            assert_eq!(
                from_accelerator(&accelerator).as_deref(),
                Some(stored),
                "{stored} became {accelerator}"
            );
        }
        assert_eq!(to_accelerator("mod+shift+p").unwrap(), "<Primary><Shift>p");
        assert_eq!(to_accelerator("mod+,").unwrap(), "<Primary>comma");
    }

    #[test]
    fn anything_the_stored_format_forbids_is_refused() {
        // The order is fixed, the modifier set is fixed, and Primary is required.
        assert!(to_accelerator("mod+shift+alt+p").is_none());
        assert!(to_accelerator("ctrl+p").is_none());
        assert!(to_accelerator("mod+P").is_none());
        assert!(to_accelerator("").is_none());
        assert!(from_accelerator("<Shift>p").is_none());
        assert!(from_accelerator("p").is_none());
        assert!(from_accelerator("<Primary>").is_none());
    }

    #[test]
    fn every_action_resolves_to_something_usable() {
        let resolved = resolve(&BTreeMap::new());
        assert_eq!(resolved.len(), SHORTCUT_ACTIONS.len());
        assert!(resolved.iter().all(|(_, _, accelerator)| !accelerator.is_empty()));

        let mut saved = BTreeMap::new();
        saved.insert("palette".to_string(), "mod+shift+k".to_string());
        let resolved = resolve(&saved);
        let palette = resolved.iter().find(|(name, ..)| *name == "palette").unwrap();
        assert_eq!(palette.2, "<Primary><Shift>k", "a saved value wins");
    }
}
