//! One server-derived, per-record Memory ACL decision for every read path.
//!
//! Collection labels are only a prefilter. Final admission checks the effective MemoryScope,
//! project/session ownership, purpose, sensitivity, lifecycle, validity and classification before
//! search, prefetch, review, citation, similar-record or resume callers may expose a record.

use crate::{
    json_digest, MemoryClassification, MemoryRecord, MemoryScope, MemorySensitivity, MemoryState,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const MEMORY_ACL_REQUEST_SCHEMA: &str = "kiana.memory-acl-request.v1";
pub const MEMORY_ACL_DECISION_SCHEMA: &str = "kiana.memory-acl-decision.v1";
pub const MEMORY_ACL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
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
pub enum MemoryAccessPath {
    Search,
    AutoPrefetch,
    ReviewList,
    Citation,
    ProposalSimilar,
    Resume,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryAclRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub scope: MemoryScope,
    pub path: MemoryAccessPath,
    pub sensitivity_ceiling: MemorySensitivity,
    pub now_ms: u64,
    pub data_epoch: u64,
    pub request_digest: String,
}

impl MemoryAclRequest {
    pub fn new(
        scope: MemoryScope,
        path: MemoryAccessPath,
        sensitivity_ceiling: MemorySensitivity,
        now_ms: u64,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: MEMORY_ACL_REQUEST_SCHEMA.to_owned(),
            version: MEMORY_ACL_VERSION,
            scope,
            path,
            sensitivity_ceiling,
            now_ms,
            data_epoch,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_ACL_REQUEST_SCHEMA
            || !self.version.is_compatible_with(&MEMORY_ACL_VERSION)
            || self.now_ms == 0
            || self.data_epoch == 0
        {
            return Err("memory_acl_request_header_invalid".to_owned());
        }
        self.scope.validate()?;
        digest(&self.request_digest, "memory_acl_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("memory_acl_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "scope": self.scope,
            "path": self.path,
            "sensitivity_ceiling": self.sensitivity_ceiling,
            "now_ms": self.now_ms,
            "data_epoch": self.data_epoch,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryAclDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub request_digest: String,
    pub scope_digest: String,
    pub record_id: String,
    pub record_revision: u64,
    pub allowed: bool,
    pub reason: String,
    pub decision_digest: String,
}

impl MemoryAclDecision {
    pub fn evaluate(request: &MemoryAclRequest, record: &MemoryRecord) -> Result<Self, String> {
        request.validate()?;
        record.validate_lifecycle()?;
        let collection = crate::MemoryCollection::parse(&record.collection)
            .ok_or_else(|| "memory_acl_record_collection_invalid".to_owned())?;
        let (allowed, reason) = if !request.scope.allows_collection(&collection) {
            (false, "collection_denied")
        } else if !project_matches(request, record) {
            (false, "project_denied")
        } else if record.layer == crate::MEMORY_LAYER_INSTANCE_SCRATCH
            && record.session_id != request.scope.session_id.as_str()
        {
            (false, "scratch_session_denied")
        } else if record.layer == crate::MEMORY_LAYER_USER
            && record.session_id != request.scope.session_id.as_str()
        {
            (false, "user_private_session_denied")
        } else if record
            .purpose
            .as_ref()
            .is_none_or(|purpose| purpose.id != request.scope.purpose.id)
        {
            (false, "purpose_denied")
        } else if sensitivity_rank(record.sensitivity)
            > sensitivity_rank(request.sensitivity_ceiling)
            || record.sensitivity == MemorySensitivity::Unknown
        {
            (false, "sensitivity_denied")
        } else if !valid_now(record, request.now_ms) {
            (false, "validity_denied")
        } else if record.classification != MemoryClassification::Unknown
            && record.classification != MemoryClassification::for_collection(&collection)
        {
            (false, "classification_denied")
        } else if !admitted_for_path(request.path, record) {
            (false, "lifecycle_denied")
        } else {
            (true, "allowed")
        };
        let mut decision = Self {
            schema: MEMORY_ACL_DECISION_SCHEMA.to_owned(),
            version: MEMORY_ACL_VERSION,
            request_digest: request.request_digest.clone(),
            scope_digest: request.scope.scope_digest.clone(),
            record_id: record.id.clone(),
            record_revision: record.revision,
            allowed,
            reason: reason.to_owned(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMORY_ACL_DECISION_SCHEMA
            || !self.version.is_compatible_with(&MEMORY_ACL_VERSION)
            || self.record_revision == 0
            || self.reason.trim().is_empty()
        {
            return Err("memory_acl_decision_header_invalid".to_owned());
        }
        digest(&self.request_digest, "memory_acl_decision_request_digest")?;
        digest(&self.scope_digest, "memory_acl_decision_scope_digest")?;
        digest(&self.decision_digest, "memory_acl_decision_digest")?;
        required(&self.record_id, "memory_acl_decision_record_id", 256)?;
        if self.decision_digest != self.digest() {
            return Err("memory_acl_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "request_digest": self.request_digest,
            "scope_digest": self.scope_digest,
            "record_id": self.record_id,
            "record_revision": self.record_revision,
            "allowed": self.allowed,
            "reason": self.reason,
        }))
    }
}

fn project_matches(request: &MemoryAclRequest, record: &MemoryRecord) -> bool {
    let expected = request.scope.project.canonical_root.as_str();
    let root_matches = record.project_root == request.scope.project.root
        || record.project_root == expected
        || (record.project_root.is_empty() && record.layer == crate::MEMORY_LAYER_COMPANY);
    root_matches
}

fn valid_now(record: &MemoryRecord, now_ms: u64) -> bool {
    record
        .validity
        .valid_from_ms
        .is_none_or(|from| from <= now_ms)
        && record.validity.valid_to_ms.is_none_or(|to| now_ms < to)
}

fn admitted_for_path(path: MemoryAccessPath, record: &MemoryRecord) -> bool {
    match path {
        MemoryAccessPath::ReviewList => {
            record.state != MemoryState::Rejected
                && record.admission_state != crate::MemoryAdmission::Rejected
        }
        MemoryAccessPath::Search
        | MemoryAccessPath::AutoPrefetch
        | MemoryAccessPath::Citation
        | MemoryAccessPath::ProposalSimilar
        | MemoryAccessPath::Resume => record.searchable(),
    }
}

fn sensitivity_rank(sensitivity: MemorySensitivity) -> u8 {
    match sensitivity {
        MemorySensitivity::Unknown => 0,
        MemorySensitivity::Public => 1,
        MemorySensitivity::Internal => 2,
        MemorySensitivity::Confidential => 3,
        MemorySensitivity::Restricted => 4,
    }
}
