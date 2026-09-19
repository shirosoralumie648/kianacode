//! Migration runner state, fencing, resume-token and quarantine contracts.
//!
//! This module is a pure state machine. It emits facts for a future EventLog adapter and never
//! applies a migration itself. In particular, a failed state cannot be advanced in memory just
//! because a caller presents an old checkpoint or resume token.

use crate::{
    json_digest, MigrationBatchPlan, MigrationCheckpoint, MigrationPrimitivePhase,
    MigrationRegistry, SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const MIGRATION_RUNNER_SCHEMA: &str = "kiana.migration-runner.v1";
pub const MIGRATION_RUNNER_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MIGRATION_RESUME_TOKEN_SCHEMA: &str = "kiana.migration-resume-token.v1";
pub const MIGRATION_RESUME_TOKEN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MIGRATION_RUNNER_EVENT_SCHEMA: &str = "kiana.migration-runner-event.v1";
pub const MAX_MIGRATION_RUN_ID: usize = 256;
pub const MAX_MIGRATION_REASON: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRunnerStatus {
    Active,
    Quarantined,
    Completed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationRunnerEventKind {
    Started,
    Step,
    Blocked,
    Completed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRunnerEvent {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: MigrationRunnerEventKind,
    pub run_id: String,
    pub registry_digest: String,
    pub owner: String,
    pub fence_token: u64,
    pub sequence: u64,
    pub checkpoint_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub event_digest: String,
}

impl MigrationRunnerEvent {
    fn new(
        kind: MigrationRunnerEventKind,
        state: &MigrationRunnerState,
        sequence: u64,
        checkpoint_digest: String,
        reason: Option<String>,
    ) -> Result<Self, String> {
        let mut event = Self {
            schema: MIGRATION_RUNNER_EVENT_SCHEMA.to_owned(),
            version: MIGRATION_RUNNER_VERSION,
            kind,
            run_id: state.run_id.clone(),
            registry_digest: state.registry_digest.clone(),
            owner: state.owner.clone(),
            fence_token: state.fence_token,
            sequence,
            checkpoint_digest,
            reason,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_RUNNER_EVENT_SCHEMA
            || self.version != MIGRATION_RUNNER_VERSION
            || !bounded(&self.run_id, MAX_MIGRATION_RUN_ID)
            || !bounded(&self.owner, MAX_MIGRATION_RUN_ID)
            || self.fence_token == 0
            || self.sequence == 0
            || self.reason.as_deref().is_some_and(|reason| {
                !bounded(reason, MAX_MIGRATION_REASON) || reason.contains('\n')
            })
        {
            return Err("migration_runner_event_invalid".to_owned());
        }
        validate_digest(
            &self.registry_digest,
            "migration_runner_event_registry_digest",
        )?;
        validate_digest(
            &self.checkpoint_digest,
            "migration_runner_event_checkpoint_digest",
        )?;
        validate_digest(&self.event_digest, "migration_runner_event_digest")?;
        if self.event_digest != self.digest() {
            return Err("migration_runner_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "run_id": self.run_id,
            "registry_digest": self.registry_digest,
            "owner": self.owner,
            "fence_token": self.fence_token,
            "sequence": self.sequence,
            "checkpoint_digest": self.checkpoint_digest,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationResumeToken {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: String,
    pub registry_digest: String,
    pub owner: String,
    pub fence_token: u64,
    pub event_sequence: u64,
    pub checkpoint_digest: String,
    pub token_digest: String,
}

impl MigrationResumeToken {
    fn from_state(state: &MigrationRunnerState) -> Result<Self, String> {
        let mut token = Self {
            schema: MIGRATION_RESUME_TOKEN_SCHEMA.to_owned(),
            version: MIGRATION_RESUME_TOKEN_VERSION,
            run_id: state.run_id.clone(),
            registry_digest: state.registry_digest.clone(),
            owner: state.owner.clone(),
            fence_token: state.fence_token,
            event_sequence: state.last_event_sequence,
            checkpoint_digest: state.checkpoint.checkpoint_digest.clone(),
            token_digest: String::new(),
        };
        token.token_digest = token.digest();
        token.validate()?;
        Ok(token)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_RESUME_TOKEN_SCHEMA
            || self.version != MIGRATION_RESUME_TOKEN_VERSION
            || !bounded(&self.run_id, MAX_MIGRATION_RUN_ID)
            || !bounded(&self.owner, MAX_MIGRATION_RUN_ID)
            || self.fence_token == 0
            || self.event_sequence == 0
        {
            return Err("migration_resume_token_invalid".to_owned());
        }
        validate_digest(&self.registry_digest, "migration_resume_registry_digest")?;
        validate_digest(
            &self.checkpoint_digest,
            "migration_resume_checkpoint_digest",
        )?;
        validate_digest(&self.token_digest, "migration_resume_token_digest")?;
        if self.token_digest != self.digest() {
            return Err("migration_resume_token_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "registry_digest": self.registry_digest,
            "owner": self.owner,
            "fence_token": self.fence_token,
            "event_sequence": self.event_sequence,
            "checkpoint_digest": self.checkpoint_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationRunnerState {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: String,
    pub registry_digest: String,
    pub owner: String,
    pub fence_token: u64,
    pub lease_expires_at_unix_ms: u64,
    pub status: MigrationRunnerStatus,
    pub checkpoint: MigrationCheckpoint,
    pub last_event_sequence: u64,
    pub state_digest: String,
}

impl MigrationRunnerState {
    pub fn start(
        registry: &MigrationRegistry,
        run_id: impl Into<String>,
        owner: impl Into<String>,
        fence_token: u64,
        now_unix_ms: u64,
        lease_expires_at_unix_ms: u64,
        existing: Option<&Self>,
    ) -> Result<(Self, MigrationRunnerEvent), String> {
        registry.validate()?;
        let run_id = run_id.into();
        let owner = owner.into();
        if !bounded(&run_id, MAX_MIGRATION_RUN_ID)
            || !bounded(&owner, MAX_MIGRATION_RUN_ID)
            || fence_token == 0
            || now_unix_ms == 0
            || lease_expires_at_unix_ms <= now_unix_ms
        {
            return Err("migration_runner_lease_invalid".to_owned());
        }
        if let Some(existing) = existing {
            existing.validate()?;
            if existing.status == MigrationRunnerStatus::Active
                && existing.lease_expires_at_unix_ms > now_unix_ms
            {
                return Err("migration_runner_concurrent".to_owned());
            }
            if fence_token <= existing.fence_token {
                return Err("migration_runner_stale_fence".to_owned());
            }
            if existing.status == MigrationRunnerStatus::Quarantined {
                return Err("migration_failure_quarantined".to_owned());
            }
        }
        let checkpoint = MigrationCheckpoint::initial(
            registry,
            &registry
                .ordered_steps()
                .first()
                .ok_or_else(|| "migration_registry_header_invalid".to_owned())?
                .step_id,
        )?;
        let mut state = Self {
            schema: MIGRATION_RUNNER_SCHEMA.to_owned(),
            version: MIGRATION_RUNNER_VERSION,
            run_id,
            registry_digest: registry.registry_digest.clone(),
            owner,
            fence_token,
            lease_expires_at_unix_ms,
            status: MigrationRunnerStatus::Active,
            checkpoint,
            last_event_sequence: 1,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state.validate()?;
        let event = MigrationRunnerEvent::new(
            MigrationRunnerEventKind::Started,
            &state,
            1,
            state.checkpoint.checkpoint_digest.clone(),
            None,
        )?;
        Ok((state, event))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MIGRATION_RUNNER_SCHEMA
            || self.version != MIGRATION_RUNNER_VERSION
            || !bounded(&self.run_id, MAX_MIGRATION_RUN_ID)
            || !bounded(&self.owner, MAX_MIGRATION_RUN_ID)
            || self.fence_token == 0
            || self.lease_expires_at_unix_ms == 0
            || self.last_event_sequence == 0
        {
            return Err("migration_runner_state_invalid".to_owned());
        }
        validate_digest(&self.registry_digest, "migration_runner_registry_digest")?;
        self.checkpoint.validate()?;
        validate_digest(&self.state_digest, "migration_runner_state_digest")?;
        if self.state_digest != self.digest() {
            return Err("migration_runner_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn resume(&self, token: &MigrationResumeToken, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        token.validate()?;
        if now_unix_ms == 0 || self.lease_expires_at_unix_ms <= now_unix_ms {
            return Err("migration_lease_expired".to_owned());
        }
        if self.status != MigrationRunnerStatus::Active {
            return Err(match self.status {
                MigrationRunnerStatus::Quarantined => "migration_failure_quarantined",
                MigrationRunnerStatus::Completed => "migration_already_completed",
                MigrationRunnerStatus::Active => "migration_runner_state_invalid",
            }
            .to_owned());
        }
        if token.run_id != self.run_id
            || token.registry_digest != self.registry_digest
            || token.owner != self.owner
            || token.fence_token != self.fence_token
            || token.event_sequence != self.last_event_sequence
            || token.checkpoint_digest != self.checkpoint.checkpoint_digest
        {
            return Err("migration_resume_token_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn resume_token(&self) -> Result<MigrationResumeToken, String> {
        self.validate()?;
        if self.status != MigrationRunnerStatus::Active {
            return Err("migration_resume_not_active".to_owned());
        }
        MigrationResumeToken::from_state(self)
    }

    pub fn apply_step(
        &mut self,
        registry: &MigrationRegistry,
        plan: &MigrationBatchPlan,
        now_unix_ms: u64,
    ) -> Result<MigrationRunnerEvent, String> {
        self.validate()?;
        registry.validate()?;
        if now_unix_ms == 0 || self.lease_expires_at_unix_ms <= now_unix_ms {
            return Err("migration_lease_expired".to_owned());
        }
        if self.status != MigrationRunnerStatus::Active {
            return Err(match self.status {
                MigrationRunnerStatus::Quarantined => "migration_failure_quarantined",
                MigrationRunnerStatus::Completed => "migration_already_completed",
                MigrationRunnerStatus::Active => "migration_runner_state_invalid",
            }
            .to_owned());
        }
        if self.registry_digest != registry.registry_digest {
            return Err("migration_registry_checksum_drift".to_owned());
        }
        let next_checkpoint = self.checkpoint.advance(registry, plan)?;
        self.checkpoint = next_checkpoint;
        self.last_event_sequence = self
            .last_event_sequence
            .checked_add(1)
            .ok_or_else(|| "migration_runner_event_sequence_overflow".to_owned())?;
        self.state_digest = self.digest();
        self.validate()?;
        MigrationRunnerEvent::new(
            MigrationRunnerEventKind::Step,
            self,
            self.last_event_sequence,
            self.checkpoint.checkpoint_digest.clone(),
            None,
        )
    }

    pub fn block(&mut self, reason: impl Into<String>) -> Result<MigrationRunnerEvent, String> {
        self.validate()?;
        let reason = reason.into();
        if !bounded(&reason, MAX_MIGRATION_REASON) || reason.contains('\n') {
            return Err("migration_block_reason_invalid".to_owned());
        }
        if self.status != MigrationRunnerStatus::Active {
            return Err("migration_failure_quarantined".to_owned());
        }
        self.status = MigrationRunnerStatus::Quarantined;
        self.last_event_sequence = self
            .last_event_sequence
            .checked_add(1)
            .ok_or_else(|| "migration_runner_event_sequence_overflow".to_owned())?;
        self.state_digest = self.digest();
        self.validate()?;
        MigrationRunnerEvent::new(
            MigrationRunnerEventKind::Blocked,
            self,
            self.last_event_sequence,
            self.checkpoint.checkpoint_digest.clone(),
            Some(reason),
        )
    }

    pub fn complete(&mut self, now_unix_ms: u64) -> Result<MigrationRunnerEvent, String> {
        self.validate()?;
        if now_unix_ms == 0 || self.lease_expires_at_unix_ms <= now_unix_ms {
            return Err("migration_lease_expired".to_owned());
        }
        if self.status != MigrationRunnerStatus::Active {
            return Err("migration_completion_not_active".to_owned());
        }
        if self.checkpoint.phase != MigrationPrimitivePhase::Contract
            || !self.checkpoint.phase_complete
            || !self.checkpoint.verified
        {
            return Err("migration_contract_not_verified".to_owned());
        }
        self.status = MigrationRunnerStatus::Completed;
        self.last_event_sequence = self
            .last_event_sequence
            .checked_add(1)
            .ok_or_else(|| "migration_runner_event_sequence_overflow".to_owned())?;
        self.state_digest = self.digest();
        self.validate()?;
        MigrationRunnerEvent::new(
            MigrationRunnerEventKind::Completed,
            self,
            self.last_event_sequence,
            self.checkpoint.checkpoint_digest.clone(),
            None,
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "registry_digest": self.registry_digest,
            "owner": self.owner,
            "fence_token": self.fence_token,
            "lease_expires_at_unix_ms": self.lease_expires_at_unix_ms,
            "status": self.status,
            "checkpoint": self.checkpoint,
            "last_event_sequence": self.last_event_sequence,
        }))
    }
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
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
