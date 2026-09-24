//! BQ-11 model-attempt lifecycle facts.
//!
//! The lifecycle is a small, append-only contract around the already admitted model call.  It
//! links the server-owned run/turn/model identity to the billing `AttemptId`, quota reservation,
//! normalized usage and a read-only receipt reference.  This module is deliberately pure: it
//! does not append to an EventStore, call a provider, consume a permit or release a reservation.
//! Adapters must commit the returned facts through the ControlPlane before dispatching any
//! provider effect.

use crate::{
    json_digest, AttemptId, EventId, ModelAttemptIdentity, NormalizedUsage, QuotaReservation,
    QuotaReservationId, QuotaReservationState, ReceiptId, RequestId, RunId, SchemaVersion, TurnId,
};
use serde::{Deserialize, Serialize};

pub const MODEL_ATTEMPT_LIFECYCLE_SCHEMA: &str = "kiana.model-attempt-lifecycle.v1";
pub const MODEL_ATTEMPT_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MODEL_ATTEMPT_EVENT_SCHEMA: &str = "kiana.model-attempt-event.v1";
pub const MODEL_ATTEMPT_EVENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
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

/// The only lifecycle states that may be written as model-attempt facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAttemptState {
    Prepared,
    Dispatching,
    Observed,
    Settled,
    Unknown,
}

impl ModelAttemptState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Unknown)
    }

    pub const fn event_kind(self) -> ModelAttemptEventKind {
        match self {
            Self::Prepared => ModelAttemptEventKind::Prepared,
            Self::Dispatching => ModelAttemptEventKind::Dispatching,
            Self::Observed => ModelAttemptEventKind::Observed,
            Self::Settled => ModelAttemptEventKind::Settled,
            Self::Unknown => ModelAttemptEventKind::Unknown,
        }
    }

    pub fn transition(self, next: Self) -> Result<Self, String> {
        let allowed = matches!(
            (self, next),
            (Self::Prepared, Self::Dispatching | Self::Unknown)
                | (Self::Dispatching, Self::Observed | Self::Unknown)
                | (Self::Observed, Self::Settled | Self::Unknown)
        );
        if self == next {
            return Err("model_attempt_state_duplicate".to_owned());
        }
        allowed
            .then_some(next)
            .ok_or_else(|| "model_attempt_state_transition_invalid".to_owned())
    }
}

/// Runtime event names owned by the BQ-11 lifecycle projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelAttemptEventKind {
    Prepared,
    Dispatching,
    Observed,
    Settled,
    Unknown,
}

impl ModelAttemptEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "model.prepared",
            Self::Dispatching => "model.dispatching",
            Self::Observed => "model.observed",
            Self::Settled => "model.settled",
            Self::Unknown => "model.unknown",
        }
    }

    pub const fn state(self) -> ModelAttemptState {
        match self {
            Self::Prepared => ModelAttemptState::Prepared,
            Self::Dispatching => ModelAttemptState::Dispatching,
            Self::Observed => ModelAttemptState::Observed,
            Self::Settled => ModelAttemptState::Settled,
            Self::Unknown => ModelAttemptState::Unknown,
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Unknown)
    }
}

/// A bounded model/provider failure retained on failed or unknown attempts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptError {
    pub code: String,
    pub phase: String,
    pub request_sent: bool,
}

impl ModelAttemptError {
    pub fn new(
        code: impl Into<String>,
        phase: impl Into<String>,
        request_sent: bool,
    ) -> Result<Self, String> {
        let error = Self {
            code: code.into(),
            phase: phase.into(),
            request_sent,
        };
        error.validate()?;
        Ok(error)
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.code, "model_attempt_error_code", 128)?;
        bounded(&self.phase, "model_attempt_error_phase", 64)
    }
}

/// One immutable lifecycle fact.  A serialized fact carries all server-owned links needed for a
/// projector to reject a cross-run/cross-attempt fold without consulting the transcript.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptLifecycleEvent {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: ModelAttemptEventKind,
    pub event_id: EventId,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub model_attempt_id: crate::ModelAttemptId,
    pub attempt_id: AttemptId,
    pub reservation_id: QuotaReservationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permit_id: Option<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<NormalizedUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<ReceiptId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ModelAttemptError>,
    /// A dispatch fact may only exist after the prepared append received a flush acknowledgement.
    pub prepared_flushed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flush_sequence: Option<u64>,
    pub revision: u64,
    pub event_digest: String,
}

impl ModelAttemptLifecycleEvent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_ATTEMPT_EVENT_SCHEMA
            || !self
                .version
                .is_compatible_with(&MODEL_ATTEMPT_EVENT_VERSION)
            || self.event_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.reservation_id.as_uuid().is_nil()
            || self.revision == 0
            || self.kind.state() == ModelAttemptState::Prepared && self.revision != 1
            || self.prepared_flushed && self.flush_sequence.is_none()
            || self.flush_sequence.is_some_and(|sequence| sequence == 0)
        {
            return Err("model_attempt_event_header_invalid".to_owned());
        }
        if self.kind == ModelAttemptEventKind::Dispatching
            && (!self.prepared_flushed || self.flush_sequence.is_none())
        {
            return Err("model_attempt_dispatch_requires_prepared_flush".to_owned());
        }
        if matches!(
            self.kind,
            ModelAttemptEventKind::Dispatching
                | ModelAttemptEventKind::Observed
                | ModelAttemptEventKind::Settled
        ) && self.permit_id.is_none()
        {
            return Err("model_attempt_permit_required".to_owned());
        }
        if self
            .permit_id
            .is_some_and(|permit_id| permit_id.as_uuid().is_nil())
        {
            return Err("model_attempt_permit_invalid".to_owned());
        }
        if self
            .receipt_id
            .is_some_and(|receipt_id| receipt_id.as_uuid().is_nil())
        {
            return Err("model_attempt_receipt_invalid".to_owned());
        }
        if let Some(usage) = &self.usage {
            usage.validate()?;
            if usage.attempt_id != self.attempt_id || usage.run_id != self.run_id {
                return Err("model_attempt_usage_identity_mismatch".to_owned());
            }
        }
        match self.kind {
            ModelAttemptEventKind::Prepared | ModelAttemptEventKind::Dispatching => {
                if self.usage.is_some() || self.receipt_id.is_some() || self.error.is_some() {
                    return Err("model_attempt_pre_terminal_payload_invalid".to_owned());
                }
            }
            ModelAttemptEventKind::Observed => {
                if self.usage.is_none() || self.receipt_id.is_some() || self.error.is_some() {
                    return Err("model_attempt_observation_payload_invalid".to_owned());
                }
            }
            ModelAttemptEventKind::Settled => {
                let Some(usage) = &self.usage else {
                    return Err("model_attempt_settlement_usage_required".to_owned());
                };
                if usage.confidence == crate::UsageConfidence::Unknown
                    || self.receipt_id.is_none()
                    || self.error.is_some()
                {
                    return Err("model_attempt_settlement_payload_invalid".to_owned());
                }
            }
            ModelAttemptEventKind::Unknown => {
                let Some(error) = &self.error else {
                    return Err("model_attempt_unknown_error_required".to_owned());
                };
                error.validate()?;
                if self.receipt_id.is_some() {
                    return Err("model_attempt_unknown_receipt_invalid".to_owned());
                }
            }
        }
        valid_digest(&self.event_digest, "model_attempt_event_digest")?;
        if self.event_digest != self.digest() {
            return Err("model_attempt_event_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "event_id": self.event_id,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "model_attempt_id": self.model_attempt_id,
            "attempt_id": self.attempt_id,
            "reservation_id": self.reservation_id,
            "permit_id": self.permit_id,
            "usage": self.usage,
            "receipt_id": self.receipt_id,
            "error": self.error,
            "prepared_flushed": self.prepared_flushed,
            "flush_sequence": self.flush_sequence,
            "revision": self.revision,
        }))
    }
}

/// Read-only latest state for a single model attempt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAttemptLifecycleRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub model_attempt_id: crate::ModelAttemptId,
    pub attempt_id: AttemptId,
    pub reservation_id: QuotaReservationId,
    pub state: ModelAttemptState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permit_id: Option<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<NormalizedUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<ReceiptId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ModelAttemptError>,
    pub prepared_flushed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flush_sequence: Option<u64>,
    pub revision: u64,
    pub source_event_ids: Vec<EventId>,
    pub record_digest: String,
}

impl ModelAttemptLifecycleRecord {
    pub fn from_event(event: &ModelAttemptLifecycleEvent) -> Result<Self, String> {
        event.validate()?;
        let mut record = Self {
            schema: MODEL_ATTEMPT_LIFECYCLE_SCHEMA.to_owned(),
            version: MODEL_ATTEMPT_LIFECYCLE_VERSION,
            run_id: event.run_id,
            turn_id: event.turn_id,
            model_attempt_id: event.model_attempt_id,
            attempt_id: event.attempt_id,
            reservation_id: event.reservation_id,
            state: event.kind.state(),
            permit_id: event.permit_id,
            usage: event.usage.clone(),
            receipt_id: event.receipt_id,
            error: event.error.clone(),
            prepared_flushed: event.prepared_flushed,
            flush_sequence: event.flush_sequence,
            revision: event.revision,
            source_event_ids: vec![event.event_id],
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn with_source_event(mut self, event_id: EventId) -> Result<Self, String> {
        if event_id.as_uuid().is_nil() {
            return Err("model_attempt_source_event_invalid".to_owned());
        }
        if !self.source_event_ids.contains(&event_id) {
            self.source_event_ids.push(event_id);
        }
        self.record_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MODEL_ATTEMPT_LIFECYCLE_SCHEMA
            || !self
                .version
                .is_compatible_with(&MODEL_ATTEMPT_LIFECYCLE_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.reservation_id.as_uuid().is_nil()
            || self.revision == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > 256
            || self
                .source_event_ids
                .iter()
                .any(|event_id| event_id.as_uuid().is_nil())
            || self.prepared_flushed && self.flush_sequence.is_none()
            || self.flush_sequence.is_some_and(|sequence| sequence == 0)
        {
            return Err("model_attempt_record_header_invalid".to_owned());
        }
        if let Some(permit_id) = self.permit_id {
            if permit_id.as_uuid().is_nil() {
                return Err("model_attempt_permit_invalid".to_owned());
            }
        }
        if self
            .receipt_id
            .is_some_and(|receipt_id| receipt_id.as_uuid().is_nil())
        {
            return Err("model_attempt_receipt_invalid".to_owned());
        }
        if let Some(usage) = &self.usage {
            usage.validate()?;
            if usage.attempt_id != self.attempt_id || usage.run_id != self.run_id {
                return Err("model_attempt_usage_identity_mismatch".to_owned());
            }
        }
        if matches!(
            self.state,
            ModelAttemptState::Dispatching
                | ModelAttemptState::Observed
                | ModelAttemptState::Settled
        ) && self.permit_id.is_none()
        {
            return Err("model_attempt_permit_required".to_owned());
        }
        if self.state == ModelAttemptState::Dispatching
            && (!self.prepared_flushed || self.flush_sequence.is_none())
        {
            return Err("model_attempt_dispatch_requires_prepared_flush".to_owned());
        }
        match self.state {
            ModelAttemptState::Prepared | ModelAttemptState::Dispatching => {
                if self.usage.is_some() || self.receipt_id.is_some() || self.error.is_some() {
                    return Err("model_attempt_pre_terminal_payload_invalid".to_owned());
                }
            }
            ModelAttemptState::Observed => {
                if self.usage.is_none() || self.receipt_id.is_some() || self.error.is_some() {
                    return Err("model_attempt_observation_payload_invalid".to_owned());
                }
            }
            ModelAttemptState::Settled => {
                let Some(usage) = &self.usage else {
                    return Err("model_attempt_settlement_usage_required".to_owned());
                };
                if usage.confidence == crate::UsageConfidence::Unknown
                    || self.receipt_id.is_none()
                    || self.error.is_some()
                {
                    return Err("model_attempt_settlement_payload_invalid".to_owned());
                }
            }
            ModelAttemptState::Unknown => {
                let Some(error) = &self.error else {
                    return Err("model_attempt_unknown_error_required".to_owned());
                };
                error.validate()?;
                if self.receipt_id.is_some() {
                    return Err("model_attempt_unknown_receipt_invalid".to_owned());
                }
            }
        }
        valid_digest(&self.record_digest, "model_attempt_record_digest")?;
        if self.record_digest != self.digest() {
            return Err("model_attempt_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "turn_id": self.turn_id,
            "model_attempt_id": self.model_attempt_id,
            "attempt_id": self.attempt_id,
            "reservation_id": self.reservation_id,
            "state": self.state,
            "permit_id": self.permit_id,
            "usage": self.usage,
            "receipt_id": self.receipt_id,
            "error": self.error,
            "prepared_flushed": self.prepared_flushed,
            "flush_sequence": self.flush_sequence,
            "revision": self.revision,
            "source_event_ids": self.source_event_ids,
        }))
    }
}

/// In-memory reducer used by adapters and CI fixtures.  It returns immutable facts; it does not
/// imply that a fact reached the EventStore until the caller receives a commit/flush receipt.
#[derive(Clone, Debug)]
pub struct ModelAttemptLifecycleLedger {
    identity: ModelAttemptIdentity,
    attempt_id: AttemptId,
    reservation_id: QuotaReservationId,
    state: Option<ModelAttemptState>,
    permit_id: Option<RequestId>,
    usage: Option<NormalizedUsage>,
    receipt_id: Option<ReceiptId>,
    error: Option<ModelAttemptError>,
    prepared_appended: bool,
    prepared_flushed: bool,
    flush_sequence: Option<u64>,
    revision: u64,
    events: Vec<ModelAttemptLifecycleEvent>,
}

impl ModelAttemptLifecycleLedger {
    pub fn new(
        identity: ModelAttemptIdentity,
        attempt_id: AttemptId,
        reservation: &QuotaReservation,
    ) -> Result<Self, String> {
        identity.validate()?;
        reservation.validate()?;
        if reservation.state != QuotaReservationState::Reserved {
            return Err("model_attempt_reservation_not_reserved".to_owned());
        }
        if reservation.owner_run != identity.run_id || reservation.owner_attempt != attempt_id {
            return Err("model_attempt_reservation_identity_mismatch".to_owned());
        }
        if attempt_id.as_uuid().is_nil() {
            return Err("model_attempt_attempt_id_invalid".to_owned());
        }
        Ok(Self {
            identity,
            attempt_id,
            reservation_id: reservation.reservation_id,
            state: None,
            permit_id: None,
            usage: None,
            receipt_id: None,
            error: None,
            prepared_appended: false,
            prepared_flushed: false,
            flush_sequence: None,
            revision: 0,
            events: Vec::new(),
        })
    }

    pub fn state(&self) -> Option<ModelAttemptState> {
        self.state
    }

    pub fn events(&self) -> &[ModelAttemptLifecycleEvent] {
        &self.events
    }

    pub fn prepared_flushed(&self) -> bool {
        self.prepared_flushed
    }

    pub fn prepare<P>(&mut self, permit_id: P) -> Result<ModelAttemptLifecycleEvent, String>
    where
        P: Into<Option<RequestId>>,
    {
        if self.state.is_some() || self.prepared_appended {
            return Err("model_attempt_already_prepared".to_owned());
        }
        let permit_id = permit_id.into();
        if permit_id.is_some_and(|id| id.as_uuid().is_nil()) {
            return Err("model_attempt_permit_invalid".to_owned());
        }
        self.state = Some(ModelAttemptState::Prepared);
        self.permit_id = permit_id;
        self.prepared_appended = true;
        self.emit(ModelAttemptEventKind::Prepared)
    }

    /// Record the EventStore flush acknowledgement for `model.prepared`.  Dispatch must remain
    /// fenced until this method succeeds.
    pub fn mark_prepared_flushed(&mut self, flush_sequence: u64) -> Result<(), String> {
        if self.state != Some(ModelAttemptState::Prepared) || !self.prepared_appended {
            return Err("model_attempt_prepared_append_required".to_owned());
        }
        if flush_sequence == 0 {
            return Err("model_attempt_flush_sequence_invalid".to_owned());
        }
        self.prepared_flushed = true;
        self.flush_sequence = Some(flush_sequence);
        Ok(())
    }

    pub fn dispatch<P>(&mut self, permit_id: P) -> Result<ModelAttemptLifecycleEvent, String>
    where
        P: Into<Option<RequestId>>,
    {
        if self.state.is_none() {
            return Err("model_attempt_not_prepared".to_owned());
        }
        if self.state != Some(ModelAttemptState::Prepared) {
            return Err("model_attempt_dispatch_state_invalid".to_owned());
        }
        if !self.prepared_flushed {
            return Err("model_attempt_prepared_not_flushed".to_owned());
        }
        let Some(permit_id) = permit_id.into() else {
            return Err("model_attempt_permit_required".to_owned());
        };
        if self.permit_id != Some(permit_id) {
            return Err("model_attempt_permit_mismatch".to_owned());
        }
        self.state = Some(ModelAttemptState::Dispatching);
        self.emit(ModelAttemptEventKind::Dispatching)
    }

    pub fn observe(
        &mut self,
        usage: NormalizedUsage,
    ) -> Result<ModelAttemptLifecycleEvent, String> {
        if self.state != Some(ModelAttemptState::Dispatching) {
            return Err("model_attempt_observe_state_invalid".to_owned());
        }
        self.validate_usage(&usage)?;
        if usage.confidence == crate::UsageConfidence::Unknown {
            return Err("model_attempt_unknown_usage_requires_unknown_state".to_owned());
        }
        self.usage = Some(usage);
        self.state = Some(ModelAttemptState::Observed);
        self.emit(ModelAttemptEventKind::Observed)
    }

    pub fn settle(
        &mut self,
        receipt_id: ReceiptId,
        usage: Option<NormalizedUsage>,
    ) -> Result<ModelAttemptLifecycleEvent, String> {
        if self.state != Some(ModelAttemptState::Observed) {
            if self.state == Some(ModelAttemptState::Settled) {
                return Err("model_attempt_duplicate_settlement".to_owned());
            }
            return Err("model_attempt_settle_state_invalid".to_owned());
        }
        if receipt_id.as_uuid().is_nil() {
            return Err("model_attempt_receipt_required".to_owned());
        }
        if let Some(usage) = usage {
            self.validate_usage(&usage)?;
            if self
                .usage
                .as_ref()
                .is_some_and(|observed| observed.usage_digest != usage.usage_digest)
            {
                return Err("model_attempt_settlement_usage_mismatch".to_owned());
            }
            self.usage = Some(usage);
        }
        if self.usage.is_none() {
            return Err("model_attempt_settlement_usage_required".to_owned());
        }
        if self
            .usage
            .as_ref()
            .is_some_and(|usage| usage.confidence == crate::UsageConfidence::Unknown)
        {
            return Err("model_attempt_unknown_cannot_settle".to_owned());
        }
        self.receipt_id = Some(receipt_id);
        self.state = Some(ModelAttemptState::Settled);
        self.emit(ModelAttemptEventKind::Settled)
    }

    pub fn unknown(
        &mut self,
        usage: Option<NormalizedUsage>,
        error: ModelAttemptError,
    ) -> Result<ModelAttemptLifecycleEvent, String> {
        if !matches!(
            self.state,
            Some(ModelAttemptState::Dispatching | ModelAttemptState::Observed)
        ) {
            return Err("model_attempt_unknown_state_invalid".to_owned());
        }
        error.validate()?;
        if let Some(usage) = usage {
            self.validate_usage(&usage)?;
            self.usage = Some(usage);
        }
        self.error = Some(error);
        self.state = Some(ModelAttemptState::Unknown);
        self.emit(ModelAttemptEventKind::Unknown)
    }

    pub fn record(&self) -> Result<ModelAttemptLifecycleRecord, String> {
        let Some(state) = self.state else {
            return Err("model_attempt_not_prepared".to_owned());
        };
        let mut record = ModelAttemptLifecycleRecord {
            schema: MODEL_ATTEMPT_LIFECYCLE_SCHEMA.to_owned(),
            version: MODEL_ATTEMPT_LIFECYCLE_VERSION,
            run_id: self.identity.run_id,
            turn_id: self.identity.turn_id,
            model_attempt_id: self.identity.model_attempt_id,
            attempt_id: self.attempt_id,
            reservation_id: self.reservation_id,
            state,
            permit_id: self.permit_id,
            usage: self.usage.clone(),
            receipt_id: self.receipt_id,
            error: self.error.clone(),
            prepared_flushed: self.prepared_flushed,
            flush_sequence: self.flush_sequence,
            revision: self.revision,
            source_event_ids: self.events.iter().map(|event| event.event_id).collect(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    fn validate_usage(&self, usage: &NormalizedUsage) -> Result<(), String> {
        usage.validate()?;
        if usage.attempt_id != self.attempt_id || usage.run_id != self.identity.run_id {
            return Err("model_attempt_usage_identity_mismatch".to_owned());
        }
        Ok(())
    }

    fn emit(&mut self, kind: ModelAttemptEventKind) -> Result<ModelAttemptLifecycleEvent, String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "model_attempt_revision_overflow".to_owned())?;
        let mut event = ModelAttemptLifecycleEvent {
            schema: MODEL_ATTEMPT_EVENT_SCHEMA.to_owned(),
            version: MODEL_ATTEMPT_EVENT_VERSION,
            kind,
            event_id: EventId::new(),
            run_id: self.identity.run_id,
            turn_id: self.identity.turn_id,
            model_attempt_id: self.identity.model_attempt_id,
            attempt_id: self.attempt_id,
            reservation_id: self.reservation_id,
            permit_id: self.permit_id,
            usage: self.usage.clone(),
            receipt_id: self.receipt_id,
            error: self.error.clone(),
            prepared_flushed: self.prepared_flushed,
            flush_sequence: self.flush_sequence,
            revision: self.revision,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        self.events.push(event.clone());
        Ok(event)
    }
}

/// Apply a serialized lifecycle event to a latest record.  Core uses this helper while folding
/// committed EventLog pages; it is intentionally strict about append order and terminal replay.
pub fn apply_model_attempt_event(
    current: Option<ModelAttemptLifecycleRecord>,
    event: &ModelAttemptLifecycleEvent,
    source_event_id: Option<EventId>,
) -> Result<ModelAttemptLifecycleRecord, String> {
    event.validate()?;
    let had_current = current.is_some();
    let mut next = ModelAttemptLifecycleRecord::from_event(event)?;
    if !had_current && event.kind != ModelAttemptEventKind::Prepared {
        return Err("model_attempt_event_requires_prepared".to_owned());
    }
    if let Some(current) = current {
        if current.run_id != event.run_id
            || current.turn_id != event.turn_id
            || current.model_attempt_id != event.model_attempt_id
            || current.attempt_id != event.attempt_id
            || current.reservation_id != event.reservation_id
            || current.revision.checked_add(1) != Some(event.revision)
        {
            return Err("model_attempt_event_identity_or_revision_conflict".to_owned());
        }
        current
            .state
            .transition(event.kind.state())
            .map_err(|_| "model_attempt_event_transition_invalid".to_owned())?;
        if current.permit_id != event.permit_id {
            return Err("model_attempt_permit_drift".to_owned());
        }
        if current.prepared_flushed && !event.prepared_flushed {
            return Err("model_attempt_flush_regression".to_owned());
        }
        next.source_event_ids = current.source_event_ids;
        if !next.source_event_ids.contains(&event.event_id) {
            next.source_event_ids.push(event.event_id);
        }
    }
    if let Some(source_event_id) = source_event_id {
        if source_event_id.as_uuid().is_nil() {
            return Err("model_attempt_source_event_invalid".to_owned());
        }
        if had_current {
            if !next.source_event_ids.contains(&source_event_id) {
                next.source_event_ids.push(source_event_id);
            }
        } else {
            next.source_event_ids = vec![source_event_id];
        }
    }
    next.record_digest = next.digest();
    next.validate()?;
    Ok(next)
}
