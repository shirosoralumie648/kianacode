//! Deterministic UI reconnect/replay/crash recovery decisions.
//!
//! Recovery is a read/query plan only. It never retries a side effect, resumes a run, approves a
//! permission, or talks to a Broker. The original command ID and server cursor remain the facts
//! used by the next ControlPlane query.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const UI_RECOVERY_SCHEMA: &str = "kiana.ui-recovery.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Snapshot,
    Feed,
    ActionAccepted,
    ArtifactFetch,
    Cancel,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryFault {
    Disconnect,
    WorkerKilled,
    Delay,
    Duplicate,
    Gap,
    OldEpoch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryDecision {
    HydrateSnapshot,
    ReplayFeed,
    QueryOriginalCommand,
    QueryArtifact,
    ReconcileUnknown,
    ReplayTerminal,
    Reject,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryInput {
    pub schema: String,
    pub phase: RecoveryPhase,
    pub fault: RecoveryFault,
    pub instance_id: String,
    pub epoch: String,
    pub sequence: u64,
    pub command_id: String,
    #[serde(default)]
    pub terminal_status: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPlan {
    pub schema: String,
    pub decision: RecoveryDecision,
    pub command_id: String,
    pub instance_id: String,
    pub epoch: String,
    pub sequence: u64,
    pub new_effect_allowed: bool,
    pub snapshot_required: bool,
    pub limitation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeedDisposition {
    Accepted { sequence: u64, terminal: bool },
    Duplicate { sequence: u64 },
    Gap { expected: u64, received: u64 },
    OldEpoch,
    LateAfterTerminal,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum RecoveryError {
    #[error("ui_recovery_schema_invalid")]
    SchemaInvalid,
    #[error("ui_recovery_identity_invalid")]
    IdentityInvalid,
    #[error("ui_recovery_sequence_invalid")]
    SequenceInvalid,
    #[error("ui_recovery_terminal_status_invalid")]
    TerminalStatusInvalid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryFence {
    instance_id: String,
    epoch: String,
    last_sequence: u64,
    terminal_seen: bool,
}

impl RecoveryFence {
    pub fn new(
        instance_id: impl Into<String>,
        epoch: impl Into<String>,
    ) -> Result<Self, RecoveryError> {
        let instance_id = bounded(instance_id.into())?;
        let epoch = bounded(epoch.into())?;
        Ok(Self {
            instance_id,
            epoch,
            last_sequence: 0,
            terminal_seen: false,
        })
    }

    pub fn accept_feed(&mut self, epoch: &str, sequence: u64, terminal: bool) -> FeedDisposition {
        if epoch != self.epoch {
            return FeedDisposition::OldEpoch;
        }
        if self.terminal_seen && sequence > self.last_sequence {
            return FeedDisposition::LateAfterTerminal;
        }
        if sequence <= self.last_sequence {
            return FeedDisposition::Duplicate { sequence };
        }
        if sequence != self.last_sequence + 1 {
            return FeedDisposition::Gap {
                expected: self.last_sequence + 1,
                received: sequence,
            };
        }
        self.last_sequence = sequence;
        self.terminal_seen = terminal;
        FeedDisposition::Accepted { sequence, terminal }
    }

    pub fn last_sequence(&self) -> u64 {
        self.last_sequence
    }

    pub fn terminal_seen(&self) -> bool {
        self.terminal_seen
    }

    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }
}

pub fn plan_recovery(input: &RecoveryInput) -> Result<RecoveryPlan, RecoveryError> {
    if input.schema != UI_RECOVERY_SCHEMA {
        return Err(RecoveryError::SchemaInvalid);
    }
    let instance_id = bounded(input.instance_id.clone())?;
    let epoch = bounded(input.epoch.clone())?;
    let command_id = bounded(input.command_id.clone())?;
    if input.sequence == 0 {
        return Err(RecoveryError::SequenceInvalid);
    }
    if input.phase == RecoveryPhase::Terminal && input.terminal_status.is_none() {
        return Err(RecoveryError::TerminalStatusInvalid);
    }
    let (decision, snapshot_required, limitation) = match (input.phase, input.fault) {
        (RecoveryPhase::Snapshot, RecoveryFault::Disconnect | RecoveryFault::OldEpoch)
        | (RecoveryPhase::Feed, RecoveryFault::Gap | RecoveryFault::OldEpoch)
        | (RecoveryPhase::Feed, RecoveryFault::WorkerKilled) => (
            RecoveryDecision::HydrateSnapshot,
            true,
            "snapshot_required_after_epoch_or_gap",
        ),
        (RecoveryPhase::Feed, RecoveryFault::Duplicate) => (
            RecoveryDecision::ReplayFeed,
            false,
            "duplicate_feed_is_display_only",
        ),
        (RecoveryPhase::ActionAccepted, _)
        | (RecoveryPhase::Cancel, RecoveryFault::Disconnect | RecoveryFault::Delay) => (
            RecoveryDecision::QueryOriginalCommand,
            false,
            "query_original_command_no_resubmit",
        ),
        (RecoveryPhase::Cancel, RecoveryFault::WorkerKilled) => (
            RecoveryDecision::ReconcileUnknown,
            true,
            "cancel_result_unknown_requires_reconcile",
        ),
        (RecoveryPhase::ArtifactFetch, _) => (
            RecoveryDecision::QueryArtifact,
            false,
            "query_server_artifact_reference",
        ),
        (
            RecoveryPhase::Terminal,
            RecoveryFault::Duplicate | RecoveryFault::Disconnect | RecoveryFault::Delay,
        ) => (
            RecoveryDecision::ReplayTerminal,
            false,
            "terminal_replay_no_duplicate_effect",
        ),
        (RecoveryPhase::Cancel, RecoveryFault::Gap | RecoveryFault::OldEpoch) => (
            RecoveryDecision::ReconcileUnknown,
            true,
            "cancel_result_unknown_requires_reconcile",
        ),
        (
            RecoveryPhase::Terminal,
            RecoveryFault::WorkerKilled | RecoveryFault::Gap | RecoveryFault::OldEpoch,
        ) => (
            RecoveryDecision::ReconcileUnknown,
            true,
            "terminal_uncertain_requires_reconcile",
        ),
        _ => (
            RecoveryDecision::Reject,
            false,
            "recovery_combination_unsupported",
        ),
    };
    Ok(RecoveryPlan {
        schema: UI_RECOVERY_SCHEMA.to_owned(),
        decision,
        command_id,
        instance_id,
        epoch,
        sequence: input.sequence,
        new_effect_allowed: false,
        snapshot_required,
        limitation: limitation.to_owned(),
    })
}

fn bounded(value: String) -> Result<String, RecoveryError> {
    if value.trim().is_empty() || value.len() > 256 || value.contains('\0') {
        return Err(RecoveryError::IdentityInvalid);
    }
    Ok(value)
}
