//! PD-26 revocation/delete/expire propagation across the derived data layers.
//!
//! A revoke, delete or expire decision is not finished when the tombstone is committed. It is
//! finished when every derived layer has acknowledged that same tombstone and no longer serves the
//! payload, in a fixed order: facts -> artifact -> memory -> index -> cache -> checkpoint. A layer
//! that still answers with revoked data is the failure this slice exists to name, so a propagation
//! report cannot reach `Complete` while any layer still serves the payload, is unestablished, is
//! missing its receipt, sits at a superseded epoch, or was acknowledged out of order.
//!
//! This module is a read-only decision over adapter-reported observations. It never erases bytes,
//! rewrites a fact, mutates a projection, compacts an index or calls an adapter; the tombstone
//! itself is appended through the existing EventLog path. A rebuild or restore that resurrects a
//! revoked object is refused by [`admit_derived_read_after_recovery`], and a write issued under a
//! superseded data epoch is refused by [`admit_derived_write`].

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, RequestId, SchemaVersion,
    SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const REVOCATION_OBSERVATION_SCHEMA: &str = "kiana.revocation-layer-observation.v1";
pub const REVOCATION_REQUEST_SCHEMA: &str = "kiana.revocation-propagation-request.v1";
pub const REVOCATION_REPORT_SCHEMA: &str = "kiana.revocation-propagation-report.v1";
pub const REVOCATION_PROPAGATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_REVOCATION_TEXT: usize = 256;

/// The derived layers a tombstone has to reach, in the order it must reach them.
///
/// The order is fixed, not advisory. A layer that still answers with revoked data stays reachable
/// for as long as the layer before it in this list has not been fenced, so acknowledgement that
/// skips a layer cannot be treated as progress.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationLayer {
    /// The sealed event/projection facts that record the tombstone itself.
    Facts,
    /// The artifact store that holds the bytes.
    Artifact,
    /// Projected memory rows and summaries.
    Memory,
    /// BM25/dense/repo indexes built from those rows.
    Index,
    /// Prompt/result caches and prepared context plans.
    Cache,
    /// Resume checkpoints and undo snapshots.
    Checkpoint,
}

impl RevocationLayer {
    pub const ALL: [Self; 6] = [
        Self::Facts,
        Self::Artifact,
        Self::Memory,
        Self::Index,
        Self::Cache,
        Self::Checkpoint,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Facts => "facts",
            Self::Artifact => "artifact",
            Self::Memory => "memory",
            Self::Index => "index",
            Self::Cache => "cache",
            Self::Checkpoint => "checkpoint",
        }
    }

    /// Position in the propagation order; the facts layer is step zero.
    pub const fn rank(self) -> usize {
        match self {
            Self::Facts => 0,
            Self::Artifact => 1,
            Self::Memory => 2,
            Self::Index => 3,
            Self::Cache => 4,
            Self::Checkpoint => 5,
        }
    }
}

/// Which governance action is propagating. All three move the same tombstone through the same
/// layers; they differ only in why the payload stops being served.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationKind {
    Revoke,
    Delete,
    Expire,
}

impl RevocationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Revoke => "revoke",
            Self::Delete => "delete",
            Self::Expire => "expire",
        }
    }
}

/// What a derived layer reported about one tombstoned object.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationLayerState {
    /// The layer holds the tombstone at the stated epoch and no longer serves the payload.
    Tombstoned,
    /// The layer still answers with the revoked payload. This is the failure the card names first.
    StillServing,
    /// The layer is reachable and holds the object, but the tombstone has not reached it yet.
    Pending,
    /// The adapter could not establish the state; silence is not acknowledgement.
    Unreachable,
}

impl RevocationLayerState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tombstoned => "tombstoned",
            Self::StillServing => "still_serving",
            Self::Pending => "pending",
            Self::Unreachable => "unreachable",
        }
    }
}

/// One adapter-reported observation about a single derived layer.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationLayerObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub layer: RevocationLayer,
    pub state: RevocationLayerState,
    /// Data epoch the layer holds. A layer restored from an older backup reports the older epoch,
    /// which is exactly how a revoked object comes back after a recovery.
    pub data_epoch: u64,
    /// Cursor the layer has consumed. A layer that lags the tombstone cursor has not seen it yet.
    pub source_cursor: EventCursor,
    /// The layer's own receipt for this tombstone. Acknowledgement without a receipt is a claim.
    pub receipt_digest: Option<String>,
    pub observed_at_ms: u64,
    pub observation_digest: String,
}

impl RevocationLayerObservation {
    pub fn new(
        layer: RevocationLayer,
        state: RevocationLayerState,
        data_epoch: u64,
        source_cursor: EventCursor,
        receipt_digest: Option<String>,
        observed_at_ms: u64,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: REVOCATION_OBSERVATION_SCHEMA.to_owned(),
            version: REVOCATION_PROPAGATION_VERSION,
            layer,
            state,
            data_epoch,
            source_cursor,
            receipt_digest,
            observed_at_ms,
            observation_digest: String::new(),
        };
        value.observation_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REVOCATION_OBSERVATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&REVOCATION_PROPAGATION_VERSION)
            || self.data_epoch == 0
            || self.source_cursor == 0
            || self.observed_at_ms == 0
        {
            return Err("revocation_observation_header_invalid".to_owned());
        }
        // An unreachable layer cannot produce a receipt, a layer that reached a state must show its
        // work, and a layer still holding the object cannot have receipted the tombstone yet.
        // Allowing any of these shapes to float makes "acknowledged" unfalsifiable.
        match (self.state, self.receipt_digest.is_some()) {
            (RevocationLayerState::Unreachable, true) => {
                return Err("revocation_observation_unreachable_with_receipt".to_owned());
            }
            (RevocationLayerState::Tombstoned, false) => {
                return Err("revocation_observation_receipt_required".to_owned());
            }
            (RevocationLayerState::Pending, true) => {
                return Err("revocation_observation_pending_with_receipt".to_owned());
            }
            _ => {}
        }
        if let Some(receipt_digest) = &self.receipt_digest {
            valid_digest(receipt_digest, "revocation_observation_receipt_digest")?;
        }
        valid_digest(&self.observation_digest, "revocation_observation_digest")?;
        if self.observation_digest != self.digest() {
            return Err("revocation_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// A layer counts as acknowledged only when it is tombstoned *and* shows a receipt.
    pub fn acknowledged(&self) -> bool {
        self.state == RevocationLayerState::Tombstoned && self.receipt_digest.is_some()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "layer": self.layer,
            "state": self.state,
            "data_epoch": self.data_epoch,
            "source_cursor": self.source_cursor,
            "receipt_digest": self.receipt_digest,
            "observed_at_ms": self.observed_at_ms,
        }))
    }
}

/// One revoke/delete/expire propagation attempt, with what every layer reported.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationPropagationRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub kind: RevocationKind,
    pub actor_id: String,
    pub project_ref: String,
    pub object_ref: String,
    /// Digest of the committed tombstone every layer must reference.
    pub tombstone_digest: String,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    /// Cursor of the committed tombstone fact. No layer may acknowledge before it.
    pub source_cursor: EventCursor,
    /// One observation per layer, in [`RevocationLayer::ALL`] order.
    pub observations: Vec<RevocationLayerObservation>,
    pub request_digest: String,
}

impl RevocationPropagationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: RequestId,
        kind: RevocationKind,
        actor_id: impl Into<String>,
        project_ref: impl Into<String>,
        object_ref: impl Into<String>,
        tombstone_digest: impl Into<String>,
        previous_epoch: u64,
        data_epoch: u64,
        source_cursor: EventCursor,
        observations: Vec<RevocationLayerObservation>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: REVOCATION_REQUEST_SCHEMA.to_owned(),
            version: REVOCATION_PROPAGATION_VERSION,
            request_id,
            kind,
            actor_id: actor_id.into(),
            project_ref: project_ref.into(),
            object_ref: object_ref.into(),
            tombstone_digest: tombstone_digest.into(),
            previous_epoch,
            data_epoch,
            source_cursor,
            observations,
            request_digest: String::new(),
        };
        value.request_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != REVOCATION_REQUEST_SCHEMA
            || !self
                .version
                .is_compatible_with(&REVOCATION_PROPAGATION_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.previous_epoch == 0
            || self.data_epoch <= self.previous_epoch
            || self.source_cursor == 0
            || self.observations.len() != RevocationLayer::ALL.len()
        {
            return Err("revocation_request_header_invalid".to_owned());
        }
        safe_text(&self.actor_id, "revocation_actor")?;
        safe_text(&self.project_ref, "revocation_project")?;
        safe_text(&self.object_ref, "revocation_object_ref")?;
        valid_digest(&self.tombstone_digest, "revocation_tombstone_digest")?;
        // Observations arrive in the fixed propagation order so the wire form is stable and a
        // reordered report cannot be mistaken for a different attempt.
        for (index, observation) in self.observations.iter().enumerate() {
            observation.validate()?;
            let Some(expected) = RevocationLayer::ALL.get(index) else {
                return Err("revocation_request_observation_count_invalid".to_owned());
            };
            if observation.layer != *expected {
                return Err("revocation_request_observation_out_of_order".to_owned());
            }
        }
        valid_digest(&self.request_digest, "revocation_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("revocation_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// The layers that have not yet acknowledged this tombstone, in propagation order.
    pub fn pending_layers(&self) -> Result<Vec<RevocationLayer>, String> {
        self.validate()?;
        Ok(pending_layers(self))
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "kind": self.kind,
            "actor_id": self.actor_id,
            "project_ref": self.project_ref,
            "object_ref": self.object_ref,
            "tombstone_digest": self.tombstone_digest,
            "previous_epoch": self.previous_epoch,
            "data_epoch": self.data_epoch,
            "source_cursor": self.source_cursor,
            "observations": self.observations,
        }))
    }
}

/// Whether a propagation attempt may be treated as finished.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevocationPropagationStatus {
    /// Every layer acknowledged the tombstone at the current epoch, in order.
    Complete,
    /// The tombstone has not reached every layer yet.
    Incomplete,
    /// At least one layer is still serving the revoked payload. Never a completion.
    RevokedDataServed,
}

impl RevocationPropagationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Incomplete => "incomplete",
            Self::RevokedDataServed => "revoked_data_served",
        }
    }
}

/// The ordered decision for one propagation attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationPropagationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub kind: RevocationKind,
    pub status: RevocationPropagationStatus,
    pub data_epoch: u64,
    /// The leading run of layers that acknowledged the tombstone, in propagation order.
    pub acknowledged_layers: Vec<RevocationLayer>,
    pub pending_layers: Vec<RevocationLayer>,
    /// Stable denial code, suffixed with the offending layer. Empty on a complete propagation.
    pub reason: String,
    pub remediation: String,
    pub request_digest: String,
    pub report_digest: String,
}

impl RevocationPropagationReport {
    pub fn evaluate(request: &RevocationPropagationRequest) -> Result<Self, String> {
        request.validate()?;
        let (status, acknowledged, pending, reason, remediation) = derive(request);
        let mut report = Self {
            schema: REVOCATION_REPORT_SCHEMA.to_owned(),
            version: REVOCATION_PROPAGATION_VERSION,
            request_id: request.request_id,
            kind: request.kind,
            status,
            data_epoch: request.data_epoch,
            acknowledged_layers: acknowledged,
            pending_layers: pending,
            reason,
            remediation,
            request_digest: request.request_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(request)?;
        Ok(report)
    }

    pub fn validate_against(&self, request: &RevocationPropagationRequest) -> Result<(), String> {
        request.validate()?;
        let (status, acknowledged, pending, reason, remediation) = derive(request);
        if self.schema != REVOCATION_REPORT_SCHEMA
            || !self
                .version
                .is_compatible_with(&REVOCATION_PROPAGATION_VERSION)
            || self.request_id != request.request_id
            || self.kind != request.kind
            || self.status != status
            || self.data_epoch != request.data_epoch
            || self.acknowledged_layers != acknowledged
            || self.pending_layers != pending
            || self.reason != reason
            || self.remediation != remediation
            || self.request_digest != request.request_digest
        {
            return Err("revocation_report_binding_invalid".to_owned());
        }
        // A report that names pending layers is not a completion, whatever else it claims.
        if self.status == RevocationPropagationStatus::Complete
            && (!self.pending_layers.is_empty() || !self.reason.is_empty())
        {
            return Err("revocation_report_completion_with_pending".to_owned());
        }
        valid_digest(&self.report_digest, "revocation_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("revocation_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether the object may now be treated as unreadable in every derived layer.
    pub fn complete(&self) -> bool {
        self.status == RevocationPropagationStatus::Complete
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "kind": self.kind,
            "status": self.status,
            "data_epoch": self.data_epoch,
            "acknowledged_layers": self.acknowledged_layers,
            "pending_layers": self.pending_layers,
            "reason": self.reason,
            "remediation": self.remediation,
            "request_digest": self.request_digest,
        }))
    }
}

/// Decide in a fixed order, so the reported reason is the first violated rule and therefore the
/// same for the same facts. A layer that still serves the payload outranks everything else,
/// because it is a positive observation of a leak rather than an absence of proof. The remaining
/// rules are each an absence of proof, so they are ordered by how much a caller could be misled
/// by getting them wrong: an unestablished layer, a layer that has not taken the tombstone, a
/// layer whose receipt never arrived, a layer sitting at the wrong epoch, and finally a layer that
/// acknowledged out of order rather than merely not yet.
fn derive(
    request: &RevocationPropagationRequest,
) -> (
    RevocationPropagationStatus,
    Vec<RevocationLayer>,
    Vec<RevocationLayer>,
    String,
    String,
) {
    let acknowledged = acknowledged_prefix(request);
    let pending = pending_layers(request);
    if let Some(observation) = request
        .observations
        .iter()
        .find(|observation| observation.state == RevocationLayerState::StillServing)
    {
        let layer = observation.layer;
        return (
            RevocationPropagationStatus::RevokedDataServed,
            acknowledged,
            pending,
            format!(
                "revocation_derived_layer_still_serves_data:{}",
                layer.as_str()
            ),
            format!(
                "invalidate the {} layer at epoch {} before reporting propagation",
                layer.as_str(),
                request.data_epoch
            ),
        );
    }
    if let Some(observation) = request
        .observations
        .iter()
        .find(|observation| observation.state == RevocationLayerState::Unreachable)
    {
        let layer = observation.layer;
        return (
            RevocationPropagationStatus::Incomplete,
            acknowledged,
            pending,
            format!("revocation_layer_state_unknown:{}", layer.as_str()),
            format!(
                "establish the {} layer's state; an unreachable layer is not an acknowledgement",
                layer.as_str()
            ),
        );
    }
    if let Some(observation) = request
        .observations
        .iter()
        .find(|observation| observation.state == RevocationLayerState::Pending)
    {
        let layer = observation.layer;
        return (
            RevocationPropagationStatus::Incomplete,
            acknowledged,
            pending,
            format!("revocation_layer_tombstone_not_applied:{}", layer.as_str()),
            format!(
                "apply the tombstone to the {} layer at epoch {} before continuing",
                layer.as_str(),
                request.data_epoch
            ),
        );
    }
    if let Some(observation) = request
        .observations
        .iter()
        .find(|observation| observation.data_epoch != request.data_epoch)
    {
        let layer = observation.layer;
        return (
            RevocationPropagationStatus::Incomplete,
            acknowledged,
            pending,
            format!("revocation_layer_epoch_stale:{}", layer.as_str()),
            format!(
                "re-run the {} layer at epoch {}; it currently holds epoch {}",
                layer.as_str(),
                request.data_epoch,
                observation.data_epoch
            ),
        );
    }
    // Acknowledgement is credited only as a contiguous prefix of the propagation order, so the
    // one case that is not merely "not finished" is a layer that jumped the queue. The earlier
    // layer is still reachable, which makes the later acknowledgement unearned rather than early.
    let skipped = RevocationLayer::ALL.into_iter().find(|layer| {
        pending.contains(layer)
            && acknowledged
                .last()
                .is_some_and(|done| done.rank() > layer.rank())
    });
    if let Some(skipped) = skipped {
        return (
            RevocationPropagationStatus::Incomplete,
            acknowledged,
            pending,
            format!("revocation_layer_order_violation:{}", skipped.as_str()),
            format!(
                "acknowledge the {} layer before anything after it in the propagation order",
                skipped.as_str()
            ),
        );
    }
    if let Some(first_pending) = pending.first() {
        let reason = format!(
            "revocation_propagation_incomplete:{}",
            first_pending.as_str()
        );
        let remediation = format!(
            "propagate the tombstone to the remaining {} of {} layers",
            pending.len(),
            RevocationLayer::ALL.len()
        );
        return (
            RevocationPropagationStatus::Incomplete,
            acknowledged,
            pending,
            reason,
            remediation,
        );
    }
    (
        RevocationPropagationStatus::Complete,
        acknowledged,
        pending,
        String::new(),
        String::new(),
    )
}

/// The leading run of acknowledged layers. Acknowledgement is only credited contiguously, so an
/// out-of-order acknowledgement cannot shorten the gap it left behind.
fn acknowledged_prefix(request: &RevocationPropagationRequest) -> Vec<RevocationLayer> {
    let mut acknowledged = Vec::with_capacity(RevocationLayer::ALL.len());
    for layer in RevocationLayer::ALL {
        let done =
            observation_of(request, layer).is_some_and(RevocationLayerObservation::acknowledged);
        if !done {
            break;
        }
        acknowledged.push(layer);
    }
    acknowledged
}

/// Every layer that has not acknowledged, in propagation order.
fn pending_layers(request: &RevocationPropagationRequest) -> Vec<RevocationLayer> {
    RevocationLayer::ALL
        .into_iter()
        .filter(|layer| {
            !observation_of(request, *layer).is_some_and(RevocationLayerObservation::acknowledged)
        })
        .collect()
}

fn observation_of(
    request: &RevocationPropagationRequest,
    layer: RevocationLayer,
) -> Option<&RevocationLayerObservation> {
    request
        .observations
        .iter()
        .find(|observation| observation.layer == layer)
}

/// Fold a repeated layer report into the one already recorded.
///
/// Replaying the same receipt is idempotent: a retried adapter call must not move the recorded
/// state, and it must not be reported as a second deletion either. A different receipt for a layer
/// that already acknowledged is a conflict, not an update, because the tombstone it names is the
/// same one and two receipts for it mean one of them is fabricated.
pub fn merge_layer_observation(
    existing: &RevocationLayerObservation,
    incoming: &RevocationLayerObservation,
) -> Result<RevocationLayerObservation, String> {
    existing.validate()?;
    incoming.validate()?;
    if existing.layer != incoming.layer {
        return Err("revocation_receipt_layer_mismatch".to_owned());
    }
    if existing == incoming {
        return Ok(existing.clone());
    }
    if existing.acknowledged() && !incoming.acknowledged() {
        return Err("revocation_receipt_regression".to_owned());
    }
    if existing.acknowledged()
        && incoming.acknowledged()
        && existing.receipt_digest != incoming.receipt_digest
    {
        return Err("revocation_receipt_conflict".to_owned());
    }
    if existing.observed_at_ms > incoming.observed_at_ms {
        return Err("revocation_receipt_regression".to_owned());
    }
    Ok(incoming.clone())
}

/// Refuse a derived-store write that was issued under a superseded data epoch.
///
/// A writer that has not observed the epoch bump would re-materialize exactly what the tombstone
/// removed, so the write is rejected before it reaches a store. An epoch ahead of the current one
/// is rejected too: it means the writer and the authority have diverged.
pub fn admit_derived_write(write_data_epoch: u64, current_data_epoch: u64) -> Result<(), String> {
    if write_data_epoch == 0 || current_data_epoch == 0 {
        return Err("revocation_write_epoch_required".to_owned());
    }
    if write_data_epoch < current_data_epoch {
        return Err("revocation_stale_data_epoch_write".to_owned());
    }
    if write_data_epoch > current_data_epoch {
        return Err("revocation_write_epoch_ahead".to_owned());
    }
    Ok(())
}

/// Refuse a read from a rebuilt or restored derived store that still serves revoked data.
///
/// Recovery restores bytes, not governance. A layer that answers from a pre-revocation backup
/// either still serves the payload or reports the older epoch it was restored at; both are
/// refusals, because a successful restore is not evidence that the tombstone survived it.
pub fn admit_derived_read_after_recovery(
    observation: &RevocationLayerObservation,
    current_data_epoch: u64,
) -> Result<(), String> {
    observation.validate()?;
    if current_data_epoch == 0 {
        return Err("revocation_read_epoch_required".to_owned());
    }
    if observation.state == RevocationLayerState::StillServing {
        return Err("revocation_tombstone_revival_denied".to_owned());
    }
    if observation.data_epoch < current_data_epoch {
        return Err("revocation_recovered_epoch_superseded".to_owned());
    }
    Ok(())
}

/// The tombstone references every layer must agree on, for an audit that has to name them.
pub fn propagation_scope() -> BTreeSet<&'static str> {
    RevocationLayer::ALL
        .into_iter()
        .map(RevocationLayer::as_str)
        .collect()
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_REVOCATION_TEXT
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
