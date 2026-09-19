//! EQ-20 version-bound event, trace, artifact and receipt digests.

use crate::{VolatileEvent, VolatileEventTrace, VOLATILE_NORMALIZATION_VERSION};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DIGEST_SCHEMA: &str = "kiana.quality-evidence-digest.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestKind {
    Event,
    Trace,
    Artifact,
    Receipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedEvidenceDigest {
    pub schema: String,
    pub kind: DigestKind,
    pub normalization_version: String,
    pub digest: String,
}

impl VersionedEvidenceDigest {
    pub fn validate(&self) -> Result<(), DigestError> {
        if self.schema != DIGEST_SCHEMA || self.normalization_version.trim().is_empty() {
            return Err(DigestError::HeaderInvalid);
        }
        if !is_digest(&self.digest) {
            return Err(DigestError::DigestInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DigestError {
    #[error("quality_digest_version_required")]
    VersionRequired,
    #[error("quality_digest_header_invalid")]
    HeaderInvalid,
    #[error("quality_digest_invalid")]
    DigestInvalid,
    #[error("quality_digest_encode_failed")]
    EncodeFailed,
    #[error("quality_digest_trace_version_invalid")]
    TraceVersionInvalid,
    #[error("quality_digest_trace_empty")]
    TraceEmpty,
}

fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn require_version(version: &str) -> Result<(), DigestError> {
    if version.trim().is_empty() {
        Err(DigestError::VersionRequired)
    } else {
        Ok(())
    }
}

fn versioned_digest(
    kind: DigestKind,
    normalization_version: &str,
    value: Value,
) -> Result<VersionedEvidenceDigest, DigestError> {
    require_version(normalization_version)?;
    let digest = json_digest(&json!({
        "kind": kind,
        "normalization_version": normalization_version,
        "value": value,
    }));
    let result = VersionedEvidenceDigest {
        schema: DIGEST_SCHEMA.to_owned(),
        kind,
        normalization_version: normalization_version.to_owned(),
        digest,
    };
    result.validate()?;
    Ok(result)
}

/// Digest one already canonical/redacted event. The source cursor and event kind are part of the
/// envelope; raw source identity fields are not read outside the normalized event value.
pub fn event_digest(
    event: &VolatileEvent,
    normalization_version: &str,
) -> Result<VersionedEvidenceDigest, DigestError> {
    versioned_digest(
        DigestKind::Event,
        normalization_version,
        json!({
            "source_cursor": event.source_cursor,
            "kind": event.kind.clone(),
            "value": event.value.clone(),
        }),
    )
}

/// Digest the normalized event sequence and bind its exact normalizer version and replacement
/// accounting. Changing normalization rules therefore cannot silently reuse an old trace digest.
pub fn trace_digest(trace: &VolatileEventTrace) -> Result<VersionedEvidenceDigest, DigestError> {
    if trace.normalization_version != VOLATILE_NORMALIZATION_VERSION {
        return Err(DigestError::TraceVersionInvalid);
    }
    if trace.events.is_empty() {
        return Err(DigestError::TraceEmpty);
    }
    let event_digests = trace
        .events
        .iter()
        .map(|event| event_digest(event, &trace.normalization_version))
        .collect::<Result<Vec<_>, _>>()?;
    versioned_digest(
        DigestKind::Trace,
        &trace.normalization_version,
        json!({
            "source_normalization_version": trace.source_normalization_version,
            "array_policy": trace.array_policy,
            "source_cursor_start": trace.source_cursor_start,
            "source_cursor_end": trace.source_cursor_end,
            "replacement_count": trace.replacement_count,
            "replacements": trace.replacements.clone(),
            "event_digests": event_digests,
        }),
    )
}

pub fn artifact_digest<T: Serialize>(
    artifact: &T,
    normalization_version: &str,
) -> Result<VersionedEvidenceDigest, DigestError> {
    let value = serde_json::to_value(artifact).map_err(|_| DigestError::EncodeFailed)?;
    versioned_digest(DigestKind::Artifact, normalization_version, value)
}

pub fn receipt_digest<T: Serialize>(
    receipt: &T,
    normalization_version: &str,
) -> Result<VersionedEvidenceDigest, DigestError> {
    let value = serde_json::to_value(receipt).map_err(|_| DigestError::EncodeFailed)?;
    versioned_digest(DigestKind::Receipt, normalization_version, value)
}
