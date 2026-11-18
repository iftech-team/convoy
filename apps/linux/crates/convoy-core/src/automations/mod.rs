//! Scheduled automations: a prompt a project's agent runs on its own, daily,
//! on chosen weekdays or every few hours.
//!
//! They are kept in a file of their own beside the workspace,
//! `automations.json`, not in `workspace.json`: the macOS app rewrites that
//! file whole and drops keys it does not know.
//!
//! This module holds the rules — the record, the checks, when one is due and
//! what a run leaves behind. The app's scheduler runs them every half minute.

pub mod schedule;

use crate::json::utf16_len;
use crate::patterns::MODEL;
use crate::workspace::model::{Agent, VIEWS};
use crate::workspace::{new_id, Workspace};
use crate::{bail, ensure, ConvoyError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub use schedule::{
    local_stamp, next_run, next_runs, Day, Instant, LocalZone, RuleZone, Schedule, Zone,
};

/// Runs kept per automation, newest first.
pub const HISTORY: usize = 20;
/// Runs the scheduler starts in one pass at most; the rest wait for the next.
pub const MAX_STARTS_PER_TICK: usize = 3;
/// How often the scheduler looks, in seconds.
pub const TICK_SECONDS: u64 = 30;
/// A run found this long after its time was missed — the app was closed or
/// the computer asleep — rather than merely picked up on the next pass.
pub const MISSED_AFTER: Instant = 10 * 60;

const FILE_VERSION: u32 = 1;

fn yes() -> bool {
    true
}

fn terminal() -> String {
    "terminal".into()
}

/// How a run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    /// Its session was started and has not finished its turn.
    Started,
    Done,
    Failed,
    /// Not started: the previous run was still going.
    Skipped,
}

/// One run, or one that was passed over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// When it started, or was skipped.
    pub at: Instant,
    /// The scheduled time it was for; none for Run now.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<Instant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: RunStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Started with Run now.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub manual: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Automation {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub prompt: String,
    pub agent: Agent,
    /// Empty for the agent's default.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    /// The account it signs in with; none is the system login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    /// `chat` or `terminal`.
    #[serde(default = "terminal")]
    pub view: String,
    /// Each run in a fresh worktree of its own.
    #[serde(default)]
    pub worktree: bool,
    pub schedule: Schedule,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// A run missed while Convoy was closed runs once when it opens.
    #[serde(default = "yes")]
    pub catch_up: bool,
    pub created: Instant,
    /// Last edited or switched on: times before it are not owed.
    #[serde(default)]
    pub updated: Instant,
    /// The last scheduled time that was dealt with — run, skipped or let go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_slot: Option<Instant>,
    /// Newest first, at most [`HISTORY`].
    #[serde(default)]
    pub history: Vec<Run>,
    /// Skills each run's session starts with, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Automation {
    pub fn last_run(&self) -> Option<&Run> {
        self.history.first()
    }

    /// The run still going, if any.
    pub fn active_run(&self) -> Option<&Run> {
        self.history
            .iter()
            .find(|run| run.status == RunStatus::Started)
    }

    /// Scheduled times after this are owed.
    pub fn baseline(&self) -> Instant {
        self.created
            .max(self.updated)
            .max(self.last_slot.unwrap_or(Instant::MIN))
    }

    /// The next time it runs, when it is on.
    pub fn next_run(&self, now: Instant, zone: &dyn Zone) -> Option<Instant> {
        if !self.enabled {
            return None;
        }
        // One owed already is next, however late.
        match due(self, now, zone) {
            Some(Due::Run { slot, .. }) => Some(slot),
            _ => next_run(&self.schedule, now, zone),
        }
    }

    fn push(&mut self, run: Run) {
        self.history.insert(0, run);
        self.history.truncate(HISTORY);
    }
}

/// What the editor sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationInput {
    /// None to create one.
    #[serde(default)]
    pub id: Option<String>,
    pub project_id: String,
    pub title: String,
    pub prompt: String,
    pub agent: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default = "terminal")]
    pub view: String,
    #[serde(default)]
    pub worktree: bool,
    pub schedule: Schedule,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub catch_up: bool,
    /// Skills its runs start with. None leaves an existing automation's as
    /// they are; a new one then has none.
    #[serde(default)]
    pub skills: Option<Vec<String>>,
}

/// Checks what the editor sent against the workspace it will run in.
pub fn validate(input: &AutomationInput, workspace: &Workspace) -> Result<Agent> {
    let title = input.title.trim();
    ensure!(!title.is_empty(), "Give the automation a title.");
    ensure!(
        utf16_len(title) <= 120,
        "The title is too long (120 characters at most)."
    );
    let prompt = input.prompt.trim();
    ensure!(!prompt.is_empty(), "Write the prompt the agent runs.");
    ensure!(
        utf16_len(prompt) <= 32_000,
        "The prompt is too long (32,000 characters at most)."
    );
    workspace.project(&input.project_id)?;
    let agent = Agent::parse(&input.agent)
        .ok_or_else(|| ConvoyError::message("Unknown agent. Use claude or codex."))?;
    ensure!(
        utf16_len(&input.model) <= 100 && MODEL.is_match(&input.model),
        "Invalid model name."
    );
    ensure!(
        input.effort.is_empty() || crate::provider::valid_effort(agent, &input.effort),
        "Invalid effort."
    );
    if let Some(profile) = input.profile_id.as_deref().filter(|id| !id.is_empty()) {
        let known = workspace
            .state()
            .profiles
            .iter()
            .any(|item| item.id == profile && item.agent == agent);
        ensure!(known, "That account is not one of this agent's.");
    }
    ensure!(
        VIEWS.contains(&input.view.as_str()),
        "Pick Chat or Terminal."
    );
    if let Some(problem) = input.schedule.problem() {
        bail!("{problem}");
    }
    Ok(agent)
}

/// What one pass of the scheduler should do with one automation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    /// Run it for this scheduled time. `missed` when the time passed while
    /// Convoy was closed or the computer slept.
    Run { slot: Instant, missed: bool },
    /// A time was missed and it does not catch up: let it go quietly.
    Pass { slot: Instant },
}

/// Whether an automation is due at `now`. However many times were missed,
/// it is due once, for the latest of them.
pub fn due(automation: &Automation, now: Instant, zone: &dyn Zone) -> Option<Due> {
    if !automation.enabled {
        return None;
    }
    let first = next_run(&automation.schedule, automation.baseline(), zone)?;
    if first > now {
        return None;
    }
    // The latest time owed; nine days back covers every schedule's cycle.
    let mut slot = first;
    let mut from = first.max(now - 9 * 86_400);
    while let Some(next) = next_run(&automation.schedule, from, zone) {
        if next > now {
            break;
        }
        slot = slot.max(next);
        from = next;
    }
    let missed = now - slot > MISSED_AFTER;
    Some(if missed && !automation.catch_up {
        Due::Pass { slot }
    } else {
        Due::Run { slot, missed }
    })
}

/// What one pass decided.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tick {
    /// `(automation, slot, missed)` to start now.
    pub start: Vec<(String, Instant, bool)>,
    /// `(automation, slot)` skipped: its last run is still going.
    pub skip: Vec<(String, Instant)>,
    /// `(automation, slot)` missed and let go.
    pass: Vec<(String, Instant)>,
}

impl Tick {
    pub fn passed(&self) -> &[(String, Instant)] {
        &self.pass
    }

    pub fn is_empty(&self) -> bool {
        self.start.is_empty() && self.skip.is_empty() && self.pass.is_empty()
    }
}

/// One pass over every automation. `busy` says whether an automation's run
/// is still going; at most [`MAX_STARTS_PER_TICK`] start, the rest wait.
pub fn plan_tick(
    automations: &[Automation],
    now: Instant,
    zone: &dyn Zone,
    busy: &dyn Fn(&Automation) -> bool,
) -> Tick {
    let mut tick = Tick::default();
    for automation in automations {
        match due(automation, now, zone) {
            None => {}
            Some(Due::Pass { slot }) => tick.pass.push((automation.id.clone(), slot)),
            Some(Due::Run { slot, missed }) => {
                if busy(automation) {
                    tick.skip.push((automation.id.clone(), slot));
                } else if tick.start.len() < MAX_STARTS_PER_TICK {
                    tick.start.push((automation.id.clone(), slot, missed));
                }
            }
        }
    }
    tick
}

/// A run's session title: the automation's, and when.
pub fn session_title(title: &str, zone: &dyn Zone, at: Instant) -> String {
    format!("{} · {}", title.trim(), local_stamp(zone, at))
}

/// `automation/<slug>-<yyyymmdd-hhmm>`.
pub fn branch_for(title: &str, zone: &dyn Zone, at: Instant) -> String {
    let mut slug = String::new();
    for c in title.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(40).collect();
    let slug = slug.trim_end_matches('-');
    let stamp = schedule::compact_stamp(zone, at);
    if slug.is_empty() {
        format!("automation/run-{stamp}")
    } else {
        format!("automation/{slug}-{stamp}")
    }
}

// ---------------------------------------------------------------------------
// The file.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationsFile {
    pub version: u32,
    #[serde(default)]
    pub automations: Vec<Automation>,
    #[serde(flatten)]
    pub unknown: Map<String, Value>,
}

impl Default for AutomationsFile {
    fn default() -> Self {
        AutomationsFile {
            version: FILE_VERSION,
            automations: Vec::new(),
            unknown: Map::new(),
        }
    }
}

/// `automations.json`, loaded once and written whole, atomically.
#[derive(Debug)]
pub struct Store {
    file: PathBuf,
    state: AutomationsFile,
}

impl Store {
    pub fn load(file: impl Into<PathBuf>) -> Result<Store> {
        let file = file.into();
        let state = match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str::<AutomationsFile>(&text).map_err(|_| {
                ConvoyError::message(format!(
                    "The automations file is damaged: {}",
                    file.display()
                ))
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                AutomationsFile::default()
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            state.version <= FILE_VERSION,
            "The automations file was written by a newer Convoy."
        );
        Ok(Store { file, state })
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn all(&self) -> &[Automation] {
        &self.state.automations
    }

    pub fn get(&self, id: &str) -> Result<&Automation> {
        self.state
            .automations
            .iter()
            .find(|automation| automation.id == id)
            .ok_or_else(|| ConvoyError::message("Automation not found."))
    }

    pub fn for_project<'a>(&'a self, project_id: &'a str) -> impl Iterator<Item = &'a Automation> {
        self.state
            .automations
            .iter()
            .filter(move |automation| automation.project_id == project_id)
    }

    /// Applies a change and writes the file; a failed write changes nothing.
    pub fn update<T>(
        &mut self,
        change: impl FnOnce(&mut AutomationsFile) -> Result<T>,
    ) -> Result<T> {
        let mut next = self.state.clone();
        let value = change(&mut next)?;
        let body = serde_json::to_string_pretty(&next)?;
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut temporary = self.file.clone().into_os_string();
        temporary.push(format!(".{}.tmp", Uuid::new_v4()));
        let temporary = PathBuf::from(temporary);
        let outcome = (|| -> Result<()> {
            let mut handle = crate::fs::create_private(&temporary)?;
            std::io::Write::write_all(&mut handle, body.as_bytes())?;
            handle.sync_all()?;
            drop(handle);
            fs::rename(&temporary, &self.file)?;
            Ok(())
        })();
        let _ = fs::remove_file(&temporary);
        outcome?;
        self.state = next;
        Ok(value)
    }

    fn with<T>(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Automation) -> Result<T>,
    ) -> Result<T> {
        let id = id.to_string();
        self.update(move |file| {
            let automation = file
                .automations
                .iter_mut()
                .find(|automation| automation.id == id)
                .ok_or_else(|| ConvoyError::message("Automation not found."))?;
            change(automation)
        })
    }

    /// Creates or edits one. Returns its id.
    pub fn save(
        &mut self,
        input: AutomationInput,
        workspace: &Workspace,
        now: Instant,
    ) -> Result<String> {
        let agent = validate(&input, workspace)?;
        let profile_id = input.profile_id.clone().filter(|id| !id.is_empty());
        match input.id.clone() {
            Some(id) => self.with(&id, |automation| {
                ensure!(
                    automation.project_id == input.project_id,
                    "An automation stays in its project."
                );
                let timing =
                    automation.schedule != input.schedule || (!automation.enabled && input.enabled);
                automation.title = input.title.trim().to_string();
                automation.prompt = input.prompt.trim().to_string();
                automation.agent = agent;
                automation.model = input.model.clone();
                automation.effort = input.effort.clone();
                automation.profile_id = profile_id;
                automation.view = input.view.clone();
                automation.worktree = input.worktree;
                automation.schedule = input.schedule.clone();
                automation.enabled = input.enabled;
                automation.catch_up = input.catch_up;
                if let Some(skills) = input.skills.clone() {
                    automation.skills = crate::planning::attached(skills);
                }
                // A new schedule owes nothing from before it.
                if timing {
                    automation.updated = now;
                }
                Ok(id.clone())
            }),
            None => {
                let id = new_id();
                let automation = Automation {
                    id: id.clone(),
                    project_id: input.project_id.clone(),
                    title: input.title.trim().to_string(),
                    prompt: input.prompt.trim().to_string(),
                    agent,
                    model: input.model.clone(),
                    effort: input.effort.clone(),
                    profile_id,
                    view: input.view.clone(),
                    worktree: input.worktree,
                    schedule: input.schedule.clone(),
                    enabled: input.enabled,
                    catch_up: input.catch_up,
                    created: now,
                    updated: now,
                    last_slot: None,
                    history: Vec::new(),
                    skills: crate::planning::attached(input.skills.clone().unwrap_or_default()),
                    unknown: Map::new(),
                };
                self.update(move |file| {
                    file.automations.push(automation);
                    Ok(())
                })?;
                Ok(id)
            }
        }
    }

    pub fn remove(&mut self, id: &str) -> Result<()> {
        self.get(id)?;
        let id = id.to_string();
        self.update(move |file| {
            file.automations.retain(|automation| automation.id != id);
            Ok(())
        })
    }

    /// Skills that no longer exist come off every automation.
    pub fn detach_skills(&mut self, ids: &[String]) -> Result<()> {
        let holds = |automation: &Automation| {
            automation
                .skills
                .as_ref()
                .is_some_and(|list| list.iter().any(|id| ids.contains(id)))
        };
        if !self.state.automations.iter().any(holds) {
            return Ok(());
        }
        let ids = ids.to_vec();
        self.update(move |file| {
            for automation in file.automations.iter_mut() {
                if let Some(kept) = automation.skills.take() {
                    automation.skills = crate::planning::attached(
                        kept.into_iter().filter(|id| !ids.contains(id)).collect(),
                    );
                }
            }
            Ok(())
        })
    }

    /// Switching one on owes nothing from while it was off.
    pub fn set_enabled(&mut self, id: &str, enabled: bool, now: Instant) -> Result<()> {
        self.with(id, |automation| {
            if enabled && !automation.enabled {
                automation.updated = now;
            }
            automation.enabled = enabled;
            Ok(())
        })
    }

    /// A project went: so do its automations. Returns how many.
    pub fn remove_project(&mut self, project_id: &str) -> Result<usize> {
        let count = self.for_project(project_id).count();
        if count == 0 {
            return Ok(0);
        }
        let project_id = project_id.to_string();
        self.update(move |file| {
            file.automations
                .retain(|automation| automation.project_id != project_id);
            Ok(count)
        })
    }

    pub fn remove_all(&mut self) -> Result<()> {
        if self.state.automations.is_empty() {
            return Ok(());
        }
        self.update(|file| {
            file.automations.clear();
            Ok(())
        })
    }

    /// Drops automations whose project is no longer in the workspace — one
    /// removed by another build, which knows nothing of this file.
    pub fn prune(&mut self, workspace: &Workspace) -> Result<usize> {
        let known = |id: &str| workspace.state().projects.iter().any(|p| p.id == id);
        let gone = self
            .state
            .automations
            .iter()
            .filter(|automation| !known(&automation.project_id))
            .count();
        if gone == 0 {
            return Ok(0);
        }
        let projects: Vec<String> = workspace
            .state()
            .projects
            .iter()
            .map(|project| project.id.clone())
            .collect();
        self.update(move |file| {
            file.automations
                .retain(|automation| projects.contains(&automation.project_id));
            Ok(gone)
        })
    }

    /// A run started: its session, and the time it was for.
    pub fn record_start(
        &mut self,
        id: &str,
        session_id: &str,
        slot: Option<Instant>,
        now: Instant,
    ) -> Result<()> {
        let session_id = session_id.to_string();
        self.with(id, move |automation| {
            if let Some(slot) = slot {
                automation.last_slot = Some(slot);
            }
            automation.push(Run {
                at: now,
                slot,
                session_id: Some(session_id),
                status: RunStatus::Started,
                note: String::new(),
                manual: slot.is_none(),
            });
            Ok(())
        })
    }

    /// A run that could not start, or was skipped. The time it was for is
    /// dealt with either way: it is not tried again.
    pub fn record_outcome(
        &mut self,
        id: &str,
        status: RunStatus,
        note: &str,
        session_id: Option<&str>,
        slot: Option<Instant>,
        now: Instant,
    ) -> Result<()> {
        let note = note.to_string();
        let session_id = session_id.map(str::to_string);
        self.with(id, move |automation| {
            if let Some(slot) = slot {
                automation.last_slot = Some(slot);
            }
            automation.push(Run {
                at: now,
                slot,
                session_id,
                status,
                note,
                manual: slot.is_none(),
            });
            Ok(())
        })
    }

    /// A missed time let go without a run or a trace in the history.
    pub fn pass(&mut self, id: &str, slot: Instant) -> Result<()> {
        self.with(id, |automation| {
            automation.last_slot = Some(slot);
            Ok(())
        })
    }

    /// A run's session finished. Returns the automation it belonged to, when
    /// that run was still open.
    pub fn finish(
        &mut self,
        session_id: &str,
        status: RunStatus,
        note: &str,
    ) -> Result<Option<Automation>> {
        let open = self.state.automations.iter().position(|automation| {
            automation.history.iter().any(|run| {
                run.status == RunStatus::Started && run.session_id.as_deref() == Some(session_id)
            })
        });
        let Some(index) = open else {
            return Ok(None);
        };
        let (session_id, note) = (session_id.to_string(), note.to_string());
        self.update(move |file| {
            let automation = &mut file.automations[index];
            for run in automation.history.iter_mut() {
                if run.status == RunStatus::Started
                    && run.session_id.as_deref() == Some(session_id.as_str())
                {
                    run.status = status;
                    run.note = note.clone();
                }
            }
            Ok(Some(automation.clone()))
        })
    }

    /// Runs left open by a Convoy that closed while they went: failed, since
    /// nothing will ever report on them. `alive` says whether a session's
    /// run is still followed.
    pub fn settle_interrupted(&mut self, alive: &dyn Fn(&str) -> bool) -> Result<usize> {
        let stale = |run: &Run| {
            run.status == RunStatus::Started && !run.session_id.as_deref().is_some_and(alive)
        };
        let count: usize = self
            .state
            .automations
            .iter()
            .map(|automation| automation.history.iter().filter(|run| stale(run)).count())
            .sum();
        if count == 0 {
            return Ok(0);
        }
        self.update(|file| {
            for automation in file.automations.iter_mut() {
                for run in automation.history.iter_mut() {
                    if stale(run) {
                        run.status = RunStatus::Failed;
                        run.note = "Convoy closed while it ran.".into();
                    }
                }
            }
            Ok(count)
        })
    }
}

#[cfg(test)]
mod tests;
