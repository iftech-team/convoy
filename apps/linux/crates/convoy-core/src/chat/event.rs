//! What a chat window is told, whichever agent is behind it.
//!
//! Both agents speak their own protocol; the window only ever sees these
//! events. The shape is the contract with the front end, so field names are
//! the ones it reads — change them there too, or not at all.

use crate::json::{head, tail};
use serde::{Deserialize, Serialize};

/// The longest `detail` a tool card carries. A whole file pasted into an
/// input would otherwise travel to the window on every render.
pub const DETAIL_LIMIT: usize = 4_000;
/// The longest `output` of a tool result; the end is kept, where errors are.
pub const OUTPUT_LIMIT: usize = 8_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    /// The agent's own conversation id. For Codex this is the thread id, the
    /// one `codex resume <id>` takes, and it is recorded on the session.
    Session {
        provider_id: String,
    },
    User {
        id: String,
        text: String,
    },
    TurnStarted,
    /// Appends to assistant message `id`, creating it when new.
    Text {
        id: String,
        delta: String,
    },
    /// The final text of message `id`, replacing what the deltas built.
    TextDone {
        id: String,
        text: String,
    },
    Thinking {
        id: String,
        delta: String,
    },
    Tool {
        id: String,
        kind: ToolKind,
        name: String,
        title: String,
        detail: String,
    },
    ToolResult {
        id: String,
        ok: bool,
        output: String,
    },
    FileChange {
        id: String,
        path: String,
        diff: String,
    },
    Approval {
        request: String,
        kind: ApprovalKind,
        title: String,
        detail: String,
        /// Whether an "allow for this session" answer exists.
        always: bool,
    },
    ApprovalResolved {
        request: String,
    },
    TurnEnded {
        usage: Usage,
        error: Option<String>,
    },
    Error {
        message: String,
    },
    /// The models the agent offers this account, as it listed them itself,
    /// and the session's current choice (none: the agent's default).
    Models {
        models: Vec<ChatModel>,
        model: Option<String>,
        effort: Option<String>,
    },
}

/// One model a chat can switch to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatModel {
    /// What `--model`, `set_model` and `turn/start` take.
    pub id: String,
    pub name: String,
    /// The effort levels it takes, as the agent lists them; empty when it
    /// takes none.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    /// The model the agent uses when none is chosen.
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Command,
    Edit,
    Read,
    Search,
    Web,
    Task,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalKind {
    Command,
    Edit,
    Tool,
}

/// Tokens a turn used. `input` excludes cache reads for both agents, so the
/// four add up to what was billed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// An image attached to a user turn, as the window read it from a file or
/// the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub media_type: String,
    /// Base64, without a `data:` prefix.
    pub data: String,
}

/// The user's answer to an approval card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Allow, and do not ask again for the rest of the session.
    Always,
    Deny,
}

impl Decision {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(Decision::Allow),
            "always" => Some(Decision::Always),
            "deny" => Some(Decision::Deny),
            _ => None,
        }
    }
}

/// The start of `text`, marked when cut.
pub fn clip_head(text: &str, max: usize) -> String {
    let kept = head(text, max);
    if kept.len() == text.len() {
        text.to_string()
    } else {
        format!("{kept}\u{2026}")
    }
}

/// The end of `text`, marked when cut.
pub fn clip_tail(text: &str, max: usize) -> String {
    let kept = tail(text, max);
    if kept.len() == text.len() {
        text.to_string()
    } else {
        format!("\u{2026}{kept}")
    }
}

/// The first line of `text`, short enough for a card title.
pub fn one_line(text: &str, max: usize) -> String {
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let more = text.trim().lines().count() > 1;
    let line = clip_head(first.trim(), max);
    if more && !line.ends_with('\u{2026}') {
        format!("{line} \u{2026}")
    } else {
        line
    }
}

/// A unified diff of one replacement, reduced to the lines that changed and
/// three lines of context. An edit tool sends only the snippet it replaces,
/// so line numbers count from the snippet, not the file.
pub fn replacement_diff(path: &str, old: &str, new: &str) -> String {
    let path = path.trim_start_matches('/');
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let prefix = old
        .iter()
        .zip(new.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let start = prefix.saturating_sub(3);
    let old_end = (old.len() - suffix + 3).min(old.len());
    let new_end = (new.len() - suffix + 3).min(new.len());

    let mut out = format!(
        "--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
        start + 1,
        old_end - start,
        start + 1,
        new_end - start
    );
    for line in &old[start..prefix] {
        out.push_str(&format!(" {line}\n"));
    }
    for line in &old[prefix..old.len() - suffix] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &new[prefix..new.len() - suffix] {
        out.push_str(&format!("+{line}\n"));
    }
    for line in &new[new.len() - suffix..new_end] {
        out.push_str(&format!(" {line}\n"));
    }
    out
}

/// A new file as a diff: every line added.
pub fn added_diff(path: &str, content: &str) -> String {
    let path = path.trim_start_matches('/');
    let lines: Vec<&str> = content.lines().collect();
    let mut out = format!(
        "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        out.push_str(&format!("+{line}\n"));
    }
    out
}

/// A path as the user thinks of it: inside the folder, relative to it.
pub fn short_path(path: &str, cwd: Option<&str>) -> String {
    cwd.map(|cwd| cwd.trim_end_matches(['/', '\\']))
        .filter(|cwd| !cwd.is_empty())
        .and_then(|cwd| path.strip_prefix(cwd))
        .and_then(|rest| rest.strip_prefix('/').or(rest.strip_prefix('\\')))
        .filter(|rest| !rest.is_empty())
        .unwrap_or(path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_serialise_in_the_shape_the_window_reads() {
        assert_eq!(
            serde_json::to_value(ChatEvent::TurnStarted).unwrap(),
            json!({ "type": "turn_started" })
        );
        assert_eq!(
            serde_json::to_value(ChatEvent::TurnEnded {
                usage: Usage {
                    input: 1,
                    output: 2,
                    cache_read: 3,
                    cache_write: 4
                },
                error: None
            })
            .unwrap(),
            json!({ "type": "turn_ended", "usage": { "input": 1, "output": 2, "cache_read": 3, "cache_write": 4 }, "error": null })
        );
        assert_eq!(
            serde_json::to_value(ChatEvent::Models {
                models: vec![ChatModel {
                    id: "opus".into(),
                    name: "Opus".into(),
                    efforts: vec!["high".into()],
                    default_effort: None,
                    is_default: true,
                }],
                model: None,
                effort: Some("high".into()),
            })
            .unwrap(),
            json!({ "type": "models", "models": [{ "id": "opus", "name": "Opus", "efforts": ["high"], "default_effort": null, "is_default": true }], "model": null, "effort": "high" })
        );
        assert_eq!(
            serde_json::to_value(ChatEvent::Tool {
                id: "t".into(),
                kind: ToolKind::Command,
                name: "Bash".into(),
                title: "Run `ls`".into(),
                detail: "ls".into()
            })
            .unwrap()["kind"],
            "command"
        );
    }

    #[test]
    fn a_replacement_diff_keeps_only_the_change_and_its_context() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let new = "a\nb\nc\nd\nE\nf\ng\nh\n";
        assert_eq!(
            replacement_diff("x.txt", old, new),
            "--- a/x.txt\n+++ b/x.txt\n@@ -2,7 +2,7 @@\n b\n c\n d\n-e\n+E\n f\n g\n h\n"
        );
        assert_eq!(
            added_diff("hello.txt", "hi\n"),
            "--- /dev/null\n+++ b/hello.txt\n@@ -0,0 +1,1 @@\n+hi\n"
        );
    }

    #[test]
    fn long_text_is_cut_and_marked() {
        assert_eq!(clip_head("abcdef", 3), "abc\u{2026}");
        assert_eq!(clip_tail("abcdef", 3), "\u{2026}def");
        assert_eq!(clip_head("abc", 3), "abc");
        assert_eq!(one_line("npm test\nnpm run lint", 80), "npm test \u{2026}");
    }
}
