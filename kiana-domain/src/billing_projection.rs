//! Rebuildable BQ-20 billing projection contracts.
//!
//! These values describe a read model only.  They do not append facts, consume a reservation or
//! grant an approval.  The EventLog remains the only source of truth; a projection may always be
//! discarded and rebuilt from committed pages.

use crate::{json_digest, EventCursor, EventId, Money, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const BILLING_PROJECTION_SCHEMA: &str = "kiana.billing-ledger-projection.v1";
pub const BILLING_PROJECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const BILLING_PROJECTION_NUMBER: u64 = 1;
pub const BILLING_WINDOW_MILLIS: u64 = 3_600_000;
pub const MAX_BILLING_SOURCE_EVENT_IDS: usize = 512;
pub const MAX_BILLING_QUARANTINE: usize = 1_024;

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value
            .bytes()
            .any(|byte| byte == 0 || byte == b'\r' || byte == b'\n')
}

/// A cursor is bound to one source epoch and one projection implementation version.  The replay
/// fence is a hash chain over committed event IDs; it makes a cursor from a different replay
/// sequence fail closed even when its numeric cursor happens to match.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingProjectionCursor {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_epoch: u64,
    pub source_cursor: EventCursor,
    pub projection_version: u64,
    pub replay_fence: String,
    #[serde(default)]
    pub source_event_ids: Vec<EventId>,
    pub cursor_digest: String,
}

impl BillingProjectionCursor {
    pub fn new(
        source_epoch: u64,
        source_cursor: EventCursor,
        projection_version: u64,
        replay_fence: impl Into<String>,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        let mut cursor = Self {
            schema: BILLING_PROJECTION_SCHEMA.to_owned(),
            version: BILLING_PROJECTION_VERSION,
            source_epoch,
            source_cursor,
            projection_version,
            replay_fence: replay_fence.into(),
            source_event_ids,
            cursor_digest: String::new(),
        };
        cursor.cursor_digest = cursor.digest();
        cursor.validate()?;
        Ok(cursor)
    }

    pub fn initial(source_epoch: u64) -> Result<Self, String> {
        let replay_fence = json_digest(&json!({
            "source_epoch": source_epoch,
            "source_cursor": 0,
            "projection_version": BILLING_PROJECTION_NUMBER,
            "event_id": ValueDigest::empty(),
        }));
        Self::new(
            source_epoch,
            0,
            BILLING_PROJECTION_NUMBER,
            replay_fence,
            Vec::new(),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_PROJECTION_SCHEMA
            || !self.version.is_compatible_with(&BILLING_PROJECTION_VERSION)
            || self.source_epoch == 0
            || self.projection_version == 0
            || self.source_event_ids.len() > MAX_BILLING_SOURCE_EVENT_IDS
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
            || !valid_digest(&self.replay_fence)
            || !valid_digest(&self.cursor_digest)
        {
            return Err("billing_projection_cursor_header_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        if self.source_event_ids.iter().any(|id| !ids.insert(*id)) {
            return Err("billing_projection_cursor_event_duplicate".to_owned());
        }
        if self.cursor_digest != self.digest() {
            return Err("billing_projection_cursor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_epoch": self.source_epoch,
            "source_cursor": self.source_cursor,
            "projection_version": self.projection_version,
            "replay_fence": self.replay_fence,
            "source_event_ids": self.source_event_ids,
        }))
    }
}

/// Small private JSON marker used to make the initial replay fence explicit without inventing an
/// event ID.  It serializes as a stable string and is intentionally not exported as a fact.
#[derive(Serialize)]
struct ValueDigest;
impl ValueDigest {
    const fn empty() -> &'static str {
        "empty"
    }
}

/// A malformed committed fact is retained in the read model instead of being silently skipped.
/// Quarantine is diagnostic state and never mutates the EventLog fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingQuarantineRecord {
    pub source_cursor: EventCursor,
    pub event_id: EventId,
    pub kind: String,
    pub reason: String,
    pub payload_digest: String,
}

impl BillingQuarantineRecord {
    pub fn new(
        source_cursor: EventCursor,
        event_id: EventId,
        kind: impl Into<String>,
        reason: impl Into<String>,
        payload_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let record = Self {
            source_cursor,
            event_id,
            kind: kind.into(),
            reason: reason.into(),
            payload_digest: payload_digest.into(),
        };
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.source_cursor == 0
            || self.event_id.as_uuid().is_nil()
            || !bounded(&self.kind, 256)
            || !bounded(&self.reason, 512)
            || !valid_digest(&self.payload_digest)
        {
            return Err("billing_quarantine_record_invalid".to_owned());
        }
        Ok(())
    }
}

/// Totals intentionally retain estimate, measured and correction amounts in separate slots.  A
/// missing amount is `None`; it is never represented as a fabricated numeric zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingRollupTotals {
    pub estimated: Option<Money>,
    pub measured: Option<Money>,
    pub correction_estimated: Option<Money>,
    pub correction_measured: Option<Money>,
    pub unknown_count: u64,
    pub usage_count: u64,
    pub ledger_entry_count: u64,
    pub allocation_count: u64,
}

impl BillingRollupTotals {
    pub fn add_estimated(&mut self, amount: &Money) -> Result<(), String> {
        add_money(&mut self.estimated, amount)
    }

    pub fn add_measured(&mut self, amount: &Money) -> Result<(), String> {
        add_money(&mut self.measured, amount)
    }

    pub fn add_correction_estimated(&mut self, amount: &Money) -> Result<(), String> {
        add_money(&mut self.correction_estimated, amount)
    }

    pub fn add_correction_measured(&mut self, amount: &Money) -> Result<(), String> {
        add_money(&mut self.correction_measured, amount)
    }

    pub fn validate(&self) -> Result<(), String> {
        for amount in [
            self.estimated.as_ref(),
            self.measured.as_ref(),
            self.correction_estimated.as_ref(),
            self.correction_measured.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            amount.validate()?;
        }
        Ok(())
    }
}

fn add_money(slot: &mut Option<Money>, amount: &Money) -> Result<(), String> {
    amount.validate()?;
    *slot = Some(match slot.take() {
        Some(existing) => existing.checked_add(amount)?,
        None => amount.clone(),
    });
    Ok(())
}

/// Complete rebuildable billing read model.  `ledger` contains canonical BQ-14 facts, `usage`
/// contains BQ-13 cost observations, and `allocation` contains BQ-19 attribution views.  The
/// latter is deliberately separate so one usage leaf is never charged once per dimension.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BillingProjectionSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub projection_version: u64,
    pub source: BillingProjectionCursor,
    pub usage: BillingRollupTotals,
    pub ledger: BillingRollupTotals,
    pub allocation: BillingRollupTotals,
    pub daily_rollups: BTreeMap<String, BillingRollupTotals>,
    pub window_rollups: BTreeMap<u64, BillingRollupTotals>,
    #[serde(default)]
    pub quarantine: Vec<BillingQuarantineRecord>,
    pub projection_digest: String,
}

impl BillingProjectionSnapshot {
    pub fn empty(source_epoch: u64) -> Result<Self, String> {
        let source = BillingProjectionCursor::initial(source_epoch)?;
        Self::new(
            source,
            BillingRollupTotals::default(),
            BillingRollupTotals::default(),
            BillingRollupTotals::default(),
            BTreeMap::new(),
            BTreeMap::new(),
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: BillingProjectionCursor,
        usage: BillingRollupTotals,
        ledger: BillingRollupTotals,
        allocation: BillingRollupTotals,
        daily_rollups: BTreeMap<String, BillingRollupTotals>,
        window_rollups: BTreeMap<u64, BillingRollupTotals>,
        quarantine: Vec<BillingQuarantineRecord>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: BILLING_PROJECTION_SCHEMA.to_owned(),
            version: BILLING_PROJECTION_VERSION,
            projection_version: source.projection_version,
            source,
            usage,
            ledger,
            allocation,
            daily_rollups,
            window_rollups,
            quarantine,
            projection_digest: String::new(),
        };
        snapshot.projection_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BILLING_PROJECTION_SCHEMA
            || !self.version.is_compatible_with(&BILLING_PROJECTION_VERSION)
            || self.projection_version == 0
            || self.projection_version != self.source.projection_version
            || self.quarantine.len() > MAX_BILLING_QUARANTINE
            || !valid_digest(&self.projection_digest)
        {
            return Err("billing_projection_snapshot_header_invalid".to_owned());
        }
        self.source.validate()?;
        self.usage.validate()?;
        self.ledger.validate()?;
        self.allocation.validate()?;
        for totals in self
            .daily_rollups
            .values()
            .chain(self.window_rollups.values())
        {
            totals.validate()?;
        }
        for day in self.daily_rollups.keys() {
            if !bounded(day, 32) {
                return Err("billing_projection_day_key_invalid".to_owned());
            }
        }
        for window in self.window_rollups.keys() {
            if *window % BILLING_WINDOW_MILLIS != 0 {
                return Err("billing_projection_window_key_invalid".to_owned());
            }
        }
        for record in &self.quarantine {
            record.validate()?;
        }
        if self.projection_digest != self.digest() {
            return Err("billing_projection_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projection_version": self.projection_version,
            "source": self.source,
            "usage": self.usage,
            "ledger": self.ledger,
            "allocation": self.allocation,
            "daily_rollups": self.daily_rollups,
            "window_rollups": self.window_rollups,
            "quarantine": self.quarantine,
        }))
    }
}
