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

/// One row of a side-by-side diff: what the file had, and what it has now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SplitRow {
    pub left: Option<(u32, String)>,
    pub right: Option<(u32, String)>,
    /// A hunk header rather than content, shown spanning both columns.
    pub separator: Option<String>,
}

/// Turns a unified diff into aligned columns with real line numbers.
///
/// Removals and additions inside one run are paired up, so a changed line sits
/// opposite the line it replaced instead of being pushed down the column.
pub fn split(text: &str) -> Vec<SplitRow> {
    let mut rows: Vec<SplitRow> = Vec::new();
    let mut left_number = 0u32;
    let mut right_number = 0u32;
    let mut removals: Vec<(u32, String)> = Vec::new();
    let mut additions: Vec<(u32, String)> = Vec::new();

    let flush = |rows: &mut Vec<SplitRow>,
                 removals: &mut Vec<(u32, String)>,
                 additions: &mut Vec<(u32, String)>| {
        let pairs = removals.len().max(additions.len());
        for index in 0..pairs {
            rows.push(SplitRow {
                left: removals.get(index).cloned(),
                right: additions.get(index).cloned(),
                separator: None,
            });
        }
        removals.clear();
        additions.clear();
    };

    for line in text.split('\n') {
        if line.starts_with("@@ ") {
            flush(&mut rows, &mut removals, &mut additions);
            if let Some((left, right)) = hunk_start(line) {
                left_number = left;
                right_number = right;
            }
            rows.push(SplitRow {
                separator: Some(line.to_string()),
                ..SplitRow::default()
            });
            continue;
        }
        if rows.is_empty() {
            // Still in the file header, before the first hunk.
            continue;
        }
        let Some(marker) = line.chars().next() else {
            continue;
        };
        let body = line.get(1..).unwrap_or_default().to_string();
        match marker {
            '-' => {
                removals.push((left_number, body));
                left_number += 1;
            }
            '+' => {
                additions.push((right_number, body));
                right_number += 1;
            }
            '\\' => {} // "\ No newline at end of file"
            _ => {
                flush(&mut rows, &mut removals, &mut additions);
                rows.push(SplitRow {
                    left: Some((left_number, body.clone())),
                    right: Some((right_number, body)),
                    separator: None,
                });
                left_number += 1;
                right_number += 1;
            }
        }
    }
    flush(&mut rows, &mut removals, &mut additions);
    rows
}

/// `@@ -12,7 +12,9 @@` — the first line number on each side.
fn hunk_start(line: &str) -> Option<(u32, u32)> {
    let inner = line.strip_prefix("@@ ")?.split(" @@").next()?;
    let mut parts = inner.split_whitespace();
    let left = parts.next()?.strip_prefix('-')?;
    let right = parts.next()?.strip_prefix('+')?;
    let number = |value: &str| value.split(',').next()?.parse::<u32>().ok();
    Some((number(left)?, number(right)?))
}

#[cfg(test)]
mod split_tests {
    use super::*;

    const DIFF: &str = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -10,4 +10,4 @@\n context\n-old one\n-old two\n+new one\n c\n";

    #[test]
    fn pairs_removals_with_the_additions_that_replaced_them() {
        let rows = split(DIFF);
        assert_eq!(rows[0].separator.as_deref(), Some("@@ -10,4 +10,4 @@"));
        assert_eq!(rows[1].left, Some((10, "context".into())));
        assert_eq!(rows[1].right, Some((10, "context".into())));

        // Two removals against one addition: the extra removal has no partner.
        assert_eq!(rows[2].left, Some((11, "old one".into())));
        assert_eq!(rows[2].right, Some((11, "new one".into())));
        assert_eq!(rows[3].left, Some((12, "old two".into())));
        assert_eq!(rows[3].right, None);

        // Numbering continues from the right side's own count.
        assert_eq!(rows[4].left, Some((13, "c".into())));
        assert_eq!(rows[4].right, Some((12, "c".into())));
    }

    #[test]
    fn ignores_the_file_header_and_survives_an_empty_diff() {
        assert!(split("").is_empty());
        assert!(split("diff --git a/f b/f\nBinary files differ\n").is_empty());
        assert_eq!(
            split(DIFF).iter().filter(|row| row.separator.is_some()).count(),
            1
        );
    }
}
