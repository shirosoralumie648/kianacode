//! Shared budget reservation/settlement facts for model and capability work.
//!
//! These values are intentionally inert: they carry the bounded accounting decision and its
//! idempotency key, while the ControlPlane/CellRegistry remain the only components that can
//! commit or consume a reservation.

use crate::{json_digest, BudgetLeaseId, ExecutionId, RequestId, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const BUDGET_RESERVATION_SCHEMA: &str = "kiana.budget-reservation-fact.v1";
pub const BUDGET_SETTLEMENT_SCHEMA: &str = "kiana.budget-settlement-fact.v1";
pub const BUDGET_FACT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetScope {
    Turn,
    Run,
    Project,
    Parent,
    Child,
}

impl BudgetScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Run => "run",
            Self::Project => "project",
            Self::Parent => "parent",
            Self::Child => "child",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetReservationFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub reservation_id: RequestId,
    pub execution_id: ExecutionId,
    pub run_id: RunId,
    pub budget_lease_id: BudgetLeaseId,
    pub scope: BudgetScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_reservation_id: Option<RequestId>,
    pub model_calls: u64,
    pub tool_calls: u64,
    pub tokens: u64,
    pub effects: u32,
    pub max_output_tokens: u64,
    pub authority_epoch: u64,
    pub expected_version: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub reservation_digest: String,
}

impl BudgetReservationFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reservation_id: RequestId,
        execution_id: ExecutionId,
        run_id: RunId,
        budget_lease_id: BudgetLeaseId,
        scope: BudgetScope,
        parent_reservation_id: Option<RequestId>,
        model_calls: u64,
        tool_calls: u64,
        tokens: u64,
        effects: u32,
        max_output_tokens: u64,
        authority_epoch: u64,
        expected_version: u64,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: BUDGET_RESERVATION_SCHEMA.to_owned(),
            version: BUDGET_FACT_VERSION,
            reservation_id,
            execution_id,
            run_id,
            budget_lease_id,
            scope,
            parent_reservation_id,
            model_calls,
            tool_calls,
            tokens,
            effects,
            max_output_tokens,
            authority_epoch,
            expected_version,
            issued_at_unix_ms,
            expires_at_unix_ms,
            reservation_digest: String::new(),
        };
        fact.reservation_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUDGET_RESERVATION_SCHEMA
            || !self.version.is_compatible_with(&BUDGET_FACT_VERSION)
            || self.reservation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.budget_lease_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.model_calls.saturating_add(self.tool_calls) == 0
            || (self.model_calls > 0
                && (self.tokens == 0
                    || self.max_output_tokens == 0
                    || self.max_output_tokens > self.tokens))
            || matches!(self.scope, BudgetScope::Child) && self.parent_reservation_id.is_none()
            || self
                .parent_reservation_id
                .is_some_and(|id| id.as_uuid().is_nil())
        {
            return Err("budget_reservation_fact_invalid".to_owned());
        }
        validate_digest(&self.reservation_digest, "budget_reservation_fact_digest")?;
        if self.reservation_digest != self.digest() {
            return Err("budget_reservation_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "reservation_id": self.reservation_id,
            "execution_id": self.execution_id,
            "run_id": self.run_id,
            "budget_lease_id": self.budget_lease_id,
            "scope": self.scope,
            "parent_reservation_id": self.parent_reservation_id,
            "model_calls": self.model_calls,
            "tool_calls": self.tool_calls,
            "tokens": self.tokens,
            "effects": self.effects,
            "max_output_tokens": self.max_output_tokens,
            "authority_epoch": self.authority_epoch,
            "expected_version": self.expected_version,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSettlementFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub reservation_id: RequestId,
    pub execution_id: ExecutionId,
    pub run_id: RunId,
    pub budget_lease_id: BudgetLeaseId,
    pub scope: BudgetScope,
    pub model_calls: u64,
    pub tool_calls: u64,
    pub charged_tokens: Option<u64>,
    pub effects: u32,
    pub usage_known: bool,
    pub settled_at_unix_ms: u64,
    pub settlement_digest: String,
}

impl BudgetSettlementFact {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reservation: &BudgetReservationFact,
        charged_tokens: Option<u64>,
        settled_at_unix_ms: u64,
    ) -> Result<Self, String> {
        reservation.validate()?;
        let mut fact = Self {
            schema: BUDGET_SETTLEMENT_SCHEMA.to_owned(),
            version: BUDGET_FACT_VERSION,
            reservation_id: reservation.reservation_id,
            execution_id: reservation.execution_id,
            run_id: reservation.run_id,
            budget_lease_id: reservation.budget_lease_id,
            scope: reservation.scope,
            model_calls: reservation.model_calls,
            tool_calls: reservation.tool_calls,
            charged_tokens,
            effects: reservation.effects,
            usage_known: charged_tokens.is_some(),
            settled_at_unix_ms,
            settlement_digest: String::new(),
        };
        fact.settlement_digest = fact.digest();
        fact.validate_against(reservation)?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BUDGET_SETTLEMENT_SCHEMA
            || !self.version.is_compatible_with(&BUDGET_FACT_VERSION)
            || self.reservation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self.budget_lease_id.as_uuid().is_nil()
            || self.settled_at_unix_ms == 0
            || self.usage_known != self.charged_tokens.is_some()
        {
            return Err("budget_settlement_fact_invalid".to_owned());
        }
        validate_digest(&self.settlement_digest, "budget_settlement_fact_digest")?;
        if self.settlement_digest != self.digest() {
            return Err("budget_settlement_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, reservation: &BudgetReservationFact) -> Result<(), String> {
        self.validate()?;
        if self.reservation_id != reservation.reservation_id
            || self.execution_id != reservation.execution_id
            || self.run_id != reservation.run_id
            || self.budget_lease_id != reservation.budget_lease_id
            || self.scope != reservation.scope
            || self.model_calls != reservation.model_calls
            || self.tool_calls != reservation.tool_calls
            || self.effects != reservation.effects
            || self.settled_at_unix_ms < reservation.issued_at_unix_ms
            || self
                .charged_tokens
                .is_some_and(|tokens| tokens > reservation.tokens)
        {
            return Err("budget_settlement_fact_reservation_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "reservation_id": self.reservation_id,
            "execution_id": self.execution_id,
            "run_id": self.run_id,
            "budget_lease_id": self.budget_lease_id,
            "scope": self.scope,
            "model_calls": self.model_calls,
            "tool_calls": self.tool_calls,
            "charged_tokens": self.charged_tokens,
            "effects": self.effects,
            "usage_known": self.usage_known,
            "settled_at_unix_ms": self.settled_at_unix_ms,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
