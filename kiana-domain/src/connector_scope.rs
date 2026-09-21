//! Connector account/project/data scope intersection contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CONNECTOR_SCOPE_SCHEMA: &str = "kiana.connector-scope.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorBindingStatus {
    Active,
    Revoked,
    Expired,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorScopeBinding {
    pub schema: String,
    pub binding_id: String,
    pub owner_id: String,
    pub project_root: String,
    pub read_scopes: BTreeSet<String>,
    pub write_scopes: BTreeSet<String>,
    pub data_classes: BTreeSet<String>,
    pub data_epoch: u64,
    pub expires_at_unix_ms: u64,
    pub status: ConnectorBindingStatus,
    pub revision: u64,
    pub scope_digest: String,
}

impl ConnectorScopeBinding {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_SCOPE_SCHEMA
            || self.binding_id.trim().is_empty()
            || self.owner_id.trim().is_empty()
            || self.project_root.trim().is_empty()
            || self.data_epoch == 0
            || self.expires_at_unix_ms == 0
            || self.revision == 0
            || self.read_scopes.is_empty()
        {
            return Err("connector_scope_binding_header_invalid".to_owned());
        }
        if self
            .write_scopes
            .iter()
            .any(|scope| !self.read_scopes.contains(scope))
        {
            return Err("connector_scope_write_not_in_read".to_owned());
        }
        if self.read_scopes.iter().any(|scope| scope.trim().is_empty())
            || self
                .data_classes
                .iter()
                .any(|class| class.trim().is_empty())
        {
            return Err("connector_scope_value_invalid".to_owned());
        }
        let Some(hex) = self.scope_digest.strip_prefix("sha256:") else {
            return Err("connector_scope_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("connector_scope_digest_invalid".to_owned());
        }
        if self.scope_digest != self.digest() {
            return Err("connector_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn intersect(&self, requested: &Self) -> Result<Self, String> {
        self.validate()?;
        requested.validate()?;
        if self.binding_id != requested.binding_id
            || self.owner_id != requested.owner_id
            || self.project_root != requested.project_root
            || self.data_epoch != requested.data_epoch
            || self.status != ConnectorBindingStatus::Active
            || requested.status != ConnectorBindingStatus::Active
        {
            return Err("connector_scope_identity_or_epoch_mismatch".to_owned());
        }
        if !requested.read_scopes.is_subset(&self.read_scopes)
            || !requested.write_scopes.is_subset(&self.write_scopes)
            || !requested.data_classes.is_subset(&self.data_classes)
            || requested.expires_at_unix_ms > self.expires_at_unix_ms
            || requested.revision < self.revision
        {
            return Err("connector_scope_widening_denied".to_owned());
        }
        Ok(requested.clone())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "binding_id": self.binding_id,
            "owner_id": self.owner_id,
            "project_root": self.project_root,
            "read_scopes": self.read_scopes,
            "write_scopes": self.write_scopes,
            "data_classes": self.data_classes,
            "data_epoch": self.data_epoch,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "status": self.status,
            "revision": self.revision,
        }))
    }
}
