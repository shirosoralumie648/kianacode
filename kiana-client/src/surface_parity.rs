//! Cross-surface parity comparison for the shared UI protocol trace.
//!
//! CLI, Workbench, Web and Desktop may render different text, but their server-owned command,
//! cursor, disposition, retry and receipt identity must agree. This is a read-only comparator;
//! it does not invoke a client, retry a command, or execute an effect.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

pub const SURFACE_PARITY_SCHEMA: &str = "kiana.surface-parity.v1";
const REQUIRED_SURFACES: [ParitySurface; 4] = [
    ParitySurface::Cli,
    ParitySurface::Workbench,
    ParitySurface::Web,
    ParitySurface::Desktop,
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParitySurface {
    Cli,
    Workbench,
    Web,
    Desktop,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceTrace {
    pub schema: String,
    pub surface: ParitySurface,
    pub command_id: String,
    pub operation: String,
    pub disposition: String,
    pub retry: String,
    pub cursor_epoch: String,
    pub cursor_sequence: u64,
    pub revision: u64,
    #[serde(default)]
    pub receipt_digest: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub sensitive_field_count: u32,
    #[serde(default)]
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceParityReport {
    pub schema: String,
    pub command_id: String,
    pub operation: String,
    pub disposition: String,
    pub retry: String,
    pub cursor_epoch: String,
    pub cursor_sequence: u64,
    pub revision: u64,
    pub receipt_digest: Option<String>,
    pub surfaces: Vec<ParitySurface>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Error, Clone, Eq, PartialEq)]
pub enum SurfaceParityError {
    #[error("surface_parity_empty")]
    Empty,
    #[error("surface_parity_trace_invalid:{0}")]
    TraceInvalid(&'static str),
    #[error("surface_parity_surface_missing")]
    SurfaceMissing,
    #[error("surface_parity_duplicate_surface")]
    DuplicateSurface,
    #[error("surface_parity_sensitive_fields")]
    SensitiveFields,
    #[error("surface_parity_mismatch:{0}")]
    Mismatch(&'static str),
}

impl SurfaceTrace {
    pub fn validate(&self) -> Result<(), SurfaceParityError> {
        if self.schema != SURFACE_PARITY_SCHEMA {
            return Err(SurfaceParityError::TraceInvalid("schema"));
        }
        if self.command_id.trim().is_empty() || self.command_id.len() > 256 {
            return Err(SurfaceParityError::TraceInvalid("command_id"));
        }
        if self.operation.trim().is_empty() || self.operation.len() > 256 {
            return Err(SurfaceParityError::TraceInvalid("operation"));
        }
        if !matches!(
            self.disposition.as_str(),
            "accepted" | "applied" | "rejected" | "unknown"
        ) {
            return Err(SurfaceParityError::TraceInvalid("disposition"));
        }
        if !matches!(
            self.retry.as_str(),
            "query_original" | "safe_retry" | "do_not_retry"
        ) {
            return Err(SurfaceParityError::TraceInvalid("retry"));
        }
        if self.cursor_epoch.trim().is_empty()
            || self.cursor_epoch.len() > 256
            || self.cursor_sequence == 0
        {
            return Err(SurfaceParityError::TraceInvalid("cursor"));
        }
        if self.revision == 0 || self.sensitive_field_count > 0 || self.limitations.len() > 32 {
            return if self.sensitive_field_count > 0 {
                Err(SurfaceParityError::SensitiveFields)
            } else {
                Err(SurfaceParityError::TraceInvalid("revision_or_limitations"))
            };
        }
        if self.disposition == "applied" && self.receipt_digest.is_none() {
            return Err(SurfaceParityError::TraceInvalid("receipt"));
        }
        Ok(())
    }
}

pub fn compare_surface_traces(
    traces: &[SurfaceTrace],
) -> Result<SurfaceParityReport, SurfaceParityError> {
    if traces.is_empty() {
        return Err(SurfaceParityError::Empty);
    }
    for trace in traces {
        trace.validate()?;
    }
    let mut surfaces = BTreeSet::new();
    for trace in traces {
        if !surfaces.insert(trace.surface) {
            return Err(SurfaceParityError::DuplicateSurface);
        }
    }
    if REQUIRED_SURFACES
        .iter()
        .any(|surface| !surfaces.contains(surface))
    {
        return Err(SurfaceParityError::SurfaceMissing);
    }
    let first = &traces[0];
    for trace in &traces[1..] {
        if trace.command_id != first.command_id {
            return Err(SurfaceParityError::Mismatch("command_id"));
        }
        if trace.operation != first.operation {
            return Err(SurfaceParityError::Mismatch("operation"));
        }
        if trace.disposition != first.disposition {
            return Err(SurfaceParityError::Mismatch("disposition"));
        }
        if trace.retry != first.retry {
            return Err(SurfaceParityError::Mismatch("retry"));
        }
        if trace.cursor_epoch != first.cursor_epoch
            || trace.cursor_sequence != first.cursor_sequence
        {
            return Err(SurfaceParityError::Mismatch("cursor"));
        }
        if trace.revision != first.revision {
            return Err(SurfaceParityError::Mismatch("revision"));
        }
        if trace.receipt_digest != first.receipt_digest {
            return Err(SurfaceParityError::Mismatch("receipt"));
        }
        if trace.error_code != first.error_code {
            return Err(SurfaceParityError::Mismatch("error_code"));
        }
    }
    let limitations = traces
        .iter()
        .flat_map(|trace| trace.limitations.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(SurfaceParityReport {
        schema: SURFACE_PARITY_SCHEMA.to_owned(),
        command_id: first.command_id.clone(),
        operation: first.operation.clone(),
        disposition: first.disposition.clone(),
        retry: first.retry.clone(),
        cursor_epoch: first.cursor_epoch.clone(),
        cursor_sequence: first.cursor_sequence,
        revision: first.revision,
        receipt_digest: first.receipt_digest.clone(),
        surfaces: surfaces.into_iter().collect(),
        limitations,
    })
}
