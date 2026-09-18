//! Provider-neutral usage vector and normalized observation contracts.
//!
//! `None` means absent/unknown while `Some(0)` is an explicitly reported zero. Stream sequence
//! monotonicity and snapshot/delta accumulation are deliberately left to BQ-03; this module only
//! validates one bounded observation and its provenance.

use crate::{
    json_digest, AttemptId, BillingUnknownReason, CellId, OrganizationId, ProjectId, RunId,
    SchemaVersion, UsageId,
};
use serde::{Deserialize, Serialize};

pub const USAGE_VECTOR_SCHEMA: &str = "kiana.usage-vector.v1";
pub const NORMALIZED_USAGE_SCHEMA: &str = "kiana.normalized-usage.v1";
pub const USAGE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsagePresence {
    Absent,
    ExplicitZero,
    Present,
}

impl UsagePresence {
    pub const fn of(value: Option<u64>) -> Self {
        match value {
            None => Self::Absent,
            Some(0) => Self::ExplicitZero,
            Some(_) => Self::Present,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageVector {
    pub schema: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub audio_input_tokens: Option<u64>,
    pub audio_output_tokens: Option<u64>,
    pub tool_calls: u64,
    pub effect_count: u64,
    pub wall_time_ms: u64,
    pub output_bytes: u64,
    pub artifact_bytes: u64,
    pub storage_bytes: u64,
}

impl UsageVector {
    pub fn zero() -> Self {
        Self {
            schema: USAGE_VECTOR_SCHEMA.to_owned(),
            input_tokens: Some(0),
            output_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_output_tokens: Some(0),
            audio_input_tokens: Some(0),
            audio_output_tokens: Some(0),
            tool_calls: 0,
            effect_count: 0,
            wall_time_ms: 0,
            output_bytes: 0,
            artifact_bytes: 0,
            storage_bytes: 0,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != USAGE_VECTOR_SCHEMA {
            return Err("usage_vector_schema_invalid".to_owned());
        }
        Ok(())
    }

    pub const fn input_presence(&self) -> UsagePresence {
        UsagePresence::of(self.input_tokens)
    }

    pub const fn output_presence(&self) -> UsagePresence {
        UsagePresence::of(self.output_tokens)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageSource {
    Provider,
    LocalExecutor,
    Derived,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageObservation {
    Snapshot,
    Delta,
    Final,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageConfidence {
    Known,
    Partial,
    Unknown,
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && !value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedUsage {
    pub schema: String,
    pub version: SchemaVersion,
    pub usage_id: UsageId,
    pub attempt_id: AttemptId,
    pub invocation_id: Option<crate::InvocationId>,
    pub run_id: RunId,
    pub cell_id: Option<CellId>,
    pub project_id: Option<ProjectId>,
    pub organization_id: Option<OrganizationId>,
    pub provider_id: Option<String>,
    pub requested_model_id: String,
    pub served_model_id: Option<String>,
    pub route_id: String,
    pub retry_ordinal: u32,
    pub vector: UsageVector,
    pub source: UsageSource,
    pub observation: UsageObservation,
    pub sequence: Option<u64>,
    pub confidence: UsageConfidence,
    pub unknown_reason: Option<BillingUnknownReason>,
    pub basis: String,
    pub raw_digest: String,
    pub usage_digest: String,
}

impl NormalizedUsage {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        usage_id: UsageId,
        attempt_id: AttemptId,
        run_id: RunId,
        provider_id: Option<String>,
        requested_model_id: impl Into<String>,
        route_id: impl Into<String>,
        vector: UsageVector,
        source: UsageSource,
        observation: UsageObservation,
        sequence: Option<u64>,
        confidence: UsageConfidence,
        unknown_reason: Option<BillingUnknownReason>,
        basis: impl Into<String>,
        raw_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut usage = Self {
            schema: NORMALIZED_USAGE_SCHEMA.to_owned(),
            version: USAGE_SCHEMA_VERSION,
            usage_id,
            attempt_id,
            invocation_id: None,
            run_id,
            cell_id: None,
            project_id: None,
            organization_id: None,
            provider_id,
            requested_model_id: requested_model_id.into(),
            served_model_id: None,
            route_id: route_id.into(),
            retry_ordinal: 0,
            vector,
            source,
            observation,
            sequence,
            confidence,
            unknown_reason,
            basis: basis.into(),
            raw_digest: raw_digest.into(),
            usage_digest: String::new(),
        };
        usage.usage_digest = usage.digest();
        usage.validate()?;
        Ok(usage)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NORMALIZED_USAGE_SCHEMA
            || !self.version.is_compatible_with(&USAGE_SCHEMA_VERSION)
            || self.usage_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.run_id.as_uuid().is_nil()
            || self
                .provider_id
                .as_deref()
                .is_some_and(|id| !bounded(id, 256))
            || !bounded(&self.requested_model_id, 256)
            || self
                .served_model_id
                .as_deref()
                .is_some_and(|id| !bounded(id, 256))
            || !bounded(&self.route_id, 256)
            || !bounded(&self.basis, 512)
            || !valid_digest(&self.raw_digest)
            || !valid_digest(&self.usage_digest)
            || self.usage_digest != self.digest()
        {
            return Err("normalized_usage_header_invalid".to_owned());
        }
        self.vector.validate()?;
        if matches!(self.observation, UsageObservation::Delta)
            && self.sequence.is_none_or(|sequence| sequence == 0)
        {
            return Err("normalized_usage_delta_sequence_required".to_owned());
        }
        match self.confidence {
            UsageConfidence::Known if self.unknown_reason.is_some() => {
                return Err("normalized_usage_known_reason_invalid".to_owned())
            }
            UsageConfidence::Partial | UsageConfidence::Unknown
                if self.unknown_reason.is_none() =>
            {
                return Err("normalized_usage_unknown_reason_required".to_owned())
            }
            _ => {}
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "usage_id": self.usage_id,
            "attempt_id": self.attempt_id,
            "invocation_id": self.invocation_id,
            "run_id": self.run_id,
            "cell_id": self.cell_id,
            "project_id": self.project_id,
            "organization_id": self.organization_id,
            "provider_id": self.provider_id,
            "requested_model_id": self.requested_model_id,
            "served_model_id": self.served_model_id,
            "route_id": self.route_id,
            "retry_ordinal": self.retry_ordinal,
            "vector": self.vector,
            "source": self.source,
            "observation": self.observation,
            "sequence": self.sequence,
            "confidence": self.confidence,
            "unknown_reason": self.unknown_reason,
            "basis": self.basis,
            "raw_digest": self.raw_digest,
        }))
    }
}
