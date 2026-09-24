//! Port of `hunks()` and `digest()`.
//!
//! Splitting a unified diff into its header and individual hunks is what makes
//! per-hunk discard possible: the header plus one hunk is a complete patch that
//! Git can apply in reverse.

use crate::hash::sha256_hex;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Hunks {
    pub header: String,
    pub patches: Vec<String>,
}

pub fn hunks(text: &str) -> Hunks {
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(start) = lines.iter().position(|line| line.starts_with("@@ ")) else {
        return Hunks::default();
    };
    let header = format!("{}\n", lines[..start].join("\n"));
    let mut patches: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in &lines[start..] {
        if line.starts_with("@@ ") && !current.is_empty() {
            patches.push(format!("{}\n", current.join("\n")));
            current.clear();
        }
        current.push(line);
    }
    if !current.is_empty() {
        let joined = current.join("\n");
        patches.push(format!("{}\n", joined.strip_suffix('\n').unwrap_or(&joined)));
    }
    Hunks { header, patches }
}

/// The hash a discard request must still match. Stale diffs are rejected
/// rather than applied to changed content.
pub fn digest(text: &str) -> String {
    sha256_hex(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_two_hunk_diff() {
        let text = "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-a\n+b\n c\n@@ -20,3 +20,3 @@\n-x\n+y\n z\n";
        let parsed = hunks(text);
        assert!(parsed.header.ends_with("+++ b/f\n"));
        assert_eq!(parsed.patches.len(), 2);
        assert!(parsed.patches[0].starts_with("@@ -1,3 +1,3 @@\n"));
        assert!(parsed.patches[1].ends_with(" z\n"));
        assert_eq!(parsed.header + &parsed.patches[0] + &parsed.patches[1], text);
    }

    #[test]
    fn returns_nothing_for_a_diff_without_hunks() {
        assert_eq!(hunks("diff --git a/f b/f\nBinary files differ\n"), Hunks::default());
    }
}
