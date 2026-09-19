//! Orchestrated rollout lifecycle, retention and post-deploy verification contract.
//!
//! This is the bounded state machine used by a future orchestrator adapter. It records pause,
//! drain, promote, rollback and retirement decisions, but never changes traffic, kills a worker
//! or deletes an old root on its own.

use crate::{
    json_digest, OrchestratedRolloutAction, OrchestratedRolloutPlan, RevisionDrain,
    RevisionDrainStatus, SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const ROLLOUT_LIFECYCLE_SCHEMA: &str = "kiana.rollout-lifecycle.v1";
pub const ROLLOUT_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutLifecyclePhase {
    Planned,
    Running,
    Paused,
    Draining,
    Promoted,
    RolledBack,
    Retired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RolloutHealthWindow {
    pub started_at_unix_ms: u64,
    pub ended_at_unix_ms: u64,
    pub sample_count: u64,
    pub failure_count: u64,
    pub max_failure_count: u64,
    pub passed: bool,
    pub evidence_digest: String,
}

impl RolloutHealthWindow {
    pub fn new(
        started_at_unix_ms: u64,
        ended_at_unix_ms: u64,
        sample_count: u64,
        failure_count: u64,
        max_failure_count: u64,
    ) -> Result<Self, String> {
        let mut window = Self {
            started_at_unix_ms,
            ended_at_unix_ms,
            sample_count,
            failure_count,
            max_failure_count,
            passed: failure_count <= max_failure_count,
            evidence_digest: String::new(),
        };
        window.evidence_digest = window.digest();
        window.validate()?;
        Ok(window)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.started_at_unix_ms == 0
            || self.ended_at_unix_ms <= self.started_at_unix_ms
            || self.sample_count == 0
            || self.failure_count > self.sample_count
            || self.max_failure_count > self.sample_count
        {
            return Err("rollout_health_window_invalid".to_owned());
        }
        if self.passed != (self.failure_count <= self.max_failure_count) {
            return Err("rollout_health_window_pass_mismatch".to_owned());
        }
        validate_digest(&self.evidence_digest, "rollout_health_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("rollout_health_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "started_at_unix_ms": self.started_at_unix_ms,
            "ended_at_unix_ms": self.ended_at_unix_ms,
            "sample_count": self.sample_count,
            "failure_count": self.failure_count,
            "max_failure_count": self.max_failure_count,
            "passed": self.passed,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostDeployVerification {
    pub target_revision_id: String,
    pub readiness_passed: bool,
    pub liveness_passed: bool,
    pub health_window_digest: String,
    pub receipt_digest: String,
    pub verification_digest: String,
}

impl PostDeployVerification {
    pub fn new(
        target_revision_id: impl Into<String>,
        readiness_passed: bool,
        liveness_passed: bool,
        health_window: &RolloutHealthWindow,
        receipt_digest: impl Into<String>,
    ) -> Result<Self, String> {
        health_window.validate()?;
        let mut verification = Self {
            target_revision_id: target_revision_id.into(),
            readiness_passed,
            liveness_passed,
            health_window_digest: health_window.evidence_digest.clone(),
            receipt_digest: receipt_digest.into(),
            verification_digest: String::new(),
        };
        verification.verification_digest = verification.digest();
        verification.validate()?;
        Ok(verification)
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.target_revision_id, "rollout_verification_target")?;
        validate_digest(
            &self.health_window_digest,
            "rollout_verification_health_digest",
        )?;
        validate_digest(&self.receipt_digest, "rollout_verification_receipt_digest")?;
        validate_digest(&self.verification_digest, "rollout_verification_digest")?;
        if self.verification_digest != self.digest() {
            return Err("rollout_verification_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn passed(&self, health_window: &RolloutHealthWindow) -> bool {
        self.validate().is_ok()
            && health_window.validate().is_ok()
            && self.health_window_digest == health_window.evidence_digest
            && self.readiness_passed
            && self.liveness_passed
            && health_window.passed
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "target_revision_id": self.target_revision_id,
            "readiness_passed": self.readiness_passed,
            "liveness_passed": self.liveness_passed,
            "health_window_digest": self.health_window_digest,
            "receipt_digest": self.receipt_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OldRevisionRetention {
    pub old_root_digest: String,
    pub retain_until_unix_ms: u64,
    pub old_root_retained: bool,
    pub deletion_eligible: bool,
    pub retention_digest: String,
}

impl OldRevisionRetention {
    pub fn new(
        old_root_digest: impl Into<String>,
        retain_until_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut retention = Self {
            old_root_digest: old_root_digest.into(),
            retain_until_unix_ms,
            old_root_retained: true,
            deletion_eligible: false,
            retention_digest: String::new(),
        };
        retention.retention_digest = retention.digest();
        retention.validate()?;
        Ok(retention)
    }

    fn validate(&self) -> Result<(), String> {
        validate_digest(&self.old_root_digest, "rollout_old_root_digest")?;
        if self.retain_until_unix_ms == 0 || self.deletion_eligible && !self.old_root_retained {
            return Err("rollout_retention_invalid".to_owned());
        }
        validate_digest(&self.retention_digest, "rollout_retention_digest")?;
        if self.retention_digest != self.digest() {
            return Err("rollout_retention_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "old_root_digest": self.old_root_digest,
            "retain_until_unix_ms": self.retain_until_unix_ms,
            "old_root_retained": self.old_root_retained,
            "deletion_eligible": self.deletion_eligible,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestratedRolloutState {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan: OrchestratedRolloutPlan,
    pub phase: RolloutLifecyclePhase,
    pub drain: RevisionDrain,
    pub health_window: Option<RolloutHealthWindow>,
    pub verification: Option<PostDeployVerification>,
    pub retention: OldRevisionRetention,
    pub pause_reason: Option<String>,
    pub phase_sequence: u64,
    pub state_digest: String,
}

impl OrchestratedRolloutState {
    pub fn new(
        plan: OrchestratedRolloutPlan,
        now_unix_ms: u64,
        old_root_digest: impl Into<String>,
        retain_until_unix_ms: u64,
    ) -> Result<Self, String> {
        plan.validate()?;
        if now_unix_ms == 0 || retain_until_unix_ms <= now_unix_ms {
            return Err("rollout_state_time_invalid".to_owned());
        }
        let old_pin = plan
            .routing
            .route(&plan.rollback_revision_id)
            .ok_or_else(|| "rollout_rollback_route_missing".to_owned())?
            .pin
            .clone();
        let mut state = Self {
            schema: ROLLOUT_LIFECYCLE_SCHEMA.to_owned(),
            version: ROLLOUT_LIFECYCLE_VERSION,
            plan,
            phase: RolloutLifecyclePhase::Planned,
            drain: RevisionDrain::new(old_pin)?,
            health_window: None,
            verification: None,
            retention: OldRevisionRetention::new(old_root_digest, retain_until_unix_ms)?,
            pause_reason: None,
            phase_sequence: 1,
            state_digest: String::new(),
        };
        state.state_digest = state.digest();
        state.validate()?;
        Ok(state)
    }

    pub fn start(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if self.phase != RolloutLifecyclePhase::Planned {
            return Err("rollout_start_phase_invalid".to_owned());
        }
        if now_unix_ms == 0 || now_unix_ms > self.plan.progress_deadline_unix_ms {
            return Err("rollout_progress_deadline_exceeded".to_owned());
        }
        self.phase = RolloutLifecyclePhase::Running;
        self.bump()
    }

    pub fn pause(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.validate()?;
        if !matches!(
            self.phase,
            RolloutLifecyclePhase::Running | RolloutLifecyclePhase::Draining
        ) {
            return Err("rollout_pause_phase_invalid".to_owned());
        }
        let reason = reason.into();
        bounded(&reason, "rollout_pause_reason")?;
        self.phase = RolloutLifecyclePhase::Paused;
        self.pause_reason = Some(reason);
        self.bump()
    }

    pub fn resume(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if self.phase != RolloutLifecyclePhase::Paused {
            return Err("rollout_resume_phase_invalid".to_owned());
        }
        let decision = self
            .plan
            .decide(OrchestratedRolloutAction::Resume, now_unix_ms, None)?;
        if !decision.allowed {
            return Err(decision.reason);
        }
        self.phase = RolloutLifecyclePhase::Running;
        self.bump()
    }

    pub fn begin_drain(
        &mut self,
        now_unix_ms: u64,
        deadline_unix_ms: u64,
        replacement_ready: bool,
    ) -> Result<(), String> {
        self.validate()?;
        if self.phase != RolloutLifecyclePhase::Running {
            return Err("rollout_drain_phase_invalid".to_owned());
        }
        if !replacement_ready {
            return Err("rollout_replacement_not_ready".to_owned());
        }
        self.drain
            .begin(now_unix_ms, deadline_unix_ms, replacement_ready)?;
        self.phase = RolloutLifecyclePhase::Draining;
        self.bump()
    }

    pub fn observe_drain(
        &mut self,
        active_run_count: u32,
        active_writer_count: u32,
    ) -> Result<(), String> {
        self.validate()?;
        if self.phase != RolloutLifecyclePhase::Draining {
            return Err("rollout_drain_phase_invalid".to_owned());
        }
        self.drain.observe(active_run_count, active_writer_count)?;
        self.bump()
    }

    pub fn promote(
        &mut self,
        now_unix_ms: u64,
        health_window: RolloutHealthWindow,
        verification: PostDeployVerification,
    ) -> Result<(), String> {
        self.validate()?;
        if self.phase != RolloutLifecyclePhase::Draining {
            return Err("rollout_promote_phase_invalid".to_owned());
        }
        let decision = self
            .plan
            .decide(OrchestratedRolloutAction::Promote, now_unix_ms, None)?;
        if !decision.allowed {
            return Err(decision.reason);
        }
        health_window.validate()?;
        verification.validate()?;
        if verification.target_revision_id != self.plan.target_revision_id
            || !verification.passed(&health_window)
        {
            return Err("rollout_post_deploy_verification_failed".to_owned());
        }
        if self.drain.status != RevisionDrainStatus::Draining
            || !self.drain.replacement_ready
            || self.drain.active_run_count > 0
            || self.drain.active_writer_count > 0
        {
            return Err("rollout_drain_not_empty".to_owned());
        }
        self.health_window = Some(health_window);
        self.verification = Some(verification);
        self.phase = RolloutLifecyclePhase::Promoted;
        self.bump()
    }

    pub fn rollback(&mut self, now_unix_ms: u64, reason: impl Into<String>) -> Result<(), String> {
        self.validate()?;
        if matches!(
            self.phase,
            RolloutLifecyclePhase::RolledBack | RolloutLifecyclePhase::Retired
        ) {
            return Err("rollout_rollback_phase_invalid".to_owned());
        }
        let reason = reason.into();
        bounded(&reason, "rollout_rollback_reason")?;
        let decision = self.plan.decide(
            OrchestratedRolloutAction::Rollback,
            now_unix_ms,
            Some(&reason),
        )?;
        if !decision.allowed {
            return Err(decision.reason);
        }
        if !self.retention.old_root_retained {
            return Err("rollout_old_root_not_retained".to_owned());
        }
        self.phase = RolloutLifecyclePhase::RolledBack;
        self.pause_reason = Some(reason);
        self.bump()
    }

    pub fn retire_old_revision(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if self.phase != RolloutLifecyclePhase::Promoted {
            return Err("rollout_retire_phase_invalid".to_owned());
        }
        if now_unix_ms < self.retention.retain_until_unix_ms {
            return Err("rollout_retention_window_open".to_owned());
        }
        if self
            .verification
            .as_ref()
            .zip(self.health_window.as_ref())
            .is_none_or(|(verification, health)| !verification.passed(health))
        {
            return Err("rollout_post_deploy_verification_missing".to_owned());
        }
        if self.drain.status != RevisionDrainStatus::Draining
            || self.drain.active_run_count > 0
            || self.drain.active_writer_count > 0
        {
            return Err("rollout_old_revision_still_active".to_owned());
        }
        self.drain.retire(now_unix_ms)?;
        self.retention.deletion_eligible = true;
        self.retention.retention_digest = self.retention.digest();
        self.phase = RolloutLifecyclePhase::Retired;
        self.bump()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROLLOUT_LIFECYCLE_SCHEMA
            || self.version != ROLLOUT_LIFECYCLE_VERSION
            || self.phase_sequence == 0
        {
            return Err("rollout_state_invalid".to_owned());
        }
        self.plan.validate()?;
        self.drain.validate()?;
        self.retention.validate()?;
        if let Some(health) = &self.health_window {
            health.validate()?;
        }
        if let Some(verification) = &self.verification {
            verification.validate()?;
        }
        if let Some(reason) = &self.pause_reason {
            bounded(reason, "rollout_pause_reason")?;
        }
        if matches!(
            self.phase,
            RolloutLifecyclePhase::Promoted | RolloutLifecyclePhase::Retired
        ) && (self.health_window.is_none() || self.verification.is_none())
        {
            return Err("rollout_post_deploy_verification_missing".to_owned());
        }
        if self.phase == RolloutLifecyclePhase::Retired
            && (self.drain.status != RevisionDrainStatus::Retired
                || !self.retention.deletion_eligible)
        {
            return Err("rollout_retirement_evidence_missing".to_owned());
        }
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
            "plan": self.plan,
            "phase": self.phase,
            "drain": self.drain,
            "health_window": self.health_window,
            "verification": self.verification,
            "retention": self.retention,
            "pause_reason": self.pause_reason,
            "phase_sequence": self.phase_sequence,
        }))
    }

    fn bump(&mut self) -> Result<(), String> {
        self.phase_sequence = self
            .phase_sequence
            .checked_add(1)
            .ok_or_else(|| "rollout_phase_sequence_overflow".to_owned())?;
        self.state_digest = self.digest();
        self.validate()
    }
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 512 || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
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
