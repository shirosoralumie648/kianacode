//! EventStore facts and deterministic projection contract for Memory JSONL.
//!
//! Memory files are a cache/projection.  A successful memory mutation must first commit one
//! `memory.fact` event to the authority journal; only then may the JSONL row become visible.
//! Replaying the committed stream reconstructs the latest row for each record and exposes a
//! projection lag instead of silently treating a stale file as an empty memory store.

use crate::{
    json_digest, MemoryAdmission, MemoryImportMode, MemoryMutation, MemoryMutationAuthority,
    MemoryMutationOperation, MemoryOrigin, MemoryRecord, MemoryState, RuntimeEvent,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MEMORY_FACT_EVENT_KIND: &str = "memory.fact";
pub const MEMORY_FACT_SCHEMA: &str = "kiana.memory-fact.v1";
pub const MEMORY_BODY_REF_SCHEMA: &str = "kiana.memory-body-ref.v1";
pub const MEMORY_MUTATION_JOURNAL_SCHEMA: &str = "kiana.memory-mutation-journal.v1";
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMutationJournalStage {
    Candidate,
    Draft,
    Ephemeral,
    Qualify,
    Approve,
    Supersede,
    Tombstone,
}

impl MemoryMutationJournalStage {
    fn validate_record(
        self,
        mutation: &MemoryMutation,
        record: &MemoryRecord,
    ) -> Result<(), String> {
        let expected = mutation
            .expected_revisions
            .first()
            .ok_or_else(|| "memory_mutation_journal_target_missing".to_owned())?;
        if mutation.expected_revisions.len() != 1
            || expected.record_id != record.id
            || expected.collection != record.collection
            || record.last_mutation_key.as_deref() != Some(mutation.idempotency_key.as_str())
            || record.revision != expected.expected_revision.saturating_add(1)
        {
            return Err("memory_mutation_journal_target_mismatch".to_owned());
        }
        if record.import_mode == MemoryImportMode::Native && record.origin == MemoryOrigin::Unknown
        {
            return Err("memory_mutation_journal_origin_unresolved".to_owned());
        }
        match self {
            Self::Candidate | Self::Draft => {
                if !matches!(
                    (record.admission_state, record.state),
                    (MemoryAdmission::Candidate, MemoryState::Draft)
                ) || !matches!(
                    mutation.operation,
                    MemoryMutationOperation::Add | MemoryMutationOperation::Update
                ) {
                    return Err("memory_mutation_journal_candidate_invalid".to_owned());
                }
            }
            Self::Ephemeral => {
                if !matches!(
                    (record.admission_state, record.state),
                    (MemoryAdmission::Ephemeral, MemoryState::Active)
                ) {
                    return Err("memory_mutation_journal_ephemeral_invalid".to_owned());
                }
            }
            Self::Qualify | Self::Approve => {
                if !matches!(
                    (record.admission_state, record.state),
                    (MemoryAdmission::Qualified, MemoryState::Active)
                ) || !matches!(
                    mutation.operation,
                    MemoryMutationOperation::Approve | MemoryMutationOperation::Publish
                ) || mutation.authority == MemoryMutationAuthority::Agent
                    || record.reviewed_by.is_none()
                    || record.reviewed_at_ms.is_none()
                {
                    return Err("memory_mutation_journal_approval_invalid".to_owned());
                }
            }
            Self::Supersede => {
                if record.supersedes.is_none()
                    || !matches!(
                        mutation.operation,
                        MemoryMutationOperation::Update | MemoryMutationOperation::Publish
                    )
                    || mutation.authority == MemoryMutationAuthority::Agent
                {
                    return Err("memory_mutation_journal_supersede_invalid".to_owned());
                }
            }
            Self::Tombstone => {
                if !matches!(
                    (record.admission_state, record.state),
                    (MemoryAdmission::Rejected, MemoryState::Rejected)
                ) || !matches!(
                    mutation.operation,
                    MemoryMutationOperation::Delete
                        | MemoryMutationOperation::Expire
                        | MemoryMutationOperation::Revoke
                ) || mutation.authority == MemoryMutationAuthority::Agent
                {
                    return Err("memory_mutation_journal_tombstone_invalid".to_owned());
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationJournal {
    pub schema: String,
    pub mutation: MemoryMutation,
    pub stage: MemoryMutationJournalStage,
    pub mutation_digest: String,
}

impl MemoryMutationJournal {
    pub fn new(
        mutation: MemoryMutation,
        stage: MemoryMutationJournalStage,
    ) -> Result<Self, String> {
        mutation.validate()?;
        let journal = Self {
            schema: MEMORY_MUTATION_JOURNAL_SCHEMA.to_owned(),
            mutation_digest: mutation.digest(),
            mutation,
            stage,
        };
        journal.validate()?;
        Ok(journal)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_MUTATION_JOURNAL_SCHEMA
            || self.mutation_digest != self.mutation.digest()
        {
            return Err("memory_mutation_journal_header_invalid".to_owned());
        }
        self.mutation.validate()
    }

    pub fn validate_for_record(&self, record: &MemoryRecord) -> Result<(), String> {
        self.validate()?;
        record.validate_lifecycle()?;
        self.stage.validate_record(&self.mutation, record)
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation: Option<MemoryMutationJournal>,
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
            mutation: None,
        }
    }

    pub fn with_mutation(
        mut self,
        mutation: MemoryMutation,
        stage: MemoryMutationJournalStage,
    ) -> Result<Self, String> {
        let journal = MemoryMutationJournal::new(mutation, stage)?;
        journal.validate_for_record(&self.record)?;
        self.mutation = Some(journal);
        Ok(self)
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
        if let Some(mutation) = &self.mutation {
            mutation.validate_for_record(&self.record)?;
            if mutation.mutation.idempotency_key != self.mutation_key {
                return Err("memory_mutation_journal_key_mismatch".to_owned());
            }
        }
        Ok(())
    }
}

/// Deterministic latest-record projection of a committed memory stream.
#[derive(Clone, Debug, Default)]
pub struct MemoryProjection {
    pub source_cursor: u64,
    pub records: BTreeMap<String, Value>,
    pub mutation_digests: BTreeMap<String, String>,
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
    let mut mutation_keys = BTreeSet::new();
    let mut previous_records = BTreeMap::new();
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
        if let Some(mutation) = &fact.mutation {
            if !mutation_keys.insert(mutation.mutation.idempotency_key.clone()) {
                return Err("memory_mutation_journal_duplicate".to_owned());
            }
            mutation.validate_for_record(&fact.record)?;
            if let Some(previous) = previous_records.get(&fact.record.id) {
                if previous.revision.saturating_add(1) != fact.record.revision {
                    return Err("memory_mutation_journal_revision_gap".to_owned());
                }
            }
            projection.mutation_digests.insert(
                mutation.mutation.idempotency_key.clone(),
                mutation.mutation_digest.clone(),
            );
        }
        previous_records.insert(fact.record.id.clone(), fact.record.clone());
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
