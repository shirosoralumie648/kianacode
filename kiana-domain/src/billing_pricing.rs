//! Integer-micros money and versioned rate-card arithmetic.
//!
//! Pricing is an estimate unless a later provider receipt explicitly supplies measured cost. This
//! module never authorizes execution or treats an unknown price/usage dimension as zero.

use crate::{json_digest, BillingUnknownReason, RateCardId, SchemaVersion, UsageVector};
use serde::{Deserialize, Serialize};

pub const MONEY_SCHEMA: &str = "kiana.money.v1";
pub const RATE_CARD_SCHEMA: &str = "kiana.rate-card.v1";
pub const COST_ESTIMATE_SCHEMA: &str = "kiana.cost-estimate.v1";
pub const PRICING_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn currency_valid(currency: &str) -> bool {
    currency.len() == 3 && currency.bytes().all(|byte| byte.is_ascii_uppercase())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Money {
    pub schema: String,
    pub currency: String,
    pub micros: i128,
}

impl Money {
    pub fn new(currency: impl Into<String>, micros: i128) -> Result<Self, String> {
        let money = Self {
            schema: MONEY_SCHEMA.to_owned(),
            currency: currency.into(),
            micros,
        };
        money.validate()?;
        Ok(money)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MONEY_SCHEMA || !currency_valid(&self.currency) {
            return Err("money_invalid".to_owned());
        }
        Ok(())
    }

    pub fn checked_add(&self, other: &Self) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        if self.currency != other.currency {
            return Err("money_currency_mismatch".to_owned());
        }
        Self::new(
            self.currency.clone(),
            self.micros
                .checked_add(other.micros)
                .ok_or_else(|| "money_arithmetic_overflow".to_owned())?,
        )
    }

    pub fn checked_mul_units(&self, units: u64) -> Result<Self, String> {
        self.validate()?;
        Self::new(
            self.currency.clone(),
            self.micros
                .checked_mul(i128::from(units))
                .ok_or_else(|| "money_arithmetic_overflow".to_owned())?,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateCard {
    pub schema: String,
    pub version: SchemaVersion,
    pub rate_card_id: RateCardId,
    pub provider_id: String,
    pub model_selector: String,
    pub currency: String,
    pub input_price_per_unit: Option<u64>,
    pub output_price_per_unit: Option<u64>,
    pub cache_read_price_per_unit: Option<u64>,
    pub cache_write_price_per_unit: Option<u64>,
    pub reasoning_price_per_unit: Option<u64>,
    pub request_price: Option<u64>,
    pub tool_price: Option<u64>,
    pub effective_from_unix_ms: u64,
    pub effective_to_unix_ms: Option<u64>,
    pub card_version: u64,
    pub source: String,
    pub rate_card_digest: String,
}

impl RateCard {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rate_card_id: RateCardId,
        provider_id: impl Into<String>,
        model_selector: impl Into<String>,
        currency: impl Into<String>,
        input_price_per_unit: Option<u64>,
        output_price_per_unit: Option<u64>,
        effective_from_unix_ms: u64,
        effective_to_unix_ms: Option<u64>,
        card_version: u64,
        source: impl Into<String>,
    ) -> Result<Self, String> {
        let mut card = Self {
            schema: RATE_CARD_SCHEMA.to_owned(),
            version: PRICING_SCHEMA_VERSION,
            rate_card_id,
            provider_id: provider_id.into(),
            model_selector: model_selector.into(),
            currency: currency.into(),
            input_price_per_unit,
            output_price_per_unit,
            cache_read_price_per_unit: None,
            cache_write_price_per_unit: None,
            reasoning_price_per_unit: None,
            request_price: None,
            tool_price: None,
            effective_from_unix_ms,
            effective_to_unix_ms,
            card_version,
            source: source.into(),
            rate_card_digest: String::new(),
        };
        card.rate_card_digest = card.digest();
        card.validate()?;
        Ok(card)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RATE_CARD_SCHEMA
            || !self.version.is_compatible_with(&PRICING_SCHEMA_VERSION)
            || self.rate_card_id.as_uuid().is_nil()
            || self.provider_id.trim().is_empty()
            || self.provider_id.len() > 256
            || self.model_selector.trim().is_empty()
            || self.model_selector.len() > 256
            || !currency_valid(&self.currency)
            || self.effective_from_unix_ms == 0
            || self
                .effective_to_unix_ms
                .is_some_and(|to| to <= self.effective_from_unix_ms)
            || self.card_version == 0
            || self.source.trim().is_empty()
            || self.source.len() > 512
            || [
                self.input_price_per_unit,
                self.output_price_per_unit,
                self.cache_read_price_per_unit,
                self.cache_write_price_per_unit,
                self.reasoning_price_per_unit,
                self.request_price,
                self.tool_price,
            ]
            .iter()
            .all(Option::is_none)
            || self.rate_card_digest != self.digest()
            || !valid_digest(&self.rate_card_digest)
        {
            return Err("rate_card_invalid".to_owned());
        }
        Ok(())
    }

    pub fn is_effective_at(&self, now_unix_ms: u64) -> bool {
        now_unix_ms >= self.effective_from_unix_ms
            && self.effective_to_unix_ms.is_none_or(|to| now_unix_ms < to)
    }

    pub fn estimate(
        &self,
        usage: &UsageVector,
        request_count: u64,
    ) -> Result<CostEstimate, String> {
        self.validate()?;
        usage.validate()?;
        let mut total = Money::new(self.currency.clone(), 0)?;
        let mut unknown = None;
        total = add_dimension(
            total,
            usage.input_tokens,
            self.input_price_per_unit,
            &self.currency,
            "input_tokens",
            &mut unknown,
        )?;
        total = add_dimension(
            total,
            usage.output_tokens,
            self.output_price_per_unit,
            &self.currency,
            "output_tokens",
            &mut unknown,
        )?;
        total = add_dimension(
            total,
            usage.cache_read_tokens,
            self.cache_read_price_per_unit,
            &self.currency,
            "cache_read_tokens",
            &mut unknown,
        )?;
        total = add_dimension(
            total,
            usage.cache_write_tokens,
            self.cache_write_price_per_unit,
            &self.currency,
            "cache_write_tokens",
            &mut unknown,
        )?;
        total = add_dimension(
            total,
            usage.reasoning_output_tokens,
            self.reasoning_price_per_unit,
            &self.currency,
            "reasoning_output_tokens",
            &mut unknown,
        )?;
        total = add_dimension(
            total,
            Some(request_count),
            self.request_price,
            &self.currency,
            "requests",
            &mut unknown,
        )?;
        total = add_dimension(
            total,
            Some(usage.tool_calls),
            self.tool_price,
            &self.currency,
            "tool_calls",
            &mut unknown,
        )?;
        CostEstimate::new(self.rate_card_id, total, unknown)
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "rate_card_id": self.rate_card_id,
            "provider_id": self.provider_id,
            "model_selector": self.model_selector,
            "currency": self.currency,
            "input_price_per_unit": self.input_price_per_unit,
            "output_price_per_unit": self.output_price_per_unit,
            "cache_read_price_per_unit": self.cache_read_price_per_unit,
            "cache_write_price_per_unit": self.cache_write_price_per_unit,
            "reasoning_price_per_unit": self.reasoning_price_per_unit,
            "request_price": self.request_price,
            "tool_price": self.tool_price,
            "effective_from_unix_ms": self.effective_from_unix_ms,
            "effective_to_unix_ms": self.effective_to_unix_ms,
            "card_version": self.card_version,
            "source": self.source,
        }))
    }
}

fn add_dimension(
    total: Money,
    units: Option<u64>,
    price: Option<u64>,
    currency: &str,
    _dimension: &str,
    unknown: &mut Option<BillingUnknownReason>,
) -> Result<Money, String> {
    match (units, price) {
        (Some(units), Some(price)) => total
            .checked_add(&Money::new(
                currency.to_owned(),
                i128::from(price)
                    .checked_mul(i128::from(units))
                    .ok_or_else(|| "pricing_arithmetic_overflow".to_owned())?,
            )?)
            .map_err(|_| "pricing_arithmetic_overflow".to_owned()),
        (Some(_), None) => {
            *unknown = Some(BillingUnknownReason::RateCardMissing);
            Ok(total)
        }
        (None, Some(_)) => {
            *unknown = Some(BillingUnknownReason::Partial);
            Ok(total)
        }
        (None, None) => Ok(total),
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostEstimate {
    pub schema: String,
    pub rate_card_id: RateCardId,
    pub amount: Option<Money>,
    pub unknown_reason: Option<BillingUnknownReason>,
    pub estimate_digest: String,
}

impl CostEstimate {
    fn new(
        rate_card_id: RateCardId,
        amount: Money,
        unknown_reason: Option<BillingUnknownReason>,
    ) -> Result<Self, String> {
        let mut estimate = Self {
            schema: COST_ESTIMATE_SCHEMA.to_owned(),
            rate_card_id,
            amount: unknown_reason.is_none().then_some(amount),
            unknown_reason,
            estimate_digest: String::new(),
        };
        estimate.estimate_digest = estimate.digest();
        Ok(estimate)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COST_ESTIMATE_SCHEMA
            || self.rate_card_id.as_uuid().is_nil()
            || (self.amount.is_some() && self.unknown_reason.is_some())
            || (self.amount.is_none() && self.unknown_reason.is_none())
            || !valid_digest(&self.estimate_digest)
            || self.estimate_digest != self.digest()
        {
            return Err("cost_estimate_invalid".to_owned());
        }
        if let Some(amount) = &self.amount {
            amount.validate()?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "rate_card_id": self.rate_card_id,
            "amount": self.amount,
            "unknown_reason": self.unknown_reason,
        }))
    }
}
