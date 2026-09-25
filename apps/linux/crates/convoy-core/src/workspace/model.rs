//! Typed mirror of `workspace.json` schema 3.
//!
//! The on-disk format is shared by every client, so they can all read the
//! same file. Two consequences shape every struct here:
//!
//! * `#[serde(flatten)] unknown` on every record. JavaScript keeps unknown keys
//!   for free through object spread; serde drops them silently. Without these
//!   maps a client would quietly delete fields a newer build wrote.
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
    /// Finished, and it opened a pull request: review happens there.
    Pr,
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
            TaskStatus::Pr => "pr",
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
    /// Preselected for this project's new sessions and tasks; `None` follows
    /// the global default.
    #[serde(
        rename = "defaultAgent",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub default_agent: Option<Agent>,
    /// What new worktrees start from; `None` is the current HEAD.
    #[serde(rename = "baseRef", default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    /// Overrides the global branch prefix.
    #[serde(
        rename = "branchPrefix",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub branch_prefix: Option<String>,
    /// How this project's new tasks finish: `pr`, `push` or `none`.
    #[serde(rename = "taskMode", default, skip_serializing_if = "Option::is_none")]
    pub task_mode: Option<String>,
    /// Start the next queued task when one finishes.
    #[serde(
        rename = "autoRunTasks",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub auto_run_tasks: Option<bool>,
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
    /// The pull request its session opened, as the agent printed it.
    #[serde(rename = "prURL", default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
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
    /// Start new sessions in their own git worktree.
    #[serde(rename = "worktreeByDefault", default)]
    pub worktree_by_default: bool,
    /// Prepended to generated branch names, e.g. `feature` → `feature/fix-login`.
    #[serde(rename = "branchPrefix", default)]
    pub branch_prefix: String,
    #[serde(rename = "sortProjects", default)]
    pub sort_projects: bool,
    /// `--dangerously-skip-permissions` for Claude Code.
    #[serde(rename = "yoloClaude", default)]
    pub yolo_claude: bool,
    /// `--dangerously-bypass-approvals-and-sandbox` for Codex.
    #[serde(rename = "yoloCodex", default)]
    pub yolo_codex: bool,
    /// The review brief's instructions unless a project sets its own.
    #[serde(rename = "reviewTemplate", default)]
    pub review_template: String,
    #[serde(rename = "notifyWaiting", default = "yes")]
    pub notify_waiting: bool,
    #[serde(rename = "notifyDone", default = "yes")]
    pub notify_done: bool,
    /// Notify even while the window has focus.
    #[serde(rename = "notifyWhenFocused", default)]
    pub notify_when_focused: bool,
    /// `default`, `none`, or a system sound name such as `Glass`.
    #[serde(rename = "notificationSound", default = "default_sound")]
    pub notification_sound: String,
    /// Mark a session's folder trusted for its agent before launch.
    #[serde(rename = "autoTrust", default = "yes")]
    pub auto_trust: bool,
    /// Claude's per-session status hooks (working / waiting / done).
    #[serde(rename = "statusHooks", default = "yes")]
    pub status_hooks: bool,
    /// Show branch and changed-file counts.
    #[serde(rename = "gitStatus", default = "yes")]
    pub git_status: bool,
    /// Seconds between Git refreshes; 0 refreshes only on demand.
    #[serde(rename = "gitPollSeconds", default = "default_git_poll")]
    pub git_poll_seconds: i64,
    /// Sidebar rows without the second line of detail.
    #[serde(rename = "compactSidebar", default = "yes")]
    pub compact_sidebar: bool,
    /// Run in every new worktree, before the project's own command.
    #[serde(rename = "worktreeSetup", default)]
    pub worktree_setup: String,
    /// Paths copied into every new worktree, one per line, before the project's.
    #[serde(rename = "worktreeShared", default)]
    pub worktree_shared: String,
    #[serde(flatten)]
    pub unknown: Unknown,
}

fn yes() -> bool {
    true
}

fn default_sound() -> String {
    "default".into()
}

fn default_git_poll() -> i64 {
    10
}

/// The sounds a notification may play. `default` is the system's own; the
/// named ones are macOS system sounds and are offered only there.
pub const NOTIFICATION_SOUNDS: &[&str] = &[
    "default",
    "none",
    "Basso",
    "Blow",
    "Bottle",
    "Frog",
    "Funk",
    "Glass",
    "Hero",
    "Morse",
    "Ping",
    "Pop",
    "Purr",
    "Sosumi",
    "Submarine",
    "Tink",
];

/// 13 points, the macOS app's terminal size.
fn default_font_size() -> i64 {
    13
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
            worktree_by_default: false,
            branch_prefix: String::new(),
            sort_projects: false,
            yolo_claude: false,
            yolo_codex: false,
            review_template: String::new(),
            notify_waiting: true,
            notify_done: true,
            notify_when_focused: false,
            notification_sound: default_sound(),
            auto_trust: true,
            status_hooks: true,
            git_status: true,
            git_poll_seconds: default_git_poll(),
            compact_sidebar: true,
            worktree_setup: String::new(),
            worktree_shared: String::new(),
            unknown: Map::new(),
        }
    }
}

/// The actions that may carry a user-defined accelerator. The first seven
/// are the original set; the rest follow the macOS app's.
pub const SHORTCUT_ACTIONS: &[&str] = &[
    "palette",
    "newSession",
    "files",
    "next",
    "previous",
    "settings",
    "search",
    "resume",
    "stop",
    "sleep",
    "pin",
    "edit",
    "review",
    "feedback",
    "quick",
    "split",
    "openFolder",
    "newTask",
    "newSpec",
    "importIssues",
    "sessionsTab",
    "reviewsTab",
    "specsTab",
    "tasksTab",
    "docsTab",
    "closeTab",
    "sidebar",
    "layout1",
    "layout2",
    "layout4",
    "paneNext",
    "panePrevious",
    "paneClose",
    "activity",
    "nextTab",
    "previousTab",
    "reveal",
    "copyPath",
    "gitRefresh",
    "theme",
    "wake",
    "limits",
    "limitsRefresh",
    "switcher",
    "reopenTab",
    "home",
    "dashboard",
    "projectRefresh",
];
