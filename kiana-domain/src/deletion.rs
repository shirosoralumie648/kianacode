//! Delete-request, tombstone and propagation-manifest contracts.
//!
//! A delete request is evaluated against a retention scan before any adapter is called. The
//! resulting tombstones contain only object references and digests, never payload bytes. The
//! propagation manifest keeps every unconfirmed target as `Unknown`; it is not a success receipt.

use crate::{json_digest, EventCursor, RequestId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DELETE_REQUEST_SCHEMA: &str = "kiana.delete-request.v1";
pub const DELETION_TOMBSTONE_SCHEMA: &str = "kiana.deletion-tombstone.v1";
pub const DELETION_MANIFEST_SCHEMA: &str = "kiana.deletion-manifest.v1";
pub const DELETION_CONTRACT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_DELETION_OBJECTS: usize = 512;
pub const MAX_DELETION_TARGETS: usize = 32;

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub project_ref: String,
    pub object_refs: BTreeSet<String>,
    pub purpose_id: String,
    pub legal_basis: String,
    pub requested_by: String,
    pub policy_revision: u64,
    /// Epoch observed before deletion. A plan advances it exactly once.
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub request_digest: String,
}

impl DeleteRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: RequestId,
        project_ref: impl Into<String>,
        object_refs: BTreeSet<String>,
        purpose_id: impl Into<String>,
        legal_basis: impl Into<String>,
        requested_by: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        source_cursor: EventCursor,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: DELETE_REQUEST_SCHEMA.to_owned(),
            version: DELETION_CONTRACT_VERSION,
            request_id,
            project_ref: project_ref.into(),
            object_refs,
            purpose_id: purpose_id.into(),
            legal_basis: legal_basis.into(),
            requested_by: requested_by.into(),
            policy_revision,
            data_epoch,
            source_cursor,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DELETE_REQUEST_SCHEMA
            || !self.version.is_compatible_with(&DELETION_CONTRACT_VERSION)
        {
            return Err("delete_request_schema_invalid".to_owned());
        }
        if self.request_id.as_uuid().is_nil() {
            return Err("delete_request_id_invalid".to_owned());
        }
        nonempty(&self.project_ref, "delete_request_project", 512)?;
        if self.object_refs.is_empty() || self.object_refs.len() > MAX_DELETION_OBJECTS {
            return Err("delete_request_object_limit".to_owned());
        }
        for object_ref in &self.object_refs {
            nonempty(object_ref, "delete_request_object_ref", 512)?;
        }
        nonempty(&self.purpose_id, "delete_request_purpose", 128)?;
        nonempty(&self.legal_basis, "delete_request_legal_basis", 512)?;
        nonempty(&self.requested_by, "delete_request_actor", 256)?;
        if self.policy_revision == 0 || self.data_epoch == 0 || self.source_cursor == 0 {
            return Err("delete_request_epoch_cursor_invalid".to_owned());
        }
        if !valid_digest(&self.request_digest) {
            return Err("delete_request_digest_invalid".to_owned());
        }
        if self.request_digest != self.digest() {
            return Err("delete_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "request_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionEffectState {
    Pending,
    Erased,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionTombstone {
    pub schema: String,
    pub version: SchemaVersion,
    pub tombstone_id: String,
    pub request_id: RequestId,
    pub project_ref: String,
    pub object_ref: String,
    pub source_digest: String,
    pub policy_revision: u64,
    /// The new epoch that fences old projections and permissions.
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub effect: DeletionEffectState,
    pub tombstone_digest: String,
}

impl DeletionTombstone {
    pub fn new(
        request: &DeleteRequest,
        object_ref: impl Into<String>,
        source_digest: impl Into<String>,
        next_data_epoch: u64,
    ) -> Result<Self, String> {
        request.validate()?;
        let object_ref = object_ref.into();
        let mut tombstone = Self {
            schema: DELETION_TOMBSTONE_SCHEMA.to_owned(),
            version: DELETION_CONTRACT_VERSION,
            tombstone_id: format!("{}:{object_ref}", request.request_id),
            request_id: request.request_id,
            project_ref: request.project_ref.clone(),
            object_ref,
            source_digest: source_digest.into(),
            policy_revision: request.policy_revision,
            data_epoch: next_data_epoch,
            source_cursor: request.source_cursor,
            effect: DeletionEffectState::Unknown,
            tombstone_digest: String::new(),
        };
        tombstone.tombstone_digest = tombstone.digest();
        tombstone.validate()?;
        Ok(tombstone)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DELETION_TOMBSTONE_SCHEMA
            || !self.version.is_compatible_with(&DELETION_CONTRACT_VERSION)
        {
            return Err("deletion_tombstone_schema_invalid".to_owned());
        }
        nonempty(&self.tombstone_id, "deletion_tombstone_id", 1024)?;
        if self.request_id.as_uuid().is_nil() {
            return Err("deletion_tombstone_request_invalid".to_owned());
        }
        nonempty(&self.project_ref, "deletion_tombstone_project", 512)?;
        nonempty(&self.object_ref, "deletion_tombstone_object_ref", 512)?;
        if !valid_digest(&self.source_digest) {
            return Err("deletion_tombstone_source_digest_invalid".to_owned());
        }
        if self.policy_revision == 0 || self.data_epoch == 0 || self.source_cursor == 0 {
            return Err("deletion_tombstone_boundary_invalid".to_owned());
        }
        if !valid_digest(&self.tombstone_digest) {
            return Err("deletion_tombstone_digest_invalid".to_owned());
        }
        if self.tombstone_digest != self.digest() {
            return Err("deletion_tombstone_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "tombstone_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletionPropagationState {
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionPropagation {
    pub target: String,
    pub state: DeletionPropagationState,
    pub receipt_digest: Option<String>,
    pub reason: Option<String>,
}

impl DeletionPropagation {
    pub fn unknown(target: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            state: DeletionPropagationState::Unknown,
            receipt_digest: None,
            reason: Some(reason.into()),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.target, "deletion_target", 128)?;
        match self.state {
            DeletionPropagationState::Completed => {
                let Some(receipt) = &self.receipt_digest else {
                    return Err("deletion_completed_receipt_missing".to_owned());
                };
                if !valid_digest(receipt) || self.reason.is_some() {
                    return Err("deletion_completed_receipt_invalid".to_owned());
                }
            }
            DeletionPropagationState::Unknown => {
                if self.receipt_digest.is_some() {
                    return Err("deletion_unknown_receipt_forbidden".to_owned());
                }
                let Some(reason) = &self.reason else {
                    return Err("deletion_unknown_reason_missing".to_owned());
                };
                nonempty(reason, "deletion_unknown_reason", 512)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionManifest {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_id: RequestId,
    pub project_ref: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub tombstone_ids: Vec<String>,
    pub propagation: Vec<DeletionPropagation>,
    pub manifest_digest: String,
}

impl DeletionManifest {
    pub fn new(
        request: &DeleteRequest,
        next_data_epoch: u64,
        tombstones: &[DeletionTombstone],
        propagation: Vec<DeletionPropagation>,
    ) -> Result<Self, String> {
        request.validate()?;
        let mut manifest = Self {
            schema: DELETION_MANIFEST_SCHEMA.to_owned(),
            version: DELETION_CONTRACT_VERSION,
            request_id: request.request_id,
            project_ref: request.project_ref.clone(),
            policy_revision: request.policy_revision,
            data_epoch: next_data_epoch,
            source_cursor: request.source_cursor,
            tombstone_ids: tombstones
                .iter()
                .map(|tombstone| tombstone.tombstone_id.clone())
                .collect(),
            propagation,
            manifest_digest: String::new(),
        };
        manifest.manifest_digest = manifest.digest();
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DELETION_MANIFEST_SCHEMA
            || !self.version.is_compatible_with(&DELETION_CONTRACT_VERSION)
        {
            return Err("deletion_manifest_schema_invalid".to_owned());
        }
        if self.request_id.as_uuid().is_nil() {
            return Err("deletion_manifest_request_invalid".to_owned());
        }
        nonempty(&self.project_ref, "deletion_manifest_project", 512)?;
        if self.policy_revision == 0 || self.data_epoch == 0 || self.source_cursor == 0 {
            return Err("deletion_manifest_boundary_invalid".to_owned());
        }
        if self.tombstone_ids.is_empty() || self.tombstone_ids.len() > MAX_DELETION_OBJECTS {
            return Err("deletion_manifest_tombstone_limit".to_owned());
        }
        let mut ids = BTreeSet::new();
        for tombstone_id in &self.tombstone_ids {
            nonempty(tombstone_id, "deletion_manifest_tombstone", 1024)?;
            if !ids.insert(tombstone_id) {
                return Err("deletion_manifest_tombstone_duplicate".to_owned());
            }
        }
        if self.propagation.is_empty() || self.propagation.len() > MAX_DELETION_TARGETS {
            return Err("deletion_manifest_target_limit".to_owned());
        }
        let mut targets = BTreeSet::new();
        for target in &self.propagation {
            target.validate()?;
            if !targets.insert(target.target.clone()) {
                return Err("deletion_manifest_target_duplicate".to_owned());
            }
        }
        if !valid_digest(&self.manifest_digest) {
            return Err("deletion_manifest_digest_invalid".to_owned());
        }
        if self.manifest_digest != self.digest() {
            return Err("deletion_manifest_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn has_unknown_propagation(&self) -> bool {
        self.propagation
            .iter()
            .any(|target| target.state == DeletionPropagationState::Unknown)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "manifest_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeletionPlan {
    pub request: DeleteRequest,
    pub next_data_epoch: u64,
    pub tombstones: Vec<DeletionTombstone>,
    pub manifest: DeletionManifest,
}

impl DeletionPlan {
    pub fn new(
        request: DeleteRequest,
        next_data_epoch: u64,
        tombstones: Vec<DeletionTombstone>,
        manifest: DeletionManifest,
    ) -> Result<Self, String> {
        let plan = Self {
            request,
            next_data_epoch,
            tombstones,
            manifest,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.request.validate()?;
        if self.next_data_epoch <= self.request.data_epoch {
            return Err("deletion_plan_epoch_not_advanced".to_owned());
        }
        if self.tombstones.len() != self.request.object_refs.len()
            || self.tombstones.len() > MAX_DELETION_OBJECTS
        {
            return Err("deletion_plan_tombstone_count_invalid".to_owned());
        }
        let mut objects = BTreeSet::new();
        let mut tombstone_ids = BTreeSet::new();
        for tombstone in &self.tombstones {
            tombstone.validate()?;
            if tombstone.request_id != self.request.request_id
                || tombstone.project_ref != self.request.project_ref
                || tombstone.policy_revision != self.request.policy_revision
                || tombstone.data_epoch != self.next_data_epoch
                || tombstone.source_cursor != self.request.source_cursor
                || !self.request.object_refs.contains(&tombstone.object_ref)
                || !objects.insert(tombstone.object_ref.clone())
                || !tombstone_ids.insert(tombstone.tombstone_id.clone())
            {
                return Err("deletion_plan_tombstone_boundary_invalid".to_owned());
            }
        }
        self.manifest.validate()?;
        if self.manifest.request_id != self.request.request_id
            || self.manifest.project_ref != self.request.project_ref
            || self.manifest.policy_revision != self.request.policy_revision
            || self.manifest.data_epoch != self.next_data_epoch
            || self.manifest.source_cursor != self.request.source_cursor
            || self
                .manifest
                .tombstone_ids
                .iter()
                .any(|id| !tombstone_ids.contains(id))
            || self.manifest.tombstone_ids.len() != tombstone_ids.len()
        {
            return Err("deletion_plan_manifest_boundary_invalid".to_owned());
        }
        Ok(())
    }
}
