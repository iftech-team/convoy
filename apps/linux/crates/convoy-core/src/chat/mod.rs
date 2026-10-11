//! Chat sessions: the same agent conversation as a terminal session, driven
//! over JSON pipes instead of a screen.
//!
//! Each agent has its own protocol — Claude Code's stream-json, Codex's
//! app-server JSON-RPC — and both are reduced here to [`ChatEvent`]s. The
//! process itself belongs to the front end, as the pty does; everything it
//! writes and everything it reads goes through a [`Driver`].

pub mod claude;
pub mod codex;
pub mod event;
pub mod history;

pub use event::{ApprovalKind, ChatEvent, ChatModel, Decision, Image, ToolKind, Usage};

use crate::Result;

/// One conversation's protocol, whichever agent speaks it.
#[derive(Debug)]
pub enum Driver {
    Claude(claude::ClaudeStream),
    Codex(codex::CodexClient),
}

impl Driver {
    /// The lines to write as soon as the process exists.
    pub fn start(&mut self) -> Vec<String> {
        match self {
            Driver::Claude(_) => vec![claude::initialize()],
            Driver::Codex(codex) => codex.start(),
        }
    }

    /// One line the agent printed.
    pub fn feed(&mut self, line: &str) -> Vec<ChatEvent> {
        match self {
            Driver::Claude(claude) => claude.feed(line),
            Driver::Codex(codex) => codex.feed(line),
        }
    }

    /// Lines that reading produced and that must be written back.
    pub fn take_outgoing(&mut self) -> Vec<String> {
        match self {
            Driver::Claude(claude) => claude.take_outgoing(),
            Driver::Codex(codex) => codex.take_outgoing(),
        }
    }

    /// A user turn: the lines to write, and the echo and `turn_started` the
    /// window shows at once.
    pub fn send(&mut self, text: &str, images: &[Image]) -> (Vec<String>, Vec<ChatEvent>) {
        let mut events = vec![ChatEvent::User {
            id: crate::workspace::new_id(),
            text: text.to_string(),
        }];
        let lines = match self {
            Driver::Claude(claude) => {
                let (line, started) = claude.send(text, images);
                events.extend(started);
                vec![line]
            }
            Driver::Codex(codex) => {
                let (lines, started) = codex.send(text, images);
                events.extend(started);
                lines
            }
        };
        (lines, events)
    }

    /// Answers an approval card.
    pub fn answer(&mut self, request: &str, decision: Decision) -> Result<(String, ChatEvent)> {
        match self {
            Driver::Claude(claude) => claude.answer(request, decision),
            Driver::Codex(codex) => codex.answer(request, decision),
        }
    }

    /// Stops the running turn and keeps the process.
    pub fn interrupt(&mut self) -> Vec<String> {
        match self {
            Driver::Claude(claude) => vec![claude.interrupt()],
            Driver::Codex(codex) => codex.interrupt().into_iter().collect(),
        }
    }

    /// Switches the model; none goes back to the agent's default. Claude
    /// switches at once; Codex from the next turn, so nothing is written.
    pub fn set_model(&mut self, model: Option<&str>) -> Vec<String> {
        match self {
            Driver::Claude(claude) => vec![claude.set_model(model)],
            Driver::Codex(codex) => {
                codex.set_model(model);
                Vec::new()
            }
        }
    }

    /// Switches the effort level, as [`Driver::set_model`] does the model.
    pub fn set_effort(&mut self, effort: Option<&str>) -> Vec<String> {
        match self {
            Driver::Claude(claude) => vec![claude.set_effort(effort)],
            Driver::Codex(codex) => {
                codex.set_effort(effort);
                Vec::new()
            }
        }
    }

    /// Whether the agent finished its handshake. A process that dies before
    /// this never got going, and what it printed on stderr is the reason.
    pub fn ready(&self) -> bool {
        match self {
            Driver::Claude(claude) => claude.ready(),
            Driver::Codex(codex) => codex.ready(),
        }
    }
}

/// The recorded spike traffic under `tests/fixtures/chat`.
#[cfg(test)]
pub(crate) mod fixtures {
    use serde_json::Value;

    fn read(agent: &str, name: &str) -> Vec<(String, Value)> {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/chat")
            .join(agent)
            .join(format!("{name}.ndjson"));
        std::fs::read_to_string(&file)
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()))
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let record: Value = serde_json::from_str(line).unwrap();
                (
                    record["dir"].as_str().unwrap().to_string(),
                    record["msg"].clone(),
                )
            })
            .collect()
    }

    /// What the agent printed, one line each, as the process would.
    pub fn inbound(agent: &str, name: &str) -> Vec<String> {
        read(agent, name)
            .into_iter()
            .filter(|(dir, _)| dir == "in")
            .map(|(_, message)| message.to_string())
            .collect()
    }

    /// What the spike wrote to the agent.
    pub fn outbound(agent: &str, name: &str) -> Vec<Value> {
        read(agent, name)
            .into_iter()
            .filter(|(dir, _)| dir == "out")
            .map(|(_, message)| message)
            .collect()
    }
}
