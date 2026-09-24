//! The periodic check of what each running agent is doing.
//!
//! Claude reports its own state through the hooks Convoy installs; Codex does
//! not, and its sessions simply show as running. Terminal text is never used
//! to guess whether an agent has finished — the only fact read from the buffer
//! is the Codex resume identifier.
//!
//! Timing matters here. A status file older than half an hour belongs to a
//! previous run, and one no newer than the last seen is the same report again;
//! both are ignored, so a stale `done` cannot mark a fresh launch complete.

use crate::model::TaskStatus;
use crate::workspace::Workspace;
use crate::Result;
use std::path::Path;

/// How far back a status file is still believed.
pub const MAX_AGE_MS: f64 = 30.0 * 60_000.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub at: f64,
    pub state: AgentState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Idle,
    Working,
    Waiting,
    Done,
    Ended,
}

impl AgentState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "idle" => Some(AgentState::Idle),
            "working" => Some(AgentState::Working),
            "waiting" => Some(AgentState::Waiting),
            "done" => Some(AgentState::Done),
            "ended" => Some(AgentState::Ended),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Idle => "idle",
            AgentState::Working => "working",
            AgentState::Waiting => "waiting",
            AgentState::Done => "done",
            AgentState::Ended => "ended",
        }
    }

    /// States worth telling the user about, and worth recording.
    pub fn notable(self) -> bool {
        matches!(self, AgentState::Done | AgentState::Waiting)
    }

    pub fn detail(self) -> &'static str {
        match self {
            AgentState::Done => "Agent completed a turn; acceptance remains unverified",
            AgentState::Waiting => "Agent needs input or permission",
            _ => "",
        }
    }

    pub fn headline(self) -> &'static str {
        match self {
            AgentState::Done => "Agent finished a turn",
            _ => "Agent needs your attention",
        }
    }
}

/// Reads the state a session last reported, if it is fresh enough to believe.
pub fn read_status(root: &Path, session_id: &str, since: f64, now_ms: f64) -> Option<Status> {
    let value = crate::telemetry::read(root, session_id, ".status")?;
    let at = value.get("at").and_then(|value| value.as_f64())?;
    if at <= since || at < now_ms - MAX_AGE_MS {
        return None;
    }
    let state = AgentState::parse(value.get("state")?.as_str()?)?;
    Some(Status { at, state })
}

/// What a reported state means for the task a session is building.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    None,
    /// The agent started another turn, so the task is building again.
    Building,
    /// The agent finished a turn; the task moves to review, never to done.
    Review,
}

pub fn transition(workspace: &Workspace, session_id: &str, state: AgentState) -> Transition {
    let Some(task) = workspace
        .state()
        .tasks
        .iter()
        .find(|task| task.session_id.as_deref() == Some(session_id))
    else {
        return Transition::None;
    };
    match (state, task.status) {
        (AgentState::Working, TaskStatus::Review) => Transition::Building,
        (AgentState::Done, TaskStatus::Building) => Transition::Review,
        _ => Transition::None,
    }
}

pub fn apply(workspace: &mut Workspace, session_id: &str, transition: Transition) -> Result<()> {
    let status = match transition {
        Transition::None => return Ok(()),
        Transition::Building => TaskStatus::Building,
        Transition::Review => TaskStatus::Review,
    };
    let session_id = session_id.to_string();
    workspace
        .update(move |state| {
            if let Some(task) = state
                .tasks
                .iter_mut()
                .find(|task| task.session_id.as_deref() == Some(session_id.as_str()))
            {
                task.status = status;
            }
            Ok(())
        })
        .map(|_| ())
}

/// Hibernation stops an agent only after it has said it finished a turn and
/// then sat idle for the configured period. It is never a guess: without a
/// `done` report nothing is stopped.
pub fn should_hibernate(
    state: Option<AgentState>,
    status_at: f64,
    idle_minutes: i64,
    now_ms: f64,
) -> bool {
    idle_minutes > 0
        && state == Some(AgentState::Done)
        && now_ms - status_at > idle_minutes as f64 * 60_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_or_repeated_report_is_ignored() {
        let now = 1_000_000.0;
        // Fresher than the last seen and within the window: believed.
        assert!(!should_hibernate(Some(AgentState::Done), now, 0, now));
        assert!(should_hibernate(
            Some(AgentState::Done),
            now - 11.0 * 60_000.0,
            10,
            now
        ));
        // Not yet idle long enough.
        assert!(!should_hibernate(
            Some(AgentState::Done),
            now - 60_000.0,
            10,
            now
        ));
        // Working agents are never hibernated, however long they take.
        assert!(!should_hibernate(
            Some(AgentState::Working),
            now - 24.0 * 3_600_000.0,
            10,
            now
        ));
        assert!(!should_hibernate(None, 0.0, 10, now));
    }

    #[test]
    fn states_map_to_the_words_the_user_sees() {
        assert_eq!(AgentState::parse("waiting"), Some(AgentState::Waiting));
        assert_eq!(AgentState::parse("pondering"), None);
        assert!(AgentState::Done.notable() && AgentState::Waiting.notable());
        assert!(!AgentState::Working.notable() && !AgentState::Idle.notable());
        assert!(AgentState::Done
            .detail()
            .contains("acceptance remains unverified"));
    }
}
