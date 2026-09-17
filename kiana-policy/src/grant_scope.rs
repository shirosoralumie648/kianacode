//! Monotonic capability grant scopes for parent/child Cell inheritance.
//!
//! A `GrantScope` is a policy value, not an authorization token. Child scopes are formed only by
//! intersecting server-owned layers; capability, secret and external-effect dimensions are
//! independent so a broad read scope cannot accidentally become a write/network/secret grant.

use kiana_domain::{
    json_digest, CapabilityGrant, CapabilityKind, CapabilityRequest, GrantId, PrincipalId,
    ProjectId, RiskLevel, SchemaVersion, ScopeDimension, ScopeLimit, ScopeSet,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub const GRANT_SCOPE_SCHEMA: &str = "kiana.grant-scope.v1";
pub const GRANT_SCOPE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_GRANT_CAPABILITIES: usize = 16;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantScope {
    pub schema: String,
    pub version: SchemaVersion,
    pub grant_id: GrantId,
    #[serde(default)]
    pub parent_grant_id: Option<GrantId>,
    pub principal_id: PrincipalId,
    pub project_id: ProjectId,
    pub scope: ScopeSet,
    pub capabilities: Vec<CapabilityKind>,
    pub allow_secret: bool,
    pub allow_external: bool,
    pub delegation_allowed: bool,
    pub authority_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub grant_digest: String,
}

impl GrantScope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grant_id: GrantId,
        parent_grant_id: Option<GrantId>,
        principal_id: PrincipalId,
        project_id: ProjectId,
        scope: ScopeSet,
        mut capabilities: Vec<CapabilityKind>,
        allow_secret: bool,
        allow_external: bool,
        delegation_allowed: bool,
        authority_epoch: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        capabilities.sort_by_key(capability_key);
        let mut grant = Self {
            schema: GRANT_SCOPE_SCHEMA.to_owned(),
            version: GRANT_SCOPE_VERSION,
            grant_id,
            parent_grant_id,
            principal_id,
            project_id,
            scope,
            capabilities,
            allow_secret,
            allow_external,
            delegation_allowed,
            authority_epoch,
            expires_at_unix_ms,
            grant_digest: String::new(),
        };
        grant.grant_digest = grant.digest();
        grant.validate()?;
        Ok(grant)
    }

    /// Adapt the historical CapabilityGrant into an explicit scope without widening it.
    pub fn from_capability_grant(
        grant: &CapabilityGrant,
        principal_id: PrincipalId,
        project_id: ProjectId,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        grant.validate().map_err(|error| error.to_owned())?;
        let paths = if grant.paths.is_empty() {
            ScopeDimension::NotApplicable
        } else {
            ScopeDimension::Restricted(grant.paths.clone())
        };
        let scope = ScopeSet::new(
            ScopeDimension::Restricted(vec![grant.operation.clone()]),
            paths,
            ScopeDimension::NotApplicable,
            if grant.capability == CapabilityKind::Network {
                ScopeDimension::Restricted(vec!["network".to_owned()])
            } else {
                ScopeDimension::NotApplicable
            },
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )?;
        Self::new(
            GrantId::new(),
            None,
            principal_id,
            project_id,
            scope,
            vec![grant.capability.clone()],
            grant.capability == CapabilityKind::Secret,
            grant.capability == CapabilityKind::Network,
            grant.delegation_allowed,
            authority_epoch,
            grant.expires_at_unix_ms,
        )
    }

    /// Intersect two grant layers. The resulting grant is a fresh server-owned identity and can
    /// never be used to transfer a scope to another principal/project.
    pub fn intersect(&self, other: &Self) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        if self.principal_id != other.principal_id {
            return Err("grant_scope_principal_mismatch".to_owned());
        }
        if self.project_id != other.project_id {
            return Err("grant_scope_project_mismatch".to_owned());
        }
        if self.authority_epoch != other.authority_epoch {
            return Err("grant_scope_authority_epoch_mismatch".to_owned());
        }
        let scope = self.scope.intersect(&other.scope)?;
        let capabilities = self
            .capabilities
            .iter()
            .filter(|left| other.capabilities.iter().any(|right| right == *left))
            .cloned()
            .collect::<Vec<_>>();
        if capabilities.is_empty() {
            return Err("grant_scope_capability_intersection_empty".to_owned());
        }
        Self::new(
            GrantId::new(),
            Some(self.grant_id),
            self.principal_id,
            self.project_id,
            scope,
            capabilities,
            self.allow_secret && other.allow_secret,
            self.allow_external && other.allow_external,
            self.delegation_allowed && other.delegation_allowed,
            self.authority_epoch,
            self.expires_at_unix_ms.min(other.expires_at_unix_ms),
        )
    }

    pub fn intersect_all(layers: &[Self]) -> Result<Self, String> {
        let Some((first, rest)) = layers.split_first() else {
            return Err("grant_scope_layers_required".to_owned());
        };
        rest.iter()
            .try_fold(first.clone(), |current, next| current.intersect(next))
    }

    pub fn contains(&self, child: &Self) -> Result<bool, String> {
        self.validate()?;
        child.validate()?;
        if self.principal_id != child.principal_id || self.project_id != child.project_id {
            return Ok(false);
        }
        Ok(child.authority_epoch == self.authority_epoch
            && child.expires_at_unix_ms <= self.expires_at_unix_ms
            && (!child.delegation_allowed || self.delegation_allowed)
            && child
                .capabilities
                .iter()
                .all(|capability| self.capabilities.contains(capability))
            && (!child.allow_secret || self.allow_secret)
            && (!child.allow_external || self.allow_external)
            && child.scope.is_subset_of(&self.scope)?)
    }

    pub fn allows_request(
        &self,
        request: &CapabilityRequest,
        now_unix_ms: u64,
    ) -> Result<bool, String> {
        self.validate()?;
        if now_unix_ms >= self.expires_at_unix_ms
            || !self.capabilities.contains(&request.capability)
            || !self.scope.allows_operation(&request.operation)
        {
            return Ok(false);
        }
        if request.capability == CapabilityKind::Secret && !self.allow_secret {
            return Ok(false);
        }
        if matches!(
            request.risk,
            RiskLevel::ExternalSideEffect | RiskLevel::Critical
        ) && !self.allow_external
        {
            return Ok(false);
        }
        if let Some(path) = request.arguments.get("path").and_then(Value::as_str) {
            if !self.scope.allows_path(path) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != GRANT_SCOPE_SCHEMA
            || !self.version.is_compatible_with(&GRANT_SCOPE_VERSION)
            || self.grant_id.as_uuid().is_nil()
            || self.principal_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.expires_at_unix_ms == 0
            || self.capabilities.is_empty()
            || self.capabilities.len() > MAX_GRANT_CAPABILITIES
        {
            return Err("grant_scope_header_invalid".to_owned());
        }
        if self.parent_grant_id == Some(self.grant_id) {
            return Err("grant_scope_parent_self".to_owned());
        }
        self.scope.validate()?;
        let mut seen = BTreeSet::new();
        for capability in &self.capabilities {
            if !seen.insert(capability_key(capability)) {
                return Err("grant_scope_capabilities_duplicate".to_owned());
            }
            if *capability == CapabilityKind::Other(String::new()) {
                return Err("grant_scope_capability_unknown".to_owned());
            }
        }
        if self
            .capabilities
            .windows(2)
            .any(|pair| capability_key(&pair[0]) >= capability_key(&pair[1]))
        {
            return Err("grant_scope_capabilities_noncanonical".to_owned());
        }
        if self.allow_secret && !self.capabilities.contains(&CapabilityKind::Secret) {
            return Err("grant_scope_secret_dimension_mismatch".to_owned());
        }
        if self.allow_external && !self.capabilities.contains(&CapabilityKind::Network) {
            return Err("grant_scope_external_dimension_mismatch".to_owned());
        }
        validate_digest(&self.grant_digest, "grant_scope_digest")?;
        if self.grant_digest != self.digest() {
            return Err("grant_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let grant: Self = serde_json::from_value(value.clone())
            .map_err(|_| "grant_scope_decode_failed".to_owned())?;
        grant.validate()?;
        Ok(grant)
    }

    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "grant_scope_encode_failed".to_owned())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "grant_id": self.grant_id,
            "parent_grant_id": self.parent_grant_id,
            "principal_id": self.principal_id,
            "project_id": self.project_id,
            "scope": self.scope,
            "capabilities": self.capabilities,
            "allow_secret": self.allow_secret,
            "allow_external": self.allow_external,
            "delegation_allowed": self.delegation_allowed,
            "authority_epoch": self.authority_epoch,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

fn capability_key(capability: &CapabilityKind) -> String {
    match capability {
        CapabilityKind::Other(value) => format!("other:{value}"),
        _ => format!("{capability:?}").to_ascii_lowercase(),
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
