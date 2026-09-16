//! Server-owned role and project assignment contracts.
//!
//! RoleSpec is a static template; these assignments bind it to an authenticated principal and a
//! bounded organization/project scope. They are intentionally pure values here. Durable storage,
//! expiry/revocation events and authority-epoch persistence belong to the identity adapter.

use crate::{
    json_digest, AssignmentId, AuthenticatedPrincipalRef, OrganizationId, ProjectAssignmentId,
    ProjectId, RoleSpec,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const ROLE_ASSIGNMENT_SCHEMA: &str = "kiana.role-assignment.v1";
pub const PROJECT_ASSIGNMENT_SCHEMA: &str = "kiana.project-assignment.v1";
pub const RESOLVED_ASSIGNMENT_SCHEMA: &str = "kiana.resolved-assignment.v1";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn one_or_more<T>(values: &[T], field: &str, max: usize) -> Result<(), String> {
    if values.is_empty() || values.len() > max {
        return Err(field.to_owned());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleAssignmentStatus {
    Requested,
    Approved,
    Active,
    Reduced,
    Suspended,
    Revoked,
    Expired,
}

impl RoleAssignmentStatus {
    pub fn usable(self) -> bool {
        matches!(self, Self::Active)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleAssignment {
    pub schema: String,
    pub assignment_id: AssignmentId,
    pub principal: AuthenticatedPrincipalRef,
    pub organization_id: OrganizationId,
    pub role_id: String,
    pub department_id: String,
    pub project_ids: Vec<ProjectId>,
    pub valid_from_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub status: RoleAssignmentStatus,
    pub revision: u64,
    pub authority_epoch: u64,
    pub assignment_digest: String,
}

impl RoleAssignment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        assignment_id: AssignmentId,
        principal: AuthenticatedPrincipalRef,
        organization_id: OrganizationId,
        role_id: impl Into<String>,
        department_id: impl Into<String>,
        project_ids: Vec<ProjectId>,
        valid_from_unix_ms: u64,
        expires_at_unix_ms: u64,
        status: RoleAssignmentStatus,
        revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut assignment = Self {
            schema: ROLE_ASSIGNMENT_SCHEMA.to_owned(),
            assignment_id,
            principal,
            organization_id,
            role_id: role_id.into(),
            department_id: department_id.into(),
            project_ids,
            valid_from_unix_ms,
            expires_at_unix_ms,
            status,
            revision,
            authority_epoch,
            assignment_digest: String::new(),
        };
        assignment.assignment_digest = assignment.digest();
        assignment.validate()?;
        Ok(assignment)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROLE_ASSIGNMENT_SCHEMA
            || self.revision == 0
            || self.authority_epoch == 0
            || self.valid_from_unix_ms == 0
            || self.expires_at_unix_ms <= self.valid_from_unix_ms
        {
            return Err("role_assignment_header_invalid".to_owned());
        }
        self.principal.validate()?;
        required(&self.role_id, "role_assignment_role", 128)?;
        required(&self.department_id, "role_assignment_department", 128)?;
        let role = RoleSpec::lookup(&self.role_id)
            .ok_or_else(|| "role_assignment_role_unknown".to_owned())?;
        if role.department_id != self.department_id {
            return Err("role_assignment_department_mismatch".to_owned());
        }
        one_or_more(&self.project_ids, "role_assignment_projects_required", 256)?;
        if self
            .project_ids
            .windows(2)
            .any(|pair| pair[0].to_string() >= pair[1].to_string())
        {
            return Err("role_assignment_projects_noncanonical".to_owned());
        }
        let Some(hex) = self.assignment_digest.strip_prefix("sha256:") else {
            return Err("role_assignment_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("role_assignment_digest_invalid".to_owned());
        }
        if self.assignment_digest != self.digest() {
            return Err("role_assignment_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, project_id: ProjectId, now_unix_ms: u64) -> bool {
        self.status.usable()
            && now_unix_ms >= self.valid_from_unix_ms
            && now_unix_ms < self.expires_at_unix_ms
            && self.project_ids.contains(&project_id)
    }

    pub fn revoke(&self) -> Result<Self, String> {
        let mut next = self.clone();
        next.status = RoleAssignmentStatus::Revoked;
        next.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "role_assignment_revision_exhausted".to_owned())?;
        next.authority_epoch = self
            .authority_epoch
            .checked_add(1)
            .ok_or_else(|| "role_assignment_epoch_exhausted".to_owned())?;
        next.assignment_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "assignment_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectAssignment {
    pub schema: String,
    pub project_assignment_id: ProjectAssignmentId,
    pub role_assignment_id: AssignmentId,
    pub principal: AuthenticatedPrincipalRef,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub valid_from_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub revoked: bool,
    pub revision: u64,
    pub authority_epoch: u64,
    pub assignment_digest: String,
}

impl ProjectAssignment {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_assignment_id: ProjectAssignmentId,
        role_assignment_id: AssignmentId,
        principal: AuthenticatedPrincipalRef,
        organization_id: OrganizationId,
        project_id: ProjectId,
        valid_from_unix_ms: u64,
        expires_at_unix_ms: u64,
        revoked: bool,
        revision: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut assignment = Self {
            schema: PROJECT_ASSIGNMENT_SCHEMA.to_owned(),
            project_assignment_id,
            role_assignment_id,
            principal,
            organization_id,
            project_id,
            valid_from_unix_ms,
            expires_at_unix_ms,
            revoked,
            revision,
            authority_epoch,
            assignment_digest: String::new(),
        };
        assignment.assignment_digest = assignment.digest();
        assignment.validate()?;
        Ok(assignment)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROJECT_ASSIGNMENT_SCHEMA
            || self.revision == 0
            || self.authority_epoch == 0
            || self.valid_from_unix_ms == 0
            || self.expires_at_unix_ms <= self.valid_from_unix_ms
        {
            return Err("project_assignment_header_invalid".to_owned());
        }
        self.principal.validate()?;
        let Some(hex) = self.assignment_digest.strip_prefix("sha256:") else {
            return Err("project_assignment_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("project_assignment_digest_invalid".to_owned());
        }
        if self.assignment_digest != self.digest() {
            return Err("project_assignment_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        !self.revoked
            && now_unix_ms >= self.valid_from_unix_ms
            && now_unix_ms < self.expires_at_unix_ms
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "assignment_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedAssignment {
    pub schema: String,
    pub assignment_id: AssignmentId,
    pub project_assignment_id: ProjectAssignmentId,
    pub principal: AuthenticatedPrincipalRef,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub role_id: String,
    pub department_id: String,
    pub valid_from_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub assignment_revision: u64,
    pub authority_epoch: u64,
    pub resolved_at_unix_ms: u64,
    pub resolution_digest: String,
}

impl ResolvedAssignment {
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "assignment": self.assignment_id,
            "project_assignment": self.project_assignment_id,
            "principal": self.principal.principal_digest,
            "organization_id": self.organization_id,
            "project_id": self.project_id,
            "role_id": self.role_id,
            "department_id": self.department_id,
            "valid_from_unix_ms": self.valid_from_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "assignment_revision": self.assignment_revision,
            "authority_epoch": self.authority_epoch,
            "resolved_at_unix_ms": self.resolved_at_unix_ms,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RESOLVED_ASSIGNMENT_SCHEMA
            || self.assignment_revision == 0
            || self.authority_epoch == 0
            || self.resolved_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.valid_from_unix_ms
        {
            return Err("resolved_assignment_header_invalid".to_owned());
        }
        self.principal.validate()?;
        required(&self.role_id, "resolved_assignment_role", 128)?;
        required(&self.department_id, "resolved_assignment_department", 128)?;
        let Some(hex) = self.resolution_digest.strip_prefix("sha256:") else {
            return Err("resolved_assignment_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("resolved_assignment_digest_invalid".to_owned());
        }
        if RoleSpec::lookup(&self.role_id)
            .is_none_or(|role| role.department_id != self.department_id)
        {
            return Err("resolved_assignment_role_invalid".to_owned());
        }
        if self.resolution_digest != self.digest() {
            return Err("resolved_assignment_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct AssignmentDirectory {
    roles: BTreeMap<String, RoleAssignment>,
    projects: BTreeMap<String, ProjectAssignment>,
}

impl AssignmentDirectory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_role(&mut self, assignment: RoleAssignment) -> Result<(), String> {
        assignment.validate()?;
        let key = assignment.assignment_id.to_string();
        if self.roles.contains_key(&key) {
            return Err("role_assignment_duplicate".to_owned());
        }
        self.roles.insert(key, assignment);
        Ok(())
    }

    pub fn register_project(&mut self, assignment: ProjectAssignment) -> Result<(), String> {
        assignment.validate()?;
        let role = self
            .roles
            .get(&assignment.role_assignment_id.to_string())
            .ok_or_else(|| "role_assignment_missing".to_owned())?;
        if role.principal != assignment.principal
            || role.organization_id != assignment.organization_id
            || !role.project_ids.contains(&assignment.project_id)
        {
            return Err("project_assignment_binding_mismatch".to_owned());
        }
        if assignment.valid_from_unix_ms < role.valid_from_unix_ms
            || assignment.expires_at_unix_ms > role.expires_at_unix_ms
        {
            return Err("project_assignment_window_mismatch".to_owned());
        }
        let key = assignment.project_assignment_id.to_string();
        if self.projects.contains_key(&key) {
            return Err("project_assignment_duplicate".to_owned());
        }
        self.projects.insert(key, assignment);
        Ok(())
    }

    pub fn resolve(
        &self,
        principal: &AuthenticatedPrincipalRef,
        organization_id: OrganizationId,
        project_id: ProjectId,
        role_id: &str,
        now_unix_ms: u64,
    ) -> Result<ResolvedAssignment, String> {
        principal.validate()?;
        let mut matches = self.projects.values().filter_map(|project| {
            if project.organization_id != organization_id
                || project.project_id != project_id
                || project.principal != *principal
                || !project.active_at(now_unix_ms)
            {
                return None;
            }
            let role = self.roles.get(&project.role_assignment_id.to_string())?;
            role.active_at(project_id, now_unix_ms)
                .then(|| (role, project))
        });
        let (role, project) = matches
            .next()
            .ok_or_else(|| "assignment_expired_or_missing".to_owned())?;
        if matches.next().is_some() {
            return Err("assignment_ambiguous".to_owned());
        }
        if role.role_id != role_id {
            return Err("assignment_role_mismatch".to_owned());
        }
        let resolved_at_unix_ms = now_unix_ms;
        let resolved = ResolvedAssignment {
            schema: RESOLVED_ASSIGNMENT_SCHEMA.to_owned(),
            assignment_id: role.assignment_id,
            project_assignment_id: project.project_assignment_id,
            principal: principal.clone(),
            organization_id,
            project_id,
            role_id: role.role_id.clone(),
            department_id: role.department_id.clone(),
            valid_from_unix_ms: role.valid_from_unix_ms.max(project.valid_from_unix_ms),
            expires_at_unix_ms: role.expires_at_unix_ms.min(project.expires_at_unix_ms),
            assignment_revision: role.revision.max(project.revision),
            authority_epoch: role.authority_epoch.max(project.authority_epoch),
            resolved_at_unix_ms,
            resolution_digest: String::new(),
        };
        let mut resolved = resolved;
        resolved.resolution_digest = resolved.digest();
        resolved.validate()?;
        Ok(resolved)
    }

    pub fn revoke_role(&mut self, assignment_id: AssignmentId) -> Result<RoleAssignment, String> {
        let role = self
            .roles
            .get_mut(&assignment_id.to_string())
            .ok_or_else(|| "role_assignment_missing".to_owned())?;
        let next = role.revoke()?;
        *role = next.clone();
        for project in self.projects.values_mut() {
            if project.role_assignment_id == assignment_id {
                project.revoked = true;
                project.revision = project
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| "project_assignment_revision_exhausted".to_owned())?;
                project.authority_epoch = next.authority_epoch;
                project.assignment_digest = project.digest();
            }
        }
        Ok(next)
    }

    pub fn role_assignments(&self) -> impl Iterator<Item = &RoleAssignment> {
        self.roles.values()
    }

    pub fn project_assignments(&self) -> impl Iterator<Item = &ProjectAssignment> {
        self.projects.values()
    }
}
