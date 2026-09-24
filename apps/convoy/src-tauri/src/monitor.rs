//! The periodic check of what each running agent is doing.
//!
//! Claude reports its own state through the hooks Convoy installs; Codex does
//! not, and its sessions simply show as running. Terminal text is never used
//! to guess whether an agent has finished — the only fact read from a buffer
//! is the Codex resume identifier.

use convoy_core::monitor::{self, AgentState, Transition};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::State;

use crate::commands::Workspace;
use crate::pty::Terminals;

/// What each session last reported, and when. Held in memory: a stale report
/// from a previous run must not colour a fresh launch.
#[derive(Default)]
pub struct Reports {
    seen: Mutex<HashMap<String, (f64, AgentState)>>,
}

#[derive(Serialize)]
pub struct StateReport {
    pub id: String,
    pub title: String,
    pub state: String,
    /// Worth telling the user about: a finished turn, or a request for input.
    pub notable: bool,
}

#[derive(Serialize)]
pub struct Tick {
    pub states: Vec<StateReport>,
    /// Sessions that have been idle past the configured delay and should stop.
    pub hibernate: Vec<String>,
    /// Whether a task moved, so the front end reloads rather than guessing.
    pub changed: bool,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as f64)
        .unwrap_or(0.0)
}

#[tauri::command]
pub fn monitor_tick(
    workspace: State<'_, Workspace>,
    terminals: State<'_, Arc<Terminals>>,
    reports: State<'_, Reports>,
) -> Result<Tick, String> {
    let now = now_ms();
    let root = workspace.storage.telemetry();
    let running = terminals.ids();

    let mut states = Vec::new();
    let mut changed = false;

    for id in &running {
        let since = reports
            .seen
            .lock()
            .ok()
            .and_then(|seen| seen.get(id).map(|(at, _)| *at))
            .unwrap_or(0.0);
        let Some(status) = monitor::read_status(&root, id, since, now) else {
            continue;
        };

        let previous = reports
            .seen
            .lock()
            .ok()
            .and_then(|seen| seen.get(id).map(|(_, state)| *state));
        let moved = previous != Some(status.state);
        if let Ok(mut seen) = reports.seen.lock() {
            seen.insert(id.clone(), (status.at, status.state));
        }

        let title = workspace
            .with(|core| {
                Ok(core
                    .session(id)
                    .map(|s| s.title.clone())
                    .unwrap_or_default())
            })
            .unwrap_or_default();

        if moved && status.state.notable() {
            let kind = match status.state {
                AgentState::Done => convoy_core::model::ActivityKind::Done,
                _ => convoy_core::model::ActivityKind::Waiting,
            };
            let _ = workspace.act(|core| core.record(kind, id, status.state.detail()).map(|_| ()));
            changed = true;
        }

        let transition = workspace
            .with(|core| Ok(monitor::transition(core, id, status.state)))
            .unwrap_or(Transition::None);
        if transition != Transition::None {
            workspace.act(|core| monitor::apply(core, id, transition))?;
            changed = true;
        }

        states.push(StateReport {
            id: id.clone(),
            title,
            state: status.state.as_str().to_string(),
            notable: moved && status.state.notable(),
        });
    }

    // Hibernation: stop a session that said it finished and then sat idle.
    // Resuming it stays an explicit action.
    let idle = workspace
        .with(|core| Ok(core.settings().hibernate_minutes))
        .unwrap_or(0);
    let mut hibernate = Vec::new();
    if idle > 0 {
        if let Ok(seen) = reports.seen.lock() {
            for id in &running {
                let Some((at, state)) = seen.get(id) else {
                    continue;
                };
                if monitor::should_hibernate(Some(*state), *at, idle, now) {
                    hibernate.push(id.clone());
                }
            }
        }
        for id in &hibernate {
            let _ = workspace.act(|core| {
                core.record(
                    convoy_core::model::ActivityKind::Hibernated,
                    id,
                    "Stopped after completed-turn idle timeout",
                )
                .map(|_| ())
            });
        }
    }

    // A session that stopped leaves nothing behind to compare against.
    if let Ok(mut seen) = reports.seen.lock() {
        seen.retain(|id, _| running.contains(id));
    }

    Ok(Tick {
        states,
        hibernate,
        changed,
    })
}
