//! Deterministic deployment startup ordering and readiness gate.
//!
//! The coordinator consumes server-owned facts from adapters. It does not open a store, acquire
//! a lease, run a migration, rebuild a projector or probe capacity. A missing, stale or unknown
//! fact keeps startup blocked; a previous operation is never resumed implicitly.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const DEPLOYMENT_STARTUP_FACT_SCHEMA: &str = "kiana.deployment-startup-fact.v1";
pub const DEPLOYMENT_STARTUP_REQUEST_SCHEMA: &str = "kiana.deployment-startup-request.v1";
pub const DEPLOYMENT_STARTUP_REPORT_SCHEMA: &str = "kiana.deployment-startup-report.v1";
pub const DEPLOYMENT_STARTUP_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_STARTUP_FACTS: usize = 8;
pub const MAX_STARTUP_TEXT_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupStage {
    Manifest,
    Root,
    Trust,
    Lease,
    Store,
    Migration,
    Projector,
    Capacity,
}

impl StartupStage {
    pub const ALL: [Self; MAX_STARTUP_FACTS] = [
        Self::Manifest,
        Self::Root,
        Self::Trust,
        Self::Lease,
        Self::Store,
        Self::Migration,
        Self::Projector,
        Self::Capacity,
    ];

    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Manifest => 1,
            Self::Root => 2,
            Self::Trust => 3,
            Self::Lease => 4,
            Self::Store => 5,
            Self::Migration => 6,
            Self::Projector => 7,
            Self::Capacity => 8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupFactStatus {
    Pending,
    Ready,
    Blocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupResumeMode {
    Fresh,
    ExplicitRecovery,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupCoordinatorStatus {
    Ready,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupStageFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub stage: StartupStage,
    pub status: StartupFactStatus,
    pub source_digest: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub source_cursor: u64,
    pub reason: String,
    pub remediation: String,
    pub fact_digest: String,
}

impl StartupStageFact {
    pub fn new(
        stage: StartupStage,
        status: StartupFactStatus,
        source_digest: impl Into<String>,
        authority_epoch: u64,
        data_epoch: u64,
        generation: u64,
        source_cursor: u64,
        reason: impl Into<String>,
        remediation: impl Into<String>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: DEPLOYMENT_STARTUP_FACT_SCHEMA.to_owned(),
            version: DEPLOYMENT_STARTUP_VERSION,
            stage,
            status,
            source_digest: source_digest.into(),
            authority_epoch,
            data_epoch,
            generation,
            source_cursor,
            reason: reason.into(),
            remediation: remediation.into(),
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_STARTUP_FACT_SCHEMA
            || self.version != DEPLOYMENT_STARTUP_VERSION
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
        {
            return Err("deployment_startup_fact_header_invalid".to_owned());
        }
        valid_digest(&self.source_digest, "deployment_startup_fact_source_digest")?;
        valid_digest(&self.fact_digest, "deployment_startup_fact_digest")?;
        bounded(&self.reason, "deployment_startup_fact_reason")?;
        bounded(&self.remediation, "deployment_startup_fact_remediation")?;
        match self.status {
            StartupFactStatus::Ready if self.reason != "ok" || self.remediation != "none" => {
                return Err("deployment_startup_ready_fact_explanation_invalid".to_owned());
            }
            StartupFactStatus::Pending
            | StartupFactStatus::Blocked
            | StartupFactStatus::Unknown
                if self.reason == "ok" || self.remediation == "none" =>
            {
                return Err("deployment_startup_nonready_fact_explanation_invalid".to_owned());
            }
            _ => {}
        }
        if self.fact_digest != self.digest() {
            return Err("deployment_startup_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "stage": self.stage,
            "status": self.status,
            "source_digest": self.source_digest,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

/// Inputs are adapter-supplied facts. They are intentionally separate from the wire request so a
/// caller cannot omit a source digest by relying on a default value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupCoordinatorInput {
    pub release_manifest_digest: String,
    pub storage_root_digest: String,
    pub project_trust_digest: String,
    pub project_trusted: bool,
    pub lease_digest: String,
    pub store_digest: String,
    pub migration_registry_digest: String,
    pub projector_digest: String,
    pub capacity_digest: String,
    pub config_revision: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub pending_operation: bool,
    pub journal_valid: bool,
    pub journal_digest: String,
    pub resume_mode: StartupResumeMode,
    pub recovery_evidence_digest: Option<String>,
    pub facts: Vec<StartupStageFact>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupCoordinatorRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub release_manifest_digest: String,
    pub storage_root_digest: String,
    pub project_trust_digest: String,
    pub project_trusted: bool,
    pub lease_digest: String,
    pub store_digest: String,
    pub migration_registry_digest: String,
    pub projector_digest: String,
    pub capacity_digest: String,
    pub config_revision: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    pub generation: u64,
    pub pending_operation: bool,
    pub journal_valid: bool,
    pub journal_digest: String,
    pub resume_mode: StartupResumeMode,
    pub recovery_evidence_digest: Option<String>,
    pub facts: Vec<StartupStageFact>,
    pub request_digest: String,
}

impl StartupCoordinatorRequest {
    pub fn new(input: StartupCoordinatorInput) -> Result<Self, String> {
        let mut request = Self {
            schema: DEPLOYMENT_STARTUP_REQUEST_SCHEMA.to_owned(),
            version: DEPLOYMENT_STARTUP_VERSION,
            release_manifest_digest: input.release_manifest_digest,
            storage_root_digest: input.storage_root_digest,
            project_trust_digest: input.project_trust_digest,
            project_trusted: input.project_trusted,
            lease_digest: input.lease_digest,
            store_digest: input.store_digest,
            migration_registry_digest: input.migration_registry_digest,
            projector_digest: input.projector_digest,
            capacity_digest: input.capacity_digest,
            config_revision: input.config_revision,
            authority_epoch: input.authority_epoch,
            data_epoch: input.data_epoch,
            generation: input.generation,
            pending_operation: input.pending_operation,
            journal_valid: input.journal_valid,
            journal_digest: input.journal_digest,
            resume_mode: input.resume_mode,
            recovery_evidence_digest: input.recovery_evidence_digest,
            facts: input.facts,
            request_digest: String::new(),
        };
        request.facts.sort_by_key(|fact| fact.stage.ordinal());
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_STARTUP_REQUEST_SCHEMA
            || self.version != DEPLOYMENT_STARTUP_VERSION
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.generation == 0
            || self.facts.len() > MAX_STARTUP_FACTS
            || self
                .facts
                .windows(2)
                .any(|pair| pair[0].stage.ordinal() >= pair[1].stage.ordinal())
        {
            return Err("deployment_startup_request_header_invalid".to_owned());
        }
        for (digest, field) in [
            (
                &self.release_manifest_digest,
                "deployment_startup_manifest_digest",
            ),
            (
                &self.storage_root_digest,
                "deployment_startup_storage_root_digest",
            ),
            (
                &self.project_trust_digest,
                "deployment_startup_project_trust_digest",
            ),
            (&self.lease_digest, "deployment_startup_lease_digest"),
            (&self.store_digest, "deployment_startup_store_digest"),
            (
                &self.migration_registry_digest,
                "deployment_startup_migration_digest",
            ),
            (
                &self.projector_digest,
                "deployment_startup_projector_digest",
            ),
            (&self.capacity_digest, "deployment_startup_capacity_digest"),
            (&self.config_revision, "deployment_startup_config_revision"),
            (&self.journal_digest, "deployment_startup_journal_digest"),
            (&self.request_digest, "deployment_startup_request_digest"),
        ] {
            valid_digest(digest, field)?;
        }
        if self.pending_operation
            && self.resume_mode == StartupResumeMode::ExplicitRecovery
            && self.recovery_evidence_digest.is_none()
        {
            return Err("deployment_startup_recovery_evidence_required".to_owned());
        }
        match (&self.resume_mode, &self.recovery_evidence_digest) {
            (StartupResumeMode::Fresh, Some(_)) => {
                return Err("deployment_startup_fresh_recovery_evidence_invalid".to_owned())
            }
            (_, Some(digest)) => valid_digest(digest, "deployment_startup_recovery_digest")?,
            _ => {}
        }
        for fact in &self.facts {
            fact.validate()?;
        }
        if self
            .facts
            .iter()
            .map(|fact| fact.stage)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != self.facts.len()
        {
            return Err("deployment_startup_duplicate_stage".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("deployment_startup_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "release_manifest_digest": self.release_manifest_digest,
            "storage_root_digest": self.storage_root_digest,
            "project_trust_digest": self.project_trust_digest,
            "project_trusted": self.project_trusted,
            "lease_digest": self.lease_digest,
            "store_digest": self.store_digest,
            "migration_registry_digest": self.migration_registry_digest,
            "projector_digest": self.projector_digest,
            "capacity_digest": self.capacity_digest,
            "config_revision": self.config_revision,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "generation": self.generation,
            "pending_operation": self.pending_operation,
            "journal_valid": self.journal_valid,
            "journal_digest": self.journal_digest,
            "resume_mode": self.resume_mode,
            "recovery_evidence_digest": self.recovery_evidence_digest,
            "facts": self.facts,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartupCoordinatorReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_digest: String,
    pub status: StartupCoordinatorStatus,
    pub completed_stages: Vec<StartupStage>,
    pub blocked_stage: Option<StartupStage>,
    pub reason: String,
    pub remediation: String,
    pub report_digest: String,
}

impl StartupCoordinatorReport {
    pub fn evaluate(request: &StartupCoordinatorRequest) -> Result<Self, String> {
        request.validate()?;
        let (status, completed_stages, blocked_stage, reason, remediation) = derive_result(request);
        let mut report = Self {
            schema: DEPLOYMENT_STARTUP_REPORT_SCHEMA.to_owned(),
            version: DEPLOYMENT_STARTUP_VERSION,
            request_digest: request.request_digest.clone(),
            status,
            completed_stages,
            blocked_stage,
            reason,
            remediation,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPLOYMENT_STARTUP_REPORT_SCHEMA
            || self.version != DEPLOYMENT_STARTUP_VERSION
            || self.completed_stages.len() > MAX_STARTUP_FACTS
            || self
                .completed_stages
                .windows(2)
                .any(|pair| pair[0].ordinal() >= pair[1].ordinal())
        {
            return Err("deployment_startup_report_header_invalid".to_owned());
        }
        valid_digest(
            &self.request_digest,
            "deployment_startup_report_request_digest",
        )?;
        valid_digest(&self.report_digest, "deployment_startup_report_digest")?;
        bounded(&self.reason, "deployment_startup_report_reason")?;
        bounded(&self.remediation, "deployment_startup_report_remediation")?;
        if self.report_digest != self.digest() {
            return Err("deployment_startup_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, request: &StartupCoordinatorRequest) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        let expected = derive_result(request);
        if self.request_digest != request.request_digest
            || (
                self.status,
                &self.completed_stages,
                self.blocked_stage,
                &self.reason,
                &self.remediation,
            ) != (
                expected.0,
                &expected.1,
                expected.2,
                &expected.3,
                &expected.4,
            )
        {
            return Err("deployment_startup_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_digest": self.request_digest,
            "status": self.status,
            "completed_stages": self.completed_stages,
            "blocked_stage": self.blocked_stage,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn derive_result(
    request: &StartupCoordinatorRequest,
) -> (
    StartupCoordinatorStatus,
    Vec<StartupStage>,
    Option<StartupStage>,
    String,
    String,
) {
    let mut completed = Vec::new();
    for stage in StartupStage::ALL {
        let Some(fact) = request.facts.iter().find(|fact| fact.stage == stage) else {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_stage_missing".to_owned(),
                format!("supply_verified_{stage:?}_fact"),
            );
        };
        if fact.authority_epoch != request.authority_epoch
            || fact.data_epoch != request.data_epoch
            || fact.generation != request.generation
        {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_epoch_or_generation_mismatch".to_owned(),
                "rebuild_facts_for_current_startup_epoch".to_owned(),
            );
        }
        let expected_digest = match stage {
            StartupStage::Manifest => &request.release_manifest_digest,
            StartupStage::Root => &request.storage_root_digest,
            StartupStage::Trust => &request.project_trust_digest,
            StartupStage::Lease => &request.lease_digest,
            StartupStage::Store => &request.store_digest,
            StartupStage::Migration => &request.migration_registry_digest,
            StartupStage::Projector => &request.projector_digest,
            StartupStage::Capacity => &request.capacity_digest,
        };
        if &fact.source_digest != expected_digest {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_stage_digest_mismatch".to_owned(),
                "recompute_stage_from_current_revision".to_owned(),
            );
        }
        if stage == StartupStage::Trust && !request.project_trusted {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_project_untrusted".to_owned(),
                "trust_project_before_startup".to_owned(),
            );
        }
        if stage == StartupStage::Store && !request.journal_valid {
            return (
                StartupCoordinatorStatus::Blocked,
                completed,
                Some(stage),
                "startup_journal_invalid".to_owned(),
                "quarantine_and_reconcile_journal".to_owned(),
            );
        }
        if fact.status != StartupFactStatus::Ready {
            return (
                if fact.status == StartupFactStatus::Unknown {
                    StartupCoordinatorStatus::Unknown
                } else {
                    StartupCoordinatorStatus::Blocked
                },
                completed,
                Some(stage),
                fact.reason.clone(),
                fact.remediation.clone(),
            );
        }
        completed.push(stage);
    }
    if request.pending_operation && request.resume_mode == StartupResumeMode::Fresh {
        return (
            StartupCoordinatorStatus::Blocked,
            completed,
            Some(StartupStage::Lease),
            "startup_resume_requires_explicit_recovery".to_owned(),
            "record_recovery_evidence_before_resume".to_owned(),
        );
    }
    (
        StartupCoordinatorStatus::Ready,
        completed,
        None,
        "ok".to_owned(),
        "none".to_owned(),
    )
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_STARTUP_TEXT_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
