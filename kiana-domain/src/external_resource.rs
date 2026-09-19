//! Untrusted external context/resource admission and revocable snapshots.
//!
//! MCP resources, connector results and user imports share this contract. External payloads are
//! evidence/context only: they cannot widen the server-derived memory scope or create write
//! authority.

use crate::{json_digest, EvidenceStatus, Freshness, SourceKind, SourceRef, SourceSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const EXTERNAL_RESOURCE_REQUEST_SCHEMA: &str = "kiana.external-resource-request.v1";
pub const EXTERNAL_RESOURCE_SNAPSHOT_SCHEMA: &str = "kiana.external-resource-snapshot.v1";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalResourceKind {
    McpResource,
    Connector,
    UserImport,
}

impl ExternalResourceKind {
    pub const fn source_kind(self) -> SourceKind {
        match self {
            Self::McpResource | Self::Connector => SourceKind::Connector,
            Self::UserImport => SourceKind::UserImport,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalResourceRequest {
    pub schema: String,
    pub kind: ExternalResourceKind,
    pub source_id: String,
    pub locator_digest: String,
    pub content_digest: String,
    pub processing_grant_digest: String,
    pub quota_digest: String,
    pub base_scope_digest: String,
    pub requested_scope_digest: String,
    pub requested_memory_collections: Vec<String>,
    pub max_bytes: u64,
    pub max_items: u32,
    pub allow_memory_write: bool,
    pub allow_scope_widening: bool,
    pub request_digest: String,
}

impl ExternalResourceRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: ExternalResourceKind,
        source_id: impl Into<String>,
        locator_digest: impl Into<String>,
        content_digest: impl Into<String>,
        processing_grant_digest: impl Into<String>,
        quota_digest: impl Into<String>,
        base_scope_digest: impl Into<String>,
        requested_scope_digest: impl Into<String>,
        requested_memory_collections: Vec<String>,
        max_bytes: u64,
        max_items: u32,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: EXTERNAL_RESOURCE_REQUEST_SCHEMA.to_owned(),
            kind,
            source_id: source_id.into(),
            locator_digest: locator_digest.into(),
            content_digest: content_digest.into(),
            processing_grant_digest: processing_grant_digest.into(),
            quota_digest: quota_digest.into(),
            base_scope_digest: base_scope_digest.into(),
            requested_scope_digest: requested_scope_digest.into(),
            requested_memory_collections,
            max_bytes,
            max_items,
            allow_memory_write: false,
            allow_scope_widening: false,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_RESOURCE_REQUEST_SCHEMA
            || self.max_bytes == 0
            || self.max_items == 0
            || self.requested_memory_collections.len() > 64
            || self.allow_memory_write
            || self.allow_scope_widening
            || self.requested_scope_digest != self.base_scope_digest
        {
            return Err("external_resource_scope_widening_denied".to_owned());
        }
        required(&self.source_id, "external_resource_source_id", 512)?;
        for (value, field) in [
            (&self.locator_digest, "external_resource_locator_digest"),
            (&self.content_digest, "external_resource_content_digest"),
            (
                &self.processing_grant_digest,
                "external_resource_processing_grant_digest",
            ),
            (&self.quota_digest, "external_resource_quota_digest"),
            (&self.base_scope_digest, "external_resource_scope_digest"),
            (&self.request_digest, "external_resource_request_digest"),
        ] {
            digest(value, field)?;
        }
        if self
            .requested_memory_collections
            .iter()
            .any(|collection| collection.trim().is_empty() || collection.len() > 256)
        {
            return Err("external_resource_collection_invalid".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("external_resource_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "kind": self.kind,
            "source_id": self.source_id,
            "locator_digest": self.locator_digest,
            "content_digest": self.content_digest,
            "processing_grant_digest": self.processing_grant_digest,
            "quota_digest": self.quota_digest,
            "base_scope_digest": self.base_scope_digest,
            "requested_scope_digest": self.requested_scope_digest,
            "requested_memory_collections": self.requested_memory_collections,
            "max_bytes": self.max_bytes,
            "max_items": self.max_items,
            "allow_memory_write": self.allow_memory_write,
            "allow_scope_widening": self.allow_scope_widening,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalResourceSnapshot {
    pub schema: String,
    pub source: SourceSnapshot,
    pub scope_digest: String,
    pub processing_grant_digest: String,
    pub quota_digest: String,
    pub data_epoch: u64,
    pub expires_at_ms: Option<u64>,
    pub revoked_at_ms: Option<u64>,
    pub snapshot_digest: String,
}

impl ExternalResourceSnapshot {
    pub fn new(
        request: &ExternalResourceRequest,
        revision: impl Into<String>,
        observed_at_ms: u64,
        expires_at_ms: Option<u64>,
        data_epoch: u64,
    ) -> Result<Self, String> {
        request.validate()?;
        if observed_at_ms == 0 || data_epoch == 0 {
            return Err("external_resource_snapshot_epoch_invalid".to_owned());
        }
        if expires_at_ms.is_some_and(|expiry| expiry <= observed_at_ms) {
            return Err("external_resource_snapshot_expiry_invalid".to_owned());
        }
        let source_ref = SourceRef::new(
            format!("external:{}", request.source_id),
            request.kind.source_kind(),
            format!("external:{}", request.source_id),
            revision,
            request.content_digest.clone(),
            None,
            EvidenceStatus::Attributed,
        )?;
        let source = SourceSnapshot::new(
            source_ref,
            Freshness::Current,
            EvidenceStatus::Attributed,
            Some(observed_at_ms),
        )?;
        let mut snapshot = Self {
            schema: EXTERNAL_RESOURCE_SNAPSHOT_SCHEMA.to_owned(),
            source,
            scope_digest: request.requested_scope_digest.clone(),
            processing_grant_digest: request.processing_grant_digest.clone(),
            quota_digest: request.quota_digest.clone(),
            data_epoch,
            expires_at_ms,
            revoked_at_ms: None,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn revoke(&mut self, revoked_at_ms: u64) -> Result<(), String> {
        if revoked_at_ms == 0 {
            return Err("external_resource_revoke_time_invalid".to_owned());
        }
        self.revoked_at_ms = Some(revoked_at_ms);
        self.snapshot_digest = self.digest();
        self.validate()
    }

    pub fn readable_at(&self, now_ms: u64, data_epoch: u64) -> bool {
        self.validate().is_ok()
            && now_ms > 0
            && data_epoch >= self.data_epoch
            && self.revoked_at_ms.is_none()
            && self.expires_at_ms.is_none_or(|expiry| now_ms < expiry)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTERNAL_RESOURCE_SNAPSHOT_SCHEMA
            || self.data_epoch == 0
            || self.revoked_at_ms.is_some_and(|time| time == 0)
            || self.expires_at_ms.is_some_and(|time| time == 0)
        {
            return Err("external_resource_snapshot_header_invalid".to_owned());
        }
        self.source.validate()?;
        if self.source.evidence != EvidenceStatus::Attributed {
            return Err("external_resource_evidence_must_remain_untrusted".to_owned());
        }
        for (value, field) in [
            (
                &self.scope_digest,
                "external_resource_snapshot_scope_digest",
            ),
            (
                &self.processing_grant_digest,
                "external_resource_snapshot_processing_grant_digest",
            ),
            (
                &self.quota_digest,
                "external_resource_snapshot_quota_digest",
            ),
            (&self.snapshot_digest, "external_resource_snapshot_digest"),
        ] {
            digest(value, field)?;
        }
        if self.snapshot_digest != self.digest() {
            return Err("external_resource_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "source": self.source,
            "scope_digest": self.scope_digest,
            "processing_grant_digest": self.processing_grant_digest,
            "quota_digest": self.quota_digest,
            "data_epoch": self.data_epoch,
            "expires_at_ms": self.expires_at_ms,
            "revoked_at_ms": self.revoked_at_ms,
        }))
    }
}
