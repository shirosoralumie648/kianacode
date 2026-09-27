//! SW-06 bounded queue, fair ordering and claim-fence source contract.

use crate::{json_digest, DispatchIntentId, QueueEntryId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SWARM_QUEUE_SCHEMA: &str = "kiana.swarm-queue.v1";
pub const SWARM_QUEUE_ENTRY_SCHEMA: &str = "kiana.swarm-queue-entry.v1";
const MAX_ENTRIES: usize = 256;
const MAX_BACKOFF_MS: u64 = 86_400_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmQueueEntryState {
    Ready,
    Delayed,
    Claimed,
    Completed,
    Fenced,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmQueueEntry {
    pub schema: String,
    pub entry_id: QueueEntryId,
    pub dispatch_intent_id: DispatchIntentId,
    pub work_fingerprint: String,
    pub session_key: String,
    pub sequence: u64,
    pub attempt: u32,
    pub max_attempts: u32,
    pub not_before_unix_ms: u64,
    pub fence_epoch: u64,
    pub state: SwarmQueueEntryState,
    pub entry_digest: String,
}

impl SwarmQueueEntry {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_QUEUE_ENTRY_SCHEMA
            || self.entry_id.as_uuid().is_nil()
            || self.dispatch_intent_id.as_uuid().is_nil()
            || !valid_digest(&self.work_fingerprint)
            || !valid_text(&self.session_key)
            || self.sequence == 0
            || self.max_attempts == 0
            || self.attempt > self.max_attempts
            || self.fence_epoch == 0
            || !valid_digest(&self.entry_digest)
            || self.entry_digest != self.canonical_digest()
        {
            return Err("swarm_queue_entry_invalid");
        }
        if self.state == SwarmQueueEntryState::Completed && self.attempt == 0 {
            return Err("swarm_queue_terminal_attempt_invalid");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "entry_id": self.entry_id,
            "dispatch_intent_id": self.dispatch_intent_id,
            "work_fingerprint": self.work_fingerprint,
            "session_key": self.session_key,
            "sequence": self.sequence,
            "attempt": self.attempt,
            "max_attempts": self.max_attempts,
            "not_before_unix_ms": self.not_before_unix_ms,
            "fence_epoch": self.fence_epoch,
            "state": self.state,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwarmQueueLedger {
    pub schema: String,
    pub capacity: u32,
    pub worker_epoch: u64,
    pub revision: u64,
    entries: BTreeMap<QueueEntryId, SwarmQueueEntry>,
    active: u32,
}

impl SwarmQueueLedger {
    pub fn new(capacity: u32, worker_epoch: u64) -> Result<Self, &'static str> {
        if capacity == 0 || capacity as usize > MAX_ENTRIES || worker_epoch == 0 {
            return Err("swarm_queue_header_invalid");
        }
        Ok(Self {
            schema: SWARM_QUEUE_SCHEMA.to_owned(),
            capacity,
            worker_epoch,
            revision: 0,
            entries: BTreeMap::new(),
            active: 0,
        })
    }

    pub fn enqueue(&mut self, entry: SwarmQueueEntry) -> Result<(), &'static str> {
        entry.validate()?;
        if self.entries.len() >= MAX_ENTRIES || self.entries.contains_key(&entry.entry_id) {
            return Err("swarm_queue_entry_conflict");
        }
        if entry.fence_epoch != self.worker_epoch {
            return Err("swarm_queue_stale_fence");
        }
        self.entries.insert(entry.entry_id, entry);
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    /// Claim is ordered by sequence, then session, then stable entry ID.  It is a queue claim,
    /// not an execution permit; the effect boundary must perform its own fence check.
    pub fn claim(&mut self, now_unix_ms: u64) -> Result<SwarmQueueEntry, &'static str> {
        if self.active >= self.capacity {
            return Err("swarm_queue_capacity_exceeded");
        }
        let candidate = self
            .entries
            .values()
            .filter(|entry| {
                matches!(
                    entry.state,
                    SwarmQueueEntryState::Ready | SwarmQueueEntryState::Delayed
                ) && entry.fence_epoch == self.worker_epoch
                    && entry.not_before_unix_ms <= now_unix_ms
            })
            .min_by(|left, right| {
                (left.sequence, &left.session_key, left.entry_id).cmp(&(
                    right.sequence,
                    &right.session_key,
                    right.entry_id,
                ))
            })
            .cloned()
            .ok_or("swarm_queue_no_ready_entry")?;
        let mut claimed = candidate.clone();
        claimed.state = SwarmQueueEntryState::Claimed;
        claimed.attempt = claimed.attempt.saturating_add(1);
        claimed.entry_digest = claimed.canonical_digest();
        claimed.validate()?;
        self.entries.insert(claimed.entry_id, claimed.clone());
        self.active = self.active.saturating_add(1);
        self.revision = self.revision.saturating_add(1);
        Ok(claimed)
    }

    pub fn complete(
        &mut self,
        entry_id: QueueEntryId,
        fence_epoch: u64,
    ) -> Result<(), &'static str> {
        if fence_epoch != self.worker_epoch {
            return Err("swarm_queue_stale_fence");
        }
        let entry_state = self
            .entries
            .get(&entry_id)
            .ok_or("swarm_queue_entry_missing")?;
        if entry_state.state != SwarmQueueEntryState::Claimed {
            return Err("swarm_queue_claim_state_invalid");
        }
        self.active = self
            .active
            .checked_sub(1)
            .ok_or("swarm_queue_active_underflow")?;
        let entry = self
            .entries
            .get_mut(&entry_id)
            .ok_or("swarm_queue_entry_missing")?;
        entry.state = SwarmQueueEntryState::Completed;
        entry.entry_digest = entry.canonical_digest();
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }

    pub fn retry(
        &mut self,
        entry_id: QueueEntryId,
        fence_epoch: u64,
        now_unix_ms: u64,
        backoff_ms: u64,
    ) -> Result<(), &'static str> {
        if fence_epoch != self.worker_epoch {
            return Err("swarm_queue_stale_fence");
        }
        if backoff_ms == 0 || backoff_ms > MAX_BACKOFF_MS {
            return Err("swarm_queue_backoff_invalid");
        }
        let entry_state = self
            .entries
            .get(&entry_id)
            .ok_or("swarm_queue_entry_missing")?;
        if entry_state.state != SwarmQueueEntryState::Claimed {
            return Err("swarm_queue_claim_state_invalid");
        }
        self.active = self
            .active
            .checked_sub(1)
            .ok_or("swarm_queue_active_underflow")?;
        let entry = self
            .entries
            .get_mut(&entry_id)
            .ok_or("swarm_queue_entry_missing")?;
        if entry.attempt >= entry.max_attempts {
            entry.state = SwarmQueueEntryState::Fenced;
        } else {
            entry.state = SwarmQueueEntryState::Delayed;
            entry.not_before_unix_ms = now_unix_ms.saturating_add(backoff_ms);
        }
        entry.entry_digest = entry.canonical_digest();
        self.revision = self.revision.saturating_add(1);
        Ok(())
    }
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 256
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
