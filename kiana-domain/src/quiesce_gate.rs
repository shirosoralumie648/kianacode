//! Quiesce and consistency gate for a consistent storage snapshot.
//!
//! Taking a snapshot is a two-part contract. The manifest describes *what* a backup contains; this
//! module decides *whether the store is in a state where a snapshot is coherent at all*. It reads
//! adapter-reported facts and returns a decision. It never stops a writer, flushes a buffer,
//! copies a byte, fsyncs a file or resumes work — those effects belong to the later adapter.

use crate::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, OperationId, SchemaVersion,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const QUIESCE_REQUEST_SCHEMA: &str = "kiana.quiesce-request.v1";
pub const QUIESCE_COMPONENT_SCHEMA: &str = "kiana.quiesce-component.v1";
pub const QUIESCE_OBSERVATION_SCHEMA: &str = "kiana.quiesce-observation.v1";
pub const QUIESCE_REPORT_SCHEMA: &str = "kiana.quiesce-report.v1";
pub const QUIESCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_QUIESCE_COMPONENTS: usize = 5;
pub const MAX_QUIESCE_TEXT: usize = 256;

/// The storage components a coherent snapshot must cover.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuiesceComponent {
    /// The EventLog main file.
    EventLogMain,
    /// The EventLog write-ahead log tail.
    EventLogWal,
    /// The artifact store.
    ArtifactStore,
    /// Projector and index checkpoints.
    ProjectionCheckpoint,
    /// The migration registry, so a restore knows which format the data is in.
    MigrationRegistry,
}

impl QuiesceComponent {
    pub const ALL: [Self; 5] = [
        Self::EventLogMain,
        Self::EventLogWal,
        Self::ArtifactStore,
        Self::ProjectionCheckpoint,
        Self::MigrationRegistry,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EventLogMain => "event_log_main",
            Self::EventLogWal => "event_log_wal",
            Self::ArtifactStore => "artifact_store",
            Self::ProjectionCheckpoint => "projection_checkpoint",
            Self::MigrationRegistry => "migration_registry",
        }
    }
}

/// The write state a component was observed in.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuiesceWriteState {
    /// No writer holds this component.
    Idle,
    /// A writer holds it and it is still accepting appends.
    ActiveWriter,
    /// Appends are blocked but buffered bytes are not yet durable.
    Draining,
    /// Every accepted byte is durable.
    Flushed,
    /// The adapter could not establish the state.
    Unknown,
}

impl QuiesceWriteState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::ActiveWriter => "active_writer",
            Self::Draining => "draining",
            Self::Flushed => "flushed",
            Self::Unknown => "unknown",
        }
    }

    /// Only idle and fully flushed components are safe to snapshot. A draining component still
    /// holds unflushed bytes, and an unknown one has not been established at all.
    pub const fn is_snapshot_safe(self) -> bool {
        matches!(self, Self::Idle | Self::Flushed)
    }
}

/// What the adapter observed about one component.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuiesceObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub component: QuiesceComponent,
    pub write_state: QuiesceWriteState,
    /// Last cursor durably written to this component. Zero when the component is empty.
    pub durable_cursor: EventCursor,
    /// Highest cursor the component contains in any form, durable or not.
    pub visible_cursor: EventCursor,
    /// For the WAL: whether it agrees with the main file's durable cursor.
    pub wal_consistent: Option<bool>,
    pub observation_digest: String,
}

impl QuiesceObservation {
    pub fn new(
        component: QuiesceComponent,
        write_state: QuiesceWriteState,
        durable_cursor: EventCursor,
        visible_cursor: EventCursor,
        wal_consistent: Option<bool>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: QUIESCE_OBSERVATION_SCHEMA.to_owned(),
            version: QUIESCE_VERSION,
            component,
            write_state,
            durable_cursor,
            visible_cursor,
            wal_consistent,
            observation_digest: String::new(),
        };
        value.observation_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUIESCE_OBSERVATION_SCHEMA || self.version != QUIESCE_VERSION {
            return Err("quiesce_observation_header_invalid".to_owned());
        }
        if self.visible_cursor < self.durable_cursor {
            return Err("quiesce_observation_cursor_regression".to_owned());
        }
        // A WAL observation that does not state whether it agrees with the main file cannot
        // support a snapshot decision; silence is not consistency.
        if self.component == QuiesceComponent::EventLogWal && self.wal_consistent.is_none() {
            return Err("quiesce_observation_wal_consistency_missing".to_owned());
        }
        if self.component != QuiesceComponent::EventLogWal && self.wal_consistent.is_some() {
            return Err("quiesce_observation_wal_consistency_unexpected".to_owned());
        }
        valid_digest(&self.observation_digest, "quiesce_observation_digest")?;
        if self.observation_digest != self.digest() {
            return Err("quiesce_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "component": self.component,
            "write_state": self.write_state,
            "durable_cursor": self.durable_cursor,
            "visible_cursor": self.visible_cursor,
            "wal_consistent": self.wal_consistent,
        }))
    }
}

/// The ordered quiesce request: what the caller wants snapshotted, and at which boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuiesceRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub actor_id: String,
    /// Cursor the caller intends to snapshot up to. The store must already be durable past it.
    pub target_cursor: EventCursor,
    pub data_epoch: u64,
    pub authority_epoch: u64,
    /// Highest cursor any projector has consumed. It may never exceed the store's source cursor.
    pub projector_cursor: EventCursor,
    pub observations: Vec<QuiesceObservation>,
    pub request_digest: String,
}

impl QuiesceRequest {
    pub fn new(
        operation_id: OperationId,
        actor_id: impl Into<String>,
        target_cursor: EventCursor,
        data_epoch: u64,
        authority_epoch: u64,
        projector_cursor: EventCursor,
        observations: Vec<QuiesceObservation>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: QUIESCE_REQUEST_SCHEMA.to_owned(),
            version: QUIESCE_VERSION,
            operation_id,
            actor_id: actor_id.into(),
            target_cursor,
            data_epoch,
            authority_epoch,
            projector_cursor,
            observations,
            request_digest: String::new(),
        };
        value.request_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUIESCE_REQUEST_SCHEMA
            || self.version != QUIESCE_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.target_cursor == 0
            || self.data_epoch == 0
            || self.authority_epoch == 0
        {
            return Err("quiesce_request_header_invalid".to_owned());
        }
        safe_text(&self.actor_id, "quiesce_actor")?;
        if self.observations.is_empty() || self.observations.len() > MAX_QUIESCE_COMPONENTS {
            return Err("quiesce_request_observation_count_invalid".to_owned());
        }
        let mut seen = BTreeSet::new();
        for observation in &self.observations {
            observation.validate()?;
            if !seen.insert(observation.component) {
                return Err("quiesce_request_observation_duplicate".to_owned());
            }
        }
        for required in QuiesceComponent::ALL {
            if !seen.contains(&required) {
                return Err(format!("quiesce_request_missing_{}", required.as_str()));
            }
        }
        // A projector that claims to have consumed more than the store durably holds has
        // regressed or fabricated a position; either way the snapshot boundary is not trustworthy.
        if self.projector_cursor > self.target_cursor {
            return Err("quiesce_projector_cursor_ahead_of_source".to_owned());
        }
        valid_digest(&self.request_digest, "quiesce_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("quiesce_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "actor_id": self.actor_id,
            "target_cursor": self.target_cursor,
            "data_epoch": self.data_epoch,
            "authority_epoch": self.authority_epoch,
            "projector_cursor": self.projector_cursor,
            "observations": self.observations,
        }))
    }
}

/// The decision for one quiesce attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuiesceStatus {
    /// Every component is coherent; a snapshot may be taken now.
    Quiesced,
    /// A writer is still active or a component is still draining.
    Draining,
    /// A coherence rule was violated; the store must not be snapshotted.
    Inconsistent,
    /// The state could not be established and must not be assumed safe.
    Unknown,
}

impl QuiesceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quiesced => "quiesced",
            Self::Draining => "draining",
            Self::Inconsistent => "inconsistent",
            Self::Unknown => "unknown",
        }
    }
}

/// The ordered decision for a quiesce request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuiesceReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub operation_id: OperationId,
    pub status: QuiesceStatus,
    /// The cursor a snapshot may be taken at. Zero when the store is not quiesced.
    pub snapshot_cursor: EventCursor,
    /// Stable denial code when the status is not `Quiesced`.
    pub reason: String,
    pub remediation: String,
    pub request_digest: String,
    pub report_digest: String,
}

impl QuiesceReport {
    pub fn evaluate(request: &QuiesceRequest) -> Result<Self, String> {
        request.validate()?;
        let (status, snapshot_cursor, reason, remediation) = derive(request);
        let mut report = Self {
            schema: QUIESCE_REPORT_SCHEMA.to_owned(),
            version: QUIESCE_VERSION,
            operation_id: request.operation_id,
            status,
            snapshot_cursor,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            request_digest: request.request_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    pub fn validate_against(&self, request: &QuiesceRequest) -> Result<(), String> {
        request.validate()?;
        let (status, snapshot_cursor, reason, remediation) = derive(request);
        if self.schema != QUIESCE_REPORT_SCHEMA
            || self.version != QUIESCE_VERSION
            || self.operation_id != request.operation_id
            || self.status != status
            || self.snapshot_cursor != snapshot_cursor
            || self.reason != reason
            || self.remediation != remediation
            || self.request_digest != request.request_digest
        {
            return Err("quiesce_report_binding_invalid".to_owned());
        }
        // A non-quiesced decision must never hand back a cursor a caller could snapshot at.
        if self.status != QuiesceStatus::Quiesced && self.snapshot_cursor != 0 {
            return Err("quiesce_report_cursor_leak".to_owned());
        }
        valid_digest(&self.report_digest, "quiesce_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("quiesce_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "status": self.status,
            "snapshot_cursor": self.snapshot_cursor,
            "reason": self.reason,
            "remediation": self.remediation,
            "request_digest": self.request_digest,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore
/// deterministic for the same facts. Unknown is checked before Inconsistent because an
/// unestablished state cannot be called inconsistent.
fn derive(request: &QuiesceRequest) -> (QuiesceStatus, EventCursor, &'static str, &'static str) {
    if request
        .observations
        .iter()
        .any(|observation| observation.write_state == QuiesceWriteState::Unknown)
    {
        return (
            QuiesceStatus::Unknown,
            0,
            "quiesce_state_unknown",
            "establish every component's write state before snapshotting",
        );
    }
    if request
        .observations
        .iter()
        .any(|observation| observation.write_state == QuiesceWriteState::ActiveWriter)
    {
        return (
            QuiesceStatus::Draining,
            0,
            "quiesce_active_writer",
            "pause admission and let the active writer release the store",
        );
    }
    if request
        .observations
        .iter()
        .any(|observation| observation.write_state == QuiesceWriteState::Draining)
    {
        return (
            QuiesceStatus::Draining,
            0,
            "quiesce_unflushed_remainder",
            "flush buffered appends and confirm the durable cursor",
        );
    }
    // A component that is not yet durable to the target cannot anchor the snapshot boundary.
    if request
        .observations
        .iter()
        .any(|observation| observation.durable_cursor < request.target_cursor)
    {
        return (
            QuiesceStatus::Inconsistent,
            0,
            "quiesce_target_not_durable",
            "flush to the target cursor, or snapshot at the last durable cursor",
        );
    }
    if request.observations.iter().any(|observation| {
        observation.component == QuiesceComponent::EventLogWal
            && observation.wal_consistent == Some(false)
    }) {
        return (
            QuiesceStatus::Inconsistent,
            0,
            "quiesce_wal_main_divergence",
            "recover the write-ahead log into the main file before snapshotting",
        );
    }
    (
        QuiesceStatus::Quiesced,
        request.target_cursor,
        "quiesce_snapshot_admitted",
        "none",
    )
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_QUIESCE_TEXT
        || value.contains(['\0', '\r', '\n'])
        || value.contains("..")
        || value.contains("://")
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
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
