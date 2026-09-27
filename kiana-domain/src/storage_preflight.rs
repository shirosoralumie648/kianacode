//! Deterministic storage root, trust and filesystem preflight contracts.
//!
//! The resolver consumes adapter-supplied metadata. It never reads a path, follows a symlink,
//! creates a directory, checks free space or acquires a lock. Unsupported or unproven adapter
//! capabilities remain explicit blocked reasons instead of becoming authorization.

use crate::{
    enforce_path_containment, json_digest, normalize_role_path, ProjectId, ProjectTrustSnapshot,
    SchemaVersion, StorageBackend, StorageFileIdentity, StorageRoot, StorageSecurityCapabilities,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const STORAGE_PREFLIGHT_SCHEMA: &str = "kiana.storage-preflight.v1";
pub const STORAGE_PREFLIGHT_REPORT_SCHEMA: &str = "kiana.storage-preflight-report.v1";
pub const STORAGE_PREFLIGHT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_PREFLIGHT_PATHS: usize = 256;
pub const MAX_PREFLIGHT_REASONS: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePreflightStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePreflightRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub root: StorageRoot,
    pub project_trust: ProjectTrustSnapshot,
    pub project_root_digest: String,
    pub capabilities: StorageSecurityCapabilities,
    pub requested_paths: Vec<String>,
    pub file_identities: Vec<StorageFileIdentity>,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub required_inodes: u64,
    pub available_inodes: u64,
    pub request_digest: String,
}

impl StoragePreflightRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root: StorageRoot,
        project_trust: ProjectTrustSnapshot,
        project_root_digest: impl Into<String>,
        capabilities: StorageSecurityCapabilities,
        requested_paths: Vec<String>,
        file_identities: Vec<StorageFileIdentity>,
        required_bytes: u64,
        available_bytes: u64,
        required_inodes: u64,
        available_inodes: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: STORAGE_PREFLIGHT_SCHEMA.to_owned(),
            version: STORAGE_PREFLIGHT_VERSION,
            root,
            project_trust,
            project_root_digest: project_root_digest.into(),
            capabilities,
            requested_paths,
            file_identities,
            required_bytes,
            available_bytes,
            required_inodes,
            available_inodes,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_PREFLIGHT_SCHEMA || self.version != STORAGE_PREFLIGHT_VERSION {
            return Err("storage_preflight_header_invalid".to_owned());
        }
        self.project_trust.validate()?;
        valid_digest(
            &self.project_root_digest,
            "storage_preflight_project_root_digest",
        )?;
        self.capabilities.validate()?;
        if self.requested_paths.is_empty()
            || self.requested_paths.len() > MAX_PREFLIGHT_PATHS
            || self.file_identities.len() > MAX_PREFLIGHT_PATHS
        {
            return Err("storage_preflight_path_limit".to_owned());
        }
        for path in &self.requested_paths {
            if path.trim().is_empty()
                || path.len() > 4_096
                || path.contains('\\')
                || path.bytes().any(|byte| byte == 0 || byte < 0x20)
            {
                return Err("storage_preflight_path_invalid".to_owned());
            }
        }
        for identity in &self.file_identities {
            validate_identity_metadata(identity)?;
        }
        valid_digest(&self.request_digest, "storage_preflight_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("storage_preflight_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root": self.root,
            "project_trust": self.project_trust,
            "project_root_digest": self.project_root_digest,
            "capabilities": self.capabilities,
            "requested_paths": self.requested_paths,
            "file_identities": self.file_identities,
            "required_bytes": self.required_bytes,
            "available_bytes": self.available_bytes,
            "required_inodes": self.required_inodes,
            "available_inodes": self.available_inodes,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePreflightReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub root_id: crate::StorageRootId,
    pub root_digest: String,
    pub backend: StorageBackend,
    pub project_id: ProjectId,
    pub project_root_digest: String,
    pub trust_digest: String,
    pub capability_digest: String,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub required_inodes: u64,
    pub available_inodes: u64,
    pub checked_paths: Vec<String>,
    pub status: StoragePreflightStatus,
    pub reasons: Vec<String>,
    pub remediation: Vec<String>,
    pub report_digest: String,
}

impl StoragePreflightReport {
    pub fn evaluate(request: &StoragePreflightRequest) -> Result<Self, String> {
        request.validate()?;
        let mut checked_paths = BTreeSet::new();
        let mut reasons = BTreeSet::new();

        let root_validation = request.root.validate();
        if let Err(error) = root_validation {
            reasons.insert(storage_root_reason(&error).to_owned());
        }
        if request
            .root
            .owner_scope
            .project_id
            .is_some_and(|id| id != request.project_trust.project_id)
            || request.project_trust.canonical_root_digest != request.project_root_digest
        {
            reasons.insert("project_root_mismatch".to_owned());
        }
        if !request.project_trust.trusted {
            reasons.insert("project_untrusted".to_owned());
        }
        for path in &request.requested_paths {
            match normalize_role_path(path) {
                Some(normalized) => {
                    checked_paths.insert(normalized.clone());
                    if enforce_path_containment(&[".".to_owned()], &normalized).is_err() {
                        reasons.insert("path_escape".to_owned());
                    }
                }
                None => {
                    reasons.insert("path_escape".to_owned());
                }
            }
        }
        let requested = checked_paths.iter().cloned().collect::<BTreeSet<_>>();
        for identity in &request.file_identities {
            if identity.root_digest != request.root.root_digest {
                reasons.insert("file_identity_root_mismatch".to_owned());
            }
            if !requested.contains(&identity.relative_path) {
                reasons.insert("file_identity_unrequested".to_owned());
            }
            if identity.symlink {
                reasons.insert("symlink_target".to_owned());
            }
            if identity.hardlink {
                reasons.insert("hardlink_target".to_owned());
            }
            if identity.exists && !identity.regular_file {
                reasons.insert("non_regular_target".to_owned());
            }
            if identity.mode.is_some_and(|mode| mode & 0o077 != 0) {
                reasons.insert("permissions_too_broad".to_owned());
            }
        }
        if !request.capabilities.symlink_guard {
            reasons.insert("symlink_guard_unavailable".to_owned());
        }
        if !request.capabilities.hardlink_guard {
            reasons.insert("hardlink_guard_unavailable".to_owned());
        }
        if !request.capabilities.permission_guard {
            reasons.insert("permission_guard_unavailable".to_owned());
        }
        if !request.capabilities.atomic_replace {
            reasons.insert("atomic_replace_unproven".to_owned());
        }
        if request.available_bytes < request.required_bytes {
            reasons.insert("capacity_bytes_insufficient".to_owned());
        }
        if request.available_inodes < request.required_inodes {
            reasons.insert("capacity_inodes_insufficient".to_owned());
        }
        let reasons = reasons.into_iter().collect::<Vec<_>>();
        let remediation = reasons
            .iter()
            .map(|reason| format!("remediate:{reason}"))
            .collect::<Vec<_>>();
        let mut report = Self {
            schema: STORAGE_PREFLIGHT_REPORT_SCHEMA.to_owned(),
            version: STORAGE_PREFLIGHT_VERSION,
            root_id: request.root.root_id,
            root_digest: request.root.root_digest.clone(),
            backend: request.root.backend,
            project_id: request.project_trust.project_id,
            project_root_digest: request.project_root_digest.clone(),
            trust_digest: request.project_trust.trust_digest.clone(),
            capability_digest: request.capabilities.capability_digest.clone(),
            required_bytes: request.required_bytes,
            available_bytes: request.available_bytes,
            required_inodes: request.required_inodes,
            available_inodes: request.available_inodes,
            checked_paths: checked_paths.into_iter().collect(),
            status: if reasons.is_empty() {
                StoragePreflightStatus::Ready
            } else {
                StoragePreflightStatus::Blocked
            },
            reasons,
            remediation,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_PREFLIGHT_REPORT_SCHEMA
            || self.version != STORAGE_PREFLIGHT_VERSION
            || self.root_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
        {
            return Err("storage_preflight_report_header_invalid".to_owned());
        }
        valid_digest(&self.root_digest, "storage_preflight_report_root_digest")?;
        valid_digest(
            &self.project_root_digest,
            "storage_preflight_report_project_root_digest",
        )?;
        valid_digest(&self.trust_digest, "storage_preflight_report_trust_digest")?;
        valid_digest(
            &self.capability_digest,
            "storage_preflight_report_capability_digest",
        )?;
        if self.checked_paths.len() > MAX_PREFLIGHT_PATHS
            || (self.checked_paths.is_empty() && self.status == StoragePreflightStatus::Ready)
        {
            return Err("storage_preflight_report_path_limit".to_owned());
        }
        if self.checked_paths.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("storage_preflight_report_paths_noncanonical".to_owned());
        }
        if self.reasons.len() > MAX_PREFLIGHT_REASONS
            || self.reasons.windows(2).any(|pair| pair[0] >= pair[1])
            || self.remediation
                != self
                    .reasons
                    .iter()
                    .map(|reason| format!("remediate:{reason}"))
                    .collect::<Vec<_>>()
        {
            return Err("storage_preflight_report_reasons_invalid".to_owned());
        }
        let expected_status = if self.reasons.is_empty() {
            StoragePreflightStatus::Ready
        } else {
            StoragePreflightStatus::Blocked
        };
        if self.status != expected_status {
            return Err("storage_preflight_report_status_mismatch".to_owned());
        }
        valid_digest(&self.report_digest, "storage_preflight_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("storage_preflight_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root_id": self.root_id,
            "root_digest": self.root_digest,
            "backend": self.backend,
            "project_id": self.project_id,
            "project_root_digest": self.project_root_digest,
            "trust_digest": self.trust_digest,
            "capability_digest": self.capability_digest,
            "required_bytes": self.required_bytes,
            "available_bytes": self.available_bytes,
            "required_inodes": self.required_inodes,
            "available_inodes": self.available_inodes,
            "checked_paths": self.checked_paths,
            "status": self.status,
            "reasons": self.reasons,
            "remediation": self.remediation,
        }))
    }
}

fn validate_identity_metadata(identity: &StorageFileIdentity) -> Result<(), String> {
    if identity.schema != crate::STORAGE_FILE_IDENTITY_SCHEMA
        || identity.version != crate::STORAGE_SECURITY_SCHEMA_VERSION
    {
        return Err("storage_preflight_identity_header_invalid".to_owned());
    }
    valid_digest(
        &identity.root_digest,
        "storage_preflight_identity_root_digest",
    )?;
    if normalize_role_path(&identity.relative_path).as_deref()
        != Some(identity.relative_path.as_str())
    {
        return Err("storage_preflight_identity_path_invalid".to_owned());
    }
    valid_digest(
        &identity.identity_digest,
        "storage_preflight_identity_digest",
    )?;
    if identity.identity_digest != identity.digest() {
        return Err("storage_preflight_identity_digest_mismatch".to_owned());
    }
    Ok(())
}

fn storage_root_reason(error: &str) -> &str {
    if error.contains("network_filesystem") {
        "remote_filesystem_unsupported"
    } else if error.contains("namespace") {
        "storage_namespace_invalid"
    } else if error.contains("owner") || error.contains("project") {
        "storage_owner_mismatch"
    } else {
        "storage_root_invalid"
    }
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
