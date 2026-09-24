//! Provider usage, price snapshot and settlement contracts.
//!
//! This module is deliberately a value-level boundary.  It normalizes the usage that a provider
//! reported, pins the rate-card version used for an estimate and keeps unknown/measured cost
//! distinct.  It does not reserve a capability, write an EventLog or authorize a retry.

use crate::{
    json_digest, AttemptId, BillingState, BillingUnknownReason, CostEstimate, ExecutionId, Money,
    NormalizedUsage, ProviderReceiptRef, RateCard, SchemaVersion, UsageConfidence, UsageId,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const PROVIDER_USAGE_SETTLEMENT_SCHEMA: &str = "kiana.provider-usage-settlement.v1";
pub const PROVIDER_USAGE_SETTLEMENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SettlementCost {
    Unknown { reason: BillingUnknownReason },
    Estimated { estimate: CostEstimate },
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
                if provider_receipt.as_str().trim().is_empty() {
                    return Err("provider_receipt_missing".to_owned());
                }
                Ok(())
            }
        }
    }

    fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
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
    #[allow(clippy::too_many_arguments)]
    pub fn from_usage(
        usage: &NormalizedUsage,
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
                if card.provider_id != provider_id
                    || !card.is_effective_at(observed_at_unix_ms)
                    || (card.model_selector != "*"
                        && card.model_selector != usage.requested_model_id)
                {
                    return Err("provider_rate_card_scope_mismatch".to_owned());
                }
                match card.estimate(&usage.vector, request_count)? {
                    estimate if estimate.amount.is_some() => SettlementCost::Estimated { estimate },
                    estimate => SettlementCost::Unknown {
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
            state: if usage.confidence == UsageConfidence::Known {
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

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_USAGE_SETTLEMENT_SCHEMA
            || !self
                .version
                .is_compatible_with(&PROVIDER_USAGE_SETTLEMENT_VERSION)
            || self.usage_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
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
        provider_receipt.as_str();
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
        self.state = next;
        self.settlement_digest = self.digest();
        self.validate()
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
