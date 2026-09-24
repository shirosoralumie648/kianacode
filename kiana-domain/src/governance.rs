use crate::{json_digest, EventCursor, EventId, SchemaVersion, MAX_SOURCE_EVENT_IDS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const DATA_POLICY_SCHEMA: &str = "kiana.data-policy.v1";
pub const DATA_GOVERNANCE_SNAPSHOT_SCHEMA: &str = "kiana.data-governance-snapshot.v1";
pub const DATA_GOVERNANCE_SNAPSHOT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_DATA_OBSERVATIONS: usize = 256;
pub const MAX_DERIVED_DATA_STORES: usize = 32;

fn nonempty(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn sha256_digest(value: &str, field: &str) -> Result<(), String> {
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
pub enum DataClass {
    Public,
    Internal,
    Confidential,
    Restricted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Purpose {
    pub id: String,
    pub description: String,
}

impl Purpose {
    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.id, "purpose_id", 128)?;
        nonempty(&self.description, "purpose_description", 256)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Retention {
    pub expires_at_ms: Option<u64>,
    pub retain_audit_metadata: bool,
}

impl Retention {
    pub fn validate(&self) -> Result<(), String> {
        if self.expires_at_ms == Some(0) {
            return Err("retention_expiry_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessingGrant {
    pub id: String,
    pub source_path: String,
    pub content_hash: String,
    pub class: DataClass,
    pub purpose: Purpose,
    pub retention: Retention,
    pub parent_ids: Vec<String>,
    pub created_by: String,
    pub revoked: bool,
}

impl ProcessingGrant {
    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.id, "processing_grant_id", 128)?;
        if crate::normalize_role_path(&self.source_path).is_none() {
            return Err("processing_grant_source_invalid".to_owned());
        }
        sha256_digest(&self.content_hash, "processing_grant_content_hash")?;
        self.purpose.validate()?;
        self.retention.validate()?;
        nonempty(&self.created_by, "processing_grant_creator", 256)?;
        if self.parent_ids.len() > MAX_DATA_OBSERVATIONS {
            return Err("processing_grant_parent_limit".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataPolicy {
    #[serde(default = "default_policy_schema")]
    pub schema: String,
    #[serde(default = "default_policy_version")]
    pub version: SchemaVersion,
    pub revision: u64,
    #[serde(default = "default_data_epoch")]
    pub data_epoch: u64,
    pub grants: BTreeMap<String, ProcessingGrant>,
    pub revoked_sources: BTreeSet<String>,
    #[serde(default)]
    pub policy_digest: String,
}

fn default_policy_schema() -> String {
    DATA_POLICY_SCHEMA.to_owned()
}

fn default_policy_version() -> SchemaVersion {
    SchemaVersion::new(1, 0)
}

fn default_data_epoch() -> u64 {
    1
}

impl Default for DataPolicy {
    fn default() -> Self {
        let mut policy = Self {
            schema: default_policy_schema(),
            version: default_policy_version(),
            revision: 0,
            data_epoch: default_data_epoch(),
            grants: BTreeMap::new(),
            revoked_sources: BTreeSet::new(),
            policy_digest: String::new(),
        };
        policy.refresh_digest();
        policy
    }
}

impl DataPolicy {
    pub fn register(&mut self, grant: ProcessingGrant) -> Result<(), String> {
        grant.validate()?;
        if grant.revoked
            || self.grants.contains_key(&grant.id)
            || self.revoked_sources.contains(&grant.source_path)
            || grant
                .parent_ids
                .iter()
                .any(|id| self.grants.get(id).is_none_or(|parent| parent.revoked))
        {
            return Err("processing_grant_invalid".to_owned());
        }
        self.grants.insert(grant.id.clone(), grant);
        self.revision = self.revision.saturating_add(1);
        self.refresh_digest();
        Ok(())
    }

    pub fn revoke(&mut self, id: &str) -> Result<Vec<String>, String> {
        if !self.grants.contains_key(id) {
            return Err("processing_grant_not_found".to_owned());
        }
        let mut affected = BTreeSet::from([id.to_owned()]);
        loop {
            let before = affected.len();
            for grant in self.grants.values() {
                if grant
                    .parent_ids
                    .iter()
                    .any(|parent| affected.contains(parent))
                {
                    affected.insert(grant.id.clone());
                }
            }
            if before == affected.len() {
                break;
            }
        }
        for grant_id in &affected {
            let grant = self
                .grants
                .get_mut(grant_id)
                .expect("derived existing grant");
            grant.revoked = true;
            self.revoked_sources.insert(grant.source_path.clone());
        }
        self.revision = self.revision.saturating_add(1);
        self.data_epoch = self.data_epoch.saturating_add(1).max(1);
        self.refresh_digest();
        Ok(affected.into_iter().collect())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DATA_POLICY_SCHEMA
            || !self.version.is_compatible_with(&SchemaVersion::new(1, 0))
            || self.data_epoch == 0
        {
            return Err("data_policy_schema_invalid".to_owned());
        }
        for grant in self.grants.values() {
            grant.validate()?;
            if grant.revoked && !self.revoked_sources.contains(&grant.source_path) {
                return Err("data_policy_revocation_binding_invalid".to_owned());
            }
        }
        for source in &self.revoked_sources {
            nonempty(source, "data_policy_revoked_source", 512)?;
        }
        sha256_digest(&self.policy_digest, "data_policy_digest")?;
        if self.policy_digest != self.digest() {
            return Err("data_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn refresh_digest(&mut self) {
        self.policy_digest = self.digest();
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "policy_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataPayloadState {
    Available,
    Expired,
    Revoked,
    Unknown,
}

/// A receipt keeps audit metadata and payload references in separate fields.  The reference is
/// an opaque identity/digest only; it never contains the bytes that the receipt describes.
pub const RECEIPT_DATA_BINDING_SCHEMA: &str = "kiana.receipt-data-binding.v1";
pub const DATA_PROPAGATION_PLAN_SCHEMA: &str = "kiana.data-propagation-plan.v1";
pub const DATA_PROPAGATION_RECEIPT_SCHEMA: &str = "kiana.data-propagation-receipt.v1";
pub const IMMUTABLE_EVENT_SEAL_SCHEMA: &str = "kiana.immutable-event-seal.v1";
pub const DATA_PROPAGATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_RECEIPT_PAYLOAD_REFS: usize = 256;
pub const MAX_PROPAGATION_TARGETS: usize = 16;

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Metadata retained for audit/replay.  No payload body or authorization decision is stored here.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptAuditMetadata {
    pub project_ref: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub source_event_ids: Vec<EventId>,
    pub redaction_profile: String,
    pub retained: bool,
}

impl ReceiptAuditMetadata {
    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.project_ref, "receipt_metadata_project", 512)?;
        if self.policy_revision == 0
            || self.data_epoch == 0
            || self.source_cursor == 0
            || self.source_event_ids.is_empty()
            || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS
            || self.redaction_profile.trim().is_empty()
        {
            return Err("receipt_metadata_boundary_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        if self
            .source_event_ids
            .iter()
            .any(|event_id| !ids.insert(event_id.to_string()))
        {
            return Err("receipt_metadata_source_duplicate".to_owned());
        }
        Ok(())
    }
}

/// Digest-only payload reference kept beside, rather than inside, audit metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptPayloadRef {
    pub object_ref: String,
    pub source_digest: String,
    pub data_epoch: u64,
    pub payload_digest: String,
}

impl ReceiptPayloadRef {
    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.object_ref, "receipt_payload_object", 512)?;
        if self.data_epoch == 0
            || !valid_digest(&self.source_digest)
            || !valid_digest(&self.payload_digest)
        {
            return Err("receipt_payload_ref_invalid".to_owned());
        }
        Ok(())
    }
}

/// A server-derived receipt data fence. Redaction can remove refs from a view, but only the
/// current policy/data epoch may authorize returning payload bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptDataBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub receipt_digest: String,
    pub metadata: ReceiptAuditMetadata,
    pub payload_refs: Vec<ReceiptPayloadRef>,
    pub payload_state: DataPayloadState,
    pub binding_digest: String,
}

impl ReceiptDataBinding {
    pub fn new(
        receipt_digest: impl Into<String>,
        metadata: ReceiptAuditMetadata,
        payload_refs: Vec<ReceiptPayloadRef>,
        payload_state: DataPayloadState,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: RECEIPT_DATA_BINDING_SCHEMA.to_owned(),
            version: DATA_PROPAGATION_VERSION,
            receipt_digest: receipt_digest.into(),
            metadata,
            payload_refs,
            payload_state,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RECEIPT_DATA_BINDING_SCHEMA
            || !self.version.is_compatible_with(&DATA_PROPAGATION_VERSION)
            || !valid_digest(&self.receipt_digest)
            || !valid_digest(&self.binding_digest)
            || self.payload_refs.len() > MAX_RECEIPT_PAYLOAD_REFS
        {
            return Err("receipt_data_binding_header_invalid".to_owned());
        }
        self.metadata.validate()?;
        let mut objects = BTreeSet::new();
        for payload_ref in &self.payload_refs {
            payload_ref.validate()?;
            if !objects.insert(payload_ref.object_ref.clone())
                || payload_ref.data_epoch != self.metadata.data_epoch
            {
                return Err("receipt_payload_ref_boundary_invalid".to_owned());
            }
        }
        if self.binding_digest != self.digest() {
            return Err("receipt_data_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Produces a redacted view without changing the governing state or policy metadata.
    pub fn redact_payload_refs(&self) -> Result<Self, String> {
        self.validate()?;
        let mut redacted = self.clone();
        redacted.payload_refs.clear();
        redacted.binding_digest = redacted.digest();
        redacted.validate()?;
        Ok(redacted)
    }

    /// Redaction is not authorization: a payload is readable only while state and epoch match.
    pub fn authorize_payload(&self, current_data_epoch: u64) -> Result<(), String> {
        self.validate()?;
        if self.payload_refs.is_empty()
            || self.payload_state != DataPayloadState::Available
            || self.metadata.data_epoch != current_data_epoch
        {
            return Err("receipt_payload_authorization_denied".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "receipt_digest": self.receipt_digest,
            "metadata": self.metadata,
            "payload_refs": self.payload_refs,
            "payload_state": self.payload_state,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataPropagationTarget {
    EventProjection,
    Receipt,
    Audit,
    Artifact,
    Memory,
    Index,
    Cache,
    Checkpoint,
    Export,
}

impl DataPropagationTarget {
    pub const ALL: [Self; 9] = [
        Self::EventProjection,
        Self::Receipt,
        Self::Audit,
        Self::Artifact,
        Self::Memory,
        Self::Index,
        Self::Cache,
        Self::Checkpoint,
        Self::Export,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EventProjection => "event_projection",
            Self::Receipt => "receipt",
            Self::Audit => "audit",
            Self::Artifact => "artifact",
            Self::Memory => "memory",
            Self::Index => "index",
            Self::Cache => "cache",
            Self::Checkpoint => "checkpoint",
            Self::Export => "export",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataPropagationState {
    Unknown,
    Invalidated,
    PreservedMetadata,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPropagationReceipt {
    pub schema: String,
    pub version: SchemaVersion,
    pub target: DataPropagationTarget,
    pub state: DataPropagationState,
    pub project_ref: String,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub tombstone_digest: String,
    pub receipt_digest: Option<String>,
    pub observed_at_ms: u64,
}

impl DataPropagationReceipt {
    pub fn unknown(
        target: DataPropagationTarget,
        project_ref: impl Into<String>,
        previous_epoch: u64,
        data_epoch: u64,
        source_cursor: EventCursor,
        tombstone_digest: impl Into<String>,
        observed_at_ms: u64,
    ) -> Self {
        Self {
            schema: DATA_PROPAGATION_RECEIPT_SCHEMA.to_owned(),
            version: DATA_PROPAGATION_VERSION,
            target,
            state: DataPropagationState::Unknown,
            project_ref: project_ref.into(),
            previous_epoch,
            data_epoch,
            source_cursor,
            tombstone_digest: tombstone_digest.into(),
            receipt_digest: None,
            observed_at_ms,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DATA_PROPAGATION_RECEIPT_SCHEMA
            || !self.version.is_compatible_with(&DATA_PROPAGATION_VERSION)
            || self.previous_epoch == 0
            || self.data_epoch <= self.previous_epoch
            || self.source_cursor == 0
            || self.observed_at_ms == 0
            || self.receipt_digest.is_some() && self.state == DataPropagationState::Unknown
            || self.state != DataPropagationState::Unknown && self.receipt_digest.is_none()
            || !valid_digest(&self.tombstone_digest)
        {
            return Err("data_propagation_receipt_invalid".to_owned());
        }
        nonempty(&self.project_ref, "data_propagation_project", 512)?;
        if let Some(receipt_digest) = &self.receipt_digest {
            if !valid_digest(receipt_digest) {
                return Err("data_propagation_receipt_digest_invalid".to_owned());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPropagationPlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_ref: String,
    pub kind: String,
    pub previous_epoch: u64,
    pub data_epoch: u64,
    pub source_cursor: EventCursor,
    pub tombstone_digest: String,
    pub targets: Vec<DataPropagationReceipt>,
    pub immutable_event_seal: bool,
    pub plan_digest: String,
}

impl DataPropagationPlan {
    pub fn from_snapshot(
        snapshot: &DataGovernanceSnapshot,
        previous_epoch: u64,
        kind: impl Into<String>,
        tombstone_digest: impl Into<String>,
        observed_at_ms: u64,
    ) -> Result<Self, String> {
        snapshot.validate()?;
        let tombstone_digest = tombstone_digest.into();
        let targets = DataPropagationTarget::ALL
            .into_iter()
            .map(|target| {
                let state = if target == DataPropagationTarget::EventProjection
                    || (target == DataPropagationTarget::Audit && snapshot.audit_metadata_retained)
                {
                    DataPropagationState::PreservedMetadata
                } else {
                    DataPropagationState::Unknown
                };
                let mut receipt = DataPropagationReceipt {
                    schema: DATA_PROPAGATION_RECEIPT_SCHEMA.to_owned(),
                    version: DATA_PROPAGATION_VERSION,
                    target,
                    state,
                    project_ref: snapshot.project_ref.clone(),
                    previous_epoch,
                    data_epoch: snapshot.data_epoch,
                    source_cursor: snapshot.source_cursor,
                    tombstone_digest: tombstone_digest.clone(),
                    receipt_digest: None,
                    observed_at_ms,
                };
                if state == DataPropagationState::PreservedMetadata {
                    receipt.receipt_digest = Some(json_digest(&serde_json::json!({
                        "target": receipt.target,
                        "project_ref": receipt.project_ref,
                        "data_epoch": receipt.data_epoch,
                        "source_cursor": receipt.source_cursor,
                        "tombstone_digest": receipt.tombstone_digest,
                    })));
                }
                receipt
            })
            .collect::<Vec<_>>();
        let mut plan = Self {
            schema: DATA_PROPAGATION_PLAN_SCHEMA.to_owned(),
            version: DATA_PROPAGATION_VERSION,
            project_ref: snapshot.project_ref.clone(),
            kind: kind.into(),
            previous_epoch,
            data_epoch: snapshot.data_epoch,
            source_cursor: snapshot.source_cursor,
            tombstone_digest,
            targets,
            immutable_event_seal: true,
            plan_digest: String::new(),
        };
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DATA_PROPAGATION_PLAN_SCHEMA
            || !self.version.is_compatible_with(&DATA_PROPAGATION_VERSION)
            || self.previous_epoch == 0
            || self.data_epoch <= self.previous_epoch
            || self.source_cursor == 0
            || !self.immutable_event_seal
            || self.targets.len() != DataPropagationTarget::ALL.len()
            || self.targets.len() > MAX_PROPAGATION_TARGETS
            || !valid_digest(&self.tombstone_digest)
            || !valid_digest(&self.plan_digest)
        {
            return Err("data_propagation_plan_header_invalid".to_owned());
        }
        nonempty(&self.project_ref, "data_propagation_plan_project", 512)?;
        nonempty(&self.kind, "data_propagation_plan_kind", 128)?;
        let mut seen = BTreeSet::new();
        for receipt in &self.targets {
            receipt.validate()?;
            if receipt.project_ref != self.project_ref
                || receipt.previous_epoch != self.previous_epoch
                || receipt.data_epoch != self.data_epoch
                || receipt.source_cursor != self.source_cursor
                || receipt.tombstone_digest != self.tombstone_digest
                || !seen.insert(receipt.target)
            {
                return Err("data_propagation_target_boundary_invalid".to_owned());
            }
        }
        if DataPropagationTarget::ALL
            .iter()
            .any(|target| !seen.contains(target))
            || self.plan_digest != self.digest()
        {
            return Err("data_propagation_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn unresolved_targets(&self) -> Vec<DataPropagationTarget> {
        self.targets
            .iter()
            .filter(|receipt| receipt.state == DataPropagationState::Unknown)
            .map(|receipt| receipt.target)
            .collect()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_ref": self.project_ref,
            "kind": self.kind,
            "previous_epoch": self.previous_epoch,
            "data_epoch": self.data_epoch,
            "source_cursor": self.source_cursor,
            "tombstone_digest": self.tombstone_digest,
            "targets": self.targets,
            "immutable_event_seal": self.immutable_event_seal,
        }))
    }
}

/// A seal for immutable source facts. Data governance may add tombstones and projections, but it
/// cannot rewrite the events that produced a receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImmutableEventSeal {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_cursor: EventCursor,
    pub data_epoch: u64,
    pub source_event_ids: Vec<EventId>,
    pub encryption_profile: String,
    pub sealed: bool,
    pub seal_digest: String,
}

impl ImmutableEventSeal {
    pub fn new(
        source_cursor: EventCursor,
        data_epoch: u64,
        source_event_ids: Vec<EventId>,
        encryption_profile: impl Into<String>,
    ) -> Result<Self, String> {
        let mut seal = Self {
            schema: IMMUTABLE_EVENT_SEAL_SCHEMA.to_owned(),
            version: DATA_PROPAGATION_VERSION,
            source_cursor,
            data_epoch,
            source_event_ids,
            encryption_profile: encryption_profile.into(),
            sealed: true,
            seal_digest: String::new(),
        };
        seal.seal_digest = seal.digest();
        seal.validate()?;
        Ok(seal)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != IMMUTABLE_EVENT_SEAL_SCHEMA
            || !self.version.is_compatible_with(&DATA_PROPAGATION_VERSION)
            || self.source_cursor == 0
            || self.data_epoch == 0
            || self.source_event_ids.is_empty()
            || !self.sealed
            || !valid_digest(&self.seal_digest)
        {
            return Err("immutable_event_seal_invalid".to_owned());
        }
        nonempty(
            &self.encryption_profile,
            "immutable_event_seal_encryption_profile",
            128,
        )?;
        let mut ids = BTreeSet::new();
        if self
            .source_event_ids
            .iter()
            .any(|event_id| !ids.insert(event_id.to_string()))
            || self.seal_digest != self.digest()
        {
            return Err("immutable_event_seal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "source_event_ids": self.source_event_ids,
            "encryption_profile": self.encryption_profile,
            "sealed": self.sealed,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataRetentionObservation {
    pub source_ref: String,
    pub class: DataClass,
    pub purpose_id: String,
    pub payload: DataPayloadState,
    pub audit_metadata_retained: bool,
    pub source_digest: String,
}

impl DataRetentionObservation {
    pub fn validate(&self) -> Result<(), String> {
        nonempty(&self.source_ref, "data_source_ref", 512)?;
        nonempty(&self.purpose_id, "data_purpose_id", 128)?;
        sha256_digest(&self.source_digest, "data_source_digest")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataGovernanceSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_ref: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub source_cursor: u64,
    pub source_event_ids: Vec<EventId>,
    pub observations: Vec<DataRetentionObservation>,
    pub derived_store_states: BTreeMap<String, DataPayloadState>,
    pub audit_metadata_retained: bool,
    pub snapshot_digest: String,
}

impl DataGovernanceSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_ref: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        source_cursor: u64,
        source_event_ids: Vec<EventId>,
        observations: Vec<DataRetentionObservation>,
        derived_store_states: BTreeMap<String, DataPayloadState>,
        audit_metadata_retained: bool,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: DATA_GOVERNANCE_SNAPSHOT_SCHEMA.to_owned(),
            version: DATA_GOVERNANCE_SNAPSHOT_SCHEMA_VERSION,
            project_ref: project_ref.into(),
            policy_revision,
            data_epoch,
            source_cursor,
            source_event_ids,
            observations,
            derived_store_states,
            audit_metadata_retained,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DATA_GOVERNANCE_SNAPSHOT_SCHEMA
            || !self
                .version
                .is_compatible_with(&DATA_GOVERNANCE_SNAPSHOT_SCHEMA_VERSION)
        {
            return Err("data_governance_snapshot_schema_invalid".to_owned());
        }
        nonempty(&self.project_ref, "data_project_ref", 512)?;
        if self.data_epoch == 0 || self.source_cursor == 0 {
            return Err("data_governance_snapshot_cursor_required".to_owned());
        }
        if self.source_event_ids.is_empty() || self.source_event_ids.len() > MAX_SOURCE_EVENT_IDS {
            return Err("data_governance_snapshot_source_ids_invalid".to_owned());
        }
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            if !ids.insert(event_id.to_string()) {
                return Err("data_governance_snapshot_source_duplicate".to_owned());
            }
        }
        if self.observations.len() > MAX_DATA_OBSERVATIONS {
            return Err("data_governance_observation_limit".to_owned());
        }
        for observation in &self.observations {
            observation.validate()?;
        }
        if self.derived_store_states.len() > MAX_DERIVED_DATA_STORES {
            return Err("data_governance_derived_store_limit".to_owned());
        }
        for store in self.derived_store_states.keys() {
            nonempty(store, "data_derived_store", 64)?;
        }
        if self.observations.iter().any(|observation| {
            observation.audit_metadata_retained
                && !self.audit_metadata_retained
                && observation.payload != DataPayloadState::Available
        }) {
            return Err("data_audit_metadata_propagation_conflict".to_owned());
        }
        sha256_digest(&self.snapshot_digest, "data_governance_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("data_governance_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        crate::canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "snapshot_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

/// Derive data visibility and all derived-store propagation from one server-owned policy.
pub fn project_data_governance(
    policy: &DataPolicy,
    project_ref: impl Into<String>,
    source_cursor: u64,
    source_event_ids: Vec<EventId>,
    now_ms: u64,
) -> Result<DataGovernanceSnapshot, String> {
    policy.validate()?;
    let mut observations = Vec::with_capacity(policy.grants.len());
    for grant in policy.grants.values() {
        let payload = if grant.revoked {
            DataPayloadState::Revoked
        } else if grant
            .retention
            .expires_at_ms
            .is_some_and(|expiry| expiry <= now_ms)
        {
            DataPayloadState::Expired
        } else {
            DataPayloadState::Available
        };
        observations.push(DataRetentionObservation {
            source_ref: grant.source_path.clone(),
            class: grant.class,
            purpose_id: grant.purpose.id.clone(),
            payload,
            audit_metadata_retained: grant.retention.retain_audit_metadata,
            source_digest: grant.content_hash.clone(),
        });
    }
    let state = if observations
        .iter()
        .any(|observation| observation.payload == DataPayloadState::Revoked)
    {
        DataPayloadState::Revoked
    } else if observations
        .iter()
        .any(|observation| observation.payload == DataPayloadState::Expired)
    {
        DataPayloadState::Expired
    } else {
        DataPayloadState::Available
    };
    let derived_store_states = [
        "receipt",
        "audit",
        "artifact",
        "memory",
        "memory-jsonl",
        "memory-body",
        "index",
        "bm25-index",
        "dense-index",
        "repo-index",
        "cache",
        "prompt-cache",
        "context-plan",
        "summary",
        "checkpoint",
        "ui",
        "export",
    ]
    .into_iter()
    .map(|store| (store.to_owned(), state))
    .collect();
    let audit_metadata_retained = observations
        .iter()
        .any(|observation| observation.audit_metadata_retained);
    DataGovernanceSnapshot::new(
        project_ref,
        policy.revision,
        policy.data_epoch,
        source_cursor,
        source_event_ids,
        observations,
        derived_store_states,
        audit_metadata_retained,
    )
}
