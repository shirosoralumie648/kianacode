//! Provider usage, price snapshot and settlement contracts.
//!
//! This module is deliberately a value-level boundary.  It normalizes the usage that a provider
//! reported, pins the rate-card version used for an estimate and keeps unknown/measured cost
//! distinct.  It does not reserve a capability, write an EventLog or authorize a retry.  Only the
//! accumulator's cumulative observation settles an attempt: a raw delta is rejected so a fraction
//! of an attempt's usage can never masquerade as its total.

use crate::{
    json_digest, AttemptId, BillingState, BillingUnknownReason, CostEstimate, ExecutionId,
    ModelProtocol, Money, NormalizedUsage, ProviderReceiptRef, RateCard, RateCardId, SchemaVersion,
    UsageConfidence, UsageId, UsageObservation, UsageVector,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROVIDER_USAGE_SETTLEMENT_SCHEMA: &str = "kiana.provider-usage-settlement.v1";
pub const PROVIDER_USAGE_SETTLEMENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const PROVIDER_USAGE_CONTAINMENT_SCHEMA: &str = "kiana.provider-usage-containment.v1";
pub const PROVIDER_RATE_CARD_SNAPSHOT_SCHEMA: &str = "kiana.provider-rate-card-snapshot.v1";
pub const PROVIDER_USAGE_CORRECTION_SCHEMA: &str = "kiana.provider-usage-correction.v1";
pub const PROVIDER_ATTEMPT_RESERVATION_SCHEMA: &str = "kiana.provider-attempt-reservation.v1";
pub const PROVIDER_USAGE_ATTEMPT_LEDGER_SCHEMA: &str = "kiana.provider-usage-attempt-ledger.v1";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SettlementCost {
    Unknown {
        reason: BillingUnknownReason,
    },
    Estimated {
        estimate: CostEstimate,
    },
    Measured {
        amount: Money,
        provider_receipt: ProviderReceiptRef,
    },
}

impl SettlementCost {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Unknown { .. } => Ok(()),
            Self::Estimated { estimate } => estimate.validate(),
            Self::Measured {
                amount,
                provider_receipt,
            } => {
                amount.validate()?;
                validate_provider_receipt(provider_receipt)?;
                Ok(())
            }
        }
    }

    fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// The reason a settlement has no amount, when it has none.
    ///
    /// A Receipt must be able to display *why* an amount is missing instead of showing a zero: an
    /// unknown observation carries its own reason, an unpriced dimension keeps the estimate's
    /// reason, and only a measured cost has nothing to explain.
    fn unknown_reason(&self) -> Option<BillingUnknownReason> {
        match self {
            Self::Unknown { reason } => Some(*reason),
            Self::Estimated { estimate } => estimate.unknown_reason,
            Self::Measured { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsageSettlement {
    pub schema: String,
    pub version: SchemaVersion,
    pub execution_id: Option<ExecutionId>,
    pub usage_id: UsageId,
    pub attempt_id: AttemptId,
    pub run_id: crate::RunId,
    pub provider_id: String,
    pub requested_model_id: String,
    pub served_model_id: Option<String>,
    pub route_id: String,
    pub retry_ordinal: u32,
    pub usage: NormalizedUsage,
    pub cost: SettlementCost,
    pub state: BillingState,
    pub idempotency_key: String,
    pub observed_at_unix_ms: u64,
    pub settlement_digest: String,
}

impl ProviderUsageSettlement {
    /// Settlement without a declared wire protocol.
    ///
    /// The rate-card arithmetic stays on the conservative overlap rule because no containment
    /// table was declared for this observation; see [`Self::from_usage_with_protocol`] when the
    /// admitted route knows its protocol.
    #[allow(clippy::too_many_arguments)]
    pub fn from_usage(
        usage: &NormalizedUsage,
        rate_card: Option<&RateCard>,
        request_count: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::build(usage, None, rate_card, request_count, observed_at_unix_ms)
    }

    /// Settlement that consumes the declared containment table of the attempt's wire protocol.
    ///
    /// Contained dimensions (for example cached prompt tokens inside `prompt_tokens`) are priced
    /// once: on their own line when the card prices them, otherwise inside their container.  A
    /// self-hosted protocol can never show a zero external cost as a priced estimate, because a
    /// locally served model establishes no external price.
    #[allow(clippy::too_many_arguments)]
    pub fn from_usage_with_protocol(
        usage: &NormalizedUsage,
        protocol: ModelProtocol,
        rate_card: Option<&RateCard>,
        request_count: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::build(
            usage,
            Some(ProviderUsageContainment::declared(protocol)),
            rate_card,
            request_count,
            observed_at_unix_ms,
        )
    }

    fn build(
        usage: &NormalizedUsage,
        containment: Option<ProviderUsageContainment>,
        rate_card: Option<&RateCard>,
        request_count: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        usage.validate()?;
        if usage.provider_id.is_none() {
            return Err("provider_usage_provider_missing".to_owned());
        }
        if observed_at_unix_ms == 0 || request_count == 0 {
            return Err("provider_usage_observation_invalid".to_owned());
        }
        let provider_id = usage
            .provider_id
            .clone()
            .ok_or_else(|| "provider_usage_provider_missing".to_owned())?;
        let cost = match (usage.confidence, rate_card) {
            (UsageConfidence::Known, Some(card)) => {
                validate_rate_card_scope(
                    card,
                    &provider_id,
                    &usage.requested_model_id,
                    observed_at_unix_ms,
                )?;
                let estimate = match containment {
                    Some(containment) => {
                        card.estimate_with_containment(&usage.vector, request_count, &containment)?
                    }
                    None => card.estimate(&usage.vector, request_count)?,
                };
                match estimate.amount.clone() {
                    // A locally served model establishes token usage but no external price: a zero
                    // total must stay unknown instead of being presented as a free call, and any
                    // more specific unknown reason (an unpriced dimension) wins over the generic
                    // self-hosted one.
                    Some(amount)
                        if amount.micros == 0
                            && containment.is_some_and(|containment| {
                                ProviderUsageContainment::is_self_hosted(containment.protocol)
                            }) =>
                    {
                        SettlementCost::Unknown {
                            reason: estimate
                                .unknown_reason
                                .unwrap_or(BillingUnknownReason::Unsupported),
                        }
                    }
                    Some(_) => SettlementCost::Estimated { estimate },
                    None => SettlementCost::Unknown {
                        reason: estimate
                            .unknown_reason
                            .unwrap_or(BillingUnknownReason::RateCardMissing),
                    },
                }
            }
            (UsageConfidence::Known, None) => SettlementCost::Unknown {
                reason: BillingUnknownReason::RateCardMissing,
            },
            (_, _) => SettlementCost::Unknown {
                reason: usage
                    .unknown_reason
                    .unwrap_or(BillingUnknownReason::ProviderUnreported),
            },
        };
        let cost_is_known = !cost.is_unknown();
        let mut result = Self {
            schema: PROVIDER_USAGE_SETTLEMENT_SCHEMA.to_owned(),
            version: PROVIDER_USAGE_SETTLEMENT_VERSION,
            execution_id: None,
            usage_id: usage.usage_id,
            attempt_id: usage.attempt_id,
            run_id: usage.run_id,
            provider_id,
            requested_model_id: usage.requested_model_id.clone(),
            served_model_id: usage.served_model_id.clone(),
            route_id: usage.route_id.clone(),
            retry_ordinal: usage.retry_ordinal,
            usage: usage.clone(),
            cost,
            state: if cost_is_known {
                BillingState::Observed
            } else {
                BillingState::Unknown
            },
            idempotency_key: format!("usage:{}:{}", usage.attempt_id, usage.usage_digest),
            observed_at_unix_ms,
            settlement_digest: String::new(),
        };
        result.settlement_digest = result.digest();
        result.validate()?;
        Ok(result)
    }

    /// Bind a server-owned execution identity when the authoritative admission layer has one.
    /// Provider response data is never an input to this binding.
    pub fn from_usage_with_execution_id(
        usage: &NormalizedUsage,
        execution_id: crate::ExecutionId,
        rate_card: Option<&RateCard>,
        request_count: u64,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        if execution_id.as_uuid().is_nil() {
            return Err("provider_usage_execution_id_invalid".to_owned());
        }
        let mut result = Self::from_usage(usage, rate_card, request_count, observed_at_unix_ms)?;
        result.execution_id = Some(execution_id);
        result.settlement_digest = result.digest();
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_USAGE_SETTLEMENT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROVIDER_USAGE_SETTLEMENT_VERSION)
            || self.usage_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self
                .execution_id
                .is_some_and(|execution_id| execution_id.as_uuid().is_nil())
            || self.provider_id.trim().is_empty()
            || self.provider_id.len() > 256
            || self.requested_model_id.trim().is_empty()
            || self.requested_model_id.len() > 256
            || self
                .served_model_id
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 256)
            || self.route_id.trim().is_empty()
            || self.route_id.len() > 256
            || self.idempotency_key.trim().is_empty()
            || self.idempotency_key.len() > 512
            || self.observed_at_unix_ms == 0
            || self.settlement_digest != self.digest()
            || !valid_digest(&self.settlement_digest)
        {
            return Err("provider_usage_settlement_invalid".to_owned());
        }
        self.usage.validate()?;
        if self.usage.provider_id.as_deref() != Some(self.provider_id.as_str())
            || self.usage.attempt_id != self.attempt_id
            || self.usage.run_id != self.run_id
        {
            return Err("provider_usage_settlement_identity_mismatch".to_owned());
        }
        // A settlement is the terminal observation of one attempt.  A delta only reports the
        // increment since the previous observation, so billing it here would charge a fraction of
        // the attempt's consumption; it must be accumulated first.
        if matches!(self.usage.observation, UsageObservation::Delta) {
            return Err("provider_usage_delta_observation_not_settleable".to_owned());
        }
        self.cost.validate()?;
        if self.state == BillingState::Unknown && !self.cost.is_unknown() {
            return Err("provider_usage_unknown_cost_mismatch".to_owned());
        }
        if matches!(
            self.state,
            BillingState::Observed | BillingState::Settled | BillingState::Corrected
        ) && self.cost.is_unknown()
        {
            return Err("provider_usage_known_state_without_cost".to_owned());
        }
        Ok(())
    }

    pub fn attach_measured(
        &mut self,
        amount: Money,
        provider_receipt: ProviderReceiptRef,
    ) -> Result<(), String> {
        self.validate()?;
        if !self.cost.is_unknown() {
            return Err("provider_usage_cost_already_known".to_owned());
        }
        amount.validate()?;
        validate_provider_receipt(&provider_receipt)?;
        self.cost = SettlementCost::Measured {
            amount,
            provider_receipt,
        };
        self.state = BillingState::Settled;
        self.settlement_digest = self.digest();
        self.validate()
    }

    pub fn transition(&mut self, next: BillingState) -> Result<(), String> {
        self.validate()?;
        self.state.transition(next).map_err(str::to_owned)?;
        if next == BillingState::Settled && self.cost.is_unknown() {
            return Err("provider_usage_unknown_cannot_settle".to_owned());
        }
        if next == BillingState::Corrected && self.cost.is_unknown() {
            return Err("provider_usage_unknown_cannot_correct".to_owned());
        }
        if next == BillingState::Unknown && !self.cost.is_unknown() {
            return Err("provider_usage_known_cost_cannot_be_unknown".to_owned());
        }
        let mut candidate = self.clone();
        candidate.state = next;
        candidate.settlement_digest = candidate.digest();
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "execution_id": self.execution_id,
            "usage_id": self.usage_id,
            "attempt_id": self.attempt_id,
            "run_id": self.run_id,
            "provider_id": self.provider_id,
            "requested_model_id": self.requested_model_id,
            "served_model_id": self.served_model_id,
            "route_id": self.route_id,
            "retry_ordinal": self.retry_ordinal,
            "usage": self.usage,
            "cost": self.cost,
            "state": self.state,
            "idempotency_key": self.idempotency_key,
            "observed_at_unix_ms": self.observed_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettlementApplyOutcome {
    Applied,
    Duplicate,
}

#[derive(Default)]
pub struct ProviderUsageLedger {
    entries: BTreeMap<AttemptId, ProviderUsageSettlement>,
}

impl ProviderUsageLedger {
    pub fn apply(
        &mut self,
        settlement: ProviderUsageSettlement,
    ) -> Result<SettlementApplyOutcome, String> {
        settlement.validate()?;
        if let Some(existing) = self.entries.get(&settlement.attempt_id) {
            if existing.settlement_digest == settlement.settlement_digest {
                return Ok(SettlementApplyOutcome::Duplicate);
            }
            return Err("provider_usage_attempt_conflict".to_owned());
        }
        self.entries.insert(settlement.attempt_id, settlement);
        Ok(SettlementApplyOutcome::Applied)
    }

    pub fn get(&self, attempt_id: AttemptId) -> Option<&ProviderUsageSettlement> {
        self.entries.get(&attempt_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_provider_receipt(provider_receipt: &ProviderReceiptRef) -> Result<(), String> {
    if provider_receipt.as_str().trim().is_empty() {
        return Err("provider_receipt_missing".to_owned());
    }
    ProviderReceiptRef::new(provider_receipt.as_str().to_owned())
        .map(|_| ())
        .map_err(|_| "provider_receipt_invalid".to_owned())
}

/// Validate that a rate card is provider/model scoped and effective at the observation instant.
fn validate_rate_card_scope(
    card: &RateCard,
    provider_id: &str,
    requested_model_id: &str,
    observed_at_unix_ms: u64,
) -> Result<(), String> {
    if card.provider_id != provider_id
        || !card.is_effective_at(observed_at_unix_ms)
        || (card.model_selector != "*" && card.model_selector != requested_model_id)
    {
        return Err("provider_rate_card_scope_mismatch".to_owned());
    }
    Ok(())
}

/// Declared containment of the provider-reported usage dimensions for one wire protocol.
///
/// A protocol whose `prompt_tokens` already include the cached tokens (or whose
/// `cached_content_token_count` is inside `prompt_token_count`) must not be priced by adding the
/// cached dimension on top of the input dimension: the same tokens would be charged twice.
/// Anthropic reports cached tokens *outside* `input_tokens`, so those dimensions are disjoint.
///
/// Each field is a three-way declaration:
/// - `Some(true)` — the dimension is reported inside its container;
/// - `Some(false)` — the dimension is reported outside its container (additive);
/// - `None` — the wire shape declares nothing, so the same conservative overlap guard that
///   governs a protocol-less observation still applies to that dimension.
///
/// The table is the only proof of the relationship; it never replaces the guard, it can only
/// narrow it to the pairs the protocol actually documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsageContainment {
    pub protocol: ModelProtocol,
    pub cache_read_contained_in_input: Option<bool>,
    pub cache_write_contained_in_input: Option<bool>,
    pub reasoning_contained_in_output: Option<bool>,
}

/// Units attributed to each token price dimension after resolving contained dimensions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PricedUnits {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    pub reasoning: Option<u64>,
}

/// Net token units of one observation: every reported token is counted exactly once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChargeableUsage {
    pub input_units: u64,
    pub output_units: u64,
    pub cache_read_units: u64,
    pub cache_write_units: u64,
    pub reasoning_units: u64,
    pub audio_input_units: u64,
    pub audio_output_units: u64,
    pub total_units: u64,
}

impl ProviderUsageContainment {
    /// Declared containment table; the match is exhaustive so a new protocol must decide instead
    /// of silently inheriting another protocol's accounting rules.
    ///
    /// OpenAI (chat and responses) nests `cached_tokens` inside the prompt/input count and
    /// `reasoning_tokens` inside the completion/output count.  Gemini nests
    /// `cached_content_token_count` inside `prompt_token_count` but reports thinking tokens on top
    /// of the candidate count.  Anthropic reports cache read/creation *outside* `input_tokens`.
    /// Ollama and the legacy fake route report none of these dimensions, so nothing is declared
    /// for them and the conservative guard keeps applying if a value ever shows up.
    pub const fn declared(protocol: ModelProtocol) -> Self {
        match protocol {
            ModelProtocol::OpenAiChat | ModelProtocol::OpenAiResponses => Self {
                protocol,
                cache_read_contained_in_input: Some(true),
                cache_write_contained_in_input: None,
                reasoning_contained_in_output: Some(true),
            },
            ModelProtocol::GeminiInteractions => Self {
                protocol,
                cache_read_contained_in_input: Some(true),
                cache_write_contained_in_input: None,
                reasoning_contained_in_output: Some(false),
            },
            ModelProtocol::AnthropicMessages => Self {
                protocol,
                cache_read_contained_in_input: Some(false),
                cache_write_contained_in_input: Some(false),
                reasoning_contained_in_output: None,
            },
            ModelProtocol::OllamaChat | ModelProtocol::Legacy => Self {
                protocol,
                cache_read_contained_in_input: None,
                cache_write_contained_in_input: None,
                reasoning_contained_in_output: None,
            },
        }
    }

    /// True when the observation reports both a container and a dimension whose relationship the
    /// protocol leaves undeclared.
    ///
    /// Such tokens might already be inside the container, so the observation cannot be priced
    /// additively; the caller keeps the conservative `Unsupported` cost estimate instead.  This is
    /// the same guard a protocol-less observation gets, narrowed to the pairs this protocol does
    /// not declare.
    pub fn has_undeclared_overlap(&self, usage: &UsageVector) -> bool {
        let positive = |value: Option<u64>| value.is_some_and(|units| units > 0);
        (positive(usage.input_tokens)
            && ((self.cache_read_contained_in_input.is_none()
                && positive(usage.cache_read_tokens))
                || (self.cache_write_contained_in_input.is_none()
                    && positive(usage.cache_write_tokens))))
            || (positive(usage.output_tokens)
                && self.reasoning_contained_in_output.is_none()
                && positive(usage.reasoning_output_tokens))
    }

    /// A locally served model establishes token usage but no external monetary price.
    pub const fn is_self_hosted(protocol: ModelProtocol) -> bool {
        matches!(protocol, ModelProtocol::OllamaChat)
    }

    pub const fn schema(&self) -> &'static str {
        PROVIDER_USAGE_CONTAINMENT_SCHEMA
    }

    /// Attribute units to price dimensions, given which dimensions the card actually prices.
    ///
    /// A contained dimension that the card prices keeps its own units and is subtracted from its
    /// container, so the same token is charged once at each dimension's own rate.  A contained
    /// dimension the card does not price stays inside its container (units `Some(0)`, no separate
    /// charge, and no "unpriced dimension" unknown for tokens the container already covers).  A
    /// dimension that is not contained is priced as its own line, and an unpriced line keeps the
    /// estimate incomplete exactly as before.
    pub fn priced_units(
        &self,
        usage: &UsageVector,
        cache_read_priced: bool,
        cache_write_priced: bool,
        reasoning_priced: bool,
    ) -> Result<PricedUnits, String> {
        usage.validate()?;
        let contained = |declaration: Option<bool>| declaration == Some(true);
        let cache_read_separate =
            contained(self.cache_read_contained_in_input) && cache_read_priced;
        let cache_write_separate =
            contained(self.cache_write_contained_in_input) && cache_write_priced;
        let reasoning_separate = contained(self.reasoning_contained_in_output) && reasoning_priced;
        let input = subtract_contained(
            usage.input_tokens,
            &[
                cache_read_separate
                    .then_some(usage.cache_read_tokens)
                    .flatten(),
                cache_write_separate
                    .then_some(usage.cache_write_tokens)
                    .flatten(),
            ],
            "input_tokens",
        )?;
        let output = subtract_contained(
            usage.output_tokens,
            &[reasoning_separate
                .then_some(usage.reasoning_output_tokens)
                .flatten()],
            "output_tokens",
        )?;
        Ok(PricedUnits {
            input,
            output,
            cache_read: if contained(self.cache_read_contained_in_input) && !cache_read_separate {
                Some(0)
            } else {
                usage.cache_read_tokens
            },
            cache_write: if contained(self.cache_write_contained_in_input) && !cache_write_separate
            {
                Some(0)
            } else {
                usage.cache_write_tokens
            },
            reasoning: if contained(self.reasoning_contained_in_output) && !reasoning_separate {
                Some(0)
            } else {
                usage.reasoning_output_tokens
            },
        })
    }

    /// Net token units of one observation with every contained dimension counted once.
    ///
    /// A contained dimension is never counted twice.  An undeclared dimension is counted
    /// additively: for a token budget that is the conservative reading, because the alternative
    /// (silently dropping tokens that were not actually inside their container) would undercount
    /// consumption.  Cost stays a separate question — an undeclared overlap still yields
    /// `Unsupported` rather than a priced estimate.
    pub fn chargeable(&self, usage: &UsageVector) -> Result<ChargeableUsage, String> {
        let priced = self.priced_units(usage, true, true, true)?;
        let mut total = 0u64;
        for units in [
            priced.input,
            priced.output,
            priced.cache_read,
            priced.cache_write,
            priced.reasoning,
            usage.audio_input_tokens,
            usage.audio_output_tokens,
        ] {
            total = total
                .checked_add(units.unwrap_or(0))
                .ok_or_else(|| "provider_usage_overflow".to_owned())?;
        }
        Ok(ChargeableUsage {
            input_units: priced.input.unwrap_or(0),
            output_units: priced.output.unwrap_or(0),
            cache_read_units: priced.cache_read.unwrap_or(0),
            cache_write_units: priced.cache_write.unwrap_or(0),
            reasoning_units: priced.reasoning.unwrap_or(0),
            audio_input_units: usage.audio_input_tokens.unwrap_or(0),
            audio_output_units: usage.audio_output_tokens.unwrap_or(0),
            total_units: total,
        })
    }
}

/// Subtract contained dimensions from their container without ever wrapping silently.
///
/// A zero subtrahend needs no container: "0 extra tokens" is provable on its own.  A positive
/// subtrahend without a reported container is unprovable, and a subtrahend larger than the
/// container proves the two reported numbers cannot both be right.
fn subtract_contained(
    container: Option<u64>,
    subtrahends: &[Option<u64>],
    field: &str,
) -> Result<Option<u64>, String> {
    let mut remaining = container;
    for subtrahend in subtrahends.iter().flatten().filter(|units| **units > 0) {
        let container_units =
            remaining.ok_or_else(|| format!("provider_usage_containment_unprovable:{field}"))?;
        remaining = Some(
            container_units
                .checked_sub(*subtrahend)
                .ok_or_else(|| format!("provider_usage_containment_violated:{field}"))?,
        );
    }
    Ok(remaining)
}

/// Immutable copy of the rate-card identity and validity window used for one attempt.
///
/// The ledger keeps this so a Receipt can show currency, source, version and validity without
/// re-reading a mutable rate-card store, and so a later estimate can be checked against the card
/// that was actually selected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateCardSnapshot {
    pub schema: String,
    pub rate_card_id: RateCardId,
    pub card_version: u64,
    pub currency: String,
    pub source: String,
    pub effective_from_unix_ms: u64,
    pub effective_to_unix_ms: Option<u64>,
    pub rate_card_digest: String,
}

impl RateCardSnapshot {
    pub fn of(card: &RateCard) -> Self {
        Self {
            schema: PROVIDER_RATE_CARD_SNAPSHOT_SCHEMA.to_owned(),
            rate_card_id: card.rate_card_id,
            card_version: card.card_version,
            currency: card.currency.clone(),
            source: card.source.clone(),
            effective_from_unix_ms: card.effective_from_unix_ms,
            effective_to_unix_ms: card.effective_to_unix_ms,
            rate_card_digest: card.rate_card_digest.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_RATE_CARD_SNAPSHOT_SCHEMA
            || self.rate_card_id.as_uuid().is_nil()
            || self.card_version == 0
            || self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.source.trim().is_empty()
            || self.source.len() > 256
            || self.source.contains(['\0', '\r', '\n'])
            || !valid_digest(&self.rate_card_digest)
            || self
                .effective_to_unix_ms
                .is_some_and(|to| to <= self.effective_from_unix_ms)
        {
            return Err("provider_rate_card_snapshot_invalid".to_owned());
        }
        Ok(())
    }

    pub fn is_effective_at(&self, now_unix_ms: u64) -> bool {
        now_unix_ms >= self.effective_from_unix_ms
            && self.effective_to_unix_ms.is_none_or(|to| now_unix_ms < to)
    }
}

/// Late usage for an already settled attempt.
///
/// A correction never rewrites the original settlement fact; it is appended with the digest it
/// replaces, so the original observation and the correction stay independently auditable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsageCorrection {
    pub schema: String,
    pub version: SchemaVersion,
    pub attempt_id: AttemptId,
    pub run_id: crate::RunId,
    pub replaces_settlement_digest: String,
    pub usage: NormalizedUsage,
    pub cost: SettlementCost,
    pub reason: String,
    pub observed_at_unix_ms: u64,
    pub correction_digest: String,
}

impl ProviderUsageCorrection {
    pub fn new(
        usage: &NormalizedUsage,
        cost: SettlementCost,
        replaces_settlement_digest: impl Into<String>,
        reason: impl Into<String>,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut correction = Self {
            schema: PROVIDER_USAGE_CORRECTION_SCHEMA.to_owned(),
            version: PROVIDER_USAGE_SETTLEMENT_VERSION,
            attempt_id: usage.attempt_id,
            run_id: usage.run_id,
            replaces_settlement_digest: replaces_settlement_digest.into(),
            usage: usage.clone(),
            cost,
            reason: reason.into(),
            observed_at_unix_ms,
            correction_digest: String::new(),
        };
        correction.correction_digest = correction.digest();
        correction.validate()?;
        Ok(correction)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_USAGE_CORRECTION_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROVIDER_USAGE_SETTLEMENT_VERSION)
            || self.attempt_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || !valid_digest(&self.replaces_settlement_digest)
            || self.reason.trim().is_empty()
            || self.reason.len() > 256
            || self.reason.contains(['\0', '\r', '\n'])
            || self.observed_at_unix_ms == 0
            || !valid_digest(&self.correction_digest)
            || self.correction_digest != self.digest()
        {
            return Err("provider_usage_correction_invalid".to_owned());
        }
        self.usage.validate()?;
        if self.usage.attempt_id != self.attempt_id || self.usage.run_id != self.run_id {
            return Err("provider_usage_correction_identity_mismatch".to_owned());
        }
        // A correction is a terminal observation too: a delta must be accumulated before it can
        // replace (or explain) an already settled charge.
        if matches!(self.usage.observation, UsageObservation::Delta) {
            return Err("provider_usage_delta_observation_not_settleable".to_owned());
        }
        self.cost.validate()
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "attempt_id": self.attempt_id,
            "run_id": self.run_id,
            "replaces_settlement_digest": self.replaces_settlement_digest,
            "usage": self.usage,
            "cost": self.cost,
            "reason": self.reason,
            "observed_at_unix_ms": self.observed_at_unix_ms,
        }))
    }
}

/// Worst-case token hold recorded before a provider attempt is dispatched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAttemptReservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub attempt_id: AttemptId,
    pub run_id: crate::RunId,
    pub execution_id: Option<ExecutionId>,
    pub protocol: ModelProtocol,
    pub retry_ordinal: u32,
    pub reserved_tokens: u64,
    pub rate_card: Option<RateCardSnapshot>,
    pub reserved_at_unix_ms: u64,
    pub reservation_digest: String,
}

impl ProviderAttemptReservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attempt_id: AttemptId,
        run_id: crate::RunId,
        execution_id: Option<ExecutionId>,
        protocol: ModelProtocol,
        retry_ordinal: u32,
        reserved_tokens: u64,
        rate_card: Option<RateCardSnapshot>,
        reserved_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut reservation = Self {
            schema: PROVIDER_ATTEMPT_RESERVATION_SCHEMA.to_owned(),
            version: PROVIDER_USAGE_SETTLEMENT_VERSION,
            attempt_id,
            run_id,
            execution_id,
            protocol,
            retry_ordinal,
            reserved_tokens,
            rate_card,
            reserved_at_unix_ms,
            reservation_digest: String::new(),
        };
        reservation.reservation_digest = reservation.digest();
        reservation.validate()?;
        Ok(reservation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_ATTEMPT_RESERVATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROVIDER_USAGE_SETTLEMENT_VERSION)
            || self.attempt_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self
                .execution_id
                .is_some_and(|execution_id| execution_id.as_uuid().is_nil())
            || self.retry_ordinal > 1_000_000
            || self.reserved_tokens == 0
            || self.reserved_at_unix_ms == 0
            || !valid_digest(&self.reservation_digest)
            || self.reservation_digest != self.digest()
        {
            return Err("provider_usage_reservation_invalid".to_owned());
        }
        if let Some(rate_card) = &self.rate_card {
            rate_card.validate()?;
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "attempt_id": self.attempt_id,
            "run_id": self.run_id,
            "execution_id": self.execution_id,
            "protocol": self.protocol,
            "retry_ordinal": self.retry_ordinal,
            "reserved_tokens": self.reserved_tokens,
            "rate_card": self.rate_card,
            "reserved_at_unix_ms": self.reserved_at_unix_ms,
        }))
    }
}

/// What the ledger charges one attempt for.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderChargeBasis {
    /// The provider reported usable usage; the charge is derived from its net token units.
    KnownUsage,
    /// Consumption cannot be proven, so the reserved upper bound stays charged.
    ReservedUpperBound,
    /// The attempt provably never reached the provider and released its hold.
    Released,
}

/// One attempt's reservation, settlement and correction facts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderUsageLedgerEntry {
    pub attempt_id: AttemptId,
    pub run_id: crate::RunId,
    pub execution_id: Option<ExecutionId>,
    pub protocol: ModelProtocol,
    pub retry_ordinal: u32,
    pub state: BillingState,
    pub reserved_tokens: u64,
    pub charged_tokens: Option<u64>,
    pub charge_basis: ProviderChargeBasis,
    pub rate_card: Option<RateCardSnapshot>,
    pub settlement: Option<ProviderUsageSettlement>,
    pub correction: Option<ProviderUsageCorrection>,
    pub over_reservation: bool,
    pub unknown_cost_reason: Option<BillingUnknownReason>,
}

impl ProviderUsageLedgerEntry {
    /// A reservation still holds budget: it neither settled nor released yet.
    ///
    /// A settled attempt may still be in [`BillingState::Observed`] (its cost is an estimate, not a
    /// measurement), so the recorded settlement — not the state alone — decides whether the hold
    /// has been replaced by a charge.
    pub const fn is_open(&self) -> bool {
        matches!(self.state, BillingState::Reserved | BillingState::Observed)
            && self.settlement.is_none()
            && self.correction.is_none()
    }

    /// Conservative charge in token units; never zero for unproven consumption.
    pub fn conservative_charge(&self) -> u64 {
        match self.charge_basis {
            ProviderChargeBasis::KnownUsage => self.charged_tokens.unwrap_or(self.reserved_tokens),
            ProviderChargeBasis::ReservedUpperBound => self.reserved_tokens,
            ProviderChargeBasis::Released => 0,
        }
    }

    /// Cost to display, preferring the latest correction over the original settlement.
    pub fn effective_cost(&self) -> Option<&SettlementCost> {
        self.correction
            .as_ref()
            .map(|correction| &correction.cost)
            .or_else(|| self.settlement.as_ref().map(|settlement| &settlement.cost))
    }
}

/// Attempt-scoped ledger that ties one CP-11 budget ledger to provider usage settlements.
///
/// Reservation, dispatch, settlement and release are recorded separately, so a retry's added cost
/// stays explainable per attempt.  The ledger is a deterministic in-memory value boundary: the
/// real cross-process atomicity of a reservation still belongs to the ControlPlane event-log CAS
/// that owns the budget.  Its admission rule mirrors that ledger: the sum of settled charges, open
/// reserved holds and a new reservation must stay within `limit_tokens`.
pub struct ProviderUsageAttemptLedger {
    limit_tokens: u64,
    entries: BTreeMap<AttemptId, ProviderUsageLedgerEntry>,
}

impl ProviderUsageAttemptLedger {
    pub fn new(limit_tokens: u64) -> Result<Self, String> {
        if limit_tokens == 0 {
            return Err("provider_usage_budget_limit_invalid".to_owned());
        }
        Ok(Self {
            limit_tokens,
            entries: BTreeMap::new(),
        })
    }

    pub const fn schema(&self) -> &'static str {
        PROVIDER_USAGE_ATTEMPT_LEDGER_SCHEMA
    }

    pub const fn limit_tokens(&self) -> u64 {
        self.limit_tokens
    }

    /// Admit one attempt's worst-case hold, or reject it before any provider call.
    ///
    /// The check uses checked arithmetic over settled charges, open holds and the new hold, so two
    /// calls admitted against the same stale balance cannot both pass and overspend the limit.
    pub fn reserve(&mut self, reservation: ProviderAttemptReservation) -> Result<(), String> {
        reservation.validate()?;
        if self.entries.contains_key(&reservation.attempt_id) {
            return Err("provider_usage_attempt_conflict".to_owned());
        }
        let committed = self.charged_tokens()?;
        let open = self.open_reserved_tokens()?;
        let admitted = committed
            .checked_add(open)
            .and_then(|total| total.checked_add(reservation.reserved_tokens))
            .ok_or_else(|| "provider_usage_overflow".to_owned())?;
        if admitted > self.limit_tokens {
            return Err("provider_usage_budget_exceeded".to_owned());
        }
        self.entries.insert(
            reservation.attempt_id,
            ProviderUsageLedgerEntry {
                attempt_id: reservation.attempt_id,
                run_id: reservation.run_id,
                execution_id: reservation.execution_id,
                protocol: reservation.protocol,
                retry_ordinal: reservation.retry_ordinal,
                state: BillingState::Reserved,
                reserved_tokens: reservation.reserved_tokens,
                charged_tokens: None,
                charge_basis: ProviderChargeBasis::ReservedUpperBound,
                rate_card: reservation.rate_card,
                settlement: None,
                correction: None,
                over_reservation: false,
                unknown_cost_reason: None,
            },
        );
        Ok(())
    }

    /// Mark an admitted attempt as dispatched (the permit was consumed).
    pub fn dispatch(&mut self, attempt_id: AttemptId) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&attempt_id)
            .ok_or_else(|| "provider_usage_attempt_unknown".to_owned())?;
        match entry.state {
            BillingState::Reserved => {
                entry.state = BillingState::Observed;
                Ok(())
            }
            BillingState::Observed if entry.settlement.is_some() => {
                Err("provider_usage_attempt_state_invalid".to_owned())
            }
            BillingState::Observed => Err("billing_state_duplicate".to_owned()),
            BillingState::Released => {
                Err("provider_usage_released_attempt_cannot_dispatch".to_owned())
            }
            _ => Err("provider_usage_attempt_state_invalid".to_owned()),
        }
    }

    /// Record the provider observation for one attempt exactly once.
    ///
    /// The attempt must have been dispatched first: a hold that never reached the provider has no
    /// observation to record and must be released instead.  An exact replay returns
    /// [`SettlementApplyOutcome::Duplicate`] without charging twice; a different settlement for the
    /// same attempt is a conflict.  Unknown usage keeps the reserved upper bound charged, and a
    /// charge above the hold is committed before the error is returned, mirroring the ControlPlane
    /// rule that an overspend fact is never dropped.
    pub fn settle(
        &mut self,
        settlement: ProviderUsageSettlement,
    ) -> Result<SettlementApplyOutcome, String> {
        settlement.validate()?;
        if !matches!(
            settlement.state,
            BillingState::Observed | BillingState::Settled | BillingState::Unknown
        ) {
            return Err("provider_usage_settlement_state_invalid".to_owned());
        }
        let entry = self
            .entries
            .get_mut(&settlement.attempt_id)
            .ok_or_else(|| "provider_usage_settlement_orphan".to_owned())?;
        if entry.run_id != settlement.run_id {
            return Err("provider_usage_settlement_identity_mismatch".to_owned());
        }
        if entry
            .execution_id
            .is_some_and(|execution_id| settlement.execution_id.as_ref() != Some(&execution_id))
        {
            return Err("provider_usage_execution_id_mismatch".to_owned());
        }
        if let Some(existing) = &entry.settlement {
            if existing.settlement_digest == settlement.settlement_digest {
                return Ok(SettlementApplyOutcome::Duplicate);
            }
            return Err("provider_usage_attempt_conflict".to_owned());
        }
        match entry.state {
            BillingState::Observed => {}
            BillingState::Reserved => {
                return Err("provider_usage_attempt_not_dispatched".to_owned())
            }
            BillingState::Released => {
                return Err("provider_usage_released_attempt_cannot_settle".to_owned())
            }
            _ => return Err("provider_usage_attempt_state_invalid".to_owned()),
        }
        if let (Some(snapshot), SettlementCost::Estimated { estimate }) =
            (&entry.rate_card, &settlement.cost)
        {
            if snapshot.rate_card_id != estimate.rate_card_id
                || snapshot.card_version != estimate.rate_card_version
            {
                return Err("provider_usage_rate_card_mismatch".to_owned());
            }
        }
        let containment = ProviderUsageContainment::declared(entry.protocol);
        let (charged_tokens, charge_basis) = match settlement.usage.confidence {
            UsageConfidence::Known => (
                Some(
                    containment
                        .chargeable(&settlement.usage.vector)?
                        .total_units,
                ),
                ProviderChargeBasis::KnownUsage,
            ),
            _ => (None, ProviderChargeBasis::ReservedUpperBound),
        };
        entry.state = settlement.state;
        entry.charged_tokens = charged_tokens;
        entry.charge_basis = charge_basis;
        entry.unknown_cost_reason = settlement.cost.unknown_reason();
        entry.over_reservation =
            charged_tokens.is_some_and(|charged_tokens| charged_tokens > entry.reserved_tokens);
        entry.settlement = Some(settlement);
        if entry.over_reservation {
            return Err("provider_usage_charge_exceeds_reservation".to_owned());
        }
        Ok(SettlementApplyOutcome::Applied)
    }

    /// Release a hold for an attempt that provably never reached the provider.
    ///
    /// A dispatched attempt can never be released: neither a known charge nor a cancellation can
    /// prove that no consumption happened, so its hold stays conservative until it settles.
    pub fn release(&mut self, attempt_id: AttemptId) -> Result<(), String> {
        let entry = self
            .entries
            .get_mut(&attempt_id)
            .ok_or_else(|| "provider_usage_attempt_unknown".to_owned())?;
        match entry.state {
            BillingState::Reserved => {
                entry.state = BillingState::Released;
                entry.charged_tokens = Some(0);
                entry.charge_basis = ProviderChargeBasis::Released;
                entry.unknown_cost_reason = None;
                entry.over_reservation = false;
                Ok(())
            }
            BillingState::Observed => {
                Err("provider_usage_dispatched_attempt_cannot_release".to_owned())
            }
            BillingState::Settled | BillingState::Unknown => {
                Err("provider_usage_settled_attempt_cannot_release".to_owned())
            }
            _ => Err("provider_usage_attempt_state_invalid".to_owned()),
        }
    }

    /// Apply late usage as a correction to an already settled attempt.
    pub fn correct(
        &mut self,
        correction: ProviderUsageCorrection,
    ) -> Result<SettlementApplyOutcome, String> {
        correction.validate()?;
        let entry = self
            .entries
            .get_mut(&correction.attempt_id)
            .ok_or_else(|| "provider_usage_correction_orphan".to_owned())?;
        if entry.run_id != correction.run_id {
            return Err("provider_usage_settlement_identity_mismatch".to_owned());
        }
        let settlement = entry
            .settlement
            .as_ref()
            .ok_or_else(|| "provider_usage_correction_orphan".to_owned())?;
        if settlement.settlement_digest != correction.replaces_settlement_digest {
            return Err("provider_usage_correction_target_mismatch".to_owned());
        }
        if let Some(existing) = &entry.correction {
            if existing.correction_digest == correction.correction_digest {
                return Ok(SettlementApplyOutcome::Duplicate);
            }
            return Err("provider_usage_correction_conflict".to_owned());
        }
        let containment = ProviderUsageContainment::declared(entry.protocol);
        let (charged_tokens, charge_basis) = match correction.usage.confidence {
            UsageConfidence::Known => (
                Some(
                    containment
                        .chargeable(&correction.usage.vector)?
                        .total_units,
                ),
                ProviderChargeBasis::KnownUsage,
            ),
            _ => (None, ProviderChargeBasis::ReservedUpperBound),
        };
        entry.state = BillingState::Corrected;
        entry.charged_tokens = charged_tokens;
        entry.charge_basis = charge_basis;
        entry.unknown_cost_reason = correction.cost.unknown_reason();
        entry.over_reservation =
            charged_tokens.is_some_and(|charged_tokens| charged_tokens > entry.reserved_tokens);
        entry.correction = Some(correction);
        if entry.over_reservation {
            return Err("provider_usage_charge_exceeds_reservation".to_owned());
        }
        Ok(SettlementApplyOutcome::Applied)
    }

    pub fn entry(&self, attempt_id: AttemptId) -> Option<&ProviderUsageLedgerEntry> {
        self.entries.get(&attempt_id)
    }

    pub fn entries(&self) -> impl Iterator<Item = &ProviderUsageLedgerEntry> {
        self.entries.values()
    }

    /// Charges committed by attempts that no longer hold budget.
    ///
    /// Settled, released and corrected attempts count here, with unproven consumption charged at the
    /// reserved upper bound; open holds are reported separately by [`Self::open_reserved_tokens`],
    /// so one token is never counted in both totals.
    pub fn charged_tokens(&self) -> Result<u64, String> {
        self.entries
            .values()
            .filter(|entry| !entry.is_open())
            .try_fold(0u64, |sum, entry| {
                sum.checked_add(entry.conservative_charge())
                    .ok_or_else(|| "provider_usage_overflow".to_owned())
            })
    }

    /// Worst-case holds of attempts that are neither settled nor released.
    pub fn open_reserved_tokens(&self) -> Result<u64, String> {
        self.entries
            .values()
            .filter(|entry| entry.is_open())
            .try_fold(0u64, |sum, entry| {
                sum.checked_add(entry.reserved_tokens)
                    .ok_or_else(|| "provider_usage_overflow".to_owned())
            })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
