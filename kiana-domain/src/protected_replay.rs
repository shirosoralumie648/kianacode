//! Bounded, reference-only provider replay material.
//!
//! Provider reasoning/signature bytes are private wire material. This module keeps them in an
//! in-process bounded store and exposes only a digest-bound reference to receipts, EventLog and
//! protocol projections. A durable adapter may replace the store later; scope checks remain the
//! same.

use crate::{json_digest, ModelError, ModelProtocol, ModelRoute, ProtectedReplayRef, RequestId};
use serde_json::json;
use std::collections::BTreeMap;

pub const PROTECTED_REPLAY_SCHEMA: &str = "kiana.protected-replay.v1";
const MAX_MATERIAL_BYTES: usize = 512 * 1024;
const MAX_ENTRIES: usize = 32;

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
        }
    }
}

/// Private bytes are intentionally not serializable or included in Debug output.
#[derive(Clone, Eq, PartialEq)]
pub struct ProtectedReplayMaterial {
    pub scope: ProtectedReplayScope,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for ProtectedReplayMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProtectedReplayMaterial")
            .field("scope", &self.scope)
            .field("byte_len", &self.bytes.len())
            .finish()
    }
}

impl ProtectedReplayMaterial {
    pub fn new(scope: ProtectedReplayScope, bytes: impl Into<Vec<u8>>) -> Result<Self, ModelError> {
        scope.validate()?;
        let bytes = bytes.into();
        if bytes.is_empty() || bytes.len() > MAX_MATERIAL_BYTES {
            return Err(ModelError::invalid("protected_replay_material_size_invalid"));
        }
        Ok(Self { scope, bytes })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn reference(&self, artifact_id: impl Into<String>) -> ProtectedReplayRef {
        self.scope.reference(artifact_id, &self.bytes)
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
        {
            return Err(ModelError::invalid(
                "replay_material_cannot_cross_connection_or_model_scope",
            ));
        }
        Ok(())
    }
}

/// Process-local store used when a deployment has no durable protected artifact service.
/// Deleting an entry invalidates all future reads instead of leaving a stale resume projection.
#[derive(Default)]
pub struct ProtectedReplayStore {
    entries: BTreeMap<String, ProtectedReplayMaterial>,
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
        let reference = material.reference(&artifact_id);
        self.entries.insert(artifact_id, material);
        Ok(reference)
    }

    pub fn get(
        &self,
        reference: &ProtectedReplayRef,
        route: &ModelRoute,
        now_unix_ms: u64,
    ) -> Result<&ProtectedReplayMaterial, ModelError> {
        let material = self
            .entries
            .get(&reference.artifact_id)
            .ok_or_else(|| ModelError::invalid("missing_reasoning_material_blocks_resume"))?;
        material.validate_for(reference, route, now_unix_ms)?;
        Ok(material)
    }

    pub fn delete(&mut self, artifact_id: &str) -> bool {
        self.entries.remove(artifact_id).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
