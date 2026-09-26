//! ER-31 replay-only Event/Receipt/Recovery crash-point matrix.
//!
//! The matrix classifies the required safety result at each effect boundary. It never kills a
//! process, calls a provider/Broker, writes an EventLog fact or converts a simulated case into
//! durable/live evidence. Existing EventStore, Receipt and recovery adapters remain authoritative.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const ER31_FAULT_MATRIX_SCHEMA: &str = "kiana.er31-fault-matrix.v1";
pub const ER31_FAULT_CASE_SCHEMA: &str = "kiana.er31-fault-case.v1";
pub const ER31_FAULT_MAX_CASES: usize = 12;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er31FaultPoint {
    PrepareBefore,
    PrepareAfter,
    PermitConsume,
    Spawn,
    PartialWrite,
    PatchRename,
    StopReap,
    ResultCommit,
    ResultDelivery,
    ProjectionCheckpoint,
    ArtifactPublish,
    Cleanup,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er31Disposition {
    Rejected,
    Unknown,
    Reconciled,
    Observed,
}

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er31FaultCase {
    pub schema: String,
    pub point: Er31FaultPoint,
    pub seed: u64,
    pub source_cursor: u64,
    pub source_event_refs: Vec<String>,
    pub disposition: Er31Disposition,
    pub effect_started: bool,
    pub effect_confirmed: bool,
    pub duplicate_effect: bool,
    pub false_success: bool,
    pub unknown_queryable: bool,
    pub resources_fenced: bool,
    pub receipt_limitations: Vec<String>,
    pub case_digest: String,
}

impl Er31FaultCase {
    fn new(
        point: Er31FaultPoint,
        seed: u64,
        source_cursor: u64,
        source_event_refs: Vec<String>,
    ) -> Self {
        let unknown = matches!(
            point,
            Er31FaultPoint::PermitConsume
                | Er31FaultPoint::Spawn
                | Er31FaultPoint::PartialWrite
                | Er31FaultPoint::PatchRename
                | Er31FaultPoint::StopReap
                | Er31FaultPoint::ResultCommit
                | Er31FaultPoint::ResultDelivery
                | Er31FaultPoint::ProjectionCheckpoint
                | Er31FaultPoint::ArtifactPublish
                | Er31FaultPoint::Cleanup
        );
        let rejected = matches!(
            point,
            Er31FaultPoint::PrepareBefore | Er31FaultPoint::PrepareAfter
        );
        let mut case = Self {
            schema: ER31_FAULT_CASE_SCHEMA.to_owned(),
            point,
            seed,
            source_cursor,
            source_event_refs,
            disposition: if rejected {
                Er31Disposition::Rejected
            } else if unknown {
                Er31Disposition::Unknown
            } else {
                Er31Disposition::Observed
            },
            effect_started: !rejected,
            effect_confirmed: false,
            duplicate_effect: false,
            false_success: false,
            unknown_queryable: unknown,
            resources_fenced: unknown || rejected,
            receipt_limitations: if unknown {
                vec!["effect confirmation remains pending".to_owned()]
            } else {
                Vec::new()
            },
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER31_FAULT_CASE_SCHEMA
            || self.seed == 0
            || self.source_cursor == 0
            || self.source_event_refs.is_empty()
            || self.duplicate_effect
            || self.false_success
        {
            return Err("er31_fault_case_header_or_safety_invalid".to_owned());
        }
        let mut refs = BTreeSet::new();
        for source in &self.source_event_refs {
            required(source, "er31_fault_source_ref_invalid")?;
            if !refs.insert(source) {
                return Err("er31_fault_source_ref_duplicate".to_owned());
            }
        }
        for limitation in &self.receipt_limitations {
            required(limitation, "er31_fault_limitation_invalid")?;
        }
        match self.disposition {
            Er31Disposition::Rejected => {
                if self.effect_started
                    || self.effect_confirmed
                    || !self.resources_fenced
                    || !self.receipt_limitations.is_empty()
                {
                    return Err("er31_rejected_case_effect_conflict".to_owned());
                }
            }
            Er31Disposition::Unknown | Er31Disposition::Reconciled => {
                if self.effect_confirmed
                    || !self.unknown_queryable
                    || !self.resources_fenced
                    || self.receipt_limitations.is_empty()
                {
                    return Err("er31_unknown_case_fence_or_limit_missing".to_owned());
                }
            }
            Er31Disposition::Observed => {
                if !self.effect_started || !self.effect_confirmed || !self.unknown_queryable {
                    return Err("er31_observed_case_confirmation_invalid".to_owned());
                }
            }
        }
        digest(&self.case_digest, "er31_fault_case_digest_invalid")?;
        if self.case_digest != self.digest() {
            return Err("er31_fault_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "point": self.point,
            "seed": self.seed,
            "source_cursor": self.source_cursor,
            "source_event_refs": self.source_event_refs,
            "disposition": self.disposition,
            "effect_started": self.effect_started,
            "effect_confirmed": self.effect_confirmed,
            "duplicate_effect": self.duplicate_effect,
            "false_success": self.false_success,
            "unknown_queryable": self.unknown_queryable,
            "resources_fenced": self.resources_fenced,
            "receipt_limitations": self.receipt_limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er31FaultMatrix {
    pub schema: String,
    pub seed: u64,
    pub source_cursor: u64,
    pub source_event_refs: Vec<String>,
    pub cases: Vec<Er31FaultCase>,
    pub matrix_digest: String,
}

impl Er31FaultMatrix {
    pub fn new(
        seed: u64,
        source_cursor: u64,
        mut source_event_refs: Vec<String>,
    ) -> Result<Self, String> {
        if seed == 0 || source_cursor == 0 || source_event_refs.is_empty() {
            return Err("er31_fault_matrix_header_invalid".to_owned());
        }
        source_event_refs.sort();
        let source_count = source_event_refs.len();
        source_event_refs.dedup();
        if source_event_refs.len() != source_count {
            return Err("er31_fault_source_duplicate".to_owned());
        }
        let points = [
            Er31FaultPoint::PrepareBefore,
            Er31FaultPoint::PrepareAfter,
            Er31FaultPoint::PermitConsume,
            Er31FaultPoint::Spawn,
            Er31FaultPoint::PartialWrite,
            Er31FaultPoint::PatchRename,
            Er31FaultPoint::StopReap,
            Er31FaultPoint::ResultCommit,
            Er31FaultPoint::ResultDelivery,
            Er31FaultPoint::ProjectionCheckpoint,
            Er31FaultPoint::ArtifactPublish,
            Er31FaultPoint::Cleanup,
        ];
        let cases = points
            .into_iter()
            .map(|point| Er31FaultCase::new(point, seed, source_cursor, source_event_refs.clone()))
            .collect::<Vec<_>>();
        let mut matrix = Self {
            schema: ER31_FAULT_MATRIX_SCHEMA.to_owned(),
            seed,
            source_cursor,
            source_event_refs,
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER31_FAULT_MATRIX_SCHEMA
            || self.seed == 0
            || self.source_cursor == 0
            || self.source_event_refs.is_empty()
            || self.cases.len() != ER31_FAULT_MAX_CASES
        {
            return Err("er31_fault_matrix_header_invalid".to_owned());
        }
        let mut points = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.seed != self.seed
                || case.source_cursor != self.source_cursor
                || case.source_event_refs != self.source_event_refs
                || !points.insert(case.point)
            {
                return Err("er31_fault_case_matrix_binding_invalid".to_owned());
            }
        }
        digest(&self.matrix_digest, "er31_fault_matrix_digest_invalid")?;
        if self.matrix_digest != self.digest() {
            return Err("er31_fault_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "source_cursor": self.source_cursor,
            "source_event_refs": self.source_event_refs,
            "cases": self.cases,
        }))
    }
}
