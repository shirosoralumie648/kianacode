//! Deterministic fault-injection contracts for replay-only reliability fixtures.
//!
//! A fault case describes expected safety classification at an injection point. It is a test and
//! diagnostic artifact, not permission to execute a command or an external effect.

use crate::{canonical_journal_bytes, json_digest, EventId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const FAULT_MATRIX_SCHEMA: &str = "kiana.fault-matrix.v1";
pub const FAULT_CASE_SCHEMA: &str = "kiana.fault-case.v1";
pub const FAULT_MATRIX_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const FAULT_CASE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_FAULT_CASES: usize = 16;
pub const MAX_FAULT_ERROR_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultInjectionPoint {
    Prepare,
    Commit,
    Dispatch,
    Result,
    Flush,
    Projector,
    Export,
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultCaseStatus {
    Rejected,
    Unknown,
    Observed,
}

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_ids(source_cursor: u64, source_event_ids: &[EventId]) -> Result<(), String> {
    if source_cursor == 0 || source_event_ids.is_empty() || source_event_ids.len() > 256 {
        return Err("fault_source_binding_invalid".to_owned());
    }
    let mut ids = BTreeSet::new();
    for event_id in source_event_ids {
        if !ids.insert(event_id.to_string()) {
            return Err("fault_source_event_duplicate".to_owned());
        }
    }
    Ok(())
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultCase {
    pub schema: String,
    pub version: SchemaVersion,
    pub point: FaultInjectionPoint,
    pub seed: u64,
    pub status: FaultCaseStatus,
    pub effect_started: bool,
    pub effect_known: bool,
    pub resource_fenced: bool,
    pub duplicate_effect: bool,
    pub false_success: bool,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub error_code: String,
    pub case_digest: String,
}

impl FaultCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        point: FaultInjectionPoint,
        seed: u64,
        status: FaultCaseStatus,
        effect_started: bool,
        effect_known: bool,
        resource_fenced: bool,
        duplicate_effect: bool,
        false_success: bool,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        error_code: impl Into<String>,
    ) -> Result<Self, String> {
        let mut case = Self {
            schema: FAULT_CASE_SCHEMA.to_owned(),
            version: FAULT_CASE_SCHEMA_VERSION,
            point,
            seed,
            status,
            effect_started,
            effect_known,
            resource_fenced,
            duplicate_effect,
            false_success,
            source_cursor,
            source_event_ids,
            error_code: error_code.into(),
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FAULT_CASE_SCHEMA
            || !self.version.is_compatible_with(&FAULT_CASE_SCHEMA_VERSION)
            || self.seed == 0
        {
            return Err("fault_case_header_invalid".to_owned());
        }
        validate_ids(self.source_cursor, &self.source_event_ids)?;
        nonempty(&self.error_code, "fault_error_code", MAX_FAULT_ERROR_BYTES)?;
        if self.duplicate_effect || self.false_success {
            return Err("fault_safety_invariant_failed".to_owned());
        }
        match self.status {
            FaultCaseStatus::Rejected => {
                if self.effect_started || !self.effect_known {
                    return Err("fault_rejected_effect_conflict".to_owned());
                }
            }
            FaultCaseStatus::Unknown => {
                if !self.effect_started && self.effect_known {
                    return Err("fault_unknown_effect_conflict".to_owned());
                }
                if !self.resource_fenced {
                    return Err("fault_unknown_not_fenced".to_owned());
                }
            }
            FaultCaseStatus::Observed => {
                if !self.effect_started || !self.effect_known || self.resource_fenced {
                    return Err("fault_observed_effect_conflict".to_owned());
                }
            }
        }
        validate_digest(&self.case_digest, "fault_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("fault_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "case_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub seed: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub cases: Vec<FaultCase>,
    pub matrix_digest: String,
}

impl FaultMatrix {
    pub fn new(
        seed: u64,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        cases: Vec<FaultCase>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: FAULT_MATRIX_SCHEMA.to_owned(),
            version: FAULT_MATRIX_SCHEMA_VERSION,
            seed,
            source_cursor,
            source_event_ids,
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != FAULT_MATRIX_SCHEMA
            || !self
                .version
                .is_compatible_with(&FAULT_MATRIX_SCHEMA_VERSION)
            || self.seed == 0
        {
            return Err("fault_matrix_header_invalid".to_owned());
        }
        validate_ids(self.source_cursor, &self.source_event_ids)?;
        if self.cases.is_empty() || self.cases.len() > MAX_FAULT_CASES {
            return Err("fault_matrix_case_limit".to_owned());
        }
        let mut points = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.seed != self.seed
                || case.source_cursor != self.source_cursor
                || case.source_event_ids != self.source_event_ids
            {
                return Err("fault_case_matrix_binding_mismatch".to_owned());
            }
            if !points.insert(case.point) {
                return Err("fault_matrix_point_duplicate".to_owned());
            }
        }
        validate_digest(&self.matrix_digest, "fault_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("fault_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "matrix_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
