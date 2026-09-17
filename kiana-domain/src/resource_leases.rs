//! Resource ownership and fencing contracts for path/Cell execution.
//!
//! A `ResourceLease` is an immutable observation. It does not open an OS lock or authorize a
//! handler by itself; callers must revalidate it against the current authority epoch immediately
//! before the effect boundary.

use crate::{
    json_digest, normalize_role_path, path_locks_conflict, CellId, FenceTokenId, RunId,
    SchemaVersion, SessionId, StorageLockId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const RESOURCE_LEASE_SCHEMA: &str = "kiana.resource-lease.v1";
pub const RESOURCE_LEASE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceLease {
    pub schema: String,
    pub version: SchemaVersion,
    pub lease_id: StorageLockId,
    pub fence_token: FenceTokenId,
    pub resource: String,
    pub owner_run_id: RunId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_cell_id: Option<CellId>,
    pub session_id: SessionId,
    pub authority_epoch: u64,
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_digest: Option<String>,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub resource_digest: String,
    pub lease_digest: String,
}

impl ResourceLease {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        lease_id: StorageLockId,
        fence_token: FenceTokenId,
        resource: impl Into<String>,
        owner_run_id: RunId,
        owner_cell_id: Option<CellId>,
        session_id: impl Into<String>,
        authority_epoch: u64,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        Self::build(
            lease_id,
            fence_token,
            canonical_resource(&resource.into())?,
            owner_run_id,
            owner_cell_id,
            SessionId::new(session_id),
            authority_epoch,
            1,
            None,
            issued_at_unix_ms,
            expires_at_unix_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn successor(
        previous: &Self,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        previous.validate()?;
        if fence_token == previous.fence_token {
            return Err("resource_lease_fence_token_reused".to_owned());
        }
        if authority_epoch < previous.authority_epoch {
            return Err("resource_lease_authority_epoch_rollback".to_owned());
        }
        let sequence = previous
            .sequence
            .checked_add(1)
            .ok_or_else(|| "resource_lease_sequence_exhausted".to_owned())?;
        Self::build(
            previous.lease_id,
            fence_token,
            previous.resource.clone(),
            previous.owner_run_id,
            previous.owner_cell_id,
            previous.session_id.clone(),
            authority_epoch,
            sequence,
            Some(previous.lease_digest.clone()),
            issued_at_unix_ms,
            expires_at_unix_ms,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        lease_id: StorageLockId,
        fence_token: FenceTokenId,
        resource: String,
        owner_run_id: RunId,
        owner_cell_id: Option<CellId>,
        session_id: SessionId,
        authority_epoch: u64,
        sequence: u64,
        parent_digest: Option<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut lease = Self {
            schema: RESOURCE_LEASE_SCHEMA.to_owned(),
            version: RESOURCE_LEASE_VERSION,
            lease_id,
            fence_token,
            resource,
            owner_run_id,
            owner_cell_id,
            session_id,
            authority_epoch,
            sequence,
            parent_digest,
            issued_at_unix_ms,
            expires_at_unix_ms,
            resource_digest: String::new(),
            lease_digest: String::new(),
        };
        lease.resource_digest = json_digest(&json!({"resource": lease.resource}));
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let lease: Self = serde_json::from_value(value.clone())
            .map_err(|_| "resource_lease_decode_failed".to_owned())?;
        lease.validate()?;
        Ok(lease)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "resource_lease_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESOURCE_LEASE_SCHEMA
            || !self.version.is_compatible_with(&RESOURCE_LEASE_VERSION)
            || self.lease_id.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.owner_run_id.as_uuid().is_nil()
            || self.owner_cell_id.is_some_and(|id| id.as_uuid().is_nil())
            || self.session_id.is_empty()
            || self.session_id.as_str().len() > 256
            || self.authority_epoch == 0
            || self.sequence == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self
                .parent_digest
                .as_deref()
                .is_some_and(|digest| !valid_digest(digest))
        {
            return Err("resource_lease_header_invalid".to_owned());
        }
        if self.sequence == 1 && self.parent_digest.is_some() {
            return Err("resource_lease_genesis_parent_unexpected".to_owned());
        }
        if self.sequence > 1 && self.parent_digest.is_none() {
            return Err("resource_lease_parent_required".to_owned());
        }
        let resource = canonical_resource(&self.resource)?;
        if resource != self.resource {
            return Err("resource_lease_resource_noncanonical".to_owned());
        }
        if self.resource_digest != json_digest(&json!({"resource": self.resource})) {
            return Err("resource_lease_resource_digest_mismatch".to_owned());
        }
        if !valid_digest(&self.lease_digest) || self.lease_digest != self.digest() {
            return Err("resource_lease_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_successor(&self, previous: &Self) -> Result<(), String> {
        previous.validate()?;
        self.validate()?;
        if self.lease_id != previous.lease_id
            || self.resource != previous.resource
            || self.owner_run_id != previous.owner_run_id
            || self.owner_cell_id != previous.owner_cell_id
            || self.session_id != previous.session_id
            || self.sequence != previous.sequence.saturating_add(1)
            || self.parent_digest.as_deref() != Some(previous.lease_digest.as_str())
            || self.authority_epoch < previous.authority_epoch
        {
            return Err("resource_lease_successor_mismatch".to_owned());
        }
        if self.fence_token == previous.fence_token {
            return Err("resource_lease_fence_token_reused".to_owned());
        }
        Ok(())
    }

    pub fn validate_current(
        &self,
        now_unix_ms: u64,
        authority_epoch: u64,
        fence_token: FenceTokenId,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms < self.issued_at_unix_ms || now_unix_ms >= self.expires_at_unix_ms {
            return Err("resource_lease_expired".to_owned());
        }
        if authority_epoch != self.authority_epoch {
            return Err(if authority_epoch < self.authority_epoch {
                "resource_lease_authority_epoch_rollback"
            } else {
                "resource_lease_authority_epoch_stale"
            }
            .to_owned());
        }
        if fence_token != self.fence_token {
            return Err("resource_lease_fence_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn covers(&self, path: &str) -> Result<bool, String> {
        self.validate()?;
        let path = canonical_resource(path)?;
        Ok(self.resource == "*"
            || self.resource == "."
            || self.resource == path
            || path.starts_with(&format!("{}/", self.resource)))
    }

    pub fn conflicts(&self, path: &str) -> Result<bool, String> {
        self.validate()?;
        let path = canonical_resource(path)?;
        Ok(path_locks_conflict(&self.resource, &path))
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "lease_id": self.lease_id,
            "fence_token": self.fence_token,
            "resource": self.resource,
            "owner_run_id": self.owner_run_id,
            "owner_cell_id": self.owner_cell_id,
            "session_id": self.session_id,
            "authority_epoch": self.authority_epoch,
            "sequence": self.sequence,
            "parent_digest": self.parent_digest,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "resource_digest": self.resource_digest,
        }))
    }
}

/// Normalize an ordered write-set without silently dropping malformed paths.
pub fn canonical_resource_set(paths: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = paths
        .iter()
        .map(|path| canonical_resource(path))
        .collect::<Result<Vec<_>, _>>()?;
    normalized.sort();
    normalized.dedup();
    if normalized.is_empty() {
        return Ok(vec!["*".to_owned()]);
    }
    if normalized.iter().any(|path| path == "*") {
        return Ok(vec!["*".to_owned()]);
    }
    if normalized.iter().any(|path| path == ".") {
        return Ok(vec![".".to_owned()]);
    }
    Ok(normalized)
}

fn canonical_resource(resource: &str) -> Result<String, String> {
    let resource = resource.trim();
    if resource == "*" {
        return Ok(resource.to_owned());
    }
    normalize_role_path(resource)
        .filter(|path| path.len() <= 4_096 && !path.contains('\0'))
        .ok_or_else(|| "resource_lease_resource_invalid".to_owned())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
