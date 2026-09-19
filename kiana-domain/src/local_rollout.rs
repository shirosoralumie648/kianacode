//! Managed-local and embedded-local rollout phase contract.
//!
//! This is a pure phase machine. It records which evidence is required before each transition;
//! it does not stop a process, copy a root, promote a binary or publish a ready state by itself.

use crate::{json_digest, ExecutionRevisionPin, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const LOCAL_ROLLOUT_SCHEMA: &str = "kiana.local-rollout.v1";
pub const LOCAL_ROLLOUT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalRolloutMode {
    ManagedLocal,
    EmbeddedLocal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalRolloutPhase {
    Plan,
    Preflight,
    Backup,
    Drain,
    Replace,
    Ready,
    Promote,
}

impl LocalRolloutPhase {
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Plan => Some(Self::Preflight),
            Self::Preflight => Some(Self::Backup),
            Self::Backup => Some(Self::Drain),
            Self::Drain => Some(Self::Replace),
            Self::Replace => Some(Self::Ready),
            Self::Ready => Some(Self::Promote),
            Self::Promote => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRolloutEvidence {
    pub preflight_ready: bool,
    pub backup_verified: bool,
    pub active_run_count: u32,
    pub active_writer_count: u32,
    pub old_revision_fenced: bool,
    pub replacement_started: bool,
    pub readiness_verified: bool,
    pub old_root_retained: bool,
    pub evidence_digest: String,
}

impl LocalRolloutEvidence {
    pub fn validate(&self) -> Result<(), String> {
        validate_digest(&self.evidence_digest, "rollout_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("rollout_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or_default())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalRolloutState {
    pub schema: String,
    pub version: SchemaVersion,
    pub rollout_id: String,
    pub mode: LocalRolloutMode,
    pub pin: ExecutionRevisionPin,
    pub phase: LocalRolloutPhase,
    pub evidence: LocalRolloutEvidence,
    pub phase_sequence: u64,
    pub state_digest: String,
}

impl LocalRolloutState {
    pub fn new(
        rollout_id: impl Into<String>,
        mode: LocalRolloutMode,
        pin: ExecutionRevisionPin,
        evidence: LocalRolloutEvidence,
    ) -> Result<Self, String> {
        let mut state = Self {
            schema: LOCAL_ROLLOUT_SCHEMA.to_owned(),
            version: LOCAL_ROLLOUT_VERSION,
            rollout_id: rollout_id.into(),
            mode,
            pin,
            phase: LocalRolloutPhase::Plan,
            evidence,
            phase_sequence: 1,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state.validate()?;
        Ok(state)
    }

    pub fn advance(
        &mut self,
        next: LocalRolloutPhase,
        evidence: LocalRolloutEvidence,
    ) -> Result<(), String> {
        self.validate()?;
        evidence.validate()?;
        if self.phase.next() != Some(next) {
            return Err("rollout_phase_order_invalid".to_owned());
        }
        match next {
            LocalRolloutPhase::Preflight if !evidence.preflight_ready => {
                return Err("rollout_preflight_blocked".to_owned())
            }
            LocalRolloutPhase::Backup if !evidence.preflight_ready => {
                return Err("rollout_preflight_required".to_owned())
            }
            LocalRolloutPhase::Drain
                if !evidence.backup_verified
                    || !evidence.old_root_retained
                    || evidence.active_run_count > 0
                    || evidence.active_writer_count > 0 =>
            {
                return Err("rollout_drain_not_safe".to_owned())
            }
            LocalRolloutPhase::Replace
                if !evidence.old_revision_fenced
                    || !evidence.backup_verified
                    || !evidence.old_root_retained =>
            {
                return Err("rollout_replace_fence_or_backup_missing".to_owned())
            }
            LocalRolloutPhase::Ready
                if !evidence.replacement_started || !evidence.readiness_verified =>
            {
                return Err("rollout_readiness_not_verified".to_owned())
            }
            LocalRolloutPhase::Promote
                if !evidence.readiness_verified
                    || !evidence.old_revision_fenced
                    || !evidence.old_root_retained =>
            {
                return Err("rollout_promote_gate_blocked".to_owned())
            }
            _ => {}
        }
        self.phase = next;
        self.evidence = evidence;
        self.phase_sequence = self
            .phase_sequence
            .checked_add(1)
            .ok_or_else(|| "rollout_phase_sequence_overflow".to_owned())?;
        self.state_digest = self.digest();
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != LOCAL_ROLLOUT_SCHEMA
            || self.version != LOCAL_ROLLOUT_VERSION
            || self.rollout_id.trim().is_empty()
            || self.rollout_id.len() > 256
            || self.rollout_id.contains('\0')
            || self.phase_sequence == 0
        {
            return Err("rollout_state_invalid".to_owned());
        }
        self.pin.validate()?;
        self.evidence.validate()?;
        validate_digest(&self.state_digest, "rollout_state_digest")?;
        if self.state_digest != self.digest() {
            return Err("rollout_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rollout_id": self.rollout_id,
            "mode": self.mode,
            "pin": self.pin,
            "phase": self.phase,
            "evidence": self.evidence,
            "phase_sequence": self.phase_sequence,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
