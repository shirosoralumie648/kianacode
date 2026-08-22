//! Inbox derived from DeepSeek Harness (MIT) `packages/core/agent`.
//!
//! Waking input goes to `next-turn`; steering/inject goes to `next-step`.
//! Claiming is a delete-only splice so a rejected pre-step still owns the
//! turn boundary without re-processing the same messages.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InboxTarget {
    NextTurn,
    NextStep,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboxMessage {
    pub text: String,
}

impl InboxMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Inbox {
    pub next_turn: Vec<InboxMessage>,
    pub next_step: Vec<InboxMessage>,
}

impl Inbox {
    pub fn insert(&mut self, target: InboxTarget, message: InboxMessage) {
        self.queue_mut(target).push(message);
    }

    pub fn claim(&mut self, target: InboxTarget) -> Vec<InboxMessage> {
        std::mem::take(self.queue_mut(target))
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.next_turn.clear();
        self.next_step.clear();
    }

    #[allow(dead_code)]
    pub fn has_pending(&self) -> bool {
        !self.next_turn.is_empty() || !self.next_step.is_empty()
    }

    fn queue_mut(&mut self, target: InboxTarget) -> &mut Vec<InboxMessage> {
        match target {
            InboxTarget::NextTurn => &mut self.next_turn,
            InboxTarget::NextStep => &mut self.next_step,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_is_delete_only_and_does_not_restore_messages() {
        let mut inbox = Inbox::default();
        inbox.insert(InboxTarget::NextTurn, InboxMessage::user("one"));
        inbox.insert(InboxTarget::NextStep, InboxMessage::user("steer"));
        assert_eq!(inbox.claim(InboxTarget::NextTurn)[0].text, "one");
        assert!(inbox.next_turn.is_empty());
        assert_eq!(inbox.claim(InboxTarget::NextStep)[0].text, "steer");
        assert!(!inbox.has_pending());
    }
}
