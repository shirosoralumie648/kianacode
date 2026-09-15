//! Immutable server-derived execution scope.
//!
//! An ExecutionScope is an input snapshot for policy, approval, Broker and handler adapters. It
//! carries no authority by itself: only a committed permit can authorize an effect. Fields are
//! intentionally explicit so a model/tool payload cannot silently widen a missing dimension.

use crate::{
    canonical_journal_bytes, json_digest, AuthenticatedPrincipalRef, BudgetLeaseId,
    CapabilityGrantId, CapabilityRequest, CellId, ProjectIdentity, RunId, SchemaVersion, ScopeSet,
    SessionId, TurnId,
};
use serde::{Deserialize, Serialize};

pub const EXECUTION_SCOPE_SCHEMA: &str = "kiana.execution-scope.v1";
pub const EXECUTION_SCOPE_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EXECUTION_SCOPE_PATHS: usize = 256;
pub const MAX_EXECUTION_SCOPE_REFS: usize = 128;

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded_list(values: &[String], field: &str) -> Result<(), String> {
    if values.len() > MAX_EXECUTION_SCOPE_PATHS
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 4_096 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionScope {
    pub schema: String,
    pub version: SchemaVersion,
    pub principal: AuthenticatedPrincipalRef,
    pub project: ProjectIdentity,
    pub session_id: SessionId,
    #[serde(default)]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    #[serde(default)]
    pub cell_id: Option<CellId>,
    #[serde(default)]
    pub grant_refs: Vec<CapabilityGrantId>,
    #[serde(default)]
    pub budget_lease_id: Option<BudgetLeaseId>,
    #[serde(default)]
    pub work_packet_id: Option<String>,
    pub environment_id: String,
    #[serde(default)]
    pub workspace_revision: Option<String>,
    pub permission_scope: ScopeSet,
    #[serde(default)]
    pub read_roots: Vec<String>,
    #[serde(default)]
    pub write_roots: Vec<String>,
    #[serde(default)]
    pub read_denies: Vec<String>,
    #[serde(default)]
    pub write_denies: Vec<String>,
    #[serde(default)]
    pub memory_scopes: Vec<String>,
    #[serde(default)]
    pub server_scopes: Vec<String>,
    #[serde(default)]
    pub network_policy: Vec<String>,
    pub authority_epoch: u64,
    pub trust_revision: String,
    pub data_epoch: u64,
    pub cancellation_epoch: u64,
    pub deadline_unix_ms: u64,
    pub fencing_token: u64,
    pub catalog_digest: String,
    pub action_digest: String,
    pub permission_scope_digest: String,
    pub scope_digest: String,
}

impl ExecutionScope {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXECUTION_SCOPE_SCHEMA
            || !self
                .version
                .is_compatible_with(&EXECUTION_SCOPE_SCHEMA_VERSION)
            || self.session_id.is_empty()
            || self.environment_id.trim().is_empty()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.cancellation_epoch == 0
            || self.deadline_unix_ms == 0
            || self.fencing_token == 0
        {
            return Err("execution_scope_header_invalid".to_owned());
        }
        self.principal.validate()?;
        self.project.validate()?;
        self.permission_scope.validate()?;
        if [
            &self.permission_scope.operations,
            &self.permission_scope.paths,
            &self.permission_scope.namespaces,
            &self.permission_scope.network,
        ]
        .iter()
        .any(|dimension| {
            matches!(dimension, crate::ScopeDimension::Restricted(values) if values.is_empty())
        })
        {
            return Err("execution_scope_empty".to_owned());
        }
        if self.run_id.is_none() && self.turn_id.is_some() {
            return Err("execution_scope_turn_requires_run".to_owned());
        }
        if self.grant_refs.len() > MAX_EXECUTION_SCOPE_REFS
            || self.grant_refs.windows(2).any(|pair| pair[0] == pair[1])
        {
            return Err("execution_scope_grants_invalid".to_owned());
        }
        for (values, field) in [
            (&self.read_roots, "execution_scope_read_roots"),
            (&self.write_roots, "execution_scope_write_roots"),
            (&self.read_denies, "execution_scope_read_denies"),
            (&self.write_denies, "execution_scope_write_denies"),
            (&self.memory_scopes, "execution_scope_memory_scopes"),
            (&self.server_scopes, "execution_scope_server_scopes"),
            (&self.network_policy, "execution_scope_network_policy"),
        ] {
            bounded_list(values, field)?;
        }
        if self
            .work_packet_id
            .as_deref()
            .is_some_and(|id| id.trim().is_empty() || id.len() > 256)
        {
            return Err("execution_scope_work_packet_invalid".to_owned());
        }
        for (value, field) in [
            (&self.trust_revision, "execution_scope_trust_revision"),
            (&self.catalog_digest, "execution_scope_catalog_digest"),
            (&self.action_digest, "execution_scope_action_digest"),
            (
                &self.permission_scope_digest,
                "execution_scope_permission_scope_digest",
            ),
            (&self.scope_digest, "execution_scope_scope_digest"),
        ] {
            digest(value, field)?;
        }
        if self.permission_scope_digest != self.permission_scope.digest() {
            return Err("execution_scope_permission_scope_digest_mismatch".to_owned());
        }
        if self.scope_digest != self.digest() {
            return Err("execution_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// Verify the scope against the normalized request that carries it. The request's optional
    /// scope is cleared while computing the action digest to avoid a self-referential hash.
    pub fn validate_for_request(&self, request: &CapabilityRequest) -> Result<(), String> {
        self.validate()?;
        if self.catalog_digest != crate::capability_action_catalog_digest() {
            return Err("execution_scope_catalog_changed".to_owned());
        }
        let mut without_scope = request.clone();
        without_scope.execution_scope = None;
        if self.action_digest != crate::capability_action_digest(&without_scope) {
            return Err("execution_scope_action_digest_mismatch".to_owned());
        }
        if self
            .grant_refs
            .first()
            .is_some_and(|grant| Some(*grant) != request.capability_grant_id)
            || self.budget_lease_id != request.budget_lease_id
        {
            return Err("execution_scope_resource_mismatch".to_owned());
        }
        if let Some(run_id) = request
            .arguments
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .and_then(RunId::parse_str)
        {
            if self.run_id != Some(run_id) {
                return Err("execution_scope_run_mismatch".to_owned());
            }
        }
        if let Some(turn_id) = request
            .arguments
            .get("turn_id")
            .and_then(|value| serde_json::from_value::<TurnId>(value.clone()).ok())
        {
            if self.turn_id != Some(turn_id) {
                return Err("execution_scope_turn_mismatch".to_owned());
            }
        }
        Ok(())
    }

    pub fn is_subset_of(&self, parent: &Self) -> Result<bool, String> {
        self.validate()?;
        parent.validate()?;
        Ok(self.principal.principal_id == parent.principal.principal_id
            && self.project.project_id == parent.project.project_id
            && self.session_id == parent.session_id
            && self
                .permission_scope
                .is_subset_of(&parent.permission_scope)?
            && self.deadline_unix_ms <= parent.deadline_unix_ms
            && self.authority_epoch == parent.authority_epoch
            && self.data_epoch == parent.data_epoch
            && self.cancellation_epoch >= parent.cancellation_epoch
            && self.fencing_token >= parent.fencing_token
            && self
                .grant_refs
                .iter()
                .all(|grant| parent.grant_refs.iter().any(|candidate| candidate == grant)))
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scope_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
