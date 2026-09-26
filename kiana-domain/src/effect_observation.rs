//! Provider/external effect observations kept separate from local result success.

use crate::{
    json_digest, provider_payload_hash_valid, valid_extension_identifier, ExecutionId,
    InvocationId, ProviderOutcome, ProviderReceipt, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const EFFECT_OBSERVATION_SCHEMA: &str = "kiana.effect-observation.v1";
pub const EFFECT_OBSERVATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_EFFECT_EVIDENCE: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectObservationState {
    ConfirmedSuccess,
    ConfirmedFailure,
    NoEffect,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectObservation {
    pub schema: String,
    pub version: SchemaVersion,
    pub execution_id: ExecutionId,
    pub invocation_id: InvocationId,
    pub attempt: u32,
    pub owner_digest: String,
    pub audience_digest: String,
    pub idempotency_key_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_status: Option<String>,
    pub observed_at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_digest: Option<String>,
    #[serde(default)]
    pub evidence_ref_digests: Vec<String>,
    pub state: EffectObservationState,
    pub observation_digest: String,
}

impl EffectObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: ExecutionId,
        invocation_id: InvocationId,
        attempt: u32,
        owner_digest: impl Into<String>,
        audience_digest: impl Into<String>,
        idempotency_key_digest: impl Into<String>,
        provider_receipt_id: Option<String>,
        remote_status: Option<String>,
        observed_at_unix_ms: u64,
        query_digest: Option<String>,
        mut evidence_ref_digests: Vec<String>,
        state: EffectObservationState,
    ) -> Result<Self, String> {
        Self::new_with_payload_sha256(
            execution_id,
            invocation_id,
            attempt,
            owner_digest,
            audience_digest,
            idempotency_key_digest,
            provider_receipt_id,
            remote_status,
            observed_at_unix_ms,
            query_digest,
            evidence_ref_digests,
            None,
            state,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_payload_sha256(
        execution_id: ExecutionId,
        invocation_id: InvocationId,
        attempt: u32,
        owner_digest: impl Into<String>,
        audience_digest: impl Into<String>,
        idempotency_key_digest: impl Into<String>,
        provider_receipt_id: Option<String>,
        remote_status: Option<String>,
        observed_at_unix_ms: u64,
        query_digest: Option<String>,
        mut evidence_ref_digests: Vec<String>,
        payload_sha256: Option<String>,
        state: EffectObservationState,
    ) -> Result<Self, String> {
        evidence_ref_digests.sort();
        evidence_ref_digests.dedup();
        let mut observation = Self {
            schema: EFFECT_OBSERVATION_SCHEMA.to_owned(),
            version: EFFECT_OBSERVATION_VERSION,
            execution_id,
            invocation_id,
            attempt,
            owner_digest: owner_digest.into(),
            audience_digest: audience_digest.into(),
            idempotency_key_digest: idempotency_key_digest.into(),
            payload_sha256,
            provider_receipt_id,
            remote_status,
            observed_at_unix_ms,
            query_digest,
            evidence_ref_digests,
            state,
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate()?;
        Ok(observation)
    }

    pub fn from_provider_receipt(
        receipt: &ProviderReceipt,
        execution_id: ExecutionId,
        invocation_id: InvocationId,
        attempt: u32,
        owner_digest: impl Into<String>,
        audience_digest: impl Into<String>,
        observed_at_unix_ms: u64,
    ) -> Result<Self, String> {
        receipt
            .validate()
            .map_err(|_| "effect_observation_provider_receipt_invalid".to_owned())?;
        if !valid_extension_identifier(&receipt.connector_id)
            || !valid_extension_identifier(&receipt.binding_id)
            || !valid_extension_identifier(&receipt.account_id)
            || !valid_extension_identifier(&receipt.operation)
            || !provider_payload_hash_valid(&receipt.final_payload_sha256)
        {
            return Err("effect_observation_provider_receipt_invalid".to_owned());
        }
        let state = match receipt.outcome {
            ProviderOutcome::Succeeded => EffectObservationState::ConfirmedSuccess,
            ProviderOutcome::Failed => EffectObservationState::ConfirmedFailure,
            ProviderOutcome::Unknown => EffectObservationState::Unknown,
        };
        Self::new_with_payload_sha256(
            execution_id,
            invocation_id,
            attempt,
            owner_digest,
            audience_digest,
            json_digest(&json!({"idempotency_key":receipt.idempotency_key})),
            Some(receipt.provider_receipt_id.clone()),
            Some(format!("{:?}", receipt.outcome).to_ascii_lowercase()),
            observed_at_unix_ms,
            None,
            Vec::new(),
            Some(receipt.final_payload_sha256.clone()),
            state,
        )
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let observation: Self = serde_json::from_value(value.clone())
            .map_err(|_| "effect_observation_decode_failed".to_owned())?;
        observation.validate()?;
        Ok(observation)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "effect_observation_encode_failed".to_owned())
    }

    pub fn validate_for_scope(
        &self,
        owner_digest: &str,
        audience_digest: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.owner_digest != owner_digest || self.audience_digest != audience_digest {
            return Err("effect_observation_owner_or_audience_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_receipt(
        &self,
        receipt: &ProviderReceipt,
        owner_digest: &str,
        audience_digest: &str,
    ) -> Result<(), String> {
        receipt.validate()?;
        self.validate_for_scope(owner_digest, audience_digest)?;
        let expected_remote_status = format!("{:?}", receipt.outcome).to_ascii_lowercase();
        if self.payload_sha256.as_deref() != Some(receipt.final_payload_sha256.as_str())
            || self.idempotency_key_digest
                != json_digest(&json!({"idempotency_key": receipt.idempotency_key}))
            || self.provider_receipt_id.as_deref() != Some(receipt.provider_receipt_id.as_str())
            || self.remote_status.as_deref() != Some(expected_remote_status.as_str())
            || !matches!(
                (self.state, receipt.outcome),
                (
                    EffectObservationState::ConfirmedSuccess,
                    ProviderOutcome::Succeeded
                ) | (
                    EffectObservationState::ConfirmedFailure,
                    ProviderOutcome::Failed
                ) | (EffectObservationState::Unknown, ProviderOutcome::Unknown)
            )
        {
            return Err("effect_observation_receipt_binding_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EFFECT_OBSERVATION_SCHEMA
            || !self.version.is_compatible_with(&EFFECT_OBSERVATION_VERSION)
            || self.execution_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.attempt == 0
            || !valid_digest(&self.owner_digest)
            || !valid_digest(&self.audience_digest)
            || !valid_digest(&self.idempotency_key_digest)
            || self
                .payload_sha256
                .as_deref()
                .is_some_and(|digest| !provider_payload_hash_valid(digest))
            || self.observed_at_unix_ms == 0
            || self.evidence_ref_digests.len() > MAX_EFFECT_EVIDENCE
            || self
                .evidence_ref_digests
                .iter()
                .any(|digest| !valid_digest(digest))
            || self
                .provider_receipt_id
                .as_ref()
                .is_some_and(|id| !valid_extension_identifier(id))
            || self.remote_status.as_ref().is_some_and(|status| {
                status.trim().is_empty() || status.len() > 128 || status.contains('\0')
            })
            || self
                .query_digest
                .as_ref()
                .is_some_and(|digest| !valid_digest(digest))
            || !valid_digest(&self.observation_digest)
        {
            return Err("effect_observation_header_invalid".to_owned());
        }
        match self.state {
            EffectObservationState::ConfirmedSuccess | EffectObservationState::ConfirmedFailure => {
                if self.provider_receipt_id.is_none() {
                    return Err("effect_observation_provider_receipt_required".to_owned());
                }
            }
            EffectObservationState::NoEffect => {
                if self.query_digest.is_none() {
                    return Err("effect_observation_query_required".to_owned());
                }
            }
            EffectObservationState::Unknown => {}
        }
        if self.observation_digest != self.digest() {
            return Err("effect_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "execution_id": self.execution_id,
            "invocation_id": self.invocation_id,
            "attempt": self.attempt,
            "owner_digest": self.owner_digest,
            "audience_digest": self.audience_digest,
            "idempotency_key_digest": self.idempotency_key_digest,
            "payload_sha256": self.payload_sha256,
            "provider_receipt_id": self.provider_receipt_id,
            "remote_status": self.remote_status,
            "observed_at_unix_ms": self.observed_at_unix_ms,
            "query_digest": self.query_digest,
            "evidence_ref_digests": self.evidence_ref_digests,
            "state": self.state,
        }))
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
