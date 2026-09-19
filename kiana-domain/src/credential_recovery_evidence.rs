//! Credential/config recovery projection and explicit re-admission boundary.

use crate::{json_digest, validate_runtime_event, RequestId, RuntimeEvent, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CREDENTIAL_RECOVERY_SCHEMA: &str = "kiana.credential-recovery-projection.v1";
pub const CREDENTIAL_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CREDENTIAL_RECOVERY_EVENT_SCHEMA: &str = "kiana.credential-recovery-event.v1";
pub const CREDENTIAL_RECOVERY_EVENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const CREDENTIAL_RECOVERY_EVENT_KIND: &str = "recovery.credential";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialRecoveryStatus {
    Paused,
    ReAdmissionRequired,
    Ready,
    Blocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialRecoveryBlocker {
    UnknownSchema,
    StaleAuthorityEpoch,
    StaleConfigRevision,
    StaleCredentialRevision,
    StaleCredentialGeneration,
    LeaseMissing,
    CredentialRefreshFailed,
    ResultUnknown,
    AuditBindingInvalid,
    RedactionBindingInvalid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialLeaseProjectionState {
    Active,
    Expired,
    Revoked,
    Missing,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialRecoveryEventKind {
    RestartDetected,
    CredentialRotated,
    CredentialRevoked,
    CredentialRefreshFailed,
    ResultUnknown,
    ReAdmissionAuthorized,
}

/// A server-derived, redacted recovery fact stored inside a versioned RuntimeEvent.
///
/// The fact carries references and digests only.  It never carries a token, refresh material or
/// a provider response.  A recovery projector must validate every fact before using it as
/// authority; a syntactically valid event is not itself permission to resume a run.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRecoveryFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_ref: String,
    pub principal_digest: String,
    pub assignment_digest: String,
    pub config_snapshot_digest: String,
    pub credential_ref_digest: String,
    pub audit_projection_digest: String,
    pub redaction_profile_digest: String,
    pub generation: u64,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub credential_revision: String,
    pub lease_state: CredentialLeaseProjectionState,
    pub event: CredentialRecoveryEventKind,
    pub credential_refresh_succeeded: bool,
    pub result_unknown: bool,
    pub explicit_re_admission: bool,
    pub fact_digest: String,
}

impl CredentialRecoveryFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_ref: impl Into<String>,
        principal_digest: impl Into<String>,
        assignment_digest: impl Into<String>,
        config_snapshot_digest: impl Into<String>,
        credential_ref_digest: impl Into<String>,
        audit_projection_digest: impl Into<String>,
        redaction_profile_digest: impl Into<String>,
        generation: u64,
        authority_epoch: u64,
        config_revision: impl Into<String>,
        credential_revision: impl Into<String>,
        lease_state: CredentialLeaseProjectionState,
        event: CredentialRecoveryEventKind,
    ) -> Result<Self, String> {
        let (credential_refresh_succeeded, result_unknown, explicit_re_admission) = match event {
            CredentialRecoveryEventKind::CredentialRotated => (true, false, false),
            CredentialRecoveryEventKind::CredentialRefreshFailed => (false, false, false),
            CredentialRecoveryEventKind::ResultUnknown => (false, true, false),
            CredentialRecoveryEventKind::ReAdmissionAuthorized => (true, false, true),
            CredentialRecoveryEventKind::CredentialRevoked
            | CredentialRecoveryEventKind::RestartDetected => (false, false, false),
        };
        let mut fact = Self {
            schema: CREDENTIAL_RECOVERY_EVENT_SCHEMA.to_owned(),
            version: CREDENTIAL_RECOVERY_EVENT_VERSION,
            run_ref: run_ref.into(),
            principal_digest: principal_digest.into(),
            assignment_digest: assignment_digest.into(),
            config_snapshot_digest: config_snapshot_digest.into(),
            credential_ref_digest: credential_ref_digest.into(),
            audit_projection_digest: audit_projection_digest.into(),
            redaction_profile_digest: redaction_profile_digest.into(),
            generation,
            authority_epoch,
            config_revision: config_revision.into(),
            credential_revision: credential_revision.into(),
            lease_state,
            event,
            credential_refresh_succeeded,
            result_unknown,
            explicit_re_admission,
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CREDENTIAL_RECOVERY_EVENT_SCHEMA
            || self.version != CREDENTIAL_RECOVERY_EVENT_VERSION
        {
            return Err("credential_recovery_event_schema_unknown".to_owned());
        }
        bounded(&self.run_ref, "credential_recovery_event_run_ref", 256)?;
        for (value, field) in [
            (
                &self.principal_digest,
                "credential_recovery_event_principal_digest",
            ),
            (
                &self.assignment_digest,
                "credential_recovery_event_assignment_digest",
            ),
            (
                &self.config_snapshot_digest,
                "credential_recovery_event_config_snapshot_digest",
            ),
            (
                &self.credential_ref_digest,
                "credential_recovery_event_credential_ref_digest",
            ),
            (
                &self.audit_projection_digest,
                "credential_recovery_event_audit_projection_digest",
            ),
            (
                &self.redaction_profile_digest,
                "credential_recovery_event_redaction_profile_digest",
            ),
        ] {
            digest(value, field)?;
        }
        if self.generation == 0 || self.authority_epoch == 0 {
            return Err("credential_recovery_event_epoch_invalid".to_owned());
        }
        bounded(
            &self.config_revision,
            "credential_recovery_event_config_revision",
            256,
        )?;
        bounded(
            &self.credential_revision,
            "credential_recovery_event_credential_revision",
            256,
        )?;
        match self.event {
            CredentialRecoveryEventKind::CredentialRotated
                if !self.credential_refresh_succeeded
                    || self.lease_state != CredentialLeaseProjectionState::Active =>
            {
                return Err("credential_recovery_rotation_state_invalid".to_owned())
            }
            CredentialRecoveryEventKind::CredentialRefreshFailed
                if self.credential_refresh_succeeded =>
            {
                return Err("credential_recovery_refresh_success_conflict".to_owned())
            }
            CredentialRecoveryEventKind::CredentialRevoked
                if self.lease_state != CredentialLeaseProjectionState::Revoked =>
            {
                return Err("credential_recovery_revoke_state_invalid".to_owned())
            }
            CredentialRecoveryEventKind::ResultUnknown if !self.result_unknown => {
                return Err("credential_recovery_unknown_state_invalid".to_owned())
            }
            CredentialRecoveryEventKind::ReAdmissionAuthorized
                if !self.explicit_re_admission
                    || !self.credential_refresh_succeeded
                    || self.lease_state != CredentialLeaseProjectionState::Active =>
            {
                return Err("credential_recovery_admission_state_invalid".to_owned())
            }
            CredentialRecoveryEventKind::RestartDetected
                if self.explicit_re_admission || self.result_unknown =>
            {
                return Err("credential_recovery_restart_state_invalid".to_owned())
            }
            _ => {}
        }
        if self.fact_digest != self.digest() {
            return Err("credential_recovery_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_ref": self.run_ref,
            "principal_digest": self.principal_digest,
            "assignment_digest": self.assignment_digest,
            "config_snapshot_digest": self.config_snapshot_digest,
            "credential_ref_digest": self.credential_ref_digest,
            "audit_projection_digest": self.audit_projection_digest,
            "redaction_profile_digest": self.redaction_profile_digest,
            "generation": self.generation,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "credential_revision": self.credential_revision,
            "lease_state": self.lease_state,
            "event": self.event,
            "credential_refresh_succeeded": self.credential_refresh_succeeded,
            "result_unknown": self.result_unknown,
            "explicit_re_admission": self.explicit_re_admission,
        }))
    }

    pub fn to_runtime_event(
        &self,
        request_id: RequestId,
        sequence: u64,
    ) -> Result<RuntimeEvent, String> {
        self.validate()?;
        let event = RuntimeEvent::new(
            request_id,
            sequence,
            CREDENTIAL_RECOVERY_EVENT_KIND,
            json!({"run_id": self.run_ref, "recovery": self}),
        )
        .map_err(|error| error.to_string())?;
        validate_runtime_event(&event)?;
        Ok(event)
    }

    pub fn from_runtime_event(event: &RuntimeEvent) -> Result<Self, String> {
        if event.kind != CREDENTIAL_RECOVERY_EVENT_KIND {
            return Err("credential_recovery_event_kind_invalid".to_owned());
        }
        validate_runtime_event(event)?;
        let run_ref = event
            .data
            .get("run_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "credential_recovery_event_run_missing".to_owned())?;
        let fact: Self = serde_json::from_value(
            event
                .data
                .get("recovery")
                .cloned()
                .ok_or_else(|| "credential_recovery_event_payload_missing".to_owned())?,
        )
        .map_err(|_| "credential_recovery_event_payload_invalid".to_owned())?;
        fact.validate()?;
        if fact.run_ref != run_ref {
            return Err("credential_recovery_event_run_mismatch".to_owned());
        }
        Ok(fact)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialRecoveryProjection {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_ref: String,
    pub principal_digest: String,
    pub assignment_digest: String,
    pub config_snapshot_digest: String,
    pub credential_ref_digest: String,
    pub audit_projection_digest: String,
    pub redaction_profile_digest: String,
    pub generation: u64,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub credential_revision: String,
    pub lease_state: CredentialLeaseProjectionState,
    pub blockers: Vec<CredentialRecoveryBlocker>,
    pub status: CredentialRecoveryStatus,
    pub explicit_re_admission: bool,
    pub resume_authorized: bool,
    pub result_unknown: bool,
    pub projection_digest: String,
}

impl CredentialRecoveryProjection {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_ref: impl Into<String>,
        principal_digest: impl Into<String>,
        assignment_digest: impl Into<String>,
        config_snapshot_digest: impl Into<String>,
        credential_ref_digest: impl Into<String>,
        audit_projection_digest: impl Into<String>,
        redaction_profile_digest: impl Into<String>,
        generation: u64,
        authority_epoch: u64,
        config_revision: impl Into<String>,
        credential_revision: impl Into<String>,
        lease_state: CredentialLeaseProjectionState,
        blockers: Vec<CredentialRecoveryBlocker>,
        status: CredentialRecoveryStatus,
        explicit_re_admission: bool,
        resume_authorized: bool,
        result_unknown: bool,
    ) -> Result<Self, String> {
        let mut projection = Self {
            schema: CREDENTIAL_RECOVERY_SCHEMA.to_owned(),
            version: CREDENTIAL_RECOVERY_VERSION,
            run_ref: run_ref.into(),
            principal_digest: principal_digest.into(),
            assignment_digest: assignment_digest.into(),
            config_snapshot_digest: config_snapshot_digest.into(),
            credential_ref_digest: credential_ref_digest.into(),
            audit_projection_digest: audit_projection_digest.into(),
            redaction_profile_digest: redaction_profile_digest.into(),
            generation,
            authority_epoch,
            config_revision: config_revision.into(),
            credential_revision: credential_revision.into(),
            lease_state,
            blockers,
            status,
            explicit_re_admission,
            resume_authorized,
            result_unknown,
            projection_digest: String::new(),
        };
        projection.projection_digest = projection.digest();
        projection.validate()?;
        Ok(projection)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CREDENTIAL_RECOVERY_SCHEMA || self.version != CREDENTIAL_RECOVERY_VERSION
        {
            return Err("credential_recovery_projection_header_invalid".to_owned());
        }
        bounded(&self.run_ref, "credential_recovery_run_ref", 256)?;
        for (value, field) in [
            (
                &self.principal_digest,
                "credential_recovery_principal_digest",
            ),
            (
                &self.assignment_digest,
                "credential_recovery_assignment_digest",
            ),
            (
                &self.config_snapshot_digest,
                "credential_recovery_config_snapshot_digest",
            ),
            (
                &self.credential_ref_digest,
                "credential_recovery_credential_ref_digest",
            ),
            (
                &self.audit_projection_digest,
                "credential_recovery_audit_projection_digest",
            ),
            (
                &self.redaction_profile_digest,
                "credential_recovery_redaction_profile_digest",
            ),
        ] {
            digest(value, field)?;
        }
        if self.generation == 0 || self.authority_epoch == 0 {
            return Err("credential_recovery_epoch_invalid".to_owned());
        }
        bounded(
            &self.config_revision,
            "credential_recovery_config_revision",
            256,
        )?;
        bounded(
            &self.credential_revision,
            "credential_recovery_credential_revision",
            256,
        )?;
        if self.blockers.len() > 16 {
            return Err("credential_recovery_blocker_limit".to_owned());
        }
        let mut blockers = BTreeSet::new();
        for blocker in &self.blockers {
            if !blockers.insert(*blocker) {
                return Err("credential_recovery_blocker_duplicate".to_owned());
            }
        }
        if self.resume_authorized
            && (self.status != CredentialRecoveryStatus::Ready
                || !self.explicit_re_admission
                || self.result_unknown
                || !self.blockers.is_empty()
                || self.lease_state != CredentialLeaseProjectionState::Active)
        {
            return Err("credential_recovery_auto_resume_forbidden".to_owned());
        }
        if self.status == CredentialRecoveryStatus::Ready
            && (!self.explicit_re_admission
                || !self.resume_authorized
                || self.result_unknown
                || !self.blockers.is_empty()
                || self.lease_state != CredentialLeaseProjectionState::Active)
        {
            return Err("credential_recovery_ready_admission_incomplete".to_owned());
        }
        if self.status != CredentialRecoveryStatus::Ready && self.resume_authorized {
            return Err("credential_recovery_resume_status_invalid".to_owned());
        }
        if self.result_unknown
            && !self
                .blockers
                .contains(&CredentialRecoveryBlocker::ResultUnknown)
        {
            return Err("credential_recovery_unknown_reason_missing".to_owned());
        }
        if self.projection_digest != self.digest() {
            return Err("credential_recovery_projection_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_ref": self.run_ref,
            "principal_digest": self.principal_digest,
            "assignment_digest": self.assignment_digest,
            "config_snapshot_digest": self.config_snapshot_digest,
            "credential_ref_digest": self.credential_ref_digest,
            "audit_projection_digest": self.audit_projection_digest,
            "redaction_profile_digest": self.redaction_profile_digest,
            "generation": self.generation,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "credential_revision": self.credential_revision,
            "lease_state": self.lease_state,
            "blockers": self.blockers,
            "status": self.status,
            "explicit_re_admission": self.explicit_re_admission,
            "resume_authorized": self.resume_authorized,
            "result_unknown": self.result_unknown,
        }))
    }

    /// Convert a validated recovery fact into the paused, post-restart default state.
    ///
    /// A previously persisted `Ready` state is intentionally not copied into this projection;
    /// only a later `ReAdmissionAuthorized` fact may set `resume_authorized=true`.
    pub fn paused_from_fact(fact: &CredentialRecoveryFact) -> Result<Self, String> {
        fact.validate()?;
        Self::new(
            fact.run_ref.clone(),
            fact.principal_digest.clone(),
            fact.assignment_digest.clone(),
            fact.config_snapshot_digest.clone(),
            fact.credential_ref_digest.clone(),
            fact.audit_projection_digest.clone(),
            fact.redaction_profile_digest.clone(),
            fact.generation,
            fact.authority_epoch,
            fact.config_revision.clone(),
            fact.credential_revision.clone(),
            fact.lease_state,
            Vec::new(),
            CredentialRecoveryStatus::ReAdmissionRequired,
            false,
            false,
            false,
        )
    }

    /// Apply an explicit, server-derived re-admission fact.  This is deliberately separate from
    /// replay so a recovery scan can never infer authorization from an old `Ready` projection.
    pub fn explicit_re_admit(&self, fact: &CredentialRecoveryFact) -> Result<Self, String> {
        self.validate()?;
        fact.validate()?;
        if fact.event != CredentialRecoveryEventKind::ReAdmissionAuthorized {
            return Err("credential_recovery_re_admission_event_required".to_owned());
        }
        if !self.blockers.is_empty() || self.result_unknown {
            return Err("credential_recovery_reconciliation_required".to_owned());
        }
        if self.run_ref != fact.run_ref
            || self.principal_digest != fact.principal_digest
            || self.assignment_digest != fact.assignment_digest
            || self.config_snapshot_digest != fact.config_snapshot_digest
            || self.credential_ref_digest != fact.credential_ref_digest
            || self.audit_projection_digest != fact.audit_projection_digest
            || self.redaction_profile_digest != fact.redaction_profile_digest
            || self.generation != fact.generation
            || self.authority_epoch != fact.authority_epoch
            || self.config_revision != fact.config_revision
            || self.credential_revision != fact.credential_revision
            || fact.lease_state != CredentialLeaseProjectionState::Active
        {
            return Err("credential_recovery_admission_binding_mismatch".to_owned());
        }
        let mut admitted = self.clone();
        admitted.lease_state = CredentialLeaseProjectionState::Active;
        admitted.status = CredentialRecoveryStatus::Ready;
        admitted.explicit_re_admission = true;
        admitted.resume_authorized = true;
        admitted.projection_digest = admitted.digest();
        admitted.validate()?;
        Ok(admitted)
    }
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\n', '\r']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
