//! ER-32 Memory/JSONL adapter conformance evidence.
//!
//! This contract compares logical adapter outcomes without claiming that an in-memory adapter is
//! durable. It does not open files, append frames, run property generators or execute recovery.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const ER32_CONFORMANCE_SCHEMA: &str = "kiana.er32-adapter-conformance.v1";
pub const ER32_OBSERVATION_SCHEMA: &str = "kiana.er32-adapter-observation.v1";

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er32AdapterKind {
    Memory,
    Jsonl,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er32Operation {
    Commit,
    Replay,
    Conflict,
    CursorGap,
    Unknown,
    SchemaMigration,
    Redaction,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Er32ResultClass {
    Committed,
    Replayed,
    Conflict,
    Rejected,
    Unknown,
    SnapshotRequired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er32AdapterObservation {
    pub schema: String,
    pub adapter: Er32AdapterKind,
    pub operation: Er32Operation,
    pub source_digest: String,
    pub output_digest: String,
    pub result: Er32ResultClass,
    pub durable_claim: bool,
    pub sync_ack: bool,
    pub duplicate_effect: bool,
    pub false_success: bool,
    pub secret_free: bool,
    pub digest: String,
}

impl Er32AdapterObservation {
    pub fn new(
        adapter: Er32AdapterKind,
        operation: Er32Operation,
        source_digest: impl Into<String>,
        output_digest: impl Into<String>,
        result: Er32ResultClass,
        durable_claim: bool,
        sync_ack: bool,
    ) -> Self {
        let mut observation = Self {
            schema: ER32_OBSERVATION_SCHEMA.to_owned(),
            adapter,
            operation,
            source_digest: source_digest.into(),
            output_digest: output_digest.into(),
            result,
            durable_claim,
            sync_ack,
            duplicate_effect: false,
            false_success: false,
            secret_free: true,
            digest: String::new(),
        };
        observation.digest = observation.canonical_digest();
        observation
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER32_OBSERVATION_SCHEMA
            || self.duplicate_effect
            || self.false_success
            || !self.secret_free
        {
            return Err("er32_observation_safety_invariant_failed".to_owned());
        }
        digest(&self.source_digest, "er32_source_digest_invalid")?;
        digest(&self.output_digest, "er32_output_digest_invalid")?;
        digest(&self.digest, "er32_observation_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("er32_observation_digest_mismatch".to_owned());
        }
        if self.adapter == Er32AdapterKind::Memory && self.durable_claim {
            return Err("er32_memory_cannot_claim_durable".to_owned());
        }
        if self.durable_claim && (self.adapter != Er32AdapterKind::Jsonl || !self.sync_ack) {
            return Err("er32_durable_claim_requires_jsonl_sync".to_owned());
        }
        match self.operation {
            Er32Operation::Replay => {
                if self.result != Er32ResultClass::Replayed {
                    return Err("er32_replay_result_invalid".to_owned());
                }
            }
            Er32Operation::Conflict => {
                if self.result != Er32ResultClass::Conflict {
                    return Err("er32_conflict_result_invalid".to_owned());
                }
            }
            Er32Operation::CursorGap => {
                if self.result != Er32ResultClass::SnapshotRequired {
                    return Err("er32_cursor_gap_result_invalid".to_owned());
                }
            }
            Er32Operation::Unknown => {
                if self.result != Er32ResultClass::Unknown {
                    return Err("er32_unknown_result_invalid".to_owned());
                }
            }
            Er32Operation::SchemaMigration => {
                if self.result != Er32ResultClass::Rejected
                    && self.result != Er32ResultClass::Committed
                {
                    return Err("er32_schema_migration_result_invalid".to_owned());
                }
            }
            Er32Operation::Commit | Er32Operation::Redaction => {}
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "adapter": self.adapter,
            "operation": self.operation,
            "source_digest": self.source_digest,
            "output_digest": self.output_digest,
            "result": self.result,
            "durable_claim": self.durable_claim,
            "sync_ack": self.sync_ack,
            "duplicate_effect": self.duplicate_effect,
            "false_success": self.false_success,
            "secret_free": self.secret_free,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Er32ConformanceReport {
    pub schema: String,
    pub seed: u64,
    pub observations: Vec<Er32AdapterObservation>,
    pub digest: String,
}

impl Er32ConformanceReport {
    pub fn new(seed: u64, observations: Vec<Er32AdapterObservation>) -> Self {
        let mut report = Self {
            schema: ER32_CONFORMANCE_SCHEMA.to_owned(),
            seed,
            observations,
            digest: String::new(),
        };
        report.digest = report.canonical_digest();
        report
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ER32_CONFORMANCE_SCHEMA
            || self.seed == 0
            || self.observations.is_empty()
            || self.observations.len() > 64
        {
            return Err("er32_report_header_invalid".to_owned());
        }
        digest(&self.digest, "er32_report_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("er32_report_digest_mismatch".to_owned());
        }
        let mut keys = BTreeSet::new();
        for observation in &self.observations {
            observation.validate()?;
            let key = format!("{:?}:{:?}", observation.adapter, observation.operation);
            if !keys.insert(key) {
                return Err("er32_observation_duplicate".to_owned());
            }
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "observations": self.observations,
        }))
    }
}
