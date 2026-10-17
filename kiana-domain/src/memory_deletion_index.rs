//! Read-time deletion index for projected Memory rows.
//!
//! A committed tombstone is not finished when the row disappears. Every derived retrieval surface
//! named by PD-18 — lexical ranking, vector ranking, caches and receipt projections — can still
//! hold a copy, and a restored backup, a stale cache or a re-imported JSONL row can make a deleted
//! object searchable again. This module is the read-time fence that closes that gap: it is a
//! projection of committed deletion facts, bound to the same project, policy revision and data
//! epoch as the memory projection, and it carries one evidence-bound receipt per retrieval surface.
//!
//! The index never deletes, erases, compacts or rebuilds anything. `check_read` only decides
//! whether a row may still be served. A surface that has not produced a receipt stays `Unknown`
//! and keeps the row unservable with an explicit `..._tombstone_revival_denied:<surface>` reason,
//! so a cache that still answers is denied instead of being trusted.

use crate::{json_digest, DataPolicy, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const MEMORY_DELETION_INDEX_SCHEMA: &str = "kiana.memory-deletion-index.v1";
pub const MEMORY_DELETION_INDEX_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_MEMORY_DELETION_ENTRIES: usize = 4096;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    if valid_memory_deletion_digest(value) {
        Ok(())
    } else {
        Err(format!("{field}_invalid"))
    }
}

/// `sha256:` plus 64 lowercase-or-uppercase hex digits; the only accepted evidence shape.
pub fn valid_memory_deletion_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The derived surfaces a memory tombstone has to reach before it is fully propagated.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryRetrievalSurface {
    Lexical,
    Vector,
    Cache,
    Receipt,
}

impl MemoryRetrievalSurface {
    pub const ALL: [Self; 4] = [Self::Lexical, Self::Vector, Self::Cache, Self::Receipt];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Vector => "vector",
            Self::Cache => "cache",
            Self::Receipt => "receipt",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySurfaceState {
    /// The surface produced a receipt for this tombstone; it may not serve the payload again.
    Invalidated,
    /// No receipt yet. Absence of proof is not an acknowledgement.
    Unknown,
}

/// One surface's acknowledgement for one tombstone.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemorySurfaceReceipt {
    pub surface: MemoryRetrievalSurface,
    pub state: MemorySurfaceState,
    pub observed_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
}

impl MemorySurfaceReceipt {
    /// A surface can only claim invalidation together with its receipt digest.
    pub fn fenced(
        surface: MemoryRetrievalSurface,
        observed_at_ms: u64,
        evidence_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let receipt = Self {
            surface,
            state: MemorySurfaceState::Invalidated,
            observed_at_ms,
            evidence_digest: Some(evidence_digest.into()),
        };
        receipt.validate()?;
        Ok(receipt)
    }

    /// The honest default: this surface has not proven anything yet.
    pub fn pending(surface: MemoryRetrievalSurface) -> Self {
        Self {
            surface,
            state: MemorySurfaceState::Unknown,
            observed_at_ms: 0,
            evidence_digest: None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self.state {
            MemorySurfaceState::Invalidated => {
                if self.observed_at_ms == 0 {
                    return Err("memory_deletion_surface_observed_at_required".to_owned());
                }
                match &self.evidence_digest {
                    Some(evidence) => {
                        digest(evidence, "memory_deletion_surface_evidence_digest")?;
                    }
                    None => return Err("memory_deletion_surface_evidence_required".to_owned()),
                }
            }
            MemorySurfaceState::Unknown => {
                if self.observed_at_ms != 0 || self.evidence_digest.is_some() {
                    return Err("memory_deletion_surface_unproven_receipt".to_owned());
                }
            }
        }
        Ok(())
    }

    pub fn invalidated(&self) -> bool {
        self.state == MemorySurfaceState::Invalidated
    }
}

/// One deleted object and the acknowledgement of each retrieval surface.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDeletionEntry {
    pub record_id: String,
    pub collection: String,
    pub record_revision: u64,
    pub tombstone_digest: String,
    pub deleted_at_ms: u64,
    pub surfaces: Vec<MemorySurfaceReceipt>,
}

impl MemoryDeletionEntry {
    pub fn new(
        record_id: impl Into<String>,
        collection: impl Into<String>,
        record_revision: u64,
        tombstone_digest: impl Into<String>,
        deleted_at_ms: u64,
    ) -> Result<Self, String> {
        let entry = Self {
            record_id: record_id.into(),
            collection: collection.into(),
            record_revision,
            tombstone_digest: tombstone_digest.into(),
            deleted_at_ms,
            surfaces: MemoryRetrievalSurface::ALL
                .into_iter()
                .map(MemorySurfaceReceipt::pending)
                .collect(),
        };
        entry.validate()?;
        Ok(entry)
    }

    /// Replace one pending surface with its evidence-bound receipt.
    pub fn fence_surface(
        &mut self,
        surface: MemoryRetrievalSurface,
        observed_at_ms: u64,
        evidence_digest: impl Into<String>,
    ) -> Result<(), String> {
        let receipt = MemorySurfaceReceipt::fenced(surface, observed_at_ms, evidence_digest)?;
        let slot = self
            .surfaces
            .iter_mut()
            .find(|existing| existing.surface == surface)
            .ok_or_else(|| "memory_deletion_surface_missing".to_owned())?;
        *slot = receipt;
        self.validate()
    }

    pub fn validate(&self) -> Result<(), String> {
        required(&self.record_id, "memory_deletion_record_id", 512)?;
        required(&self.collection, "memory_deletion_collection", 128)?;
        if self.record_revision == 0 || self.deleted_at_ms == 0 {
            return Err("memory_deletion_entry_boundary_invalid".to_owned());
        }
        digest(
            &self.tombstone_digest,
            "memory_deletion_entry_tombstone_digest",
        )?;
        if self.surfaces.len() != MemoryRetrievalSurface::ALL.len() {
            return Err("memory_deletion_surface_set_incomplete".to_owned());
        }
        let mut seen = BTreeSet::new();
        for surface in &self.surfaces {
            surface.validate()?;
            if !seen.insert(surface.surface) {
                return Err("memory_deletion_surface_duplicate".to_owned());
            }
        }
        if MemoryRetrievalSurface::ALL
            .iter()
            .any(|surface| !seen.contains(surface))
        {
            return Err("memory_deletion_surface_set_incomplete".to_owned());
        }
        Ok(())
    }

    /// A tombstone covers the record it deleted and every later revision of that record.
    pub fn covers(&self, record_id: &str, record_revision: u64) -> bool {
        self.record_id == record_id && self.record_revision >= record_revision
    }

    pub fn pending_surfaces(&self) -> Vec<&'static str> {
        MemoryRetrievalSurface::ALL
            .into_iter()
            .filter(|surface| {
                !self
                    .surfaces
                    .iter()
                    .any(|receipt| receipt.surface == *surface && receipt.invalidated())
            })
            .map(MemoryRetrievalSurface::as_str)
            .collect()
    }

    pub fn fully_invalidated(&self) -> bool {
        self.pending_surfaces().is_empty()
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "record_id": self.record_id,
            "collection": self.collection,
            "record_revision": self.record_revision,
            "tombstone_digest": self.tombstone_digest,
            "deleted_at_ms": self.deleted_at_ms,
            "surfaces": self.surfaces,
        }))
    }
}

/// Merge two entry sets by record id. The higher revision wins; on a tie the set with the most
/// evidence-bound surfaces wins, so re-reading an older index can never drop a receipt.
pub fn merge_memory_deletion_entries(
    existing: &[MemoryDeletionEntry],
    incoming: &[MemoryDeletionEntry],
) -> Vec<MemoryDeletionEntry> {
    let mut merged: BTreeMap<String, MemoryDeletionEntry> = BTreeMap::new();
    for entry in existing.iter().chain(incoming.iter()) {
        let replace = match merged.get(&entry.record_id) {
            Some(current) => {
                current.record_revision < entry.record_revision
                    || (current.record_revision == entry.record_revision
                        && current.pending_surfaces().len() > entry.pending_surfaces().len())
            }
            None => true,
        };
        if replace {
            merged.insert(entry.record_id.clone(), entry.clone());
        }
    }
    merged.into_values().collect()
}

/// Projection of committed deletion facts plus persisted surface receipts, fenced to one project.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDeletionIndex {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_root: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub observed_at_ms: u64,
    pub entries: Vec<MemoryDeletionEntry>,
    pub index_digest: String,
}

impl MemoryDeletionIndex {
    pub fn new(
        project_root: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        observed_at_ms: u64,
        entries: Vec<MemoryDeletionEntry>,
    ) -> Result<Self, String> {
        let mut index = Self {
            schema: MEMORY_DELETION_INDEX_SCHEMA.to_owned(),
            version: MEMORY_DELETION_INDEX_VERSION,
            project_root: project_root.into(),
            policy_revision,
            data_epoch,
            observed_at_ms,
            entries,
            index_digest: String::new(),
        };
        index.index_digest = index.digest();
        index.validate()?;
        Ok(index)
    }

    pub fn empty(
        project_root: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        observed_at_ms: u64,
    ) -> Result<Self, String> {
        Self::new(
            project_root,
            policy_revision,
            data_epoch,
            observed_at_ms,
            Vec::new(),
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_DELETION_INDEX_SCHEMA
            || !self
                .version
                .is_compatible_with(&MEMORY_DELETION_INDEX_VERSION)
            || self.data_epoch == 0
            || self.observed_at_ms == 0
            || self.entries.len() > MAX_MEMORY_DELETION_ENTRIES
        {
            return Err("memory_deletion_index_header_invalid".to_owned());
        }
        required(&self.project_root, "memory_deletion_index_project", 1024)?;
        let mut ids = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(entry.record_id.clone()) {
                return Err("memory_deletion_index_entry_duplicate".to_owned());
            }
        }
        if !valid_memory_deletion_digest(&self.index_digest) {
            return Err("memory_deletion_index_digest_invalid".to_owned());
        }
        if self.index_digest != self.digest() {
            return Err("memory_deletion_index_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Same project, same governance epoch. A newer policy revision is fine; a newer index is not,
    /// because it would claim deletions the current policy cannot have produced yet.
    pub fn bind_policy(&self, policy: &DataPolicy) -> Result<(), String> {
        if self.data_epoch != policy.data_epoch {
            return Err("memory_deletion_index_epoch_stale".to_owned());
        }
        if self.policy_revision > policy.revision {
            return Err("memory_deletion_index_policy_ahead".to_owned());
        }
        Ok(())
    }

    /// String or canonical-path comparison; a different directory is never the same index scope.
    pub fn project_root_conflicts(&self, requested_project_root: &str) -> bool {
        if requested_project_root.trim().is_empty() || self.project_root == requested_project_root {
            return false;
        }
        if self.project_root.trim().is_empty() {
            return false;
        }
        let left = std::fs::canonicalize(&self.project_root);
        let right = std::fs::canonicalize(requested_project_root);
        match (left, right) {
            (Ok(left), Ok(right)) => left != right,
            _ => true,
        }
    }

    pub fn entry_covering(
        &self,
        record_id: &str,
        record_revision: u64,
    ) -> Option<&MemoryDeletionEntry> {
        self.entries
            .iter()
            .find(|entry| entry.covers(record_id, record_revision))
    }

    /// Deny-first read decision: a covered row is never searchable, whatever it looks like.
    pub fn check_read(&self, record_id: &str, record_revision: u64) -> Result<(), String> {
        let Some(entry) = self.entry_covering(record_id, record_revision) else {
            return Ok(());
        };
        let pending = entry.pending_surfaces();
        match pending.first() {
            Some(surface) => Err(format!(
                "memory_deletion_tombstone_revival_denied:{surface}"
            )),
            None => Err("memory_deletion_tombstoned".to_owned()),
        }
    }

    pub fn pending_surfaces(&self) -> Vec<&'static str> {
        MemoryRetrievalSurface::ALL
            .into_iter()
            .filter(|surface| {
                self.entries
                    .iter()
                    .any(|entry| entry.pending_surfaces().contains(&surface.as_str()))
            })
            .map(MemoryRetrievalSurface::as_str)
            .collect()
    }

    pub fn tombstones(&self) -> usize {
        self.entries.len()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "project_root": self.project_root,
            "policy_revision": self.policy_revision,
            "data_epoch": self.data_epoch,
            "observed_at_ms": self.observed_at_ms,
            "entries": self.entries,
        }))
    }
}
