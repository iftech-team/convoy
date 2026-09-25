//! Keyboard shortcuts.
//!
//! The workspace stores them in one portable form — `mod+shift+p` — because
//! every build reads the same file. GTK wants `<Primary><Shift>p`, so the two
//! notations are converted here rather than in the UI, and the stored form
//! never changes.

use crate::model::SHORTCUT_ACTIONS;
use crate::patterns::SHORTCUT;
use std::collections::BTreeMap;

/// What each action does, for the shortcut editor and the command palette.
pub const DESCRIPTIONS: &[(&str, &str)] = &[
    ("palette", "Command palette"),
    ("newSession", "New session"),
    ("files", "Files & Changes"),
    ("next", "Next tab"),
    ("previous", "Previous tab"),
    ("settings", "Settings"),
    ("search", "Search projects and sessions"),
    ("resume", "Resume session"),
    ("stop", "Stop session"),
    ("sleep", "Sleep session"),
    ("pin", "Pin or unpin session"),
    ("edit", "Edit name and notes"),
    ("review", "Start review"),
    ("feedback", "Send feedback to builder"),
    ("quick", "Quick commands"),
    ("split", "Toggle two panes"),
    ("openFolder", "Open folder"),
    ("newTask", "New task"),
    ("newSpec", "New specification"),
    ("importIssues", "Import Linear or Jira issues"),
    ("sessionsTab", "Sessions"),
    ("reviewsTab", "Reviews"),
    ("specsTab", "Specs"),
    ("tasksTab", "Tasks"),
    ("docsTab", "Docs & specs"),
    ("closeTab", "Close tab"),
    ("sidebar", "Toggle sidebar"),
    ("layout1", "One pane"),
    ("layout2", "Two panes"),
    ("layout4", "Four panes"),
    ("paneNext", "Focus next pane"),
    ("panePrevious", "Focus previous pane"),
    ("paneClose", "Close focused pane"),
    ("activity", "Activity feed"),
    ("nextTab", "Next section"),
    ("previousTab", "Previous section"),
    ("reveal", "Show project folder"),
    ("copyPath", "Copy folder path"),
    ("gitRefresh", "Refresh Git status"),
    ("theme", "Cycle theme"),
    ("wake", "Toggle keep awake"),
    ("limits", "AI Limits"),
    ("limitsRefresh", "Refresh AI limits"),
];

/// Defaults used when the workspace has no shortcut for an action: the
/// macOS app's bindings. Next and previous session moved to ⌃Tab / ⌃⇧Tab
/// as there, which freed ⌘⌥P for pin and ⌘⌥N for a new specification.
pub const DEFAULTS: &[(&str, &str)] = &[
    ("palette", "mod+k"),
    ("newSession", "mod+n"),
    ("files", "mod+shift+g"),
    ("next", "ctrl+tab"),
    ("previous", "ctrl+shift+tab"),
    ("settings", "mod+,"),
    ("search", "mod+f"),
    ("resume", "mod+shift+r"),
    ("stop", "mod+."),
    ("sleep", "mod+alt+z"),
    ("pin", "mod+alt+p"),
    ("edit", "mod+i"),
    ("review", "mod+alt+r"),
    ("feedback", "mod+shift+b"),
    ("quick", "mod+/"),
    ("split", "mod+\\"),
    ("openFolder", "mod+o"),
    ("newTask", "mod+shift+n"),
    ("newSpec", "mod+alt+n"),
    ("importIssues", "mod+shift+i"),
    ("sessionsTab", "mod+alt+1"),
    ("reviewsTab", "mod+alt+2"),
    ("specsTab", "mod+alt+3"),
    ("tasksTab", "mod+alt+4"),
    ("docsTab", "mod+alt+5"),
    ("closeTab", "mod+w"),
    ("sidebar", "mod+b"),
    ("layout1", "ctrl+shift+1"),
    ("layout2", "ctrl+shift+2"),
    ("layout4", "ctrl+shift+4"),
    ("paneNext", "mod+alt+right"),
    ("panePrevious", "mod+alt+left"),
    ("paneClose", "mod+shift+w"),
    ("activity", "mod+shift+a"),
    ("nextTab", "mod+shift+]"),
    ("previousTab", "mod+shift+["),
    ("reveal", "mod+alt+f"),
    ("copyPath", "mod+alt+c"),
    ("gitRefresh", "mod+alt+g"),
    ("theme", "mod+alt+t"),
    ("wake", "mod+alt+k"),
    ("limits", "mod+shift+l"),
    ("limitsRefresh", "mod+alt+l"),
];

/// `mod+shift+p` → `<Primary><Shift>p`. Returns nothing for anything the
/// stored format does not allow, so a hand-edited file cannot bind nonsense.
pub fn to_accelerator(value: &str) -> Option<String> {
    if !SHORTCUT.is_match(value) {
        return None;
    }
    // The pattern guarantees modifiers first, in order, then one key.
    let parts: Vec<&str> = value.split('+').collect();
    let (key, modifiers) = parts.split_last()?;
    let modifiers: String = modifiers
        .iter()
        .map(|modifier| match *modifier {
            "mod" => "<Primary>",
            "ctrl" => "<Control>",
            "alt" => "<Alt>",
            _ => "<Shift>",
        })
        .collect();
    Some(format!("{modifiers}{}", key_name(key)))
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
    let result = format!(
        "mod+{}{key}",
        modifiers
            .iter()
            .map(|name| format!("{name}+"))
            .collect::<String>()
    );
    SHORTCUT.is_match(&result).then_some(result)
}

/// GTK names punctuation keys; the stored format spells them out.
const KEY_NAMES: [(&str, &str); 12] = [
    ("tab", "Tab"),
    (",", "comma"),
    (".", "period"),
    ("/", "slash"),
    (";", "semicolon"),
    ("[", "bracketleft"),
    ("]", "bracketright"),
    ("\\", "backslash"),
    ("left", "Left"),
    ("right", "Right"),
    ("up", "Up"),
    ("down", "Down"),
];

fn key_name(key: &str) -> String {
    KEY_NAMES
        .iter()
        .find(|(stored, _)| *stored == key)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| key.to_string())
}

fn stored_key(key: &str) -> String {
    let lower = key.to_lowercase();
    KEY_NAMES
        .iter()
        .find(|(_, name)| name.to_lowercase() == lower)
        .map(|(stored, _)| stored.to_string())
        .unwrap_or(lower)
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
        for stored in [
            "mod+k",
            "mod+shift+p",
            "mod+alt+n",
            "mod+alt+shift+k",
            "mod+,",
        ] {
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
        assert!(resolved
            .iter()
            .all(|(_, _, accelerator)| !accelerator.is_empty()));

        let mut saved = BTreeMap::new();
        saved.insert("palette".to_string(), "mod+shift+k".to_string());
        let resolved = resolve(&saved);
        let palette = resolved
            .iter()
            .find(|(name, ..)| *name == "palette")
            .unwrap();
        assert_eq!(palette.2, "<Primary><Shift>k", "a saved value wins");
    }
}
