//! Read-only connector health projection.
//!
//! The projector consumes committed connector binding/health facts only. It never invokes an
//! adapter, resolves a credential or turns a health status into an authorization decision.

use kiana_domain::{
    ConnectorBindingSnapshot, ConnectorHealthFact, RuntimeEvent, CONNECTOR_HEALTH_EVENT_KIND,
    CONNECTOR_HEALTH_PROJECTION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONNECTOR_HEALTH_PROJECTION_VERSION: &str = "connector-health.v1";
const MAX_HEALTH_ENTRIES: usize = 256;
const MAX_HEALTH_LIMITATIONS: usize = 16;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHealthProjectionEntry {
    pub health: ConnectorHealthFact,
    pub source_cursor: u64,
    pub projection_version: String,
    pub binding_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_epoch: Option<u64>,
    pub stale: bool,
    pub proof_level: String,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealthProjectionEntry {
    pub fn validate(&self) -> Result<(), String> {
        self.health.validate()?;
        if self.source_cursor == 0
            || self.projection_version != CONNECTOR_HEALTH_PROJECTION_VERSION
            || self.binding_revision == 0
            || self.credential_generation == Some(0)
            || self.proof_level != "source"
            || self.limitations.len() > MAX_HEALTH_LIMITATIONS
            || self
                .limitations
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
        {
            return Err("connector_health_projection_entry_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHealthProjection {
    pub schema: String,
    pub source_cursor: u64,
    pub projection_version: String,
    pub entries: Vec<ConnectorHealthProjectionEntry>,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealthProjection {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_HEALTH_PROJECTION_SCHEMA
            || self.source_cursor == 0
            || self.projection_version != CONNECTOR_HEALTH_PROJECTION_VERSION
            || self.entries.len() > MAX_HEALTH_ENTRIES
            || self.limitations.len() > MAX_HEALTH_LIMITATIONS
        {
            return Err("connector_health_projection_invalid".to_owned());
        }
        let mut bindings = std::collections::BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !bindings.insert(entry.health.binding_id.clone()) {
                return Err("connector_health_projection_duplicate_binding".to_owned());
            }
        }
        if self
            .limitations
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
        {
            return Err("connector_health_projection_limitation_invalid".to_owned());
        }
        Ok(())
    }
}

/// Project the latest committed health fact per binding. Unknown events remain opaque to this
/// query; malformed health facts fail closed instead of being displayed as a healthy status.
pub fn project_connector_health(
    events: &[RuntimeEvent],
) -> Result<ConnectorHealthProjection, String> {
    let mut bindings: BTreeMap<String, ConnectorBindingSnapshot> = BTreeMap::new();
    let mut latest: BTreeMap<String, (u64, ConnectorHealthFact, Option<u64>)> = BTreeMap::new();
    let mut source_cursor = 0;
    for event in events {
        let cursor = event.stream_version.unwrap_or_default();
        source_cursor = source_cursor.max(cursor);
        match event.kind.as_str() {
            "connector.binding" => {
                let binding: ConnectorBindingSnapshot =
                    serde_json::from_value(event.data["state"].clone())
                        .map_err(|_| "connector_health_binding_invalid".to_owned())?;
                binding.validate().map_err(str::to_owned)?;
                bindings.insert(binding.binding.binding_id.clone(), binding);
            }
            CONNECTOR_HEALTH_EVENT_KIND => {
                let fact: ConnectorHealthFact =
                    serde_json::from_value(event.data["health"].clone())
                        .map_err(|_| "connector_health_fact_invalid".to_owned())?;
                fact.validate()?;
                if !bindings.contains_key(&fact.binding_id) {
                    return Err("connector_health_binding_missing".to_owned());
                }
                latest.insert(fact.binding_id.clone(), (cursor, fact, event.data_epoch));
            }
            _ => {}
        }
    }
    if source_cursor == 0 {
        source_cursor = 1;
    }
    let mut entries = Vec::with_capacity(latest.len());
    let mut limitations = Vec::new();
    for (_binding_id, (cursor, fact, data_epoch)) in latest {
        let stale = bindings
            .get(&fact.binding_id)
            .is_none_or(|binding| binding.revision != fact.binding_revision);
        let mut entry_limitations = fact.limitations.clone();
        if stale {
            entry_limitations.truncate(MAX_HEALTH_LIMITATIONS.saturating_sub(1));
            entry_limitations.push("binding_revision_changed".to_owned());
        } else {
            entry_limitations.truncate(MAX_HEALTH_LIMITATIONS);
        }
        entries.push(ConnectorHealthProjectionEntry {
            binding_revision: fact.binding_revision,
            credential_generation: fact.credential_generation,
            health: fact,
            source_cursor: cursor,
            projection_version: CONNECTOR_HEALTH_PROJECTION_VERSION.to_owned(),
            data_epoch,
            stale,
            proof_level: "source".to_owned(),
            limitations: entry_limitations,
        });
    }
    if entries.is_empty() {
        limitations.push("connector_health_not_observed".to_owned());
    }
    limitations.truncate(MAX_HEALTH_LIMITATIONS);
    let projection = ConnectorHealthProjection {
        schema: CONNECTOR_HEALTH_PROJECTION_SCHEMA.to_owned(),
        source_cursor,
        projection_version: CONNECTOR_HEALTH_PROJECTION_VERSION.to_owned(),
        entries,
        limitations,
    };
    projection.validate()?;
    Ok(projection)
}
