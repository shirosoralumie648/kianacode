//! Stable Company organization/project/workspace scope bindings.
//!
//! A filesystem path is evidence for a workspace binding, not an OrganizationId or ProjectId.
//! The registry is a pure authority snapshot used by adapters; durable Company events remain the
//! source of truth for business transitions.

use crate::{json_digest, OrganizationId, ProjectId, WorkspaceId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use uuid::Uuid;

pub const COMPANY_SCOPE_SCHEMA: &str = "kiana.company-scope.v1";
pub const COMPANY_ORGANIZATION_BINDING_SCHEMA: &str = "kiana.company-organization-binding.v1";
pub const COMPANY_WORKSPACE_BINDING_SCHEMA: &str = "kiana.company-workspace-binding.v1";
pub const COMPANY_PROJECT_BINDING_SCHEMA: &str = "kiana.company-project-binding.v1";

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

fn stable_workspace_id(canonical_root: &str) -> WorkspaceId {
    let hash = Sha256::digest(canonical_root.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    WorkspaceId::from_uuid(Uuid::from_bytes(bytes))
}

fn valid_canonical_root(value: &str) -> bool {
    Path::new(value).is_absolute()
        && !Path::new(value)
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrganizationBinding {
    pub schema: String,
    pub organization_id: OrganizationId,
    pub name: String,
    pub owner_principal_id: String,
    #[serde(default)]
    pub members: BTreeSet<String>,
    pub revision: u64,
}

impl OrganizationBinding {
    pub fn new(
        organization_id: OrganizationId,
        name: impl Into<String>,
        owner_principal_id: impl Into<String>,
        revision: u64,
    ) -> Result<Self, String> {
        let owner_principal_id = owner_principal_id.into();
        let mut members = BTreeSet::new();
        members.insert(owner_principal_id.clone());
        let binding = Self {
            schema: COMPANY_ORGANIZATION_BINDING_SCHEMA.to_owned(),
            organization_id,
            name: name.into(),
            owner_principal_id,
            members,
            revision,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_ORGANIZATION_BINDING_SCHEMA || self.revision == 0 {
            return Err("company_organization_binding_invalid".to_owned());
        }
        required(&self.name, "company_organization_name", 256)?;
        required(&self.owner_principal_id, "company_organization_owner", 256)?;
        if self.members.is_empty()
            || self.members.len() > 256
            || self.members.iter().any(|member| {
                member.trim().is_empty() || member.len() > 256 || member.contains('\0')
            })
            || !self.members.contains(&self.owner_principal_id)
        {
            return Err("company_organization_members_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceBinding {
    pub schema: String,
    pub workspace_id: WorkspaceId,
    pub organization_id: OrganizationId,
    pub canonical_root_digest: String,
    pub trust_revision: String,
    pub revision: u64,
}

impl WorkspaceBinding {
    pub fn new(
        organization_id: OrganizationId,
        canonical_root: &str,
        trust_revision: &str,
        revision: u64,
    ) -> Result<Self, String> {
        if !valid_canonical_root(canonical_root) {
            return Err("company_workspace_root_invalid".to_owned());
        }
        required(trust_revision, "company_workspace_trust_revision", 256)?;
        let binding = Self {
            schema: COMPANY_WORKSPACE_BINDING_SCHEMA.to_owned(),
            workspace_id: stable_workspace_id(canonical_root),
            organization_id,
            canonical_root_digest: json_digest(&json!({"canonical_root": canonical_root})),
            trust_revision: json_digest(&json!({"trust_revision": trust_revision})),
            revision,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_WORKSPACE_BINDING_SCHEMA || self.revision == 0 {
            return Err("company_workspace_binding_invalid".to_owned());
        }
        digest(&self.canonical_root_digest, "company_workspace_root_digest")?;
        digest(&self.trust_revision, "company_workspace_trust_revision")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectBinding {
    pub schema: String,
    pub project_id: ProjectId,
    pub organization_id: OrganizationId,
    pub workspace_id: WorkspaceId,
    pub owner_principal_id: String,
    pub revision: u64,
}

impl ProjectBinding {
    pub fn new(
        project_id: ProjectId,
        organization_id: OrganizationId,
        workspace_id: WorkspaceId,
        owner_principal_id: impl Into<String>,
        revision: u64,
    ) -> Result<Self, String> {
        let binding = Self {
            schema: COMPANY_PROJECT_BINDING_SCHEMA.to_owned(),
            project_id,
            organization_id,
            workspace_id,
            owner_principal_id: owner_principal_id.into(),
            revision,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_PROJECT_BINDING_SCHEMA || self.revision == 0 {
            return Err("company_project_binding_invalid".to_owned());
        }
        required(&self.owner_principal_id, "company_project_owner", 256)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyScope {
    pub schema: String,
    pub principal_id: String,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub workspace_id: WorkspaceId,
    pub canonical_root_digest: String,
    pub trust_revision: String,
    pub scope_digest: String,
}

impl CompanyScope {
    fn from_bindings(
        principal_id: impl Into<String>,
        organization: &OrganizationBinding,
        workspace: &WorkspaceBinding,
        project: &ProjectBinding,
    ) -> Result<Self, String> {
        let mut scope = Self {
            schema: COMPANY_SCOPE_SCHEMA.to_owned(),
            principal_id: principal_id.into(),
            organization_id: organization.organization_id,
            project_id: project.project_id,
            workspace_id: workspace.workspace_id,
            canonical_root_digest: workspace.canonical_root_digest.clone(),
            trust_revision: workspace.trust_revision.clone(),
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPANY_SCOPE_SCHEMA {
            return Err("company_scope_schema_invalid".to_owned());
        }
        required(&self.principal_id, "company_scope_principal", 256)?;
        digest(&self.canonical_root_digest, "company_scope_root_digest")?;
        digest(&self.trust_revision, "company_scope_trust_revision")?;
        digest(&self.scope_digest, "company_scope_digest")?;
        if self.scope_digest != self.digest() {
            return Err("company_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scope_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CompanyScopeRegistry {
    organizations: BTreeMap<String, OrganizationBinding>,
    workspaces: BTreeMap<String, WorkspaceBinding>,
    projects: BTreeMap<String, ProjectBinding>,
    legacy_streams: BTreeMap<String, CompanyScope>,
}

impl CompanyScopeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_organization(&mut self, binding: OrganizationBinding) -> Result<(), String> {
        binding.validate()?;
        let key = binding.organization_id.to_string();
        if self.organizations.contains_key(&key) {
            return Err("company_organization_duplicate".to_owned());
        }
        self.organizations.insert(key, binding);
        Ok(())
    }

    pub fn add_member(
        &mut self,
        organization_id: OrganizationId,
        principal_id: impl Into<String>,
    ) -> Result<(), String> {
        let principal_id = principal_id.into();
        required(&principal_id, "company_member_principal", 256)?;
        let organization = self
            .organizations
            .get_mut(&organization_id.to_string())
            .ok_or_else(|| "company_organization_missing".to_owned())?;
        organization.members.insert(principal_id);
        organization.revision = organization
            .revision
            .checked_add(1)
            .ok_or_else(|| "company_organization_revision_exhausted".to_owned())?;
        organization.validate()
    }

    pub fn bind_workspace(&mut self, binding: WorkspaceBinding) -> Result<(), String> {
        binding.validate()?;
        if !self
            .organizations
            .contains_key(&binding.organization_id.to_string())
        {
            return Err("company_organization_missing".to_owned());
        }
        let key = binding.workspace_id.to_string();
        if self.workspaces.contains_key(&key) {
            return Err("company_workspace_duplicate".to_owned());
        }
        self.workspaces.insert(key, binding);
        Ok(())
    }

    pub fn bind_project(&mut self, binding: ProjectBinding) -> Result<(), String> {
        binding.validate()?;
        let organization = self
            .organizations
            .get(&binding.organization_id.to_string())
            .ok_or_else(|| "company_organization_missing".to_owned())?;
        if !self
            .workspaces
            .get(&binding.workspace_id.to_string())
            .is_some_and(|workspace| workspace.organization_id == binding.organization_id)
        {
            return Err("company_workspace_organization_mismatch".to_owned());
        }
        if self.projects.contains_key(&binding.project_id.to_string()) {
            return Err("company_project_duplicate".to_owned());
        }
        if !organization.members.contains(&binding.owner_principal_id) {
            return Err("company_project_owner_not_member".to_owned());
        }
        self.projects
            .insert(binding.project_id.to_string(), binding);
        Ok(())
    }

    pub fn resolve(
        &self,
        principal_id: &str,
        organization_id: OrganizationId,
        project_id: ProjectId,
        workspace_id: WorkspaceId,
    ) -> Result<CompanyScope, String> {
        required(principal_id, "company_scope_principal", 256)?;
        let organization = self
            .organizations
            .get(&organization_id.to_string())
            .ok_or_else(|| "company_organization_missing".to_owned())?;
        if !organization.members.contains(principal_id) {
            return Err("company_scope_membership_denied".to_owned());
        }
        let workspace = self
            .workspaces
            .get(&workspace_id.to_string())
            .ok_or_else(|| "company_workspace_missing".to_owned())?;
        let project = self
            .projects
            .get(&project_id.to_string())
            .ok_or_else(|| "company_project_missing".to_owned())?;
        if workspace.organization_id != organization_id
            || project.organization_id != organization_id
            || project.workspace_id != workspace_id
        {
            return Err("company_scope_binding_mismatch".to_owned());
        }
        CompanyScope::from_bindings(principal_id, organization, workspace, project)
    }

    pub fn resolve_for_root(
        &self,
        principal_id: &str,
        organization_id: OrganizationId,
        project_id: ProjectId,
        canonical_root: &str,
    ) -> Result<CompanyScope, String> {
        if !valid_canonical_root(canonical_root) {
            return Err("company_workspace_root_invalid".to_owned());
        }
        let workspace_id = stable_workspace_id(canonical_root);
        self.resolve(principal_id, organization_id, project_id, workspace_id)
    }

    /// Import one legacy actor+canonical-root stream into a stable scope. Re-importing the same
    /// stream is idempotent; mapping the stream to another project is an ambiguity and is denied.
    pub fn import_legacy_stream(
        &mut self,
        principal_id: &str,
        organization_id: OrganizationId,
        project_id: ProjectId,
        canonical_root: &str,
    ) -> Result<CompanyScope, String> {
        let scope =
            self.resolve_for_root(principal_id, organization_id, project_id, canonical_root)?;
        let key = format!("{}\u{1f}{}", principal_id, scope.canonical_root_digest);
        if let Some(previous) = self.legacy_streams.get(&key) {
            if previous.project_id != project_id || previous.organization_id != organization_id {
                return Err("company_legacy_stream_ambiguous".to_owned());
            }
            return Ok(previous.clone());
        }
        self.legacy_streams.insert(key, scope.clone());
        Ok(scope)
    }

    pub fn organizations(&self) -> impl Iterator<Item = &OrganizationBinding> {
        self.organizations.values()
    }

    pub fn workspaces(&self) -> impl Iterator<Item = &WorkspaceBinding> {
        self.workspaces.values()
    }

    pub fn projects(&self) -> impl Iterator<Item = &ProjectBinding> {
        self.projects.values()
    }
}
