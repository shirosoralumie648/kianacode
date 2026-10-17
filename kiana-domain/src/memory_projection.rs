//! Epoch- and governance-fenced visibility for projected Memory rows.
//!
//! A projection row is only searchable when the server-owned policy epoch still matches, the
//! record is in a searchable lifecycle state, the deletion index does not cover it, and
//! revocation/retention has not fenced its source. This is a pure decision contract; it never
//! erases facts or mutates an index/cache.

use crate::{
    json_digest, valid_memory_deletion_digest, DataPolicy, MemoryDeletionIndex, MemoryRecord,
    MemoryVisibility,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const MEMORY_PROJECTION_FENCE_SCHEMA: &str = "kiana.memory-projection-fence.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryProjectionDisposition {
    Searchable,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryProjectionFence {
    pub schema: String,
    pub record_id: String,
    pub record_revision: u64,
    pub project_root: String,
    pub data_epoch: u64,
    pub policy_revision: u64,
    pub disposition: MemoryProjectionDisposition,
    pub reason: String,
    /// Digest of the deletion index this decision was taken against. Absent only for callers that
    /// have no committed deletion facts (legacy file-only scopes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_index_digest: Option<String>,
    pub fence_digest: String,
}

impl MemoryProjectionFence {
    /// `deletion_index` carries the projected tombstones for the same project and governance
    /// epoch. A stale index, a foreign project or an index that covers the row all deny the read
    /// before any ranking or prompt assembly happens.
    pub fn evaluate(
        record: &MemoryRecord,
        policy: &DataPolicy,
        deletion_index: Option<&MemoryDeletionIndex>,
        expected_data_epoch: u64,
        requested_project_root: &str,
        now_ms: u64,
    ) -> Result<Self, String> {
        record.validate_lifecycle()?;
        policy.validate()?;
        if expected_data_epoch == 0 || now_ms == 0 {
            return Err("memory_projection_fence_boundary_invalid".to_owned());
        }
        let mut deletion_index_digest = None;
        let reason = if expected_data_epoch != policy.data_epoch {
            "data_epoch_stale".to_owned()
        } else if let Some(index) = deletion_index {
            index.validate()?;
            deletion_index_digest = Some(index.index_digest.clone());
            match index.bind_policy(policy) {
                Err(stale) => stale,
                Ok(()) if index.project_root_conflicts(requested_project_root) => {
                    "deletion_index_project_mismatch".to_owned()
                }
                Ok(()) => Self::row_reason(record, policy, index, requested_project_root, now_ms),
            }
        } else {
            Self::row_reason_without_index(record, policy, requested_project_root, now_ms)
        };
        Self::finish(
            record,
            policy,
            requested_project_root,
            deletion_index_digest,
            reason,
        )
    }

    /// Row-level decision when a deletion index was supplied. A covered row is denied whether or
    /// not every surface has acknowledged the tombstone, so a surviving cache copy cannot revive it.
    fn row_reason(
        record: &MemoryRecord,
        policy: &DataPolicy,
        deletion_index: &MemoryDeletionIndex,
        requested_project_root: &str,
        now_ms: u64,
    ) -> String {
        match Self::row_reason_without_index(record, policy, requested_project_root, now_ms) {
            reason if reason == "allowed" => deletion_index
                .check_read(&record.id, record.revision)
                .err()
                .unwrap_or_else(|| "allowed".to_owned()),
            reason => reason,
        }
    }

    fn row_reason_without_index(
        record: &MemoryRecord,
        policy: &DataPolicy,
        requested_project_root: &str,
        now_ms: u64,
    ) -> String {
        if !record.project_root.is_empty()
            && !requested_project_root.is_empty()
            && record.project_root != requested_project_root
            && !crate::MemoryCollection::parse(&record.collection)
                .is_some_and(|collection| collection.home_scoped())
        {
            "project_scope_denied".to_owned()
        } else if !matches!(
            record.visibility(),
            MemoryVisibility::Searchable | MemoryVisibility::SessionOnly
        ) {
            if record.admission_state == crate::MemoryAdmission::Candidate {
                "candidate_not_qualified".to_owned()
            } else {
                "tombstone_or_rejected".to_owned()
            }
        } else if record
            .retention
            .as_ref()
            .and_then(|retention| retention.expires_at_ms)
            .is_some_and(|expiry| expiry <= now_ms)
        {
            "retention_expired".to_owned()
        } else if policy
            .revoked_sources
            .iter()
            .any(|source| record.source.contains(source))
            || policy.grants.values().any(|grant| {
                record.source.contains(&grant.source_path)
                    && (grant.revoked
                        || grant
                            .retention
                            .expires_at_ms
                            .is_some_and(|expiry| expiry <= now_ms))
            })
        {
            "source_revoked_or_retention_expired".to_owned()
        } else {
            "allowed".to_owned()
        }
    }

    fn finish(
        record: &MemoryRecord,
        policy: &DataPolicy,
        requested_project_root: &str,
        deletion_index_digest: Option<String>,
        reason: String,
    ) -> Result<Self, String> {
        let disposition = if reason == "allowed" {
            MemoryProjectionDisposition::Searchable
        } else {
            MemoryProjectionDisposition::Denied
        };
        let mut fence = Self {
            schema: MEMORY_PROJECTION_FENCE_SCHEMA.to_owned(),
            record_id: record.id.clone(),
            record_revision: record.revision,
            project_root: requested_project_root.to_owned(),
            data_epoch: policy.data_epoch,
            policy_revision: policy.revision,
            disposition,
            reason,
            deletion_index_digest,
            fence_digest: String::new(),
        };
        fence.fence_digest = fence.digest();
        fence.validate()?;
        Ok(fence)
    }

    pub fn validate_epoch(policy_epoch: u64, expected_epoch: u64) -> Result<(), String> {
        if policy_epoch == 0 || expected_epoch == 0 || policy_epoch != expected_epoch {
            return Err("memory_projection_data_epoch_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn searchable(&self) -> bool {
        self.disposition == MemoryProjectionDisposition::Searchable
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_PROJECTION_FENCE_SCHEMA
            || self.record_id.trim().is_empty()
            || self.record_revision == 0
            || self.data_epoch == 0
            || self.reason.trim().is_empty()
            || self
                .deletion_index_digest
                .as_deref()
                .is_some_and(|digest| !valid_memory_deletion_digest(digest))
        {
            return Err("memory_projection_fence_invalid".to_owned());
        }
        let Some(hex) = self.fence_digest.strip_prefix("sha256:") else {
            return Err("memory_projection_fence_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("memory_projection_fence_digest_invalid".to_owned());
        }
        if self.fence_digest != self.digest() {
            return Err("memory_projection_fence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "record_id": self.record_id,
            "record_revision": self.record_revision,
            "project_root": self.project_root,
            "data_epoch": self.data_epoch,
            "policy_revision": self.policy_revision,
            "disposition": self.disposition,
            "reason": self.reason,
            "deletion_index_digest": self.deletion_index_digest,
        }))
    }
}
