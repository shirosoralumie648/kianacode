//! Cross-surface protocol conformance trace validator.
//!
//! The validator decodes and compares already-produced DTO metadata. It never submits a command,
//! retries an Unknown result, or treats a presenter-specific field as server authority.

use crate::{compare_surface_traces, ParitySurface, SurfaceParityError, SurfaceTrace};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const UI_CONFORMANCE_SCHEMA: &str = "kiana.ui-conformance.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceTrace {
    pub schema: String,
    pub surface: ParitySurface,
    pub protocol_schema: String,
    pub ui_schema: String,
    pub capability_schema: String,
    pub command_id: String,
    pub cursor_epoch: String,
    pub feed_sequence: u64,
    pub action_disposition: String,
    pub retry: String,
    #[serde(default)]
    pub artifact_digest: Option<String>,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    pub unknown_visible: bool,
    pub sensitive_field_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConformanceReport {
    pub schema: String,
    pub command_id: String,
    pub surfaces: Vec<ParitySurface>,
    pub protocol_schema: String,
    pub ui_schema: String,
    pub capability_schema: String,
    pub cursor_epoch: String,
    pub feed_sequence: u64,
    pub action_disposition: String,
    pub retry: String,
    pub artifact_digest: Option<String>,
    pub receipt_digest: Option<String>,
    pub unknown_visible: bool,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum ConformanceError {
    #[error("ui_conformance_schema_invalid:{0}")]
    SchemaInvalid(&'static str),
    #[error("ui_conformance_unknown_hidden")]
    UnknownHidden,
    #[error("ui_conformance_sensitive_fields")]
    SensitiveFields,
    #[error("ui_conformance_mismatch:{0}")]
    Mismatch(&'static str),
    #[error("ui_conformance_parity:{0}")]
    Parity(String),
}

impl ConformanceTrace {
    pub fn validate(&self) -> Result<(), ConformanceError> {
        if self.schema != UI_CONFORMANCE_SCHEMA {
            return Err(ConformanceError::SchemaInvalid("trace_schema"));
        }
        if self.protocol_schema != "kiana.protocol.v1" || self.ui_schema != "kiana.ui.v1" {
            return Err(ConformanceError::SchemaInvalid("protocol_or_ui_schema"));
        }
        if self.capability_schema != "kiana.ui-capability.v1" {
            return Err(ConformanceError::SchemaInvalid("capability_schema"));
        }
        if self.command_id.trim().is_empty()
            || self.cursor_epoch.trim().is_empty()
            || self.feed_sequence == 0
        {
            return Err(ConformanceError::SchemaInvalid("identity_or_cursor"));
        }
        if self
            .artifact_digest
            .as_deref()
            .is_some_and(|value| !valid_digest(value))
            || self
                .receipt_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
        {
            return Err(ConformanceError::SchemaInvalid(
                "artifact_or_receipt_digest",
            ));
        }
        if self.sensitive_field_count > 0 {
            return Err(ConformanceError::SensitiveFields);
        }
        if self.action_disposition == "unknown" && !self.unknown_visible {
            return Err(ConformanceError::UnknownHidden);
        }
        if !matches!(
            self.action_disposition.as_str(),
            "accepted" | "applied" | "rejected" | "unknown"
        ) {
            return Err(ConformanceError::SchemaInvalid("disposition"));
        }
        if !matches!(
            self.retry.as_str(),
            "query_original" | "safe_retry" | "do_not_retry"
        ) {
            return Err(ConformanceError::SchemaInvalid("retry"));
        }
        Ok(())
    }
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn compare_conformance(
    traces: &[ConformanceTrace],
) -> Result<ConformanceReport, ConformanceError> {
    if traces.is_empty() {
        return Err(ConformanceError::SchemaInvalid("empty"));
    }
    for trace in traces {
        trace.validate()?;
    }
    let parity_traces = traces
        .iter()
        .map(|trace| SurfaceTrace {
            schema: "kiana.surface-parity.v1".to_owned(),
            surface: trace.surface,
            command_id: trace.command_id.clone(),
            operation: "conformance.trace".to_owned(),
            disposition: trace.action_disposition.clone(),
            retry: trace.retry.clone(),
            cursor_epoch: trace.cursor_epoch.clone(),
            cursor_sequence: trace.feed_sequence,
            revision: 1,
            receipt_digest: trace.receipt_digest.clone(),
            error_code: None,
            sensitive_field_count: trace.sensitive_field_count,
            limitations: Vec::new(),
        })
        .collect::<Vec<_>>();
    compare_surface_traces(&parity_traces).map_err(|error| match error {
        SurfaceParityError::Mismatch(field) => ConformanceError::Mismatch(field),
        other => ConformanceError::Parity(other.to_string()),
    })?;
    let first = &traces[0];
    for trace in &traces[1..] {
        if trace.protocol_schema != first.protocol_schema {
            return Err(ConformanceError::Mismatch("protocol_schema"));
        }
        if trace.ui_schema != first.ui_schema {
            return Err(ConformanceError::Mismatch("ui_schema"));
        }
        if trace.capability_schema != first.capability_schema {
            return Err(ConformanceError::Mismatch("capability_schema"));
        }
        if trace.artifact_digest != first.artifact_digest {
            return Err(ConformanceError::Mismatch("artifact_digest"));
        }
        if trace.unknown_visible != first.unknown_visible {
            return Err(ConformanceError::Mismatch("unknown_visibility"));
        }
    }
    Ok(ConformanceReport {
        schema: UI_CONFORMANCE_SCHEMA.to_owned(),
        command_id: first.command_id.clone(),
        surfaces: traces.iter().map(|trace| trace.surface).collect(),
        protocol_schema: first.protocol_schema.clone(),
        ui_schema: first.ui_schema.clone(),
        capability_schema: first.capability_schema.clone(),
        cursor_epoch: first.cursor_epoch.clone(),
        feed_sequence: first.feed_sequence,
        action_disposition: first.action_disposition.clone(),
        retry: first.retry.clone(),
        artifact_digest: first.artifact_digest.clone(),
        receipt_digest: first.receipt_digest.clone(),
        unknown_visible: first.unknown_visible,
    })
}
