//! Versioned model existence/capability catalog.
//!
//! Catalog entries describe what a configured connection claims. Resolving an entry never grants
//! tools or network access; those remain policy and prepared-route decisions.

use crate::{json_digest, CapabilitySupport, ModelCapabilities};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const MODEL_CATALOG_SCHEMA: &str = "kiana.model-catalog.v1";
pub const MODEL_CATALOG_ENTRY_SCHEMA: &str = "kiana.model-catalog-entry.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCatalogSource {
    Builtin,
    Configured,
    Cache,
    LiveDiscovery,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalogEntry {
    pub schema: String,
    pub provider_id: String,
    pub connection_id: String,
    pub model_id: String,
    pub capabilities: ModelCapabilities,
    pub source: ModelCatalogSource,
    pub catalog_revision: String,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
}

impl ModelCatalogEntry {
    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), String> {
        if self.schema != MODEL_CATALOG_ENTRY_SCHEMA
            || self.provider_id.trim().is_empty()
            || self.connection_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.model_id.len() > 512
            || self.catalog_revision.trim().is_empty()
        {
            return Err("model_catalog_entry_invalid".to_owned());
        }
        if let (Some(expires), Some(now)) = (self.expires_at_unix_ms, now_unix_ms) {
            if expires <= now {
                return Err("model_catalog_entry_expired".to_owned());
            }
        }
        Ok(())
    }

    pub fn has_tool_capability(&self) -> bool {
        self.capabilities.tools == CapabilitySupport::Supported
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalog {
    pub schema: String,
    pub version: u64,
    pub entries: Vec<ModelCatalogEntry>,
    pub catalog_digest: String,
}

impl ModelCatalog {
    pub fn new(entries: Vec<ModelCatalogEntry>) -> Result<Self, String> {
        let mut catalog = Self {
            schema: MODEL_CATALOG_SCHEMA.to_owned(),
            version: 1,
            entries,
            catalog_digest: String::new(),
        };
        catalog.catalog_digest = catalog.digest();
        catalog.validate(None)?;
        Ok(catalog)
    }

    pub fn validate(&self, now_unix_ms: Option<u64>) -> Result<(), String> {
        if self.schema != MODEL_CATALOG_SCHEMA
            || self.version == 0
            || self.entries.is_empty()
            || self.entries.len() > 4_096
            || self.catalog_digest != self.digest()
        {
            return Err("model_catalog_invalid".to_owned());
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            entry.validate(now_unix_ms)?;
            if !identities.insert((
                entry.provider_id.clone(),
                entry.connection_id.clone(),
                entry.model_id.clone(),
            )) {
                return Err("model_catalog_duplicate_entry".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "entries": self.entries,
        }))
    }

    pub fn resolve(
        &self,
        model_id: &str,
        connection_id: Option<&str>,
        now_unix_ms: Option<u64>,
    ) -> Result<ModelCatalogEntry, String> {
        let mut matches = self.entries.iter().filter(|entry| {
            entry.model_id == model_id
                && connection_id.is_none_or(|connection| entry.connection_id == connection)
                && entry
                    .expires_at_unix_ms
                    .is_none_or(|expires| now_unix_ms.is_none_or(|now| expires > now))
        });
        let entry = matches
            .next()
            .ok_or_else(|| "model_catalog_entry_missing".to_owned())?;
        if matches.next().is_some() {
            return Err("model_catalog_ambiguous".to_owned());
        }
        Ok(entry.clone())
    }

    pub fn supports_tools(&self, model_id: &str, connection_id: &str) -> Result<bool, String> {
        Ok(self
            .resolve(model_id, Some(connection_id), None)?
            .has_tool_capability())
    }
}
