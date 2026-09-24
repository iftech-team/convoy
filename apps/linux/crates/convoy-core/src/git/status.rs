//! Port of `parseStatus()` and the counting loop in `gitStatus()`.
//!
//! `--porcelain=v1 -z` emits `XY path` records separated by NUL, and a rename
//! or copy adds a second record holding the original path. Status text is data:
//! it is displayed, never interpreted as a command.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub original: Option<String>,
    pub index: char,
    pub worktree: char,
    pub untracked: bool,
    pub conflict: bool,
}

const CONFLICTS: [&str; 7] = ["DD", "AU", "UD", "UA", "DU", "AA", "UU"];

pub fn parse(raw: &str) -> Vec<Change> {
    let records: Vec<&str> = raw.split('\0').collect();
    let mut result = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let mut characters = record.chars();
        let x = characters.next().unwrap_or(' ');
        let y = characters.next().unwrap_or(' ');
        let name = record.get(3..).unwrap_or_default().to_string();
        let renamed = x == 'R' || x == 'C' || y == 'R' || y == 'C';
        let original = if renamed {
            let value = records.get(index).map(|value| value.to_string());
            index += 1;
            value
        } else {
            None
        };
        result.push(Change {
            path: name,
            original,
            index: x,
            worktree: y,
            untracked: x == '?',
            conflict: CONFLICTS.contains(&format!("{x}{y}").as_str()),
        });
    }
    result
}

/// The changed-file count shown next to a session, with rename pairs counted
/// once.
pub fn count(raw: &str) -> usize {
    parse(raw).len()
}
