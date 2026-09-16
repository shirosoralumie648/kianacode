//! Server-owned memory mutation contracts and a deterministic CAS/idempotency ledger.
//!
//! A mutation is an intent, not an authorization grant.  The ControlPlane supplies the
//! authenticated scope, policy/data epochs and evidence before a handler can prepare a write.
//! The ledger below is deliberately side-effect free so the deny/replay/CAS rules can be shared
//! by JSONL and EventStore adapters without creating a second source of truth.

use crate::{json_digest, MemoryScope, SourceRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MEMORY_MUTATION_SCHEMA: &str = "kiana.memory-mutation.v1";
pub const MEMORY_MUTATION_RECEIPT_SCHEMA: &str = "kiana.memory-mutation-receipt.v1";
pub const MAX_MEMORY_MUTATION_TARGETS: usize = 64;
pub const MAX_MEMORY_MUTATION_EVIDENCE: usize = 64;

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

/// The only mutation verbs accepted by the memory authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MemoryMutationOperation {
    Add,
    Update,
    Delete,
    Approve,
    Publish,
    Expire,
    Revoke,
}

/// Compatibility aliases for callers that use the shorter domain terminology.
pub type MemoryMutationKind = MemoryMutationOperation;

impl MemoryMutationOperation {
    pub const ALL: [Self; 7] = [
        Self::Add,
        Self::Update,
        Self::Delete,
        Self::Approve,
        Self::Publish,
        Self::Expire,
        Self::Revoke,
    ];

    pub fn requires_existing_revision(self) -> bool {
        !matches!(self, Self::Add)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Add => "ADD",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
            Self::Approve => "APPROVE",
            Self::Publish => "PUBLISH",
            Self::Expire => "EXPIRE",
            Self::Revoke => "REVOKE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "ADD" => Some(Self::Add),
            "UPDATE" => Some(Self::Update),
            "DELETE" => Some(Self::Delete),
            "APPROVE" => Some(Self::Approve),
            "PUBLISH" => Some(Self::Publish),
            "EXPIRE" => Some(Self::Expire),
            "REVOKE" => Some(Self::Revoke),
            _ => None,
        }
    }
}

/// A single target and its exact expected aggregate revision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationTarget {
    pub record_id: String,
    pub collection: String,
    pub expected_revision: u64,
}

impl MemoryMutationTarget {
    pub fn new(
        record_id: impl Into<String>,
        collection: impl Into<String>,
        expected_revision: u64,
    ) -> Self {
        Self {
            record_id: record_id.into(),
            collection: collection.into(),
            expected_revision,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        required(&self.record_id, "memory_mutation_record_id", 256)?;
        required(&self.collection, "memory_mutation_collection", 256)?;
        let parsed = crate::MemoryCollection::parse(&self.collection)
            .ok_or_else(|| "memory_mutation_collection_invalid".to_owned())?;
        if parsed.collection != self.collection.trim() {
            return Err("memory_mutation_collection_noncanonical".to_owned());
        }
        Ok(())
    }

    pub fn key(&self) -> String {
        format!("{}\u{1f}{}", self.collection, self.record_id)
    }
}

/// A normalized, auditable memory intent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutation {
    pub schema: String,
    pub mutation_id: String,
    pub operation: MemoryMutationOperation,
    /// Server-resolved actor. It must match the principal carried by `scope`.
    pub actor: String,
    pub scope: MemoryScope,
    pub expected_revisions: Vec<MemoryMutationTarget>,
    pub evidence: Vec<SourceRef>,
    pub policy_epoch: u64,
    pub data_epoch: u64,
    pub idempotency_key: String,
    /// Digest of the protected payload prepared by the adapter; raw正文 is not in this intent.
    pub payload_digest: String,
}

impl MemoryMutation {
    /// Build a mutation from protected payload bytes without retaining the payload in the
    /// authority journal. The digest is still part of the command identity.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mutation_id: impl Into<String>,
        operation: MemoryMutationOperation,
        actor: impl Into<String>,
        scope: MemoryScope,
        expected_revisions: Vec<MemoryMutationTarget>,
        evidence: Vec<SourceRef>,
        policy_epoch: u64,
        data_epoch: u64,
        idempotency_key: impl Into<String>,
        payload: &Value,
    ) -> Result<Self, String> {
        Self::with_payload_digest(
            mutation_id,
            operation,
            actor,
            scope,
            expected_revisions,
            evidence,
            policy_epoch,
            data_epoch,
            idempotency_key,
            json_digest(payload),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_payload_digest(
        mutation_id: impl Into<String>,
        operation: MemoryMutationOperation,
        actor: impl Into<String>,
        scope: MemoryScope,
        expected_revisions: Vec<MemoryMutationTarget>,
        evidence: Vec<SourceRef>,
        policy_epoch: u64,
        data_epoch: u64,
        idempotency_key: impl Into<String>,
        payload_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mutation = Self {
            schema: MEMORY_MUTATION_SCHEMA.to_owned(),
            mutation_id: mutation_id.into(),
            operation,
            actor: actor.into(),
            scope,
            expected_revisions,
            evidence,
            policy_epoch,
            data_epoch,
            idempotency_key: idempotency_key.into(),
            payload_digest: payload_digest.into(),
        };
        mutation.validate()?;
        Ok(mutation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_MUTATION_SCHEMA {
            return Err("memory_mutation_schema_invalid".to_owned());
        }
        required(&self.mutation_id, "memory_mutation_id", 256)?;
        required(&self.actor, "memory_mutation_actor", 256)?;
        required(
            &self.idempotency_key,
            "memory_mutation_idempotency_key",
            256,
        )?;
        digest(&self.payload_digest, "memory_mutation_payload_digest")?;
        if self.policy_epoch == 0 || self.data_epoch == 0 {
            return Err("memory_mutation_epoch_invalid".to_owned());
        }
        self.scope.validate()?;
        if !self.scope.allow_write {
            return Err("memory_mutation_write_scope_required".to_owned());
        }
        if self.actor != self.scope.principal.principal_id {
            return Err("memory_mutation_actor_scope_mismatch".to_owned());
        }
        if self.expected_revisions.is_empty()
            || self.expected_revisions.len() > MAX_MEMORY_MUTATION_TARGETS
        {
            return Err("memory_mutation_target_limit".to_owned());
        }
        let mut targets = BTreeSet::new();
        for target in &self.expected_revisions {
            target.validate()?;
            if !targets.insert(target.key()) {
                return Err("memory_mutation_target_duplicate".to_owned());
            }
            let collection = crate::MemoryCollection::parse(&target.collection)
                .ok_or_else(|| "memory_mutation_collection_invalid".to_owned())?;
            if !self.scope.allows_collection(&collection) {
                return Err("memory_mutation_scope_denied".to_owned());
            }
            if self.operation.requires_existing_revision() {
                if target.expected_revision == 0 {
                    return Err("memory_mutation_expected_revision_required".to_owned());
                }
            } else if target.expected_revision != 0 {
                return Err("memory_mutation_add_revision_must_be_zero".to_owned());
            }
        }
        if self.evidence.is_empty() || self.evidence.len() > MAX_MEMORY_MUTATION_EVIDENCE {
            return Err("memory_mutation_evidence_required".to_owned());
        }
        let mut evidence = BTreeSet::new();
        for source in &self.evidence {
            source.validate()?;
            if !evidence.insert(source.digest()) {
                return Err("memory_mutation_evidence_duplicate".to_owned());
            }
        }
        Ok(())
    }

    /// Stable command identity used to reject a changed payload under a reused idempotency key.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::to_value(self).unwrap_or(Value::Null))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationTargetRevision {
    pub record_id: String,
    pub collection: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationReceipt {
    pub schema: String,
    pub mutation_id: String,
    pub operation: MemoryMutationOperation,
    pub actor: String,
    pub scope_digest: String,
    pub idempotency_key: String,
    pub mutation_digest: String,
    pub committed_revision: u64,
    pub target_revisions: Vec<MemoryMutationTargetRevision>,
    pub policy_epoch: u64,
    pub data_epoch: u64,
}

impl MemoryMutationReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_MUTATION_RECEIPT_SCHEMA {
            return Err("memory_mutation_receipt_schema_invalid".to_owned());
        }
        required(&self.mutation_id, "memory_mutation_receipt_id", 256)?;
        required(
            &self.idempotency_key,
            "memory_mutation_receipt_idempotency_key",
            256,
        )?;
        required(&self.actor, "memory_mutation_receipt_actor", 256)?;
        digest(&self.scope_digest, "memory_mutation_receipt_scope_digest")?;
        digest(
            &self.mutation_digest,
            "memory_mutation_receipt_mutation_digest",
        )?;
        if self.committed_revision == 0 || self.policy_epoch == 0 || self.data_epoch == 0 {
            return Err("memory_mutation_receipt_revision_invalid".to_owned());
        }
        if self.target_revisions.is_empty()
            || self.target_revisions.len() > MAX_MEMORY_MUTATION_TARGETS
        {
            return Err("memory_mutation_receipt_target_limit".to_owned());
        }
        for target in &self.target_revisions {
            required(&target.record_id, "memory_mutation_receipt_record_id", 256)?;
            required(
                &target.collection,
                "memory_mutation_receipt_collection",
                256,
            )?;
            let collection = crate::MemoryCollection::parse(&target.collection)
                .ok_or_else(|| "memory_mutation_receipt_collection_invalid".to_owned())?;
            if collection.collection != target.collection.trim() {
                return Err("memory_mutation_receipt_collection_noncanonical".to_owned());
            }
            if target.revision == 0 {
                return Err("memory_mutation_receipt_target_revision_invalid".to_owned());
            }
        }
        let mut targets = BTreeSet::new();
        if self.target_revisions.iter().any(|target| {
            !targets.insert(format!("{}\u{1f}{}", target.collection, target.record_id))
        }) {
            return Err("memory_mutation_receipt_target_duplicate".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, mutation: &MemoryMutation) -> Result<(), String> {
        mutation.validate()?;
        self.validate()?;
        if self.mutation_id != mutation.mutation_id
            || self.operation != mutation.operation
            || self.actor != mutation.actor
            || self.scope_digest != mutation.scope.scope_digest
            || self.idempotency_key != mutation.idempotency_key
            || self.mutation_digest != mutation.digest()
            || self.policy_epoch != mutation.policy_epoch
            || self.data_epoch != mutation.data_epoch
        {
            return Err("memory_mutation_receipt_identity_mismatch".to_owned());
        }
        if self.target_revisions.len() != mutation.expected_revisions.len()
            || self
                .target_revisions
                .iter()
                .zip(&mutation.expected_revisions)
                .any(|(actual, expected)| {
                    actual.record_id != expected.record_id
                        || actual.collection != expected.collection
                        || actual.revision != expected.expected_revision.saturating_add(1)
                })
        {
            return Err("memory_mutation_receipt_targets_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryMutationOutcome {
    Committed { receipt: MemoryMutationReceipt },
    Replayed { original: MemoryMutationReceipt },
}

pub type MemoryMutationResult = MemoryMutationOutcome;

/// A pure in-memory model of the CAS/idempotency rules shared by storage adapters.
#[derive(Clone, Debug, Default)]
pub struct MemoryMutationLedger {
    revisions: BTreeMap<String, u64>,
    receipts: BTreeMap<String, MemoryMutationReceipt>,
}

impl MemoryMutationLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed the current projection before applying a new mutation. This is a read-only snapshot
    /// operation; it never creates a mutation receipt or grants write authority.
    pub fn seed_record(
        &mut self,
        record_id: impl Into<String>,
        collection: impl Into<String>,
        revision: u64,
    ) -> Result<(), String> {
        let target = MemoryMutationTarget::new(record_id, collection, revision);
        target.validate()?;
        if revision == 0 {
            return Err("memory_mutation_seed_revision_invalid".to_owned());
        }
        self.revisions.insert(target.key(), revision);
        Ok(())
    }

    pub fn current_revision(&self, record_id: &str, collection: &str) -> u64 {
        let key = MemoryMutationTarget::new(record_id, collection, 0).key();
        self.revisions.get(&key).copied().unwrap_or(0)
    }

    /// Validate every target before changing any revision. A stale member therefore cannot
    /// partially advance a batch or turn the operation into last-write-wins.
    pub fn preflight(
        &self,
        mutation: &MemoryMutation,
    ) -> Result<Vec<MemoryMutationTargetRevision>, String> {
        mutation.validate()?;
        let mut next = Vec::with_capacity(mutation.expected_revisions.len());
        for target in &mutation.expected_revisions {
            let current = self.revisions.get(&target.key()).copied().unwrap_or(0);
            if current != target.expected_revision {
                return Err(format!(
                    "memory_mutation_revision_conflict:{}:{}:expected={}:actual={}",
                    target.collection, target.record_id, target.expected_revision, current
                ));
            }
            let revision = current
                .checked_add(1)
                .ok_or_else(|| "memory_mutation_revision_exhausted".to_owned())?;
            next.push(MemoryMutationTargetRevision {
                record_id: target.record_id.clone(),
                collection: target.collection.clone(),
                revision,
            });
        }
        Ok(next)
    }

    pub fn apply(&mut self, mutation: MemoryMutation) -> Result<MemoryMutationOutcome, String> {
        mutation.validate()?;
        let digest = mutation.digest();
        if let Some(original) = self.receipts.get(&mutation.idempotency_key) {
            if original.mutation_digest == digest {
                return Ok(MemoryMutationOutcome::Replayed {
                    original: original.clone(),
                });
            }
            return Err("memory_mutation_idempotency_conflict".to_owned());
        }
        let target_revisions = self.preflight(&mutation)?;
        let committed_revision = target_revisions
            .iter()
            .map(|target| target.revision)
            .max()
            .unwrap_or(0);
        let receipt = MemoryMutationReceipt {
            schema: MEMORY_MUTATION_RECEIPT_SCHEMA.to_owned(),
            mutation_id: mutation.mutation_id.clone(),
            operation: mutation.operation,
            actor: mutation.actor.clone(),
            scope_digest: mutation.scope.scope_digest.clone(),
            idempotency_key: mutation.idempotency_key.clone(),
            mutation_digest: digest,
            committed_revision,
            target_revisions,
            policy_epoch: mutation.policy_epoch,
            data_epoch: mutation.data_epoch,
        };
        receipt.validate()?;
        // All checks, including every target, completed before this first state mutation.
        for target in &receipt.target_revisions {
            self.revisions.insert(
                MemoryMutationTarget::new(
                    target.record_id.clone(),
                    target.collection.clone(),
                    target.revision,
                )
                .key(),
                target.revision,
            );
        }
        self.receipts
            .insert(mutation.idempotency_key, receipt.clone());
        Ok(MemoryMutationOutcome::Committed { receipt })
    }

    pub fn receipt_for(&self, idempotency_key: &str) -> Option<&MemoryMutationReceipt> {
        self.receipts.get(idempotency_key)
    }
}
