//! Credential/config recovery projection and explicit re-admission boundary.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CREDENTIAL_RECOVERY_SCHEMA: &str = "kiana.credential-recovery-projection.v1";
pub const CREDENTIAL_RECOVERY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

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
    LeaseMissing,
    CredentialRefreshFailed,
    ResultUnknown,
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
