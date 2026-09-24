//! BQ-20 read-only EventLog billing projector.
//!
//! The projector consumes only committed BQ-13 cost observations, BQ-14 ledger/correction facts
//! and BQ-19 allocation facts. Reservation and approval events are intentionally ignored. A page
//! is folded into a candidate state first; the source cursor is changed only when that candidate
//! validates, so a failed page cannot advance the projection.

use kiana_domain::{
    json_digest, AttemptId, BillingProjectionCursor, BillingProjectionSnapshot,
    BillingQuarantineRecord, BillingRollupTotals, CostAllocation, CostBreakdown, CostBreakdownKind,
    CostCorrection, CostLedgerEntry, EventCursor, EventId, EventStoreHealth, JournalPage,
    LedgerEntryId, RuntimeEvent, SchemaVersion, UsageId, BILLING_PROJECTION_NUMBER,
    BILLING_WINDOW_MILLIS, COST_ALLOCATION_EVENT, COST_CORRECTION_EVENT, COST_EVENT_ESTIMATED,
    COST_EVENT_MEASURED, COST_EVENT_UNKNOWN, COST_LEDGER_ENTRY_EVENT,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const BILLING_LEDGER_PROJECTOR_SCHEMA: &str = "kiana.billing-ledger-projector.v1";
pub const BILLING_LEDGER_PROJECTOR_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const BILLING_LEDGER_PROJECTOR_IS_READ_ONLY: bool = true;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BillingLedgerProjectionError {
    #[error("billing_projection_source_epoch_invalid")]
    SourceEpochInvalid,
    #[error("billing_projection_source_cursor_invalid")]
    SourceCursorInvalid,
    #[error("billing_projection_source_cursor_gap")]
    SourceCursorGap,
    #[error("billing_projection_source_replay")]
    SourceReplay,
    #[error("billing_projection_source_event_duplicate")]
    SourceEventDuplicate,
    #[error("billing_projection_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
    #[error("billing_projection_invalid:{0}")]
    Invalid(String),
}

/// A RuntimeEvent paired with its EventLog logical cursor. RuntimeEvent itself only carries a
/// request-local sequence, which must never be mistaken for the global source cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct BillingSourceEvent {
    pub source_cursor: EventCursor,
    pub event: RuntimeEvent,
}

impl BillingSourceEvent {
    pub fn new(source_cursor: EventCursor, event: RuntimeEvent) -> Result<Self, String> {
        if source_cursor == 0 || event.event_id.as_uuid().is_nil() {
            return Err("billing_source_event_invalid".to_owned());
        }
        Ok(Self {
            source_cursor,
            event,
        })
    }
}

#[derive(Clone)]
struct UsageFact {
    cursor: EventCursor,
    revision: u64,
    fact: CostBreakdown,
    occurred_at_unix_ms: Option<u64>,
}

#[derive(Clone)]
struct LedgerFact {
    cursor: EventCursor,
    fact: CostLedgerEntry,
}

#[derive(Clone)]
struct AllocationFact {
    cursor: EventCursor,
    fact: CostAllocation,
}

#[derive(Clone)]
struct CorrectionFact {
    cursor: EventCursor,
    fact: CostCorrection,
}

/// Stateful source projector. Its state is deliberately private; callers receive a signed,
/// serializable snapshot and can rebuild this object from the EventLog in a new process.
#[derive(Clone)]
pub struct BillingLedgerProjector {
    source_epoch: u64,
    snapshot: BillingProjectionSnapshot,
    seen_event_ids: BTreeSet<EventId>,
    usage: BTreeMap<AttemptId, UsageFact>,
    ledger: BTreeMap<LedgerEntryId, LedgerFact>,
    allocations: BTreeMap<UsageId, AllocationFact>,
    corrections: BTreeMap<kiana_domain::CostCorrectionId, CorrectionFact>,
}

impl std::fmt::Debug for BillingLedgerProjector {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BillingLedgerProjector")
            .field("source_epoch", &self.source_epoch)
            .field("source_cursor", &self.snapshot.source.source_cursor)
            .field("quarantine", &self.snapshot.quarantine.len())
            .finish_non_exhaustive()
    }
}

impl BillingLedgerProjector {
    pub fn new(source_epoch: u64) -> Result<Self, BillingLedgerProjectionError> {
        if source_epoch == 0 {
            return Err(BillingLedgerProjectionError::SourceEpochInvalid);
        }
        let snapshot = BillingProjectionSnapshot::empty(source_epoch)
            .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        Ok(Self {
            source_epoch,
            snapshot,
            seen_event_ids: BTreeSet::new(),
            usage: BTreeMap::new(),
            ledger: BTreeMap::new(),
            allocations: BTreeMap::new(),
            corrections: BTreeMap::new(),
        })
    }

    pub fn from_snapshot(
        snapshot: BillingProjectionSnapshot,
    ) -> Result<Self, BillingLedgerProjectionError> {
        snapshot
            .validate()
            .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        // A serialized read model is safe to inspect, but incremental folding requires the source
        // facts needed to supersede an earlier usage revision. Rebuild from EventLog before
        // applying a tail rather than guessing those facts from aggregate totals.
        Err(BillingLedgerProjectionError::Invalid(
            "billing_projection_snapshot_requires_rebuild".to_owned(),
        ))
    }

    pub fn snapshot(&self) -> &BillingProjectionSnapshot {
        &self.snapshot
    }

    pub fn source_cursor(&self) -> EventCursor {
        self.snapshot.source.source_cursor
    }

    pub fn source_epoch(&self) -> u64 {
        self.source_epoch
    }

    /// Apply one complete EventLog page. All folds happen against a clone and are published as a
    /// single snapshot assignment; cursor publication therefore follows projection publication.
    pub fn apply_page(
        &mut self,
        page: &kiana_domain::JournalPage,
        source_epoch: u64,
    ) -> Result<(), BillingLedgerProjectionError> {
        if source_epoch == 0 || source_epoch != self.source_epoch {
            return Err(BillingLedgerProjectionError::SourceEpochInvalid);
        }
        let after = self.source_cursor();
        if page.cursor < after {
            return Err(BillingLedgerProjectionError::SourceReplay);
        }
        if page.events.is_empty() {
            if page.cursor != after {
                return Err(BillingLedgerProjectionError::SourceCursorInvalid);
            }
            return Ok(());
        }
        let first_cursor = after
            .checked_add(1)
            .ok_or(BillingLedgerProjectionError::SourceCursorInvalid)?;
        let expected_last = first_cursor
            .checked_add(page.events.len() as u64 - 1)
            .ok_or(BillingLedgerProjectionError::SourceCursorInvalid)?;
        if page.cursor != expected_last {
            return Err(BillingLedgerProjectionError::SourceCursorGap);
        }
        let mut candidate = self.clone();
        for (offset, event) in page.events.iter().cloned().enumerate() {
            let cursor = first_cursor + offset as u64;
            candidate.apply_event(cursor, event)?;
        }
        candidate.rebuild_snapshot(page.cursor)?;
        *self = candidate;
        Ok(())
    }

    /// Convenience API for a full read-from-zero rebuild. The supplied cursor must be the
    /// EventLog cursor at the end of the supplied contiguous event list.
    pub fn rebuild(
        source_epoch: u64,
        source_cursor: EventCursor,
        events: &[RuntimeEvent],
    ) -> Result<BillingProjectionSnapshot, BillingLedgerProjectionError> {
        if events.is_empty() {
            if source_cursor != 0 {
                return Err(BillingLedgerProjectionError::SourceCursorInvalid);
            }
            return Self::new(source_epoch).map(|projector| projector.snapshot.clone());
        }
        let mut projector = Self::new(source_epoch)?;
        let page = JournalPage {
            events: events.to_vec(),
            cursor: source_cursor,
            has_more: false,
        };
        projector.apply_page(&page, source_epoch)?;
        Ok(projector.snapshot.clone())
    }

    fn apply_event(
        &mut self,
        source_cursor: EventCursor,
        event: RuntimeEvent,
    ) -> Result<(), BillingLedgerProjectionError> {
        if !self.seen_event_ids.insert(event.event_id) {
            return Err(BillingLedgerProjectionError::SourceEventDuplicate);
        }
        let fence = next_replay_fence(
            &self.snapshot.source.replay_fence,
            source_cursor,
            event.event_id,
        );
        self.snapshot.source.replay_fence = fence;
        self.snapshot.source.source_cursor = source_cursor;
        self.snapshot.source.source_event_ids.push(event.event_id);
        if self.snapshot.source.source_event_ids.len() > kiana_domain::MAX_BILLING_SOURCE_EVENT_IDS
        {
            let trim = self.snapshot.source.source_event_ids.len()
                - kiana_domain::MAX_BILLING_SOURCE_EVENT_IDS;
            self.snapshot.source.source_event_ids.drain(..trim);
        }

        match event.kind.as_str() {
            COST_EVENT_ESTIMATED | COST_EVENT_MEASURED | COST_EVENT_UNKNOWN => {
                if let Err(reason) = self.fold_usage(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            COST_LEDGER_ENTRY_EVENT => {
                if let Err(reason) = self.fold_ledger(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            COST_CORRECTION_EVENT => {
                if let Err(reason) = self.fold_correction(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            COST_ALLOCATION_EVENT => {
                if let Err(reason) = self.fold_allocation(source_cursor, &event) {
                    self.quarantine(source_cursor, &event, reason)?;
                }
            }
            // Reservations, approval lifecycle and all other RuntimeEvents are not billing facts
            // consumed by this projection. Ignoring them is deliberate and does not erase them.
            _ => {}
        }
        Ok(())
    }

    fn fold_usage(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostBreakdown = serde_json::from_value(event.data.clone())
            .map_err(|_| "usage_cost_decode_failed".to_owned())?;
        fact.validate()?;
        if fact.kind.event_kind() != event.kind {
            return Err("usage_cost_kind_mismatch".to_owned());
        }
        let attempt_id = fact
            .attempt_id
            .ok_or_else(|| "usage_cost_attempt_missing".to_owned())?;
        if event.aggregate_type.as_deref() != Some("billing_attempt")
            || event.aggregate_id.as_deref() != Some(attempt_id.to_string().as_str())
        {
            return Err("usage_cost_stream_mismatch".to_owned());
        }
        let revision = event
            .stream_version
            .filter(|revision| *revision > 0)
            .ok_or_else(|| "usage_cost_revision_missing".to_owned())?;
        if let Some(previous) = self.usage.get(&attempt_id) {
            if revision != previous.revision.saturating_add(1)
                || previous.fact.usage_digest != fact.usage_digest
            {
                return Err("usage_cost_revision_gap".to_owned());
            }
            let allowed = matches!(
                (&previous.fact.kind, &fact.kind),
                (
                    CostBreakdownKind::Unknown { .. },
                    CostBreakdownKind::Estimated { .. }
                ) | (
                    CostBreakdownKind::Unknown { .. },
                    CostBreakdownKind::Measured { .. }
                ) | (
                    CostBreakdownKind::Estimated { .. },
                    CostBreakdownKind::Measured { .. }
                )
            );
            if !allowed {
                return Err("usage_cost_transition_invalid".to_owned());
            }
        }
        let occurred_at_unix_ms = event_time(&event.data);
        self.usage.insert(
            attempt_id,
            UsageFact {
                cursor,
                revision,
                fact,
                occurred_at_unix_ms,
            },
        );
        Ok(())
    }

    fn fold_ledger(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostLedgerEntry = serde_json::from_value(event.data.clone())
            .map_err(|_| "ledger_entry_decode_failed".to_owned())?;
        fact.validate()?;
        if event.aggregate_type.as_deref() != Some("cost_ledger")
            || event.aggregate_id.as_deref() != Some(fact.entry_id.to_string().as_str())
            || event.stream_version != Some(fact.revision)
        {
            return Err("ledger_entry_stream_mismatch".to_owned());
        }
        if self.ledger.contains_key(&fact.entry_id) {
            return Err("ledger_entry_duplicate".to_owned());
        }
        self.ledger
            .insert(fact.entry_id, LedgerFact { cursor, fact });
        Ok(())
    }

    fn fold_correction(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostCorrection = serde_json::from_value(event.data.clone())
            .map_err(|_| "correction_decode_failed".to_owned())?;
        fact.validate()?;
        if event.aggregate_type.as_deref() != Some("cost_ledger")
            || event.aggregate_id.as_deref() != Some(fact.target_entry_id.to_string().as_str())
            || event.stream_version.is_none_or(|version| version == 0)
        {
            return Err("correction_stream_mismatch".to_owned());
        }
        let Some(target) = self.ledger.get(&fact.target_entry_id) else {
            return Err("correction_target_missing".to_owned());
        };
        if fact.target_entry_digest != target.fact.entry_digest
            || fact.target_run_id != target.fact.run_id
            || fact.target_source_cursor != target.fact.source_cursor
            || fact.target_revision != target.fact.revision
        {
            return Err("correction_target_mismatch".to_owned());
        }
        if self.corrections.contains_key(&fact.correction_id) {
            return Err("correction_duplicate".to_owned());
        }
        self.corrections
            .insert(fact.correction_id, CorrectionFact { cursor, fact });
        Ok(())
    }

    fn fold_allocation(&mut self, cursor: EventCursor, event: &RuntimeEvent) -> Result<(), String> {
        let fact: CostAllocation = serde_json::from_value(event.data.clone())
            .map_err(|_| "allocation_decode_failed".to_owned())?;
        fact.validate()?;
        if event.aggregate_type.as_deref() != Some("cost_allocation")
            || event.aggregate_id.as_deref() != Some(fact.allocation_id.to_string().as_str())
            || event.stream_version != Some(fact.revision)
        {
            return Err("allocation_stream_mismatch".to_owned());
        }
        if self.allocations.contains_key(&fact.usage_id) {
            return Err("allocation_usage_duplicate".to_owned());
        }
        self.allocations
            .insert(fact.usage_id, AllocationFact { cursor, fact });
        Ok(())
    }

    fn quarantine(
        &mut self,
        source_cursor: EventCursor,
        event: &RuntimeEvent,
        reason: String,
    ) -> Result<(), BillingLedgerProjectionError> {
        if self.snapshot.quarantine.len() >= kiana_domain::MAX_BILLING_QUARANTINE {
            return Err(BillingLedgerProjectionError::Invalid(
                "billing_quarantine_limit".to_owned(),
            ));
        }
        let record = BillingQuarantineRecord::new(
            source_cursor,
            event.event_id,
            event.kind.clone(),
            reason,
            json_digest(&event.data),
        )
        .map_err(BillingLedgerProjectionError::Invalid)?;
        self.snapshot.quarantine.push(record);
        Ok(())
    }

    fn rebuild_snapshot(
        &mut self,
        source_cursor: EventCursor,
    ) -> Result<(), BillingLedgerProjectionError> {
        let mut usage_totals = BillingRollupTotals::default();
        let mut ledger_totals = BillingRollupTotals::default();
        let mut allocation_totals = BillingRollupTotals::default();
        let mut daily_rollups: BTreeMap<String, BillingRollupTotals> = BTreeMap::new();
        let mut window_rollups: BTreeMap<u64, BillingRollupTotals> = BTreeMap::new();

        for usage in self.usage.values() {
            usage_totals.usage_count = usage_totals.usage_count.saturating_add(1);
            add_cost_kind(&mut usage_totals, &usage.fact.kind, false)?;
            if let Some(timestamp) = usage.occurred_at_unix_ms {
                add_bucket(
                    &mut daily_rollups,
                    &mut window_rollups,
                    timestamp,
                    |totals| {
                        totals.usage_count = totals.usage_count.saturating_add(1);
                        add_cost_kind(totals, &usage.fact.kind, false)
                    },
                )?;
            }
        }
        for ledger in self.ledger.values() {
            ledger_totals.ledger_entry_count = ledger_totals.ledger_entry_count.saturating_add(1);
            add_ledger_kind(&mut ledger_totals, &ledger.fact, false)?;
            add_bucket(
                &mut daily_rollups,
                &mut window_rollups,
                ledger.fact.created_at_unix_ms,
                |totals| {
                    totals.ledger_entry_count = totals.ledger_entry_count.saturating_add(1);
                    add_ledger_kind(totals, &ledger.fact, false)
                },
            )?;
        }
        for correction in self.corrections.values() {
            add_correction_kind(&mut ledger_totals, &correction.fact)?;
            add_bucket(
                &mut daily_rollups,
                &mut window_rollups,
                correction.fact.created_at_unix_ms,
                |totals| add_correction_kind(totals, &correction.fact),
            )?;
        }
        for allocation in self.allocations.values() {
            allocation_totals.allocation_count =
                allocation_totals.allocation_count.saturating_add(1);
            add_allocation_kind(&mut allocation_totals, &allocation.fact.cost)?;
        }

        let mut ids = self.snapshot.source.source_event_ids.clone();
        ids.dedup();
        let source = BillingProjectionCursor::new(
            self.source_epoch,
            source_cursor,
            BILLING_PROJECTION_NUMBER,
            self.snapshot.source.replay_fence.clone(),
            ids,
        )
        .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        self.snapshot = BillingProjectionSnapshot::new(
            source,
            usage_totals,
            ledger_totals,
            allocation_totals,
            daily_rollups,
            window_rollups,
            self.snapshot.quarantine.clone(),
        )
        .map_err(BillingLedgerProjectionError::SnapshotInvalid)?;
        Ok(())
    }
}

fn add_cost_kind(
    totals: &mut BillingRollupTotals,
    kind: &CostBreakdownKind,
    correction: bool,
) -> Result<(), BillingLedgerProjectionError> {
    match kind {
        CostBreakdownKind::Estimated { estimate } => {
            let amount = estimate.amount.as_ref().ok_or_else(|| {
                BillingLedgerProjectionError::Invalid("estimated_amount_missing".to_owned())
            })?;
            if correction {
                totals
                    .add_correction_estimated(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            } else {
                totals
                    .add_estimated(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            }
        }
        CostBreakdownKind::Measured { amount, .. } => {
            if correction {
                totals
                    .add_correction_measured(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            } else {
                totals
                    .add_measured(amount)
                    .map_err(BillingLedgerProjectionError::Invalid)
            }
        }
        CostBreakdownKind::Unknown { .. } => {
            totals.unknown_count = totals.unknown_count.saturating_add(1);
            Ok(())
        }
    }
}

fn add_ledger_kind(
    totals: &mut BillingRollupTotals,
    fact: &CostLedgerEntry,
    correction: bool,
) -> Result<(), BillingLedgerProjectionError> {
    if let Some(amount) = fact.estimated_cost.as_ref() {
        if correction {
            totals
                .add_correction_estimated(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        } else {
            totals
                .add_estimated(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        }
    }
    if let Some(amount) = fact.measured_cost.as_ref() {
        if correction {
            totals
                .add_correction_measured(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        } else {
            totals
                .add_measured(amount)
                .map_err(BillingLedgerProjectionError::Invalid)?;
        }
    }
    if fact.estimated_cost.is_none() && fact.measured_cost.is_none() {
        totals.unknown_count = totals.unknown_count.saturating_add(1);
    }
    Ok(())
}

fn add_correction_kind(
    totals: &mut BillingRollupTotals,
    fact: &CostCorrection,
) -> Result<(), BillingLedgerProjectionError> {
    if let Some(amount) = fact.delta_estimated.as_ref() {
        totals
            .add_correction_estimated(amount)
            .map_err(BillingLedgerProjectionError::Invalid)?;
    }
    if let Some(amount) = fact.delta_measured.as_ref() {
        totals
            .add_correction_measured(amount)
            .map_err(BillingLedgerProjectionError::Invalid)?;
    }
    Ok(())
}

fn add_allocation_kind(
    totals: &mut BillingRollupTotals,
    kind: &kiana_domain::AllocationCostKind,
) -> Result<(), BillingLedgerProjectionError> {
    match kind {
        kiana_domain::AllocationCostKind::Estimated { amount, .. } => totals
            .add_estimated(amount)
            .map_err(BillingLedgerProjectionError::Invalid),
        kiana_domain::AllocationCostKind::Measured { amount, .. } => totals
            .add_measured(amount)
            .map_err(BillingLedgerProjectionError::Invalid),
        kiana_domain::AllocationCostKind::Unknown { .. } => {
            totals.unknown_count = totals.unknown_count.saturating_add(1);
            Ok(())
        }
    }
}

fn add_bucket(
    daily: &mut BTreeMap<String, BillingRollupTotals>,
    windows: &mut BTreeMap<u64, BillingRollupTotals>,
    timestamp: u64,
    mut add: impl FnMut(&mut BillingRollupTotals) -> Result<(), BillingLedgerProjectionError>,
) -> Result<(), BillingLedgerProjectionError> {
    if timestamp == 0 {
        return Err(BillingLedgerProjectionError::Invalid(
            "billing_fact_timestamp_invalid".to_owned(),
        ));
    }
    let day = utc_day_key(timestamp);
    let daily_totals = daily.entry(day).or_default();
    add(daily_totals)?;
    let window = timestamp / BILLING_WINDOW_MILLIS * BILLING_WINDOW_MILLIS;
    let window_totals = windows.entry(window).or_default();
    add(window_totals)?;
    Ok(())
}

fn event_time(data: &Value) -> Option<u64> {
    [
        "occurred_at_unix_ms",
        "observed_at_unix_ms",
        "created_at_unix_ms",
    ]
    .into_iter()
    .find_map(|field| {
        data.get(field)
            .and_then(Value::as_u64)
            .filter(|value| *value > 0)
    })
}

fn utc_day_key(timestamp: u64) -> String {
    // Howard Hinnant's civil_from_days algorithm, with an unsigned millisecond input bounded to
    // the Unix epoch. Keeping this local avoids making chrono a domain dependency.
    let days = (timestamp / 86_400_000) as i64;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let y = y + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

fn next_replay_fence(previous: &str, source_cursor: EventCursor, event_id: EventId) -> String {
    json_digest(&serde_json::json!({
        "previous": previous,
        "source_cursor": source_cursor,
        "event_id": event_id,
    }))
}

/// Rebuild a projection from a fully read committed journal. This function is the query-facing
/// entrypoint used by adapters and deliberately accepts no EventStore, authorization or Broker.
pub fn project_billing_ledger(
    source_epoch: u64,
    source_cursor: EventCursor,
    events: &[RuntimeEvent],
) -> Result<BillingProjectionSnapshot, BillingLedgerProjectionError> {
    BillingLedgerProjector::rebuild(source_epoch, source_cursor, events)
}

/// Source health is metadata only; it cannot be consumed as a billing reservation or approval.
pub fn billing_source_is_rebuildable(health: &EventStoreHealth) -> bool {
    health.validate().is_ok() && health.last_durable_cursor > 0
}

/// Marker used by source guards to assert that this module contains no EventStore writes.
pub const BILLING_PROJECTOR_NO_FACT_WRITES: bool = true;
