//! Common, bounded result metadata for daemon-owned adapters.
//!
//! The envelope is deliberately digest-only. Adapter payloads remain in the redacted
//! `CapabilityResult`/EventLog projection, while process/effect/stop and the adapter's own
//! commit boundary are represented once for all handler families.

use crate::{
    json_digest, validate_json_limits, CapabilityEffectState, CapabilityProcessState,
    CapabilityResult, CapabilityStopState, RequestId, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const ADAPTER_RESULT_SCHEMA: &str = "kiana.adapter-result.v1";
pub const ADAPTER_RESULT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const ADAPTER_RESULT_MAX_OUTPUT_BYTES: usize = 512 * 1024;
pub const ADAPTER_RESULT_MAX_EVIDENCE: usize = 256;

/// Which bounded adapter produced the result. `Shell` is included so the legacy five-tool
/// surface and the specialized Hook/MCP/Memory/Patch handlers share the same contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterResultKind {
    Hook,
    Shell,
    Patch,
    Mcp,
    Memory,
}

/// The adapter-local persistence boundary, not the global EventLog commit. `Unknown` means the
/// adapter may have started work but cannot prove its own output/effect boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterCommitState {
    NotStarted,
    Committed,
    Unknown,
}

/// Digest-only metadata shared by all handler families before ControlPlane result commit.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterResult {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub adapter: AdapterResultKind,
    pub commit: AdapterCommitState,
    pub process: CapabilityProcessState,
    pub stop: CapabilityStopState,
    pub effect: CapabilityEffectState,
    pub output_bytes: u64,
    pub output_digest: String,
    pub result_digest: String,
    #[serde(default)]
    pub evidence_ref_digests: Vec<String>,
    pub reconciliation_required: bool,
    pub adapter_digest: String,
}

impl AdapterResult {
    /// Derive the envelope from a normalized adapter result without copying its raw output.
    pub fn from_result(
        result: &CapabilityResult,
        adapter: AdapterResultKind,
        commit: AdapterCommitState,
    ) -> Result<Self, String> {
        let dimensions = result.dimensions();
        let reconciliation_required = dimensions.effect == CapabilityEffectState::Unknown
            || dimensions.stop == CapabilityStopState::Unconfirmed
            || result
                .failure_code()
                .is_some_and(|code| code.policy().requires_reconciliation);
        let result_digest =
            json_digest(&serde_json::to_value(result).map_err(|_| "adapter_result_encode_failed")?);
        Self::from_parts(
            result.request_id,
            adapter,
            commit,
            dimensions.process,
            dimensions.stop,
            dimensions.effect,
            &result.output,
            result_digest,
            &result.evidence_refs,
            reconciliation_required,
        )
    }

    /// Build metadata for a pre-tool Hook, whose decision is not itself a capability result.
    /// `result_digest` is a digest of metadata and the bounded redacted output, never raw output.
    pub fn from_observation(
        request_id: RequestId,
        adapter: AdapterResultKind,
        commit: AdapterCommitState,
        process: CapabilityProcessState,
        stop: CapabilityStopState,
        effect: CapabilityEffectState,
        output: &Value,
        evidence_refs: &[String],
        reconciliation_required: bool,
    ) -> Result<Self, String> {
        let output_digest = json_digest(output);
        let result_digest = json_digest(&json!({
            "request_id": request_id,
            "adapter": adapter,
            "process": process,
            "stop": stop,
            "effect": effect,
            "output_digest": output_digest,
        }));
        Self::from_parts(
            request_id,
            adapter,
            commit,
            process,
            stop,
            effect,
            output,
            result_digest,
            evidence_refs,
            reconciliation_required,
        )
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let result: Self = serde_json::from_value(value.clone())
            .map_err(|_| "adapter_result_decode_failed".to_owned())?;
        result.validate()?;
        Ok(result)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "adapter_result_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ADAPTER_RESULT_SCHEMA
            || !self.version.is_compatible_with(&ADAPTER_RESULT_VERSION)
            || self.request_id.as_uuid().is_nil()
            || self.output_bytes > ADAPTER_RESULT_MAX_OUTPUT_BYTES as u64
            || !valid_digest(&self.output_digest)
            || !valid_digest(&self.result_digest)
            || !valid_digest(&self.adapter_digest)
            || self.evidence_ref_digests.len() > ADAPTER_RESULT_MAX_EVIDENCE
            || self
                .evidence_ref_digests
                .iter()
                .any(|digest| !valid_digest(digest))
            || self
                .evidence_ref_digests
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("adapter_result_header_invalid".to_owned());
        }
        if self.process == CapabilityProcessState::NotStarted
            && self.effect != CapabilityEffectState::NotStarted
        {
            return Err("adapter_result_process_effect_conflict".to_owned());
        }
        if self.commit == AdapterCommitState::NotStarted
            && self.process != CapabilityProcessState::NotStarted
        {
            return Err("adapter_result_commit_started_conflict".to_owned());
        }
        if self.effect == CapabilityEffectState::Unknown && !self.reconciliation_required {
            return Err("adapter_result_unknown_not_fenced".to_owned());
        }
        if self.commit == AdapterCommitState::Unknown && !self.reconciliation_required {
            return Err("adapter_result_commit_unknown_not_fenced".to_owned());
        }
        if self.adapter_digest != self.digest() {
            return Err("adapter_result_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_id": self.request_id,
            "adapter": self.adapter,
            "commit": self.commit,
            "process": self.process,
            "stop": self.stop,
            "effect": self.effect,
            "output_bytes": self.output_bytes,
            "output_digest": self.output_digest,
            "result_digest": self.result_digest,
            "evidence_ref_digests": self.evidence_ref_digests,
            "reconciliation_required": self.reconciliation_required,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        request_id: RequestId,
        adapter: AdapterResultKind,
        commit: AdapterCommitState,
        process: CapabilityProcessState,
        stop: CapabilityStopState,
        effect: CapabilityEffectState,
        output: &Value,
        result_digest: String,
        evidence_refs: &[String],
        reconciliation_required: bool,
    ) -> Result<Self, String> {
        validate_json_limits(output).map_err(|_| "adapter_result_output_limit".to_owned())?;
        let encoded = serde_json::to_vec(output)
            .map_err(|_| "adapter_result_output_encode_failed".to_owned())?;
        if encoded.len() > ADAPTER_RESULT_MAX_OUTPUT_BYTES {
            return Err("adapter_result_output_limit".to_owned());
        }
        if evidence_refs.len() > ADAPTER_RESULT_MAX_EVIDENCE
            || evidence_refs
                .iter()
                .any(|reference| reference.len() > 4096 || reference.contains('\0'))
        {
            return Err("adapter_result_evidence_limit".to_owned());
        }
        let mut evidence_ref_digests = evidence_refs
            .iter()
            .map(|reference| json_digest(&json!(reference)))
            .collect::<Vec<_>>();
        evidence_ref_digests.sort();
        evidence_ref_digests.dedup();
        let mut result = Self {
            schema: ADAPTER_RESULT_SCHEMA.to_owned(),
            version: ADAPTER_RESULT_VERSION,
            request_id,
            adapter,
            commit,
            process,
            stop,
            effect,
            output_bytes: encoded.len() as u64,
            output_digest: json_digest(output),
            result_digest,
            evidence_ref_digests,
            reconciliation_required,
            adapter_digest: String::new(),
        };
        result.adapter_digest = result.digest();
        result.validate()?;
        Ok(result)
    }
}

/// Add the common envelope to an object-shaped CapabilityResult. The original payload is kept
/// intact for compatibility; only digest/state metadata is added under `adapter_result`.
pub fn attach_adapter_result(
    mut result: CapabilityResult,
    adapter: AdapterResultKind,
    commit: AdapterCommitState,
) -> Result<CapabilityResult, String> {
    if !result.output.is_object() {
        return Err("adapter_result_object_required".to_owned());
    }
    let envelope = AdapterResult::from_result(&result, adapter, commit)?;
    result.output["adapter_result"] = envelope.to_json()?;
    Ok(result)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
