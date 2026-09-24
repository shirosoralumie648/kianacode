//! Shared deterministic transaction planning and complete-frame replay.
use crate::event_store_core::{
    plan_append, plan_idempotent_append, validate_event_for_storage, AppendPlan,
};
use kiana_domain::*;
use kiana_ports::{EventAppendResult, PortError};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub(crate) struct JournalState {
    pub events: Vec<RuntimeEvent>,
    pub commands: HashMap<RequestId, CommandReceipt>,
    versions: BTreeMap<(String, String), u64>,
    event_ids: HashSet<EventId>,
    keys: HashMap<String, usize>,
    streams: HashMap<(String, String), Vec<usize>>,
    requests: HashMap<RequestId, Vec<usize>>,
    boundaries: Vec<u64>,
}
#[derive(Debug)]
pub(crate) enum TransitionPlan {
    Append {
        batch: TransitionBatch,
        receipt: CommandReceipt,
    },
    Replay(CommandReceipt),
    Conflict(Vec<AggregateVersion>),
}
fn invalid(reason: impl Into<String>) -> PortError {
    PortError::Failed(reason.into())
}
fn conflict(reason: &str) -> PortError {
    PortError::Conflict(reason.into())
}
impl JournalState {
    pub fn plan_transition(
        &self,
        batch: TransitionBatch,
        commit_id: EventId,
    ) -> Result<TransitionPlan, PortError> {
        batch.validate_identity().map_err(invalid)?;
        for event in &batch.events {
            validate_event_for_storage(event)?;
        }
        if let Some(original) = self.commands.get(&batch.command_id) {
            if original.command_digest != batch.command_digest {
                return Err(conflict("event_store_command_digest_mismatch"));
            }
            return Ok(TransitionPlan::Replay(original.clone()));
        }
        batch.validate().map_err(invalid)?;
        if self.events.len().saturating_add(batch.events.len()) > MAX_JOURNAL_EVENTS {
            return Err(invalid("eventlog_event_limit"));
        }
        let mut changed = Vec::new();
        let mut versions = BTreeMap::new();
        for expected in &batch.expected_versions {
            let key = (
                expected.aggregate_type.clone(),
                expected.aggregate_id.clone(),
            );
            let actual = self.versions.get(&key).copied().unwrap_or(0);
            if actual != expected.version {
                changed.push(AggregateVersion::new(&key.0, &key.1, actual));
            }
            versions.insert(key, actual);
        }
        if !changed.is_empty() {
            changed.sort();
            return Ok(TransitionPlan::Conflict(changed));
        }
        let mut new_ids = HashSet::new();
        let mut new_keys = HashSet::new();
        for event in &batch.events {
            if self.event_ids.contains(&event.event_id) || !new_ids.insert(event.event_id) {
                return Err(conflict("event_id_duplicate"));
            }
            if let Some(key) = &event.idempotency_key {
                if self.keys.contains_key(key) || !new_keys.insert(key) {
                    return Err(conflict("event_idempotency_key_duplicate"));
                }
            }
            let key = (
                event.aggregate_type.clone().expect("validated aggregate"),
                event.aggregate_id.clone().expect("validated aggregate"),
            );
            let version = versions.get_mut(&key).expect("validated read set");
            *version = version
                .checked_add(1)
                .ok_or_else(|| invalid("event_stream_version_exhausted"))?;
            if event.stream_version != Some(*version) {
                return Err(conflict("event_stream_version_not_contiguous"));
            }
        }
        let receipt = CommandReceipt {
            command_id: batch.command_id,
            command_digest: batch.command_digest.clone(),
            commit_id,
            first_cursor: self.events.len() as u64 + 1,
            cursor: (self.events.len() + batch.events.len()) as u64,
            event_ids: batch.events.iter().map(|e| e.event_id).collect(),
            versions: versions
                .into_iter()
                .map(|((kind, id), version)| AggregateVersion::new(kind, id, version))
                .collect(),
        };
        // Memory and JSONL enforce the same encoded frame bound before exposing any event.
        JournalFrame::new(JournalFramePayload::Transition {
            batch: batch.clone(),
            receipt: receipt.clone(),
        })
        .map_err(invalid)?;
        Ok(TransitionPlan::Append { batch, receipt })
    }
    pub fn apply_transition(&mut self, batch: TransitionBatch, receipt: CommandReceipt) {
        for event in batch.events {
            self.index_event(event);
        }
        self.boundaries.push(self.events.len() as u64);
        self.commands.insert(receipt.command_id, receipt);
    }
    pub fn plan_legacy(
        &self,
        event: RuntimeEvent,
        expected: Option<u64>,
        idempotent: bool,
    ) -> Result<AppendPlan, PortError> {
        if event.sequence == 0 || event.kind.is_empty() {
            return Err(invalid("eventlog_event_invalid"));
        }
        if canonical_journal_bytes(&event).map_err(invalid)?.len() > MAX_JOURNAL_EVENT_BYTES {
            return Err(invalid("eventlog_event_size_limit"));
        }
        let plan = if idempotent {
            plan_idempotent_append(&self.events, event, expected)?
        } else {
            AppendPlan::Append(plan_append(&self.events, event, expected)?)
        };
        if matches!(plan, AppendPlan::Append(_)) && self.events.len() >= MAX_JOURNAL_EVENTS {
            return Err(invalid("eventlog_event_limit"));
        }
        if let AppendPlan::Append(event) = &plan {
            if let Some(key) = &event.idempotency_key {
                if self.keys.contains_key(key) {
                    return Err(conflict("event_idempotency_key_duplicate"));
                }
            }
        }
        Ok(plan)
    }
    pub fn apply_legacy(&mut self, event: RuntimeEvent) {
        self.index_event(event);
        self.boundaries.push(self.events.len() as u64);
    }
    fn index_event(&mut self, event: RuntimeEvent) {
        let index = self.events.len();
        self.event_ids.insert(event.event_id);
        if let Some(key) = &event.idempotency_key {
            self.keys.insert(key.clone(), index);
        }
        self.requests
            .entry(event.request_id)
            .or_default()
            .push(index);
        if let (Some(kind), Some(id)) = (&event.aggregate_type, &event.aggregate_id) {
            let key = (kind.clone(), id.clone());
            self.versions
                .insert(key.clone(), event.stream_version.unwrap_or(event.sequence));
            self.streams.entry(key).or_default().push(index);
        }
        self.events.push(event);
    }
    pub fn accept_frame(&mut self, frame: JournalFrame) -> Result<(), PortError> {
        frame.validate().map_err(invalid)?;
        match frame.body {
            JournalFramePayload::Transition { batch, receipt } => {
                match self.plan_transition(batch, receipt.commit_id)? {
                    TransitionPlan::Append {
                        batch,
                        receipt: expected,
                    } if receipt == expected => {
                        self.apply_transition(batch, receipt);
                        Ok(())
                    }
                    _ => Err(invalid("eventlog_transaction_replay_conflict")),
                }
            }
            JournalFramePayload::Event { event } => {
                let AppendPlan::Append(event) = self.plan_legacy(event, None, false)? else {
                    return Err(invalid("eventlog_legacy_replay_conflict"));
                };
                self.apply_legacy(event);
                Ok(())
            }
        }
    }
    pub fn request(&self, id: &RequestId) -> Vec<RuntimeEvent> {
        self.requests
            .get(id)
            .into_iter()
            .flatten()
            .map(|i| self.events[*i].clone())
            .collect()
    }
    pub fn stream(&self, kind: &str, id: &str) -> Vec<RuntimeEvent> {
        self.streams
            .get(&(kind.into(), id.into()))
            .into_iter()
            .flatten()
            .map(|i| self.events[*i].clone())
            .collect()
    }
    pub fn page(&self, cursor: u64, limit: usize) -> Result<JournalPage, PortError> {
        if limit == 0 || limit > MAX_JOURNAL_PAGE_EVENTS {
            return Err(invalid("eventlog_page_limit"));
        }
        if cursor > self.events.len() as u64
            || (cursor != 0 && self.boundaries.binary_search(&cursor).is_err())
        {
            return Err(conflict("eventlog_cursor_not_commit_boundary"));
        }
        let mut end = cursor;
        let start = self
            .boundaries
            .partition_point(|boundary| *boundary <= cursor);
        for boundary in self.boundaries[start..].iter().copied() {
            if end != cursor && boundary - cursor > limit as u64 {
                break;
            }
            end = boundary;
            if end - cursor >= limit as u64 {
                break;
            }
        }
        Ok(JournalPage {
            events: self.events[cursor as usize..end as usize].to_vec(),
            cursor: end,
            has_more: end < self.events.len() as u64,
        })
    }
}
pub(crate) fn append_result(event: RuntimeEvent, replayed: bool) -> EventAppendResult {
    EventAppendResult { event, replayed }
}
pub(crate) fn capabilities(durable: bool) -> EventStoreCapabilities {
    EventStoreCapabilities {
        atomic_transitions: true,
        durable_commits: durable,
        command_receipts: true,
        cursor_reads: true,
        writer_format_version: JOURNAL_WRITER_VERSION,
        max_frame_bytes: MAX_JOURNAL_FRAME_BYTES,
        max_batch_events: MAX_TRANSITION_EVENTS,
    }
}
