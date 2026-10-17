//! Provider-side protected replay codecs.
//!
//! Two directions, one scope contract:
//!
//! * **capture** reads the private continuation material a provider produced (Anthropic
//!   `thinking`/`redacted_thinking` signature, OpenAI Responses `reasoning` item, DeepSeek
//!   `reasoning_content`, Gemini Interactions `thought` step) and hands it to the caller as
//!   opaque bytes. The caller binds scope/TTL and stores it through a protected artifact
//!   adapter; the provider never invents scope, TTL or persistence.
//! * **injection** puts that material back into the next request of the same connection, model,
//!   protocol and effort, byte for byte, so a tool continuation keeps the provider signature.
//!
//! Private material is not serializable, redacts itself in `Debug`, and is refused unless a
//! protected source resolves it for the exact route. A deployment without such a source keeps
//! the existing fail-closed `model_replay_storage_unavailable` behaviour.
//!
//! A short-lived envelope ([`RingReplaySealer`]) is the durable form: ChaCha20-Poly1305 keyed by
//! caller-supplied key material, bound to the artifact metadata as associated data. The key is
//! never written to the repository, an event or a receipt.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use kiana_domain::{
    json_digest, ModelCallSpec, ModelError, ModelProtocol, ModelRequest, ModelRoute,
    ProtectedReplayMaterial, ProtectedReplayRef, ProtectedReplayScope, ProtectedReplaySource,
    PublicReasoningSummary, ReasoningReplayKind, RequestId,
};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, CHACHA20_POLY1305, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const SEALED_REPLAY_ARTIFACT_SCHEMA: &str = "kiana.protected-replay-sealed.v1";
const MAX_CAPTURE_BYTES: usize = 512 * 1024;
const MAX_CAPTURE_ITEMS: usize = 64;
const MAX_SEALED_ENTRIES: usize = 64;

/// Private wire material captured from one provider response.
///
/// Intentionally not `Serialize`, and `Debug` never prints the payload. Use
/// [`CapturedReplay::encode`] when the bytes have to cross into a protected store.
#[derive(Clone, PartialEq)]
pub struct CapturedReplay {
    kind: ReasoningReplayKind,
    items: Vec<Value>,
}

impl std::fmt::Debug for CapturedReplay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CapturedReplay")
            .field("kind", &self.kind)
            .field("items", &self.items.len())
            .field("redacted", &true)
            .finish()
    }
}

impl CapturedReplay {
    pub fn new(kind: ReasoningReplayKind, items: Vec<Value>) -> Result<Self, ModelError> {
        if items.is_empty() || items.len() > MAX_CAPTURE_ITEMS {
            return Err(ModelError::invalid("protected_replay_capture_size_invalid"));
        }
        Ok(Self { kind, items })
    }

    pub fn kind(&self) -> ReasoningReplayKind {
        self.kind
    }

    pub fn items(&self) -> &[Value] {
        &self.items
    }

    /// Canonical private payload. The caller may persist this only through a protected adapter.
    pub fn encode(&self) -> Result<Vec<u8>, ModelError> {
        let bytes = serde_json::to_vec(&self.items)
            .map_err(|_| ModelError::invalid("protected_replay_capture_encode_failed"))?;
        if bytes.len() > MAX_CAPTURE_BYTES {
            return Err(ModelError::invalid("protected_replay_capture_size_invalid"));
        }
        Ok(bytes)
    }

    pub fn decode(kind: ReasoningReplayKind, bytes: &[u8]) -> Result<Self, ModelError> {
        let items = serde_json::from_slice::<Vec<Value>>(bytes)
            .map_err(|_| ModelError::invalid("protected_replay_capture_decode_failed"))?;
        Self::new(kind, items)
    }

    /// Bind the captured bytes to an explicit caller-owned scope. The provider does not choose
    /// TTL, effort or prompt revision; it only refuses to cross the protocol boundary.
    pub fn material(
        self,
        scope: ProtectedReplayScope,
    ) -> Result<ProtectedReplayMaterial, ModelError> {
        if scope.protocol != self.kind.protocol() {
            return Err(ModelError::invalid(
                "protected_replay_kind_protocol_mismatch",
            ));
        }
        ProtectedReplayMaterial::new_with_kind(scope, Some(self.kind), self.encode()?)
    }
}

/// Bounded capture buffer used by the streaming accumulator.
///
/// Disabled by default so a deployment without a protected adapter keeps refusing private
/// material instead of silently dropping it.
#[derive(Clone, Debug, Default)]
pub struct ReplayCaptureBuffer {
    enabled: bool,
    protocol: Option<ModelProtocol>,
    items: BTreeMap<usize, Value>,
    bytes: usize,
}

impl ReplayCaptureBuffer {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn enabled(protocol: ModelProtocol) -> Self {
        Self {
            enabled: true,
            protocol: Some(protocol),
            ..Self::default()
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn protocol(&self) -> Option<ModelProtocol> {
        self.protocol
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn record(&mut self, index: usize, item: Value) -> Result<(), ModelError> {
        if !self.enabled {
            return Err(ModelError::invalid("protected_replay_capture_disabled"));
        }
        if self.items.len() >= MAX_CAPTURE_ITEMS {
            return Err(ModelError::invalid("protected_replay_capture_size_invalid"));
        }
        self.bytes = self
            .bytes
            .saturating_add(serde_json::to_string(&item).map_or(MAX_CAPTURE_BYTES, |v| v.len()));
        if self.bytes > MAX_CAPTURE_BYTES {
            return Err(ModelError::invalid("protected_replay_capture_size_invalid"));
        }
        self.items.insert(index, item);
        Ok(())
    }

    pub fn take_capture(&mut self) -> Result<Option<CapturedReplay>, ModelError> {
        if self.items.is_empty() {
            return Ok(None);
        }
        let protocol = self
            .protocol
            .ok_or_else(|| ModelError::invalid("protected_replay_capture_disabled"))?;
        let kind = ReasoningReplayKind::for_protocol(protocol)
            .ok_or_else(|| ModelError::invalid("protected_replay_protocol_unsupported"))?;
        let items = std::mem::take(&mut self.items)
            .into_values()
            .collect::<Vec<_>>();
        self.bytes = 0;
        Ok(Some(CapturedReplay::new(kind, items)?))
    }
}

/// Capture the private continuation material of a complete (non-streaming) provider response.
///
/// Returns `None` when the response carried no private material. The public decoder still owns
/// the decision about which public blocks are valid; this helper only extracts the private
/// dialect so the caller can bind it to a protected store.
pub fn capture_from_value(
    route: &ModelRoute,
    value: &Value,
) -> Result<Option<CapturedReplay>, ModelError> {
    let kind = match ReasoningReplayKind::for_protocol(route.protocol) {
        Some(kind) => kind,
        None => return Ok(None),
    };
    let mut items = Vec::new();
    match kind {
        ReasoningReplayKind::AnthropicThinkingSignature => {
            if let Some(content) = value["content"].as_array() {
                for block in content {
                    if matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    ) {
                        items.push(block.clone());
                    }
                }
            }
        }
        ReasoningReplayKind::DeepSeekReasoningContent => {
            if let Some(text) = value["choices"][0]["message"]["reasoning_content"].as_str() {
                if !text.trim().is_empty() {
                    items.push(Value::String(text.to_owned()));
                }
            }
        }
        ReasoningReplayKind::OpenAiResponsesReasoningItem => {
            if let Some(output) = value["output"].as_array() {
                for item in output {
                    if item["type"] == "reasoning" {
                        items.push(item.clone());
                    }
                }
            }
        }
        ReasoningReplayKind::GeminiThoughtSignature => {
            if let Some(steps) = value["steps"].as_array() {
                for step in steps {
                    if matches!(step["type"].as_str(), Some("thought" | "thought_summary")) {
                        items.push(step.clone());
                    }
                }
            }
        }
    }
    if items.is_empty() {
        return Ok(None);
    }
    Ok(Some(CapturedReplay::new(kind, items)?))
}

/// Remove provider-private items from a response envelope so the public decoder only ever sees
/// public blocks. The bytes must already be captured through [`capture_from_value`]; callers that
/// did not capture must not strip, or private material would be silently dropped.
pub fn strip_private_items(route: &ModelRoute, value: &mut Value) {
    match ReasoningReplayKind::for_protocol(route.protocol) {
        Some(ReasoningReplayKind::AnthropicThinkingSignature) => {
            if let Some(content) = value["content"].as_array_mut() {
                content.retain(|block| {
                    !matches!(
                        block["type"].as_str(),
                        Some("thinking" | "redacted_thinking")
                    )
                });
            }
        }
        Some(ReasoningReplayKind::DeepSeekReasoningContent) => {
            if let Some(message) = value["choices"][0]["message"].as_object_mut() {
                message.remove("reasoning_content");
            }
        }
        Some(ReasoningReplayKind::OpenAiResponsesReasoningItem) => {
            if let Some(output) = value["output"].as_array_mut() {
                output.retain(|item| item["type"] != "reasoning");
            }
        }
        Some(ReasoningReplayKind::GeminiThoughtSignature) => {
            if let Some(steps) = value["steps"].as_array_mut() {
                steps.retain(|step| {
                    !matches!(step["type"].as_str(), Some("thought" | "thought_summary"))
                });
            }
        }
        None => {}
    }
}

/// Per-message injection plan: which private wire items belong before the public parts of one
/// history message.
#[derive(Clone, Debug, Default)]
pub struct ReplayInjection {
    kind: Option<ReasoningReplayKind>,
    items: BTreeMap<usize, Vec<Value>>,
}

impl ReplayInjection {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn kind(&self) -> Option<ReasoningReplayKind> {
        self.kind
    }

    /// Private items for one history message index; empty when the message has no continuation.
    pub fn items_for(&self, index: usize) -> &[Value] {
        match self.items.get(&index) {
            Some(items) => items.as_slice(),
            None => &[],
        }
    }
}

/// Resolve every declared replay reference through the protected source and bind it to the
/// history message that carries the matching continuation reference.
///
/// Fails closed when material is missing, revoked, expired, bound to another connection/model/
/// effort, or when the declared dialect does not match the route protocol.
pub fn plan_injection(
    route: &ModelRoute,
    request: &ModelRequest,
    spec: &ModelCallSpec,
    source: &dyn ProtectedReplaySource,
    now_unix_ms: u64,
) -> Result<ReplayInjection, ModelError> {
    let mut continuations = BTreeMap::new();
    for (index, message) in request.messages.iter().enumerate() {
        if let Some(continuation) = &message.continuation {
            continuation.validate()?;
            if continuation.provider_id != route.provider_id
                || continuation.protocol != route.protocol
            {
                return Err(ModelError::invalid("opaque_item_cannot_cross_provider"));
            }
            if continuations
                .insert(continuation.item_ref.clone(), index)
                .is_some()
            {
                return Err(ModelError::invalid(
                    "protected_replay_duplicate_continuation",
                ));
            }
        }
    }
    if spec.replay.is_empty() {
        if continuations.is_empty() {
            return Ok(ReplayInjection::default());
        }
        return Err(ModelError::invalid(
            "missing_reasoning_material_blocks_resume",
        ));
    }
    if ReasoningReplayKind::for_protocol(route.protocol).is_none() {
        return Err(ModelError::invalid("protected_replay_protocol_unsupported"));
    }
    let mut injection = ReplayInjection::default();
    let mut consumed = Vec::new();
    for reference in &spec.replay {
        reference.validate_for_call(route, spec.call_id, spec.deadline_unix_ms)?;
        let material = source.resolve_material(reference, route, now_unix_ms)?;
        let kind = reference
            .kind
            .or_else(|| material.declared_kind())
            .or_else(|| ReasoningReplayKind::for_protocol(route.protocol))
            .ok_or_else(|| ModelError::invalid("protected_replay_kind_unknown"))?;
        if kind.protocol() != route.protocol {
            return Err(ModelError::invalid(
                "protected_replay_kind_protocol_mismatch",
            ));
        }
        let index = continuations
            .get(&reference.artifact_id)
            .copied()
            .ok_or_else(|| ModelError::invalid("replay_material_unbound_to_history"))?;
        let capture = CapturedReplay::decode(kind, material.bytes())?;
        if injection.items.contains_key(&index) {
            return Err(ModelError::invalid("protected_replay_duplicate_injection"));
        }
        injection.kind = Some(kind);
        injection.items.insert(index, capture.items().to_vec());
        consumed.push(reference.artifact_id.clone());
    }
    if continuations.len() != consumed.len() {
        return Err(ModelError::invalid(
            "missing_reasoning_material_blocks_resume",
        ));
    }
    Ok(injection)
}

/// Short-lived sealed envelope. Only this shape may be persisted or exported: it holds no
/// plaintext, and opening it requires both the caller key and the exact route scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedReplayArtifact {
    pub schema: String,
    pub artifact_id: String,
    pub kind: ReasoningReplayKind,
    pub connection_id: String,
    pub model_id: String,
    pub route_digest: String,
    pub effort: Option<String>,
    pub prompt_digest: String,
    pub tool_catalog_digest: String,
    pub data_revision: String,
    pub source_call_id: RequestId,
    pub expires_at_unix_ms: u64,
    /// base64 of `nonce || ciphertext || tag`; never contains plaintext.
    pub sealed_bytes: String,
    /// Digest of `sealed_bytes` so a truncated export is rejected before decryption.
    pub sealed_sha256: String,
}

impl SealedReplayArtifact {
    fn scope(&self) -> ProtectedReplayScope {
        ProtectedReplayScope {
            connection_id: self.connection_id.clone(),
            protocol: self.kind.protocol(),
            model_id: self.model_id.clone(),
            route_digest: self.route_digest.clone(),
            effort: self.effort.clone(),
            prompt_digest: self.prompt_digest.clone(),
            tool_catalog_digest: self.tool_catalog_digest.clone(),
            data_revision: self.data_revision.clone(),
            source_call_id: self.source_call_id,
            expires_at_unix_ms: self.expires_at_unix_ms,
        }
    }

    pub fn summary(&self) -> PublicReasoningSummary {
        PublicReasoningSummary {
            schema: kiana_domain::PUBLIC_REPLAY_SUMMARY_SCHEMA.to_owned(),
            kind: self.kind,
            artifact_id: self.artifact_id.clone(),
            material_sha256: self.sealed_sha256.clone(),
            material_bytes: self.sealed_bytes.len() as u64,
            connection_id: self.connection_id.clone(),
            model_id: self.model_id.clone(),
            route_digest: self.route_digest.clone(),
            source_call_id: self.source_call_id,
            expires_at_unix_ms: self.expires_at_unix_ms,
            redaction: kiana_domain::PUBLIC_REPLAY_REDACTION.to_owned(),
        }
    }

    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != SEALED_REPLAY_ARTIFACT_SCHEMA
            || self.artifact_id.trim().is_empty()
            || self.connection_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.route_digest.trim().is_empty()
            || self.prompt_digest.trim().is_empty()
            || self.tool_catalog_digest.trim().is_empty()
            || self.data_revision.trim().is_empty()
            || self.expires_at_unix_ms == 0
            || self.sealed_bytes.trim().is_empty()
            || self.sealed_sha256 != json_digest(&json!({"sealed": self.sealed_bytes}))
        {
            return Err(ModelError::invalid("sealed_replay_artifact_invalid"));
        }
        self.scope().validate()
    }

    fn aad(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "artifact_id": self.artifact_id,
            "kind": self.kind,
            "connection_id": self.connection_id,
            "model_id": self.model_id,
            "route_digest": self.route_digest,
            "effort": self.effort,
            "prompt_digest": self.prompt_digest,
            "tool_catalog_digest": self.tool_catalog_digest,
            "data_revision": self.data_revision,
            "source_call_id": self.source_call_id,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

/// ChaCha20-Poly1305 sealer. Key material is supplied by the caller at construction time and is
/// never serialized, logged or stored in this struct's `Debug` output.
pub struct RingReplaySealer {
    key: LessSafeKey,
    rng: SystemRandom,
}

impl std::fmt::Debug for RingReplaySealer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RingReplaySealer")
            .field("algorithm", &"chacha20_poly1305")
            .field("key", &"redacted")
            .finish()
    }
}

impl RingReplaySealer {
    pub fn new(key: &[u8; 32]) -> Result<Self, ModelError> {
        let unbound = UnboundKey::new(&CHACHA20_POLY1305, key)
            .map_err(|_| ModelError::invalid("protected_replay_sealer_key_invalid"))?;
        Ok(Self {
            key: LessSafeKey::new(unbound),
            rng: SystemRandom::new(),
        })
    }

    /// Seal one material entry into an exportable short-lived envelope.
    pub fn seal(
        &self,
        artifact_id: &str,
        material: &ProtectedReplayMaterial,
    ) -> Result<SealedReplayArtifact, ModelError> {
        material.scope.validate()?;
        let kind = material
            .kind()
            .ok_or_else(|| ModelError::invalid("protected_replay_kind_unknown"))?;
        if artifact_id.trim().is_empty() {
            return Err(ModelError::invalid("protected_replay_artifact_id_invalid"));
        }
        let mut artifact = SealedReplayArtifact {
            schema: SEALED_REPLAY_ARTIFACT_SCHEMA.to_owned(),
            artifact_id: artifact_id.to_owned(),
            kind,
            connection_id: material.scope.connection_id.clone(),
            model_id: material.scope.model_id.clone(),
            route_digest: material.scope.route_digest.clone(),
            effort: material.scope.effort.clone(),
            prompt_digest: material.scope.prompt_digest.clone(),
            tool_catalog_digest: material.scope.tool_catalog_digest.clone(),
            data_revision: material.scope.data_revision.clone(),
            source_call_id: material.scope.source_call_id,
            expires_at_unix_ms: material.scope.expires_at_unix_ms,
            sealed_bytes: String::new(),
            sealed_sha256: String::new(),
        };
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|_| ModelError::invalid("protected_replay_sealer_rng_unavailable"))?;
        let mut in_out = material.bytes().to_vec();
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(artifact.aad().as_bytes()),
                &mut in_out,
            )
            .map_err(|_| ModelError::invalid("protected_replay_seal_failed"))?;
        let mut sealed = nonce_bytes.to_vec();
        sealed.extend_from_slice(&in_out);
        artifact.sealed_bytes = STANDARD.encode(&sealed);
        artifact.sealed_sha256 = json_digest(&json!({"sealed": artifact.sealed_bytes}));
        Ok(artifact)
    }

    /// Open an envelope after checking schema, integrity, TTL and route scope.
    pub fn open(
        &self,
        artifact: &SealedReplayArtifact,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<ProtectedReplayMaterial, ModelError> {
        artifact.validate()?;
        if now_unix_ms >= artifact.expires_at_unix_ms {
            return Err(ModelError::invalid("protected_replay_expired"));
        }
        if artifact.route_digest != route.digest()
            || artifact.connection_id != route.connection_id
            || artifact.model_id != route.model_id
            || artifact.kind.protocol() != route.protocol
        {
            return Err(ModelError::invalid(
                "replay_material_cannot_cross_connection_or_model_scope",
            ));
        }
        let sealed = STANDARD
            .decode(&artifact.sealed_bytes)
            .map_err(|_| ModelError::invalid("sealed_replay_artifact_invalid"))?;
        if sealed.len() <= NONCE_LEN {
            return Err(ModelError::invalid("sealed_replay_artifact_invalid"));
        }
        let (nonce, ciphertext) = sealed.split_at(NONCE_LEN);
        let nonce_bytes: [u8; NONCE_LEN] = nonce
            .try_into()
            .map_err(|_| ModelError::invalid("sealed_replay_artifact_invalid"))?;
        let mut in_out = ciphertext.to_vec();
        let plaintext = self
            .key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_bytes),
                Aad::from(artifact.aad().as_bytes()),
                &mut in_out,
            )
            .map_err(|_| ModelError::invalid("replay_material_authentication_failed"))?;
        ProtectedReplayMaterial::new_with_kind(
            artifact.scope(),
            Some(artifact.kind),
            plaintext.to_vec(),
        )
    }
}

/// Ciphertext-only short-lived artifact store. Losing the key must lose the material, so the
/// store keeps envelopes and never a second plaintext copy.
#[derive(Default)]
pub struct SealedReplayArtifactStore {
    artifacts: BTreeMap<String, SealedReplayArtifact>,
}

impl SealedReplayArtifactStore {
    pub fn len(&self) -> usize {
        self.artifacts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.artifacts.is_empty()
    }

    pub fn insert(&mut self, artifact: SealedReplayArtifact) -> Result<(), ModelError> {
        artifact.validate()?;
        if !self.artifacts.contains_key(&artifact.artifact_id)
            && self.artifacts.len() >= MAX_SEALED_ENTRIES
        {
            return Err(ModelError::invalid("sealed_replay_store_full"));
        }
        self.artifacts
            .insert(artifact.artifact_id.clone(), artifact);
        Ok(())
    }

    pub fn get(&self, artifact_id: &str) -> Option<&SealedReplayArtifact> {
        self.artifacts.get(artifact_id)
    }

    pub fn revoke(&mut self, artifact_id: &str) -> bool {
        self.artifacts.remove(artifact_id).is_some()
    }

    /// Serialize every envelope for the durable side. The output contains ciphertext only.
    pub fn export(&self) -> Result<String, ModelError> {
        serde_json::to_string(&self.artifacts.values().collect::<Vec<_>>())
            .map_err(|_| ModelError::invalid("sealed_replay_export_failed"))
    }

    pub fn import(&mut self, json: &str) -> Result<usize, ModelError> {
        let artifacts = serde_json::from_str::<Vec<SealedReplayArtifact>>(json)
            .map_err(|_| ModelError::invalid("sealed_replay_import_failed"))?;
        let mut imported = 0;
        for artifact in artifacts {
            self.insert(artifact)?;
            imported += 1;
        }
        Ok(imported)
    }
}

/// Protected replay adapter used by the provider call path.
///
/// Holds the process-local plaintext store plus the sealed short-lived artifacts that survive a
/// process restart when the caller re-imports its export. A missing entry, a revoked entry, an
/// expired TTL or a scope mismatch all fail closed before network dispatch.
pub struct ProtectedReplayService {
    sealer: RingReplaySealer,
    in_process: kiana_domain::ProtectedReplayStore,
    sealed: SealedReplayArtifactStore,
}

impl std::fmt::Debug for ProtectedReplayService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProtectedReplayService")
            .field("sealer", &self.sealer)
            .field("in_process_len", &self.in_process.len())
            .field("sealed_len", &self.sealed.len())
            .finish()
    }
}

impl ProtectedReplayService {
    pub fn new(key: &[u8; 32]) -> Result<Self, ModelError> {
        Ok(Self {
            sealer: RingReplaySealer::new(key)?,
            in_process: kiana_domain::ProtectedReplayStore::default(),
            sealed: SealedReplayArtifactStore::default(),
        })
    }

    /// Store one captured material entry in the process-local store and seal a durable copy.
    pub fn store(
        &mut self,
        artifact_id: &str,
        material: &ProtectedReplayMaterial,
    ) -> Result<ProtectedReplayRef, ModelError> {
        let artifact = self.sealer.seal(artifact_id, material)?;
        let reference = self.in_process.insert(artifact_id, material.clone())?;
        if let Err(error) = self.sealed.insert(artifact) {
            self.in_process.delete(artifact_id);
            return Err(error);
        }
        Ok(reference)
    }

    pub fn revoke(&mut self, artifact_id: &str, reason: &str) -> Result<(), ModelError> {
        self.sealed.revoke(artifact_id);
        self.in_process.revoke(artifact_id, reason)
    }

    pub fn invalidate_route_change(&mut self, route: &ModelRoute) -> usize {
        let invalidated = self.in_process.invalidate_route_change(route);
        let stale = self
            .sealed
            .artifacts
            .iter()
            .filter(|(_, artifact)| {
                artifact.route_digest != route.digest()
                    || artifact.connection_id != route.connection_id
                    || artifact.model_id != route.model_id
                    || artifact.kind.protocol() != route.protocol
            })
            .map(|(artifact_id, _)| artifact_id.clone())
            .collect::<Vec<_>>();
        for artifact_id in &stale {
            self.sealed.revoke(artifact_id);
        }
        invalidated + stale.len()
    }

    pub fn sealed_envelope(&self, artifact_id: &str) -> Option<&SealedReplayArtifact> {
        self.sealed.get(artifact_id)
    }

    pub fn export_sealed(&self) -> Result<String, ModelError> {
        self.sealed.export()
    }

    pub fn import_sealed(&mut self, json: &str) -> Result<usize, ModelError> {
        self.sealed.import(json)
    }

    pub fn len(&self) -> usize {
        self.in_process.len()
    }

    pub fn is_empty(&self) -> bool {
        self.in_process.is_empty()
    }

    pub fn revocation_reason(&self, artifact_id: &str) -> Option<&str> {
        self.in_process.revocation_reason(artifact_id)
    }
}

impl ProtectedReplaySource for ProtectedReplayService {
    fn resolve_material(
        &self,
        reference: &ProtectedReplayRef,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<ProtectedReplayMaterial, ModelError> {
        match self.in_process.get(reference, route, now_unix_ms) {
            Ok(material) => Ok(material.clone()),
            Err(error) if error.code == "missing_reasoning_material_blocks_resume" => {
                let artifact = self.sealed.get(&reference.artifact_id).ok_or(error)?;
                self.sealer.open(artifact, route, now_unix_ms)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(protocol: ModelProtocol) -> ModelRoute {
        ModelRoute {
            provider_id: "fixture".to_owned(),
            protocol,
            connection_id: "connection-a".to_owned(),
            model_id: "model-a".to_owned(),
            profile: "default".to_owned(),
            configuration_revision: "config.v1".to_owned(),
            streaming: true,
        }
    }

    fn scope(route: &ModelRoute) -> ProtectedReplayScope {
        ProtectedReplayScope {
            connection_id: route.connection_id.clone(),
            protocol: route.protocol,
            model_id: route.model_id.clone(),
            route_digest: route.digest(),
            effort: Some("medium".to_owned()),
            prompt_digest: "sha256:prompt".to_owned(),
            tool_catalog_digest: "sha256:tools".to_owned(),
            data_revision: "data.v1".to_owned(),
            source_call_id: RequestId::new(),
            expires_at_unix_ms: 20_000,
        }
    }

    #[test]
    fn capture_selects_the_protocol_dialect() {
        let route = route(ModelProtocol::GeminiInteractions);
        let response = json!({
            "id": "interaction-1",
            "status": "requires_action",
            "steps": [
                {"type": "thought", "signature": "sig-1", "content": [{"type": "text", "text": "private"}]},
                {"type": "function_call", "id": "call-1", "name": "shell", "arguments": {}}
            ]
        });
        let capture = capture_from_value(&route, &response)
            .expect("capture")
            .expect("private material");
        assert_eq!(capture.kind(), ReasoningReplayKind::GeminiThoughtSignature);
        let material = capture.material(scope(&route)).expect("material");
        assert!(std::str::from_utf8(material.bytes())
            .expect("utf8")
            .contains("sig-1"));
        let summary = material.public_summary("artifact-1").expect("summary");
        assert!(!serde_json::to_string(&summary)
            .expect("json")
            .contains("sig-1"));
    }

    #[test]
    fn sealed_envelope_never_contains_plaintext() {
        let route = route(ModelProtocol::AnthropicMessages);
        let material = CapturedReplay::new(
            ReasoningReplayKind::AnthropicThinkingSignature,
            vec![json!({"type": "thinking", "thinking": "chain", "signature": "sig-1"})],
        )
        .expect("capture")
        .material(scope(&route))
        .expect("material");
        let sealer = RingReplaySealer::new(&[7u8; 32]).expect("sealer");
        let sealed = sealer.seal("artifact-1", &material).expect("seal");
        assert!(!sealed.sealed_bytes.contains("sig-1"));
        assert_eq!(
            sealer.open(&sealed, &route, 1_000).expect("open").bytes(),
            material.bytes()
        );
    }
}
