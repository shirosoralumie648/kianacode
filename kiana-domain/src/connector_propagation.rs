//! INT-26 connector data-class/purpose/retention/revocation propagation contract.
//!
//! Connector output remains scoped data. This fact binds the source/target project, data class,
//! purpose, SharingGrant/retention digests and data epoch, then records whether derived
//! Memory/Index/Cache views are invalidated. Revocation never silently leaves a derived view
//! usable and this module does not write any store.

use crate::json_digest;
use serde::{Deserialize, Serialize};

pub const CONNECTOR_PROPAGATION_SCHEMA: &str = "kiana.connector-propagation.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorPropagationState {
    Active,
    Revoked,
    Expired,
    Quarantined,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorPropagationFact {
    pub schema: String,
    pub connector_id: String,
    pub binding_id: String,
    pub account_id: String,
    pub source_project: String,
    pub target_project: String,
    pub object_ref: String,
    pub data_class: String,
    pub purpose: String,
    pub scope_digest: String,
    pub sharing_grant_digest: Option<String>,
    pub retention_policy_digest: String,
    pub source_cursor: u64,
    pub data_epoch: u64,
    pub state: ConnectorPropagationState,
    pub tombstone_epoch: Option<u64>,
    pub memory_invalidated: bool,
    pub index_invalidated: bool,
    pub cache_invalidated: bool,
    pub propagation_digest: String,
}

impl ConnectorPropagationFact {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CONNECTOR_PROPAGATION_SCHEMA
            || !valid_text(&self.connector_id)
            || !valid_text(&self.binding_id)
            || !valid_text(&self.account_id)
            || !valid_text(&self.source_project)
            || !valid_text(&self.target_project)
            || !valid_text(&self.object_ref)
            || !valid_text(&self.data_class)
            || !valid_text(&self.purpose)
            || !valid_digest(&self.scope_digest)
            || self
                .sharing_grant_digest
                .as_deref()
                .is_some_and(|value| !valid_digest(value))
            || !valid_digest(&self.retention_policy_digest)
            || self.source_cursor == 0
            || self.data_epoch == 0
            || !valid_tombstone(self.tombstone_epoch, self.data_epoch)
            || !valid_digest(&self.propagation_digest)
            || self.propagation_digest != self.digest()
        {
            return Err("connector_propagation_fact_invalid");
        }
        if self.source_project != self.target_project && self.sharing_grant_digest.is_none() {
            return Err("connector_propagation_sharing_grant_required");
        }
        match self.state {
            ConnectorPropagationState::Active => {
                if self.tombstone_epoch.is_some()
                    || self.memory_invalidated
                    || self.index_invalidated
                    || self.cache_invalidated
                {
                    return Err("connector_propagation_active_invalidation_invalid");
                }
            }
            ConnectorPropagationState::Revoked
            | ConnectorPropagationState::Expired
            | ConnectorPropagationState::Quarantined => {
                if self.tombstone_epoch.is_none()
                    || !self.memory_invalidated
                    || !self.index_invalidated
                    || !self.cache_invalidated
                {
                    return Err("connector_propagation_derived_view_not_invalidated");
                }
            }
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "connector_id": self.connector_id,
            "binding_id": self.binding_id,
            "account_id": self.account_id,
            "source_project": self.source_project,
            "target_project": self.target_project,
            "object_ref": self.object_ref,
            "data_class": self.data_class,
            "purpose": self.purpose,
            "scope_digest": self.scope_digest,
            "sharing_grant_digest": self.sharing_grant_digest,
            "retention_policy_digest": self.retention_policy_digest,
            "source_cursor": self.source_cursor,
            "data_epoch": self.data_epoch,
            "state": self.state,
            "tombstone_epoch": self.tombstone_epoch,
            "memory_invalidated": self.memory_invalidated,
            "index_invalidated": self.index_invalidated,
            "cache_invalidated": self.cache_invalidated,
        }))
    }
}

pub fn validate_connector_propagation(fact: &ConnectorPropagationFact) -> Result<(), &'static str> {
    fact.validate()
}

fn valid_tombstone(tombstone_epoch: Option<u64>, data_epoch: u64) -> bool {
    tombstone_epoch.is_none_or(|epoch| epoch >= data_epoch && epoch != 0)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
