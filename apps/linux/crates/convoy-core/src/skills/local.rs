//! The agents' own skills on this machine, brought into Convoy's library.
//!
//! Claude Code, Codex, Gemini CLI and the shared Agent Skills convention keep a
//! skill as a folder with a `SKILL.md` — front matter with a name and a
//! description, then Markdown — in the user's folder (`~/.claude/skills`,
//! `~/.codex/skills`, `~/.gemini/skills`, `~/.agents/skills`) and in a
//! repository's own (`.claude/skills` and so on). Convoy does not
//! read them by itself: a person picks the ones they want and the projects
//! they are for, and each becomes a Convoy skill, which then reaches either
//! agent and the task roles.
//!
//! A skill's folder may hold scripts and templates beside its `SKILL.md`;
//! the imported text says where they are, so the agent reads them there.
//! Importing one that is already in the library refreshes it.

use super::{Agents, SkillInput, Store, MAX_BODY};
use crate::automations::Instant;
use crate::json::utf16_len;
use crate::model::Role;
use crate::workspace::Workspace;
use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A skill found on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub name: String,
    pub description: String,
    /// Its `SKILL.md`.
    pub path: PathBuf,
    /// The project whose repository it is in; none for the user's folder.
    pub project_id: Option<String>,
    /// Whose folder it is in: `Claude Code`, `Codex` or `Agent Skills`.
    pub source: String,
    /// Whether a skill of this name is in the library already.
    pub imported: bool,
}

/// The front matter's name and description, and the text after it. A file
/// with no front matter is named after its folder.
pub fn parse(text: &str) -> (Option<String>, String, String) {
    let text = text.trim_start_matches('\u{feff}');
    let Some(rest) = text.strip_prefix("---") else {
        return (None, String::new(), text.trim().to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (None, String::new(), text.trim().to_string());
    };
    let (header, body) = (&rest[..end], &rest[end + 4..]);
    let body = body.trim_start_matches(['-']).trim();
    let mut name = None;
    let mut description = String::new();
    let mut lines = header.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let mut value = value.trim().to_string();
        // A folded or literal block: the indented lines that follow.
        if value == ">" || value == "|" || value == ">-" || value == "|-" {
            let mut parts = Vec::new();
            while let Some(next) = lines.peek() {
                if next.starts_with([' ', '\t']) || next.trim().is_empty() {
                    parts.push(next.trim().to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            value = parts
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
        }
        let value = value.trim_matches(['"', '\'']).to_string();
        match key.trim() {
            "name" => name = Some(value).filter(|value| !value.is_empty()),
            "description" => description = value,
            _ => {}
        }
    }
    (name, description, body.to_string())
}

/// The folders skills are kept in, by the agent that reads them; each is
/// under the home folder for the user's own, or a repository for its.
pub const SOURCES: [(&str, &str); 4] = [
    ("Claude Code", ".claude/skills"),
    ("Codex", ".codex/skills"),
    ("Gemini CLI", ".gemini/skills"),
    ("Agent Skills", ".agents/skills"),
];

fn skills_in(folder: &Path, project_id: Option<&str>, source: &str, store: &Store) -> Vec<Found> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<Found> = entries
        .flatten()
        // Codex's own built-in skills live in `.system`.
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            let path = entry.path().join("SKILL.md");
            let text = std::fs::read_to_string(&path).ok()?;
            let (name, description, _) = parse(&text);
            let name = name.unwrap_or_else(|| entry.file_name().to_string_lossy().into_owned());
            let imported = store
                .all()
                .iter()
                .any(|skill| skill.name.eq_ignore_ascii_case(&name));
            Some(Found {
                name,
                description,
                path,
                project_id: project_id.map(str::to_string),
                source: source.to_string(),
                imported,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Every agent skill on this machine that Convoy can see: the user's own,
/// then each project's. A folder reached twice — one agent's linked to
/// another's — is listed once.
pub fn discover(workspace: &Workspace, home: &Path, store: &Store) -> Vec<Found> {
    let mut found = Vec::new();
    for (source, folder) in SOURCES {
        found.extend(skills_in(&home.join(folder), None, source, store));
    }
    for project in &workspace.state().projects {
        for (source, folder) in SOURCES {
            found.extend(skills_in(
                &project.path.join(folder),
                Some(&project.id),
                source,
                store,
            ));
        }
    }
    let mut seen = Vec::new();
    found.retain(|skill| {
        let real = skill
            .path
            .canonicalize()
            .unwrap_or_else(|_| skill.path.clone());
        if seen.contains(&real) {
            return false;
        }
        seen.push(real);
        true
    });
    found
}

/// The text a skill is imported with: its own, then where its other files
/// are, when it has any.
pub fn body(path: &Path) -> Result<(String, String, String)> {
    let text = std::fs::read_to_string(path)?;
    let (name, description, body) = parse(&text);
    let folder = path.parent().unwrap_or(Path::new(""));
    let name = name.unwrap_or_else(|| {
        folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Imported skill".into())
    });
    let others = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_name() != "SKILL.md")
        })
        .unwrap_or(false);
    let mut body = body;
    if others {
        body.push_str(&format!(
            "\n\n---\nThis skill's own files (scripts, templates) are in {}: read and run them from there.",
            folder.display()
        ));
    }
    if utf16_len(&body) > MAX_BODY {
        let note = format!(
            "\n\n…(shortened; read the whole skill at {})",
            path.display()
        );
        let keep = MAX_BODY - utf16_len(&note) - 10;
        let mut cut = String::new();
        for c in body.chars() {
            if utf16_len(&cut) + c.len_utf16() > keep {
                break;
            }
            cut.push(c);
        }
        body = cut + &note;
    }
    let description: String = description.chars().take(super::MAX_DESCRIPTION).collect();
    Ok((name, description, body))
}

/// Imports one into the library for these projects (none is every
/// project) and roles, or refreshes the one of its name. Returns its id.
pub fn import(
    store: &mut Store,
    workspace: &Workspace,
    path: &Path,
    projects: Vec<String>,
    roles: Option<Vec<Role>>,
    auto: bool,
    now: Instant,
) -> Result<String> {
    let (name, description, body) = body(path)?;
    let existing = store
        .all()
        .iter()
        .find(|skill| skill.name.eq_ignore_ascii_case(&name))
        .map(|skill| (skill.id.clone(), skill.agents));
    let agents = match existing
        .as_ref()
        .map(|(_, agents)| *agents)
        .unwrap_or_default()
    {
        Agents::Claude => "claude",
        Agents::Codex => "codex",
        Agents::Both => "both",
    };
    store.save(
        SkillInput {
            id: existing.map(|(id, _)| id),
            name,
            description,
            body,
            project_id: None,
            agents: agents.into(),
            auto,
            roles,
            projects: Some(projects),
        },
        workspace,
        now,
    )
}
