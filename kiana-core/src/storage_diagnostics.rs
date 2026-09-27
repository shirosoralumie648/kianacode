//! PD-29 storage health, projection-lag and maintenance-metric diagnostic contract.
//!
//! This module is a **display** path and nothing else. It reads server-owned evidence and turns
//! it into redacted DTOs a UI can render: store health, projection lag/generation, backup /
//! migration / retention counters, and a bounded limitation list. It never opens a store, reads a
//! byte, schedules maintenance or grants an admission.
//!
//! Three rules are enforced structurally rather than by convention:
//!
//! 1. **A diagnostic never authorizes.** [`StorageDiagnosticReport`] carries `display_only: true`
//!    and [`StorageDiagnosticReport::authorizes`] is hard-wired to `false`; the UI view repeats the
//!    refusal as a checked `authorizes: false` field. Admission stays with the control plane and
//!    the gate/policy engines.
//! 2. **Stale, unknown and corrupt are never rendered healthy.** Each status is derived by a fixed
//!    documented order, and the aggregate is the worst of its parts. A status of `Ok` also
//!    requires an empty limitation list, so "green with a caveat" is not representable.
//! 3. **Output is redacted.** Every operator-visible string is a bounded token checked with
//!    [`redact_text`] and [`scan_secret_sentinels`], carrying no JSON payload and no path. Adapter
//!    error text (for example an incident `code`) is deliberately **not** copied: only counts and
//!    classes cross this boundary.
//!
//! Logs, metrics and traces stay separate signals from facts. The metric catalog is closed, and a
//! name in the reserved fact namespace is refused outright, so no adapter can restate or re-derive
//! a ledger entry through the observability channels.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, SchemaVersion, SecretScanChannel,
    SignalStatus, StorageHealth, StorageHealthStatus, StorageIntegrityIncident,
    StorageIntegrityIncidentClass, StoreIdentityId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const STORAGE_DIAGNOSTIC_INPUT_SCHEMA: &str = "kiana.storage-diagnostic-input.v1";
pub const STORAGE_DIAGNOSTIC_REPORT_SCHEMA: &str = "kiana.storage-diagnostic-report.v1";
pub const STORAGE_DIAGNOSTIC_VIEW_SCHEMA: &str = "kiana.storage-diagnostic-view.v1";
pub const STORAGE_DIAGNOSTIC_SIGNAL_SCHEMA: &str = "kiana.storage-diagnostic-signal.v1";
pub const STORAGE_DIAGNOSTIC_NOTE_SCHEMA: &str = "kiana.storage-diagnostic-note.v1";
pub const STORAGE_DIAGNOSTIC_LAG_SCHEMA: &str = "kiana.storage-diagnostic-lag.v1";
pub const STORAGE_DIAGNOSTIC_MAINTENANCE_SCHEMA: &str = "kiana.storage-diagnostic-maintenance.v1";
pub const STORAGE_DIAGNOSTIC_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Diagnostic DTOs are display-only by construction. The PD-29 source guard asserts this constant
/// so a later edit cannot quietly turn a health report into an authority.
pub const STORAGE_DIAGNOSTIC_DISPLAY_ONLY: bool = true;

pub const MAX_DIAGNOSTIC_NOTE_BYTES: usize = 128;
pub const MAX_DIAGNOSTIC_PROJECTOR_ID_BYTES: usize = 64;
pub const MAX_DIAGNOSTIC_INCIDENTS: usize = 32;
pub const MAX_DIAGNOSTIC_LIMITATIONS: usize = 16;
pub const MAX_DIAGNOSTIC_SIGNALS: usize = 16;
/// Display values are bounded counters, not log storage. No real cursor comes near this.
pub const MAX_DIAGNOSTIC_COUNTER: u64 = 1_000_000_000;

/// Logs, metrics and traces are observability signals. A fact has no channel here.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageSignalChannel {
    Logs,
    Metrics,
    Traces,
}

impl StorageSignalChannel {
    pub const ALL: [Self; 3] = [Self::Logs, Self::Metrics, Self::Traces];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Metrics => "metrics",
            Self::Traces => "traces",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMetricUnit {
    Count,
    Events,
}

/// Where a projection checkpoint stands relative to the fact cursor it was built from.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageCheckpointState {
    CaughtUp,
    Lagging,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageMaintenanceSubject {
    Backup,
    Migration,
    Retention,
}

impl StorageMaintenanceSubject {
    pub const ALL: [Self; 3] = [Self::Backup, Self::Migration, Self::Retention];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Backup => "backup",
            Self::Migration => "migration",
            Self::Retention => "retention",
        }
    }
}

/// Name prefixes reserved for the fact ledger. A diagnostic metric may not use one: the
/// observability channels report on the store, they never restate a fact.
pub const STORAGE_DIAGNOSTIC_FACT_RESERVED_PREFIXES: [&str; 6] = [
    "kiana.fact",
    "kiana.event",
    "kiana.receipt",
    "kiana.command",
    "eventlog.",
    "journal.",
];

/// The closed metric catalog. The channel is part of the catalog entry, so a name cannot be moved
/// to a different observability channel without editing this table.
pub const STORAGE_DIAGNOSTIC_METRIC_CATALOG: [(&str, StorageMetricUnit, StorageSignalChannel); 16] = [
    (
        "kiana.storage.projection_lag_events",
        StorageMetricUnit::Events,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.projection_checkpoint_stale",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.integrity_incidents_open",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.backup_verified_total",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.backup_never_verified",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.backup_pending",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.backup_failed_total",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.migration_pending",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.migration_failed_total",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.retention_blocked_total",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.retention_sweep_incomplete",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.retention_failed_total",
        StorageMetricUnit::Count,
        StorageSignalChannel::Metrics,
    ),
    (
        "kiana.storage.log_records",
        StorageMetricUnit::Count,
        StorageSignalChannel::Logs,
    ),
    (
        "kiana.storage.log_dropped",
        StorageMetricUnit::Count,
        StorageSignalChannel::Logs,
    ),
    (
        "kiana.storage.trace_spans",
        StorageMetricUnit::Count,
        StorageSignalChannel::Traces,
    ),
    (
        "kiana.storage.trace_incomplete",
        StorageMetricUnit::Count,
        StorageSignalChannel::Traces,
    ),
];

/// One bounded, redacted operator-visible token.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDiagnosticNote {
    pub schema: String,
    pub text: String,
    pub note_digest: String,
}

impl StorageDiagnosticNote {
    pub fn new(text: impl Into<String>) -> Result<Self, String> {
        let mut note = Self {
            schema: STORAGE_DIAGNOSTIC_NOTE_SCHEMA.to_owned(),
            text: text.into(),
            note_digest: String::new(),
        };
        note.note_digest = note.digest();
        note.validate()?;
        Ok(note)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_NOTE_SCHEMA {
            return Err("storage_diagnostic_note_schema_invalid".to_owned());
        }
        validate_note_text(&self.text)?;
        valid_digest(&self.note_digest, "storage_diagnostic_note_digest")?;
        if self.note_digest != self.digest() {
            return Err("storage_diagnostic_note_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "text": self.text,
        }))
    }
}

/// Cursor arithmetic plus the display metadata a UI needs: lag, generation and data epoch.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageProjectionLag {
    pub schema: String,
    pub projector_id: String,
    pub source_cursor: EventCursor,
    pub projection_cursor: EventCursor,
    pub projection_generation: u64,
    pub data_epoch: u64,
    pub checkpoint_state: StorageCheckpointState,
    pub lag_digest: String,
}

impl StorageProjectionLag {
    pub fn new(
        projector_id: impl Into<String>,
        source_cursor: EventCursor,
        projection_cursor: EventCursor,
        projection_generation: u64,
        data_epoch: u64,
        checkpoint_state: StorageCheckpointState,
    ) -> Result<Self, String> {
        let mut lag = Self {
            schema: STORAGE_DIAGNOSTIC_LAG_SCHEMA.to_owned(),
            projector_id: projector_id.into(),
            source_cursor,
            projection_cursor,
            projection_generation,
            data_epoch,
            checkpoint_state,
            lag_digest: String::new(),
        };
        lag.lag_digest = lag.digest();
        lag.validate()?;
        Ok(lag)
    }

    /// Events the projection has not applied yet. Saturating, because a cursor ahead of the fact
    /// cursor is refused by [`StorageProjectionLag::validate`] rather than silently wrapped.
    pub fn lag_events(&self) -> u64 {
        self.source_cursor.saturating_sub(self.projection_cursor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_LAG_SCHEMA
            || self.projector_id.is_empty()
            || self.projector_id.len() > MAX_DIAGNOSTIC_PROJECTOR_ID_BYTES
            || self.source_cursor == 0
            || self.source_cursor > MAX_DIAGNOSTIC_COUNTER
            || self.projection_cursor == 0
            || self.projection_cursor > MAX_DIAGNOSTIC_COUNTER
            || self.projection_generation == 0
            || self.data_epoch == 0
        {
            return Err("storage_diagnostic_lag_header_invalid".to_owned());
        }
        validate_note_text(&self.projector_id)?;
        // A projection cursor past the fact cursor is a broken claim about the store, not a lag.
        if self.projection_cursor > self.source_cursor {
            return Err("storage_diagnostics_projection_cursor_ahead".to_owned());
        }
        // A declared checkpoint state that contradicts the cursors would let a UI render a lagging
        // projection as caught up.
        let lag = self.lag_events();
        match self.checkpoint_state {
            StorageCheckpointState::CaughtUp if lag != 0 => Err(checkpoint_state_reason()),
            StorageCheckpointState::Lagging if lag == 0 => Err(checkpoint_state_reason()),
            StorageCheckpointState::CaughtUp
            | StorageCheckpointState::Lagging
            | StorageCheckpointState::Unknown => Ok(()),
        }?;
        valid_digest(&self.lag_digest, "storage_diagnostic_lag_digest")?;
        if self.lag_digest != self.digest() {
            return Err("storage_diagnostic_lag_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "projector_id": self.projector_id,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "checkpoint_state": self.checkpoint_state,
        }))
    }
}

fn checkpoint_state_reason() -> String {
    "storage_diagnostic_checkpoint_state_contradicts_cursor".to_owned()
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageMaintenanceCounters {
    pub pending: u64,
    pub failed: u64,
    pub blocked: u64,
}

/// Adapter-reported counters for one maintenance subject. `Ok` is only reachable once a bounded
/// timestamp says the subject was actually observed.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageMaintenanceObservation {
    pub schema: String,
    pub subject: StorageMaintenanceSubject,
    pub status: SignalStatus,
    pub last_ok_at_unix_ms: Option<u64>,
    pub last_failure_at_unix_ms: Option<u64>,
    pub counters: StorageMaintenanceCounters,
    pub reason: Option<StorageDiagnosticNote>,
    pub observation_digest: String,
}

impl StorageMaintenanceObservation {
    pub fn new(
        subject: StorageMaintenanceSubject,
        status: SignalStatus,
        last_ok_at_unix_ms: Option<u64>,
        last_failure_at_unix_ms: Option<u64>,
        counters: StorageMaintenanceCounters,
        reason: Option<StorageDiagnosticNote>,
    ) -> Result<Self, String> {
        let mut observation = Self {
            schema: STORAGE_DIAGNOSTIC_MAINTENANCE_SCHEMA.to_owned(),
            subject,
            status,
            last_ok_at_unix_ms,
            last_failure_at_unix_ms,
            counters,
            reason,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    /// The timestamp bound is supplied by the caller so a fixture cannot validate a
    /// future-dated "verified" backup against an invented clock.
    pub fn validate_at(
        &self,
        subject: StorageMaintenanceSubject,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_MAINTENANCE_SCHEMA
            || self.subject != subject
            || self
                .last_ok_at_unix_ms
                .is_some_and(|value| value == 0 || value > now_unix_ms)
            || self
                .last_failure_at_unix_ms
                .is_some_and(|value| value == 0 || value > now_unix_ms)
            || self.counters.pending > MAX_DIAGNOSTIC_COUNTER
            || self.counters.failed > MAX_DIAGNOSTIC_COUNTER
            || self.counters.blocked > MAX_DIAGNOSTIC_COUNTER
        {
            return Err("storage_diagnostic_maintenance_header_invalid".to_owned());
        }
        if let Some(reason) = &self.reason {
            reason.validate()?;
        }
        // A never-observed subject must not report healthy just because nothing has failed yet.
        if self.status == SignalStatus::Ok && self.last_ok_at_unix_ms.is_none() {
            return Err("storage_diagnostic_maintenance_not_observed".to_owned());
        }
        // A failure newer than the last success contradicts an `Ok` status.
        let last_ok = self.last_ok_at_unix_ms.unwrap_or(0);
        if self.status == SignalStatus::Ok && self.last_failure_at_unix_ms.unwrap_or(0) > last_ok {
            return Err("storage_diagnostic_maintenance_stale_status".to_owned());
        }
        if self.status != SignalStatus::Ok && self.reason.is_none() {
            return Err("storage_diagnostic_maintenance_reason_required".to_owned());
        }
        valid_digest(
            &self.observation_digest,
            "storage_diagnostic_maintenance_digest",
        )?;
        if self.observation_digest != self.digest() {
            return Err("storage_diagnostic_maintenance_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_at(self.subject, u64::MAX)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "subject": self.subject,
            "status": self.status,
            "last_ok_at_unix_ms": self.last_ok_at_unix_ms,
            "last_failure_at_unix_ms": self.last_failure_at_unix_ms,
            "counters": self.counters,
            "reason": self.reason,
        }))
    }
}

/// One redacted diagnostic signal. The unit is not a caller-supplied field: it is looked up from
/// the closed catalog, so a signal cannot claim a unit the catalog does not define.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDiagnosticSignal {
    pub schema: String,
    pub channel: StorageSignalChannel,
    pub name: String,
    pub value: u64,
    pub status: SignalStatus,
    pub reason: Option<StorageDiagnosticNote>,
    pub signal_digest: String,
}

impl StorageDiagnosticSignal {
    pub fn new(
        channel: StorageSignalChannel,
        name: impl Into<String>,
        value: u64,
        status: SignalStatus,
        reason: Option<StorageDiagnosticNote>,
    ) -> Result<Self, String> {
        let mut signal = Self {
            schema: STORAGE_DIAGNOSTIC_SIGNAL_SCHEMA.to_owned(),
            channel,
            name: name.into(),
            value,
            status,
            reason,
            signal_digest: String::new(),
        };
        signal.signal_digest = signal.digest();
        signal.validate()?;
        Ok(signal)
    }

    pub fn unit(&self) -> Option<StorageMetricUnit> {
        catalog_entry(&self.name).map(|(_, unit, _)| unit)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_SIGNAL_SCHEMA || self.value > MAX_DIAGNOSTIC_COUNTER {
            return Err("storage_diagnostic_signal_header_invalid".to_owned());
        }
        // A ledger fact is not an observability signal and may not travel on these channels.
        if STORAGE_DIAGNOSTIC_FACT_RESERVED_PREFIXES
            .iter()
            .any(|prefix| self.name.starts_with(prefix))
        {
            return Err("storage_diagnostics_fact_signal_rejected".to_owned());
        }
        let Some((_, _, catalog_channel)) = catalog_entry(&self.name) else {
            return Err("storage_diagnostics_metric_not_in_catalog".to_owned());
        };
        if catalog_channel != self.channel {
            return Err("storage_diagnostics_channel_mismatch".to_owned());
        }
        if self.status != SignalStatus::Ok && self.reason.is_none() {
            return Err("storage_diagnostic_signal_reason_required".to_owned());
        }
        if let Some(reason) = &self.reason {
            reason.validate()?;
        }
        valid_digest(&self.signal_digest, "storage_diagnostic_signal_digest")?;
        if self.signal_digest != self.digest() {
            return Err("storage_diagnostic_signal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "channel": self.channel,
            "name": self.name,
            "value": self.value,
            "status": self.status,
            "reason": self.reason,
        }))
    }
}

fn catalog_entry(name: &str) -> Option<(&'static str, StorageMetricUnit, StorageSignalChannel)> {
    STORAGE_DIAGNOSTIC_METRIC_CATALOG
        .iter()
        .find(|(metric, _, _)| *metric == name)
        .copied()
}

/// The shape a UI renders. It repeats cursor, generation, epoch and the adapter's declared limits
/// so an operator can see how fresh and how bounded the store is, and it carries `authorizes:
/// false` as a checked, non-optional statement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDiagnosticUiView {
    pub schema: String,
    pub version: SchemaVersion,
    pub store_id: StoreIdentityId,
    pub display_status: SignalStatus,
    pub source_cursor: EventCursor,
    pub projection_cursor: EventCursor,
    pub projection_lag: u64,
    pub projection_generation: u64,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    pub durable_commits: bool,
    pub max_frame_bytes: u64,
    pub max_batch_events: u64,
    pub limitations: Vec<String>,
    pub authorizes: bool,
    pub view_digest: String,
}

impl StorageDiagnosticUiView {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_VIEW_SCHEMA
            || self.version != STORAGE_DIAGNOSTIC_VERSION
            || self.store_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.projection_cursor == 0
            || self.projection_generation == 0
            || self.data_epoch == 0
            || self.authority_epoch == 0
            || self.max_frame_bytes == 0
            || self.max_batch_events == 0
            || self.projection_cursor > self.source_cursor
            || self.projection_lag != self.source_cursor.saturating_sub(self.projection_cursor)
            || self.limitations.len() > MAX_DIAGNOSTIC_LIMITATIONS
        {
            return Err("storage_diagnostic_view_header_invalid".to_owned());
        }
        for limitation in &self.limitations {
            validate_note_text(limitation)?;
        }
        if self.authorizes {
            return Err("storage_diagnostics_view_authorizes".to_owned());
        }
        // The view may never look healthier than the report it was derived from.
        if self.display_status == SignalStatus::Ok && !self.limitations.is_empty() {
            return Err("storage_diagnostics_unhealthy_reported_healthy".to_owned());
        }
        valid_digest(&self.view_digest, "storage_diagnostic_view_digest")?;
        if self.view_digest != self.digest() {
            return Err("storage_diagnostic_view_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "store_id": self.store_id,
            "display_status": self.display_status,
            "source_cursor": self.source_cursor,
            "projection_cursor": self.projection_cursor,
            "projection_lag": self.projection_lag,
            "projection_generation": self.projection_generation,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "durable_commits": self.durable_commits,
            "max_frame_bytes": self.max_frame_bytes,
            "max_batch_events": self.max_batch_events,
            "limitations": self.limitations,
            "authorizes": self.authorizes,
        }))
    }
}

/// The redacted diagnostic DTO. It displays; it never authorizes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDiagnosticReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub input_digest: String,
    pub store_id: StoreIdentityId,
    pub display_only: bool,
    pub store_status: SignalStatus,
    pub display_status: SignalStatus,
    pub projection: StorageProjectionLag,
    pub signals: Vec<StorageDiagnosticSignal>,
    pub limitations: Vec<StorageDiagnosticNote>,
    pub view: StorageDiagnosticUiView,
    pub report_digest: String,
}

impl StorageDiagnosticReport {
    pub fn evaluate(input: &StorageDiagnosticInput) -> Result<Self, String> {
        input.validate()?;
        let report = build_report(input)?;
        report.validate_against(input)?;
        Ok(report)
    }

    /// A diagnostic never grants an admission or a capability. That is the entire reason
    /// the report carries `display_only`.
    pub const fn authorizes(&self) -> bool {
        false
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_REPORT_SCHEMA
            || self.version != STORAGE_DIAGNOSTIC_VERSION
            || self.store_id.as_uuid().is_nil()
            || self.signals.len() != STORAGE_DIAGNOSTIC_METRIC_CATALOG.len()
            || self.signals.len() > MAX_DIAGNOSTIC_SIGNALS
            || self.limitations.len() > MAX_DIAGNOSTIC_LIMITATIONS
        {
            return Err("storage_diagnostic_report_header_invalid".to_owned());
        }
        if !self.display_only {
            return Err("storage_diagnostics_health_is_display_only".to_owned());
        }
        self.projection.validate()?;
        for signal in &self.signals {
            signal.validate()?;
        }
        for limitation in &self.limitations {
            limitation.validate()?;
        }
        // The aggregate is the worst of its parts. Flipping one component to a healthier value
        // without re-deriving the aggregate is refused here.
        let aggregate = worst_status(
            std::iter::once(self.store_status).chain(self.signals.iter().map(|item| item.status)),
        );
        if self.display_status != aggregate {
            return Err("storage_diagnostics_unhealthy_reported_healthy".to_owned());
        }
        if self.display_status == SignalStatus::Ok && !self.limitations.is_empty() {
            return Err("storage_diagnostics_unhealthy_reported_healthy".to_owned());
        }
        if self.view.display_status != self.display_status || self.view.store_id != self.store_id {
            return Err("storage_diagnostic_view_binding_mismatch".to_owned());
        }
        self.view.validate()?;
        valid_digest(&self.input_digest, "storage_diagnostic_report_input_digest")?;
        valid_digest(&self.report_digest, "storage_diagnostic_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("storage_diagnostic_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, input: &StorageDiagnosticInput) -> Result<(), String> {
        input.validate()?;
        self.validate()?;
        let expected = build_report(input)?;
        if self != &expected {
            return Err("storage_diagnostics_report_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "input_digest": self.input_digest,
            "store_id": self.store_id,
            "display_only": self.display_only,
            "store_status": self.store_status,
            "display_status": self.display_status,
            "projection": self.projection,
            "signals": self.signals,
            "limitations": self.limitations,
            "view": self.view,
        }))
    }
}

/// Server-owned evidence for one storage diagnostic evaluation. There is no constructor that can
/// fabricate a report: the caller supplies facts and the reducer derives every status.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageDiagnosticInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub store_id: StoreIdentityId,
    pub health: StorageHealth,
    pub projection: StorageProjectionLag,
    pub backup: StorageMaintenanceObservation,
    pub migration: StorageMaintenanceObservation,
    pub retention: StorageMaintenanceObservation,
    pub incidents: Vec<StorageIntegrityIncident>,
    pub unknown_observations: u64,
    pub observed_at_unix_ms: u64,
    pub input_digest: String,
}

impl StorageDiagnosticInput {
    /// Seal the digest of a hand-assembled input. Re-sealing is required after any field change.
    pub fn sealed(mut self) -> Result<Self, String> {
        self.input_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_DIAGNOSTIC_INPUT_SCHEMA
            || self.version != STORAGE_DIAGNOSTIC_VERSION
            || self.store_id.as_uuid().is_nil()
            || self.observed_at_unix_ms == 0
            || self.observed_at_unix_ms < self.health.observed_at_unix_ms
            || self.incidents.len() > MAX_DIAGNOSTIC_INCIDENTS
            || self.unknown_observations > MAX_DIAGNOSTIC_COUNTER
        {
            return Err("storage_diagnostic_input_header_invalid".to_owned());
        }
        self.health
            .validate()
            .map_err(|_| "storage_diagnostic_input_health_invalid".to_owned())?;
        if self.health.store_id != self.store_id {
            return Err("storage_diagnostic_input_store_mismatch".to_owned());
        }
        self.projection.validate()?;
        // The lag fact and the health snapshot describe the same store at the same cursor.
        if self.projection.source_cursor != self.health.source_cursor {
            return Err("storage_diagnostic_input_cursor_mismatch".to_owned());
        }
        self.backup
            .validate_at(StorageMaintenanceSubject::Backup, self.observed_at_unix_ms)?;
        self.migration.validate_at(
            StorageMaintenanceSubject::Migration,
            self.observed_at_unix_ms,
        )?;
        self.retention.validate_at(
            StorageMaintenanceSubject::Retention,
            self.observed_at_unix_ms,
        )?;
        let mut seen = BTreeSet::new();
        for incident in &self.incidents {
            incident
                .validate()
                .map_err(|_| "storage_diagnostic_input_incident_invalid".to_owned())?;
            if incident.store_id != self.store_id {
                return Err("storage_diagnostic_input_incident_store_mismatch".to_owned());
            }
            if !seen.insert(incident.incident_id) {
                return Err("storage_diagnostic_input_incident_duplicate".to_owned());
            }
        }
        valid_digest(&self.input_digest, "storage_diagnostic_input_digest")?;
        if self.input_digest != self.digest() {
            return Err("storage_diagnostic_input_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "store_id": self.store_id,
            "health": self.health,
            "projection": self.projection,
            "backup": self.backup,
            "migration": self.migration,
            "retention": self.retention,
            "incidents": self.incidents,
            "unknown_observations": self.unknown_observations,
            "observed_at_unix_ms": self.observed_at_unix_ms,
        }))
    }
}

/// Evaluate one storage diagnostic. Pure: no store is opened, no maintenance is scheduled and no
/// admission decision is made.
pub fn evaluate_storage_diagnostics(
    input: &StorageDiagnosticInput,
) -> Result<StorageDiagnosticReport, String> {
    StorageDiagnosticReport::evaluate(input)
}

pub fn validate_storage_diagnostic_report(
    input: &StorageDiagnosticInput,
    report: &StorageDiagnosticReport,
) -> Result<(), String> {
    report.validate_against(input)
}

/// The render-only DTO. It is a copy of the view the report already sealed, returned so a surface
/// can hand the UI exactly what was validated.
pub fn storage_diagnostic_ui_view(
    report: &StorageDiagnosticReport,
) -> Result<StorageDiagnosticUiView, String> {
    report.validate()?;
    Ok(report.view.clone())
}

pub fn validate_storage_diagnostic_ui_view(view: &StorageDiagnosticUiView) -> Result<(), String> {
    view.validate()
}

/// Store status decision order, fixed and documented so a reader can predict every value:
///
/// 1. corrupt store or an open `Corrupt` incident → `Error`
/// 2. unavailable store → `Error`
/// 3. unknown store, an open `Unknown`/`ResultUnknown` incident, or unclassified observations → `Unknown`
/// 4. empty/degraded store, missing durability, projection lag, or a checkpoint that is not caught
///    up → `Degraded`
/// 5. only then `Ok`
fn store_status(input: &StorageDiagnosticInput) -> SignalStatus {
    let open = open_incidents(input);
    if input.health.status == StorageHealthStatus::Corrupt
        || open
            .iter()
            .any(|incident| incident.class == StorageIntegrityIncidentClass::Corrupt)
    {
        return SignalStatus::Error;
    }
    if input.health.status == StorageHealthStatus::Unavailable {
        return SignalStatus::Error;
    }
    if input.health.status == StorageHealthStatus::Unknown
        || input.unknown_observations > 0
        || open.iter().any(|incident| {
            matches!(
                incident.class,
                StorageIntegrityIncidentClass::Unknown
                    | StorageIntegrityIncidentClass::ResultUnknown
            )
        })
    {
        return SignalStatus::Unknown;
    }
    if input.health.status == StorageHealthStatus::Empty
        || input.health.status == StorageHealthStatus::Degraded
        || !input.health.capabilities.durable_commits
        || input.projection.lag_events() > 0
        || input.projection.checkpoint_state != StorageCheckpointState::CaughtUp
    {
        return SignalStatus::Degraded;
    }
    SignalStatus::Ok
}

fn open_incidents(input: &StorageDiagnosticInput) -> Vec<&StorageIntegrityIncident> {
    input
        .incidents
        .iter()
        .filter(|incident| incident.resolved_at_unix_ms.is_none())
        .collect()
}

fn incident_status(input: &StorageDiagnosticInput) -> SignalStatus {
    let open = open_incidents(input);
    if open
        .iter()
        .any(|incident| incident.class == StorageIntegrityIncidentClass::Corrupt)
    {
        SignalStatus::Error
    } else if open.is_empty() {
        SignalStatus::Ok
    } else {
        SignalStatus::Unknown
    }
}

fn metric(
    name: &str,
    value: u64,
    status: SignalStatus,
    reason_text: Option<&str>,
) -> Result<StorageDiagnosticSignal, String> {
    let Some((_, _, channel)) = catalog_entry(name) else {
        return Err("storage_diagnostics_metric_not_in_catalog".to_owned());
    };
    let reason = reason_text.map(StorageDiagnosticNote::new).transpose()?;
    StorageDiagnosticSignal::new(channel, name, value, status, reason)
}

fn build_signals(input: &StorageDiagnosticInput) -> Result<Vec<StorageDiagnosticSignal>, String> {
    let lag = input.projection.lag_events();
    let checkpoint_stale = input.projection.checkpoint_state != StorageCheckpointState::CaughtUp;
    let (projection_status, projection_reason) = if lag > 0 {
        (SignalStatus::Degraded, Some("projection_lag_present"))
    } else if checkpoint_stale {
        (
            SignalStatus::Degraded,
            Some("projection_checkpoint_not_caught_up"),
        )
    } else {
        (SignalStatus::Ok, None)
    };
    let incidents = incident_status(input);
    let incident_reason = (incidents != SignalStatus::Ok).then_some("integrity_incident_open");
    let open_count = u64::try_from(open_incidents(input).len()).unwrap_or(MAX_DIAGNOSTIC_COUNTER);
    let backup = &input.backup;
    let migration = &input.migration;
    let retention = &input.retention;
    let backup_reason = backup.reason.as_ref().map(|note| note.text.as_str());
    let migration_reason = migration.reason.as_ref().map(|note| note.text.as_str());
    let retention_reason = retention.reason.as_ref().map(|note| note.text.as_str());

    let mut signals = vec![
        metric(
            "kiana.storage.projection_lag_events",
            lag,
            projection_status,
            projection_reason,
        )?,
        metric(
            "kiana.storage.projection_checkpoint_stale",
            u64::from(checkpoint_stale),
            projection_status,
            projection_reason,
        )?,
        metric(
            "kiana.storage.integrity_incidents_open",
            open_count,
            incidents,
            incident_reason,
        )?,
        metric(
            "kiana.storage.backup_verified_total",
            u64::from(backup.last_ok_at_unix_ms.is_some()),
            backup.status,
            backup_reason,
        )?,
        metric(
            "kiana.storage.backup_never_verified",
            u64::from(backup.last_ok_at_unix_ms.is_none()),
            backup.status,
            backup_reason,
        )?,
        metric(
            "kiana.storage.backup_pending",
            backup.counters.pending,
            backup.status,
            backup_reason,
        )?,
        metric(
            "kiana.storage.backup_failed_total",
            backup.counters.failed,
            backup.status,
            backup_reason,
        )?,
        metric(
            "kiana.storage.migration_pending",
            migration.counters.pending,
            migration.status,
            migration_reason,
        )?,
        metric(
            "kiana.storage.migration_failed_total",
            migration.counters.failed,
            migration.status,
            migration_reason,
        )?,
        metric(
            "kiana.storage.retention_blocked_total",
            retention.counters.blocked,
            retention.status,
            retention_reason,
        )?,
        metric(
            "kiana.storage.retention_sweep_incomplete",
            u64::from(retention.counters.pending > 0),
            retention.status,
            retention_reason,
        )?,
        metric(
            "kiana.storage.retention_failed_total",
            retention.counters.failed,
            retention.status,
            retention_reason,
        )?,
    ];
    // The log and trace counters report on the observability channels themselves. A channel that
    // has not been observed is not a channel with nothing to say, so it never reports `Ok`.
    for name in [
        "kiana.storage.log_records",
        "kiana.storage.log_dropped",
        "kiana.storage.trace_spans",
        "kiana.storage.trace_incomplete",
    ] {
        signals.push(metric(
            name,
            0,
            incidents,
            incident_reason.map(|reason| {
                if reason == "integrity_incident_open" {
                    "observability_channel_unobserved"
                } else {
                    "observability_channel_not_healthy"
                }
            }),
        )?);
    }
    Ok(signals)
}

fn push_limitation(limitations: &mut Vec<StorageDiagnosticNote>, text: &str) -> Result<(), String> {
    if limitations.len() < MAX_DIAGNOSTIC_LIMITATIONS
        && !limitations.iter().any(|item| item.text == text)
    {
        limitations.push(StorageDiagnosticNote::new(text)?);
    }
    Ok(())
}

fn push_observation(
    limitations: &mut Vec<StorageDiagnosticNote>,
    observation: &StorageMaintenanceObservation,
) -> Result<(), String> {
    if observation.status == SignalStatus::Ok {
        return Ok(());
    }
    let fallback = format!("{}_not_observed", observation.subject.as_str());
    let text = observation
        .reason
        .as_ref()
        .map_or(fallback.as_str(), |note| note.text.as_str());
    push_limitation(limitations, text)
}

fn build_limitations(
    input: &StorageDiagnosticInput,
    store: SignalStatus,
) -> Result<Vec<StorageDiagnosticNote>, String> {
    let mut limitations = Vec::new();
    if store != SignalStatus::Ok {
        let token = match input.health.status {
            StorageHealthStatus::Corrupt => "storage_health_corrupt",
            StorageHealthStatus::Unavailable => "storage_health_unavailable",
            StorageHealthStatus::Unknown => "storage_health_unknown",
            StorageHealthStatus::Empty => "storage_health_empty",
            StorageHealthStatus::Degraded => "storage_health_degraded",
            StorageHealthStatus::Ready => "storage_store_degraded",
        };
        push_limitation(&mut limitations, token)?;
    }
    if !input.health.capabilities.durable_commits {
        push_limitation(&mut limitations, "storage_durability_unavailable")?;
    }
    if input.projection.lag_events() > 0 {
        push_limitation(&mut limitations, "projection_lag_present")?;
    }
    if input.projection.checkpoint_state != StorageCheckpointState::CaughtUp {
        push_limitation(&mut limitations, "projection_checkpoint_not_caught_up")?;
    }
    let open = open_incidents(input);
    if open
        .iter()
        .any(|incident| incident.class == StorageIntegrityIncidentClass::Corrupt)
    {
        push_limitation(&mut limitations, "integrity_incident_corrupt_open")?;
    } else if !open.is_empty() {
        push_limitation(&mut limitations, "integrity_incident_unknown_open")?;
    }
    if input.unknown_observations > 0 {
        push_limitation(&mut limitations, "unknown_observation_present")?;
    }
    push_observation(&mut limitations, &input.backup)?;
    push_observation(&mut limitations, &input.migration)?;
    push_observation(&mut limitations, &input.retention)?;
    Ok(limitations)
}

fn build_report(input: &StorageDiagnosticInput) -> Result<StorageDiagnosticReport, String> {
    let store = store_status(input);
    let signals = build_signals(input)?;
    let limitations = build_limitations(input, store)?;
    let display_status =
        worst_status(std::iter::once(store).chain(signals.iter().map(|signal| signal.status)));
    let mut view = StorageDiagnosticUiView {
        schema: STORAGE_DIAGNOSTIC_VIEW_SCHEMA.to_owned(),
        version: STORAGE_DIAGNOSTIC_VERSION,
        store_id: input.store_id,
        display_status,
        source_cursor: input.projection.source_cursor,
        projection_cursor: input.projection.projection_cursor,
        projection_lag: input.projection.lag_events(),
        projection_generation: input.projection.projection_generation,
        data_epoch: input.projection.data_epoch,
        authority_epoch: input.health.authority_epoch,
        durable_commits: input.health.capabilities.durable_commits,
        max_frame_bytes: input.health.capabilities.max_frame_bytes as u64,
        max_batch_events: input.health.capabilities.max_batch_events as u64,
        limitations: limitations.iter().map(|note| note.text.clone()).collect(),
        authorizes: false,
        view_digest: String::new(),
    };
    view.view_digest = view.digest();
    let mut report = StorageDiagnosticReport {
        schema: STORAGE_DIAGNOSTIC_REPORT_SCHEMA.to_owned(),
        version: STORAGE_DIAGNOSTIC_VERSION,
        input_digest: input.input_digest.clone(),
        store_id: input.store_id,
        display_only: true,
        store_status: store,
        display_status,
        projection: input.projection.clone(),
        signals,
        limitations,
        view,
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    Ok(report)
}

/// A diagnostic note is a short opaque token: never a serialized payload, never a path, never a
/// secret. The same rule is applied to a hand-assembled UI view.
fn validate_note_text(text: &str) -> Result<(), String> {
    if text.is_empty() || text.len() > MAX_DIAGNOSTIC_NOTE_BYTES || text.contains('\0') {
        return Err("storage_diagnostic_note_header_invalid".to_owned());
    }
    if text.starts_with('{') || text.starts_with('[') || text.starts_with('"') {
        return Err("storage_diagnostic_note_payload".to_owned());
    }
    if redact_text(text) != text || scan_secret_sentinels(SecretScanChannel::Receipt, text).is_err()
    {
        return Err("storage_diagnostic_note_secret".to_owned());
    }
    if text.contains('/')
        || text.contains('\\')
        || text.contains("://")
        || text.contains("..")
        || text.starts_with('~')
    {
        return Err("storage_diagnostic_note_path".to_owned());
    }
    Ok(())
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn status_rank(status: SignalStatus) -> u8 {
    match status {
        SignalStatus::Ok => 0,
        SignalStatus::Degraded => 1,
        SignalStatus::Unknown => 2,
        SignalStatus::Error => 3,
    }
}

fn worst_status(statuses: impl IntoIterator<Item = SignalStatus>) -> SignalStatus {
    let mut worst = SignalStatus::Ok;
    for status in statuses {
        if status_rank(status) > status_rank(worst) {
            worst = status;
        }
    }
    worst
}
