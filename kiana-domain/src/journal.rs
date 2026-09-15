//! Atomic local authority-journal contracts. A committed receipt is not a dispatch receipt.
use crate::{EventId, RequestId, RuntimeEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};

pub const JOURNAL_HEADER_SCHEMA: &str = "kiana.journal-header.v2";
pub const JOURNAL_FRAME_SCHEMA: &str = "kiana.transition-frame.v1";
pub const COMMAND_RECEIPT_SCHEMA: &str = "kiana.command-receipt.v1";
pub const JOURNAL_WRITER_VERSION: u32 = 2;
pub const MAX_JOURNAL_FRAME_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_JOURNAL_EVENT_BYTES: usize = 1024 * 1024;
pub const MAX_TRANSITION_EVENTS: usize = 256;
pub const MAX_TRANSITION_READ_SET: usize = 1024;
pub const MAX_JOURNAL_PAGE_EVENTS: usize = 4096;
pub const MAX_JOURNAL_LOG_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_JOURNAL_EVENTS: usize = 1_000_000;
/// Number of fully committed logical events; zero denotes the beginning of a journal.
pub type EventCursor = u64;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateVersion {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub version: u64,
}
impl AggregateVersion {
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
        version: u64,
    ) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            aggregate_id: aggregate_id.into(),
            version,
        }
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.aggregate_type.trim().is_empty()
            || self.aggregate_type.len() > 128
            || self.aggregate_id.trim().is_empty()
            || self.aggregate_id.len() > 4096
        {
            return Err("journal_aggregate_invalid");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionBatch {
    pub command_id: RequestId,
    /// Digest of the immutable command intent, not a regenerated authorization decision.
    pub command_digest: String,
    /// Every written aggregate and every authority dependency must appear exactly once.
    pub expected_versions: Vec<AggregateVersion>,
    pub events: Vec<RuntimeEvent>,
}
impl TransitionBatch {
    pub fn validate_identity(&self) -> Result<(), &'static str> {
        if self.command_digest.len() != 64
            || !self
                .command_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("journal_command_digest_invalid");
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        self.validate_identity()?;
        if self.events.is_empty() || self.events.len() > MAX_TRANSITION_EVENTS {
            return Err("journal_batch_event_limit");
        }
        if self.expected_versions.is_empty()
            || self.expected_versions.len() > MAX_TRANSITION_READ_SET
        {
            return Err("journal_read_set_limit");
        }
        let mut seen = BTreeSet::new();
        for version in &self.expected_versions {
            version.validate()?;
            if !seen.insert((&version.aggregate_type, &version.aggregate_id)) {
                return Err("journal_read_set_duplicate");
            }
        }
        for event in &self.events {
            if event.sequence == 0 || event.kind.trim().is_empty() || event.kind.len() > 256 {
                return Err("journal_event_invalid");
            }
            let Some(kind) = event.aggregate_type.as_ref() else {
                return Err("journal_event_aggregate_required");
            };
            let Some(id) = event.aggregate_id.as_ref() else {
                return Err("journal_event_aggregate_required");
            };
            if !seen.contains(&(kind, id)) || event.stream_version.is_none_or(|v| v == 0) {
                return Err("journal_write_not_in_read_set");
            }
            if event
                .idempotency_key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty() || key.len() > 4096)
            {
                return Err("journal_event_idempotency_invalid");
            }
            if canonical_journal_bytes(event)
                .map_err(|_| "journal_event_encode_failed")?
                .len()
                > MAX_JOURNAL_EVENT_BYTES
            {
                return Err("journal_event_size_limit");
            }
        }
        if canonical_journal_bytes(self)
            .map_err(|_| "journal_batch_encode_failed")?
            .len()
            > MAX_JOURNAL_FRAME_BYTES
        {
            return Err("journal_frame_size_limit");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandReceipt {
    pub command_id: RequestId,
    pub command_digest: String,
    pub commit_id: EventId,
    pub first_cursor: EventCursor,
    pub cursor: EventCursor,
    pub event_ids: Vec<EventId>,
    /// Actual versions after this commit, including unchanged read dependencies.
    pub versions: Vec<AggregateVersion>,
}

impl CommandReceipt {
    /// Validate that a receipt describes exactly the supplied committed batch.
    pub fn validate_against(&self, batch: &TransitionBatch) -> Result<(), String> {
        batch.validate().map_err(|error| error.to_owned())?;
        if self.command_id != batch.command_id || self.command_digest != batch.command_digest {
            return Err("command_receipt_identity_mismatch".to_owned());
        }
        if self.first_cursor == 0
            || self.cursor < self.first_cursor
            || self.cursor - self.first_cursor + 1 != batch.events.len() as u64
        {
            return Err("command_receipt_cursor_invalid".to_owned());
        }
        let expected_ids = batch
            .events
            .iter()
            .map(|event| event.event_id)
            .collect::<Vec<_>>();
        if self.event_ids != expected_ids {
            return Err("command_receipt_event_ids_mismatch".to_owned());
        }
        if self.event_ids.iter().copied().collect::<HashSet<_>>().len() != self.event_ids.len() {
            return Err("command_receipt_event_ids_duplicate".to_owned());
        }
        for expected in &batch.expected_versions {
            let Some(actual) = self.versions.iter().find(|version| {
                version.aggregate_type == expected.aggregate_type
                    && version.aggregate_id == expected.aggregate_id
            }) else {
                return Err("command_receipt_read_set_missing".to_owned());
            };
            if actual.version < expected.version {
                return Err("command_receipt_version_regressed".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum CommitOutcome {
    Committed {
        receipt: CommandReceipt,
    },
    Replayed {
        original: CommandReceipt,
    },
    Conflict {
        changed: Vec<AggregateVersion>,
    },
    /// No dispatch is permitted until read_command confirms the original command.
    Unknown {
        command_id: RequestId,
        reason: String,
    },
}
impl CommitOutcome {
    pub fn receipt(&self) -> Option<&CommandReceipt> {
        match self {
            Self::Committed { receipt } => Some(receipt),
            Self::Replayed { original } => Some(original),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventStoreCapabilities {
    pub atomic_transitions: bool,
    pub durable_commits: bool,
    pub command_receipts: bool,
    pub cursor_reads: bool,
    pub writer_format_version: u32,
    pub max_frame_bytes: usize,
    pub max_batch_events: usize,
}
impl Default for EventStoreCapabilities {
    fn default() -> Self {
        Self {
            atomic_transitions: false,
            durable_commits: false,
            command_receipts: false,
            cursor_reads: false,
            writer_format_version: 0,
            max_frame_bytes: 0,
            max_batch_events: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalPage {
    pub events: Vec<RuntimeEvent>,
    pub cursor: EventCursor,
    pub has_more: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalHeader {
    pub schema: String,
    pub required: bool,
    pub writer_version: u32,
}
impl Default for JournalHeader {
    fn default() -> Self {
        Self {
            schema: JOURNAL_HEADER_SCHEMA.into(),
            required: true,
            writer_version: JOURNAL_WRITER_VERSION,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JournalFramePayload {
    Transition {
        batch: TransitionBatch,
        receipt: CommandReceipt,
    },
    /// Compatibility calls keep their old event/CAS semantics in a complete v2 frame.
    Event { event: RuntimeEvent },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalFrame {
    pub schema: String,
    pub required: bool,
    pub writer_version: u32,
    pub body_len: u64,
    pub body_sha256: String,
    pub body: JournalFramePayload,
}
impl JournalFrame {
    pub fn new(body: JournalFramePayload) -> Result<Self, String> {
        let encoded = canonical_journal_bytes(&body)?;
        let frame = Self {
            schema: JOURNAL_FRAME_SCHEMA.into(),
            required: true,
            writer_version: JOURNAL_WRITER_VERSION,
            body_len: encoded.len() as u64,
            body_sha256: journal_sha256(&encoded),
            body,
        };
        if canonical_journal_bytes(&frame)?.len() > MAX_JOURNAL_FRAME_BYTES {
            return Err("journal_frame_size_limit".into());
        }
        Ok(frame)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != JOURNAL_FRAME_SCHEMA
            || !self.required
            || self.writer_version != JOURNAL_WRITER_VERSION
        {
            return Err("journal_writer_version_unsupported".into());
        }
        let encoded = canonical_journal_bytes(&self.body)?;
        if encoded.len() as u64 != self.body_len || journal_sha256(&encoded) != self.body_sha256 {
            return Err("journal_frame_integrity_failed".into());
        }
        if canonical_journal_bytes(self)?.len() > MAX_JOURNAL_FRAME_BYTES {
            return Err("journal_frame_size_limit".into());
        }
        Ok(())
    }
}
pub fn journal_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
/// Sort object keys recursively so integrity does not depend on serde_json feature unification.
pub fn canonical_journal_bytes(value: &impl Serialize) -> Result<Vec<u8>, String> {
    fn sorted(value: serde_json::Value, depth: usize) -> Result<serde_json::Value, String> {
        if depth > 128 {
            return Err("journal_payload_depth_limit".into());
        }
        Ok(match value {
            serde_json::Value::Object(fields) => {
                let mut ordered: Vec<_> = fields.into_iter().collect();
                ordered.sort_by(|a, b| a.0.cmp(&b.0));
                let mut result = serde_json::Map::new();
                for (key, value) in ordered {
                    result.insert(key, sorted(value, depth + 1)?);
                }
                serde_json::Value::Object(result)
            }
            serde_json::Value::Array(values) => serde_json::Value::Array(
                values
                    .into_iter()
                    .map(|v| sorted(v, depth + 1))
                    .collect::<Result<_, _>>()?,
            ),
            other => other,
        })
    }
    let value = serde_json::to_value(value).map_err(|e| format!("journal_encode_failed:{e}"))?;
    serde_json::to_vec(&sorted(value, 0)?).map_err(|e| format!("journal_encode_failed:{e}"))
}
