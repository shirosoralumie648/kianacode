//! EventStore facts and deterministic projection contract for Memory JSONL.
//!
//! Memory files are a cache/projection.  A successful memory mutation must first commit one
//! `memory.fact` event to the authority journal; only then may the JSONL row become visible.
//! Replaying the committed stream reconstructs the latest row for each record and exposes a
//! projection lag instead of silently treating a stale file as an empty memory store.

use crate::{json_digest, MemoryRecord, RuntimeEvent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MEMORY_FACT_EVENT_KIND: &str = "memory.fact";
pub const MEMORY_FACT_SCHEMA: &str = "kiana.memory-fact.v1";
pub const MEMORY_BODY_REF_SCHEMA: &str = "kiana.memory-body-ref.v1";
pub const MEMORY_STREAM: &str = "memory";

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Reference to the projected body.  The authority fact carries a bounded record snapshot for
/// deterministic replay; this reference is deliberately stream-relative and contains no host
/// absolute path.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBodyRef {
    pub schema: String,
    pub stream_id: String,
    pub content_hash: String,
}

impl MemoryBodyRef {
    pub fn new(stream_id: impl Into<String>, content_hash: impl Into<String>) -> Self {
        Self {
            schema: MEMORY_BODY_REF_SCHEMA.to_owned(),
            stream_id: stream_id.into(),
            content_hash: content_hash.into(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != MEMORY_BODY_REF_SCHEMA
            || !bounded(&self.stream_id, 256)
            || !hex_digest(&self.content_hash)
        {
            return Err("memory_body_ref_invalid");
        }
        Ok(())
    }
}

/// One server-committed memory mutation/review fact.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryJournalFact {
    pub schema: String,
    pub operation: String,
    pub mutation_key: String,
    pub record: MemoryRecord,
    pub body_ref: MemoryBodyRef,
}

impl MemoryJournalFact {
    pub fn new(
        operation: impl Into<String>,
        mutation_key: impl Into<String>,
        record: MemoryRecord,
        body_ref: MemoryBodyRef,
    ) -> Self {
        Self {
            schema: MEMORY_FACT_SCHEMA.to_owned(),
            operation: operation.into(),
            mutation_key: mutation_key.into(),
            record,
            body_ref,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_FACT_SCHEMA
            || !matches!(self.operation.as_str(), "write" | "review" | "delete")
            || !bounded(&self.mutation_key, 256)
        {
            return Err("memory_fact_invalid".to_owned());
        }
        self.record.validate_lifecycle()?;
        self.body_ref.validate().map_err(str::to_owned)?;
        if self.record.content_hash != self.body_ref.content_hash {
            return Err("memory_body_ref_hash_mismatch".to_owned());
        }
        Ok(())
    }
}

/// Deterministic latest-record projection of a committed memory stream.
#[derive(Clone, Debug, Default)]
pub struct MemoryProjection {
    pub source_cursor: u64,
    pub records: BTreeMap<String, Value>,
}

impl MemoryProjection {
    /// Compare a JSONL projection without making MemoryRecord an equality contract.
    pub fn matches_records(&self, records: &[MemoryRecord]) -> bool {
        let projected = records
            .iter()
            .filter_map(|record| {
                serde_json::to_value(record)
                    .ok()
                    .map(|value| (record.id.clone(), value))
            })
            .collect::<BTreeMap<_, _>>();
        json_digest(&json!(projected)) == json_digest(&json!(self.records))
    }
}

/// Rebuild the latest memory rows from committed `memory.fact` events.
pub fn project_memory_facts(events: &[RuntimeEvent]) -> Result<MemoryProjection, String> {
    let mut projection = MemoryProjection::default();
    let mut ids = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut expected = 1u64;
    for event in events {
        if event.kind != MEMORY_FACT_EVENT_KIND {
            return Err("memory_stream_event_kind_invalid".to_owned());
        }
        if event.aggregate_type.as_deref() != Some(MEMORY_STREAM)
            || event.aggregate_id.as_deref().is_none()
            || event.stream_version != Some(expected)
        {
            return Err("memory_stream_version_invalid".to_owned());
        }
        if !ids.insert(event.event_id) {
            return Err("memory_stream_event_duplicate".to_owned());
        }
        let key = event
            .idempotency_key
            .as_deref()
            .ok_or_else(|| "memory_stream_idempotency_missing".to_owned())?;
        if !keys.insert(key.to_owned()) {
            return Err("memory_stream_idempotency_duplicate".to_owned());
        }
        let fact: MemoryJournalFact = serde_json::from_value(event.data.clone())
            .map_err(|_| "memory_fact_decode_failed".to_owned())?;
        fact.validate()?;
        if fact.body_ref.stream_id != event.aggregate_id.clone().unwrap_or_default() {
            return Err("memory_body_ref_stream_mismatch".to_owned());
        }
        projection.records.insert(
            fact.record.id.clone(),
            serde_json::to_value(fact.record)
                .map_err(|_| "memory_fact_encode_failed".to_owned())?,
        );
        projection.source_cursor = expected;
        expected = expected
            .checked_add(1)
            .ok_or_else(|| "memory_stream_version_exhausted".to_owned())?;
    }
    Ok(projection)
}
