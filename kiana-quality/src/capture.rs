//! EQ-23 explicit-source GoldenTrace capture contract.

use kiana_domain::{EvalCaseId, EvalSuiteId, GoldenTrace, GoldenTraceId, RunId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const CAPTURE_SCHEMA: &str = "kiana.quality-golden-capture.v1";
pub const MAX_DESTINATION_REF_BYTES: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSourceKind {
    Run,
    Fixture,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum CaptureSource {
    Run {
        run_id: RunId,
        source_event_digest: String,
        #[serde(default)]
        receipt_digest: Option<String>,
    },
    Fixture {
        fixture_ref: String,
        fixture_digest: String,
    },
}

impl CaptureSource {
    pub fn kind(&self) -> CaptureSourceKind {
        match self {
            Self::Run { .. } => CaptureSourceKind::Run,
            Self::Fixture { .. } => CaptureSourceKind::Fixture,
        }
    }

    fn validate(&self) -> Result<(), CaptureError> {
        match self {
            Self::Run {
                run_id,
                source_event_digest,
                receipt_digest,
            } => {
                if run_id.as_uuid().is_nil() || !is_digest(source_event_digest) {
                    return Err(CaptureError::SourceInvalid(
                        "capture_run_source_invalid".to_owned(),
                    ));
                }
                if receipt_digest
                    .as_deref()
                    .is_some_and(|digest| !is_digest(digest))
                {
                    return Err(CaptureError::SourceInvalid(
                        "capture_run_receipt_digest_invalid".to_owned(),
                    ));
                }
            }
            Self::Fixture {
                fixture_ref,
                fixture_digest,
            } => {
                if fixture_ref.trim().is_empty()
                    || fixture_ref.len() > MAX_DESTINATION_REF_BYTES
                    || fixture_ref.contains(['\0', '\n', '\r'])
                    || !is_digest(fixture_digest)
                {
                    return Err(CaptureError::SourceInvalid(
                        "capture_fixture_source_invalid".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenTraceCaptureRequest {
    pub schema: String,
    pub source: CaptureSource,
    /// Opaque logical destination. It is a new version slot, never a filesystem path.
    pub destination_ref: String,
    pub suite_id: EvalSuiteId,
    pub case_id: EvalCaseId,
    pub source_snapshot: String,
    pub input_hash: String,
    pub target_versions: BTreeMap<String, String>,
    pub event_cursor_start: u64,
    pub event_cursor_end: u64,
    pub normalized_events: Vec<Value>,
    pub artifact_hashes: Vec<String>,
    #[serde(default)]
    pub receipt_hash: Option<String>,
    pub normalization_version: String,
    pub created_at_unix_ms: u64,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    pub provenance_ref: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenTraceCaptureReceipt {
    pub schema: String,
    pub source_kind: CaptureSourceKind,
    pub destination_ref: String,
    pub trace_id: GoldenTraceId,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CaptureError {
    #[error("golden_capture_schema_invalid")]
    SchemaInvalid,
    #[error("golden_capture_source_invalid:{0}")]
    SourceInvalid(String),
    #[error("golden_capture_destination_invalid")]
    DestinationInvalid,
    #[error("golden_capture_destination_exists")]
    DestinationExists,
    #[error("golden_capture_trace_invalid:{0}")]
    TraceInvalid(String),
}

fn is_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl GoldenTraceCaptureRequest {
    pub fn validate(&self, occupied_destinations: &[String]) -> Result<(), CaptureError> {
        if self.schema != CAPTURE_SCHEMA {
            return Err(CaptureError::SchemaInvalid);
        }
        self.source.validate()?;
        if self.destination_ref.trim().is_empty()
            || self.destination_ref.len() > MAX_DESTINATION_REF_BYTES
            || self.destination_ref.contains(['\0', '\n', '\r'])
        {
            return Err(CaptureError::DestinationInvalid);
        }
        if occupied_destinations
            .iter()
            .any(|destination| destination == &self.destination_ref)
        {
            return Err(CaptureError::DestinationExists);
        }
        Ok(())
    }
}

pub fn capture_golden_trace(
    request: GoldenTraceCaptureRequest,
    occupied_destinations: &[String],
) -> Result<(GoldenTrace, GoldenTraceCaptureReceipt), CaptureError> {
    request.validate(occupied_destinations)?;
    let source_run_id = match &request.source {
        CaptureSource::Run { run_id, .. } => Some(*run_id),
        CaptureSource::Fixture { .. } => None,
    };
    let trace = GoldenTrace::new(
        request.suite_id,
        request.case_id,
        source_run_id,
        request.source_snapshot,
        request.input_hash,
        request.target_versions,
        request.event_cursor_start,
        request.event_cursor_end,
        request.normalized_events,
        request.artifact_hashes,
        request.receipt_hash,
        request.normalization_version,
        None,
        None,
        request.created_at_unix_ms,
        request.expires_at_unix_ms,
        request.provenance_ref,
    )
    .map_err(CaptureError::TraceInvalid)?;
    let receipt = GoldenTraceCaptureReceipt {
        schema: CAPTURE_SCHEMA.to_owned(),
        source_kind: request.source.kind(),
        destination_ref: request.destination_ref,
        trace_id: trace.trace_id,
    };
    Ok((trace, receipt))
}
