//! Cross-entrypoint release/upgrade/recovery UAT evidence contract.
//!
//! The matrix is a source and fixture boundary for CLI, Web, Workbench and Desktop. It requires
//! the same daemon/control-plane/harness digests, records deny/success/restart/replay/Unknown
//! outcomes and keeps fake-provider evidence separate from explicitly approved live opt-in.
//! Constructing a row never executes an operation or turns a transcript into a receipt.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const RELEASE_UAT_SCHEMA: &str = "kiana.release-uat-matrix.v1";
pub const RELEASE_UAT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RELEASE_UAT_CASES: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UatEntrypoint {
    Cli,
    Web,
    Workbench,
    Desktop,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UatScenario {
    Release,
    Upgrade,
    Rollback,
    Backup,
    Restore,
    Migration,
    Health,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UatOutcome {
    Denied,
    Succeeded,
    RestartRecovered,
    Replayed,
    ResultUnknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UatProviderMode {
    Fake,
    LiveOptIn,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UatEffectStatus {
    None,
    Known,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UatCase {
    pub entrypoint: UatEntrypoint,
    pub scenario: UatScenario,
    pub outcome: UatOutcome,
    pub provider_mode: UatProviderMode,
    pub operator_approved: bool,
    pub effect_status: UatEffectStatus,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    pub reconcile_required: bool,
    pub retry_permitted: bool,
    pub case_digest: String,
}

impl UatCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        entrypoint: UatEntrypoint,
        scenario: UatScenario,
        outcome: UatOutcome,
        provider_mode: UatProviderMode,
        operator_approved: bool,
        effect_status: UatEffectStatus,
        receipt_digest: Option<String>,
        reconcile_required: bool,
        retry_permitted: bool,
    ) -> Result<Self, String> {
        let mut case = Self {
            entrypoint,
            scenario,
            outcome,
            provider_mode,
            operator_approved,
            effect_status,
            receipt_digest,
            reconcile_required,
            retry_permitted,
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(receipt) = &self.receipt_digest {
            validate_digest(receipt, "uat_receipt_digest")?;
        }
        match self.provider_mode {
            UatProviderMode::Fake if self.operator_approved => {
                return Err("uat_fake_provider_cannot_claim_live_approval".to_owned())
            }
            UatProviderMode::LiveOptIn if !self.operator_approved => {
                return Err("uat_live_provider_approval_missing".to_owned())
            }
            _ => {}
        }
        match self.outcome {
            UatOutcome::Denied => {
                if self.effect_status != UatEffectStatus::None
                    || self.receipt_digest.is_some()
                    || self.reconcile_required
                {
                    return Err("uat_denied_effect_evidence_invalid".to_owned());
                }
            }
            UatOutcome::ResultUnknown => {
                if self.effect_status != UatEffectStatus::Unknown
                    || !self.reconcile_required
                    || self.retry_permitted
                {
                    return Err("uat_unknown_retry_forbidden".to_owned());
                }
            }
            UatOutcome::Succeeded | UatOutcome::RestartRecovered | UatOutcome::Replayed => {
                if self.effect_status != UatEffectStatus::Known
                    || self.receipt_digest.is_none()
                    || self.reconcile_required
                {
                    return Err("uat_success_receipt_evidence_missing".to_owned());
                }
            }
        }
        validate_digest(&self.case_digest, "uat_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("uat_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "entrypoint": self.entrypoint,
            "scenario": self.scenario,
            "outcome": self.outcome,
            "provider_mode": self.provider_mode,
            "operator_approved": self.operator_approved,
            "effect_status": self.effect_status,
            "receipt_digest": self.receipt_digest,
            "reconcile_required": self.reconcile_required,
            "retry_permitted": self.retry_permitted,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseUatMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub daemon_spine_digest: String,
    pub control_plane_digest: String,
    pub harness_digest: String,
    pub provider_mode: UatProviderMode,
    pub cases: Vec<UatCase>,
    pub matrix_digest: String,
}

impl ReleaseUatMatrix {
    pub fn new(
        daemon_spine_digest: impl Into<String>,
        control_plane_digest: impl Into<String>,
        harness_digest: impl Into<String>,
        provider_mode: UatProviderMode,
        cases: Vec<UatCase>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: RELEASE_UAT_SCHEMA.to_owned(),
            version: RELEASE_UAT_VERSION,
            daemon_spine_digest: daemon_spine_digest.into(),
            control_plane_digest: control_plane_digest.into(),
            harness_digest: harness_digest.into(),
            provider_mode,
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RELEASE_UAT_SCHEMA || self.version != RELEASE_UAT_VERSION {
            return Err("uat_matrix_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.daemon_spine_digest, "uat_daemon_spine_digest"),
            (&self.control_plane_digest, "uat_control_plane_digest"),
            (&self.harness_digest, "uat_harness_digest"),
            (&self.matrix_digest, "uat_matrix_digest"),
        ] {
            validate_digest(value, field)?;
        }
        if self.cases.is_empty() || self.cases.len() > MAX_RELEASE_UAT_CASES {
            return Err("uat_case_count_invalid".to_owned());
        }
        let mut keys = BTreeSet::new();
        let mut deny_success = BTreeSet::new();
        let mut has_restart = false;
        let mut has_replay = false;
        let mut has_unknown = false;
        for case in &self.cases {
            case.validate()?;
            if case.provider_mode != self.provider_mode {
                return Err("uat_provider_mode_drift".to_owned());
            }
            if !keys.insert((case.entrypoint, case.scenario, case.outcome)) {
                return Err("uat_case_duplicate".to_owned());
            }
            if matches!(case.outcome, UatOutcome::Denied | UatOutcome::Succeeded) {
                deny_success.insert((case.entrypoint, case.scenario, case.outcome));
            }
            has_restart |= case.outcome == UatOutcome::RestartRecovered;
            has_replay |= case.outcome == UatOutcome::Replayed;
            has_unknown |= case.outcome == UatOutcome::ResultUnknown;
        }
        for entrypoint in [
            UatEntrypoint::Cli,
            UatEntrypoint::Web,
            UatEntrypoint::Workbench,
            UatEntrypoint::Desktop,
        ] {
            for scenario in [
                UatScenario::Release,
                UatScenario::Upgrade,
                UatScenario::Rollback,
                UatScenario::Backup,
                UatScenario::Restore,
                UatScenario::Migration,
                UatScenario::Health,
            ] {
                if !deny_success.contains(&(entrypoint, scenario, UatOutcome::Denied)) {
                    return Err("uat_denied_coverage_missing".to_owned());
                }
                if !deny_success.contains(&(entrypoint, scenario, UatOutcome::Succeeded)) {
                    return Err("uat_success_coverage_missing".to_owned());
                }
            }
        }
        if !has_restart {
            return Err("uat_restart_coverage_missing".to_owned());
        }
        if !has_replay {
            return Err("uat_replay_coverage_missing".to_owned());
        }
        if !has_unknown {
            return Err("uat_result_unknown_coverage_missing".to_owned());
        }
        if self.matrix_digest != self.digest() {
            return Err("uat_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "daemon_spine_digest": self.daemon_spine_digest,
            "control_plane_digest": self.control_plane_digest,
            "harness_digest": self.harness_digest,
            "provider_mode": self.provider_mode,
            "cases": self.cases,
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
