//! BQ-12 settlement, release and unknown folding contracts.
//!
//! A quota reservation is held until a committed fact says exactly what happened to one model
//! attempt.  This module is deliberately a value-level reducer: it binds the reservation and
//! usage digests to the server-owned attempt identity, keeps known/partial/unknown accounting
//! separate, and refuses to infer a release from cancellation, timeout or EOF.
//!
//! The reducer does not write an EventLog, call a provider, consume a permit or reconcile an
//! external invoice.  Callers must append the returned `SettlementFoldEvent` through the existing
//! ControlPlane/EventLog boundary before treating the fold as durable.

use crate::{
    json_digest, AttemptId, BillingUnknownReason, EventId, ModelAttemptId,
    ModelAttemptLifecycleRecord, ModelAttemptState, NormalizedUsage, QuotaReservation, ReceiptId,
    RunId, SchemaVersion, UsageConfidence,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SETTLEMENT_FOLD_SCHEMA: &str = "kiana.settlement-fold.v1";
pub const SETTLEMENT_FOLD_EVENT_SCHEMA: &str = "kiana.settlement-fold-event.v1";
pub const SETTLEMENT_FOLD_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// The reservation ledger state. `Unknown` is terminal only from the perspective of automatic
/// settlement; it remains occupied until an explicit reconciliation operation is added later.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementFoldState {
    Reserved,
    Consumed,
    Released,
    Unknown,
}

impl SettlementFoldState {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Released | Self::Unknown)
    }

    pub const fn event_kind(self) -> SettlementFoldEventKind {
        match self {
            Self::Reserved => SettlementFoldEventKind::Reserved,
            Self::Consumed => SettlementFoldEventKind::Consumed,
            Self::Released => SettlementFoldEventKind::Released,
            Self::Unknown => SettlementFoldEventKind::Unknown,
        }
    }
}

/// Whether the provider usage used by a consumed fold is complete.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementUsageClass {
    Known,
    Partial,
    Unknown,
}

/// Server-owned event kinds emitted by this fold.  The event payload always carries the full
/// reservation identity and digest so a projector cannot join two attempts by position alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementFoldEventKind {
    Reserved,
    Observed,
    Consumed,
    Released,
    Unknown,
}

impl SettlementFoldEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Reserved => "usage.reserved",
            Self::Observed => "usage.observed",
            Self::Consumed => "usage.settled",
            Self::Released => "usage.released",
            Self::Unknown => "usage.unknown",
        }
    }

    pub const fn state(self) -> SettlementFoldState {
        match self {
            Self::Reserved | Self::Observed => SettlementFoldState::Reserved,
            Self::Consumed => SettlementFoldState::Consumed,
            Self::Released => SettlementFoldState::Released,
            Self::Unknown => SettlementFoldState::Unknown,
        }
    }
}

/// Units reserved by one quota admission.  Zero is meaningful only for a consumed/released
/// dimension; reservations themselves must request every dimension explicitly.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationUnits {
    pub requests: u64,
    pub tokens: u64,
    pub concurrency: u32,
}

impl ReservationUnits {
    fn from_reservation(reservation: &QuotaReservation) -> Self {
        Self {
            requests: reservation.requested_requests,
            tokens: reservation.requested_tokens,
            concurrency: reservation.requested_concurrency,
        }
    }

    fn validate(&self, field: &str, allow_zero: bool) -> Result<(), String> {
        if !allow_zero && (self.requests == 0 || self.tokens == 0 || self.concurrency == 0) {
            return Err(format!("{field}_invalid"));
        }
        Ok(())
    }

    fn checked_difference(reserved: Self, consumed: &ConsumedUnits) -> Result<Self, String> {
        let Some(tokens) = consumed.tokens else {
            return Err("settlement_release_requires_known_tokens".to_owned());
        };
        if !consumed.fits_within(reserved) {
            return Err("settlement_consumption_exceeds_reservation".to_owned());
        }
        Ok(Self {
            requests: reserved
                .requests
                .checked_sub(consumed.requests)
                .ok_or_else(|| "settlement_release_underflow".to_owned())?,
            tokens: reserved
                .tokens
                .checked_sub(tokens)
                .ok_or_else(|| "settlement_release_underflow".to_owned())?,
            concurrency: reserved
                .concurrency
                .checked_sub(consumed.concurrency)
                .ok_or_else(|| "settlement_release_underflow".to_owned())?,
        })
    }
}

/// Consumption is optional because an unknown provider result can prove that a request was sent
/// while reporting no trustworthy usage.  An explicit `Some(tokens = 0)` remains distinct from
/// “usage unknown”.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumedUnits {
    pub requests: u64,
    pub tokens: Option<u64>,
    pub concurrency: u32,
}

impl ConsumedUnits {
    fn validate(&self) -> Result<(), String> {
        if self.requests == 0 || self.concurrency == 0 {
            return Err("settlement_consumed_units_invalid".to_owned());
        }
        Ok(())
    }

    fn fits_within(&self, reserved: ReservationUnits) -> bool {
        self.requests <= reserved.requests
            && self.tokens.is_none_or(|tokens| tokens <= reserved.tokens)
            && self.concurrency <= reserved.concurrency
    }
}

/// One immutable source event reference.  Raw provider responses and invoice secrets are never
/// copied into the fold.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementSourceRef {
    pub event_id: EventId,
    pub source_digest: String,
}

impl SettlementSourceRef {
    pub fn new(event_id: EventId, source_digest: impl Into<String>) -> Result<Self, String> {
        let source = Self {
            event_id,
            source_digest: source_digest.into(),
        };
        source.validate()?;
        Ok(source)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.event_id.as_uuid().is_nil() {
            return Err("settlement_source_event_invalid".to_owned());
        }
        valid_digest(&self.source_digest, "settlement_source_digest")
    }
}

/// A BQ-12 fold record.  It is intentionally digest/ref based: the usage authority remains the
/// committed lifecycle/provider evidence and a receipt is only an opaque reference here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementFoldRecord {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub model_attempt_id: ModelAttemptId,
    pub attempt_id: AttemptId,
    pub reservation_id: crate::QuotaReservationId,
    pub reservation_digest: String,
    pub state: SettlementFoldState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_class: Option<SettlementUsageClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<ReceiptId>,
    pub reserved: ReservationUnits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed: Option<ConsumedUnits>,
    pub released: ReservationUnits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<BillingUnknownReason>,
    pub reconciliation_required: bool,
    pub source_refs: Vec<SettlementSourceRef>,
    pub revision: u64,
    pub last_event_digest: String,
    pub record_digest: String,
}

impl SettlementFoldRecord {
    /// Build the first fold event from a committed BQ-11 lifecycle record.  Cancellation,
    /// timeout and EOF are represented by the lifecycle's `Unknown` state and never become a
    /// release or a zero consumption.
    pub fn from_lifecycle(
        reservation: &QuotaReservation,
        lifecycle: &ModelAttemptLifecycleRecord,
        source: SettlementSourceRef,
    ) -> Result<SettlementFoldEvent, String> {
        reservation.validate()?;
        lifecycle.validate()?;
        source.validate()?;
        if lifecycle.run_id != reservation.owner_run
            || lifecycle.attempt_id != reservation.owner_attempt
            || lifecycle.reservation_id != reservation.reservation_id
        {
            return Err("settlement_identity_mismatch".to_owned());
        }
        let (state, usage_class, usage_digest, receipt_id, consumed, unknown_reason) =
            match lifecycle.state {
                ModelAttemptState::Prepared | ModelAttemptState::Dispatching => {
                    (SettlementFoldState::Reserved, None, None, None, None, None)
                }
                ModelAttemptState::Observed => {
                    let usage = lifecycle
                        .usage
                        .as_ref()
                        .ok_or_else(|| "settlement_observed_usage_required".to_owned())?;
                    let class = usage_class(usage)?;
                    let consumed = consumed_from_usage(usage)?;
                    (
                        SettlementFoldState::Reserved,
                        Some(class),
                        Some(usage.usage_digest.clone()),
                        None,
                        Some(consumed),
                        None,
                    )
                }
                ModelAttemptState::Settled => {
                    let usage = lifecycle
                        .usage
                        .as_ref()
                        .ok_or_else(|| "settlement_usage_required".to_owned())?;
                    let class = usage_class(usage)?;
                    if class == SettlementUsageClass::Unknown {
                        return Err("settlement_unknown_usage_cannot_consume".to_owned());
                    }
                    let receipt_id = lifecycle
                        .receipt_id
                        .ok_or_else(|| "settlement_receipt_required".to_owned())?;
                    if receipt_id.as_uuid().is_nil() {
                        return Err("settlement_receipt_invalid".to_owned());
                    }
                    (
                        SettlementFoldState::Consumed,
                        Some(class),
                        Some(usage.usage_digest.clone()),
                        Some(receipt_id),
                        Some(consumed_from_usage(usage)?),
                        None,
                    )
                }
                ModelAttemptState::Unknown => {
                    let (usage_digest, consumed) = match lifecycle.usage.as_ref() {
                        Some(usage) if usage.confidence != UsageConfidence::Unknown => (
                            Some(usage.usage_digest.clone()),
                            Some(consumed_from_usage(usage)?),
                        ),
                        _ => (None, None),
                    };
                    (
                        SettlementFoldState::Unknown,
                        Some(SettlementUsageClass::Unknown),
                        usage_digest,
                        None,
                        consumed,
                        Some(BillingUnknownReason::ResultUnknown),
                    )
                }
            };
        let reserved = ReservationUnits::from_reservation(reservation);
        let mut event = SettlementFoldEvent {
            schema: SETTLEMENT_FOLD_EVENT_SCHEMA.to_owned(),
            version: SETTLEMENT_FOLD_VERSION,
            kind: if lifecycle.state == ModelAttemptState::Observed {
                SettlementFoldEventKind::Observed
            } else {
                state.event_kind()
            },
            event_id: EventId::new(),
            run_id: lifecycle.run_id,
            model_attempt_id: lifecycle.model_attempt_id,
            attempt_id: lifecycle.attempt_id,
            reservation_id: lifecycle.reservation_id,
            reservation_digest: reservation.reservation_digest.clone(),
            usage_class,
            usage_digest,
            receipt_id,
            reserved,
            consumed,
            released: ReservationUnits::default(),
            unknown_reason,
            reconciliation_required: state == SettlementFoldState::Unknown,
            source_refs: vec![source],
            revision: 1,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        Ok(event)
    }

    fn from_event(event: &SettlementFoldEvent) -> Result<Self, String> {
        event.validate()?;
        let mut record = Self {
            schema: SETTLEMENT_FOLD_SCHEMA.to_owned(),
            version: SETTLEMENT_FOLD_VERSION,
            run_id: event.run_id,
            model_attempt_id: event.model_attempt_id,
            attempt_id: event.attempt_id,
            reservation_id: event.reservation_id,
            reservation_digest: event.reservation_digest.clone(),
            state: event.kind.state(),
            usage_class: event.usage_class,
            usage_digest: event.usage_digest.clone(),
            receipt_id: event.receipt_id,
            reserved: event.reserved,
            consumed: event.consumed,
            released: event.released,
            unknown_reason: event.unknown_reason,
            reconciliation_required: event.reconciliation_required,
            source_refs: event.source_refs.clone(),
            revision: event.revision,
            last_event_digest: event.event_digest.clone(),
            record_digest: String::new(),
        };
        record.record_digest = record.digest();
        record.validate()?;
        Ok(record)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SETTLEMENT_FOLD_SCHEMA
            || !self.version.is_compatible_with(&SETTLEMENT_FOLD_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.reservation_id.as_uuid().is_nil()
            || self.revision == 0
            || self.source_refs.is_empty()
            || self.source_refs.len() > 256
            || self.reconciliation_required != (self.state == SettlementFoldState::Unknown)
        {
            return Err("settlement_fold_record_invalid".to_owned());
        }
        valid_digest(&self.reservation_digest, "settlement_reservation_digest")?;
        valid_digest(&self.last_event_digest, "settlement_event_digest")?;
        self.reserved.validate("settlement_reserved_units", false)?;
        self.released.validate("settlement_released_units", true)?;
        for source in &self.source_refs {
            source.validate()?;
        }
        if self
            .source_refs
            .windows(2)
            .any(|window| window[0].event_id == window[1].event_id)
        {
            return Err("settlement_source_event_duplicate".to_owned());
        }
        if self.released.requests > self.reserved.requests
            || self.released.tokens > self.reserved.tokens
            || self.released.concurrency > self.reserved.concurrency
        {
            return Err("settlement_release_exceeds_reservation".to_owned());
        }
        if let Some(consumed) = self.consumed {
            consumed.validate()?;
            if !consumed.fits_within(self.reserved) {
                return Err("settlement_consumption_exceeds_reservation".to_owned());
            }
        }
        match self.state {
            SettlementFoldState::Reserved => {
                if self.receipt_id.is_some()
                    || self.unknown_reason.is_some()
                    || self.released != ReservationUnits::default()
                {
                    return Err("settlement_reserved_terminal_payload_invalid".to_owned());
                }
                if self.usage_class == Some(SettlementUsageClass::Unknown) {
                    return Err("settlement_reserved_unknown_usage_invalid".to_owned());
                }
            }
            SettlementFoldState::Consumed => {
                if self.receipt_id.is_none()
                    || self.unknown_reason.is_some()
                    || self.consumed.is_none()
                    || self.usage_digest.is_none()
                    || !matches!(
                        self.usage_class,
                        Some(SettlementUsageClass::Known | SettlementUsageClass::Partial)
                    )
                {
                    return Err("settlement_consumed_payload_invalid".to_owned());
                }
                if self.released != ReservationUnits::default() {
                    return Err("settlement_consumed_release_requires_explicit_event".to_owned());
                }
            }
            SettlementFoldState::Released => {
                if self.receipt_id.is_none()
                    || self.unknown_reason.is_some()
                    || self.consumed.is_none()
                    || self.usage_digest.is_none()
                    || self.usage_class != Some(SettlementUsageClass::Known)
                {
                    return Err("settlement_released_payload_invalid".to_owned());
                }
                let expected = ReservationUnits::checked_difference(
                    self.reserved,
                    self.consumed.as_ref().expect("checked above"),
                )?;
                if self.released != expected {
                    return Err("settlement_release_must_be_unused_reservation".to_owned());
                }
            }
            SettlementFoldState::Unknown => {
                if self.receipt_id.is_some()
                    || self.unknown_reason.is_none()
                    || !self.reconciliation_required
                    || self.usage_class != Some(SettlementUsageClass::Unknown)
                    || self.released != ReservationUnits::default()
                {
                    return Err("settlement_unknown_payload_invalid".to_owned());
                }
            }
        }
        if let Some(digest) = &self.usage_digest {
            valid_digest(digest, "settlement_usage_digest")?;
        }
        if self.record_digest != self.digest() {
            return Err("settlement_record_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "model_attempt_id": self.model_attempt_id,
            "attempt_id": self.attempt_id,
            "reservation_id": self.reservation_id,
            "reservation_digest": self.reservation_digest,
            "state": self.state,
            "usage_class": self.usage_class,
            "usage_digest": self.usage_digest,
            "receipt_id": self.receipt_id,
            "reserved": self.reserved,
            "consumed": self.consumed,
            "released": self.released,
            "unknown_reason": self.unknown_reason,
            "reconciliation_required": self.reconciliation_required,
            "source_refs": self.source_refs,
            "revision": self.revision,
            "last_event_digest": self.last_event_digest,
        }))
    }
}

/// Immutable EventLog payload for the fold.  The record is derived from this envelope, never the
/// reverse; this keeps source event identity and replay order explicit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettlementFoldEvent {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: SettlementFoldEventKind,
    pub event_id: EventId,
    pub run_id: RunId,
    pub model_attempt_id: ModelAttemptId,
    pub attempt_id: AttemptId,
    pub reservation_id: crate::QuotaReservationId,
    pub reservation_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_class: Option<SettlementUsageClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<ReceiptId>,
    pub reserved: ReservationUnits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed: Option<ConsumedUnits>,
    pub released: ReservationUnits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_reason: Option<BillingUnknownReason>,
    pub reconciliation_required: bool,
    pub source_refs: Vec<SettlementSourceRef>,
    pub revision: u64,
    pub event_digest: String,
}

impl SettlementFoldEvent {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SETTLEMENT_FOLD_EVENT_SCHEMA
            || !self.version.is_compatible_with(&SETTLEMENT_FOLD_VERSION)
            || self.event_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.reservation_id.as_uuid().is_nil()
            || self.revision == 0
            || self.source_refs.is_empty()
            || self.source_refs.len() > 256
        {
            return Err("settlement_fold_event_header_invalid".to_owned());
        }
        valid_digest(&self.reservation_digest, "settlement_reservation_digest")?;
        valid_digest(&self.event_digest, "settlement_event_digest")?;
        self.reserved.validate("settlement_reserved_units", false)?;
        self.released.validate("settlement_released_units", true)?;
        for source in &self.source_refs {
            source.validate()?;
        }
        if let Some(consumed) = self.consumed {
            consumed.validate()?;
            if !consumed.fits_within(self.reserved) {
                return Err("settlement_consumption_exceeds_reservation".to_owned());
            }
        }
        if self.released.requests > self.reserved.requests
            || self.released.tokens > self.reserved.tokens
            || self.released.concurrency > self.reserved.concurrency
        {
            return Err("settlement_release_exceeds_reservation".to_owned());
        }
        if self.kind.state() == SettlementFoldState::Unknown && !self.reconciliation_required {
            return Err("settlement_unknown_requires_reconciliation".to_owned());
        }
        if self.kind.state() != SettlementFoldState::Unknown && self.reconciliation_required {
            return Err("settlement_known_reconciliation_flag_invalid".to_owned());
        }
        if let Some(digest) = &self.usage_digest {
            valid_digest(digest, "settlement_usage_digest")?;
        }
        let record = SettlementFoldRecord::from_event_unchecked(self)?;
        record.validate_without_digest()?;
        if self.event_digest != self.digest() {
            return Err("settlement_event_digest_mismatch".to_owned());
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
            "model_attempt_id": self.model_attempt_id,
            "attempt_id": self.attempt_id,
            "reservation_id": self.reservation_id,
            "reservation_digest": self.reservation_digest,
            "usage_class": self.usage_class,
            "usage_digest": self.usage_digest,
            "receipt_id": self.receipt_id,
            "reserved": self.reserved,
            "consumed": self.consumed,
            "released": self.released,
            "unknown_reason": self.unknown_reason,
            "reconciliation_required": self.reconciliation_required,
            "source_refs": self.source_refs,
            "revision": self.revision,
        }))
    }
}

impl SettlementFoldRecord {
    fn from_event_unchecked(event: &SettlementFoldEvent) -> Result<Self, String> {
        Ok(Self {
            schema: SETTLEMENT_FOLD_SCHEMA.to_owned(),
            version: SETTLEMENT_FOLD_VERSION,
            run_id: event.run_id,
            model_attempt_id: event.model_attempt_id,
            attempt_id: event.attempt_id,
            reservation_id: event.reservation_id,
            reservation_digest: event.reservation_digest.clone(),
            state: event.kind.state(),
            usage_class: event.usage_class,
            usage_digest: event.usage_digest.clone(),
            receipt_id: event.receipt_id,
            reserved: event.reserved,
            consumed: event.consumed,
            released: event.released,
            unknown_reason: event.unknown_reason,
            reconciliation_required: event.reconciliation_required,
            source_refs: event.source_refs.clone(),
            revision: event.revision,
            last_event_digest: event.event_digest.clone(),
            record_digest: String::new(),
        })
    }

    fn validate_without_digest(&self) -> Result<(), String> {
        if self.schema != SETTLEMENT_FOLD_SCHEMA
            || !self.version.is_compatible_with(&SETTLEMENT_FOLD_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.model_attempt_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.reservation_id.as_uuid().is_nil()
            || self.revision == 0
            || self.source_refs.is_empty()
            || self.reconciliation_required != (self.state == SettlementFoldState::Unknown)
        {
            return Err("settlement_fold_record_invalid".to_owned());
        }
        valid_digest(&self.reservation_digest, "settlement_reservation_digest")?;
        self.reserved.validate("settlement_reserved_units", false)?;
        self.released.validate("settlement_released_units", true)?;
        for source in &self.source_refs {
            source.validate()?;
        }
        if let Some(consumed) = self.consumed {
            consumed.validate()?;
            if !consumed.fits_within(self.reserved) {
                return Err("settlement_consumption_exceeds_reservation".to_owned());
            }
        }
        if self.released.requests > self.reserved.requests
            || self.released.tokens > self.reserved.tokens
            || self.released.concurrency > self.reserved.concurrency
        {
            return Err("settlement_release_exceeds_reservation".to_owned());
        }
        match self.state {
            SettlementFoldState::Reserved => {
                if self.receipt_id.is_some()
                    || self.unknown_reason.is_some()
                    || self.released != ReservationUnits::default()
                    || self.usage_class == Some(SettlementUsageClass::Unknown)
                {
                    return Err("settlement_reserved_terminal_payload_invalid".to_owned());
                }
            }
            SettlementFoldState::Consumed => {
                if self.receipt_id.is_none()
                    || self.unknown_reason.is_some()
                    || self.consumed.is_none()
                    || self.usage_digest.is_none()
                    || !matches!(
                        self.usage_class,
                        Some(SettlementUsageClass::Known | SettlementUsageClass::Partial)
                    )
                    || self.released != ReservationUnits::default()
                {
                    return Err("settlement_consumed_payload_invalid".to_owned());
                }
            }
            SettlementFoldState::Released => {
                if self.receipt_id.is_none()
                    || self.unknown_reason.is_some()
                    || self.consumed.is_none()
                    || self.usage_digest.is_none()
                    || self.usage_class != Some(SettlementUsageClass::Known)
                {
                    return Err("settlement_released_payload_invalid".to_owned());
                }
                let expected = ReservationUnits::checked_difference(
                    self.reserved,
                    self.consumed.as_ref().expect("checked above"),
                )?;
                if self.released != expected {
                    return Err("settlement_release_must_be_unused_reservation".to_owned());
                }
            }
            SettlementFoldState::Unknown => {
                if self.receipt_id.is_some()
                    || self.unknown_reason.is_none()
                    || !self.reconciliation_required
                    || self.usage_class != Some(SettlementUsageClass::Unknown)
                    || self.released != ReservationUnits::default()
                {
                    return Err("settlement_unknown_payload_invalid".to_owned());
                }
            }
        }
        if let Some(digest) = &self.usage_digest {
            valid_digest(digest, "settlement_usage_digest")?;
        }
        Ok(())
    }
}

/// Fold one event into a latest record.  Replaying the same event is idempotent; a second
/// settlement/release or an identity/digest conflict is rejected.
pub fn apply_settlement_fold_event(
    current: Option<SettlementFoldRecord>,
    event: &SettlementFoldEvent,
) -> Result<SettlementFoldRecord, String> {
    event.validate()?;
    let Some(current) = current else {
        if event.kind != SettlementFoldEventKind::Reserved || event.revision != 1 {
            return Err("settlement_requires_reserved_fact".to_owned());
        }
        return SettlementFoldRecord::from_event(event);
    };
    if current.attempt_id != event.attempt_id
        || current.run_id != event.run_id
        || current.model_attempt_id != event.model_attempt_id
        || current.reservation_id != event.reservation_id
        || current.reservation_digest != event.reservation_digest
    {
        return Err("settlement_identity_or_reservation_conflict".to_owned());
    }
    if event.revision == current.revision && current.last_event_digest == event.event_digest {
        return Ok(current);
    }
    if event.revision != current.revision.saturating_add(1) {
        return Err("settlement_revision_conflict".to_owned());
    }
    if current.source_refs.iter().any(|source| {
        event
            .source_refs
            .iter()
            .any(|next| next.event_id == source.event_id)
    }) {
        return Err("settlement_source_event_replay_conflict".to_owned());
    }
    if current
        .usage_digest
        .as_ref()
        .zip(event.usage_digest.as_ref())
        .is_some_and(|(observed, settled)| observed != settled)
    {
        return Err("settlement_usage_digest_conflict".to_owned());
    }
    let allowed = matches!(
        (current.state, event.kind.state()),
        (SettlementFoldState::Reserved, SettlementFoldState::Reserved)
            | (SettlementFoldState::Reserved, SettlementFoldState::Consumed)
            | (SettlementFoldState::Reserved, SettlementFoldState::Unknown)
            | (SettlementFoldState::Consumed, SettlementFoldState::Released)
    );
    if !allowed {
        return Err(match current.state {
            SettlementFoldState::Consumed => "settlement_duplicate_settlement",
            SettlementFoldState::Released => "settlement_duplicate_release",
            SettlementFoldState::Unknown => "settlement_unknown_requires_reconciliation",
            SettlementFoldState::Reserved => "settlement_state_transition_invalid",
        }
        .to_owned());
    }
    let mut next = SettlementFoldRecord::from_event_unchecked(event)?;
    next.source_refs = current
        .source_refs
        .into_iter()
        .chain(event.source_refs.clone())
        .collect();
    next.record_digest = next.digest();
    next.validate()?;
    Ok(next)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettlementFoldApplyOutcome {
    Applied,
    Duplicate,
}

#[derive(Default)]
pub struct SettlementFoldLedger {
    entries: BTreeMap<AttemptId, SettlementFoldRecord>,
}

impl SettlementFoldLedger {
    pub fn apply(
        &mut self,
        event: SettlementFoldEvent,
    ) -> Result<SettlementFoldApplyOutcome, String> {
        event.validate()?;
        let current = self.entries.get(&event.attempt_id).cloned();
        let duplicate = current.as_ref().is_some_and(|record| {
            record.revision == event.revision && record.last_event_digest == event.event_digest
        });
        let next = apply_settlement_fold_event(current, &event)?;
        self.entries.insert(event.attempt_id, next);
        Ok(if duplicate {
            SettlementFoldApplyOutcome::Duplicate
        } else {
            SettlementFoldApplyOutcome::Applied
        })
    }

    /// Advance a held reservation from an observed BQ-11 lifecycle record to its known or unknown
    /// terminal fold. The usage digest must match any earlier observation; unknown remains held.
    pub fn settle_from_lifecycle(
        &mut self,
        reservation: &QuotaReservation,
        lifecycle: &ModelAttemptLifecycleRecord,
        source: SettlementSourceRef,
    ) -> Result<SettlementFoldEvent, String> {
        reservation.validate()?;
        lifecycle.validate()?;
        let current = self
            .entries
            .get(&lifecycle.attempt_id)
            .ok_or_else(|| "settlement_attempt_missing".to_owned())?;
        if current.state != SettlementFoldState::Reserved {
            return Err("settlement_attempt_not_reserved".to_owned());
        }
        if current.run_id != lifecycle.run_id
            || current.model_attempt_id != lifecycle.model_attempt_id
            || current.attempt_id != reservation.owner_attempt
            || current.reservation_id != reservation.reservation_id
            || current.reservation_digest != reservation.reservation_digest
        {
            return Err("settlement_identity_or_reservation_conflict".to_owned());
        }
        let mut event = SettlementFoldRecord::from_lifecycle(reservation, lifecycle, source)?;
        if !matches!(
            event.kind,
            SettlementFoldEventKind::Consumed | SettlementFoldEventKind::Unknown
        ) {
            return Err("settlement_terminal_lifecycle_required".to_owned());
        }
        event.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| "settlement_revision_overflow".to_owned())?;
        event.event_digest = event.digest();
        event.validate()?;
        self.apply(event.clone())?;
        Ok(event)
    }

    /// Record a committed observation while retaining the reservation. This is a fold update, not
    /// a settlement: the reservation cannot be released until a later known terminal fact.
    pub fn observe_from_lifecycle(
        &mut self,
        reservation: &QuotaReservation,
        lifecycle: &ModelAttemptLifecycleRecord,
        source: SettlementSourceRef,
    ) -> Result<SettlementFoldEvent, String> {
        reservation.validate()?;
        lifecycle.validate()?;
        if lifecycle.state != ModelAttemptState::Observed {
            return Err("settlement_observed_lifecycle_required".to_owned());
        }
        let current = self
            .entries
            .get(&lifecycle.attempt_id)
            .cloned()
            .ok_or_else(|| "settlement_attempt_missing".to_owned())?;
        if current.state != SettlementFoldState::Reserved
            || current.run_id != lifecycle.run_id
            || current.model_attempt_id != lifecycle.model_attempt_id
            || current.reservation_id != reservation.reservation_id
            || current.reservation_digest != reservation.reservation_digest
        {
            return Err("settlement_identity_or_reservation_conflict".to_owned());
        }
        let mut event = SettlementFoldRecord::from_lifecycle(reservation, lifecycle, source)?;
        event.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| "settlement_revision_overflow".to_owned())?;
        event.event_digest = event.digest();
        event.validate()?;
        self.apply(event.clone())?;
        Ok(event)
    }

    /// Release only the portion that was not consumed by a complete, known settlement.  Unknown
    /// and partial usage stay occupied until a later explicit reconciliation step.
    pub fn release_unused(
        &mut self,
        attempt_id: AttemptId,
        source: SettlementSourceRef,
    ) -> Result<SettlementFoldEvent, String> {
        let current = self
            .entries
            .get(&attempt_id)
            .cloned()
            .ok_or_else(|| "settlement_attempt_missing".to_owned())?;
        if current.state != SettlementFoldState::Consumed {
            return Err(match current.state {
                SettlementFoldState::Unknown => "settlement_unknown_cannot_release",
                SettlementFoldState::Released => "settlement_duplicate_release",
                SettlementFoldState::Reserved => "settlement_not_consumed",
                SettlementFoldState::Consumed => "settlement_not_consumed",
            }
            .to_owned());
        }
        if current.usage_class != Some(SettlementUsageClass::Known) {
            return Err("settlement_partial_release_requires_reconciliation".to_owned());
        }
        let consumed = current
            .consumed
            .ok_or_else(|| "settlement_consumption_missing".to_owned())?;
        let released = ReservationUnits::checked_difference(current.reserved, &consumed)?;
        let mut event = SettlementFoldEvent {
            schema: SETTLEMENT_FOLD_EVENT_SCHEMA.to_owned(),
            version: SETTLEMENT_FOLD_VERSION,
            kind: SettlementFoldEventKind::Released,
            event_id: EventId::new(),
            run_id: current.run_id,
            model_attempt_id: current.model_attempt_id,
            attempt_id: current.attempt_id,
            reservation_id: current.reservation_id,
            reservation_digest: current.reservation_digest.clone(),
            usage_class: current.usage_class,
            usage_digest: current.usage_digest.clone(),
            receipt_id: current.receipt_id,
            reserved: current.reserved,
            consumed: current.consumed,
            released,
            unknown_reason: None,
            reconciliation_required: false,
            source_refs: vec![source],
            revision: current
                .revision
                .checked_add(1)
                .ok_or_else(|| "settlement_revision_overflow".to_owned())?,
            event_digest: String::new(),
        };
        event.event_digest = event.digest();
        event.validate()?;
        self.apply(event.clone())?;
        Ok(event)
    }

    pub fn get(&self, attempt_id: AttemptId) -> Option<&SettlementFoldRecord> {
        self.entries.get(&attempt_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn usage_class(usage: &NormalizedUsage) -> Result<SettlementUsageClass, String> {
    usage.validate()?;
    Ok(match usage.confidence {
        UsageConfidence::Known => SettlementUsageClass::Known,
        UsageConfidence::Partial => SettlementUsageClass::Partial,
        UsageConfidence::Unknown => SettlementUsageClass::Unknown,
    })
}

fn consumed_from_usage(usage: &NormalizedUsage) -> Result<ConsumedUnits, String> {
    usage.validate()?;
    if usage.confidence == UsageConfidence::Unknown {
        return Err("settlement_unknown_usage_not_consumable".to_owned());
    }
    let mut tokens = 0_u64;
    let mut any_tokens = false;
    for value in [
        usage.vector.input_tokens,
        usage.vector.output_tokens,
        usage.vector.cache_read_tokens,
        usage.vector.cache_write_tokens,
        usage.vector.reasoning_output_tokens,
        usage.vector.audio_input_tokens,
        usage.vector.audio_output_tokens,
    ]
    .into_iter()
    .flatten()
    {
        any_tokens = true;
        tokens = tokens
            .checked_add(value)
            .ok_or_else(|| "settlement_usage_token_overflow".to_owned())?;
    }
    if usage.confidence == UsageConfidence::Known
        && usage.vector.input_tokens.is_none()
        && usage.vector.output_tokens.is_none()
    {
        return Err("settlement_known_usage_incomplete".to_owned());
    }
    Ok(ConsumedUnits {
        requests: 1,
        tokens: any_tokens.then_some(tokens),
        concurrency: 1,
    })
}
