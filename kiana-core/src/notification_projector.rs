//! Rebuildable notification lifecycle projector.
//!
//! The projector folds committed materializations plus append-only withdraw/supersede/expire
//! facts. It never deletes source history and never mutates HumanTask/Approval authority.
//!
//! NM-12 adds the durability surface the card asks for:
//!
//! - [`NotificationInboxSnapshot`] binds one recipient's inbox view, its lifecycle visibility and
//!   its durable read/ACK/snooze state to the committed `source_cursor` and `data_epoch`, so a
//!   fresh process rebuild and an incremental apply can be compared byte for byte;
//! - [`NotificationReadStateFact`] is the append-only fact form of the in-process
//!   `NotificationProjectionMutation` receipt, so unread/read/ACK/snooze state is rebuilt from the
//!   committed source cursor instead of dying with the process;
//! - [`NotificationRetentionDecision`] is a governance decision that may only be admitted when it
//!   is authorized by the server principal, covers every payload-bearing derived layer
//!   (summary/artifact/index/cache) at the decision's data epoch, keeps audit metadata separate
//!   from payload references, and never targets the facts layer;
//! - a withdrawal/supersede/expiry only ever appends a lifecycle fact: source events, materialized
//!   items and the HumanTask/Approval authority objects are never rewritten or removed.

use crate::{
    NotificationMaterializer, NotificationProjectionMutation, ReplayProjection, RevocationLayer,
    RevocationLayerObservation, RevocationLayerState,
};
use kiana_domain::{
    json_digest, redact_text, validate_notification_runtime_event, EventId, HumanInboxItem,
    Notification, NotificationEventSource, NotificationId, NotificationLifecycleFact,
    NotificationLifecycleKind, ProjectionCheckpoint, ReceiptAuditMetadata, ReceiptPayloadRef,
    RuntimeEvent, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const NOTIFICATION_PROJECTOR_SCHEMA: &str = "kiana.notification-projector.v1";
pub const NOTIFICATION_INBOX_SNAPSHOT_SCHEMA: &str = "kiana.notification-inbox-snapshot.v1";
pub const NOTIFICATION_READ_STATE_SCHEMA: &str = "kiana.notification-read-state.v1";
pub const NOTIFICATION_RETENTION_SCHEMA: &str = "kiana.notification-retention.v1";
pub const NOTIFICATION_RETENTION_ADMISSION_SCHEMA: &str =
    "kiana.notification-retention-admission.v1";
pub const NOTIFICATION_RETENTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_NOTIFICATION_LIFECYCLE_TEXT: usize = 256;
pub const MAX_NOTIFICATION_LIFECYCLE_REASON: usize = 512;
pub const MAX_NOTIFICATION_PAYLOAD_REFS: usize = 64;
pub const MAX_NOTIFICATION_LEGAL_HOLDS: usize = 64;

/// The payload-bearing derived layers a notification retention decision must cover.
///
/// This is the `Artifact → Memory → Index → Cache` subsequence of PD-26's
/// [`RevocationLayer::ALL`]: the projected summary lives in `Memory`, and `Facts`/`Checkpoint`
/// are deliberately absent because notification retention never deletes the original committed
/// facts or the resume checkpoint. Requiring the whole subsequence is what makes "delete only the
/// UI/cache row" fail closed: a decision that acknowledges just the cache layer cannot claim the
/// payload is gone while the summary, artifact or index still serves it.
pub const NOTIFICATION_RETENTION_PAYLOAD_LAYERS: [RevocationLayer; 4] = [
    RevocationLayer::Artifact,
    RevocationLayer::Memory,
    RevocationLayer::Index,
    RevocationLayer::Cache,
];

fn bounded_text(value: &str, field: &'static str, max: usize) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.contains(['\0', '\n', '\r'])
        || redact_text(value) != value
    {
        return Err(format!("notification_projector_invalid:{field}"));
    }
    Ok(())
}

fn payload_layer_index(layer: RevocationLayer) -> Option<usize> {
    NOTIFICATION_RETENTION_PAYLOAD_LAYERS
        .iter()
        .position(|candidate| *candidate == layer)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationVisibility {
    Active,
    Withdrawn,
    Superseded,
    Expired,
}

impl NotificationVisibility {
    fn from_fact(fact: &NotificationLifecycleFact) -> Self {
        match fact.kind {
            NotificationLifecycleKind::Withdraw => Self::Withdrawn,
            NotificationLifecycleKind::Supersede => Self::Superseded,
            NotificationLifecycleKind::Expire => Self::Expired,
        }
    }
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum NotificationProjectorError {
    #[error("notification_projector_invalid:{0}")]
    Invalid(String),
    #[error("notification_projector_cursor_conflict")]
    CursorConflict,
    #[error("notification_projector_epoch_regressed")]
    EpochRegressed,
    #[error("notification_projector_fact_conflict")]
    FactConflict,
    #[error("notification_projector_terminal_rewrite")]
    TerminalRewrite,
    #[error("notification_projector_materializer:{0}")]
    Materializer(String),
    #[error("notification_projector_projection_unavailable")]
    ProjectionUnavailable,
    #[error("notification_projector_unknown_notification")]
    UnknownNotification,
    #[error("notification_projector_read_state_conflict")]
    ReadStateConflict,
    #[error("notification_projector_read_state_regression")]
    ReadStateRegression,
    #[error("notification_projector_retention_unauthorized")]
    RetentionUnauthorized,
    #[error("notification_projector_retention_policy_stale")]
    RetentionPolicyStale,
    #[error("notification_projector_retention_legal_hold")]
    RetentionLegalHold,
    #[error("notification_projector_retention_epoch_conflict")]
    RetentionEpochConflict,
}

/// The append-only fact form of one presentation read/ACK/snooze mutation.
///
/// The in-process `NotificationProjectionMutation` receipt is what an entrypoint shows the user;
/// this fact is what a fresh process replays. They carry the same presentation fields plus the
/// committed `source_event_id`, so an exact replay is idempotent and two different facts claiming
/// the same projection revision are a conflict rather than a silent overwrite.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationReadStateFact {
    pub schema: String,
    pub item_id: String,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub projection_revision: u64,
    pub read: bool,
    pub acknowledged: bool,
    #[serde(default)]
    pub snoozed_until_unix_ms: Option<u64>,
    pub fact_digest: String,
}

impl NotificationReadStateFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        item_id: impl Into<String>,
        source_event_id: EventId,
        source_cursor: u64,
        projection_revision: u64,
        read: bool,
        acknowledged: bool,
        snoozed_until_unix_ms: Option<u64>,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: NOTIFICATION_READ_STATE_SCHEMA.to_owned(),
            item_id: item_id.into(),
            source_event_id,
            source_cursor,
            projection_revision,
            read,
            acknowledged,
            snoozed_until_unix_ms,
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    /// Turn one presentation mutation receipt into its replayable fact.
    ///
    /// The committed event id is a parameter, never copied from the receipt: the receipt is a
    /// client-visible acknowledgement and must not be able to name the fact that commits it.
    pub fn from_projection_mutation(
        mutation: &NotificationProjectionMutation,
        source_event_id: EventId,
    ) -> Result<Self, String> {
        mutation
            .validate()
            .map_err(|error| format!("notification_read_state_receipt:{error}"))?;
        Self::new(
            mutation.item_id.clone(),
            source_event_id,
            mutation.source_cursor,
            mutation.projection_revision,
            mutation.read,
            mutation.acknowledged,
            mutation.snoozed_until_unix_ms,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_READ_STATE_SCHEMA
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.projection_revision == 0
            || self.snoozed_until_unix_ms == Some(0)
        {
            return Err("notification_read_state_header_invalid".to_owned());
        }
        bounded_text(
            &self.item_id,
            "read_state_item_id",
            MAX_NOTIFICATION_LIFECYCLE_TEXT,
        )
        .map_err(|_| "notification_read_state_item_invalid".to_owned())?;
        if self.acknowledged && !self.read {
            return Err("notification_read_state_acknowledgement".to_owned());
        }
        if self.fact_digest != self.digest() {
            return Err("notification_read_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "item_id": self.item_id,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "projection_revision": self.projection_revision,
            "read": self.read,
            "acknowledged": self.acknowledged,
            "snoozed_until_unix_ms": self.snoozed_until_unix_ms,
        }))
    }
}

/// One recipient-scoped inbox view bound to the committed source cursor and data epoch.
///
/// The snapshot is a projection, not an authority record: it contains display items, their
/// lifecycle visibility and their presentation read state, and carries no HumanTask/Approval
/// status. Two rebuilds from the same committed inputs must produce the same digest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationInboxEntry {
    pub item: HumanInboxItem,
    pub visibility: NotificationVisibility,
    pub read: bool,
    pub acknowledged: bool,
    #[serde(default)]
    pub snoozed_until_unix_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationInboxSnapshot {
    pub schema: String,
    pub recipient_id: String,
    pub source_cursor: u64,
    pub data_epoch: u64,
    pub entries: Vec<NotificationInboxEntry>,
    pub snapshot_digest: String,
}

impl NotificationInboxSnapshot {
    pub fn new(
        recipient_id: impl Into<String>,
        source_cursor: u64,
        data_epoch: u64,
        entries: Vec<NotificationInboxEntry>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: NOTIFICATION_INBOX_SNAPSHOT_SCHEMA.to_owned(),
            recipient_id: recipient_id.into(),
            source_cursor,
            data_epoch,
            entries,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_INBOX_SNAPSHOT_SCHEMA
            || self.recipient_id.trim().is_empty()
            || self.recipient_id.len() > MAX_NOTIFICATION_LIFECYCLE_TEXT
            || self.source_cursor == 0
        {
            return Err("notification_inbox_snapshot_header_invalid".to_owned());
        }
        if self.snapshot_digest != self.digest() {
            return Err("notification_inbox_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "recipient_id": self.recipient_id,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "entries": self.entries,
        }))
    }
}

/// A server-authorized retention decision over one notification.
///
/// The decision is a governance fact, not a HumanTask/Approval mutation: it names the committed
/// source event and cursor it was derived from, the data epoch it advances to, the payload
/// references that stop being served, the audit metadata that is kept, and one observation per
/// payload-bearing derived layer. Audit metadata and payload references are separate fields of
/// separate types ([`ReceiptAuditMetadata`] and [`ReceiptPayloadRef`]) with `deny_unknown_fields`,
/// so a decision that smuggles a payload reference into audit metadata (or an audit field into a
/// payload reference) fails to decode instead of being reinterpreted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationRetentionDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub decision_id: String,
    pub kind: NotificationLifecycleKind,
    pub notification_id: NotificationId,
    #[serde(default)]
    pub replacement_notification_id: Option<NotificationId>,
    pub authorized_by: String,
    pub retention_policy_revision: u64,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    pub source_event_id: EventId,
    pub source_cursor: u64,
    pub audit_metadata: ReceiptAuditMetadata,
    pub payload_refs: Vec<ReceiptPayloadRef>,
    pub layer_observations: Vec<RevocationLayerObservation>,
    #[serde(default)]
    pub legal_hold_ids: Vec<String>,
    pub reason: String,
    pub decision_digest: String,
}

impl NotificationRetentionDecision {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        decision_id: impl Into<String>,
        kind: NotificationLifecycleKind,
        notification_id: NotificationId,
        replacement_notification_id: Option<NotificationId>,
        authorized_by: impl Into<String>,
        retention_policy_revision: u64,
        previous_epoch: u64,
        data_epoch: u64,
        source_event_id: EventId,
        source_cursor: u64,
        audit_metadata: ReceiptAuditMetadata,
        payload_refs: Vec<ReceiptPayloadRef>,
        layer_observations: Vec<RevocationLayerObservation>,
        legal_hold_ids: Vec<String>,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut decision = Self {
            schema: NOTIFICATION_RETENTION_SCHEMA.to_owned(),
            version: NOTIFICATION_RETENTION_VERSION,
            decision_id: decision_id.into(),
            kind,
            notification_id,
            replacement_notification_id,
            authorized_by: authorized_by.into(),
            retention_policy_revision,
            previous_epoch,
            data_epoch,
            source_event_id,
            source_cursor,
            audit_metadata,
            payload_refs,
            layer_observations,
            legal_hold_ids,
            reason: reason.into(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_RETENTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&NOTIFICATION_RETENTION_VERSION)
            || self.notification_id.as_uuid().is_nil()
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.data_epoch == 0
            || self.previous_epoch >= self.data_epoch
            || self.retention_policy_revision == 0
            || self.payload_refs.len() > MAX_NOTIFICATION_PAYLOAD_REFS
            || self.legal_hold_ids.len() > MAX_NOTIFICATION_LEGAL_HOLDS
        {
            return Err("notification_retention_header_invalid".to_owned());
        }
        bounded_text(
            &self.decision_id,
            "retention_decision_id",
            MAX_NOTIFICATION_LIFECYCLE_TEXT,
        )
        .map_err(|_| "notification_retention_decision_id_invalid".to_owned())?;
        bounded_text(
            &self.authorized_by,
            "retention_authorized_by",
            MAX_NOTIFICATION_LIFECYCLE_TEXT,
        )
        .map_err(|_| "notification_retention_authorized_by_invalid".to_owned())?;
        if self.reason.trim().is_empty()
            || self.reason.len() > MAX_NOTIFICATION_LIFECYCLE_REASON
            || self.reason.contains(['\0', '\n', '\r'])
            || redact_text(&self.reason) != self.reason
        {
            return Err("notification_retention_reason_invalid".to_owned());
        }
        match self.kind {
            NotificationLifecycleKind::Supersede => {
                let replacement = self
                    .replacement_notification_id
                    .ok_or_else(|| "notification_retention_replacement_required".to_owned())?;
                if replacement.as_uuid().is_nil() || replacement == self.notification_id {
                    return Err("notification_retention_replacement_invalid".to_owned());
                }
            }
            NotificationLifecycleKind::Withdraw | NotificationLifecycleKind::Expire => {
                if self.replacement_notification_id.is_some() {
                    return Err("notification_retention_replacement_unexpected".to_owned());
                }
            }
        }

        // The audit metadata is what survives: it must be retained, must describe the same
        // cursor/epoch/policy revision as the decision, and must name the decision's own source
        // event. A decision whose audit trail does not contain its source event has no audit
        // anchor and is refused.
        self.audit_metadata.validate()?;
        if !self.audit_metadata.retained {
            return Err("notification_retention_audit_not_retained".to_owned());
        }
        if self.audit_metadata.data_epoch != self.data_epoch {
            return Err("notification_retention_audit_epoch_mismatch".to_owned());
        }
        if self.audit_metadata.source_cursor != self.source_cursor {
            return Err("notification_retention_audit_cursor_mismatch".to_owned());
        }
        if self.audit_metadata.policy_revision != self.retention_policy_revision {
            return Err("notification_retention_policy_conflict".to_owned());
        }
        if !self
            .audit_metadata
            .source_event_ids
            .contains(&self.source_event_id)
        {
            return Err("notification_retention_source_event_unbound".to_owned());
        }

        // Payload references are digest-only identities bound to the decision epoch; they are the
        // only thing retention drops, and they can never be an audit event identity.
        if self.payload_refs.is_empty() {
            return Err("notification_retention_payload_required".to_owned());
        }
        let mut objects = BTreeSet::new();
        for payload_ref in &self.payload_refs {
            payload_ref.validate()?;
            if payload_ref.data_epoch != self.data_epoch {
                return Err("notification_retention_payload_epoch_stale".to_owned());
            }
            if !objects.insert(payload_ref.object_ref.clone()) {
                return Err("notification_retention_payload_duplicate".to_owned());
            }
            if self
                .audit_metadata
                .source_event_ids
                .iter()
                .any(|event_id| event_id.to_string() == payload_ref.object_ref)
            {
                return Err("notification_retention_audit_ref_confusion".to_owned());
            }
        }

        // Legal holds are allowed to be carried so the refusal can name them; the projector refuses
        // an admission while any hold is present.
        let mut hold_ids = BTreeSet::new();
        for hold_id in &self.legal_hold_ids {
            bounded_text(
                hold_id,
                "retention_legal_hold_id",
                MAX_NOTIFICATION_LIFECYCLE_TEXT,
            )
            .map_err(|_| "notification_retention_legal_hold_id_invalid".to_owned())?;
            if !hold_ids.insert(hold_id.clone()) {
                return Err("notification_retention_legal_hold_duplicate".to_owned());
            }
        }

        self.validate_payload_layers()?;

        if self.decision_digest != self.digest() {
            return Err("notification_retention_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Every payload-bearing derived layer must acknowledge the same epoch, in the fixed
    /// `Artifact → Memory → Index → Cache` order. The facts layer is out of scope by construction:
    /// a decision that tries to delete the original fact is refused here, before any admission.
    fn validate_payload_layers(&self) -> Result<(), String> {
        if self.layer_observations.len() != NOTIFICATION_RETENTION_PAYLOAD_LAYERS.len() {
            for observation in &self.layer_observations {
                observation.validate()?;
                if observation.layer == RevocationLayer::Facts {
                    return Err("notification_retention_facts_immutable".to_owned());
                }
                if payload_layer_index(observation.layer).is_none() {
                    return Err("notification_retention_layer_out_of_scope".to_owned());
                }
            }
            return Err("notification_retention_layer_incomplete".to_owned());
        }
        for (index, observation) in self.layer_observations.iter().enumerate() {
            observation.validate()?;
            if observation.layer == RevocationLayer::Facts {
                return Err("notification_retention_facts_immutable".to_owned());
            }
            let Some(expected) = NOTIFICATION_RETENTION_PAYLOAD_LAYERS.get(index) else {
                return Err("notification_retention_layer_out_of_scope".to_owned());
            };
            if observation.layer != *expected {
                if payload_layer_index(observation.layer).is_none() {
                    return Err("notification_retention_layer_out_of_scope".to_owned());
                }
                return Err("notification_retention_layer_out_of_order".to_owned());
            }
            if observation.source_cursor > self.source_cursor {
                return Err("notification_retention_layer_cursor_ahead".to_owned());
            }
            if observation.data_epoch != self.data_epoch {
                return Err(format!(
                    "notification_retention_layer_epoch_stale:{}",
                    observation.layer.as_str()
                ));
            }
            match observation.state {
                RevocationLayerState::Tombstoned => {}
                RevocationLayerState::StillServing => {
                    return Err(format!(
                        "notification_retention_layer_still_serving:{}",
                        observation.layer.as_str()
                    ));
                }
                RevocationLayerState::Pending => {
                    return Err(format!(
                        "notification_retention_layer_pending:{}",
                        observation.layer.as_str()
                    ));
                }
                RevocationLayerState::Unreachable => {
                    return Err(format!(
                        "notification_retention_layer_unreachable:{}",
                        observation.layer.as_str()
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "decision_id": self.decision_id,
            "kind": self.kind,
            "notification_id": self.notification_id,
            "replacement_notification_id": self.replacement_notification_id,
            "authorized_by": self.authorized_by,
            "retention_policy_revision": self.retention_policy_revision,
            "previous_epoch": self.previous_epoch,
            "data_epoch": self.data_epoch,
            "source_event_id": self.source_event_id,
            "source_cursor": self.source_cursor,
            "audit_metadata": self.audit_metadata,
            "payload_refs": self.payload_refs,
            "layer_observations": self.layer_observations,
            "legal_hold_ids": self.legal_hold_ids,
            "reason": self.reason,
        }))
    }
}

/// The projection receipt of one admitted retention decision.
///
/// The receipt records that a lifecycle fact was appended and which derived layers now sit at the
/// decision's data epoch. It carries no approval, decision or task-status field, so a forged
/// receipt carrying one fails to decode at the `deny_unknown_fields` boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotificationRetentionAdmission {
    pub schema: String,
    pub decision_digest: String,
    pub notification_id: NotificationId,
    pub kind: NotificationLifecycleKind,
    pub visibility: NotificationVisibility,
    pub source_cursor: u64,
    pub data_epoch: u64,
    pub appended_fact_digest: String,
    pub retained_source_event_count: usize,
    pub admission_digest: String,
}

impl NotificationRetentionAdmission {
    fn new(
        decision: &NotificationRetentionDecision,
        visibility: NotificationVisibility,
        appended_fact_digest: String,
        retained_source_event_count: usize,
    ) -> Result<Self, String> {
        let mut admission = Self {
            schema: NOTIFICATION_RETENTION_ADMISSION_SCHEMA.to_owned(),
            decision_digest: decision.decision_digest.clone(),
            notification_id: decision.notification_id,
            kind: decision.kind,
            visibility,
            source_cursor: decision.source_cursor,
            data_epoch: decision.data_epoch,
            appended_fact_digest,
            retained_source_event_count,
            admission_digest: String::new(),
        };
        admission.admission_digest = admission.digest();
        admission.validate()?;
        Ok(admission)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NOTIFICATION_RETENTION_ADMISSION_SCHEMA
            || self.notification_id.as_uuid().is_nil()
            || self.source_cursor == 0
            || self.data_epoch == 0
        {
            return Err("notification_retention_admission_invalid".to_owned());
        }
        if self.admission_digest != self.digest() {
            return Err("notification_retention_admission_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "decision_digest": self.decision_digest,
            "notification_id": self.notification_id,
            "kind": self.kind,
            "visibility": self.visibility,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "appended_fact_digest": self.appended_fact_digest,
            "retained_source_event_count": self.retained_source_event_count,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationProjector {
    materializer: NotificationMaterializer,
    lifecycle: BTreeMap<String, NotificationLifecycleFact>,
    read_state: BTreeMap<String, NotificationReadStateFact>,
    layer_epochs: BTreeMap<RevocationLayer, u64>,
    retention_decisions: Vec<String>,
    source_cursor: u64,
    data_epoch: u64,
}

impl NotificationProjector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn rebuild_from_committed(
        events: &[RuntimeEvent],
        source_cursor: u64,
        lifecycle_facts: &[NotificationLifecycleFact],
    ) -> Result<Self, NotificationProjectorError> {
        let mut projector = Self::new();
        projector.apply_committed(events, source_cursor, lifecycle_facts)?;
        Ok(projector)
    }

    /// Rebuild the whole durable inbox state from committed inputs in one deterministic call:
    /// committed materialization events, append-only lifecycle facts and read-state facts.
    pub fn rebuild(
        events: &[RuntimeEvent],
        source_cursor: u64,
        lifecycle_facts: &[NotificationLifecycleFact],
        read_state_facts: &[NotificationReadStateFact],
    ) -> Result<Self, NotificationProjectorError> {
        let mut projector = Self::new();
        projector.apply_committed(events, source_cursor, lifecycle_facts)?;
        projector.apply_read_state_facts(read_state_facts)?;
        Ok(projector)
    }

    /// Rebuild the durable inbox state including admitted retention decisions.
    ///
    /// Replaying the same decisions in the same order restores the same per-layer data epochs and
    /// therefore the same inbox snapshot. A decision whose lifecycle fact is already part of
    /// `lifecycle_facts` replays through the idempotent branch instead of appending a second fact.
    #[allow(clippy::too_many_arguments)]
    pub fn rebuild_with_retention(
        events: &[RuntimeEvent],
        source_cursor: u64,
        lifecycle_facts: &[NotificationLifecycleFact],
        read_state_facts: &[NotificationReadStateFact],
        retention_decisions: &[NotificationRetentionDecision],
        server_principal_id: &str,
        current_policy_revision: u64,
    ) -> Result<Self, NotificationProjectorError> {
        let mut projector = Self::new();
        projector.apply_committed(events, source_cursor, lifecycle_facts)?;
        projector.apply_read_state_facts(read_state_facts)?;
        for decision in retention_decisions {
            projector.admit_retention(decision, server_principal_id, current_policy_revision)?;
        }
        Ok(projector)
    }

    pub fn apply_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
        lifecycle_facts: &[NotificationLifecycleFact],
    ) -> Result<(), NotificationProjectorError> {
        if source_cursor == 0 || source_cursor < self.source_cursor {
            return Err(NotificationProjectorError::CursorConflict);
        }
        let mut next_materializer = self.materializer.clone();
        next_materializer
            .apply_committed(events, source_cursor)
            .map_err(NotificationProjectorError::Materializer)?;
        let mut next_lifecycle = self.lifecycle.clone();
        let mut next_epoch = self.data_epoch;
        for fact in lifecycle_facts {
            fact.validate()
                .map_err(NotificationProjectorError::Invalid)?;
            if fact.source_cursor > source_cursor {
                return Err(NotificationProjectorError::CursorConflict);
            }
            if fact.data_epoch < next_epoch {
                return Err(NotificationProjectorError::EpochRegressed);
            }
            next_epoch = next_epoch.max(fact.data_epoch);
            let key = fact.notification_id.to_string();
            if let Some(previous) = next_lifecycle.get(&key) {
                if previous.fact_digest == fact.fact_digest {
                    continue;
                }
                if previous.source_cursor >= fact.source_cursor {
                    return Err(NotificationProjectorError::FactConflict);
                }
                if NotificationVisibility::from_fact(previous) != NotificationVisibility::Active {
                    return Err(NotificationProjectorError::TerminalRewrite);
                }
            }
            next_lifecycle.insert(key, fact.clone());
        }
        self.materializer = next_materializer;
        self.lifecycle = next_lifecycle;
        self.source_cursor = source_cursor;
        self.data_epoch = next_epoch;
        Ok(())
    }

    pub fn visible_human_inbox_items(
        &self,
        recipient_id: &str,
    ) -> Result<Vec<HumanInboxItem>, NotificationProjectorError> {
        let items = self
            .materializer
            .human_inbox_items(recipient_id)
            .map_err(NotificationProjectorError::Materializer)?;
        Ok(items
            .into_iter()
            .filter(|item| {
                let Some(notification_id) = item.item_id.strip_prefix("notification:") else {
                    return false;
                };
                self.lifecycle
                    .get(notification_id)
                    .map(|fact| {
                        NotificationVisibility::from_fact(fact) == NotificationVisibility::Active
                    })
                    .unwrap_or(true)
            })
            .collect())
    }

    pub fn visibility(&self, notification_id: NotificationId) -> NotificationVisibility {
        self.lifecycle
            .get(&notification_id.to_string())
            .map(NotificationVisibility::from_fact)
            .unwrap_or(NotificationVisibility::Active)
    }

    /// Fold append-only read-state facts. A fact may not run ahead of the committed cursor, may
    /// only name an item the committed materializer already produced, and may never un-read,
    /// un-acknowledge or move a snooze backwards.
    pub fn apply_read_state_facts(
        &mut self,
        facts: &[NotificationReadStateFact],
    ) -> Result<(), NotificationProjectorError> {
        let mut next_read_state = self.read_state.clone();
        for fact in facts {
            fact.validate()
                .map_err(NotificationProjectorError::Invalid)?;
            if fact.source_cursor > self.source_cursor {
                return Err(NotificationProjectorError::CursorConflict);
            }
            if !self.knows_item(&fact.item_id) {
                return Err(NotificationProjectorError::UnknownNotification);
            }
            if let Some(previous) = next_read_state.get(&fact.item_id) {
                if previous.fact_digest == fact.fact_digest {
                    continue;
                }
                if fact.projection_revision <= previous.projection_revision {
                    return Err(NotificationProjectorError::ReadStateConflict);
                }
                if (previous.read && !fact.read)
                    || (previous.acknowledged && !fact.acknowledged)
                    || match (previous.snoozed_until_unix_ms, fact.snoozed_until_unix_ms) {
                        (Some(previous_snooze), Some(next_snooze)) => next_snooze < previous_snooze,
                        (Some(_), None) => true,
                        _ => false,
                    }
                {
                    return Err(NotificationProjectorError::ReadStateRegression);
                }
            }
            next_read_state.insert(fact.item_id.clone(), fact.clone());
        }
        self.read_state = next_read_state;
        Ok(())
    }

    /// Admit one retention decision for a notification the committed projection knows.
    ///
    /// Refusal order is fixed and documented so the same inputs always produce the same code:
    /// malformed decision, foreign principal, stale policy revision, active legal hold, then the
    /// epoch/cursor fences, then unknown notification. A successful admission appends exactly one
    /// lifecycle fact and records the decision's data epoch for every payload-bearing derived
    /// layer; it never removes a source event, a materialization or a read-state fact.
    pub fn admit_retention(
        &mut self,
        decision: &NotificationRetentionDecision,
        server_principal_id: &str,
        current_policy_revision: u64,
    ) -> Result<NotificationRetentionAdmission, NotificationProjectorError> {
        decision
            .validate()
            .map_err(NotificationProjectorError::Invalid)?;
        if server_principal_id.trim().is_empty() {
            return Err(NotificationProjectorError::Invalid(
                "server_principal_id".to_owned(),
            ));
        }
        if decision.authorized_by != server_principal_id {
            return Err(NotificationProjectorError::RetentionUnauthorized);
        }
        if decision.retention_policy_revision != current_policy_revision {
            return Err(NotificationProjectorError::RetentionPolicyStale);
        }
        if !decision.legal_hold_ids.is_empty() {
            return Err(NotificationProjectorError::RetentionLegalHold);
        }
        let fact = NotificationLifecycleFact::new(
            decision.notification_id,
            decision.source_event_id,
            decision.source_cursor,
            decision.data_epoch,
            decision.kind,
            decision.replacement_notification_id,
            decision.reason.clone(),
        )
        .map_err(NotificationProjectorError::Invalid)?;
        // An exact replay of an already-admitted decision is idempotent and must not be re-fenced
        // by the epoch advance it performed the first time. It still restores the per-layer data
        // epochs so a rebuild that passes both facts and decisions converges to the same state.
        if let Some(existing) = self.lifecycle.get(&decision.notification_id.to_string()) {
            if existing.fact_digest == fact.fact_digest {
                for layer in NOTIFICATION_RETENTION_PAYLOAD_LAYERS {
                    let epoch = self
                        .layer_epochs
                        .get(&layer)
                        .copied()
                        .unwrap_or(0)
                        .max(decision.data_epoch);
                    self.layer_epochs.insert(layer, epoch);
                }
                return NotificationRetentionAdmission::new(
                    decision,
                    NotificationVisibility::from_fact(existing),
                    existing.fact_digest.clone(),
                    self.retained_source_event_count(),
                )
                .map_err(NotificationProjectorError::Invalid);
            }
        }
        if decision.previous_epoch != self.data_epoch
            || decision.data_epoch <= decision.previous_epoch
        {
            return Err(NotificationProjectorError::RetentionEpochConflict);
        }
        if decision.source_cursor > self.source_cursor {
            return Err(NotificationProjectorError::CursorConflict);
        }
        if !self.knows_notification(decision.notification_id) {
            return Err(NotificationProjectorError::UnknownNotification);
        }
        let source_cursor = self.source_cursor;
        self.apply_committed(&[], source_cursor, std::slice::from_ref(&fact))?;
        for layer in NOTIFICATION_RETENTION_PAYLOAD_LAYERS {
            self.layer_epochs.insert(layer, decision.data_epoch);
        }
        self.retention_decisions
            .push(decision.decision_digest.clone());
        NotificationRetentionAdmission::new(
            decision,
            NotificationVisibility::from_fact(&fact),
            fact.fact_digest.clone(),
            self.retained_source_event_count(),
        )
        .map_err(NotificationProjectorError::Invalid)
    }

    /// One recipient's durable inbox view, bound to the committed cursor and data epoch.
    pub fn inbox_snapshot(
        &self,
        recipient_id: &str,
    ) -> Result<NotificationInboxSnapshot, NotificationProjectorError> {
        if recipient_id.trim().is_empty() {
            return Err(NotificationProjectorError::Invalid(
                "recipient_id".to_owned(),
            ));
        }
        if self.source_cursor == 0 {
            return Err(NotificationProjectorError::ProjectionUnavailable);
        }
        let items = self
            .materializer
            .human_inbox_items(recipient_id)
            .map_err(NotificationProjectorError::Materializer)?;
        let entries = items
            .into_iter()
            .map(|item| {
                let visibility = item
                    .item_id
                    .strip_prefix("notification:")
                    .and_then(|notification_id| self.lifecycle.get(notification_id))
                    .map(NotificationVisibility::from_fact)
                    .unwrap_or(NotificationVisibility::Active);
                let read_state = self.read_state.get(&item.item_id);
                NotificationInboxEntry {
                    item,
                    visibility,
                    read: read_state.is_some_and(|state| state.read),
                    acknowledged: read_state.is_some_and(|state| state.acknowledged),
                    snoozed_until_unix_ms: read_state.and_then(|state| state.snoozed_until_unix_ms),
                }
            })
            .collect::<Vec<_>>();
        NotificationInboxSnapshot::new(recipient_id, self.source_cursor, self.data_epoch, entries)
            .map_err(NotificationProjectorError::Invalid)
    }

    /// Number of committed source events retained by the projection. A withdraw/supersede/expire
    /// fact or a read-state fact never shrinks it.
    pub fn retained_source_event_count(&self) -> usize {
        self.materializer.source_event_ids().count()
    }

    /// Number of committed materializations retained by the projection.
    pub fn materialization_count(&self) -> usize {
        self.materializer.materializations().count()
    }

    pub fn lifecycle_facts(&self) -> impl Iterator<Item = &NotificationLifecycleFact> {
        self.lifecycle.values()
    }

    pub fn read_state(&self, item_id: &str) -> Option<&NotificationReadStateFact> {
        self.read_state.get(item_id)
    }

    pub fn read_state_count(&self) -> usize {
        self.read_state.len()
    }

    /// Data epoch every payload-bearing derived layer acknowledged in the admitted decisions.
    pub fn layer_epochs(&self) -> &BTreeMap<RevocationLayer, u64> {
        &self.layer_epochs
    }

    pub fn retention_decision_count(&self) -> usize {
        self.retention_decisions.len()
    }

    pub fn source_cursor(&self) -> u64 {
        self.source_cursor
    }

    pub fn data_epoch(&self) -> u64 {
        self.data_epoch
    }

    fn knows_notification(&self, notification_id: NotificationId) -> bool {
        self.materializer
            .materializations()
            .any(|materialization| materialization.notification.notification_id == notification_id)
    }

    fn knows_item(&self, item_id: &str) -> bool {
        self.materializer.materializations().any(|materialization| {
            format!(
                "notification:{}",
                materialization.notification.notification_id
            ) == item_id
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotificationProjection {
    checkpoint: Option<ProjectionCheckpoint>,
    notifications: Vec<Notification>,
}

impl Default for NotificationProjection {
    fn default() -> Self {
        Self::new()
    }
}

impl NotificationProjection {
    pub fn new() -> Self {
        Self {
            checkpoint: None,
            notifications: Vec::new(),
        }
    }

    pub fn apply_committed(
        &mut self,
        events: &[RuntimeEvent],
        source_cursor: u64,
    ) -> Result<(), String> {
        let initial = json!({"notifications": self.notifications});
        if let Some(checkpoint) = self.checkpoint.as_ref() {
            if source_cursor == checkpoint.source_cursor {
                if events
                    .iter()
                    .all(|event| checkpoint.source_event_ids.contains(&event.event_id))
                {
                    return Ok(());
                }
                return Err("notification_projection_cursor_gap".to_owned());
            }
        }
        let replay = match self.checkpoint.as_ref() {
            Some(checkpoint) => ReplayProjection::from_checkpoint(
                "notifications",
                checkpoint,
                events,
                source_cursor,
                fold_notification,
            ),
            None => ReplayProjection::from_zero(
                "notifications",
                initial,
                events,
                source_cursor,
                fold_notification,
            ),
        }?;
        let notifications: Vec<Notification> = serde_json::from_value(
            replay
                .state()
                .get("notifications")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
        )
        .map_err(|_| "notification_projection_state_invalid".to_owned())?;
        self.notifications = notifications;
        self.checkpoint = Some(replay.checkpoint()?);
        Ok(())
    }

    pub fn checkpoint(&self) -> Option<&ProjectionCheckpoint> {
        self.checkpoint.as_ref()
    }

    pub fn source_cursor(&self) -> u64 {
        self.checkpoint
            .as_ref()
            .map(|checkpoint| checkpoint.source_cursor)
            .unwrap_or(0)
    }

    pub fn notifications(&self) -> &[Notification] {
        &self.notifications
    }
}

fn fold_notification(mut state: Value, event: &RuntimeEvent) -> Result<Value, String> {
    let Some(_class) =
        validate_notification_runtime_event(event, NotificationEventSource::EventLog)?
    else {
        return Ok(state);
    };
    let value = event
        .data
        .get("notification")
        .cloned()
        .ok_or_else(|| "notification_projection_payload_missing".to_owned())?;
    let notification: Notification = serde_json::from_value(value)
        .map_err(|_| "notification_projection_payload_invalid".to_owned())?;
    notification.validate()?;
    let list = state
        .get_mut("notifications")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "notification_projection_state_invalid".to_owned())?;
    if !list
        .iter()
        .any(|item| item == &serde_json::to_value(&notification).unwrap())
    {
        if list.len() >= 4_096 {
            return Err("notification_projection_limit".to_owned());
        }
        list.push(
            serde_json::to_value(notification)
                .map_err(|_| "notification_projection_encode_failed".to_owned())?,
        );
    }
    Ok(state)
}
