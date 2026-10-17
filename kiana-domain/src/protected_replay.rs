//! Bounded, reference-only provider replay material.
//!
//! Provider reasoning/signature bytes are private wire material. This module keeps them in an
//! in-process bounded store and exposes only a digest-bound reference to receipts, EventLog and
//! protocol projections. A durable adapter may replace the store later; scope checks remain the
//! same.
//!
//! Two shapes exist on purpose and must not be conflated:
//!
//! * [`PublicReasoningSummary`] is digest-only metadata that may be serialized, logged or shown
//!   in a receipt. It can never be turned back into provider continuation material.
//! * [`ProtectedReplayMaterial`] holds the exact protocol bytes. It is deliberately not
//!   serializable, redacts itself in `Debug` output and is only readable through scope checks.

use crate::{
    json_digest, ModelContent, ModelError, ModelMessage, ModelProtocol, ModelRoute,
    ProtectedReplayRef, ProviderContinuation, RequestId,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const PROTECTED_REPLAY_SCHEMA: &str = "kiana.protected-replay.v1";
pub const PUBLIC_REPLAY_SUMMARY_SCHEMA: &str = "kiana.protected-replay-summary.v1";
/// Redaction marker: a digest is evidence that material existed, not a copy of it.
pub const PUBLIC_REPLAY_REDACTION: &str = "digest_only_not_lossless";
const MAX_MATERIAL_BYTES: usize = 512 * 1024;
const MAX_ENTRIES: usize = 32;

/// Which provider dialect a piece of private continuation material belongs to.
///
/// The variant fixes the only protocol that may consume the bytes again. Cross dialect reuse is
/// rejected by [`ProtectedReplayRef::validate_for_call`] and by the provider codecs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningReplayKind {
    /// Anthropic `thinking` / `redacted_thinking` block including its signature.
    AnthropicThinkingSignature,
    /// OpenAI Responses `reasoning` item (`encrypted_content` / item id).
    OpenAiResponsesReasoningItem,
    /// DeepSeek `message.reasoning_content` (also used by compatible Chat endpoints).
    DeepSeekReasoningContent,
    /// Gemini Interactions `thought` / `thought_summary` step including its signature.
    GeminiThoughtSignature,
}

impl ReasoningReplayKind {
    /// The protocol that owns this private dialect. Replay never crosses it.
    pub const fn protocol(self) -> ModelProtocol {
        match self {
            Self::AnthropicThinkingSignature => ModelProtocol::AnthropicMessages,
            Self::OpenAiResponsesReasoningItem => ModelProtocol::OpenAiResponses,
            Self::DeepSeekReasoningContent => ModelProtocol::OpenAiChat,
            Self::GeminiThoughtSignature => ModelProtocol::GeminiInteractions,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicThinkingSignature => "anthropic_thinking_signature",
            Self::OpenAiResponsesReasoningItem => "openai_responses_reasoning_item",
            Self::DeepSeekReasoningContent => "deepseek_reasoning_content",
            Self::GeminiThoughtSignature => "gemini_thought_signature",
        }
    }

    /// Protocols without a private replay dialect (Ollama, Legacy) have no kind.
    pub const fn for_protocol(protocol: ModelProtocol) -> Option<Self> {
        match protocol {
            ModelProtocol::AnthropicMessages => Some(Self::AnthropicThinkingSignature),
            ModelProtocol::OpenAiResponses => Some(Self::OpenAiResponsesReasoningItem),
            ModelProtocol::OpenAiChat => Some(Self::DeepSeekReasoningContent),
            ModelProtocol::GeminiInteractions => Some(Self::GeminiThoughtSignature),
            ModelProtocol::OllamaChat | ModelProtocol::Legacy => None,
        }
    }
}

/// Public, digest-bound projection of private replay material.
///
/// Safe for receipts, EventLog payloads and diagnostics: it carries no provider bytes and cannot
/// be used to resume a call. Treating a summary as lossless recovery is refused with
/// `public_replay_summary_is_not_resume_material`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicReasoningSummary {
    pub schema: String,
    pub kind: ReasoningReplayKind,
    pub artifact_id: String,
    pub material_sha256: String,
    pub material_bytes: u64,
    pub connection_id: String,
    pub model_id: String,
    pub route_digest: String,
    pub source_call_id: RequestId,
    pub expires_at_unix_ms: u64,
    pub redaction: String,
}

impl PublicReasoningSummary {
    /// Public projection of one private material entry. The digest and byte count are evidence
    /// that some material exists; the bytes themselves never leave the protected store.
    pub fn from_material(
        material: &ProtectedReplayMaterial,
        artifact_id: impl Into<String>,
    ) -> Result<Self, ModelError> {
        let kind = material
            .kind()
            .ok_or_else(|| ModelError::invalid("protected_replay_kind_unknown"))?;
        let artifact_id = artifact_id.into();
        Ok(Self {
            schema: PUBLIC_REPLAY_SUMMARY_SCHEMA.to_owned(),
            kind,
            artifact_id: artifact_id.clone(),
            material_sha256: material.digest(),
            material_bytes: material.bytes().len() as u64,
            connection_id: material.scope.connection_id.clone(),
            model_id: material.scope.model_id.clone(),
            route_digest: material.scope.route_digest.clone(),
            source_call_id: material.scope.source_call_id,
            expires_at_unix_ms: material.scope.expires_at_unix_ms,
            redaction: PUBLIC_REPLAY_REDACTION.to_owned(),
        })
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != PUBLIC_REPLAY_SUMMARY_SCHEMA
            || self.artifact_id.trim().is_empty()
            || self.material_sha256.trim().is_empty()
            || self.material_bytes == 0
            || self.connection_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.route_digest.trim().is_empty()
            || self.expires_at_unix_ms == 0
            || self.redaction != PUBLIC_REPLAY_REDACTION
        {
            return Err(ModelError::invalid("public_replay_summary_invalid"));
        }
        Ok(())
    }

    /// A summary is audit evidence, never continuation input.
    pub fn resume(&self) -> Result<(), ModelError> {
        self.validate()?;
        Err(ModelError::invalid(
            "public_replay_summary_is_not_resume_material",
        ))
    }
}

impl ProtectedReplayRef {
    /// Validate the reference envelope before any provider codec is allowed to use it.
    pub fn validate_for_call(
        &self,
        route: &ModelRoute,
        call_id: RequestId,
        deadline_unix_ms: u64,
    ) -> Result<(), ModelError> {
        if self.artifact_id.trim().is_empty()
            || self.sha256.trim().is_empty()
            || self.route_digest != route.digest()
            || self.connection_id != route.connection_id
            || self.protocol != Some(route.protocol)
            || self.model_id != route.model_id
            || self.source_call_id != call_id
            || self.expires_at_unix_ms == 0
            || self.expires_at_unix_ms > deadline_unix_ms
            || self.prompt_digest.trim().is_empty()
            || self.tool_catalog_digest.trim().is_empty()
            || self.data_revision.trim().is_empty()
        {
            return Err(ModelError::invalid("protected_replay_reference_invalid"));
        }
        if let Some(kind) = self.kind {
            if kind.protocol() != route.protocol {
                return Err(ModelError::invalid(
                    "protected_replay_kind_protocol_mismatch",
                ));
            }
            if self
                .effort
                .as_ref()
                .is_some_and(|effort| effort.trim().is_empty())
            {
                return Err(ModelError::invalid("protected_replay_effort_invalid"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedReplayScope {
    pub connection_id: String,
    pub protocol: ModelProtocol,
    pub model_id: String,
    pub route_digest: String,
    pub effort: Option<String>,
    pub prompt_digest: String,
    pub tool_catalog_digest: String,
    pub data_revision: String,
    pub source_call_id: RequestId,
    pub expires_at_unix_ms: u64,
}

impl ProtectedReplayScope {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.connection_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.route_digest.trim().is_empty()
            || self.prompt_digest.trim().is_empty()
            || self.tool_catalog_digest.trim().is_empty()
            || self.data_revision.trim().is_empty()
            || self.expires_at_unix_ms == 0
        {
            return Err(ModelError::invalid("protected_replay_scope_invalid"));
        }
        if self
            .effort
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ModelError::invalid("protected_replay_effort_invalid"));
        }
        Ok(())
    }

    pub fn matches_route(&self, route: &ModelRoute) -> bool {
        self.connection_id == route.connection_id
            && self.protocol == route.protocol
            && self.model_id == route.model_id
            && self.route_digest == route.digest()
    }

    pub fn reference(&self, artifact_id: impl Into<String>, bytes: &[u8]) -> ProtectedReplayRef {
        ProtectedReplayRef {
            artifact_id: artifact_id.into(),
            sha256: json_digest(&json!({"bytes": bytes})),
            route_digest: self.route_digest.clone(),
            source_call_id: self.source_call_id,
            expires_at_unix_ms: self.expires_at_unix_ms,
            connection_id: self.connection_id.clone(),
            protocol: Some(self.protocol),
            model_id: self.model_id.clone(),
            effort: self.effort.clone(),
            prompt_digest: self.prompt_digest.clone(),
            tool_catalog_digest: self.tool_catalog_digest.clone(),
            data_revision: self.data_revision.clone(),
            kind: None,
        }
    }

    /// Reference that additionally declares the private dialect it binds.
    pub fn reference_for_kind(
        &self,
        artifact_id: impl Into<String>,
        kind: ReasoningReplayKind,
        bytes: &[u8],
    ) -> ProtectedReplayRef {
        let mut reference = self.reference(artifact_id, bytes);
        reference.kind = Some(kind);
        reference
    }
}

/// Private bytes are intentionally not serializable or included in Debug output.
#[derive(Clone, Eq, PartialEq)]
pub struct ProtectedReplayMaterial {
    pub scope: ProtectedReplayScope,
    kind: Option<ReasoningReplayKind>,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for ProtectedReplayMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProtectedReplayMaterial")
            .field("scope", &self.scope)
            .field("kind", &self.kind)
            .field("byte_len", &self.bytes.len())
            .finish()
    }
}

impl ProtectedReplayMaterial {
    pub fn new(scope: ProtectedReplayScope, bytes: impl Into<Vec<u8>>) -> Result<Self, ModelError> {
        Self::new_with_kind(scope, None, bytes)
    }

    /// Material bound to one provider dialect. `kind` must match `scope.protocol`.
    pub fn new_with_kind(
        scope: ProtectedReplayScope,
        kind: Option<ReasoningReplayKind>,
        bytes: impl Into<Vec<u8>>,
    ) -> Result<Self, ModelError> {
        scope.validate()?;
        if let Some(kind) = kind {
            if kind.protocol() != scope.protocol {
                return Err(ModelError::invalid(
                    "protected_replay_kind_protocol_mismatch",
                ));
            }
        }
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_MATERIAL_BYTES {
            return Err(ModelError::invalid(
                "protected_replay_material_size_invalid",
            ));
        }
        Ok(Self { scope, kind, bytes })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn kind(&self) -> Option<ReasoningReplayKind> {
        self.kind
            .or_else(|| ReasoningReplayKind::for_protocol(self.scope.protocol))
    }

    pub fn declared_kind(&self) -> Option<ReasoningReplayKind> {
        self.kind
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({"bytes": self.bytes}))
    }

    /// Digest-only projection that may leave the protected boundary.
    pub fn public_summary(
        &self,
        artifact_id: impl Into<String>,
    ) -> Result<PublicReasoningSummary, ModelError> {
        PublicReasoningSummary::from_material(self, artifact_id)
    }

    pub fn reference(&self, artifact_id: impl Into<String>) -> ProtectedReplayRef {
        self.scope.reference(artifact_id, &self.bytes)
    }

    pub fn reference_for_kind(&self, artifact_id: impl Into<String>) -> ProtectedReplayRef {
        match self.kind {
            Some(kind) => self
                .scope
                .reference_for_kind(artifact_id, kind, &self.bytes),
            None => self.reference(artifact_id),
        }
    }

    pub fn into_bytes(mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }

    pub fn validate_for(
        &self,
        reference: &ProtectedReplayRef,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<(), ModelError> {
        if now_unix_ms >= reference.expires_at_unix_ms {
            return Err(ModelError::invalid("protected_replay_expired"));
        }
        if reference.expires_at_unix_ms > self.scope.expires_at_unix_ms {
            return Err(ModelError::invalid("protected_replay_ttl_widened"));
        }
        let expected = self.scope.reference(&reference.artifact_id, &self.bytes);
        if !self.scope.matches_route(route)
            || reference.route_digest != route.digest()
            || reference.connection_id != route.connection_id
            || reference.protocol != Some(route.protocol)
            || reference.model_id != route.model_id
            || reference.sha256 != expected.sha256
            || reference.prompt_digest != expected.prompt_digest
            || reference.tool_catalog_digest != expected.tool_catalog_digest
            || reference.data_revision != expected.data_revision
            || reference.effort != expected.effort
            || reference.source_call_id != expected.source_call_id
        {
            return Err(ModelError::invalid(
                "replay_material_cannot_cross_connection_or_model_scope",
            ));
        }
        if let Some(kind) = reference.kind {
            if kind.protocol() != route.protocol {
                return Err(ModelError::invalid(
                    "protected_replay_kind_protocol_mismatch",
                ));
            }
            if self.kind.is_some_and(|material_kind| material_kind != kind) {
                return Err(ModelError::invalid("protected_replay_kind_mismatch"));
            }
        }
        Ok(())
    }
}

/// Read side used by provider codecs. A deployment without any adapter must refuse to compile a
/// replay request instead of guessing at private bytes.
pub trait ProtectedReplaySource {
    fn resolve_material(
        &self,
        reference: &ProtectedReplayRef,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<ProtectedReplayMaterial, ModelError>;
}

/// Process-local store used when a deployment has no durable protected artifact service.
/// Deleting, revoking or switching the model route invalidates all future reads instead of
/// leaving a stale resume projection.
#[derive(Default)]
pub struct ProtectedReplayStore {
    entries: BTreeMap<String, ProtectedReplayMaterial>,
    revoked: BTreeMap<String, String>,
}

impl ProtectedReplayStore {
    pub fn insert(
        &mut self,
        artifact_id: impl Into<String>,
        material: ProtectedReplayMaterial,
    ) -> Result<ProtectedReplayRef, ModelError> {
        if self.entries.len() >= MAX_ENTRIES {
            return Err(ModelError::invalid("protected_replay_store_full"));
        }
        let artifact_id = artifact_id.into();
        if artifact_id.trim().is_empty() || self.entries.contains_key(&artifact_id) {
            return Err(ModelError::invalid("protected_replay_artifact_conflict"));
        }
        let reference = material.reference_for_kind(&artifact_id);
        self.revoked.remove(&artifact_id);
        self.entries.insert(artifact_id, material);
        Ok(reference)
    }

    pub fn get(
        &self,
        reference: &ProtectedReplayRef,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<&ProtectedReplayMaterial, ModelError> {
        if self.revoked.contains_key(&reference.artifact_id) {
            return Err(ModelError::invalid("replay_material_revoked"));
        }
        let material = self
            .entries
            .get(&reference.artifact_id)
            .ok_or_else(|| ModelError::invalid("missing_reasoning_material_blocks_resume"))?;
        material.validate_for(reference, route, now_unix_ms)?;
        Ok(material)
    }

    pub fn delete(&mut self, artifact_id: &str) -> bool {
        self.revoked.remove(artifact_id);
        self.entries.remove(artifact_id).is_some()
    }

    /// Explicit revocation (operator action, model switch, data-governance event). Bytes are
    /// dropped immediately and every later read fails with the recorded reason.
    pub fn revoke(&mut self, artifact_id: &str, reason: &str) -> Result<(), ModelError> {
        if artifact_id.trim().is_empty() || reason.trim().is_empty() {
            return Err(ModelError::invalid("protected_replay_revocation_invalid"));
        }
        if !self.entries.contains_key(artifact_id) && !self.revoked.contains_key(artifact_id) {
            return Err(ModelError::invalid("protected_replay_artifact_unknown"));
        }
        self.entries.remove(artifact_id);
        self.revoked
            .insert(artifact_id.to_owned(), reason.to_owned());
        Ok(())
    }

    pub fn revocation_reason(&self, artifact_id: &str) -> Option<&str> {
        self.revoked.get(artifact_id).map(String::as_str)
    }

    /// Revoke every entry that no longer matches the resolved route. Used when the model,
    /// connection or configuration revision changes: the old signature must not be replayed.
    pub fn invalidate_route_change(&mut self, route: &ModelRoute) -> usize {
        let stale = self
            .entries
            .iter()
            .filter(|(_, material)| !material.scope.matches_route(route))
            .map(|(artifact_id, _)| artifact_id.clone())
            .collect::<BTreeSet<_>>();
        for artifact_id in &stale {
            self.entries.remove(artifact_id);
            self.revoked
                .insert(artifact_id.clone(), "model_switch".to_owned());
        }
        stale.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl ProtectedReplaySource for ProtectedReplayStore {
    fn resolve_material(
        &self,
        reference: &ProtectedReplayRef,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<ProtectedReplayMaterial, ModelError> {
        self.get(reference, route, now_unix_ms).cloned()
    }
}

/// History/compaction boundary.
///
/// A compacted view is a lossy public summary. Provider continuation references must stay out of
/// it: they are recorded as digest-only notices so the caller can refuse to resume instead of
/// silently continuing from a placeholder.
#[derive(Clone, Debug, Default)]
pub struct ProtectedHistoryBoundary {
    /// Messages with no protected continuation; only these may be summarized.
    pub compactable: Vec<ModelMessage>,
    /// Continuation references that compaction would drop.
    pub protected: Vec<ProviderContinuation>,
    /// Digest-only notices for receipts and diagnostics.
    pub notices: Vec<String>,
}

impl ProtectedHistoryBoundary {
    pub fn split(messages: &[ModelMessage]) -> Self {
        let mut boundary = Self::default();
        for message in messages {
            let protected = message.continuation.is_some()
                || message
                    .content
                    .iter()
                    .any(|content| matches!(content, ModelContent::ProviderOpaque { .. }));
            if !protected {
                boundary.compactable.push(message.clone());
                continue;
            }
            if let Some(continuation) = &message.continuation {
                boundary.notices.push(json_digest(&json!({
                    "item_ref": continuation.item_ref,
                    "protocol": continuation.protocol,
                })));
                boundary.protected.push(continuation.clone());
            }
        }
        boundary
    }

    /// Compaction may only run on the compactable subset.
    pub fn compaction_input(&self) -> &[ModelMessage] {
        &self.compactable
    }

    /// Resuming a dropped provider continuation from the compacted view is refused explicitly.
    pub fn resume_after_compaction(&self) -> Result<(), ModelError> {
        if self.protected.is_empty() {
            return Ok(());
        }
        Err(ModelError::invalid(
            "protected_continuation_not_recoverable_from_compacted_view",
        ))
    }
}
