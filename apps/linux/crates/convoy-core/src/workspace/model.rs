//! Typed mirror of `workspace.json` schema 3.
//!
//! The on-disk format stays byte-compatible with the Electron preview so both
//! builds can read the same file. Two consequences shape every struct here:
//!
//! * `#[serde(flatten)] unknown` on every record. JavaScript keeps unknown keys
//!   for free through object spread; serde drops them silently. Without these
//!   maps the port would quietly delete fields a newer Electron build wrote.
//! * `skip_serializing_if = "Option::is_none"`, because `JSON.stringify` omits
//!   `undefined` and several code paths (`delete task.sessionID`) rely on the
//!   key disappearing rather than becoming `null`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub type Unknown = Map<String, Value>;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Queued,
    Building,
    Review,
    Changes,
    Done,
    Failed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Building => "building",
            TaskStatus::Review => "review",
            TaskStatus::Changes => "changes",
            TaskStatus::Done => "done",
            TaskStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PublishMode {
    #[default]
    None,
    Pr,
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Started,
    Resumed,
    Exited,
    Done,
    Waiting,
    Hibernated,
    Worktree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeepAwake {
    Off,
    Always,
    Sessions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u8,
    pub projects: Vec<Project>,
    pub sessions: Vec<Session>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(rename = "quickCommands", default)]
    pub quick_commands: Vec<QuickCommand>,
    #[serde(default)]
    pub specs: Vec<Spec>,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub activity: Vec<ActivityEvent>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

impl Default for State {
    fn default() -> Self {
        State {
            schema_version: 3,
            projects: Vec::new(),
            sessions: Vec::new(),
            settings: Settings::default(),
            quick_commands: Vec::new(),
            specs: Vec::new(),
            tasks: Vec::new(),
            profiles: Vec::new(),
            activity: Vec::new(),
            unknown: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(
        rename = "setupCommand",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub setup_command: Option<String>,
    #[serde(
        rename = "sharedPaths",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub shared_paths: Option<String>,
    #[serde(
        rename = "reviewTemplate",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub review_template: Option<String>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub agent: Agent,
    pub title: String,
    pub prompt: String,
    #[serde(rename = "providerID")]
    pub provider_id: String,
    pub started: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(
        rename = "workingDirectory",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub working_directory: Option<PathBuf>,
    #[serde(rename = "agentHome", default, skip_serializing_if = "Option::is_none")]
    pub agent_home: Option<PathBuf>,
    #[serde(
        rename = "ownsWorktree",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub owns_worktree: Option<bool>,
    #[serde(
        rename = "worktreeRemoved",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub worktree_removed: Option<bool>,
    #[serde(rename = "reviewOf", default, skip_serializing_if = "Option::is_none")]
    pub review_of: Option<String>,
    #[serde(rename = "taskID", default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(rename = "profileID", default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

impl Session {
    /// A minimal session record. `Workspace::add_session` is the normal way to
    /// create one; this exists for callers that need a value before it is
    /// stored, such as a launch preview or a test.
    pub fn new(project_id: impl Into<String>, agent: Agent, title: impl Into<String>) -> Self {
        Session {
            id: String::new(),
            project_id: project_id.into(),
            agent,
            title: title.into(),
            prompt: String::new(),
            provider_id: String::new(),
            started: false,
            model: None,
            notes: None,
            branch: None,
            archived: None,
            pinned: None,
            working_directory: None,
            agent_home: None,
            owns_worktree: None,
            worktree_removed: None,
            review_of: None,
            task_id: None,
            profile_id: None,
            unknown: Map::new(),
        }
    }

    pub fn is_archived(&self) -> bool {
        self.archived.unwrap_or(false)
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.unwrap_or(false)
    }

    pub fn owns_worktree(&self) -> bool {
        self.owns_worktree.unwrap_or(false)
    }

    pub fn worktree_removed(&self) -> bool {
        self.worktree_removed.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub title: String,
    pub problem: String,
    pub requirements: String,
    pub acceptance: String,
    pub constraints: String,
    pub plan: String,
    pub revision: u64,
    #[serde(
        rename = "approvedRevision",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub approved_revision: Option<u64>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

impl Spec {
    pub fn approved(&self) -> bool {
        self.approved_revision == Some(self.revision)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(rename = "projectID")]
    pub project_id: String,
    pub title: String,
    pub details: String,
    pub findings: String,
    pub agent: Agent,
    pub status: TaskStatus,
    #[serde(default)]
    pub mode: PublishMode,
    #[serde(rename = "autoReview", default, skip_serializing_if = "is_false")]
    pub auto_review: bool,
    #[serde(rename = "specID", default, skip_serializing_if = "Option::is_none")]
    pub spec_id: Option<String>,
    #[serde(rename = "sessionID", default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(
        rename = "specRevision",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub spec_revision: Option<u64>,
    #[serde(rename = "lastError", default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// CLI model for the task's session; `None` is the agent's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Set when the task was imported from Linear or Jira.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::integrations::IssueSource>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub agent: Agent,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickCommand {
    pub id: String,
    pub title: String,
    pub text: String,
    pub submit: bool,
    #[serde(rename = "projectID", default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: String,
    pub at: String,
    pub kind: ActivityKind,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub title: String,
    pub detail: String,
    #[serde(flatten)]
    pub unknown: Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "fontSize", default = "default_font_size")]
    pub font_size: i64,
    #[serde(default = "default_scrollback")]
    pub scrollback: i64,
    #[serde(default = "default_theme")]
    pub theme: Theme,
    #[serde(rename = "defaultAgent", default = "default_agent")]
    pub default_agent: Agent,
    #[serde(default)]
    pub notifications: bool,
    #[serde(rename = "keepAwake", default = "default_keep_awake")]
    pub keep_awake: KeepAwake,
    #[serde(rename = "hibernateMinutes", default)]
    pub hibernate_minutes: i64,
    #[serde(default)]
    pub shortcuts: BTreeMap<String, String>,
    #[serde(rename = "claudeUsage", default)]
    pub claude_usage: bool,
    #[serde(flatten)]
    pub unknown: Unknown,
}

fn default_font_size() -> i64 {
    14
}
fn default_scrollback() -> i64 {
    10_000
}
fn default_theme() -> Theme {
    Theme::Dark
}
fn default_agent() -> Agent {
    Agent::Claude
}
fn default_keep_awake() -> KeepAwake {
    KeepAwake::Off
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            font_size: default_font_size(),
            scrollback: default_scrollback(),
            theme: default_theme(),
            default_agent: default_agent(),
            notifications: false,
            keep_awake: default_keep_awake(),
            hibernate_minutes: 0,
            shortcuts: BTreeMap::new(),
            claude_usage: false,
            unknown: Map::new(),
        }
    }
}

/// The seven actions that may carry a user-defined accelerator.
pub const SHORTCUT_ACTIONS: [&str; 7] = [
    "palette",
    "newSession",
    "files",
    "next",
    "previous",
    "settings",
    "search",
];
