//! Server-owned authority snapshot joining project trust, role assignment and department scope.
//!
//! This is a pure validation/value layer. It never treats a wire role/project/trust field as an
//! authority source and never dispatches a capability. Durable assignment/revocation remains an
//! adapter concern.

use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, DepartmentSnapshot, ProjectTrustSnapshot,
    RequestContext, ResolvedAssignment, SchemaVersion, SecurityContextId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SECURITY_AUTHORITY_SNAPSHOT_SCHEMA: &str = "kiana.security-authority-snapshot.v1";
pub const SECURITY_AUTHORITY_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityAuthoritySnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub security_context_id: SecurityContextId,
    pub principal: AuthenticatedPrincipalRef,
    pub project_trust: ProjectTrustSnapshot,
    pub assignment: ResolvedAssignment,
    pub department: DepartmentSnapshot,
    pub authority_epoch: u64,
    pub snapshot_digest: String,
}

impl SecurityAuthoritySnapshot {
    pub fn from_parts(
        security_context_id: SecurityContextId,
        principal: AuthenticatedPrincipalRef,
        project_trust: ProjectTrustSnapshot,
        assignment: ResolvedAssignment,
        department: DepartmentSnapshot,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: SECURITY_AUTHORITY_SNAPSHOT_SCHEMA.to_owned(),
            version: SECURITY_AUTHORITY_SNAPSHOT_VERSION,
            security_context_id,
            principal,
            project_trust,
            assignment,
            department,
            authority_epoch,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let snapshot: Self = serde_json::from_value(value.clone())
            .map_err(|_| "security_authority_snapshot_decode_failed".to_owned())?;
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self)
            .map_err(|_| "security_authority_snapshot_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_AUTHORITY_SNAPSHOT_SCHEMA
            || !self
                .version
                .is_compatible_with(&SECURITY_AUTHORITY_SNAPSHOT_VERSION)
            || self.security_context_id.as_uuid().is_nil()
            || self.authority_epoch == 0
        {
            return Err("security_authority_snapshot_header_invalid".to_owned());
        }
        self.principal
            .validate()
            .map_err(|_| "AUTH_PRINCIPAL_MISSING".to_owned())?;
        self.project_trust.validate()?;
        self.assignment.validate()?;
        self.department.validate()?;
        if self.assignment.principal != self.principal {
            return Err("AUTH_PRINCIPAL_MISMATCH".to_owned());
        }
        if self.assignment.project_id != self.project_trust.project_id {
            return Err("AUTH_PROJECT_MISMATCH".to_owned());
        }
        if self.assignment.authority_epoch != self.authority_epoch {
            return Err("POLICY_AUTHORITY_EPOCH_STALE".to_owned());
        }
        if self.department.department_id != self.assignment.department_id
            || !self.department.contains_role(&self.assignment.role_id)
        {
            return Err("AUTH_ROLE_MISMATCH".to_owned());
        }
        validate_digest(&self.snapshot_digest, "security_authority_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("security_authority_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_request(&self, context: &RequestContext) -> Result<(), String> {
        self.validate()?;
        if context.actor_id.as_deref() != Some(self.principal.principal_id.as_str()) {
            return Err("AUTH_CALLER_UNTRUSTED".to_owned());
        }
        if context.project_root.trim().is_empty() {
            return Err("AUTH_PROJECT_MISMATCH".to_owned());
        }
        if context.role_id != self.assignment.role_id
            || context.department_id != self.assignment.department_id
        {
            return Err("AUTH_ROLE_MISMATCH".to_owned());
        }
        if context.project_trusted && !self.project_trust.trusted {
            return Err("AUTH_CALLER_UNTRUSTED".to_owned());
        }
        Ok(())
    }

    pub fn require_trusted_for_effect(&self) -> Result<(), String> {
        self.validate()?;
        self.project_trust
            .trusted
            .then_some(())
            .ok_or_else(|| "AUTH_PROJECT_UNTRUSTED".to_owned())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "security_context_id": self.security_context_id,
            "principal": self.principal,
            "project_trust": self.project_trust,
            "assignment": self.assignment,
            "department": self.department,
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
