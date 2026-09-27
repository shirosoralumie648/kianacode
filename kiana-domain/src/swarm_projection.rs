//! SW-17 parent/child swarm UI/event projection and terminal replay fence.

use crate::{json_digest, SwarmPlanId};
use serde::{Deserialize, Serialize};

pub const SWARM_PROJECTION_SCHEMA: &str = "kiana.swarm-projection.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmProjectionEventKind {
    Progress,
    Child,
    Terminal,
    Hydration,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmProjectionEvent {
    pub schema: String,
    pub epoch: String,
    pub sequence: u64,
    pub source_cursor: u64,
    pub swarm_plan_id: SwarmPlanId,
    pub parent_ref: String,
    pub child_ref: Option<String>,
    pub kind: SwarmProjectionEventKind,
    pub terminal: bool,
    pub payload_digest: String,
    pub event_digest: String,
}

impl SwarmProjectionEvent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_PROJECTION_SCHEMA
            || !valid_text(&self.epoch)
            || self.sequence == 0
            || self.source_cursor == 0
            || self.swarm_plan_id.as_uuid().is_nil()
            || !valid_text(&self.parent_ref)
            || self
                .child_ref
                .as_deref()
                .is_some_and(|value| !valid_text(value))
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.event_digest)
            || self.event_digest != self.digest()
        {
            return Err("swarm_projection_event_invalid".to_owned());
        }
        if self.terminal != (self.kind == SwarmProjectionEventKind::Terminal) {
            return Err("swarm_projection_terminal_kind_mismatch".to_owned());
        }
        if self.kind == SwarmProjectionEventKind::Hydration && self.child_ref.is_some() {
            return Err("swarm_projection_hydration_child_invalid".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "sequence": self.sequence,
            "source_cursor": self.source_cursor,
            "swarm_plan_id": self.swarm_plan_id,
            "parent_ref": self.parent_ref,
            "child_ref": self.child_ref,
            "kind": self.kind,
            "terminal": self.terminal,
            "payload_digest": self.payload_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmProjectionState {
    pub schema: String,
    pub epoch: String,
    pub source_cursor: u64,
    pub last_sequence: u64,
    pub terminal: bool,
    pub event_digests: Vec<String>,
    pub snapshot_digest: String,
}

impl SwarmProjectionState {
    pub fn new(epoch: impl Into<String>) -> Result<Self, String> {
        let mut state = Self {
            schema: SWARM_PROJECTION_SCHEMA.to_owned(),
            epoch: epoch.into(),
            source_cursor: 0,
            last_sequence: 0,
            terminal: false,
            event_digests: Vec::new(),
            snapshot_digest: String::new(),
        };
        state.snapshot_digest = state.digest();
        state.validate()?;
        Ok(state)
    }

    pub fn apply(&mut self, event: &SwarmProjectionEvent) -> Result<(), String> {
        event.validate()?;
        if event.epoch != self.epoch {
            return Err("swarm_projection_epoch_mismatch".to_owned());
        }
        if event.sequence <= self.last_sequence {
            if self.event_digests.contains(&event.event_digest) {
                return Ok(());
            }
            return Err("swarm_projection_sequence_conflict".to_owned());
        }
        if event.sequence != self.last_sequence.saturating_add(1) {
            return Err("swarm_projection_sequence_gap".to_owned());
        }
        if self.terminal {
            return Err("swarm_projection_terminal_resurrection".to_owned());
        }
        self.last_sequence = event.sequence;
        self.source_cursor = event.source_cursor;
        self.terminal = event.terminal;
        self.event_digests.push(event.event_digest.clone());
        self.snapshot_digest = self.digest();
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_PROJECTION_SCHEMA
            || !valid_text(&self.epoch)
            || (self.last_sequence == 0 && self.source_cursor != 0)
            || self.event_digests.len() as u64 != self.last_sequence
            || self.event_digests.iter().any(|value| !valid_digest(value))
            || !valid_digest(&self.snapshot_digest)
            || self.snapshot_digest != self.digest()
        {
            return Err("swarm_projection_state_invalid".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "epoch": self.epoch,
            "source_cursor": self.source_cursor,
            "last_sequence": self.last_sequence,
            "terminal": self.terminal,
            "event_digests": self.event_digests,
        }))
    }
}

pub fn validate_swarm_projection_event(event: &SwarmProjectionEvent) -> Result<(), String> {
    event.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
