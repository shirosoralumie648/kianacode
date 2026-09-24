//! Cache decisions are projections, never business facts.

use kiana_domain::{json_digest, DataPayloadState, DataPropagationPlan, DataPropagationTarget};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONTEXT_CACHE_DECISION_SCHEMA: &str = "kiana.context-cache-decision.v1";

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextCacheStatus {
    Hit,
    Miss,
    Stale,
    Degraded,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextCacheDecision {
    pub schema: String,
    pub status: ContextCacheStatus,
    pub reason: String,
    pub cache_read_used: bool,
    pub business_result_from_cache: bool,
    pub decision_digest: String,
}

impl ContextCacheDecision {
    pub fn from_report(
        status: &str,
        reused: usize,
        added: usize,
        changed: usize,
        removed: usize,
    ) -> Self {
        let (status, reason, cache_read_used, business_result_from_cache) = match status {
            "created" => (ContextCacheStatus::Miss, "cache_missing", false, false),
            "recovered" => (
                ContextCacheStatus::Degraded,
                "cache_invalid_rebuilt",
                false,
                false,
            ),
            "updated" if added == 0 && changed == 0 && removed == 0 && reused > 0 => {
                (ContextCacheStatus::Hit, "cache_reused", true, true)
            }
            "updated" => (
                ContextCacheStatus::Stale,
                "cache_source_changed",
                false,
                false,
            ),
            _ => (
                ContextCacheStatus::Degraded,
                "cache_status_unknown",
                false,
                false,
            ),
        };
        let mut decision = Self {
            schema: CONTEXT_CACHE_DECISION_SCHEMA.to_owned(),
            status,
            reason: reason.to_owned(),
            cache_read_used,
            business_result_from_cache,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONTEXT_CACHE_DECISION_SCHEMA || self.reason.trim().is_empty() {
            return Err("context_cache_decision_invalid".to_owned());
        }
        if self.business_result_from_cache != self.cache_read_used
            || (self.status != ContextCacheStatus::Hit && self.business_result_from_cache)
        {
            return Err("context_cache_business_result_boundary_invalid".to_owned());
        }
        if self
            .decision_digest
            .strip_prefix("sha256:")
            .is_none_or(|hex| hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        {
            return Err("context_cache_decision_digest_invalid".to_owned());
        }
        if self.decision_digest != self.digest() {
            return Err("context_cache_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "status": self.status,
            "reason": self.reason,
            "cache_read_used": self.cache_read_used,
            "business_result_from_cache": self.business_result_from_cache,
        }))
    }

    /// Governance revocation/expiry invalidates a cache before it can be used as a business
    /// result. A cache miss/rebuild remains a query concern and cannot authorize stale payloads.
    pub fn from_data_propagation(
        plan: &DataPropagationPlan,
        current_data_epoch: u64,
        payload_state: DataPayloadState,
    ) -> Result<Self, String> {
        plan.validate()?;
        let target = plan
            .targets
            .iter()
            .find(|receipt| receipt.target == DataPropagationTarget::Cache)
            .ok_or_else(|| "cache_propagation_target_missing".to_owned())?;
        let valid = current_data_epoch >= plan.data_epoch
            && target.state == kiana_domain::DataPropagationState::PreservedMetadata
            && payload_state == DataPayloadState::Available;
        let mut decision = Self {
            schema: CONTEXT_CACHE_DECISION_SCHEMA.to_owned(),
            status: if valid {
                ContextCacheStatus::Hit
            } else {
                ContextCacheStatus::Stale
            },
            reason: if valid {
                "cache_governance_current".to_owned()
            } else {
                "cache_governance_invalidated".to_owned()
            },
            cache_read_used: valid,
            business_result_from_cache: valid,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }
}
