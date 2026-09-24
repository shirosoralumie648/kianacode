//! Estimated/measured cost contracts used by receipts and read-only query projections.
//!
//! A cost breakdown is an observation of one provider attempt.  It never authorizes a request,
//! changes a reservation, or claims that an external invoice was paid.  Estimates are pinned to
//! the exact rate-card id/version used for the arithmetic; measured values carry only an opaque
//! provider receipt and are accepted only after the caller proves complete usage and rate-card
//! scope.  Missing usage and missing prices remain explicit unknowns rather than zeroes.

use crate::{
    json_digest, AttemptId, BillingPriceDimension, BillingUnknownReason, EventId, Money,
    NormalizedUsage, ProviderReceiptRef, RateCard, RateCardId, SchemaVersion, UsageConfidence,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const COST_BREAKDOWN_SCHEMA: &str = "kiana.cost-breakdown.v1";
pub const RECEIPT_COST_BREAKDOWN_SCHEMA: &str = "kiana.receipt-cost-breakdown.v1";
pub const COST_BREAKDOWN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const COST_EVENT_ESTIMATED: &str = "usage.cost_estimated";
pub const COST_EVENT_MEASURED: &str = "usage.cost_measured";
pub const COST_EVENT_UNKNOWN: &str = "usage.cost_unknown";
const MAX_COST_LINES: usize = 32;
const MAX_COST_ENTRIES: usize = 512;
const MAX_UNKNOWN_REASONS: usize = 32;
const MAX_SOURCE_EVENT_IDS: usize = 1_024;

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// One rate-card line.  An absent unit or absent price is preserved as `None`; the line never
/// substitutes zero for a dimension that was not observed or not priced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostLine {
    pub dimension: BillingPriceDimension,
    pub units: Option<u64>,
    pub unit_price_micros: Option<u64>,
    pub amount: Option<Money>,
}

impl CostLine {
    fn new(
        dimension: BillingPriceDimension,
        units: Option<u64>,
        unit_price_micros: Option<u64>,
        currency: &str,
    ) -> Result<Self, String> {
        let amount = match (units, unit_price_micros) {
            (Some(units), Some(price)) => Some(Money::new(
                currency.to_owned(),
                i128::from(units)
                    .checked_mul(i128::from(price))
                    .ok_or_else(|| "cost_line_arithmetic_overflow".to_owned())?,
            )?),
            _ => None,
        };
        let line = Self {
            dimension,
            units,
            unit_price_micros,
            amount,
        };
        line.validate()
            .map_err(|reason| format!("cost_line_{reason}"))?;
        Ok(line)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(amount) = &self.amount {
            amount.validate()?;
            let (Some(units), Some(price)) = (self.units, self.unit_price_micros) else {
                return Err("amount_without_units_or_price".to_owned());
            };
            let expected = i128::from(units)
                .checked_mul(i128::from(price))
                .ok_or_else(|| "arithmetic_overflow".to_owned())?;
            if amount.micros != expected {
                return Err("amount_mismatch".to_owned());
            }
        }
        Ok(())
    }
}

/// The mutually exclusive cost states exposed by a receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CostBreakdownKind {
    Estimated {
        estimate: crate::CostEstimate,
    },
    Measured {
        amount: Money,
        provider_receipt: ProviderReceiptRef,
    },
    Unknown {
        reason: BillingUnknownReason,
    },
}

impl CostBreakdownKind {
    pub const fn event_kind(&self) -> &'static str {
        match self {
            Self::Estimated { .. } => COST_EVENT_ESTIMATED,
            Self::Measured { .. } => COST_EVENT_MEASURED,
            Self::Unknown { .. } => COST_EVENT_UNKNOWN,
        }
    }

    pub const fn unknown_reason(&self) -> Option<BillingUnknownReason> {
        match self {
            Self::Unknown { reason } => Some(*reason),
            _ => None,
        }
    }

    pub fn amount(&self) -> Option<&Money> {
        match self {
            Self::Estimated { estimate } => estimate.amount.as_ref(),
            Self::Measured { amount, .. } => Some(amount),
            Self::Unknown { .. } => None,
        }
    }

    pub fn is_estimated(&self) -> bool {
        matches!(self, Self::Estimated { .. })
    }

    pub fn is_measured(&self) -> bool {
        matches!(self, Self::Measured { .. })
    }

    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Estimated { estimate } => {
                estimate.validate()?;
                estimate
                    .amount
                    .as_ref()
                    .map(|_| ())
                    .ok_or_else(|| "estimated_amount_missing".to_owned())
            }
            Self::Measured {
                amount,
                provider_receipt,
            } => {
                amount.validate()?;
                ProviderReceiptRef::new(provider_receipt.as_str().to_owned())
                    .map(|_| ())
                    .map_err(|_| "provider_receipt_invalid".to_owned())
            }
            Self::Unknown { .. } => Ok(()),
        }
    }
}

/// A per-attempt cost fact.  Identity and usage digest bind the amount to one normalized usage
/// observation, preventing a query or UI from joining costs by array position alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostBreakdown {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: crate::RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<AttemptId>,
    pub usage_digest: String,
    pub usage_confidence: UsageConfidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_id: Option<RateCardId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_version: Option<u64>,
    pub kind: CostBreakdownKind,
    #[serde(default)]
    pub lines: Vec<CostLine>,
    pub breakdown_digest: String,
}

impl CostBreakdown {
    /// Project a normalized usage observation into an estimate or an explicit unknown.  A missing
    /// card, incomplete usage or a card that has no price for an observed dimension never becomes
    /// a zero amount.  The estimate itself carries the exact card id and card version.
    pub fn from_usage(
        usage: &NormalizedUsage,
        rate_card: Option<&RateCard>,
        request_count: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        usage.validate()?;
        if request_count == 0 || observed_at_unix_ms == 0 {
            return Err("cost_observation_invalid".to_owned());
        }
        if !usage_is_complete(usage) {
            return Self::unknown(
                usage.run_id,
                Some(usage.attempt_id),
                usage.usage_digest.clone(),
                usage.confidence,
                BillingUnknownReason::Partial,
            );
        }
        let Some(rate_card) = rate_card else {
            return Self::unknown(
                usage.run_id,
                Some(usage.attempt_id),
                usage.usage_digest.clone(),
                usage.confidence,
                BillingUnknownReason::RateCardMissing,
            );
        };
        if usage.confidence != UsageConfidence::Known {
            return Self::unknown(
                usage.run_id,
                Some(usage.attempt_id),
                usage.usage_digest.clone(),
                usage.confidence,
                usage
                    .unknown_reason
                    .unwrap_or(BillingUnknownReason::Partial),
            );
        }
        rate_card.validate()?;
        validate_card_scope(usage, rate_card, observed_at_unix_ms)?;
        let estimate = rate_card.estimate(&usage.vector, request_count)?;
        let reason = estimate.unknown_reason;
        let lines = reason
            .is_none()
            .then(|| build_lines(rate_card, usage, request_count))
            .transpose()?
            .unwrap_or_default();
        let kind = match reason {
            Some(reason) => CostBreakdownKind::Unknown { reason },
            None => CostBreakdownKind::Estimated { estimate },
        };
        Self::new(
            usage.run_id,
            Some(usage.attempt_id),
            usage.usage_digest.clone(),
            usage.confidence,
            Some(rate_card),
            kind,
            lines,
        )
    }

    /// Attach a measured provider amount.  The caller must provide a RateCard even though the
    /// serialized measured state carries only the provider receipt: this proves the attempt was
    /// admitted under a pinned pricing scope before a measured amount is accepted.  The card is
    /// never copied into the measured payload, so measured receipts cannot be mistaken for an
    /// estimate or silently repriced later.
    pub fn from_provider_receipt(
        usage: &NormalizedUsage,
        rate_card: Option<&RateCard>,
        amount: Money,
        provider_receipt: ProviderReceiptRef,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        usage.validate()?;
        if usage.confidence != UsageConfidence::Known || !usage_is_complete(usage) {
            return Err("cost_measured_usage_incomplete".to_owned());
        }
        let Some(rate_card) = rate_card else {
            return Err("cost_measured_rate_card_missing".to_owned());
        };
        rate_card.validate()?;
        validate_card_scope(usage, rate_card, observed_at_unix_ms)?;
        amount.validate()?;
        if amount.currency != rate_card.currency {
            return Err("cost_measured_currency_mismatch".to_owned());
        }
        provider_receipt
            .validate()
            .map_err(|_| "provider_receipt_invalid".to_owned())?;
        Self::new(
            usage.run_id,
            Some(usage.attempt_id),
            usage.usage_digest.clone(),
            usage.confidence,
            None,
            CostBreakdownKind::Measured {
                amount,
                provider_receipt,
            },
            Vec::new(),
        )
    }

    /// Construct an explicit unknown cost without fabricating a numeric amount.
    pub fn unknown(
        run_id: crate::RunId,
        attempt_id: Option<AttemptId>,
        usage_digest: impl Into<String>,
        usage_confidence: UsageConfidence,
        reason: BillingUnknownReason,
    ) -> Result<Self, String> {
        Self::new(
            run_id,
            attempt_id,
            usage_digest.into(),
            usage_confidence,
            None,
            CostBreakdownKind::Unknown { reason },
            Vec::new(),
        )
    }

    fn new(
        run_id: crate::RunId,
        attempt_id: Option<AttemptId>,
        usage_digest: String,
        usage_confidence: UsageConfidence,
        rate_card: Option<&RateCard>,
        kind: CostBreakdownKind,
        lines: Vec<CostLine>,
    ) -> Result<Self, String> {
        let mut result = Self {
            schema: COST_BREAKDOWN_SCHEMA.to_owned(),
            version: COST_BREAKDOWN_VERSION,
            run_id,
            attempt_id,
            usage_digest,
            usage_confidence,
            rate_card_id: rate_card.map(|card| card.rate_card_id),
            rate_card_version: rate_card.map(|card| card.card_version),
            kind,
            lines,
            breakdown_digest: String::new(),
        };
        result.breakdown_digest = result.digest();
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_BREAKDOWN_SCHEMA
            || !self.version.is_compatible_with(&COST_BREAKDOWN_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self
                .attempt_id
                .is_some_and(|attempt_id| attempt_id.as_uuid().is_nil())
            || self.rate_card_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.rate_card_version == Some(0)
            || self.rate_card_id.is_some() != self.rate_card_version.is_some()
            || self.breakdown_digest != self.digest()
            || self.lines.len() > MAX_COST_LINES
        {
            return Err("cost_breakdown_header_invalid".to_owned());
        }
        valid_digest(&self.usage_digest, "cost_usage_digest")?;
        valid_digest(&self.breakdown_digest, "cost_breakdown_digest")?;
        self.kind.validate()?;
        match &self.kind {
            CostBreakdownKind::Estimated { estimate } => {
                if self.usage_confidence != UsageConfidence::Known
                    || self.rate_card_id != Some(estimate.rate_card_id)
                    || self.rate_card_version != Some(estimate.rate_card_version)
                {
                    return Err("cost_estimate_provenance_mismatch".to_owned());
                }
            }
            CostBreakdownKind::Measured { .. } => {
                if self.usage_confidence != UsageConfidence::Known || self.attempt_id.is_none() {
                    return Err("cost_measured_provenance_incomplete".to_owned());
                }
                if self.rate_card_id.is_some() || self.rate_card_version.is_some() {
                    return Err("cost_measured_rate_card_forbidden".to_owned());
                }
            }
            CostBreakdownKind::Unknown { .. } => {}
        }
        let mut dimensions = BTreeSet::new();
        for line in &self.lines {
            if !dimensions.insert(line.dimension as u8) {
                return Err("cost_breakdown_dimension_duplicate".to_owned());
            }
            line.validate()?;
        }
        if self.kind.is_measured() && !self.lines.is_empty() {
            return Err("cost_measured_lines_forbidden".to_owned());
        }
        if self.kind.unknown_reason().is_some() && !self.lines.is_empty() {
            return Err("cost_unknown_lines_forbidden".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "usage_digest": self.usage_digest,
            "usage_confidence": self.usage_confidence,
            "rate_card_id": self.rate_card_id,
            "rate_card_version": self.rate_card_version,
            "kind": self.kind,
            "lines": self.lines,
        }))
    }

    pub fn to_runtime_event(
        &self,
        request_id: crate::RequestId,
        sequence: u64,
    ) -> Result<crate::RuntimeEvent, String> {
        self.validate()?;
        let attempt_id = self
            .attempt_id
            .ok_or_else(|| "cost_event_attempt_required".to_owned())?;
        crate::RuntimeEvent::new(
            request_id,
            sequence,
            self.kind.event_kind(),
            serde_json::to_value(self).map_err(|_| "cost_breakdown_encode_failed".to_owned())?,
        )
        .map(|event| {
            event.with_stream_metadata("billing_attempt", attempt_id.to_string(), sequence)
        })
        .map_err(|_| "cost_breakdown_runtime_event_failed".to_owned())
    }
}

/// A receipt-level fold over per-attempt cost facts.  Estimated and measured totals remain in
/// separate fields; unknown reasons are retained even when another attempt has a known amount.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptCostBreakdown {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: crate::RunId,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub entries: Vec<CostBreakdown>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_total: Option<Money>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured_total: Option<Money>,
    #[serde(default)]
    pub unknown_reasons: Vec<BillingUnknownReason>,
    pub breakdown_digest: String,
}

impl ReceiptCostBreakdown {
    pub fn from_entries(
        mut entries: Vec<CostBreakdown>,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
    ) -> Result<Self, String> {
        if entries.is_empty() || entries.len() > MAX_COST_ENTRIES {
            return Err("receipt_cost_entries_invalid".to_owned());
        }
        let run_id = entries[0].run_id;
        if run_id.as_uuid().is_nil() || entries.iter().any(|entry| entry.run_id != run_id) {
            return Err("receipt_cost_run_mismatch".to_owned());
        }
        let source_event_ids = canonical_source_event_ids(source_event_ids)?;
        if source_cursor == 0 {
            return Err("receipt_cost_source_cursor_invalid".to_owned());
        }
        for entry in &entries {
            entry.validate()?;
        }
        entries.sort_by(|left, right| left.breakdown_digest.cmp(&right.breakdown_digest));
        let mut seen = BTreeSet::new();
        if entries
            .iter()
            .any(|entry| !seen.insert(entry.breakdown_digest.clone()))
        {
            return Err("receipt_cost_entry_duplicate".to_owned());
        }
        let (estimated_total, measured_total, unknown_reasons) = derive_totals(&entries)?;
        let mut result = Self {
            schema: RECEIPT_COST_BREAKDOWN_SCHEMA.to_owned(),
            version: COST_BREAKDOWN_VERSION,
            run_id,
            source_cursor,
            source_event_ids,
            entries,
            estimated_total,
            measured_total,
            unknown_reasons,
            breakdown_digest: String::new(),
        };
        result.breakdown_digest = result.digest();
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECEIPT_COST_BREAKDOWN_SCHEMA
            || !self.version.is_compatible_with(&COST_BREAKDOWN_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.entries.is_empty()
            || self.entries.len() > MAX_COST_ENTRIES
            || self.unknown_reasons.len() > MAX_UNKNOWN_REASONS
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self.source_event_ids.iter().any(|id| id.as_uuid().is_nil())
            || self
                .source_event_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.breakdown_digest != self.digest()
        {
            return Err("receipt_cost_breakdown_header_invalid".to_owned());
        }
        let mut seen = BTreeSet::new();
        if self.entries.iter().any(|entry| {
            entry.run_id != self.run_id
                || entry.validate().is_err()
                || !seen.insert(entry.breakdown_digest.clone())
        }) {
            return Err("receipt_cost_breakdown_entries_invalid".to_owned());
        }
        let (estimated_total, measured_total, unknown_reasons) = derive_totals(&self.entries)?;
        if self.estimated_total != estimated_total
            || self.measured_total != measured_total
            || self.unknown_reasons != unknown_reasons
        {
            return Err("receipt_cost_breakdown_total_mismatch".to_owned());
        }
        if let Some(amount) = &self.estimated_total {
            amount.validate()?;
        }
        if let Some(amount) = &self.measured_total {
            amount.validate()?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "source_cursor": self.source_cursor,
            "source_event_ids": self.source_event_ids,
            "entries": self.entries,
            "estimated_total": self.estimated_total,
            "measured_total": self.measured_total,
            "unknown_reasons": self.unknown_reasons,
        }))
    }

    pub fn has_unknown(&self) -> bool {
        !self.unknown_reasons.is_empty()
    }
}

fn add_money(current: Option<Money>, next: &Money) -> Result<Money, String> {
    match current {
        Some(current) => current.checked_add(next),
        None => Ok(next.clone()),
    }
}

fn derive_totals(
    entries: &[CostBreakdown],
) -> Result<(Option<Money>, Option<Money>, Vec<BillingUnknownReason>), String> {
    let mut estimated_total = None;
    let mut measured_total = None;
    let mut unknown_reasons = Vec::new();
    for entry in entries {
        match &entry.kind {
            CostBreakdownKind::Estimated { estimate } => {
                if let Some(amount) = &estimate.amount {
                    estimated_total = Some(add_money(estimated_total, amount)?);
                }
            }
            CostBreakdownKind::Measured { amount, .. } => {
                measured_total = Some(add_money(measured_total, amount)?);
            }
            CostBreakdownKind::Unknown { reason } => {
                if !unknown_reasons.contains(reason) {
                    unknown_reasons.push(*reason);
                }
            }
        }
    }
    unknown_reasons.sort_by_key(|reason| reason.as_str());
    Ok((estimated_total, measured_total, unknown_reasons))
}

fn usage_is_complete(usage: &NormalizedUsage) -> bool {
    [
        usage.vector.input_tokens,
        usage.vector.output_tokens,
        usage.vector.cache_read_tokens,
        usage.vector.cache_write_tokens,
        usage.vector.reasoning_output_tokens,
        usage.vector.audio_input_tokens,
        usage.vector.audio_output_tokens,
    ]
    .into_iter()
    .all(|value| value.is_some())
}

fn validate_card_scope(
    usage: &NormalizedUsage,
    rate_card: &RateCard,
    observed_at_unix_ms: u64,
) -> Result<(), String> {
    let provider = usage
        .provider_id
        .as_deref()
        .ok_or_else(|| "cost_provider_missing".to_owned())?;
    if observed_at_unix_ms == 0
        || rate_card.provider_id != provider
        || !rate_card.is_effective_at(observed_at_unix_ms)
        || rate_card.model_selector != "*" && rate_card.model_selector != usage.requested_model_id
    {
        return Err("cost_rate_card_scope_mismatch".to_owned());
    }
    Ok(())
}

fn build_lines(
    card: &RateCard,
    usage: &NormalizedUsage,
    request_count: u64,
) -> Result<Vec<CostLine>, String> {
    let vector = &usage.vector;
    [
        (
            BillingPriceDimension::InputTokens,
            vector.input_tokens,
            card.input_price_per_unit,
        ),
        (
            BillingPriceDimension::OutputTokens,
            vector.output_tokens,
            card.output_price_per_unit,
        ),
        (
            BillingPriceDimension::CacheReadTokens,
            vector.cache_read_tokens,
            card.cache_read_price_per_unit,
        ),
        (
            BillingPriceDimension::CacheWriteTokens,
            vector.cache_write_tokens,
            card.cache_write_price_per_unit,
        ),
        (
            BillingPriceDimension::ReasoningTokens,
            vector.reasoning_output_tokens,
            card.reasoning_price_per_unit,
        ),
        (
            BillingPriceDimension::AudioInputTokens,
            vector.audio_input_tokens,
            card.audio_input_price_per_unit,
        ),
        (
            BillingPriceDimension::AudioOutputTokens,
            vector.audio_output_tokens,
            card.audio_output_price_per_unit,
        ),
        (
            BillingPriceDimension::Requests,
            Some(request_count),
            card.request_price,
        ),
        (
            BillingPriceDimension::ToolCalls,
            Some(vector.tool_calls),
            card.tool_price,
        ),
        (
            BillingPriceDimension::Effects,
            Some(vector.effect_count),
            card.effect_price,
        ),
    ]
    .into_iter()
    .filter(|(_, units, price)| units.is_some() || price.is_some())
    .map(|(dimension, units, price)| CostLine::new(dimension, units, price, &card.currency))
    .collect()
}

impl ProviderReceiptRef {
    fn validate(&self) -> Result<(), String> {
        ProviderReceiptRef::new(self.as_str().to_owned())
            .map(|_| ())
            .map_err(|_| "provider_receipt_invalid".to_owned())
    }
}

impl CostBreakdown {
    /// Return the opaque provider receipt when this is a measured fact.
    pub fn provider_receipt(&self) -> Option<&ProviderReceiptRef> {
        match &self.kind {
            CostBreakdownKind::Measured {
                provider_receipt, ..
            } => Some(provider_receipt),
            _ => None,
        }
    }

    /// A cost projection is never a financial authorization or execution budget input.
    pub const fn projection_only(&self) -> bool {
        true
    }
}

/// Source event IDs are kept bounded and deduplicated by query projectors.  This helper is shared
/// by the core receipt and query adapters without making either adapter a second source of truth.
pub fn canonical_source_event_ids(mut event_ids: Vec<EventId>) -> Result<Vec<EventId>, String> {
    event_ids.sort();
    event_ids.dedup();
    if event_ids.is_empty()
        || event_ids.len() > MAX_SOURCE_EVENT_IDS
        || event_ids.iter().any(|event_id| event_id.as_uuid().is_nil())
    {
        return Err("receipt_cost_source_events_invalid".to_owned());
    }
    Ok(event_ids)
}

/// Rebuild the per-attempt cost view from committed cost events.  Only BQ-13 event kinds are
/// consumed; malformed, cross-run, unbound, duplicate or regressing facts fail closed.  A newer
/// provider measured receipt may supersede an estimate for the same attempt, but never causes both
/// amounts to be counted in the receipt total.
pub fn project_receipt_cost_breakdown(
    run_id: crate::RunId,
    events: &[crate::RuntimeEvent],
) -> Result<Option<ReceiptCostBreakdown>, String> {
    if run_id.as_uuid().is_nil() {
        return Err("receipt_cost_run_invalid".to_owned());
    }
    let cost_kinds = [
        COST_EVENT_ESTIMATED,
        COST_EVENT_MEASURED,
        COST_EVENT_UNKNOWN,
    ];
    let run_id_text = run_id.to_string();
    let mut attempts: BTreeMap<AttemptId, (u64, CostBreakdown)> = BTreeMap::new();
    let mut event_ids = Vec::new();
    let mut seen_event_ids = BTreeSet::new();
    let mut source_cursor = 0;
    for event in events {
        if !cost_kinds.contains(&event.kind.as_str()) {
            continue;
        }
        if !seen_event_ids.insert(event.event_id) {
            return Err("receipt_cost_duplicate_source_event".to_owned());
        }
        let fact: CostBreakdown = serde_json::from_value(event.data.clone())
            .map_err(|_| "receipt_cost_event_decode_failed".to_owned())?;
        fact.validate()?;
        if fact.run_id != run_id
            || event.data.get("run_id").and_then(serde_json::Value::as_str)
                != Some(run_id_text.as_str())
        {
            return Err("receipt_cost_event_run_mismatch".to_owned());
        }
        if event.kind != fact.kind.event_kind() {
            return Err("receipt_cost_event_kind_mismatch".to_owned());
        }
        let attempt_id = fact
            .attempt_id
            .ok_or_else(|| "receipt_cost_event_attempt_missing".to_owned())?;
        if event.aggregate_type.as_deref() != Some("billing_attempt")
            || event.aggregate_id.as_deref() != Some(attempt_id.to_string().as_str())
        {
            return Err("receipt_cost_event_stream_mismatch".to_owned());
        }
        let revision = event
            .stream_version
            .filter(|revision| *revision > 0)
            .ok_or_else(|| "receipt_cost_event_revision_missing".to_owned())?;
        if let Some((previous_revision, previous)) = attempts.get(&attempt_id) {
            if revision != previous_revision.saturating_add(1) {
                return Err("receipt_cost_event_revision_gap".to_owned());
            }
            let allowed = matches!(
                (&previous.kind, &fact.kind),
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
            if !allowed || previous.usage_digest != fact.usage_digest {
                return Err("receipt_cost_event_transition_invalid".to_owned());
            }
        }
        attempts.insert(attempt_id, (revision, fact));
        event_ids.push(event.event_id);
        source_cursor = source_cursor.max(event.sequence);
    }
    if event_ids.is_empty() {
        return Ok(None);
    }
    let entries = attempts
        .into_values()
        .map(|(_, fact)| fact)
        .collect::<Vec<_>>();
    ReceiptCostBreakdown::from_entries(entries, source_cursor, event_ids).map(Some)
}
