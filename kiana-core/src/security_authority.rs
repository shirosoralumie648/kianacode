//! Server-owned authority snapshot joining project trust, role assignment and department scope.
//!
//! This is a pure validation/value layer. It never treats a wire role/project/trust field as an
//! authority source and never dispatches a capability. Durable assignment/revocation remains an
//! adapter concern.

use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, DepartmentSnapshot, OrganizationId, ProjectId,
    ProjectIdentity, ProjectTrustSnapshot, RequestContext, ResolvedAssignment, SchemaVersion,
    SecurityContextId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SECURITY_AUTHORITY_SNAPSHOT_SCHEMA: &str = "kiana.security-authority-snapshot.v1";
pub const SECURITY_AUTHORITY_SNAPSHOT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

pub const SECURITY_AUTHORITY_PROVENANCE_SCHEMA: &str = "kiana.security-authority-provenance.v1";
pub const SECURITY_AUTHORITY_PROVENANCE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

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

    pub fn validate_request(
        &self,
        context: &RequestContext,
        project: &ProjectIdentity,
    ) -> Result<(), String> {
        self.validate()?;
        project
            .validate()
            .map_err(|_| "AUTH_PROJECT_MISMATCH".to_owned())?;
        let canonical_root_digest = json_digest(&json!({
            "canonical_root": project.canonical_root,
        }));
        if project.project_id != self.project_trust.project_id
            || canonical_root_digest != self.project_trust.canonical_root_digest
            || project.trust_revision != self.project_trust.trust_revision
            || (context.project_root != project.root
                && context.project_root != project.canonical_root)
        {
            return Err("AUTH_PROJECT_MISMATCH".to_owned());
        }
        if context.actor_id.as_deref() != Some(self.principal.principal_id.as_str()) {
            return Err("AUTH_CALLER_UNTRUSTED".to_owned());
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

/// Read-only, queryable provenance of the server-owned authority inputs of one request.
///
/// It answers "where did this trust decision, role and department scope come from, and which
/// revision did each carry?" without granting a role or capability, without fencing a session and
/// without dispatching. `role_source` is supplied by the resolving adapter (for the daemon that is
/// the daemon-owned assignment directory), so the label records the authority owner rather than a
/// wire value. A caller that wants to know why a request would be denied reads the same stable
/// reason codes from the normal request path; this projection never upgrades them to an allow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityAuthorityProvenance {
    pub schema: String,
    pub version: SchemaVersion,
    pub security_context_id: SecurityContextId,
    pub principal: AuthenticatedPrincipalRef,
    pub project_id: ProjectId,
    pub organization_id: OrganizationId,
    pub trust_source: String,
    pub trust_revision: String,
    pub trust_snapshot_revision: u64,
    pub trust_trusted: bool,
    pub role_source: String,
    pub role_id: String,
    pub role_revision: u64,
    pub department_id: String,
    pub department_revision: u64,
    pub authority_epoch: u64,
    pub provenance_digest: String,
}

impl SecurityAuthorityProvenance {
    /// Project the queryable source/revision fields out of a validated authority snapshot.
    pub fn from_authority(
        snapshot: &SecurityAuthoritySnapshot,
        role_source: impl Into<String>,
    ) -> Result<Self, String> {
        snapshot.validate()?;
        let mut provenance = Self {
            schema: SECURITY_AUTHORITY_PROVENANCE_SCHEMA.to_owned(),
            version: SECURITY_AUTHORITY_PROVENANCE_VERSION,
            security_context_id: snapshot.security_context_id,
            principal: snapshot.principal.clone(),
            project_id: snapshot.assignment.project_id,
            organization_id: snapshot.assignment.organization_id,
            trust_source: snapshot.project_trust.source.clone(),
            trust_revision: snapshot.project_trust.trust_revision.clone(),
            trust_snapshot_revision: snapshot.project_trust.revision,
            trust_trusted: snapshot.project_trust.trusted,
            role_source: role_source.into(),
            role_id: snapshot.assignment.role_id.clone(),
            role_revision: snapshot.assignment.assignment_revision,
            department_id: snapshot.department.department_id.clone(),
            department_revision: snapshot.department.revision,
            authority_epoch: snapshot.authority_epoch,
            provenance_digest: String::new(),
        };
        provenance.provenance_digest = provenance.digest();
        provenance.validate()?;
        Ok(provenance)
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let provenance: Self = serde_json::from_value(value.clone())
            .map_err(|_| "security_authority_provenance_decode_failed".to_owned())?;
        provenance.validate()?;
        Ok(provenance)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self)
            .map_err(|_| "security_authority_provenance_encode_failed".to_owned())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_AUTHORITY_PROVENANCE_SCHEMA
            || !self
                .version
                .is_compatible_with(&SECURITY_AUTHORITY_PROVENANCE_VERSION)
            || self.security_context_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.organization_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.trust_snapshot_revision == 0
            || self.role_revision == 0
            || self.department_revision == 0
            || self.trust_source.trim().is_empty()
            || self.trust_source.len() > 128
            || self.trust_source.contains('\0')
            || self.role_source.trim().is_empty()
            || self.role_source.len() > 128
            || self.role_source.contains('\0')
            || self.role_id.trim().is_empty()
            || self.department_id.trim().is_empty()
        {
            return Err("security_authority_provenance_header_invalid".to_owned());
        }
        self.principal
            .validate()
            .map_err(|_| "AUTH_PRINCIPAL_MISSING".to_owned())?;
        validate_digest(
            &self.trust_revision,
            "security_authority_provenance_trust_revision",
        )?;
        validate_digest(&self.provenance_digest, "security_authority_provenance_digest")?;
        if self.provenance_digest != self.digest() {
            return Err("security_authority_provenance_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "security_context_id": self.security_context_id,
            "principal": self.principal,
            "project_id": self.project_id,
            "organization_id": self.organization_id,
            "trust_source": self.trust_source,
            "trust_revision": self.trust_revision,
            "trust_snapshot_revision": self.trust_snapshot_revision,
            "trust_trusted": self.trust_trusted,
            "role_source": self.role_source,
            "role_id": self.role_id,
            "role_revision": self.role_revision,
            "department_id": self.department_id,
            "department_revision": self.department_revision,
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
