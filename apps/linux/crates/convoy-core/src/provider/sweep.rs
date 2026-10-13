//! "Import all history": every Claude Code and Codex conversation on this
//! computer, each matched to the Convoy project it most likely belongs to.
//!
//! The rules are the macOS app's. A conversation started inside a project, or
//! inside one of its sessions' worktrees, belongs to it — the deepest folder
//! wins. One started elsewhere (`~`, a parent folder) is matched by the paths
//! its tool calls touched, and only when one project clearly leads.
//!
//! Nothing the agents saved is moved or deleted. A Claude conversation that has
//! to resume from another folder is copied into that folder's slug, and never
//! over a file that is already there.

use super::transcripts;
use crate::patterns::is_uuid;
use crate::workspace::model::{Agent, Session, State};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

/// How much of one transcript is read for its tool calls.
const READ_LIMIT: u64 = 32 * 1024 * 1024;

/// Where the agents keep their conversations, and the home `~` stands for.
#[derive(Debug, Clone)]
pub struct Roots {
    /// `CLAUDE_CONFIG_DIR`, or `~/.claude`; conversations are in `projects/`.
    pub claude: PathBuf,
    /// `CODEX_HOME`, or `~/.codex`; conversations are in `sessions/`.
    pub codex: PathBuf,
    pub home: PathBuf,
}

impl Roots {
    pub fn claude_projects(&self) -> PathBuf {
        self.claude.join("projects")
    }

    pub fn agent_home(&self, agent: Agent) -> &Path {
        match agent {
            Agent::Claude => &self.claude,
            Agent::Codex => &self.codex,
        }
    }
}

/// A folder that stands for a project: its own path, or a session's worktree.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub project_id: String,
    pub path: PathBuf,
    pub alias: bool,
}

/// Every project's folder, then every session's working directory as an alias
/// of its project. Paths are normalised once here.
pub fn targets(state: &State, home: &Path) -> Vec<Target> {
    let mut result: Vec<Target> = state
        .projects
        .iter()
        .filter_map(|project| {
            Some(Target {
                project_id: project.id.clone(),
                path: normalise(&project.path.to_string_lossy(), home, None)?,
                alias: false,
            })
        })
        .collect();
    for session in &state.sessions {
        let Some(folder) = &session.working_directory else {
            continue;
        };
        if !state.projects.iter().any(|p| p.id == session.project_id) {
            continue;
        }
        let Some(path) = normalise(&folder.to_string_lossy(), home, None) else {
            continue;
        };
        if !result
            .iter()
            .any(|target| target.project_id == session.project_id && same(&target.path, &path))
        {
            result.push(Target {
                project_id: session.project_id.clone(),
                path,
                alias: true,
            });
        }
    }
    result
}

/// Every provider id already recorded as a session, archived ones included.
pub fn known(state: &State) -> HashSet<String> {
    state
        .sessions
        .iter()
        .filter(|session| !session.provider_id.is_empty())
        .map(|session| session.provider_id.to_lowercase())
        .collect()
}

/// Why a conversation was matched to its project.
#[derive(Debug, Clone, PartialEq)]
pub enum Reason {
    /// It was started in the project's folder or one of its worktrees.
    Folder,
    /// Its tool calls referred to files in the project this many times.
    Paths(u32),
    Unassigned,
}

#[derive(Debug, Clone)]
pub struct Found {
    pub agent: Agent,
    pub provider_id: String,
    pub title: String,
    /// Modification time in milliseconds since the epoch.
    pub at: f64,
    /// Where it was started, normalised; unknown for a Claude file with none.
    pub cwd: Option<PathBuf>,
    pub file: PathBuf,
    pub project_id: Option<String>,
    pub reason: Reason,
    /// For an unassigned conversation, up to three projects it mentioned.
    pub suggestions: Vec<String>,
}

/// Claude's folder name for a working directory: every character that is not
/// an ASCII letter or digit becomes `-`.
pub fn slug(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// An absolute path with `~/` expanded, `.` and `..` resolved and no trailing
/// separator. A relative one is taken against `base`; without one it is none.
pub fn normalise(text: &str, home: &Path, base: Option<&Path>) -> Option<PathBuf> {
    let joined = if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        let path = Path::new(text);
        if path.is_absolute() || text.starts_with('/') {
            path.to_path_buf()
        } else {
            base?.join(path)
        }
    };
    let mut clean = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

/// Path components as compared: folded on Windows, whose file system ignores
/// case; exact elsewhere.
fn parts(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let text = component.as_os_str().to_string_lossy();
            if cfg!(windows) {
                text.to_lowercase()
            } else {
                text.into_owned()
            }
        })
        .collect()
}

fn same(a: &Path, b: &Path) -> bool {
    parts(a) == parts(b)
}

/// `child` is `parent` or inside it. Both are already normalised.
pub fn contains(parent: &Path, child: &Path) -> bool {
    parts(child).starts_with(&parts(parent))
}

/// The deepest project folder or worktree a conversation was started in.
pub fn folder_match<'a>(cwd: &Path, targets: &'a [Target]) -> Option<&'a Target> {
    targets
        .iter()
        .filter(|target| contains(&target.path, cwd))
        .max_by_key(|target| target.path.components().count())
}

const SEPARATORS: &[char] = &[
    '"', '\'', '`', '=', ',', ';', ':', '(', ')', '[', ']', '{', '}', '<', '>', '|', '&',
];

/// The words of a tool call's text that look like paths.
pub fn path_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| character.is_whitespace() || SEPARATORS.contains(&character))
        .filter(|token| token.starts_with('/') || token.starts_with("~/") || token.contains('/'))
}

/// Every string in a value, with an array of strings (a command's argument
/// vector) joined by spaces.
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_string) => out.push(
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

/// The text of each tool call in a transcript, one list per call. Only lines
/// that can hold a call are parsed, and only the first 32 MB is read.
pub fn tool_calls(agent: Agent, file: &Path) -> Vec<Vec<String>> {
    let Ok(handle) = fs::File::open(file) else {
        return Vec::new();
    };
    let mut reader = BufReader::new(handle.take(READ_LIMIT));
    let needles: &[&str] = match agent {
        Agent::Claude => &["\"tool_use\""],
        Agent::Codex => &["\"function_call\"", "\"custom_tool_call\""],
    };
    let mut calls = Vec::new();
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        if !needles.iter().any(|needle| text.contains(needle)) {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        match agent {
            Agent::Claude => {
                let parts = record
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(Value::as_array);
                for part in parts.into_iter().flatten() {
                    if part.get("type").and_then(Value::as_str) != Some("tool_use") {
                        continue;
                    }
                    let mut found = Vec::new();
                    if let Some(input) = part.get("input") {
                        strings(input, &mut found);
                    }
                    calls.push(found);
                }
            }
            Agent::Codex => {
                let Some(payload) = record.get("payload") else {
                    continue;
                };
                let mut found = Vec::new();
                match payload.get("type").and_then(Value::as_str) {
                    Some("function_call") => match payload.get("arguments") {
                        Some(Value::String(raw)) => match serde_json::from_str::<Value>(raw) {
                            Ok(arguments) => strings(&arguments, &mut found),
                            Err(_) => found.push(raw.clone()),
                        },
                        Some(other) => strings(other, &mut found),
                        None => {}
                    },
                    Some("custom_tool_call") => {
                        if let Some(input) = payload.get("input") {
                            strings(input, &mut found);
                        }
                    }
                    _ => continue,
                }
                calls.push(found);
            }
        }
    }
    calls
}

/// One vote per tool call for each project it touched — the deepest one
/// holding each path — most votes first; a tie keeps the projects' order.
pub fn votes(
    calls: &[Vec<String>],
    cwd: Option<&Path>,
    home: &Path,
    targets: &[Target],
) -> Vec<(String, u32)> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for call in calls {
        let mut touched: HashSet<&str> = HashSet::new();
        for text in call {
            for token in path_tokens(text) {
                let Some(path) = normalise(token, home, cwd) else {
                    continue;
                };
                // The deepest folder holding the path, so a project nested
                // inside another is not tied by its parent on every call.
                let owner = targets
                    .iter()
                    .filter(|target| contains(&target.path, &path))
                    .max_by_key(|target| target.path.as_os_str().len());
                if let Some(target) = owner {
                    touched.insert(target.project_id.as_str());
                }
            }
        }
        for project in touched {
            *counts.entry(project).or_default() += 1;
        }
    }
    let order = |id: &str| targets.iter().position(|target| target.project_id == id);
    let mut result: Vec<(String, u32)> = counts
        .into_iter()
        .map(|(id, count)| (id.to_string(), count))
        .collect();
    result.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| order(&a.0).cmp(&order(&b.0))));
    result
}

/// Assigned when the leader has at least two votes and more than twice the
/// runner-up's; otherwise unassigned, suggesting the three it mentioned most.
pub fn decide(votes: &[(String, u32)]) -> (Option<String>, Reason, Vec<String>) {
    let runner_up = votes.get(1).map(|vote| vote.1).unwrap_or(0);
    match votes.first() {
        Some((id, count)) if *count >= 2 && *count > runner_up * 2 => {
            (Some(id.clone()), Reason::Paths(*count), Vec::new())
        }
        _ => (
            None,
            Reason::Unassigned,
            votes.iter().take(3).map(|vote| vote.0.clone()).collect(),
        ),
    }
}

fn modified(file: &Path) -> f64 {
    fs::metadata(file)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Everything an agent saved for one conversation under its home: Claude's
/// `<id>.jsonl` and its `<id>/` folder in every project folder, Codex's
/// rollout. Empty for an id that is not a UUID, so nothing else can match.
pub fn conversation_files(agent: Agent, home: &Path, id: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if !is_uuid(id) {
        return found;
    }
    match agent {
        Agent::Claude => {
            let Ok(folders) = fs::read_dir(home.join("projects")) else {
                return found;
            };
            for folder in folders.flatten() {
                for path in [
                    folder.path().join(format!("{id}.jsonl")),
                    folder.path().join(id),
                ] {
                    if fs::symlink_metadata(&path).is_ok() {
                        found.push(path);
                    }
                }
            }
        }
        Agent::Codex => {
            let mut files = Vec::new();
            rollouts(&home.join("sessions"), 0, &mut files);
            let suffix = format!("-{}.jsonl", id.to_lowercase());
            found.extend(files.into_iter().filter(|file| {
                file.file_name()
                    .is_some_and(|name| name.to_string_lossy().to_lowercase().ends_with(&suffix))
            }));
        }
    }
    found
}

pub(crate) fn rollouts(folder: &Path, depth: u32, files: &mut Vec<PathBuf>) {
    if depth > 8 {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => rollouts(&path, depth + 1, files),
            Ok(kind) if kind.is_file() => {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                    files.push(path);
                }
            }
            _ => {}
        }
    }
}

/// A conversation as read from disk, before it is matched.
struct Raw {
    agent: Agent,
    id: String,
    title: String,
    cwd: Option<String>,
    file: PathBuf,
}

fn claude_conversations(root: &Path) -> Vec<Raw> {
    let mut result = Vec::new();
    let Ok(folders) = fs::read_dir(root) else {
        return result;
    };
    for folder in folders.flatten() {
        if !folder.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let Ok(files) = fs::read_dir(folder.path()) else {
            continue;
        };
        for entry in files.flatten() {
            let file = entry.path();
            if file.extension().is_none_or(|value| value != "jsonl") {
                continue;
            }
            let id = file
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let empty = entry
                .metadata()
                .map(|metadata| !metadata.is_file() || metadata.len() == 0)
                .unwrap_or(true);
            if !is_uuid(&id) || empty {
                continue;
            }
            let records = transcripts::head(&file);
            let cwd = records
                .iter()
                .find_map(|record| record.get("cwd").and_then(Value::as_str))
                .filter(|cwd| !cwd.is_empty())
                .map(str::to_string);
            result.push(Raw {
                agent: Agent::Claude,
                title: transcripts::named(Agent::Claude, &file, &id, &HashMap::new())
                    .unwrap_or_else(|| transcripts::title(Agent::Claude, &records, &id)),
                id,
                cwd,
                file,
            });
        }
    }
    result
}

fn codex_conversations(home: &Path) -> Vec<Raw> {
    let mut files = Vec::new();
    rollouts(&home.join("sessions"), 0, &mut files);
    let names = transcripts::codex_names(home);
    let mut result = Vec::new();
    for file in files {
        let records = transcripts::head(&file);
        let Some(meta) = records
            .iter()
            .find(|record| record.get("type").and_then(Value::as_str) == Some("session_meta"))
            .and_then(|record| record.get("payload"))
        else {
            continue;
        };
        let id = meta
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let subagent = meta
            .get("source")
            .and_then(|source| source.get("subagent"))
            .is_some_and(crate::json::truthy);
        let nested = meta
            .get("parent_thread_id")
            .is_some_and(crate::json::truthy);
        if !is_uuid(&id) || subagent || nested {
            continue;
        }
        let cwd = meta
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.is_empty())
            .map(str::to_string);
        result.push(Raw {
            agent: Agent::Codex,
            title: transcripts::named(Agent::Codex, &file, &id, &names)
                .unwrap_or_else(|| transcripts::title(Agent::Codex, &records, &id)),
            id,
            cwd,
            file,
        });
    }
    result
}

/// Every conversation not yet in the workspace, matched to a project where
/// the rules allow, newest first. Reads the disk; run it off the UI thread.
pub fn scan(roots: &Roots, targets: &[Target], known: &HashSet<String>) -> Vec<Found> {
    let mut raw = claude_conversations(&roots.claude_projects());
    raw.extend(codex_conversations(&roots.codex));

    // The same conversation in two places counts once: the newest copy.
    let mut newest: HashMap<String, (f64, Raw)> = HashMap::new();
    for conversation in raw {
        let key = conversation.id.to_lowercase();
        if known.contains(&key) {
            continue;
        }
        let at = modified(&conversation.file);
        if newest.get(&key).is_none_or(|(seen, _)| at > *seen) {
            newest.insert(key, (at, conversation));
        }
    }

    let mut result: Vec<Found> = newest
        .into_values()
        .map(|(at, conversation)| {
            let cwd = conversation
                .cwd
                .as_deref()
                .and_then(|cwd| normalise(cwd, &roots.home, None));
            let (project_id, reason, suggestions) =
                match cwd.as_deref().and_then(|cwd| folder_match(cwd, targets)) {
                    Some(target) => (Some(target.project_id.clone()), Reason::Folder, Vec::new()),
                    None => {
                        let calls = tool_calls(conversation.agent, &conversation.file);
                        decide(&votes(&calls, cwd.as_deref(), &roots.home, targets))
                    }
                };
            Found {
                agent: conversation.agent,
                provider_id: conversation.id,
                title: conversation.title,
                at,
                cwd,
                file: conversation.file,
                project_id,
                reason,
                suggestions,
            }
        })
        .collect();
    result.sort_by(|a, b| b.at.total_cmp(&a.at));
    result
}

/// Where an imported conversation resumes, and whether Claude's file has to
/// be copied for that.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// None runs it in the project's own folder.
    pub working_directory: Option<PathBuf>,
    /// The Claude folder to copy the transcript into first.
    pub copy_into: Option<PathBuf>,
}

/// How a conversation joins the project the user chose. Started in that
/// project's folder, or in a subfolder or worktree that still exists, it
/// resumes right there. Otherwise it runs in the project's folder, and Claude
/// — which finds conversations by folder — gets a copy filed under it. Codex
/// finds them by id, so it never needs one.
pub fn place(
    found: &Found,
    project_id: &str,
    targets: &[Target],
    roots: &Roots,
) -> Option<Placement> {
    let project = targets
        .iter()
        .find(|target| !target.alias && target.project_id == project_id)?;
    if let Some(cwd) = &found.cwd {
        let inside = targets
            .iter()
            .filter(|target| target.project_id == project_id)
            .any(|target| contains(&target.path, cwd));
        if inside && same(cwd, &project.path) {
            return Some(Placement {
                working_directory: None,
                copy_into: None,
            });
        }
        if inside && cwd.is_dir() {
            return Some(Placement {
                working_directory: Some(cwd.clone()),
                copy_into: None,
            });
        }
    }
    let copy_into = match found.agent {
        Agent::Claude => {
            let into = roots.claude_projects().join(slug(&project.path));
            (found.file.parent() != Some(into.as_path())).then_some(into)
        }
        Agent::Codex => None,
    };
    Some(Placement {
        working_directory: None,
        copy_into,
    })
}

/// Creates `to` from `from`, never replacing a file already there. A copy that
/// fails part-way is removed, so a retry is not mistaken for a finished one.
fn copy_new(from: &Path, to: &Path) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Ok(());
    }
    let mut source = fs::File::open(from)?;
    let mut target = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    if let Err(error) = io::copy(&mut source, &mut target).and_then(|_| target.sync_all()) {
        drop(target);
        let _ = fs::remove_file(to);
        return Err(error);
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination)?;
        } else if kind.is_file() {
            copy_new(&entry.path(), &destination)?;
        }
    }
    Ok(())
}

/// Copies a Claude transcript, and the folder of the same name beside it when
/// there is one, into another project folder. The original is left as it is.
pub fn copy_transcript(file: &Path, into: &Path) -> io::Result<()> {
    let name = file
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Not a transcript."))?;
    fs::create_dir_all(into)?;
    copy_new(file, &into.join(name))?;
    if let (Some(parent), Some(stem)) = (file.parent(), file.file_stem()) {
        let sibling = parent.join(stem);
        if sibling.is_dir() {
            copy_tree(&sibling, &into.join(stem))?;
        }
    }
    Ok(())
}

/// A conversation the user chose to import, placed.
#[derive(Debug, Clone)]
pub struct Import {
    pub agent: Agent,
    pub provider_id: String,
    pub title: String,
    pub project_id: String,
    pub working_directory: Option<PathBuf>,
    pub agent_home: PathBuf,
    pub at: f64,
}

/// Adds the imports as resumable sessions, oldest first, as the single-row
/// import does: started, so opening one resumes it; not launched. One already
/// recorded is skipped. Returns how many were added.
pub fn record(state: &mut State, mut imports: Vec<Import>) -> usize {
    imports.sort_by(|a, b| a.at.total_cmp(&b.at));
    let mut seen = known(state);
    let mut added = 0;
    for import in imports {
        if !state.projects.iter().any(|p| p.id == import.project_id)
            || !seen.insert(import.provider_id.to_lowercase())
        {
            continue;
        }
        let title = if import.title.trim().is_empty() {
            "Imported session".to_string()
        } else {
            import.title.trim().to_string()
        };
        let mut session = Session::new(import.project_id, import.agent, title);
        session.id = crate::workspace::new_id();
        session.provider_id = import.provider_id;
        session.started = true;
        session.model = Some(String::new());
        session.notes = Some(String::new());
        session.archived = Some(false);
        session.pinned = Some(false);
        session.working_directory = import.working_directory;
        session.agent_home = Some(import.agent_home);
        state.sessions.push(session);
        added += 1;
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::model::Project;
    use std::io::Write;

    const A: &str = "0199aaaa-0000-4000-8000-000000000001";
    const B: &str = "0199aaaa-0000-4000-8000-000000000002";
    const C: &str = "0199aaaa-0000-4000-8000-000000000003";

    struct Fixture {
        _dir: tempfile::TempDir,
        roots: Roots,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().to_path_buf();
        let roots = Roots {
            claude: base.join("claude"),
            codex: base.join("codex"),
            home: base.join("home"),
        };
        fs::create_dir_all(roots.home.join("Projects/app/src")).unwrap();
        fs::create_dir_all(roots.home.join("Projects/web")).unwrap();
        Fixture { _dir: dir, roots }
    }

    fn target(id: &str, path: &Path, alias: bool) -> Target {
        Target {
            project_id: id.into(),
            path: path.to_path_buf(),
            alias,
        }
    }

    fn write(file: &Path, lines: &[Value]) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let mut handle = fs::File::create(file).unwrap();
        for line in lines {
            writeln!(handle, "{line}").unwrap();
        }
    }

    fn claude_file(roots: &Roots, cwd: &Path, id: &str, tools: &[Value]) -> PathBuf {
        let file = roots
            .claude_projects()
            .join(slug(cwd))
            .join(format!("{id}.jsonl"));
        let mut lines = vec![serde_json::json!({
            "type": "user", "cwd": cwd.to_string_lossy(),
            "message": { "role": "user", "content": "Fix the build" }
        })];
        for input in tools {
            lines.push(serde_json::json!({
                "type": "assistant",
                "message": { "content": [{ "type": "tool_use", "name": "Bash", "input": input }] }
            }));
        }
        write(&file, &lines);
        file
    }

    fn codex_file(roots: &Roots, name: &str, meta: Value, calls: &[Value]) -> PathBuf {
        let file = roots.codex.join("sessions/2026/10/01").join(name);
        let mut lines = vec![serde_json::json!({ "type": "session_meta", "payload": meta })];
        lines.extend(
            calls
                .iter()
                .map(|payload| serde_json::json!({ "type": "response_item", "payload": payload })),
        );
        write(&file, &lines);
        file
    }

    #[test]
    fn slug_replaces_every_other_character() {
        assert_eq!(slug(Path::new("/Users/me/my.app_x")), "-Users-me-my-app-x");
    }

    #[test]
    fn normalise_expands_home_and_resolves_dots() {
        let home = Path::new("/home/me");
        assert_eq!(
            normalise("~/a/./b/../c/", home, None),
            Some(PathBuf::from("/home/me/a/c"))
        );
        assert_eq!(
            normalise("src/x.rs", home, Some(home)),
            Some(PathBuf::from("/home/me/src/x.rs"))
        );
        assert_eq!(normalise("src/x.rs", home, None), None);
        assert!(contains(Path::new("/a/b"), Path::new("/a/b")));
        assert!(contains(Path::new("/a/b"), Path::new("/a/b/c")));
        assert!(!contains(Path::new("/a/b"), Path::new("/a/bc")));
    }

    #[test]
    fn folder_match_takes_the_deepest_and_worktrees() {
        let targets = vec![
            target("outer", Path::new("/w"), false),
            target("inner", Path::new("/w/inner"), false),
            target("outer", Path::new("/trees/fix"), true),
        ];
        assert_eq!(
            folder_match(Path::new("/w/inner/src"), &targets)
                .unwrap()
                .project_id,
            "inner"
        );
        assert_eq!(
            folder_match(Path::new("/w/other"), &targets)
                .unwrap()
                .project_id,
            "outer"
        );
        assert_eq!(
            folder_match(Path::new("/trees/fix/a"), &targets)
                .unwrap()
                .project_id,
            "outer"
        );
        assert!(folder_match(Path::new("/elsewhere"), &targets).is_none());
    }

    #[test]
    fn scan_matches_by_folder_and_by_paths() {
        let f = fixture();
        let home = &f.roots.home;
        let app = home.join("Projects/app");
        let web = home.join("Projects/web");
        let targets = vec![target("app", &app, false), target("web", &web, false)];
        // Started inside the project.
        claude_file(&f.roots, &app.join("src"), A, &[]);
        // Started in ~: absolute, ~/ and relative-to-~ references to app,
        // one noisy mention of web.
        claude_file(
            &f.roots,
            home,
            B,
            &[
                serde_json::json!({ "command": format!("cat {}/README.md", app.display()) }),
                serde_json::json!({ "file_path": "~/Projects/app/src/main.rs" }),
                serde_json::json!({ "command": "cd Projects/app && make" }),
                serde_json::json!({ "command": "ls Projects/web/" }),
            ],
        );
        // A single mention is not enough.
        claude_file(
            &f.roots,
            home,
            C,
            &[serde_json::json!({ "command": "ls ~/Projects/app" })],
        );
        let found = scan(&f.roots, &targets, &HashSet::new());
        let by = |id: &str| found.iter().find(|item| item.provider_id == id).unwrap();
        assert_eq!(by(A).project_id.as_deref(), Some("app"));
        assert_eq!(by(A).reason, Reason::Folder);
        assert_eq!(by(A).title, "Fix the build");
        assert_eq!(by(B).project_id.as_deref(), Some("app"));
        assert_eq!(by(B).reason, Reason::Paths(3));
        assert_eq!(by(C).project_id, None);
        assert_eq!(by(C).reason, Reason::Unassigned);
        assert_eq!(by(C).suggestions, vec!["app".to_string()]);
    }

    #[test]
    fn a_nested_project_takes_the_votes_not_its_parent() {
        let targets = vec![
            target("mono", Path::new("/p/mono"), false),
            target("api", Path::new("/p/mono/api"), false),
        ];
        let calls = vec![
            vec!["vim /p/mono/api/a".to_string()],
            vec!["vim /p/mono/api/b".to_string()],
            vec!["cat /p/mono/README.md".to_string()],
        ];
        let tally = votes(&calls, None, Path::new("/home"), &targets);
        assert_eq!(tally, vec![("api".into(), 2), ("mono".into(), 1)]);
        assert_eq!(decide(&tally).0.as_deref(), None);
        let calls = [calls, vec![vec!["vim /p/mono/api/c".to_string()]]].concat();
        let tally = votes(&calls, None, Path::new("/home"), &targets);
        assert_eq!(decide(&tally).0.as_deref(), Some("api"));
    }

    #[test]
    fn a_tie_is_unassigned_with_both_suggested() {
        let targets = vec![
            target("app", Path::new("/p/app"), false),
            target("web", Path::new("/p/web"), false),
        ];
        let calls = vec![
            vec!["vim /p/app/a".to_string()],
            vec!["vim /p/app/b".to_string()],
            vec!["diff /p/web/a /p/web/b".to_string()],
            vec!["vim /p/web/c".to_string()],
        ];
        let tally = votes(&calls, None, Path::new("/home"), &targets);
        assert_eq!(tally, vec![("app".into(), 2), ("web".into(), 2)]);
        let (project, reason, suggestions) = decide(&tally);
        assert_eq!((project, reason), (None, Reason::Unassigned));
        assert_eq!(suggestions, vec!["app".to_string(), "web".to_string()]);
        // Twice the runner-up is not more than twice.
        assert_eq!(
            decide(&[("app".into(), 4), ("web".into(), 2)]).1,
            Reason::Unassigned
        );
        assert_eq!(
            decide(&[("app".into(), 5), ("web".into(), 2)]).1,
            Reason::Paths(5)
        );
    }

    #[test]
    fn codex_reads_function_and_custom_calls_and_skips_subagents() {
        let f = fixture();
        let app = f.roots.home.join("Projects/app");
        let targets = vec![target("app", &app, false)];
        codex_file(
            &f.roots,
            "rollout-1.jsonl",
            serde_json::json!({ "id": A, "cwd": f.roots.home.to_string_lossy() }),
            &[
                serde_json::json!({ "type": "function_call", "arguments":
                    serde_json::json!({ "command": ["bash", "-lc", "make"], "workdir": app.to_string_lossy() }).to_string() }),
                serde_json::json!({ "type": "custom_tool_call", "input": "*** Update File: Projects/app/src/main.rs" }),
                serde_json::json!({ "type": "function_call_output", "output": "/p/app" }),
            ],
        );
        codex_file(
            &f.roots,
            "rollout-2.jsonl",
            serde_json::json!({ "id": B, "cwd": app.to_string_lossy(), "source": { "subagent": "review" } }),
            &[],
        );
        codex_file(
            &f.roots,
            "rollout-3.jsonl",
            serde_json::json!({ "id": C, "cwd": app.to_string_lossy(), "parent_thread_id": A }),
            &[],
        );
        let found = scan(&f.roots, &targets, &HashSet::new());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].agent, Agent::Codex);
        assert_eq!(found[0].reason, Reason::Paths(2));
    }

    #[test]
    fn scan_skips_known_empty_and_non_uuid_files() {
        let f = fixture();
        let app = f.roots.home.join("Projects/app");
        let targets = vec![target("app", &app, false)];
        claude_file(&f.roots, &app, A, &[]);
        claude_file(&f.roots, &app, "agent-1234", &[]);
        let empty = f
            .roots
            .claude_projects()
            .join(slug(&app))
            .join(format!("{B}.jsonl"));
        fs::File::create(&empty).unwrap();
        let known: HashSet<String> = [A.to_uppercase().to_lowercase()].into_iter().collect();
        assert!(scan(&f.roots, &targets, &known).is_empty());
        assert_eq!(scan(&f.roots, &targets, &HashSet::new()).len(), 1);
    }

    #[test]
    fn placement_and_copy() {
        let f = fixture();
        let home = &f.roots.home;
        let app = home.join("Projects/app");
        let tree = home.join("trees/app-fix");
        fs::create_dir_all(&tree).unwrap();
        let targets = vec![target("app", &app, false), target("app", &tree, true)];

        // Started in ~: copied into the project's folder, original intact,
        // with the sibling folder of the same name.
        let original = claude_file(&f.roots, home, A, &[]);
        let sibling = original.with_extension("");
        write(
            &sibling.join("subagents/x.jsonl"),
            &[serde_json::json!({ "a": 1 })],
        );
        let found = scan(&f.roots, &targets, &HashSet::new());
        let row = found.iter().find(|item| item.provider_id == A).unwrap();
        let placement = place(row, "app", &targets, &f.roots).unwrap();
        let into = f.roots.claude_projects().join(slug(&app));
        assert_eq!(
            placement,
            Placement {
                working_directory: None,
                copy_into: Some(into.clone())
            }
        );
        copy_transcript(&row.file, &into).unwrap();
        assert_eq!(
            fs::read(&original).unwrap(),
            fs::read(into.join(format!("{A}.jsonl"))).unwrap()
        );
        assert!(into.join(A).join("subagents/x.jsonl").is_file());
        assert!(original.is_file() && sibling.is_dir());

        // Never over an existing file.
        fs::write(into.join(format!("{A}.jsonl")), "kept").unwrap();
        copy_transcript(&row.file, &into).unwrap();
        assert_eq!(
            fs::read_to_string(into.join(format!("{A}.jsonl"))).unwrap(),
            "kept"
        );

        // In the project itself: nothing to do. In an existing worktree or
        // subfolder: resumes there. In one that is gone: copied.
        let mut inside = row.clone();
        inside.cwd = Some(app.clone());
        assert_eq!(
            place(&inside, "app", &targets, &f.roots).unwrap(),
            Placement {
                working_directory: None,
                copy_into: None
            }
        );
        inside.cwd = Some(tree.clone());
        assert_eq!(
            place(&inside, "app", &targets, &f.roots)
                .unwrap()
                .working_directory,
            Some(tree.clone())
        );
        inside.cwd = Some(app.join("gone"));
        assert_eq!(
            place(&inside, "app", &targets, &f.roots).unwrap(),
            Placement {
                working_directory: None,
                copy_into: Some(into.clone())
            }
        );
        // Codex is never copied.
        inside.agent = Agent::Codex;
        assert_eq!(
            place(&inside, "app", &targets, &f.roots).unwrap().copy_into,
            None
        );
    }

    #[test]
    fn record_adds_started_sessions_oldest_first_once() {
        let mut state = State::default();
        state.projects.push(Project {
            id: "app".into(),
            title: "App".into(),
            path: "/p/app".into(),
            group: None,
            parent_id: None,
            is_group: None,
            color: None,
            icon: None,
            setup_command: None,
            shared_paths: None,
            review_template: None,
            default_agent: None,
            base_ref: None,
            branch_prefix: None,
            task_mode: None,
            auto_run_tasks: None,
            test_commands: None,
            qa_agent: None,
            qa_auto: None,
            qa_timeout: None,
            unknown: Default::default(),
        });
        let import = |id: &str, at: f64| Import {
            agent: Agent::Claude,
            provider_id: id.into(),
            title: format!("t{at}"),
            project_id: "app".into(),
            working_directory: None,
            agent_home: "/h/.claude".into(),
            at,
        };
        assert_eq!(
            record(
                &mut state,
                vec![import(B, 2.0), import(A, 1.0), import(A, 3.0)]
            ),
            2
        );
        assert_eq!(state.sessions[0].provider_id, A);
        assert!(state
            .sessions
            .iter()
            .all(|s| s.started && s.agent_home.is_some()));
        assert_eq!(record(&mut state, vec![import(&A.to_uppercase(), 4.0)]), 0);
    }

    #[test]
    fn a_conversations_files_are_found_by_id_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let id = "0199aaaa-0000-4000-8000-0000000000aa";
        for folder in ["-a", "-b"] {
            fs::create_dir_all(home.join("projects").join(folder)).unwrap();
            fs::write(
                home.join("projects")
                    .join(folder)
                    .join(format!("{id}.jsonl")),
                "{}",
            )
            .unwrap();
        }
        fs::create_dir_all(home.join("projects/-a").join(id).join("subagents")).unwrap();
        fs::write(
            home.join("projects/-a/0199aaaa-0000-4000-8000-0000000000bb.jsonl"),
            "{}",
        )
        .unwrap();
        let mut claude = conversation_files(Agent::Claude, home, id);
        claude.sort();
        assert_eq!(
            claude,
            vec![
                home.join("projects/-a").join(id),
                home.join("projects/-a").join(format!("{id}.jsonl")),
                home.join("projects/-b").join(format!("{id}.jsonl")),
            ]
        );

        let day = home.join("sessions/2026/10/11");
        fs::create_dir_all(&day).unwrap();
        fs::write(
            day.join(format!("rollout-2026-10-11T10-00-00-{id}.jsonl")),
            "{}",
        )
        .unwrap();
        fs::write(
            day.join("rollout-2026-10-11T10-00-00-0199aaaa-0000-4000-8000-0000000000bb.jsonl"),
            "{}",
        )
        .unwrap();
        assert_eq!(
            conversation_files(Agent::Codex, home, &id.to_uppercase()),
            vec![day.join(format!("rollout-2026-10-11T10-00-00-{id}.jsonl"))]
        );
        assert!(conversation_files(Agent::Claude, home, "../projects").is_empty());
    }
}
