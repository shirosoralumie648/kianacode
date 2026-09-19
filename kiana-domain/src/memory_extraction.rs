//! Bounded, idempotent turn-extraction request and quote admission.

use crate::{json_digest, EventId, MemoryProposal, RequestId, RunId, SchemaVersion, TurnId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MEMORY_EXTRACTION_REQUEST_SCHEMA: &str = "kiana.memory-extraction-request.v1";
pub const MEMORY_EXTRACTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MEMORY_EXTRACTION_MAX_EVIDENCE: usize = 32;
pub const MEMORY_EXTRACTION_MAX_QUOTE_BYTES: usize = 16 * 1024;
pub const MEMORY_EXTRACTION_MAX_TOTAL_QUOTE_BYTES: usize = 64 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEvidenceQuote {
    pub event_id: EventId,
    pub request_id: RequestId,
    pub run_id: Option<RunId>,
    pub byte_start: u64,
    pub byte_end: u64,
    pub quote: String,
    pub source_digest: String,
}

impl MemoryEvidenceQuote {
    pub fn validate(&self) -> Result<(), String> {
        if self.event_id.as_uuid().is_nil()
            || self.request_id.as_uuid().is_nil()
            || self.byte_start >= self.byte_end
            || self.quote.is_empty()
            || self.quote.len() > MEMORY_EXTRACTION_MAX_QUOTE_BYTES
        {
            return Err("memory_extraction_quote_invalid".to_owned());
        }
        digest(&self.source_digest, "memory_extraction_source_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryExtractionRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_run_id: RunId,
    pub turn_id: TurnId,
    pub source_event_id: EventId,
    pub extractor_profile: String,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub scope_digest: String,
    pub evidence: Vec<MemoryEvidenceQuote>,
    pub idempotency_key: String,
    pub request_digest: String,
}

impl MemoryExtractionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_run_id: RunId,
        turn_id: TurnId,
        source_event_id: EventId,
        extractor_profile: impl Into<String>,
        source_cursor_start: u64,
        source_cursor_end: u64,
        scope_digest: impl Into<String>,
        evidence: Vec<MemoryEvidenceQuote>,
    ) -> Result<Self, String> {
        let extractor_profile = extractor_profile.into();
        let scope_digest = scope_digest.into();
        let idempotency_key = json_digest(&serde_json::json!({
            "source_run_id": source_run_id,
            "turn_id": turn_id,
            "source_event_id": source_event_id,
            "extractor_profile": extractor_profile,
            "source_cursor_start": source_cursor_start,
            "source_cursor_end": source_cursor_end,
            "scope_digest": scope_digest,
        }));
        let mut request = Self {
            schema: MEMORY_EXTRACTION_REQUEST_SCHEMA.to_owned(),
            version: MEMORY_EXTRACTION_VERSION,
            source_run_id,
            turn_id,
            source_event_id,
            extractor_profile,
            source_cursor_start,
            source_cursor_end,
            scope_digest,
            evidence,
            idempotency_key,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_EXTRACTION_REQUEST_SCHEMA
            || !self.version.is_compatible_with(&MEMORY_EXTRACTION_VERSION)
            || self.source_run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.source_event_id.as_uuid().is_nil()
            || self.source_cursor_start == 0
            || self.source_cursor_start > self.source_cursor_end
            || self.evidence.len() > MEMORY_EXTRACTION_MAX_EVIDENCE
        {
            return Err("memory_extraction_request_header_invalid".to_owned());
        }
        required(&self.extractor_profile, "memory_extraction_profile", 256)?;
        digest(&self.scope_digest, "memory_extraction_scope_digest")?;
        digest(&self.idempotency_key, "memory_extraction_idempotency_key")?;
        digest(&self.request_digest, "memory_extraction_request_digest")?;
        let expected_key = json_digest(&serde_json::json!({
            "source_run_id": self.source_run_id,
            "turn_id": self.turn_id,
            "source_event_id": self.source_event_id,
            "extractor_profile": self.extractor_profile,
            "source_cursor_start": self.source_cursor_start,
            "source_cursor_end": self.source_cursor_end,
            "scope_digest": self.scope_digest,
        }));
        if self.idempotency_key != expected_key {
            return Err("memory_extraction_idempotency_mismatch".to_owned());
        }
        let mut event_ids = BTreeSet::new();
        let mut total_quote_bytes = 0usize;
        for evidence in &self.evidence {
            evidence.validate()?;
            if evidence.run_id.is_some_and(|run| run != self.source_run_id)
                || !event_ids.insert((evidence.event_id, evidence.request_id))
            {
                return Err("memory_extraction_evidence_binding_invalid".to_owned());
            }
            total_quote_bytes = total_quote_bytes.saturating_add(evidence.quote.len());
        }
        if total_quote_bytes > MEMORY_EXTRACTION_MAX_TOTAL_QUOTE_BYTES {
            return Err("memory_extraction_evidence_too_large".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("memory_extraction_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Only exact, server-captured quote evidence may enter a proposal.
    pub fn validate_proposal(&self, proposal: &MemoryProposal) -> Result<(), String> {
        self.validate()?;
        proposal.validate().map_err(|error| error.to_owned())?;
        for fact in &proposal.facts {
            for evidence in &fact.evidence {
                let Some(source) = self.evidence.iter().find(|source| {
                    source.event_id == evidence.event_id
                        && source.request_id == evidence.request_id
                        && source.run_id == evidence.run_id
                }) else {
                    return Err("memory_extraction_quote_not_found".to_owned());
                };
                if evidence.quote != source.quote {
                    return Err("memory_extraction_quote_mismatch".to_owned());
                }
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_run_id": self.source_run_id,
            "turn_id": self.turn_id,
            "source_event_id": self.source_event_id,
            "extractor_profile": self.extractor_profile,
            "source_cursor_start": self.source_cursor_start,
            "source_cursor_end": self.source_cursor_end,
            "scope_digest": self.scope_digest,
            "evidence": self.evidence,
            "idempotency_key": self.idempotency_key,
        }))
    }
}
