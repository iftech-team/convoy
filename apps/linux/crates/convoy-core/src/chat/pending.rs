//! The approvals a chat is waiting on, kept beside the running process so a
//! window that reloads, or never opened the session, can still answer them.
//!
//! The events decide it: an `approval` adds one, its `approval_resolved`
//! removes it, and the end of a turn takes whatever is left — an agent that
//! finished a turn is no longer asking. The process ending drops the whole
//! list with the chat that held it.

use super::ChatEvent;
use serde::Serialize;

/// One approval still waiting: the event the window was sent and when it
/// arrived, in milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PendingApproval {
    pub at: u64,
    pub event: ChatEvent,
}

/// A session's waiting approvals, oldest first.
#[derive(Debug, Default)]
pub struct PendingApprovals {
    waiting: Vec<PendingApproval>,
}

fn request_of(event: &ChatEvent) -> Option<&str> {
    match event {
        ChatEvent::Approval { request, .. } | ChatEvent::ApprovalResolved { request } => {
            Some(request)
        }
        _ => None,
    }
}

impl PendingApprovals {
    /// Applies one event the session sent. `now_ms` stamps a new approval;
    /// one asked again keeps its first time and takes the newer wording.
    pub fn observe(&mut self, event: &ChatEvent, now_ms: u64) {
        match event {
            ChatEvent::Approval { request, .. } => {
                match self
                    .waiting
                    .iter_mut()
                    .find(|pending| request_of(&pending.event) == Some(request))
                {
                    Some(pending) => pending.event = event.clone(),
                    None => self.waiting.push(PendingApproval {
                        at: now_ms,
                        event: event.clone(),
                    }),
                }
            }
            ChatEvent::ApprovalResolved { request } => self
                .waiting
                .retain(|pending| request_of(&pending.event) != Some(request)),
            ChatEvent::TurnEnded { .. } => self.waiting.clear(),
            _ => {}
        }
    }

    pub fn list(&self) -> &[PendingApproval] {
        &self.waiting
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    pub fn clear(&mut self) {
        self.waiting.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ApprovalKind, Usage};

    fn ask(request: &str, title: &str) -> ChatEvent {
        ChatEvent::Approval {
            request: request.into(),
            kind: ApprovalKind::Command,
            title: title.into(),
            detail: "npm test".into(),
            always: true,
        }
    }

    fn requests(pending: &PendingApprovals) -> Vec<String> {
        pending
            .list()
            .iter()
            .filter_map(|item| request_of(&item.event).map(str::to_string))
            .collect()
    }

    #[test]
    fn an_approval_waits_until_it_is_resolved() {
        let mut pending = PendingApprovals::default();
        pending.observe(&ChatEvent::TurnStarted, 1);
        pending.observe(&ask("r1", "Run `npm test`"), 10);
        pending.observe(&ask("r2", "Edit a.js"), 20);
        assert_eq!(requests(&pending), ["r1", "r2"]);
        assert_eq!(pending.list()[0].at, 10);
        assert_eq!(pending.list()[0].event, ask("r1", "Run `npm test`"));

        pending.observe(
            &ChatEvent::ApprovalResolved {
                request: "r1".into(),
            },
            30,
        );
        assert_eq!(requests(&pending), ["r2"]);
        // Resolving one nobody waits for changes nothing.
        pending.observe(
            &ChatEvent::ApprovalResolved {
                request: "r9".into(),
            },
            31,
        );
        assert_eq!(requests(&pending), ["r2"]);
    }

    #[test]
    fn asked_again_it_keeps_its_place_and_first_time() {
        let mut pending = PendingApprovals::default();
        pending.observe(&ask("r1", "first"), 10);
        pending.observe(&ask("r1", "second"), 50);
        assert_eq!(pending.list().len(), 1);
        assert_eq!(pending.list()[0].at, 10);
        assert_eq!(pending.list()[0].event, ask("r1", "second"));
    }

    #[test]
    fn the_end_of_a_turn_takes_what_was_left() {
        let mut pending = PendingApprovals::default();
        pending.observe(&ask("r1", "a"), 10);
        pending.observe(&ask("r2", "b"), 11);
        pending.observe(
            &ChatEvent::TurnEnded {
                usage: Usage::default(),
                error: Some("Interrupted".into()),
            },
            12,
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn the_window_reads_the_event_it_was_sent() {
        let mut pending = PendingApprovals::default();
        pending.observe(&ask("r1", "Run `npm test`"), 42);
        let value = serde_json::to_value(&pending.list()[0]).unwrap();
        assert_eq!(value["at"], 42);
        assert_eq!(value["event"]["type"], "approval");
        assert_eq!(value["event"]["request"], "r1");
        assert_eq!(value["event"]["always"], true);
    }
}
