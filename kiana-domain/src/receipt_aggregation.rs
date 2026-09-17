//! Conservative aggregation fields for read-only receipts.

use crate::{
    json_digest, normalize_role_path, EventCursor, EventId, SchemaVersion, MAX_SOURCE_EVENT_IDS,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const RECEIPT_AGGREGATION_SCHEMA: &str = "kiana.receipt-aggregation.v1";
pub const RECEIPT_AGGREGATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_AGGREGATED_FILES: usize = 512;
const MAX_AGGREGATED_REFS: usize = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregationVerification {
    Complete,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAggregation {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub model_turns: u64,
    pub committed_executions: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub usage_unknown: bool,
    pub cost_micros: Option<u64>,
    pub cost_estimated: bool,
    pub files_changed: Vec<String>,
    pub memory_hits: u64,
    pub evidence_ref_digests: Vec<String>,
    pub provider_receipt_refs: Vec<String>,
    pub verification: AggregationVerification,
    pub aggregation_digest: String,
}

impl ReceiptAggregation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_cursor: EventCursor,
        source_event_ids: Vec<EventId>,
        model_turns: u64,
        committed_executions: u64,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        usage_unknown: bool,
        cost_micros: Option<u64>,
        cost_estimated: bool,
        mut files_changed: Vec<String>,
        memory_hits: u64,
        mut evidence_ref_digests: Vec<String>,
        mut provider_receipt_refs: Vec<String>,
        verification: AggregationVerification,
    ) -> Result<Self, String> {
        files_changed.sort();
        files_changed.dedup();
        evidence_ref_digests.sort();
        evidence_ref_digests.dedup();
        provider_receipt_refs.sort();
        provider_receipt_refs.dedup();
        let mut aggregation = Self {
            schema: RECEIPT_AGGREGATION_SCHEMA.to_owned(),
            version: RECEIPT_AGGREGATION_VERSION,
            source_cursor,
            source_event_ids: canonical_event_ids(source_event_ids),
            model_turns,
            committed_executions,
            input_tokens,
            output_tokens,
            usage_unknown,
            cost_micros,
            cost_estimated,
            files_changed,
            memory_hits,
            evidence_ref_digests,
            provider_receipt_refs,
            verification,
            aggregation_digest: String::new(),
        };
        aggregation.aggregation_digest = aggregation.digest();
        aggregation.validate()?;
        Ok(aggregation)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let aggregation: Self = serde_json::from_value(value.clone())
            .map_err(|_| "receipt_aggregation_decode_failed".to_owned())?;
        aggregation.validate()?;
        Ok(aggregation)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "receipt_aggregation_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECEIPT_AGGREGATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&RECEIPT_AGGREGATION_VERSION)
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || !unique_event_ids(&self.source_event_ids)
            || self.files_changed.len() > MAX_AGGREGATED_FILES
            || self
                .files_changed
                .iter()
                .any(|path| path.len() > 4096 || normalize_role_path(path).as_deref() != Some(path))
            || self.evidence_ref_digests.len() > MAX_AGGREGATED_REFS
            || self.provider_receipt_refs.len() > MAX_AGGREGATED_REFS
            || self
                .evidence_ref_digests
                .iter()
                .chain(self.provider_receipt_refs.iter())
                .any(|digest| !valid_digest(digest))
            || self.cost_estimated && self.cost_micros.is_none()
            || !valid_digest(&self.aggregation_digest)
        {
            return Err("receipt_aggregation_header_invalid".to_owned());
        }
        if self.aggregation_digest != self.digest() {
            return Err("receipt_aggregation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "model_turns": self.model_turns,
            "committed_executions": self.committed_executions,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "usage_unknown": self.usage_unknown,
            "cost_micros": self.cost_micros,
            "cost_estimated": self.cost_estimated,
            "files_changed": self.files_changed,
            "memory_hits": self.memory_hits,
            "evidence_ref_digests": self.evidence_ref_digests,
            "provider_receipt_refs": self.provider_receipt_refs,
            "verification": self.verification,
        }))
    }
}

fn canonical_event_ids(mut values: Vec<EventId>) -> Vec<EventId> {
    values.sort();
    values.dedup();
    values
}

fn unique_event_ids(values: &[EventId]) -> bool {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .all(|id| !id.as_uuid().is_nil() && seen.insert(*id))
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
