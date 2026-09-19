//! One epoch-fenced invalidation plan for memory, indexes, context and historical receipts.
//!
//! This is a propagation contract, not a filesystem eraser.  Adapters may complete individual
//! targets later, but every target is denied for reinjection as soon as the plan is committed.

use crate::{json_digest, DeletionTombstone, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const MEMORY_INVALIDATION_SCHEMA: &str = "kiana.memory-invalidation.v1";
pub const HISTORICAL_RECEIPT_INVALIDATION_SCHEMA: &str = "kiana.historical-receipt-invalidation.v1";
pub const MEMORY_INVALIDATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_MEMORY_INVALIDATION_TARGETS: usize = 32;
pub const MAX_HISTORICAL_RECEIPTS: usize = 512;

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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryInvalidationKind {
    Deleted,
    Expired,
    Revoked,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPropagationTarget {
    MemoryJsonl,
    MemoryBody,
    Bm25Index,
    DenseIndex,
    RepoIndex,
    ContextPlan,
    Summary,
    Checkpoint,
    PromptCache,
    Ui,
    HistoricalReceipt,
}

impl MemoryPropagationTarget {
    pub const ALL: [Self; 11] = [
        Self::MemoryJsonl,
        Self::MemoryBody,
        Self::Bm25Index,
        Self::DenseIndex,
        Self::RepoIndex,
        Self::ContextPlan,
        Self::Summary,
        Self::Checkpoint,
        Self::PromptCache,
        Self::Ui,
        Self::HistoricalReceipt,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MemoryJsonl => "memory_jsonl",
            Self::MemoryBody => "memory_body",
            Self::Bm25Index => "bm25_index",
            Self::DenseIndex => "dense_index",
            Self::RepoIndex => "repo_index",
            Self::ContextPlan => "context_plan",
            Self::Summary => "summary",
            Self::Checkpoint => "checkpoint",
            Self::PromptCache => "prompt_cache",
            Self::Ui => "ui",
            Self::HistoricalReceipt => "historical_receipt",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPropagationState {
    Invalidated,
    PreservedInvalid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryPropagationRecord {
    pub target: MemoryPropagationTarget,
    pub state: MemoryPropagationState,
    pub data_epoch: u64,
    pub invalidated_at_ms: u64,
    pub tombstone_digest: String,
    pub reinjection_allowed: bool,
}

impl MemoryPropagationRecord {
    fn validate(&self, expected_epoch: u64, tombstone_digest: &str) -> Result<(), String> {
        if self.data_epoch != expected_epoch
            || self.invalidated_at_ms == 0
            || self.tombstone_digest != tombstone_digest
            || self.reinjection_allowed
        {
            return Err("memory_propagation_record_invalid".to_owned());
        }
        digest(
            &self.tombstone_digest,
            "memory_propagation_tombstone_digest",
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoricalReceiptInvalidation {
    pub schema: String,
    pub receipt_id: String,
    pub candidate_id: String,
    pub source_revision: String,
    pub state: MemoryPropagationState,
    pub deleted_at_ms: u64,
    pub reinjection_allowed: bool,
    pub invalidation_digest: String,
}

impl HistoricalReceiptInvalidation {
    pub fn new(
        receipt_id: impl Into<String>,
        candidate_id: impl Into<String>,
        source_revision: impl Into<String>,
        deleted_at_ms: u64,
    ) -> Result<Self, String> {
        let mut receipt = Self {
            schema: HISTORICAL_RECEIPT_INVALIDATION_SCHEMA.to_owned(),
            receipt_id: receipt_id.into(),
            candidate_id: candidate_id.into(),
            source_revision: source_revision.into(),
            state: MemoryPropagationState::PreservedInvalid,
            deleted_at_ms,
            reinjection_allowed: false,
            invalidation_digest: String::new(),
        };
        receipt.invalidation_digest = receipt.digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HISTORICAL_RECEIPT_INVALIDATION_SCHEMA
            || self.state != MemoryPropagationState::PreservedInvalid
            || self.deleted_at_ms == 0
            || self.reinjection_allowed
        {
            return Err("historical_receipt_invalidation_invalid".to_owned());
        }
        required(&self.receipt_id, "historical_receipt_id", 256)?;
        required(&self.candidate_id, "historical_receipt_candidate_id", 512)?;
        required(
            &self.source_revision,
            "historical_receipt_source_revision",
            256,
        )?;
        digest(
            &self.invalidation_digest,
            "historical_receipt_invalidation_digest",
        )?;
        if self.invalidation_digest != self.digest() {
            return Err("historical_receipt_invalidation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "candidate_id": self.candidate_id,
            "source_revision": self.source_revision,
            "state": self.state,
            "deleted_at_ms": self.deleted_at_ms,
            "reinjection_allowed": self.reinjection_allowed,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryInvalidationPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: MemoryInvalidationKind,
    pub object_ref: String,
    pub source_digest: String,
    pub tombstone_digest: String,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    pub invalidated_at_ms: u64,
    pub targets: Vec<MemoryPropagationRecord>,
    pub historical_receipts: Vec<HistoricalReceiptInvalidation>,
    pub plan_digest: String,
}

impl MemoryInvalidationPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: MemoryInvalidationKind,
        object_ref: impl Into<String>,
        source_digest: impl Into<String>,
        tombstone_digest: impl Into<String>,
        previous_epoch: u64,
        data_epoch: u64,
        invalidated_at_ms: u64,
        historical_receipts: Vec<HistoricalReceiptInvalidation>,
    ) -> Result<Self, String> {
        let tombstone_digest = tombstone_digest.into();
        let targets = MemoryPropagationTarget::ALL
            .into_iter()
            .map(|target| MemoryPropagationRecord {
                target,
                state: if target == MemoryPropagationTarget::HistoricalReceipt {
                    MemoryPropagationState::PreservedInvalid
                } else {
                    MemoryPropagationState::Invalidated
                },
                data_epoch,
                invalidated_at_ms,
                tombstone_digest: tombstone_digest.clone(),
                reinjection_allowed: false,
            })
            .collect();
        let mut plan = Self {
            schema: MEMORY_INVALIDATION_SCHEMA.to_owned(),
            version: MEMORY_INVALIDATION_VERSION,
            kind,
            object_ref: object_ref.into(),
            source_digest: source_digest.into(),
            tombstone_digest,
            previous_epoch,
            data_epoch,
            invalidated_at_ms,
            targets,
            historical_receipts,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn from_tombstone(
        tombstone: &DeletionTombstone,
        kind: MemoryInvalidationKind,
        invalidated_at_ms: u64,
        historical_receipts: Vec<HistoricalReceiptInvalidation>,
    ) -> Result<Self, String> {
        tombstone.validate()?;
        Self::new(
            kind,
            tombstone.object_ref.clone(),
            tombstone.source_digest.clone(),
            tombstone.tombstone_digest.clone(),
            tombstone.data_epoch.saturating_sub(1).max(1),
            tombstone.data_epoch,
            invalidated_at_ms,
            historical_receipts,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_INVALIDATION_SCHEMA
            || !self
                .version
                .is_compatible_with(&MEMORY_INVALIDATION_VERSION)
            || self.previous_epoch == 0
            || self.data_epoch <= self.previous_epoch
            || self.invalidated_at_ms == 0
            || self.targets.len() != MemoryPropagationTarget::ALL.len()
            || self.historical_receipts.len() > MAX_HISTORICAL_RECEIPTS
        {
            return Err("memory_invalidation_header_invalid".to_owned());
        }
        required(&self.object_ref, "memory_invalidation_object_ref", 512)?;
        digest(&self.source_digest, "memory_invalidation_source_digest")?;
        digest(
            &self.tombstone_digest,
            "memory_invalidation_tombstone_digest",
        )?;
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            target.validate(self.data_epoch, &self.tombstone_digest)?;
            if !targets.insert(target.target) {
                return Err("memory_invalidation_target_duplicate".to_owned());
            }
            if target.target == MemoryPropagationTarget::HistoricalReceipt
                && target.state != MemoryPropagationState::PreservedInvalid
            {
                return Err("memory_invalidation_receipt_state_invalid".to_owned());
            }
            if target.target != MemoryPropagationTarget::HistoricalReceipt
                && target.state != MemoryPropagationState::Invalidated
            {
                return Err("memory_invalidation_target_state_invalid".to_owned());
            }
        }
        if targets.len() != MemoryPropagationTarget::ALL.len()
            || MemoryPropagationTarget::ALL
                .iter()
                .any(|target| !targets.contains(target))
        {
            return Err("memory_invalidation_target_set_incomplete".to_owned());
        }
        let mut receipt_ids = BTreeSet::new();
        for receipt in &self.historical_receipts {
            receipt.validate()?;
            if !receipt_ids.insert(receipt.receipt_id.clone()) {
                return Err("memory_invalidation_receipt_duplicate".to_owned());
            }
        }
        digest(&self.plan_digest, "memory_invalidation_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("memory_invalidation_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn can_reinject(&self, receipt_id: &str, candidate_id: &str) -> bool {
        self.historical_receipts
            .iter()
            .find(|receipt| {
                receipt.receipt_id == receipt_id && receipt.candidate_id == candidate_id
            })
            .is_some_and(|receipt| receipt.reinjection_allowed)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "object_ref": self.object_ref,
            "source_digest": self.source_digest,
            "tombstone_digest": self.tombstone_digest,
            "previous_epoch": self.previous_epoch,
            "data_epoch": self.data_epoch,
            "invalidated_at_ms": self.invalidated_at_ms,
            "targets": self.targets,
            "historical_receipts": self.historical_receipts,
        }))
    }
}
