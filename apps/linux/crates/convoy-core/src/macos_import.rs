//! Imports what the native macOS app (Swift) saved, so moving to this client
//! keeps every project, session, task and setting.
//!
//! The Swift app keeps its workspace in `~/Library/Application Support/Convoy/`
//! as Foundation JSON — dates counted from 2001, uppercase UUIDs, sessions
//! nested inside projects — and its settings in UserDefaults. None of that is
//! this workspace's shape, so everything is transformed rather than copied.
//!
//! The import merges: a project already here (matched by folder) keeps its
//! id and gains what it lacks, and a record that already exists is left alone,
//! so running it twice changes nothing. The workspace validates the result as
//! a whole, so an import that would leave it inconsistent writes nothing.

use crate::hash::sha256_hex;
use crate::integrations::{Connection, Integrations, IssueSource, TrackerAuth, TrackerKind};
use crate::model::{
    ActivityEvent, ActivityKind, Agent, KeepAwake, Project, PublishMode, QuickCommand, Session,
    Spec, State, Task, TaskStatus, Theme,
};
use crate::patterns::SHORTCUT;
use crate::storage::Storage;
use crate::workspace::Workspace;
use crate::Result;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Seconds between 1970 and 2001, the epoch Foundation encodes dates from.
const APPLE_EPOCH: f64 = 978_307_200.0;

/// What the macOS app left on disk, read once.
#[derive(Debug, Clone, Default)]
pub struct MacSources {
    /// `~/Library/Application Support/Convoy`.
    pub support: PathBuf,
    /// `workspace.json`.
    pub workspace: Value,
    /// `activity.json`, when there is one.
    pub activity: Option<Value>,
    /// The app's UserDefaults, as JSON (the caller converts the plist).
    pub defaults: Map<String, Value>,
    /// `~/.claude`, where Claude keeps conversations of the default login.
    pub claude_home: PathBuf,
}

impl MacSources {
    /// Reads the workspace and activity files under `support`; `None` when the
    /// macOS app never saved a workspace there.
    pub fn read(
        support: &Path,
        defaults: Map<String, Value>,
        claude_home: &Path,
    ) -> Result<Option<Self>> {
        let file = support.join("workspace.json");
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Ok(None);
        };
        let workspace: Value = serde_json::from_str(&text)?;
        let activity = std::fs::read_to_string(support.join("activity.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        Ok(Some(MacSources {
            support: support.to_path_buf(),
            workspace,
            activity,
            defaults,
            claude_home: claude_home.to_path_buf(),
        }))
    }
}

/// What an import brought in, or would bring in.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Report {
    pub projects: usize,
    /// Projects already here, matched by folder.
    pub projects_existing: usize,
    pub sessions: usize,
    pub tasks: usize,
    pub specs: usize,
    pub quick_commands: usize,
    pub activity: usize,
    pub settings: bool,
    pub shortcuts: usize,
    pub connections: usize,
    /// Things that did not carry over, in words for the user.
    pub warnings: Vec<String>,
}

// ------------------------------------------------------------------ helpers --

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_string)
}

fn flag(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn agent(value: Option<&str>) -> Option<Agent> {
    match value? {
        "Claude Code" | "claude" => Some(Agent::Claude),
        "Codex" | "codex" => Some(Agent::Codex),
        _ => None,
    }
}

fn clip(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// A Foundation date (seconds since 2001) as ISO 8601.
fn apple_date(value: &Value) -> Option<String> {
    let seconds = value.as_f64()? + APPLE_EPOCH;
    Some(crate::time::iso8601(
        seconds.floor() as i64,
        ((seconds.fract()) * 1000.0) as u32,
    ))
}

/// Claude names a project's conversation folder after its path, every
/// character other than a letter or digit becoming a dash.
fn claude_slug(directory: &str) -> String {
    directory
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

// ------------------------------------------------------------------ merging --

struct Merge<'a> {
    sources: &'a MacSources,
    icons: PathBuf,
    report: Report,
    /// Files to copy once the workspace is saved: (from, to).
    copies: Vec<(PathBuf, PathBuf)>,
}

impl Merge<'_> {
    /// A Swift image icon (`gh:`/`img:` + path) is copied into this client's
    /// icons folder, the only place it reads images from.
    fn icon(&mut self, icon: Option<String>) -> Option<String> {
        let icon = icon.filter(|value| !value.is_empty())?;
        let Some((prefix, path)) = icon
            .split_once(':')
            .filter(|(prefix, _)| ["gh", "img"].contains(prefix))
        else {
            return Some(icon);
        };
        let source = PathBuf::from(path);
        let name = source.file_name()?.to_owned();
        if !source.is_file() {
            return None;
        }
        let target = self.icons.join(name);
        self.copies.push((source, target.clone()));
        Some(format!("{prefix}:{}", target.to_string_lossy()))
    }

    /// Whether a session ever ran, which decides between resuming it and
    /// starting it fresh. The Swift app assigns Claude's id before launch, so
    /// the id alone proves nothing; Claude's own transcript does.
    fn started(&self, agent: Agent, session: &Value, directory: &str, id: &str) -> bool {
        let provider = text(session, "sessionID").unwrap_or_default();
        let snapshot = self
            .sources
            .support
            .join("TerminalHistory")
            .join(format!("{id}.txt"))
            .is_file();
        match agent {
            Agent::Claude => {
                let home = text(session, "agentHome")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| self.sources.claude_home.clone());
                // Claude names the folder after the path as the OS resolves it,
                // which may differ from the one stored.
                let resolved = std::fs::canonicalize(directory)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| directory.to_string());
                [directory.to_string(), resolved].iter().any(|folder| {
                    home.join("projects")
                        .join(claude_slug(folder))
                        .join(format!("{provider}.jsonl"))
                        .is_file()
                }) || snapshot
            }
            // Codex's id is only known once it printed its resume line.
            Agent::Codex => !provider.is_empty() || snapshot,
        }
    }

    fn projects(&mut self, state: &mut State, swift: &[Value]) -> HashMap<String, String> {
        // Swift id → id here, for every project, new or matched.
        let mut ids = HashMap::new();
        let names: HashMap<String, String> = swift
            .iter()
            .filter_map(|project| Some((text(project, "id")?, text(project, "name")?)))
            .collect();
        // Parents first, each in the Swift order.
        let mut ordered: Vec<&Value> = swift.iter().collect();
        ordered.sort_by_key(|project| {
            (
                project.get("parentID").is_some(),
                project
                    .get("order")
                    .and_then(Value::as_i64)
                    .unwrap_or(i64::MAX),
                text(project, "name").unwrap_or_default().to_lowercase(),
            )
        });
        for project in ordered {
            let (Some(id), Some(path)) = (text(project, "id"), text(project, "path")) else {
                continue;
            };
            // Folders are compared as the filesystem resolves them: opening a
            // folder here stores its canonical path, while the macOS app kept
            // the one the user picked (`/var/…` versus `/private/var/…`).
            let resolved = std::fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(&path));
            let path = resolved.to_string_lossy().into_owned();
            if let Some(existing) = state.projects.iter().find(|item| {
                std::fs::canonicalize(&item.path).unwrap_or_else(|_| item.path.clone()) == resolved
            }) {
                ids.insert(id, existing.id.clone());
                self.report.projects_existing += 1;
                continue;
            }
            // Swift groups are a tree; here a group is a heading, so a child
            // is filed under its parent's name.
            let group = text(project, "parentID").and_then(|parent| names.get(&parent).cloned());
            let shared: Vec<String> = project
                .get("sharedPaths")
                .and_then(Value::as_array)
                .map(|paths| {
                    paths
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            state.projects.push(Project {
                id: id.clone(),
                title: text(project, "name").unwrap_or_else(|| path.clone()),
                path: PathBuf::from(&path),
                group,
                color: text(project, "color"),
                icon: self.icon(text(project, "icon")),
                setup_command: text(project, "setupCommands"),
                shared_paths: (!shared.is_empty()).then(|| shared.join("\n")),
                review_template: text(project, "reviewTemplate"),
                default_agent: agent(project.get("defaultAgent").and_then(Value::as_str)),
                base_ref: text(project, "baseRef"),
                branch_prefix: text(project, "branchPrefix"),
                // The macOS app finishes tasks with a pull request unless told otherwise.
                task_mode: Some(text(project, "taskMode").unwrap_or_else(|| "pr".into())),
                auto_run_tasks: flag(project, "autoRunTasks"),
                unknown: Default::default(),
            });
            ids.insert(id, state.projects.last().expect("pushed").id.clone());
            self.report.projects += 1;
        }
        ids
    }

    fn sessions(&mut self, state: &mut State, swift: &[Value], projects: &HashMap<String, String>) {
        let known: HashSet<String> = state.sessions.iter().map(|item| item.id.clone()).collect();
        let mut added: Vec<(f64, Session)> = Vec::new();
        let mut worktrees = 0;
        for project in swift {
            let Some(project_id) = text(project, "id").and_then(|id| projects.get(&id).cloned())
            else {
                continue;
            };
            let folder = state
                .projects
                .iter()
                .find(|item| item.id == project_id)
                .map(|item| item.path.to_string_lossy().into_owned())
                .unwrap_or_default();
            for session in project
                .get("sessions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let (Some(id), Some(kind)) = (
                    text(session, "id"),
                    agent(session.get("agent").and_then(Value::as_str)),
                ) else {
                    continue;
                };
                if known.contains(&id) {
                    continue;
                }
                let provider = text(session, "sessionID").unwrap_or_default();
                if kind == Agent::Claude && provider.is_empty() {
                    // Login sessions: a terminal for signing in, never a conversation.
                    continue;
                }
                let directory = text(session, "workingDirectory").unwrap_or_else(|| folder.clone());
                let mut record = Session::new(
                    project_id.clone(),
                    kind,
                    clip(
                        &text(session, "title").unwrap_or_else(|| "Session".into()),
                        200,
                    ),
                );
                record.id = id.clone();
                record.provider_id = provider;
                record.prompt = clip(&text(session, "initialPrompt").unwrap_or_default(), 32_000);
                record.started = self.started(kind, session, &directory, &id);
                record.model = text(session, "model").filter(|value| !value.is_empty());
                record.notes = text(session, "notes");
                record.branch = text(session, "branch");
                record.archived = flag(session, "archived");
                record.pinned = flag(session, "pinned");
                record.working_directory = text(session, "workingDirectory").map(PathBuf::from);
                record.agent_home = text(session, "agentHome").map(PathBuf::from);
                record.review_of = text(session, "reviewOf");
                if record.working_directory.is_some() {
                    worktrees += 1;
                }
                // The terminal output saved by the macOS app, under this
                // client's name for it.
                let snapshot = self
                    .sources
                    .support
                    .join("TerminalHistory")
                    .join(format!("{id}.txt"));
                if snapshot.is_file() {
                    self.copies.push((
                        snapshot,
                        PathBuf::from(format!("history:{}", sha256_hex(&id))),
                    ));
                }
                let created = session
                    .get("createdAt")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                added.push((created, record));
            }
        }
        // Newest first, as the macOS app lists them.
        added.sort_by(|a, b| b.0.total_cmp(&a.0));
        // A review whose builder did not come across would point at nothing.
        let present: HashSet<String> = state
            .sessions
            .iter()
            .map(|item| item.id.clone())
            .chain(added.iter().map(|(_, item)| item.id.clone()))
            .collect();
        for (_, mut session) in added {
            if session
                .review_of
                .as_ref()
                .is_some_and(|id| !present.contains(id))
            {
                session.review_of = None;
            }
            state.sessions.push(session);
            self.report.sessions += 1;
        }
        if worktrees > 0 {
            self.report.warnings.push(format!(
                "{worktrees} session{} run in a worktree the macOS app made. They open and resume here; remove those worktrees from Git or the macOS app.",
                if worktrees == 1 { "" } else { "s" }
            ));
        }
    }

    fn specs(&mut self, state: &mut State, swift: &[Value], projects: &HashMap<String, String>) {
        let known: HashSet<String> = state
            .specs
            .iter()
            .map(|item| item.id.clone())
            .chain(state.tasks.iter().map(|item| item.id.clone()))
            .collect();
        for project in swift {
            let Some(project_id) = text(project, "id").and_then(|id| projects.get(&id).cloned())
            else {
                continue;
            };
            for spec in project
                .get("specs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = text(spec, "id").filter(|id| !known.contains(id)) else {
                    continue;
                };
                let revision = spec
                    .get("revision")
                    .and_then(Value::as_u64)
                    .unwrap_or(1)
                    .max(1);
                let field = |key: &str| clip(&text(spec, key).unwrap_or_default(), 16_000);
                state.specs.push(Spec {
                    id: id.clone(),
                    project_id: project_id.clone(),
                    title: clip(
                        &text(spec, "title").unwrap_or_else(|| "Specification".into()),
                        200,
                    ),
                    problem: field("problem"),
                    requirements: field("requirements"),
                    acceptance: field("acceptance"),
                    constraints: field("constraints"),
                    plan: field("plan"),
                    revision,
                    approved_revision: spec
                        .get("approvedRevision")
                        .and_then(Value::as_u64)
                        .filter(|value| *value == revision),
                    unknown: Default::default(),
                });
                self.report.specs += 1;
                // A spec's own work items become tasks linked to it.
                for work in spec
                    .get("tasks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(task_id) = text(work, "id").filter(|id| !known.contains(id)) else {
                        continue;
                    };
                    let status = match text(work, "status").as_deref() {
                        Some("Needs review") => TaskStatus::Review,
                        Some("Changes requested") => TaskStatus::Changes,
                        Some("Done") => TaskStatus::Done,
                        _ => TaskStatus::Queued,
                    };
                    state.tasks.push(Task {
                        id: task_id,
                        project_id: project_id.clone(),
                        title: clip(&text(work, "title").unwrap_or_else(|| "Task".into()), 200),
                        details: clip(&text(work, "notes").unwrap_or_default(), 16_000),
                        findings: clip(&text(work, "findings").unwrap_or_default(), 16_000),
                        agent: agent(work.get("builder").and_then(Value::as_str))
                            .unwrap_or(Agent::Claude),
                        // Done needs an approved spec; anything else re-opens for review.
                        status: if status == TaskStatus::Done
                            && state.specs.last().is_some_and(|spec| !spec.approved())
                        {
                            TaskStatus::Review
                        } else {
                            status
                        },
                        mode: PublishMode::None,
                        auto_review: false,
                        spec_id: Some(id.clone()),
                        session_id: None,
                        spec_revision: None,
                        last_error: None,
                        model: None,
                        source: None,
                        unknown: Default::default(),
                    });
                    self.report.tasks += 1;
                }
            }
        }
    }

    fn tasks(&mut self, state: &mut State, swift: &[Value], projects: &HashMap<String, String>) {
        let known: HashSet<String> = state
            .tasks
            .iter()
            .map(|item| item.id.clone())
            .chain(state.specs.iter().map(|item| item.id.clone()))
            .collect();
        for task in swift {
            let (Some(id), Some(project_id)) = (
                text(task, "id"),
                text(task, "projectID").and_then(|id| projects.get(&id).cloned()),
            ) else {
                continue;
            };
            if known.contains(&id) {
                continue;
            }
            let mut details = text(task, "details").unwrap_or_default();
            if let Some(spec) = text(task, "spec") {
                details.push_str(&format!(
                    "\n\nThe work must satisfy the specification in {spec}."
                ));
            }
            if let Some(url) = text(task, "prURL") {
                details.push_str(&format!("\n\nPull request: {url}"));
            }
            let status = match text(task, "status").as_deref() {
                Some("done") => TaskStatus::Done,
                Some("failed") => TaskStatus::Failed,
                // Agents do not move between apps: work that was running or
                // awaiting a PR is here for review.
                Some("running") | Some("review") | Some("pr") => TaskStatus::Review,
                _ => TaskStatus::Queued,
            };
            let mode = match text(task, "mode").as_deref() {
                Some("push") => PublishMode::Push,
                Some("none") => PublishMode::None,
                _ => PublishMode::Pr,
            };
            let source = task.get("source").and_then(|source| {
                Some(IssueSource {
                    tracker: match text(source, "tracker")?.to_lowercase().as_str() {
                        "jira" => TrackerKind::Jira,
                        _ => TrackerKind::Linear,
                    },
                    key: text(source, "key")?,
                    origin: text(source, "origin"),
                    url: text(source, "url"),
                    via_mcp: flag(source, "viaMCP"),
                })
            });
            // The task's session, when it came across, is linked both ways.
            let session_id = text(task, "sessionID").filter(|session| {
                state.sessions.iter().any(|item| {
                    &item.id == session && item.project_id == project_id && item.task_id.is_none()
                })
            });
            if let Some(session) = &session_id {
                if let Some(item) = state.sessions.iter_mut().find(|item| &item.id == session) {
                    item.task_id = Some(id.clone());
                }
            }
            state.tasks.push(Task {
                id,
                project_id,
                title: clip(&text(task, "title").unwrap_or_else(|| "Task".into()), 200),
                details: clip(details.trim(), 16_000),
                findings: String::new(),
                agent: agent(task.get("agent").and_then(Value::as_str)).unwrap_or(Agent::Claude),
                status,
                mode,
                auto_review: flag(task, "autoReview").unwrap_or(false),
                spec_id: None,
                session_id,
                spec_revision: None,
                last_error: None,
                model: text(task, "model").filter(|value| !value.is_empty()),
                source,
                unknown: Default::default(),
            });
            self.report.tasks += 1;
        }
    }

    fn quick_commands(
        &mut self,
        state: &mut State,
        swift: &[Value],
        projects: &HashMap<String, String>,
    ) {
        for command in swift {
            let Some(id) = text(command, "id")
                .filter(|id| !state.quick_commands.iter().any(|item| &item.id == id))
            else {
                continue;
            };
            let (title, body) = (
                text(command, "title").unwrap_or_default(),
                text(command, "text").unwrap_or_default(),
            );
            if title.trim().is_empty() || body.trim().is_empty() {
                continue;
            }
            let project_id = match text(command, "projectID") {
                Some(swift_id) => match projects.get(&swift_id) {
                    Some(id) => Some(id.clone()),
                    None => continue,
                },
                None => None,
            };
            state.quick_commands.push(QuickCommand {
                id,
                title: clip(&title, 200),
                text: clip(&body, 32_000),
                submit: flag(command, "submit").unwrap_or(false),
                project_id,
                unknown: Default::default(),
            });
            self.report.quick_commands += 1;
        }
    }

    fn activity(&mut self, state: &mut State) {
        let Some(events) = self.sources.activity.as_ref().and_then(Value::as_array) else {
            return;
        };
        let sessions: HashSet<&str> = state.sessions.iter().map(|item| item.id.as_str()).collect();
        let known: HashSet<String> = state.activity.iter().map(|item| item.id.clone()).collect();
        let mut added = Vec::new();
        for event in events {
            let (Some(id), Some(session), Some(at)) = (
                text(event, "id"),
                text(event, "sessionID"),
                event.get("date").and_then(apple_date),
            ) else {
                continue;
            };
            if known.contains(&id) || !sessions.contains(session.as_str()) {
                continue;
            }
            let kind = match text(event, "kind").as_deref() {
                Some("started") => ActivityKind::Started,
                Some("resumed") => ActivityKind::Resumed,
                Some("waiting") => ActivityKind::Waiting,
                Some("done") => ActivityKind::Done,
                Some("slept") => ActivityKind::Hibernated,
                Some("worktree") => ActivityKind::Worktree,
                _ => continue,
            };
            added.push(ActivityEvent {
                id,
                at,
                kind,
                session_id: session,
                title: clip(&text(event, "sessionTitle").unwrap_or_default(), 200),
                detail: clip(&text(event, "detail").unwrap_or_default(), 2000),
                unknown: Default::default(),
            });
        }
        self.report.activity = added.len();
        state.activity.extend(added);
        // Newest first, and the feed keeps its limit.
        state.activity.sort_by(|a, b| b.at.cmp(&a.at));
        state.activity.truncate(200);
        self.report.activity = self.report.activity.min(200);
    }

    fn settings(&mut self, state: &mut State) {
        let sources = self.sources;
        let defaults = &sources.defaults;
        if defaults.is_empty() {
            return;
        }
        let get = |key: &str| defaults.get(key);
        let boolean = |key: &str| get(key).and_then(Value::as_bool);
        let number = |key: &str| get(key).and_then(Value::as_f64);
        let string = |key: &str| get(key).and_then(Value::as_str).map(str::to_string);
        let settings = &mut state.settings;
        settings.theme = match string("appearance").as_deref() {
            Some("light") => Theme::Light,
            Some("dark") => Theme::Dark,
            // The macOS app follows the system unless told otherwise.
            _ => Theme::System,
        };
        if let Some(kind) = agent(string("defaultAgent").as_deref()) {
            settings.default_agent = kind;
        }
        settings.font_size = number("terminalFontSize")
            .map(|value| value.round() as i64)
            .unwrap_or(13)
            .clamp(10, 24);
        if let Some(value) = number("terminalScrollback") {
            settings.scrollback = (value.round() as i64).clamp(1000, 50_000);
        }
        // The macOS app's defaults when a key was never written.
        settings.notifications = boolean("notificationsEnabled").unwrap_or(true);
        settings.notify_waiting = boolean("notifyWaiting").unwrap_or(true);
        settings.notify_done = boolean("notifyDone").unwrap_or(true);
        if let Some(sound) = string("notificationSound")
            .filter(|sound| crate::model::NOTIFICATION_SOUNDS.contains(&sound.as_str()))
        {
            settings.notification_sound = sound;
        }
        settings.keep_awake = match string("wakeMode").as_deref() {
            Some("always") => KeepAwake::Always,
            Some("sessions") => KeepAwake::Sessions,
            _ => KeepAwake::Off,
        };
        if let Some(value) = number("hibernateAfterMinutes") {
            settings.hibernate_minutes = (value.round() as i64).clamp(0, 1440);
        }
        let flags: [(&str, &mut bool); 9] = [
            ("claudeLimitsIntegration", &mut settings.claude_usage),
            ("worktreeByDefault", &mut settings.worktree_by_default),
            ("yoloClaude", &mut settings.yolo_claude),
            ("yoloCodex", &mut settings.yolo_codex),
            ("sortProjectsByName", &mut settings.sort_projects),
            ("compactSidebar", &mut settings.compact_sidebar),
            ("autoTrustFolders", &mut settings.auto_trust),
            ("agentStatusHooks", &mut settings.status_hooks),
            ("gitEnabled", &mut settings.git_status),
        ];
        for (key, target) in flags {
            if let Some(value) = boolean(key) {
                *target = value;
            }
        }
        if let Some(prefix) = string("branchPrefix") {
            let prefix = prefix.trim().trim_matches('/').to_string();
            if crate::patterns::BRANCH_PREFIX.is_match(&prefix) {
                settings.branch_prefix = prefix;
            }
        }
        if let Some(value) = string("reviewTemplate") {
            settings.review_template = clip(&value, 16_000);
        }
        if let Some(value) = number("gitPollSeconds") {
            settings.git_poll_seconds = (value.round() as i64).clamp(0, 600);
        }
        if let Some(value) = string("worktreeSetupCommands") {
            settings.worktree_setup = clip(&value, 16_000);
        }
        if let Some(value) = string("worktreeSharedPaths") {
            settings.worktree_shared = clip(&value, 16_000);
        }
        self.report.settings = true;

        if let Some(bindings) = get("keybindings").and_then(Value::as_object) {
            let mut seen: HashSet<String> = HashSet::new();
            let mut shortcuts = BTreeMap::new();
            for (swift_action, binding) in bindings {
                let (Some(action), Some(stored)) = (
                    SHORTCUT_IDS
                        .iter()
                        .find(|(from, _)| from == swift_action)
                        .map(|(_, to)| *to),
                    binding.as_str().and_then(binding_here),
                ) else {
                    continue;
                };
                if seen.insert(stored.clone()) {
                    shortcuts.insert(action.to_string(), stored);
                }
            }
            self.report.shortcuts = shortcuts.len();
            if !shortcuts.is_empty() {
                state.settings.shortcuts = shortcuts;
            }
        }
    }
}

/// The macOS app's shortcut ids and their counterparts here.
const SHORTCUT_IDS: [(&str, &str); 40] = [
    ("session.new", "newSession"),
    ("session.resume", "resume"),
    ("session.stop", "stop"),
    ("session.sleep", "sleep"),
    ("session.pin", "pin"),
    ("session.edit", "edit"),
    ("session.review", "review"),
    ("session.feedback", "feedback"),
    ("session.quick", "quick"),
    ("tab.next", "next"),
    ("tab.previous", "previous"),
    ("tab.close", "closeTab"),
    ("view.layout1", "layout1"),
    ("view.layout2", "layout2"),
    ("view.layout4", "layout4"),
    ("pane.next", "paneNext"),
    ("pane.previous", "panePrevious"),
    ("pane.close", "paneClose"),
    ("project.open", "openFolder"),
    ("project.sessions", "sessionsTab"),
    ("project.reviews", "reviewsTab"),
    ("project.specs", "specsTab"),
    ("project.tasks", "tasksTab"),
    ("project.docs", "docsTab"),
    ("project.newSpec", "newSpec"),
    ("project.newTask", "newTask"),
    ("project.nextArea", "nextTab"),
    ("project.previousArea", "previousTab"),
    ("project.finder", "reveal"),
    ("project.copyPath", "copyPath"),
    ("go.palette", "palette"),
    ("go.sidebar", "sidebar"),
    ("go.activity", "activity"),
    ("go.settings", "settings"),
    ("go.theme", "theme"),
    ("go.wake", "wake"),
    ("git.panel", "files"),
    ("go.find", "search"),
    ("tab.switch", "switcher"),
    ("tab.reopen", "reopenTab"),
];

/// `cmd+shift+r` (macOS notation) as `mod+shift+r`, or nothing when this
/// client cannot bind it.
pub fn binding_here(swift: &str) -> Option<String> {
    let parts: Vec<&str> = swift.split('+').collect();
    let (key, modifiers) = parts.split_last()?;
    let has = |name: &str| modifiers.contains(&name);
    let mut out = String::new();
    if has("cmd") {
        out.push_str("mod+");
    }
    if has("ctrl") {
        out.push_str("ctrl+");
    }
    if has("opt") {
        out.push_str("alt+");
    }
    if has("shift") {
        out.push_str("shift+");
    }
    out.push_str(key);
    SHORTCUT.is_match(&out).then_some(out)
}

/// The connections the macOS app kept in UserDefaults. Their secrets live
/// in its Keychain entries, which this client does not read: each connection
/// comes across and asks for its key once.
fn connections(defaults: &Map<String, Value>) -> Vec<Connection> {
    let raw = match defaults.get("trackerConnections") {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
        Some(other) => Some(other.clone()),
        None => None,
    };
    raw.and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            Some(Connection {
                id: text(item, "id")?,
                kind: match text(item, "kind")?.to_lowercase().as_str() {
                    "jira" => TrackerKind::Jira,
                    _ => TrackerKind::Linear,
                },
                auth: match text(item, "auth").as_deref() {
                    Some("password") => TrackerAuth::Password,
                    Some("mcp") => TrackerAuth::Mcp,
                    _ => TrackerAuth::ApiKey,
                },
                name: text(item, "name")?,
                site: text(item, "site"),
                username: text(item, "username"),
                secret: String::new(),
            })
        })
        .collect()
}

fn merge(
    state: &mut State,
    sources: &MacSources,
    storage: &Storage,
    settings: bool,
) -> (Report, Vec<(PathBuf, PathBuf)>) {
    let mut merge = Merge {
        sources,
        icons: storage.root().join("icons"),
        report: Report::default(),
        copies: Vec::new(),
    };
    let empty = Vec::new();
    let projects = sources
        .workspace
        .get("projects")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let ids = merge.projects(state, projects);
    merge.sessions(state, projects, &ids);
    merge.specs(state, projects, &ids);
    let tasks = sources
        .workspace
        .get("tasks")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    merge.tasks(state, tasks, &ids);
    let commands = sources
        .workspace
        .get("quickCommands")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    merge.quick_commands(state, commands, &ids);
    merge.activity(state);
    if settings {
        merge.settings(state);
    }
    merge.report.connections = connections(&sources.defaults).len();
    (merge.report, merge.copies)
}

/// What an import would bring in, without changing anything.
pub fn preview(
    workspace: &Workspace,
    storage: &Storage,
    sources: &MacSources,
    settings: bool,
) -> Report {
    let mut state = workspace.state().clone();
    merge(&mut state, sources, storage, settings).0
}

/// Imports into `workspace`, then copies icons and saved terminal output and
/// adds the Linear and Jira connections (without their secrets).
pub fn import(
    workspace: &mut Workspace,
    storage: &Storage,
    sources: &MacSources,
    settings: bool,
) -> Result<Report> {
    let mut outcome = None;
    workspace.update(|state| {
        outcome = Some(merge(state, sources, storage, settings));
        Ok(())
    })?;
    let (mut report, copies) = outcome.expect("merged");
    for (from, to) in copies {
        let to = match to.to_string_lossy().strip_prefix("history:") {
            Some(hash) => storage.history().join(format!("{hash}.txt")),
            None => to,
        };
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&from, &to).is_err() {
            report
                .warnings
                .push(format!("Could not copy {}.", from.display()));
        }
    }
    let found = connections(&sources.defaults);
    if !found.is_empty() {
        let mut integrations = Integrations::load(storage.integrations())?;
        let mut added = 0;
        for connection in found {
            if integrations.import(connection)? {
                added += 1;
            }
        }
        report.connections = added;
        if added > 0 {
            report.warnings.push("Linear and Jira keys stay in the macOS app's Keychain: open each connection in Settings → Linear & Jira and enter its key once.".into());
        }
    }
    Ok(report)
}
