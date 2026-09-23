//! Redacted user-memory workbench projections and governed bulk mutation contracts.
//!
//! The workbench is a read/plan surface.  It never grants Memory authority, writes a record or
//! treats a UI deletion as physical erasure.  Lists are built from the server ACL decision,
//! bulk plans reuse `MemoryMutation` expected revisions, and exports carry bounded redacted rows
//! plus digests rather than an ungoverned copy of Memory.

use crate::{
    json_digest, redact_text, MemoryAccessPath, MemoryAclDecision, MemoryAclRequest,
    MemoryAdmission, MemoryClassification, MemoryMutation, MemoryMutationAuthority,
    MemoryMutationOperation, MemoryRecord, MemorySensitivity, MemoryState, MemoryVisibility,
    SchemaVersion, MEMORY_LAYER_USER,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const MEMORY_WORKBENCH_LIST_SCHEMA: &str = "kiana.memory-workbench-list.v1";
pub const MEMORY_BULK_MUTATION_PLAN_SCHEMA: &str = "kiana.memory-bulk-mutation-plan.v1";
pub const MEMORY_BULK_MUTATION_RESULT_SCHEMA: &str = "kiana.memory-bulk-mutation-result.v1";
pub const MEMORY_EXPORT_SCHEMA: &str = "kiana.memory-export.v1";
pub const MEMORY_WORKBENCH_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_WORKBENCH_ENTRIES: usize = 512;
pub const MAX_WORKBENCH_RELATED: usize = 32;
pub const MAX_BULK_MUTATIONS: usize = 64;
pub const MAX_EXPORT_ROWS: usize = 512;

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

fn content_digest(record: &MemoryRecord) -> String {
    if record.content_hash.starts_with("sha256:") {
        record.content_hash.clone()
    } else {
        json_digest(&json!({"text": record.text}))
    }
}

fn private_record(record: &MemoryRecord) -> bool {
    record.layer == MEMORY_LAYER_USER || record.classification == MemoryClassification::UserPrivate
}

fn bounded_preview(text: &str) -> Result<String, String> {
    if text.as_bytes().contains(&0) {
        return Err("memory_workbench_preview_nul_forbidden".to_owned());
    }
    Ok(text.chars().take(512).collect())
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryWorkbenchRelations {
    pub similar_record_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_set_id: Option<String>,
}

impl MemoryWorkbenchRelations {
    fn validate(&self, record_id: &str) -> Result<(), String> {
        if self.similar_record_ids.len() > MAX_WORKBENCH_RELATED {
            return Err("memory_workbench_similar_limit".to_owned());
        }
        let mut seen = BTreeSet::new();
        for related in &self.similar_record_ids {
            required(related, "memory_workbench_similar_id", 256)?;
            if related == record_id || !seen.insert(related) {
                return Err("memory_workbench_similar_duplicate".to_owned());
            }
        }
        if let Some(conflict_set_id) = &self.conflict_set_id {
            required(conflict_set_id, "memory_workbench_conflict_set", 256)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryWorkbenchListItem {
    pub schema: String,
    pub version: SchemaVersion,
    pub record_id: String,
    pub collection: String,
    pub revision: u64,
    pub admission_state: MemoryAdmission,
    pub state: MemoryState,
    pub classification: MemoryClassification,
    pub sensitivity: MemorySensitivity,
    pub origin: crate::MemoryOrigin,
    pub visibility: MemoryVisibility,
    pub content_digest: String,
    /// Private records never carry a preview.  Public/internal previews are still redacted and
    /// bounded before they reach a workbench or export adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub preview_redacted: bool,
    pub similar_record_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_set_id: Option<String>,
    pub item_digest: String,
}

impl MemoryWorkbenchListItem {
    pub fn from_record(
        record: &MemoryRecord,
        decision: &MemoryAclDecision,
        relations: MemoryWorkbenchRelations,
    ) -> Result<Self, String> {
        record.validate_lifecycle()?;
        decision.validate()?;
        if !decision.allowed
            || decision.record_id != record.id
            || decision.record_revision != record.revision
        {
            return Err("memory_workbench_record_denied".to_owned());
        }
        relations.validate(&record.id)?;
        let private = private_record(record);
        let (preview, preview_redacted) = if private {
            (None, true)
        } else {
            let bounded = bounded_preview(&record.text)?;
            let redacted = redact_text(&bounded);
            (Some(redacted), true)
        };
        let mut item = Self {
            schema: MEMORY_WORKBENCH_LIST_SCHEMA.to_owned(),
            version: MEMORY_WORKBENCH_VERSION,
            record_id: record.id.clone(),
            collection: record.collection.clone(),
            revision: record.revision,
            admission_state: record.admission_state,
            state: record.state,
            classification: record.classification,
            sensitivity: record.sensitivity,
            origin: record.origin,
            visibility: record.visibility(),
            content_digest: content_digest(record),
            preview,
            preview_redacted,
            similar_record_ids: relations.similar_record_ids,
            conflict_set_id: relations.conflict_set_id,
            item_digest: String::new(),
        };
        item.item_digest = item.digest();
        item.validate()?;
        Ok(item)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_WORKBENCH_LIST_SCHEMA
            || self.version != MEMORY_WORKBENCH_VERSION
            || self.revision == 0
        {
            return Err("memory_workbench_item_header_invalid".to_owned());
        }
        required(&self.record_id, "memory_workbench_record_id", 256)?;
        required(&self.collection, "memory_workbench_collection", 256)?;
        let collection = crate::MemoryCollection::parse(&self.collection)
            .ok_or_else(|| "memory_workbench_collection_invalid".to_owned())?;
        if collection.collection != self.collection.trim() {
            return Err("memory_workbench_collection_noncanonical".to_owned());
        }
        digest(&self.content_digest, "memory_workbench_content_digest")?;
        let private = self.classification == MemoryClassification::UserPrivate
            || collection.layer == MEMORY_LAYER_USER;
        if private && (self.preview.is_some() || !self.preview_redacted) {
            return Err("memory_workbench_private_preview_leak".to_owned());
        }
        if let Some(preview) = &self.preview {
            if preview.chars().count() > 512 || preview.as_bytes().contains(&0) {
                return Err("memory_workbench_preview_invalid".to_owned());
            }
            if redact_text(preview) != *preview {
                return Err("memory_workbench_preview_not_redacted".to_owned());
            }
        }
        let relations = MemoryWorkbenchRelations {
            similar_record_ids: self.similar_record_ids.clone(),
            conflict_set_id: self.conflict_set_id.clone(),
        };
        relations.validate(&self.record_id)?;
        digest(&self.item_digest, "memory_workbench_item_digest")?;
        if self.item_digest != self.digest() {
            return Err("memory_workbench_item_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "record_id": self.record_id,
            "collection": self.collection,
            "revision": self.revision,
            "admission_state": self.admission_state,
            "state": self.state,
            "classification": self.classification,
            "sensitivity": self.sensitivity,
            "origin": self.origin,
            "visibility": self.visibility,
            "content_digest": self.content_digest,
            "preview": self.preview,
            "preview_redacted": self.preview_redacted,
            "similar_record_ids": self.similar_record_ids,
            "conflict_set_id": self.conflict_set_id,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryWorkbenchList {
    pub schema: String,
    pub version: SchemaVersion,
    pub list_id: String,
    pub scope_digest: String,
    pub data_epoch: u64,
    pub query: String,
    pub query_digest: String,
    pub entries: Vec<MemoryWorkbenchListItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub list_digest: String,
}

impl MemoryWorkbenchList {
    pub fn new(
        list_id: impl Into<String>,
        scope_digest: impl Into<String>,
        data_epoch: u64,
        query: impl Into<String>,
        mut entries: Vec<MemoryWorkbenchListItem>,
        next_cursor: Option<String>,
    ) -> Result<Self, String> {
        entries.sort_by(|left, right| {
            left.collection
                .cmp(&right.collection)
                .then_with(|| left.record_id.cmp(&right.record_id))
        });
        let mut list = Self {
            schema: MEMORY_WORKBENCH_LIST_SCHEMA.to_owned(),
            version: MEMORY_WORKBENCH_VERSION,
            list_id: list_id.into(),
            scope_digest: scope_digest.into(),
            data_epoch,
            query: query.into(),
            query_digest: String::new(),
            entries,
            next_cursor,
            list_digest: String::new(),
        };
        list.query_digest = json_digest(&json!({"query": list.query}));
        list.list_digest = list.digest();
        list.validate()?;
        Ok(list)
    }

    pub fn from_records(
        list_id: impl Into<String>,
        request: &MemoryAclRequest,
        query: impl Into<String>,
        records: &[MemoryRecord],
        relations: &BTreeMap<String, MemoryWorkbenchRelations>,
        next_cursor: Option<String>,
    ) -> Result<Self, String> {
        request.validate()?;
        if request.path != MemoryAccessPath::ReviewList {
            return Err("memory_workbench_review_path_required".to_owned());
        }
        let mut entries = Vec::new();
        for record in records {
            let decision = MemoryAclDecision::evaluate(request, record)?;
            if !decision.allowed {
                continue;
            }
            let relation = relations.get(&record.id).cloned().unwrap_or_default();
            entries.push(MemoryWorkbenchListItem::from_record(
                record, &decision, relation,
            )?);
        }
        let visible_ids = entries
            .iter()
            .map(|entry| entry.record_id.clone())
            .collect::<BTreeSet<_>>();
        for entry in &mut entries {
            entry
                .similar_record_ids
                .retain(|record_id| visible_ids.contains(record_id));
        }
        let mut conflict_sizes = BTreeMap::<String, usize>::new();
        for conflict_set_id in entries
            .iter()
            .filter_map(|entry| entry.conflict_set_id.as_ref())
        {
            *conflict_sizes.entry(conflict_set_id.clone()).or_default() += 1;
        }
        for entry in &mut entries {
            if entry
                .conflict_set_id
                .as_ref()
                .is_some_and(|id| conflict_sizes.get(id).copied().unwrap_or_default() < 2)
            {
                entry.conflict_set_id = None;
            }
        }
        for entry in &mut entries {
            entry.item_digest = entry.digest();
        }
        Self::new(
            list_id,
            request.scope.scope_digest.clone(),
            request.data_epoch,
            query,
            entries,
            next_cursor,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_WORKBENCH_LIST_SCHEMA
            || self.version != MEMORY_WORKBENCH_VERSION
            || self.data_epoch == 0
            || self.entries.len() > MAX_WORKBENCH_ENTRIES
        {
            return Err("memory_workbench_list_header_invalid".to_owned());
        }
        required(&self.list_id, "memory_workbench_list_id", 256)?;
        digest(&self.scope_digest, "memory_workbench_scope_digest")?;
        if self.query.len() > 32 * 1024 || self.query.as_bytes().contains(&0) {
            return Err("memory_workbench_query_invalid".to_owned());
        }
        digest(&self.query_digest, "memory_workbench_query_digest")?;
        if self.query_digest != json_digest(&json!({"query": self.query})) {
            return Err("memory_workbench_query_digest_mismatch".to_owned());
        }
        if self
            .next_cursor
            .as_deref()
            .is_some_and(|cursor| cursor.trim().is_empty() || cursor.len() > 512)
        {
            return Err("memory_workbench_cursor_invalid".to_owned());
        }
        let mut records = BTreeSet::new();
        let visible_ids = self
            .entries
            .iter()
            .map(|entry| entry.record_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut conflict_sizes = BTreeMap::<&str, usize>::new();
        for entry in &self.entries {
            entry.validate()?;
            if !records.insert(entry.record_id.clone()) {
                return Err("memory_workbench_record_duplicate".to_owned());
            }
            if entry
                .similar_record_ids
                .iter()
                .any(|record_id| !visible_ids.contains(record_id.as_str()))
            {
                return Err("memory_workbench_relation_outside_list".to_owned());
            }
            if let Some(conflict_set_id) = entry.conflict_set_id.as_deref() {
                *conflict_sizes.entry(conflict_set_id).or_default() += 1;
            }
        }
        if conflict_sizes.values().any(|size| *size < 2) {
            return Err("memory_workbench_conflict_outside_list".to_owned());
        }
        digest(&self.list_digest, "memory_workbench_list_digest")?;
        if self.list_digest != self.digest() {
            return Err("memory_workbench_list_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "list_id": self.list_id,
            "scope_digest": self.scope_digest,
            "data_epoch": self.data_epoch,
            "query": self.query,
            "query_digest": self.query_digest,
            "entries": self.entries,
            "next_cursor": self.next_cursor,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBulkAtomicity {
    Atomic,
    ExplicitSplit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBulkMutationItem {
    pub group_id: String,
    pub mutation: MemoryMutation,
}

impl MemoryBulkMutationItem {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.group_id, "memory_bulk_group_id", 128)?;
        self.mutation.validate()?;
        if matches!(
            self.mutation.operation,
            MemoryMutationOperation::Approve
                | MemoryMutationOperation::Publish
                | MemoryMutationOperation::Expire
                | MemoryMutationOperation::Delete
                | MemoryMutationOperation::Revoke
        ) && self.mutation.authority == MemoryMutationAuthority::Agent
        {
            return Err("memory_workbench_operator_authority_required".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBulkMutationPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan_id: String,
    pub actor: String,
    pub atomicity: MemoryBulkAtomicity,
    pub expected_data_epoch: u64,
    pub items: Vec<MemoryBulkMutationItem>,
    pub plan_digest: String,
}

impl MemoryBulkMutationPlan {
    pub fn new(
        plan_id: impl Into<String>,
        actor: impl Into<String>,
        atomicity: MemoryBulkAtomicity,
        expected_data_epoch: u64,
        items: Vec<MemoryBulkMutationItem>,
    ) -> Result<Self, String> {
        let mut plan = Self {
            schema: MEMORY_BULK_MUTATION_PLAN_SCHEMA.to_owned(),
            version: MEMORY_WORKBENCH_VERSION,
            plan_id: plan_id.into(),
            actor: actor.into(),
            atomicity,
            expected_data_epoch,
            items,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_BULK_MUTATION_PLAN_SCHEMA
            || self.version != MEMORY_WORKBENCH_VERSION
            || self.expected_data_epoch == 0
            || self.items.is_empty()
            || self.items.len() > MAX_BULK_MUTATIONS
        {
            return Err("memory_bulk_plan_header_invalid".to_owned());
        }
        required(&self.plan_id, "memory_bulk_plan_id", 256)?;
        required(&self.actor, "memory_bulk_plan_actor", 256)?;
        let mut mutation_ids = BTreeSet::new();
        let mut idempotency_keys = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut groups: BTreeMap<String, (String, u64)> = BTreeMap::new();
        for item in &self.items {
            item.validate()?;
            let mutation = &item.mutation;
            if mutation.actor != self.actor || mutation.data_epoch != self.expected_data_epoch {
                return Err("memory_bulk_plan_identity_mismatch".to_owned());
            }
            if !mutation_ids.insert(mutation.mutation_id.clone()) {
                return Err("memory_bulk_plan_mutation_duplicate".to_owned());
            }
            if !idempotency_keys.insert(mutation.idempotency_key.clone()) {
                return Err("memory_bulk_plan_idempotency_duplicate".to_owned());
            }
            for target in &mutation.expected_revisions {
                if !targets.insert(target.key()) {
                    return Err("memory_bulk_plan_target_duplicate".to_owned());
                }
            }
            match groups.entry(item.group_id.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert((mutation.scope.scope_digest.clone(), mutation.policy_epoch));
                }
                std::collections::btree_map::Entry::Occupied(entry) => {
                    if entry.get() != &(mutation.scope.scope_digest.clone(), mutation.policy_epoch)
                    {
                        return Err("memory_bulk_plan_group_scope_mismatch".to_owned());
                    }
                }
            }
        }
        match self.atomicity {
            MemoryBulkAtomicity::Atomic if groups.len() != 1 => {
                return Err("memory_bulk_atomic_requires_one_scope".to_owned());
            }
            MemoryBulkAtomicity::ExplicitSplit if groups.is_empty() => {
                return Err("memory_bulk_split_group_required".to_owned());
            }
            _ => {}
        }
        digest(&self.plan_digest, "memory_bulk_plan_digest")?;
        if self.plan_digest != self.digest() {
            return Err("memory_bulk_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "plan_id": self.plan_id,
            "actor": self.actor,
            "atomicity": self.atomicity,
            "expected_data_epoch": self.expected_data_epoch,
            "items": self.items,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBulkItemStatus {
    Committed,
    Replayed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBulkFailure {
    RevisionConflict,
    ScopeDenied,
    ApprovalRequired,
    ValidationFailed,
    ProjectionPending,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBulkMutationItemResult {
    pub group_id: String,
    pub mutation_id: String,
    pub status: MemoryBulkItemStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<MemoryBulkFailure>,
}

impl MemoryBulkMutationItemResult {
    pub fn committed(
        item: &MemoryBulkMutationItem,
        receipt_digest: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            item,
            MemoryBulkItemStatus::Committed,
            Some(receipt_digest.into()),
            None,
        )
    }

    pub fn replayed(
        item: &MemoryBulkMutationItem,
        receipt_digest: impl Into<String>,
    ) -> Result<Self, String> {
        Self::new(
            item,
            MemoryBulkItemStatus::Replayed,
            Some(receipt_digest.into()),
            None,
        )
    }

    pub fn failed(
        item: &MemoryBulkMutationItem,
        failure: MemoryBulkFailure,
    ) -> Result<Self, String> {
        Self::new(item, MemoryBulkItemStatus::Failed, None, Some(failure))
    }

    pub fn unknown(item: &MemoryBulkMutationItem) -> Result<Self, String> {
        Self::new(
            item,
            MemoryBulkItemStatus::Unknown,
            None,
            Some(MemoryBulkFailure::ResultUnknown),
        )
    }

    fn new(
        item: &MemoryBulkMutationItem,
        status: MemoryBulkItemStatus,
        receipt_digest: Option<String>,
        failure: Option<MemoryBulkFailure>,
    ) -> Result<Self, String> {
        let result = Self {
            group_id: item.group_id.clone(),
            mutation_id: item.mutation.mutation_id.clone(),
            status,
            receipt_digest,
            failure,
        };
        result.validate()?;
        Ok(result)
    }

    fn validate(&self) -> Result<(), String> {
        required(&self.group_id, "memory_bulk_result_group_id", 128)?;
        required(&self.mutation_id, "memory_bulk_result_mutation_id", 256)?;
        match self.status {
            MemoryBulkItemStatus::Committed | MemoryBulkItemStatus::Replayed => {
                let receipt = self
                    .receipt_digest
                    .as_deref()
                    .ok_or_else(|| "memory_bulk_result_receipt_required".to_owned())?;
                digest(receipt, "memory_bulk_result_receipt_digest")?;
                if self.failure.is_some() {
                    return Err("memory_bulk_result_success_failure_conflict".to_owned());
                }
            }
            MemoryBulkItemStatus::Failed => {
                if self.receipt_digest.is_some() || self.failure.is_none() {
                    return Err("memory_bulk_result_failure_shape_invalid".to_owned());
                }
                if self.failure == Some(MemoryBulkFailure::ResultUnknown) {
                    return Err("memory_bulk_result_unknown_status_required".to_owned());
                }
            }
            MemoryBulkItemStatus::Unknown => {
                if self.receipt_digest.is_some()
                    || self.failure != Some(MemoryBulkFailure::ResultUnknown)
                {
                    return Err("memory_bulk_result_unknown_shape_invalid".to_owned());
                }
            }
        }
        Ok(())
    }
}

fn validate_explicit_split_groups(items: &[MemoryBulkMutationItemResult]) -> Result<(), String> {
    let mut groups = BTreeMap::<&str, (bool, bool)>::new();
    for item in items {
        let state = groups.entry(item.group_id.as_str()).or_default();
        if matches!(
            item.status,
            MemoryBulkItemStatus::Committed | MemoryBulkItemStatus::Replayed
        ) {
            state.0 = true;
        } else {
            state.1 = true;
        }
    }
    if groups
        .values()
        .any(|(success, non_success)| *success && *non_success)
    {
        return Err("memory_bulk_split_group_partial_result".to_owned());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBulkResultState {
    Committed,
    Replayed,
    PartiallyFailed,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBulkMutationResult {
    pub schema: String,
    pub version: SchemaVersion,
    pub plan_digest: String,
    pub atomicity: MemoryBulkAtomicity,
    pub state: MemoryBulkResultState,
    pub items: Vec<MemoryBulkMutationItemResult>,
    pub result_digest: String,
}

impl MemoryBulkMutationResult {
    pub fn from_items(
        plan: &MemoryBulkMutationPlan,
        mut items: Vec<MemoryBulkMutationItemResult>,
    ) -> Result<Self, String> {
        plan.validate()?;
        items.sort_by(|left, right| {
            left.group_id
                .cmp(&right.group_id)
                .then_with(|| left.mutation_id.cmp(&right.mutation_id))
        });
        let expected = plan
            .items
            .iter()
            .map(|item| (item.group_id.clone(), item.mutation.mutation_id.clone()))
            .collect::<BTreeSet<_>>();
        let actual = items
            .iter()
            .map(|item| (item.group_id.clone(), item.mutation_id.clone()))
            .collect::<BTreeSet<_>>();
        if expected != actual
            || actual.len() != items.len()
            || items.iter().any(|item| item.validate().is_err())
        {
            return Err("memory_bulk_result_items_mismatch".to_owned());
        }
        if plan.atomicity == MemoryBulkAtomicity::ExplicitSplit {
            validate_explicit_split_groups(&items)?;
        }
        let successes = items
            .iter()
            .filter(|item| {
                matches!(
                    item.status,
                    MemoryBulkItemStatus::Committed | MemoryBulkItemStatus::Replayed
                )
            })
            .count();
        let failures = items
            .iter()
            .filter(|item| item.status == MemoryBulkItemStatus::Failed)
            .count();
        let unknown = items
            .iter()
            .any(|item| item.status == MemoryBulkItemStatus::Unknown);
        if plan.atomicity == MemoryBulkAtomicity::Atomic
            && successes > 0
            && (failures > 0 || unknown)
        {
            return Err("memory_bulk_atomic_partial_result".to_owned());
        }
        let state = if unknown {
            MemoryBulkResultState::Unknown
        } else if successes == items.len()
            && items
                .iter()
                .all(|item| item.status == MemoryBulkItemStatus::Replayed)
        {
            MemoryBulkResultState::Replayed
        } else if successes == items.len() {
            MemoryBulkResultState::Committed
        } else if successes > 0 {
            MemoryBulkResultState::PartiallyFailed
        } else {
            MemoryBulkResultState::Rejected
        };
        let mut result = Self {
            schema: MEMORY_BULK_MUTATION_RESULT_SCHEMA.to_owned(),
            version: MEMORY_WORKBENCH_VERSION,
            plan_digest: plan.plan_digest.clone(),
            atomicity: plan.atomicity,
            state,
            items,
            result_digest: String::new(),
        };
        result.result_digest = result.digest();
        result.validate_against(plan)?;
        Ok(result)
    }

    pub fn validate_against(&self, plan: &MemoryBulkMutationPlan) -> Result<(), String> {
        plan.validate()?;
        self.validate()?;
        if self.plan_digest != plan.plan_digest || self.atomicity != plan.atomicity {
            return Err("memory_bulk_result_plan_mismatch".to_owned());
        }
        let expected = plan
            .items
            .iter()
            .map(|item| (item.group_id.clone(), item.mutation.mutation_id.clone()))
            .collect::<BTreeSet<_>>();
        let actual = self
            .items
            .iter()
            .map(|item| (item.group_id.clone(), item.mutation_id.clone()))
            .collect::<BTreeSet<_>>();
        if expected != actual {
            return Err("memory_bulk_result_item_identity_mismatch".to_owned());
        }
        if actual.len() != self.items.len() {
            return Err("memory_bulk_result_item_identity_mismatch".to_owned());
        }
        if self.atomicity == MemoryBulkAtomicity::Atomic {
            let successes = self
                .items
                .iter()
                .filter(|item| {
                    matches!(
                        item.status,
                        MemoryBulkItemStatus::Committed | MemoryBulkItemStatus::Replayed
                    )
                })
                .count();
            let non_successes = self.items.len().saturating_sub(successes);
            if successes > 0 && non_successes > 0 {
                return Err("memory_bulk_atomic_partial_result".to_owned());
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_BULK_MUTATION_RESULT_SCHEMA
            || self.version != MEMORY_WORKBENCH_VERSION
            || self.items.is_empty()
            || self.items.len() > MAX_BULK_MUTATIONS
        {
            return Err("memory_bulk_result_header_invalid".to_owned());
        }
        digest(&self.plan_digest, "memory_bulk_result_plan_digest")?;
        for item in &self.items {
            item.validate()?;
        }
        if self.atomicity == MemoryBulkAtomicity::ExplicitSplit {
            validate_explicit_split_groups(&self.items)?;
        }
        let successes = self
            .items
            .iter()
            .filter(|item| {
                matches!(
                    item.status,
                    MemoryBulkItemStatus::Committed | MemoryBulkItemStatus::Replayed
                )
            })
            .count();
        let failures = self
            .items
            .iter()
            .filter(|item| item.status == MemoryBulkItemStatus::Failed)
            .count();
        let unknown = self
            .items
            .iter()
            .any(|item| item.status == MemoryBulkItemStatus::Unknown);
        let expected_state = if unknown {
            MemoryBulkResultState::Unknown
        } else if successes == self.items.len()
            && self
                .items
                .iter()
                .all(|item| item.status == MemoryBulkItemStatus::Replayed)
        {
            MemoryBulkResultState::Replayed
        } else if successes == self.items.len() {
            MemoryBulkResultState::Committed
        } else if successes > 0 {
            MemoryBulkResultState::PartiallyFailed
        } else {
            MemoryBulkResultState::Rejected
        };
        if self.state != expected_state {
            return Err("memory_bulk_result_state_mismatch".to_owned());
        }
        if self.atomicity == MemoryBulkAtomicity::Atomic
            && successes > 0
            && (failures > 0 || unknown)
        {
            return Err("memory_bulk_atomic_partial_result".to_owned());
        }
        digest(&self.result_digest, "memory_bulk_result_digest")?;
        if self.result_digest != self.digest() {
            return Err("memory_bulk_result_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "plan_digest": self.plan_digest,
            "atomicity": self.atomicity,
            "state": self.state,
            "items": self.items,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportRow {
    pub record_id: String,
    pub collection: String,
    pub classification: MemoryClassification,
    pub revision: u64,
    pub content_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub private_redacted: bool,
    pub row_digest: String,
}

impl MemoryExportRow {
    pub fn from_list_item(item: &MemoryWorkbenchListItem) -> Result<Self, String> {
        item.validate()?;
        let private = item.classification == MemoryClassification::UserPrivate
            || crate::MemoryCollection::parse(&item.collection)
                .is_some_and(|collection| collection.layer == MEMORY_LAYER_USER);
        let row = Self {
            record_id: item.record_id.clone(),
            collection: item.collection.clone(),
            classification: item.classification,
            revision: item.revision,
            content_digest: item.content_digest.clone(),
            preview: if private { None } else { item.preview.clone() },
            private_redacted: private,
            row_digest: String::new(),
        };
        let mut row = row;
        row.row_digest = row.digest();
        row.validate()?;
        Ok(row)
    }

    pub fn validate(&self) -> Result<(), String> {
        required(&self.record_id, "memory_export_record_id", 256)?;
        required(&self.collection, "memory_export_collection", 256)?;
        let collection = crate::MemoryCollection::parse(&self.collection)
            .ok_or_else(|| "memory_export_collection_invalid".to_owned())?;
        if collection.collection != self.collection.trim() {
            return Err("memory_export_collection_noncanonical".to_owned());
        }
        if self.revision == 0 {
            return Err("memory_export_revision_invalid".to_owned());
        }
        digest(&self.content_digest, "memory_export_content_digest")?;
        if self.preview.as_deref().is_some_and(|preview| {
            preview.chars().count() > 512
                || preview.as_bytes().contains(&0)
                || redact_text(preview) != preview
        }) {
            return Err("memory_export_preview_not_redacted".to_owned());
        }
        let private = self.classification == MemoryClassification::UserPrivate
            || crate::MemoryCollection::parse(&self.collection)
                .is_some_and(|collection| collection.layer == MEMORY_LAYER_USER);
        if (private && !self.private_redacted) || (self.private_redacted && self.preview.is_some())
        {
            return Err("memory_export_private_preview_leak".to_owned());
        }
        digest(&self.row_digest, "memory_export_row_digest")?;
        if self.row_digest != self.digest() {
            return Err("memory_export_row_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "record_id": self.record_id,
            "collection": self.collection,
            "classification": self.classification,
            "revision": self.revision,
            "content_digest": self.content_digest,
            "preview": self.preview,
            "private_redacted": self.private_redacted,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryExportManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub export_id: String,
    pub scope_digest: String,
    pub purpose_digest: String,
    pub recipient_digest: String,
    pub redaction_profile_digest: String,
    pub data_epoch: u64,
    pub rows: Vec<MemoryExportRow>,
    pub export_digest: String,
}

impl MemoryExportManifest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        export_id: impl Into<String>,
        scope_digest: impl Into<String>,
        purpose_digest: impl Into<String>,
        recipient_digest: impl Into<String>,
        redaction_profile_digest: impl Into<String>,
        data_epoch: u64,
        rows: Vec<MemoryExportRow>,
    ) -> Result<Self, String> {
        let mut manifest = Self {
            schema: MEMORY_EXPORT_SCHEMA.to_owned(),
            version: MEMORY_WORKBENCH_VERSION,
            export_id: export_id.into(),
            scope_digest: scope_digest.into(),
            purpose_digest: purpose_digest.into(),
            recipient_digest: recipient_digest.into(),
            redaction_profile_digest: redaction_profile_digest.into(),
            data_epoch,
            rows,
            export_digest: String::new(),
        };
        manifest.export_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn from_list(
        export_id: impl Into<String>,
        list: &MemoryWorkbenchList,
        purpose_digest: impl Into<String>,
        recipient_digest: impl Into<String>,
        redaction_profile_digest: impl Into<String>,
    ) -> Result<Self, String> {
        list.validate()?;
        let rows = list
            .entries
            .iter()
            .map(MemoryExportRow::from_list_item)
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(
            export_id,
            list.scope_digest.clone(),
            purpose_digest,
            recipient_digest,
            redaction_profile_digest,
            list.data_epoch,
            rows,
        )
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_EXPORT_SCHEMA
            || self.version != MEMORY_WORKBENCH_VERSION
            || self.data_epoch == 0
            || self.rows.len() > MAX_EXPORT_ROWS
        {
            return Err("memory_export_header_invalid".to_owned());
        }
        required(&self.export_id, "memory_export_id", 256)?;
        digest(&self.scope_digest, "memory_export_scope_digest")?;
        digest(&self.purpose_digest, "memory_export_purpose_digest")?;
        digest(&self.recipient_digest, "memory_export_recipient_digest")?;
        digest(
            &self.redaction_profile_digest,
            "memory_export_redaction_profile_digest",
        )?;
        let mut records = BTreeSet::new();
        for row in &self.rows {
            row.validate()?;
            if !records.insert((row.collection.clone(), row.record_id.clone())) {
                return Err("memory_export_record_duplicate".to_owned());
            }
        }
        digest(&self.export_digest, "memory_export_digest")?;
        if self.export_digest != self.digest() {
            return Err("memory_export_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "export_id": self.export_id,
            "scope_digest": self.scope_digest,
            "purpose_digest": self.purpose_digest,
            "recipient_digest": self.recipient_digest,
            "redaction_profile_digest": self.redaction_profile_digest,
            "data_epoch": self.data_epoch,
            "rows": self.rows,
        }))
    }
}
