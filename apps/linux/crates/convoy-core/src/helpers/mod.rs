//! Manager sessions and the helpers they start.
//!
//! A session marked as a manager can start other sessions in its project from
//! its own shell — `convoy --session spawn …`, then `wait` and `output` — the
//! way a lead hands work out and collects the answers.
//!
//! Nothing here touches the network. The command line writes a request file
//! into the data folder; the running app picks it up, checks the caller's
//! secret, acts, and writes a result file next to it. The secret is made at
//! each manager launch and lives only in memory and in that agent's
//! environment, never in the workspace file, so a request can only come from
//! the agent it was given to.
//!
//! This module holds the rules: the file protocol, the checks, the limits and
//! how a helper's progress maps to what `wait` reports. The app runs them.

pub mod cli;

use crate::chat::ChatEvent;
use crate::json::utf16_len;
use crate::patterns::MODEL;
use crate::workspace::model::{Agent, Session};
use crate::workspace::{NewSession, Workspace};
use crate::{ensure, ConvoyError, Result, Storage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Helpers one manager may have running at once.
pub const MAX_RUNNING: usize = 4;
/// A request or result older than this is abandoned and removed.
pub const STALE_MS: u64 = 5 * 60 * 1000;
/// The app touches its heartbeat this often or more; the command line takes
/// a heartbeat older than [`ALIVE_MS`] to mean Convoy is not running.
pub const ALIVE_MS: u64 = 10_000;
/// The protocol version a request carries.
pub const VERSION: u32 = 1;
/// The largest request file read. A prompt is at most 32,000 UTF-16 units.
const MAX_REQUEST: u64 = 512 * 1024;

pub const ENV_SESSION: &str = "CONVOY_SESSION";
pub const ENV_TOKEN: &str = "CONVOY_TOKEN";
pub const ENV_BIN: &str = "CONVOY_BIN";
/// Where requests go. Optional: without it the command line uses the default
/// data folder, which is the app's own unless a test moved it.
pub const ENV_REQUESTS: &str = "CONVOY_REQUESTS";
/// Every variable a manager launch adds; any other launch has them removed.
pub const ENV_KEYS: [&str; 4] = [ENV_SESSION, ENV_TOKEN, ENV_BIN, ENV_REQUESTS];

/// The folder requests and results pass through.
pub fn requests_dir(storage: &Storage) -> PathBuf {
    storage.telemetry().join("requests")
}

// ---------------------------------------------------------------------------
// The protocol.
// ---------------------------------------------------------------------------

/// What a manager asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    Spawn(Spawn),
    /// Where each helper stands now. `wait` asks this until all are settled.
    Status {
        ids: Vec<String>,
    },
    /// A helper's last answer.
    Output {
        id: String,
    },
    /// The caller's helpers.
    List,
}

/// A helper to start.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Spawn {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// A branch for the helper's own worktree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// A worktree on a branch named after the title.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub worktree_auto: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub prompt: String,
    /// Skills to attach, by name, on top of the automatic ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

/// One request file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    pub id: String,
    /// The calling session, from its `CONVOY_SESSION`.
    pub session: String,
    /// Its secret, from `CONVOY_TOKEN`.
    pub token: String,
    /// When it was written, in milliseconds since the Unix epoch.
    pub at: u64,
    pub command: Command,
}

impl Request {
    pub fn new(session: &str, token: &str, command: Command) -> Self {
        Request {
            version: VERSION,
            id: Uuid::new_v4().to_string(),
            session: session.to_string(),
            token: token.to_string(),
            at: now_ms(),
            command,
        }
    }
}

/// One result file: a value, or the reason there is none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub value: Value,
}

impl Response {
    pub fn value(value: impl Serialize) -> Self {
        Response {
            ok: true,
            error: None,
            value: serde_json::to_value(value).unwrap_or(Value::Null),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Response {
            ok: false,
            error: Some(message.into()),
            value: Value::Null,
        }
    }

    pub fn from_result<T: Serialize>(result: Result<T>) -> Self {
        match result {
            Ok(value) => Response::value(value),
            Err(error) => Response::error(error.to_string()),
        }
    }
}

/// Where a helper stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HelperStatus {
    /// Working on its turn, or waiting for an approval.
    Running,
    /// Its turn ended without an error.
    Done,
    /// Its turn ended with an error, or its process failed.
    Failed,
    /// Stopped before it finished.
    Stopped,
    /// Still running when `wait` gave up. Only `wait` says this.
    Timeout,
}

impl HelperStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            HelperStatus::Running => "running",
            HelperStatus::Done => "done",
            HelperStatus::Failed => "failed",
            HelperStatus::Stopped => "stopped",
            HelperStatus::Timeout => "timeout",
        }
    }

    /// Whether `wait` stops waiting for it.
    pub fn settled(self) -> bool {
        self != HelperStatus::Running
    }
}

/// A helper as `status`, `list` and `wait` report it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelperView {
    pub id: String,
    pub status: HelperStatus,
    pub title: String,
    #[serde(default)]
    pub agent: String,
}

// ---------------------------------------------------------------------------
// Files.
// ---------------------------------------------------------------------------

const REQUEST_SUFFIX: &str = ".json";
const RESULT_SUFFIX: &str = ".result.json";
const HEARTBEAT: &str = ".alive";

pub fn request_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}{REQUEST_SUFFIX}"))
}

pub fn result_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}{RESULT_SUFFIX}"))
}

/// The folder, private to this user.
fn ensure_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Written under a name no reader looks at, then renamed into place, so a
/// reader never sees half a file.
fn write_atomic(dir: &Path, name: &Path, body: &[u8]) -> Result<()> {
    ensure_dir(dir)?;
    let temporary = dir.join(format!(".{}.tmp", Uuid::new_v4()));
    let written = (|| -> Result<()> {
        let mut handle = crate::fs::create_private(&temporary)?;
        handle.write_all(body)?;
        drop(handle);
        fs::rename(&temporary, name)?;
        Ok(())
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// Ids are UUIDs: nothing from a request reaches a path otherwise.
fn valid_id(id: &str) -> bool {
    crate::patterns::UUID.is_match(id)
}

pub fn write_request(dir: &Path, request: &Request) -> Result<PathBuf> {
    ensure!(valid_id(&request.id), "Invalid request.");
    let path = request_path(dir, &request.id);
    write_atomic(dir, &path, serde_json::to_string(request)?.as_bytes())?;
    Ok(path)
}

pub fn read_request(path: &Path) -> Result<Request> {
    let size = fs::metadata(path)?.len();
    ensure!(size <= MAX_REQUEST, "The request is too large.");
    let request: Request = serde_json::from_str(&fs::read_to_string(path)?)
        .map_err(|_| ConvoyError::message("Malformed request."))?;
    ensure!(
        request.version == VERSION,
        "This request was written by another version of Convoy."
    );
    ensure!(valid_id(&request.id), "Invalid request.");
    Ok(request)
}

pub fn write_result(dir: &Path, id: &str, response: &Response) -> Result<()> {
    ensure!(valid_id(id), "Invalid request.");
    write_atomic(
        dir,
        &result_path(dir, id),
        serde_json::to_string(response)?.as_bytes(),
    )
}

/// The result, once it exists. Taken: the file is removed as it is read.
pub fn take_result(dir: &Path, id: &str) -> Option<Response> {
    let path = result_path(dir, id);
    let text = fs::read_to_string(&path).ok()?;
    let response = serde_json::from_str(&text).ok()?;
    let _ = fs::remove_file(&path);
    Some(response)
}

/// Request files waiting to be handled, oldest first.
pub fn pending(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let wanted = !name.starts_with('.')
                && name.ends_with(REQUEST_SUFFIX)
                && !name.ends_with(RESULT_SUFFIX);
            if !wanted {
                return None;
            }
            let modified = entry.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((modified, entry.path()))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, path)| path).collect()
}

/// Removes requests, results and half-written files nobody came back for.
/// Returns how many went.
pub fn cleanup(dir: &Path, now: std::time::SystemTime) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == HEARTBEAT {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age.as_millis() as u64 > STALE_MS);
        if stale && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Says the app is here to answer.
pub fn heartbeat(dir: &Path) -> Result<()> {
    write_atomic(
        dir,
        &dir.join(HEARTBEAT),
        std::process::id().to_string().as_bytes(),
    )
}

/// Whether the app touched its heartbeat recently.
pub fn alive(dir: &Path, now: std::time::SystemTime) -> bool {
    fs::metadata(dir.join(HEARTBEAT))
        .and_then(|meta| meta.modified())
        .ok()
        .is_some_and(|modified| match now.duration_since(modified) {
            Ok(age) => (age.as_millis() as u64) < ALIVE_MS,
            // Written a moment "after" now: a clock step, still alive.
            Err(_) => true,
        })
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Secrets.
// ---------------------------------------------------------------------------

/// A fresh secret: 244 random bits from the system's generator.
pub fn new_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Compares in time that does not depend on where they differ.
pub fn tokens_match(expected: &str, given: &str) -> bool {
    let (a, b) = (expected.as_bytes(), given.as_bytes());
    if a.is_empty() || a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The secret each running manager was launched with. In memory only.
#[derive(Debug, Default)]
pub struct Tokens {
    by_session: HashMap<String, String>,
}

impl Tokens {
    /// A new secret for a manager launch, replacing any earlier one.
    pub fn issue(&mut self, session: &str) -> String {
        let token = new_token();
        self.by_session.insert(session.to_string(), token.clone());
        token
    }

    pub fn revoke(&mut self, session: &str) {
        self.by_session.remove(session);
    }

    pub fn check(&self, session: &str, token: &str) -> bool {
        self.by_session
            .get(session)
            .is_some_and(|expected| tokens_match(expected, token))
    }
}

/// The caller, once its secret and its standing are checked. Only a manager
/// launched by this app, holding the secret it was given, gets through.
pub fn authorize<'a>(
    workspace: &'a Workspace,
    tokens: &Tokens,
    request: &Request,
) -> Result<&'a Session> {
    ensure!(
        tokens.check(&request.session, &request.token),
        "This session cannot start helpers. Start it from Convoy as a manager session."
    );
    let session = workspace.session(&request.session)?;
    ensure!(
        session.is_manager() && session.parent_id.is_none(),
        "This session is not a manager."
    );
    ensure!(!session.is_archived(), "This session is archived.");
    Ok(session)
}

// ---------------------------------------------------------------------------
// Starting a helper.
// ---------------------------------------------------------------------------

/// A helper about to be created.
#[derive(Debug, Clone, PartialEq)]
pub struct HelperPlan {
    pub parent: String,
    pub session: NewSession,
    /// A branch for its own worktree, when asked for one.
    pub branch: Option<String>,
}

/// A manager's helpers, in the order they were started.
pub fn children<'a>(workspace: &'a Workspace, parent: &str) -> Vec<&'a Session> {
    workspace
        .state()
        .sessions
        .iter()
        .filter(|session| session.parent_id.as_deref() == Some(parent))
        .collect()
}

/// A title from the first line of a prompt.
pub fn title_from(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(|line| line.trim().trim_start_matches(['#', '*', '>', '-', ' ']))
        .find(|line| !line.is_empty())
        .unwrap_or("Helper");
    let mut title: String = line.chars().take(60).collect();
    if line.chars().count() > 60 {
        title = format!("{}…", title.chars().take(57).collect::<String>().trim_end());
    }
    title
}

/// A branch name from a title, under the project's prefix, else the one in
/// Settings, as the New session sheet suggests it. The id's start keeps two
/// helpers with one title apart.
pub fn branch_for(workspace: &Workspace, project_id: &str, title: &str) -> String {
    let prefix = workspace
        .project(project_id)
        .ok()
        .and_then(|project| project.branch_prefix.clone())
        .filter(|prefix| !prefix.is_empty())
        .unwrap_or_else(|| workspace.settings().branch_prefix.clone());
    let prefix = if prefix.is_empty() {
        "convoy".to_string()
    } else {
        prefix
    };
    let mut slug = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(40).collect();
    let slug = slug.trim_end_matches('-');
    let suffix = &Uuid::new_v4().simple().to_string()[..6];
    if slug.is_empty() {
        format!("{prefix}/helper-{suffix}")
    } else {
        format!("{prefix}/{slug}-{suffix}")
    }
}

/// A branch name Git takes and nothing can mistake for an option.
pub fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.len() <= 200
        && !branch.starts_with(['-', '/'])
        && !branch.ends_with(['/', '.'])
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.ends_with(".lock")
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
}

/// Checks a spawn and works out the helper. `running` is how many of the
/// caller's helpers are running now.
pub fn plan_spawn(
    workspace: &Workspace,
    caller: &Session,
    spawn: &Spawn,
    running: usize,
) -> Result<HelperPlan> {
    ensure!(
        caller.is_manager() && caller.parent_id.is_none(),
        "Only a manager session can start helpers."
    );
    ensure!(
        running < MAX_RUNNING,
        "{MAX_RUNNING} helpers are already running. Wait for one to finish first."
    );
    let agent = Agent::parse(spawn.agent.trim())
        .ok_or_else(|| ConvoyError::message("Unknown agent. Use claude or codex."))?;
    let prompt = spawn.prompt.trim();
    ensure!(!prompt.is_empty(), "Give the helper a prompt.");
    ensure!(
        utf16_len(prompt) <= 32_000,
        "The prompt is too long (32,000 characters at most)."
    );
    let model = spawn.model.clone().unwrap_or_default();
    ensure!(
        utf16_len(&model) <= 100 && MODEL.is_match(&model),
        "Invalid model name."
    );
    let effort = spawn.effort.clone().unwrap_or_default();
    ensure!(
        effort.is_empty() || crate::provider::valid_effort(agent, &effort),
        "Invalid effort."
    );
    let title = match spawn.title.as_deref().map(str::trim) {
        Some(title) if !title.is_empty() => title.to_string(),
        _ => title_from(prompt),
    };
    ensure!(utf16_len(&title) <= 200, "The title is too long.");
    let branch = match spawn.worktree.as_deref().map(str::trim) {
        Some(branch) if !branch.is_empty() => {
            ensure!(valid_branch(branch), "Invalid branch name.");
            Some(branch.to_string())
        }
        _ if spawn.worktree_auto => Some(branch_for(workspace, &caller.project_id, &title)),
        _ => None,
    };
    let mut session = NewSession::new(caller.project_id.clone(), agent, title);
    session.prompt = prompt.to_string();
    session.model = model;
    session.effort = effort;
    // The manager's own account when it is the same agent; otherwise the
    // system login, as a new session gets.
    session.profile_id = caller.profile_id.clone().filter(|_| caller.agent == agent);
    Ok(HelperPlan {
        parent: caller.id.clone(),
        session,
        branch,
    })
}

/// Creates the helper's session: shown as a chat, under its manager. Returns
/// its id. The worktree and the start are the caller's.
pub fn create_helper(workspace: &mut Workspace, plan: &HelperPlan) -> Result<String> {
    let before: Vec<String> = workspace
        .state()
        .sessions
        .iter()
        .map(|session| session.id.clone())
        .collect();
    workspace.add_session(plan.session.clone())?;
    let id = workspace
        .state()
        .sessions
        .iter()
        .rev()
        .find(|session| !before.contains(&session.id))
        .map(|session| session.id.clone())
        .ok_or_else(|| ConvoyError::message("Session not found."))?;
    let parent = plan.parent.clone();
    let child = id.clone();
    workspace.update(move |state| {
        let session = state
            .sessions
            .iter_mut()
            .find(|session| session.id == child)
            .ok_or_else(|| ConvoyError::message("Session not found."))?;
        session.parent_id = Some(parent);
        session.view = Some("chat".into());
        Ok(())
    })?;
    Ok(id)
}

/// Checks that every id is one of the caller's helpers.
pub fn own_helpers(workspace: &Workspace, caller: &str, ids: &[String]) -> Result<()> {
    ensure!(!ids.is_empty(), "Name at least one helper.");
    ensure!(ids.len() <= 64, "Too many helpers at once.");
    for id in ids {
        let mine = workspace
            .session(id)
            .is_ok_and(|session| session.parent_id.as_deref() == Some(caller));
        ensure!(mine, "{id} is not one of this session's helpers.");
    }
    Ok(())
}

/// The last thing the agent said, from a conversation's events.
pub fn last_answer(events: &[ChatEvent]) -> Option<String> {
    events.iter().rev().find_map(|event| match event {
        ChatEvent::TextDone { text, .. } if !text.trim().is_empty() => Some(text.clone()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Following helpers.
// ---------------------------------------------------------------------------

/// How a helper's process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Stopped,
}

/// What is known of one helper since this app started it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tracked {
    pub parent: String,
    pub in_turn: bool,
    pub turns: u32,
    pub error: Option<String>,
    pub exit: Option<Exit>,
    /// The last complete assistant message seen live.
    pub last_text: Option<String>,
}

/// A change worth recording in the manager's activity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    pub parent: String,
    pub child: String,
    pub status: HelperStatus,
}

/// Every helper this app has started, followed through its chat events.
#[derive(Debug, Default)]
pub struct Tracker {
    children: HashMap<String, Tracked>,
}

impl Tracker {
    /// Starts following a helper. It is in its first turn from the start:
    /// the prompt is on its way, and `wait` must not see it idle before then.
    pub fn track(&mut self, child: &str, parent: &str) {
        self.children.insert(
            child.to_string(),
            Tracked {
                parent: parent.to_string(),
                in_turn: true,
                ..Default::default()
            },
        );
    }

    pub fn get(&self, child: &str) -> Option<&Tracked> {
        self.children.get(child)
    }

    /// One chat event of any session; only helpers' matter. Returns the
    /// helper's new standing when a turn just ended.
    pub fn observe(&mut self, child: &str, event: &ChatEvent) -> Option<Settled> {
        let tracked = self.children.get_mut(child)?;
        match event {
            ChatEvent::TurnStarted => {
                tracked.in_turn = true;
                tracked.error = None;
                None
            }
            ChatEvent::TextDone { text, .. } if !text.trim().is_empty() => {
                tracked.last_text = Some(text.clone());
                None
            }
            ChatEvent::TurnEnded { error, .. } => {
                let was = tracked.in_turn;
                tracked.in_turn = false;
                tracked.turns += 1;
                tracked.error = error.clone();
                was.then(|| Settled {
                    parent: tracked.parent.clone(),
                    child: child.to_string(),
                    status: if error.is_some() {
                        HelperStatus::Failed
                    } else {
                        HelperStatus::Done
                    },
                })
            }
            _ => None,
        }
    }

    /// The helper's process ended. Returns its standing when that is news:
    /// a process that ends mid-turn ends the turn with it.
    pub fn ended(&mut self, child: &str, exit: Exit) -> Option<Settled> {
        let tracked = self.children.get_mut(child)?;
        let was = tracked.in_turn;
        tracked.exit = Some(exit);
        tracked.in_turn = false;
        // Ended mid-turn: the turn ends with the process, as the exit says.
        if was {
            match exit {
                Exit::Code(0) => tracked.turns += 1,
                Exit::Code(code) => {
                    tracked.turns += 1;
                    tracked.error = Some(format!("The agent exited with code {code}."));
                }
                Exit::Stopped => tracked.turns = 0,
            }
        }
        let parent = tracked.parent.clone();
        let status = self.status(child, false);
        was.then(|| Settled {
            parent,
            child: child.to_string(),
            status,
        })
    }

    /// Where the helper stands. `running` is whether its process is alive;
    /// it decides for a helper this app did not start.
    pub fn status(&self, child: &str, running: bool) -> HelperStatus {
        let Some(tracked) = self.children.get(child) else {
            return if running {
                HelperStatus::Running
            } else {
                HelperStatus::Stopped
            };
        };
        status_of(tracked)
    }
}

/// The rules `wait` follows. A finished turn is done whatever happened to the
/// process afterwards; otherwise the exit decides.
pub fn status_of(tracked: &Tracked) -> HelperStatus {
    if tracked.in_turn {
        return HelperStatus::Running;
    }
    if tracked.turns > 0 {
        return if tracked.error.is_some() {
            HelperStatus::Failed
        } else {
            HelperStatus::Done
        };
    }
    match tracked.exit {
        Some(Exit::Code(0)) => HelperStatus::Done,
        Some(Exit::Code(_)) => HelperStatus::Failed,
        Some(Exit::Stopped) => HelperStatus::Stopped,
        // Idle before any turn: it never got its prompt.
        None => HelperStatus::Stopped,
    }
}

// ---------------------------------------------------------------------------
// The manager's launch.
// ---------------------------------------------------------------------------

/// What a manager's agent is launched with on top of a session's usual.
#[derive(Debug, Clone, PartialEq)]
pub struct ManagerLaunch {
    pub token: String,
    /// The Convoy binary the agent runs.
    pub bin: PathBuf,
    pub requests: PathBuf,
}

impl ManagerLaunch {
    pub fn env(&self, session: &str) -> Vec<(String, String)> {
        vec![
            (ENV_SESSION.into(), session.to_string()),
            (ENV_TOKEN.into(), self.token.clone()),
            (ENV_BIN.into(), self.bin.to_string_lossy().into_owned()),
            (
                ENV_REQUESTS.into(),
                self.requests.to_string_lossy().into_owned(),
            ),
        ]
    }
}

/// What a manager is told about its helpers. Short: it reads this on every
/// turn.
pub fn instructions(platform: crate::Platform) -> String {
    let bin = if platform.is_windows() {
        "& $env:CONVOY_BIN"
    } else {
        "\"$CONVOY_BIN\""
    };
    format!(
        "You are a manager session in Convoy. You can hand work to helper sessions: other coding agents working in this same project, each with its own conversation. Run these from your shell:\n\
         - {bin} --session spawn --agent codex|claude --prompt \"<task>\" [--title <t>] [--model <m>] [--effort <e>] [--worktree-auto | --worktree <branch>] [--skill <name>]  → prints the helper's id. --prompt @file reads the prompt from a file. --worktree-auto gives the helper its own git worktree and branch, so parallel helpers never edit the same files. --skill attaches a skill from Convoy's library by name.\n\
         - {bin} --session wait <id> [<id>…] [--timeout 90]  → blocks until each helper finishes its turn; prints one JSON line per helper with status done, failed, stopped or timeout. On timeout, run it again.\n\
         - {bin} --session output <id>  → prints the helper's final answer.\n\
         - {bin} --session list  → your helpers and their status.\n\
         At most {MAX_RUNNING} helpers run at once. Give each one a complete, self-contained task: it sees nothing of this conversation. Spawn, then wait, then read the output, then check the work yourself. Example:\n\
         id=$({bin} --session spawn --agent codex --title \"Add parser tests\" --prompt \"Add unit tests for parse() in src/parse.rs and run them.\")\n\
         {bin} --session wait \"$id\" && {bin} --session output \"$id\""
    )
}

/// The flags that hand a manager its instructions in a terminal launch:
/// Claude's appended system prompt, Codex's `developer_instructions`
/// setting (a TOML string; JSON's escaping is valid TOML).
pub fn terminal_flags(agent: Agent, instructions: &str) -> Vec<String> {
    match agent {
        Agent::Claude => vec!["--append-system-prompt".into(), instructions.into()],
        Agent::Codex => vec![
            "-c".into(),
            format!(
                "developer_instructions={}",
                serde_json::to_string(instructions).unwrap_or_default()
            ),
        ],
    }
}

#[cfg(test)]
mod tests;
