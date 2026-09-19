//! Security recovery/retention rehearsal evidence contract.
//!
//! The rehearsal is a deny-first, source/fixture-bound model for restart, quarantine restore,
//! replay, Unknown reconciliation and retention. It never kills a process or deletes data.

use crate::{json_digest, SchemaVersion, UatOutcome};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SECURITY_REHEARSAL_SCHEMA: &str = "kiana.security-rehearsal.v1";
pub const SECURITY_REHEARSAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityRehearsalScenario {
    Restart,
    RestoreQuarantine,
    Replay,
    UnknownReconcile,
    RetentionPrune,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRehearsalCase {
    pub scenario: SecurityRehearsalScenario,
    pub outcome: UatOutcome,
    pub operation_digest: String,
    pub source_cursor: u64,
    pub fact_digest: String,
    pub old_lease_fenced: bool,
    pub quarantine_verified: bool,
    pub no_duplicate_effect: bool,
    pub unknown_reconciled: bool,
    pub retention_watermark_committed: bool,
    pub legal_hold_respected: bool,
    pub retry_permitted: bool,
    pub evidence_digest: String,
}

impl SecurityRehearsalCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scenario: SecurityRehearsalScenario,
        outcome: UatOutcome,
        operation_digest: impl Into<String>,
        source_cursor: u64,
        fact_digest: impl Into<String>,
        old_lease_fenced: bool,
        quarantine_verified: bool,
        no_duplicate_effect: bool,
        unknown_reconciled: bool,
        retention_watermark_committed: bool,
        legal_hold_respected: bool,
        retry_permitted: bool,
    ) -> Result<Self, String> {
        let mut case = Self {
            scenario,
            outcome,
            operation_digest: operation_digest.into(),
            source_cursor,
            fact_digest: fact_digest.into(),
            old_lease_fenced,
            quarantine_verified,
            no_duplicate_effect,
            unknown_reconciled,
            retention_watermark_committed,
            legal_hold_respected,
            retry_permitted,
            evidence_digest: String::new(),
        };
        case.evidence_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_digest(
            &self.operation_digest,
            "security_rehearsal_operation_digest",
        )?;
        validate_digest(&self.fact_digest, "security_rehearsal_fact_digest")?;
        validate_digest(&self.evidence_digest, "security_rehearsal_evidence_digest")?;
        if self.source_cursor == 0 {
            return Err("security_rehearsal_cursor_invalid".to_owned());
        }
        match self.outcome {
            UatOutcome::Denied => {
                if self.retry_permitted {
                    return Err("security_rehearsal_denied_retry_invalid".to_owned());
                }
            }
            UatOutcome::ResultUnknown => {
                if !self.unknown_reconciled || self.retry_permitted {
                    return Err("security_rehearsal_unknown_retry_forbidden".to_owned());
                }
            }
            UatOutcome::Succeeded | UatOutcome::RestartRecovered | UatOutcome::Replayed => {
                if !self.old_lease_fenced || !self.no_duplicate_effect {
                    return Err("security_rehearsal_fence_or_duplicate_guard_failed".to_owned());
                }
                match self.scenario {
                    SecurityRehearsalScenario::RestoreQuarantine if !self.quarantine_verified => {
                        return Err("security_rehearsal_quarantine_unverified".to_owned())
                    }
                    SecurityRehearsalScenario::Replay if !self.no_duplicate_effect => {
                        return Err("security_rehearsal_replay_duplicate_effect".to_owned())
                    }
                    SecurityRehearsalScenario::UnknownReconcile if !self.unknown_reconciled => {
                        return Err("security_rehearsal_reconcile_missing".to_owned())
                    }
                    SecurityRehearsalScenario::RetentionPrune
                        if !self.retention_watermark_committed || !self.legal_hold_respected =>
                    {
                        return Err("security_rehearsal_retention_guard_failed".to_owned())
                    }
                    _ => {}
                }
            }
        }
        if self.evidence_digest != self.digest() {
            return Err("security_rehearsal_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "scenario": self.scenario,
            "outcome": self.outcome,
            "operation_digest": self.operation_digest,
            "source_cursor": self.source_cursor,
            "fact_digest": self.fact_digest,
            "old_lease_fenced": self.old_lease_fenced,
            "quarantine_verified": self.quarantine_verified,
            "no_duplicate_effect": self.no_duplicate_effect,
            "unknown_reconciled": self.unknown_reconciled,
            "retention_watermark_committed": self.retention_watermark_committed,
            "legal_hold_respected": self.legal_hold_respected,
            "retry_permitted": self.retry_permitted,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRehearsalMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_digest: String,
    pub cases: Vec<SecurityRehearsalCase>,
    pub matrix_digest: String,
}

impl SecurityRehearsalMatrix {
    pub fn new(
        source_digest: impl Into<String>,
        cases: Vec<SecurityRehearsalCase>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: SECURITY_REHEARSAL_SCHEMA.to_owned(),
            version: SECURITY_REHEARSAL_VERSION,
            source_digest: source_digest.into(),
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_REHEARSAL_SCHEMA || self.version != SECURITY_REHEARSAL_VERSION {
            return Err("security_rehearsal_matrix_header_invalid".to_owned());
        }
        validate_digest(&self.source_digest, "security_rehearsal_source_digest")?;
        validate_digest(&self.matrix_digest, "security_rehearsal_matrix_digest")?;
        if self.cases.is_empty() || self.cases.len() > 32 {
            return Err("security_rehearsal_case_count_invalid".to_owned());
        }
        let mut keys = BTreeSet::new();
        let mut has_unknown = false;
        for case in &self.cases {
            case.validate()?;
            if !keys.insert((case.scenario, case.outcome)) {
                return Err("security_rehearsal_case_duplicate".to_owned());
            }
            has_unknown |= case.outcome == UatOutcome::ResultUnknown;
        }
        for scenario in [
            SecurityRehearsalScenario::Restart,
            SecurityRehearsalScenario::RestoreQuarantine,
            SecurityRehearsalScenario::Replay,
            SecurityRehearsalScenario::UnknownReconcile,
            SecurityRehearsalScenario::RetentionPrune,
        ] {
            if !keys.contains(&(scenario, UatOutcome::Denied))
                || !keys.contains(&(scenario, UatOutcome::Succeeded))
            {
                return Err("security_rehearsal_coverage_missing".to_owned());
            }
        }
        if !has_unknown {
            return Err("security_rehearsal_unknown_coverage_missing".to_owned());
        }
        if self.matrix_digest != self.digest() {
            return Err("security_rehearsal_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_digest": self.source_digest,
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
