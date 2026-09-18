//! Pre-effect admission upper bounds for model/tool/resource work.

use crate::{json_digest, CostEstimate, RateCard, RateCardId, SchemaVersion, UsageVector};
use serde::{Deserialize, Serialize};

pub const ADMISSION_ESTIMATE_SCHEMA: &str = "kiana.admission-estimate.v1";
pub const ADMISSION_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenEstimateBasis {
    ExactTokenizer,
    BytesUpperBound,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionEstimateInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub wire_request_digest: String,
    pub wire_request_bytes: u64,
    pub token_basis: TokenEstimateBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_input_tokens: Option<u64>,
    pub max_output_tokens: u64,
    pub retry_allowance: u32,
    pub tool_calls_upper: u64,
    pub effects_upper: u64,
    pub storage_bytes_upper: u64,
    pub cost_hard_limit_enforced: bool,
}

impl AdmissionEstimateInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ADMISSION_ESTIMATE_SCHEMA
            || !self.version.is_compatible_with(&ADMISSION_SCHEMA_VERSION)
            || !valid_digest(&self.wire_request_digest)
            || self.wire_request_bytes == 0
            || self.max_output_tokens == 0
            || self.retry_allowance > 32
            || self.exact_input_tokens.is_some_and(|tokens| tokens == 0)
            || matches!(self.token_basis, TokenEstimateBasis::ExactTokenizer)
                && self.exact_input_tokens.is_none()
            || self.cost_hard_limit_enforced
                && matches!(self.token_basis, TokenEstimateBasis::Unknown)
        {
            return Err("admission_estimate_input_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionEstimate {
    pub schema: String,
    pub version: SchemaVersion,
    pub wire_request_digest: String,
    pub requests_upper: u64,
    pub input_tokens_upper: Option<u64>,
    pub output_tokens_upper: u64,
    pub total_tokens_upper: Option<u64>,
    pub tool_calls_upper: u64,
    pub effects_upper: u64,
    pub storage_bytes_upper: u64,
    pub token_estimate_exact: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_id: Option<RateCardId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_card_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost: Option<CostEstimate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_cost_reason: Option<String>,
    pub estimate_digest: String,
}

pub fn estimate_admission(
    input: &AdmissionEstimateInput,
    rate_card: Option<&RateCard>,
) -> Result<AdmissionEstimate, String> {
    input.validate()?;
    if let Some(card) = rate_card {
        card.validate()?;
    }
    let requests_upper = u64::from(input.retry_allowance)
        .checked_add(1)
        .ok_or_else(|| "admission_estimate_overflow:requests".to_owned())?;
    let (input_tokens_upper, token_estimate_exact) = match input.token_basis {
        TokenEstimateBasis::ExactTokenizer => (input.exact_input_tokens, true),
        TokenEstimateBasis::BytesUpperBound => (
            Some(
                input
                    .wire_request_bytes
                    .checked_add(3)
                    .ok_or_else(|| "admission_estimate_overflow:bytes".to_owned())?
                    / 4,
            ),
            false,
        ),
        TokenEstimateBasis::Unknown => (None, false),
    };
    let total_tokens_upper =
        input_tokens_upper.and_then(|tokens| tokens.checked_add(input.max_output_tokens));
    if input_tokens_upper.is_some() && total_tokens_upper.is_none() {
        return Err("admission_estimate_overflow:tokens".to_owned());
    }
    let (estimated_cost, unknown_cost_reason) = if let Some(card) = rate_card {
        let mut vector = UsageVector::zero();
        vector.input_tokens =
            input_tokens_upper.and_then(|tokens| tokens.checked_mul(requests_upper));
        vector.output_tokens = input.max_output_tokens.checked_mul(requests_upper);
        vector.tool_calls = input.tool_calls_upper;
        vector.effect_count = input.effects_upper;
        let estimate = card.estimate(&vector, requests_upper)?;
        if estimate.amount.is_some() {
            (Some(estimate), None)
        } else {
            (
                None,
                estimate
                    .unknown_reason
                    .map(|reason| reason.as_str().to_owned()),
            )
        }
    } else {
        (None, Some("rate_card_missing".to_owned()))
    };
    if input.cost_hard_limit_enforced && estimated_cost.is_none() {
        return Err("cost_hard_limit_requires_known_price".to_owned());
    }
    let mut result = AdmissionEstimate {
        schema: ADMISSION_ESTIMATE_SCHEMA.to_owned(),
        version: ADMISSION_SCHEMA_VERSION,
        wire_request_digest: input.wire_request_digest.clone(),
        requests_upper,
        input_tokens_upper,
        output_tokens_upper: input.max_output_tokens,
        total_tokens_upper,
        tool_calls_upper: input.tool_calls_upper,
        effects_upper: input.effects_upper,
        storage_bytes_upper: input.storage_bytes_upper,
        token_estimate_exact,
        rate_card_id: rate_card.map(|card| card.rate_card_id),
        rate_card_version: rate_card.map(|card| card.card_version),
        estimated_cost,
        unknown_cost_reason,
        estimate_digest: String::new(),
    };
    result.estimate_digest = result.digest();
    result.validate()?;
    Ok(result)
}

impl AdmissionEstimate {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ADMISSION_ESTIMATE_SCHEMA
            || !self.version.is_compatible_with(&ADMISSION_SCHEMA_VERSION)
            || !valid_digest(&self.wire_request_digest)
            || self.requests_upper == 0
            || self.output_tokens_upper == 0
            || (self.estimated_cost.is_some() && self.unknown_cost_reason.is_some())
            || (self.estimated_cost.is_none() && self.unknown_cost_reason.is_none())
            || !valid_digest(&self.estimate_digest)
            || self.estimate_digest != self.digest()
        {
            return Err("admission_estimate_invalid".to_owned());
        }
        if let Some(cost) = &self.estimated_cost {
            cost.validate()?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "wire_request_digest": self.wire_request_digest,
            "requests_upper": self.requests_upper,
            "input_tokens_upper": self.input_tokens_upper,
            "output_tokens_upper": self.output_tokens_upper,
            "total_tokens_upper": self.total_tokens_upper,
            "tool_calls_upper": self.tool_calls_upper,
            "effects_upper": self.effects_upper,
            "storage_bytes_upper": self.storage_bytes_upper,
            "token_estimate_exact": self.token_estimate_exact,
            "rate_card_id": self.rate_card_id,
            "rate_card_version": self.rate_card_version,
            "estimated_cost": self.estimated_cost,
            "unknown_cost_reason": self.unknown_cost_reason,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}
