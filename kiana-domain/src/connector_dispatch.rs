//! Replayable connector dispatch lifecycle.
//!
//! Dispatch and effect observation are separate facts.  A prepared invocation must become
//! dispatching before an adapter is entered, an observation must be recorded before a result is
//! committed, and both terminal states are fenced against a late result.  This module is pure
//! domain logic: it performs no authorization, I/O or EventLog writes.

use crate::{json_digest, InvocationId};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const CONNECTOR_DISPATCH_LIFECYCLE_SCHEMA: &str = "kiana.connector-dispatch-lifecycle.v1";
pub const CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND: &str = "connector.dispatch.lifecycle";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorDispatchStage {
    Prepared,
    Dispatching,
    Observed,
    ResultCommitted,
    Unknown,
}

impl ConnectorDispatchStage {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::ResultCommitted | Self::Unknown)
    }
}

/// Immutable identity plus the current phase of one connector attempt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorDispatchLifecycle {
    pub schema: String,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub command_digest: String,
    pub binding_digest: String,
    pub payload_digest: String,
    pub idempotency_key_digest: String,
    pub stage: ConnectorDispatchStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub lifecycle_digest: String,
}

impl ConnectorDispatchLifecycle {
    pub fn prepared(
        invocation_id: InvocationId,
        attempt: u32,
        command_digest: impl Into<String>,
        binding_digest: impl Into<String>,
        payload_digest: impl Into<String>,
        idempotency_key_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut lifecycle = Self {
            schema: CONNECTOR_DISPATCH_LIFECYCLE_SCHEMA.to_owned(),
            invocation_id,
            attempt,
            command_digest: command_digest.into(),
            binding_digest: binding_digest.into(),
            payload_digest: payload_digest.into(),
            idempotency_key_digest: idempotency_key_digest.into(),
            stage: ConnectorDispatchStage::Prepared,
            receipt_digest: None,
            observation_digest: None,
            result_digest: None,
            reason: None,
            lifecycle_digest: String::new(),
        };
        lifecycle.lifecycle_digest = lifecycle.digest();
        lifecycle.validate()?;
        Ok(lifecycle)
    }

    /// Advance exactly one legal phase.  Unknown and ResultCommitted are terminal and cannot be
    /// changed by a late provider result or a replayed worker.
    pub fn advance(
        &self,
        next_stage: ConnectorDispatchStage,
        receipt_digest: Option<String>,
        observation_digest: Option<String>,
        result_digest: Option<String>,
        reason: Option<String>,
    ) -> Result<Self, String> {
        self.validate()?;
        let legal = matches!(
            (self.stage, next_stage),
            (
                ConnectorDispatchStage::Prepared,
                ConnectorDispatchStage::Dispatching
            ) | (
                ConnectorDispatchStage::Prepared,
                ConnectorDispatchStage::Unknown
            ) | (
                ConnectorDispatchStage::Dispatching,
                ConnectorDispatchStage::Observed
            ) | (
                ConnectorDispatchStage::Dispatching,
                ConnectorDispatchStage::Unknown
            ) | (
                ConnectorDispatchStage::Observed,
                ConnectorDispatchStage::ResultCommitted
            ) | (
                ConnectorDispatchStage::Observed,
                ConnectorDispatchStage::Unknown
            )
        );
        if !legal {
            return Err(if self.stage.is_terminal() {
                "connector_dispatch_terminal_resurrection".to_owned()
            } else {
                "connector_dispatch_transition_invalid".to_owned()
            });
        }
        if self.stage == ConnectorDispatchStage::Observed
            && (receipt_digest != self.receipt_digest
                || observation_digest != self.observation_digest)
        {
            return Err("connector_dispatch_observation_changed".to_owned());
        }
        // 【为什么这里必须判断 next_stage 而不是 self.stage】
        // 这条不变式的含义是「**还处在 effect 之前的状态时，不得已经带着 effect 的产物**」。
        // 它约束的是**转移之后**的状态，而不是转移之前的状态。
        //
        // 原写法判断 `self.stage`，于是 `Dispatching -> Observed` 这一合法转移被误伤：
        // 进入 `Observed` 按定义就要带上 receipt 与 observation（这正是「已观测」的含义），
        // 但此刻 `self.stage` 仍是 `Dispatching`，于是三个判据里前两个必然非空，
        // 直接被判 `connector_dispatch_pre_effect_artifacts_forbidden`。
        // 结果是：经 `advance` **永远无法到达 `Observed`**，整条生命周期
        // Prepared→Dispatching→Observed→ResultCommitted 在第一步之后就断了。
        //
        // 改为判断 `next_stage`：只有当**即将进入**的状态仍处于 effect 之前
        // （Prepared/Dispatching）时，携带产物才是 premature。`-> Observed` 与
        // `-> ResultCommitted` 携带产物是合法的，原有拒绝面没有被削弱——
        // 「进入 effect 之前的状态却已经带产物」这一真正危险的情形依然被拒。
        if matches!(
            next_stage,
            ConnectorDispatchStage::Prepared | ConnectorDispatchStage::Dispatching
        ) && (receipt_digest.is_some()
            || observation_digest.is_some()
            || result_digest.is_some())
        {
            return Err("connector_dispatch_pre_effect_artifacts_forbidden".to_owned());
        }
        let mut next = Self {
            schema: self.schema.clone(),
            invocation_id: self.invocation_id,
            attempt: self.attempt,
            command_digest: self.command_digest.clone(),
            binding_digest: self.binding_digest.clone(),
            payload_digest: self.payload_digest.clone(),
            idempotency_key_digest: self.idempotency_key_digest.clone(),
            stage: next_stage,
            receipt_digest,
            observation_digest,
            result_digest,
            reason,
            lifecycle_digest: String::new(),
        };
        next.lifecycle_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_DISPATCH_LIFECYCLE_SCHEMA
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !valid_digest(&self.command_digest)
            || !valid_digest(&self.binding_digest)
            || !valid_digest(&self.payload_digest)
            || !valid_digest(&self.idempotency_key_digest)
            || !valid_digest(&self.lifecycle_digest)
        {
            return Err("connector_dispatch_lifecycle_header_invalid".to_owned());
        }
        for (digest, field) in [
            (&self.receipt_digest, "connector_dispatch_receipt_digest"),
            (
                &self.observation_digest,
                "connector_dispatch_observation_digest",
            ),
            (&self.result_digest, "connector_dispatch_result_digest"),
        ] {
            if digest.as_deref().is_some_and(|value| !valid_digest(value)) {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.reason.as_deref().is_some_and(|value| {
            value.trim().is_empty() || value.len() > 256 || value.contains(['\0', '\r', '\n'])
        }) {
            return Err("connector_dispatch_reason_invalid".to_owned());
        }
        match self.stage {
            ConnectorDispatchStage::Prepared | ConnectorDispatchStage::Dispatching => {
                if self.receipt_digest.is_some()
                    || self.observation_digest.is_some()
                    || self.result_digest.is_some()
                    || self.reason.is_some()
                {
                    return Err("connector_dispatch_pre_effect_artifacts_forbidden".to_owned());
                }
            }
            ConnectorDispatchStage::Observed => {
                if self.receipt_digest.is_none()
                    || self.observation_digest.is_none()
                    || self.result_digest.is_some()
                    || self.reason.is_some()
                {
                    return Err("connector_dispatch_observation_incomplete".to_owned());
                }
            }
            ConnectorDispatchStage::ResultCommitted => {
                if self.receipt_digest.is_none()
                    || self.observation_digest.is_none()
                    || self.result_digest.is_none()
                    || self.reason.is_some()
                {
                    return Err("connector_dispatch_result_commit_incomplete".to_owned());
                }
            }
            ConnectorDispatchStage::Unknown => {
                if self.reason.is_none()
                    || self.result_digest.is_some()
                    || self.receipt_digest.is_some() != self.observation_digest.is_some()
                {
                    return Err("connector_dispatch_unknown_evidence_invalid".to_owned());
                }
            }
        }
        if self.lifecycle_digest != self.digest() {
            return Err("connector_dispatch_lifecycle_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "command_digest": self.command_digest,
            "binding_digest": self.binding_digest,
            "payload_digest": self.payload_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
            "stage": self.stage,
            "receipt_digest": self.receipt_digest,
            "observation_digest": self.observation_digest,
            "result_digest": self.result_digest,
            "reason": self.reason,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
