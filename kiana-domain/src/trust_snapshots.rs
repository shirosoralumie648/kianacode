//! Server-owned project trust and department catalog snapshots.
//!
//! These snapshots are immutable inputs to core policy. They record the source digest and scope,
//! but do not authenticate a caller or grant capabilities on their own.

use crate::{json_digest, DepartmentSpec, ProjectId, ProjectIdentity, RoleSpec, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const PROJECT_TRUST_SNAPSHOT_SCHEMA: &str = "kiana.project-trust-snapshot.v1";
pub const DEPARTMENT_SNAPSHOT_SCHEMA: &str = "kiana.department-snapshot.v1";
pub const TRUST_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectTrustSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub project_id: ProjectId,
    pub canonical_root_digest: String,
    pub trust_revision: String,
    pub trusted: bool,
    pub source: String,
    pub revision: u64,
    pub trust_digest: String,
}

impl ProjectTrustSnapshot {
    pub fn from_project(
        project: &ProjectIdentity,
        trusted: bool,
        source: impl Into<String>,
        revision: u64,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: PROJECT_TRUST_SNAPSHOT_SCHEMA.to_owned(),
            version: TRUST_SNAPSHOT_VERSION,
            project_id: project.project_id,
            canonical_root_digest: json_digest(&serde_json::json!({
                "canonical_root": project.canonical_root
            })),
            trust_revision: project.trust_revision.clone(),
            trusted,
            source: source.into(),
            revision,
            trust_digest: String::new(),
        };
        snapshot.trust_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_value(value.clone())
            .map_err(|_| "project_trust_snapshot_decode_failed".to_owned())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "project_trust_snapshot_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_TRUST_SNAPSHOT_SCHEMA
            || !self.version.is_compatible_with(&TRUST_SNAPSHOT_VERSION)
            || self.project_id.as_uuid().is_nil()
            || self.source.trim().is_empty()
            || self.source.len() > 128
            || self.source.contains('\0')
            || self.revision == 0
        {
            return Err("project_trust_snapshot_header_invalid".to_owned());
        }
        validate_digest(
            &self.canonical_root_digest,
            "project_trust_snapshot_root_digest",
        )?;
        validate_digest(&self.trust_revision, "project_trust_snapshot_revision")?;
        validate_digest(&self.trust_digest, "project_trust_snapshot_digest")?;
        if self.trust_digest != self.digest() {
            return Err("project_trust_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "project_id": self.project_id,
            "canonical_root_digest": self.canonical_root_digest,
            "trust_revision": self.trust_revision,
            "trusted": self.trusted,
            "source": self.source,
            "revision": self.revision,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepartmentSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub department_id: String,
    pub role_ids: Vec<String>,
    pub revision: u64,
    pub authority_epoch: u64,
    pub department_digest: String,
}

impl DepartmentSnapshot {
    pub fn from_spec(
        spec: &DepartmentSpec,
        revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        spec.validate()?;
        let mut role_ids = spec.roles.clone();
        role_ids.sort();
        let mut snapshot = Self {
            schema: DEPARTMENT_SNAPSHOT_SCHEMA.to_owned(),
            version: TRUST_SNAPSHOT_VERSION,
            department_id: spec.department_id.clone(),
            role_ids,
            revision,
            authority_epoch,
            department_digest: String::new(),
        };
        snapshot.department_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_value(value.clone())
            .map_err(|_| "department_snapshot_decode_failed".to_owned())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn contains_role(&self, role_id: &str) -> bool {
        self.role_ids.iter().any(|candidate| candidate == role_id)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DEPARTMENT_SNAPSHOT_SCHEMA
            || !self.version.is_compatible_with(&TRUST_SNAPSHOT_VERSION)
            || self.department_id.trim().is_empty()
            || self.revision == 0
            || self.authority_epoch == 0
            || self.role_ids.is_empty()
        {
            return Err("department_snapshot_header_invalid".to_owned());
        }
        if DepartmentSpec::lookup(&self.department_id).is_none() {
            return Err("department_snapshot_department_unknown".to_owned());
        }
        let mut ids = BTreeSet::new();
        for role_id in &self.role_ids {
            if role_id.trim().is_empty()
                || RoleSpec::lookup(role_id)
                    .is_none_or(|role| role.department_id != self.department_id)
                || !ids.insert(role_id.as_str())
            {
                return Err("department_snapshot_roles_invalid".to_owned());
            }
        }
        if self.role_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("department_snapshot_roles_noncanonical".to_owned());
        }
        validate_digest(&self.department_digest, "department_snapshot_digest")?;
        if self.department_digest != self.digest() {
            return Err("department_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "department_id": self.department_id,
            "role_ids": self.role_ids,
            "revision": self.revision,
            "authority_epoch": self.authority_epoch,
        }))
    }
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
