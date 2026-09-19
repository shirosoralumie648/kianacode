//! Retrieval result provenance, health and retry boundaries.
//!
//! A result is a derived read projection. Every hit carries source/generation evidence, while an
//! empty or degraded response must explain its status. Retryability is explicit and cannot be
//! inferred from a missing embedding or a transient-looking string.

use crate::{json_digest, EvidenceStatus, Freshness, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const RETRIEVAL_EVIDENCE_SCHEMA: &str = "kiana.retrieval-evidence.v1";
pub const RETRIEVAL_HEALTH_SCHEMA: &str = "kiana.retrieval-health.v1";
pub const RETRIEVAL_RESPONSE_SCHEMA: &str = "kiana.retrieval-response.v1";
pub const RETRIEVAL_EVIDENCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalHealthStatus {
    Ready,
    Degraded,
    Denied,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalHealth {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: RetrievalHealthStatus,
    #[serde(default)]
    pub reason: Option<String>,
    pub retryable: bool,
    pub retry_safe: bool,
    pub source_generation: u64,
    pub result_count: u32,
    pub health_digest: String,
}

impl RetrievalHealth {
    pub fn new(
        status: RetrievalHealthStatus,
        reason: Option<String>,
        retryable: bool,
        retry_safe: bool,
        source_generation: u64,
        result_count: u32,
    ) -> Result<Self, String> {
        let mut health = Self {
            schema: RETRIEVAL_HEALTH_SCHEMA.to_owned(),
            version: RETRIEVAL_EVIDENCE_VERSION,
            status,
            reason,
            retryable,
            retry_safe,
            source_generation,
            result_count,
            health_digest: String::new(),
        };
        health.health_digest = health.digest();
        health.validate()?;
        Ok(health)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_HEALTH_SCHEMA
            || self.version != RETRIEVAL_EVIDENCE_VERSION
            || self.source_generation == 0
            || (self.retryable && !self.retry_safe)
        {
            return Err("retrieval_health_header_invalid".to_owned());
        }
        if matches!(self.status, RetrievalHealthStatus::Ready)
            && (self.reason.is_some() || self.retryable)
        {
            return Err("retrieval_health_ready_has_failure".to_owned());
        }
        if matches!(
            self.status,
            RetrievalHealthStatus::Degraded
                | RetrievalHealthStatus::Denied
                | RetrievalHealthStatus::Unavailable
        ) && self.reason.as_deref().is_none_or(str::is_empty)
        {
            return Err("retrieval_health_reason_required".to_owned());
        }
        if let Some(reason) = &self.reason {
            required(reason, "retrieval_health_reason", 256)?;
        }
        digest(&self.health_digest, "retrieval_health_digest")?;
        if self.health_digest != self.digest() {
            return Err("retrieval_health_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "reason": self.reason,
            "retryable": self.retryable,
            "retry_safe": self.retry_safe,
            "source_generation": self.source_generation,
            "result_count": self.result_count,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub candidate_id: String,
    pub source_ref: String,
    pub source_revision: String,
    pub source_snapshot_digest: String,
    pub generation: u64,
    pub freshness: Freshness,
    pub evidence: EvidenceStatus,
    pub rank: u32,
    pub rank_components: BTreeMap<String, f64>,
    pub evidence_digest: String,
}

impl RetrievalEvidence {
    pub fn new(
        candidate_id: impl Into<String>,
        source_ref: impl Into<String>,
        source_revision: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        generation: u64,
        freshness: Freshness,
        evidence: EvidenceStatus,
        rank: u32,
        rank_components: BTreeMap<String, f64>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: RETRIEVAL_EVIDENCE_SCHEMA.to_owned(),
            version: RETRIEVAL_EVIDENCE_VERSION,
            candidate_id: candidate_id.into(),
            source_ref: source_ref.into(),
            source_revision: source_revision.into(),
            source_snapshot_digest: source_snapshot_digest.into(),
            generation,
            freshness,
            evidence,
            rank,
            rank_components,
            evidence_digest: String::new(),
        };
        value.evidence_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_EVIDENCE_SCHEMA
            || self.version != RETRIEVAL_EVIDENCE_VERSION
            || self.generation == 0
            || self.rank == 0
        {
            return Err("retrieval_evidence_header_invalid".to_owned());
        }
        required(&self.candidate_id, "retrieval_evidence_candidate_id", 256)?;
        required(&self.source_ref, "retrieval_evidence_source_ref", 4_096)?;
        required(
            &self.source_revision,
            "retrieval_evidence_source_revision",
            256,
        )?;
        digest(
            &self.source_snapshot_digest,
            "retrieval_evidence_source_snapshot_digest",
        )?;
        if self.rank_components.len() > 32
            || self
                .rank_components
                .values()
                .any(|value| !value.is_finite())
        {
            return Err("retrieval_evidence_rank_components_invalid".to_owned());
        }
        digest(&self.evidence_digest, "retrieval_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("retrieval_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "candidate_id": self.candidate_id,
            "source_ref": self.source_ref,
            "source_revision": self.source_revision,
            "source_snapshot_digest": self.source_snapshot_digest,
            "generation": self.generation,
            "freshness": self.freshness,
            "evidence": self.evidence,
            "rank": self.rank,
            "rank_components": self.rank_components,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalResponse {
    pub schema: String,
    pub version: SchemaVersion,
    pub query_digest: String,
    pub generation: u64,
    pub hits: Vec<RetrievalEvidence>,
    pub health: RetrievalHealth,
    #[serde(default)]
    pub empty_reason: Option<String>,
    pub response_digest: String,
}

impl RetrievalResponse {
    pub fn new(
        query_digest: impl Into<String>,
        generation: u64,
        hits: Vec<RetrievalEvidence>,
        health: RetrievalHealth,
        empty_reason: Option<String>,
    ) -> Result<Self, String> {
        let mut response = Self {
            schema: RETRIEVAL_RESPONSE_SCHEMA.to_owned(),
            version: RETRIEVAL_EVIDENCE_VERSION,
            query_digest: query_digest.into(),
            generation,
            hits,
            health,
            empty_reason,
            response_digest: String::new(),
        };
        response.response_digest = response.digest();
        response.validate()?;
        Ok(response)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETRIEVAL_RESPONSE_SCHEMA
            || self.version != RETRIEVAL_EVIDENCE_VERSION
            || self.generation == 0
            || self.hits.len() > 256
            || (self.hits.is_empty() && self.empty_reason.as_deref().is_none_or(str::is_empty))
            || (!self.hits.is_empty() && self.empty_reason.is_some())
        {
            return Err("retrieval_response_header_invalid".to_owned());
        }
        digest(&self.query_digest, "retrieval_response_query_digest")?;
        self.health.validate()?;
        if self.health.source_generation != self.generation
            || self.health.result_count != self.hits.len() as u32
        {
            return Err("retrieval_response_health_mismatch".to_owned());
        }
        if let Some(reason) = &self.empty_reason {
            required(reason, "retrieval_response_empty_reason", 256)?;
        }
        for (index, hit) in self.hits.iter().enumerate() {
            hit.validate()?;
            if hit.generation != self.generation || hit.rank != index as u32 + 1 {
                return Err("retrieval_response_hit_generation_or_rank_invalid".to_owned());
            }
        }
        digest(&self.response_digest, "retrieval_response_digest")?;
        if self.response_digest != self.digest() {
            return Err("retrieval_response_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "query_digest": self.query_digest,
            "generation": self.generation,
            "hits": self.hits,
            "health": self.health,
            "empty_reason": self.empty_reason,
        }))
    }
}
