//! Bounded, run-scoped references for complete tool output.
//!
//! Output bytes are stored outside the event/model payload. This reference carries only the
//! identity, integrity, ownership scope and expiry needed by the controlled read operation.

use crate::{json_digest, InvocationId, RequestId, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const EXECUTION_OUTPUT_REF_SCHEMA: &str = "kiana.execution-output-ref.v1";
pub const EXECUTION_OUTPUT_REF_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EXECUTION_OUTPUT_REF_BYTES: u64 = 16 * 1024 * 1024;
pub const EXECUTION_OUTPUT_BUDGET_SCHEMA: &str = "kiana.execution-output-budget.v1";

/// Separate collection, preview, persistence and observation limits for process output.
///
/// A preview limit is not permission to discard the persisted artifact, and an observed limit
/// is a hard stop for the reader rather than an unbounded diagnostic allowance.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutputBudget {
    pub schema: String,
    pub collect_max_bytes: usize,
    pub preview_max_bytes: usize,
    pub persist_max_bytes: usize,
    pub observed_max_bytes: usize,
    pub max_lines: usize,
}

impl Default for ExecutionOutputBudget {
    fn default() -> Self {
        Self {
            schema: EXECUTION_OUTPUT_BUDGET_SCHEMA.to_owned(),
            collect_max_bytes: 1024 * 1024,
            preview_max_bytes: 8 * 1024,
            persist_max_bytes: 4 * 1024 * 1024,
            observed_max_bytes: 32 * 1024 * 1024,
            max_lines: 1_000_000,
        }
    }
}

impl ExecutionOutputBudget {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_OUTPUT_BUDGET_SCHEMA
            || self.collect_max_bytes == 0
            || self.preview_max_bytes == 0
            || self.persist_max_bytes < self.collect_max_bytes
            || self.observed_max_bytes < self.collect_max_bytes
            || self.max_lines == 0
            || self.collect_max_bytes > MAX_EXECUTION_OUTPUT_REF_BYTES as usize
            || self.persist_max_bytes > MAX_EXECUTION_OUTPUT_REF_BYTES as usize
        {
            return Err("execution_output_budget_invalid".to_owned());
        }
        Ok(())
    }
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// A protected complete-output reference. The reference itself is not an authority to read;
/// ControlPlane/Broker still performs the operation and the adapter rechecks its scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutputRef {
    pub schema: String,
    pub version: SchemaVersion,
    pub output_id: RequestId,
    pub run_id: Option<RunId>,
    pub invocation_id: InvocationId,
    pub content_digest: String,
    pub size_bytes: u64,
    pub scope_digest: String,
    pub expires_at_unix_ms: u64,
    pub reference_digest: String,
}

impl ExecutionOutputRef {
    pub fn new(
        output_id: RequestId,
        run_id: Option<RunId>,
        invocation_id: InvocationId,
        content_digest: impl Into<String>,
        size_bytes: u64,
        scope_digest: impl Into<String>,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut reference = Self {
            schema: EXECUTION_OUTPUT_REF_SCHEMA.to_owned(),
            version: EXECUTION_OUTPUT_REF_VERSION,
            output_id,
            run_id,
            invocation_id,
            content_digest: content_digest.into(),
            size_bytes,
            scope_digest: scope_digest.into(),
            expires_at_unix_ms,
            reference_digest: String::new(),
        };
        reference.reference_digest = reference.digest();
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_OUTPUT_REF_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_OUTPUT_REF_VERSION)
            || self.output_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.invocation_id != InvocationId::from_uuid(self.output_id.as_uuid())
            || self.size_bytes > MAX_EXECUTION_OUTPUT_REF_BYTES
            || self.expires_at_unix_ms == 0
        {
            return Err("execution_output_ref_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.content_digest, "execution_output_content_digest"),
            (&self.scope_digest, "execution_output_scope_digest"),
            (&self.reference_digest, "execution_output_reference_digest"),
        ] {
            valid_digest(value, field)?;
        }
        if self.reference_digest != self.digest() {
            return Err("execution_output_reference_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "output_id": self.output_id,
            "run_id": self.run_id,
            "invocation_id": self.invocation_id,
            "content_digest": self.content_digest,
            "size_bytes": self.size_bytes,
            "scope_digest": self.scope_digest,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}
