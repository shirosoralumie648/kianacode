//! Container lifecycle evidence contract.
//!
//! This classifies fake-harness or target-runtime observations. It does not start a container,
//! grant an EnvironmentPort request or convert a probe observation into durable deployment truth.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONTAINER_LIFECYCLE_EVIDENCE_SCHEMA: &str = "kiana.container-lifecycle-evidence.v1";
pub const CONTAINER_LIFECYCLE_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_PHASES: usize = 6;
const MAX_LIMITATIONS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLifecycleMode {
    FakeHarness,
    RealRuntime,
    TargetOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLifecycleProofLevel {
    Source,
    LocalBehavior,
    Durable,
    Physical,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLifecycleStatus {
    Verified,
    Partial,
    Blocked,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerLifecyclePhase {
    Startup,
    Readiness,
    Liveness,
    Quiesce,
    Stop,
    Dispose,
}

impl ContainerLifecyclePhase {
    const ALL: [Self; MAX_PHASES] = [
        Self::Startup,
        Self::Readiness,
        Self::Liveness,
        Self::Quiesce,
        Self::Stop,
        Self::Dispose,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerPhaseEvidence {
    pub phase: ContainerLifecyclePhase,
    pub observation_digest: String,
    pub receipt_digest: Option<String>,
    pub result_unknown: bool,
}

impl ContainerPhaseEvidence {
    fn validate(&self) -> Result<(), String> {
        digest(
            &self.observation_digest,
            "container_phase_observation_digest",
        )?;
        if let Some(value) = &self.receipt_digest {
            digest(value, "container_phase_receipt_digest")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContainerLifecycleEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub mode: ContainerLifecycleMode,
    pub proof_level: ContainerLifecycleProofLevel,
    pub status: ContainerLifecycleStatus,
    pub image_digest: String,
    pub root_identity_digest: String,
    pub owner_digest: String,
    pub scope_digest: String,
    pub plan_digest: String,
    pub environment_allowlist_digest: String,
    pub runtime_identity_digest: Option<String>,
    pub phases: Vec<ContainerPhaseEvidence>,
    pub stop_signal: String,
    pub no_host_fallback: bool,
    pub inventory_digest: Option<String>,
    pub fence_digest: Option<String>,
    pub health_receipt_digest: Option<String>,
    pub cleanup_receipt_digest: Option<String>,
    pub result_unknown: bool,
    pub limitations: Vec<String>,
    pub evidence_digest: String,
}

impl ContainerLifecycleEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mode: ContainerLifecycleMode,
        proof_level: ContainerLifecycleProofLevel,
        status: ContainerLifecycleStatus,
        image_digest: impl Into<String>,
        root_identity_digest: impl Into<String>,
        owner_digest: impl Into<String>,
        scope_digest: impl Into<String>,
        plan_digest: impl Into<String>,
        environment_allowlist_digest: impl Into<String>,
        runtime_identity_digest: Option<String>,
        phases: Vec<ContainerPhaseEvidence>,
        stop_signal: impl Into<String>,
        no_host_fallback: bool,
        inventory_digest: Option<String>,
        fence_digest: Option<String>,
        health_receipt_digest: Option<String>,
        cleanup_receipt_digest: Option<String>,
        result_unknown: bool,
        limitations: Vec<String>,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: CONTAINER_LIFECYCLE_EVIDENCE_SCHEMA.to_owned(),
            version: CONTAINER_LIFECYCLE_EVIDENCE_VERSION,
            mode,
            proof_level,
            status,
            image_digest: image_digest.into(),
            root_identity_digest: root_identity_digest.into(),
            owner_digest: owner_digest.into(),
            scope_digest: scope_digest.into(),
            plan_digest: plan_digest.into(),
            environment_allowlist_digest: environment_allowlist_digest.into(),
            runtime_identity_digest,
            phases,
            stop_signal: stop_signal.into(),
            no_host_fallback,
            inventory_digest,
            fence_digest,
            health_receipt_digest,
            cleanup_receipt_digest,
            result_unknown,
            limitations,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTAINER_LIFECYCLE_EVIDENCE_SCHEMA
            || self.version != CONTAINER_LIFECYCLE_EVIDENCE_VERSION
        {
            return Err("container_lifecycle_evidence_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.image_digest, "container_image_digest"),
            (&self.root_identity_digest, "container_root_identity_digest"),
            (&self.owner_digest, "container_owner_digest"),
            (&self.scope_digest, "container_scope_digest"),
            (&self.plan_digest, "container_plan_digest"),
            (
                &self.environment_allowlist_digest,
                "container_environment_allowlist_digest",
            ),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                &self.runtime_identity_digest,
                "container_runtime_identity_digest",
            ),
            (&self.inventory_digest, "container_inventory_digest"),
            (&self.fence_digest, "container_fence_digest"),
            (
                &self.health_receipt_digest,
                "container_health_receipt_digest",
            ),
            (
                &self.cleanup_receipt_digest,
                "container_cleanup_receipt_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if self.phases.is_empty() || self.phases.len() > MAX_PHASES {
            return Err("container_lifecycle_phase_count_invalid".to_owned());
        }
        let mut phases = BTreeSet::new();
        for phase in &self.phases {
            phase.validate()?;
            if !phases.insert(phase.phase) {
                return Err("container_lifecycle_phase_duplicate".to_owned());
            }
        }
        if self.stop_signal != "SIGTERM" {
            return Err("container_lifecycle_stop_signal_invalid".to_owned());
        }
        if !self.no_host_fallback {
            return Err("container_lifecycle_host_fallback_forbidden".to_owned());
        }
        if self.limitations.len() > MAX_LIMITATIONS {
            return Err("container_lifecycle_limitation_limit".to_owned());
        }
        for limitation in &self.limitations {
            bounded(limitation, "container_lifecycle_limitation", 256)?;
        }
        if self.mode == ContainerLifecycleMode::TargetOnly
            && self.status == ContainerLifecycleStatus::Verified
        {
            return Err("container_lifecycle_target_cannot_verify".to_owned());
        }
        if self.mode == ContainerLifecycleMode::FakeHarness
            && matches!(
                self.proof_level,
                ContainerLifecycleProofLevel::Durable | ContainerLifecycleProofLevel::Physical
            )
        {
            return Err("container_lifecycle_fake_proof_level_invalid".to_owned());
        }
        if self.result_unknown && self.status == ContainerLifecycleStatus::Verified {
            return Err("container_lifecycle_unknown_cannot_verify".to_owned());
        }
        if self.status == ContainerLifecycleStatus::Verified {
            if self.runtime_identity_digest.is_none()
                || self.inventory_digest.is_none()
                || self.fence_digest.is_none()
                || self.health_receipt_digest.is_none()
                || self.cleanup_receipt_digest.is_none()
                || self.proof_level == ContainerLifecycleProofLevel::Source
                || ContainerLifecyclePhase::ALL
                    .iter()
                    .any(|phase| !phases.contains(phase))
                || self
                    .phases
                    .iter()
                    .any(|phase| phase.receipt_digest.is_none() || phase.result_unknown)
            {
                return Err("container_lifecycle_verified_evidence_incomplete".to_owned());
            }
        } else if self.limitations.is_empty() {
            return Err("container_lifecycle_nonverified_reason_required".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("container_lifecycle_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "mode": self.mode,
            "proof_level": self.proof_level,
            "status": self.status,
            "image_digest": self.image_digest,
            "root_identity_digest": self.root_identity_digest,
            "owner_digest": self.owner_digest,
            "scope_digest": self.scope_digest,
            "plan_digest": self.plan_digest,
            "environment_allowlist_digest": self.environment_allowlist_digest,
            "runtime_identity_digest": self.runtime_identity_digest,
            "phases": self.phases,
            "stop_signal": self.stop_signal,
            "no_host_fallback": self.no_host_fallback,
            "inventory_digest": self.inventory_digest,
            "fence_digest": self.fence_digest,
            "health_receipt_digest": self.health_receipt_digest,
            "cleanup_receipt_digest": self.cleanup_receipt_digest,
            "result_unknown": self.result_unknown,
            "limitations": self.limitations,
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
