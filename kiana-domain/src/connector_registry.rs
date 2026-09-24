//! Immutable Connector registry snapshot and CAS boundary.

use crate::{json_digest, ConnectorDefinition};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const CONNECTOR_REGISTRY_SCHEMA: &str = "kiana.connector-registry.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorRegistrySnapshot {
    pub schema: String,
    pub registry_version: u64,
    pub definitions: BTreeMap<String, ConnectorDefinition>,
    pub signature_digest: String,
    pub content_digest: String,
}

impl ConnectorRegistrySnapshot {
    pub fn new(
        registry_version: u64,
        definitions: BTreeMap<String, ConnectorDefinition>,
        signature_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: CONNECTOR_REGISTRY_SCHEMA.to_owned(),
            registry_version,
            definitions,
            signature_digest: signature_digest.into(),
            content_digest: String::new(),
        };
        snapshot.content_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn cas_replace(&self, expected_version: u64, next: Self) -> Result<Self, String> {
        self.validate()?;
        next.validate()?;
        if expected_version != self.registry_version {
            return Err("connector_registry_cas_conflict".to_owned());
        }
        if next.registry_version != self.registry_version.saturating_add(1) {
            return Err("connector_registry_version_not_monotonic".to_owned());
        }
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_REGISTRY_SCHEMA
            || self.registry_version == 0
            || self.definitions.is_empty()
        {
            return Err("connector_registry_header_invalid".to_owned());
        }
        for (key, definition) in &self.definitions {
            if key != &definition.connector_id
                || definition.schema != "kiana.connector-definition.v1"
                || definition.connector_id.trim().is_empty()
                || definition.version.trim().is_empty()
                || definition.operations.is_empty()
                || !matches!(
                    definition.adapter.as_str(),
                    "local_fixture" | "local_only" | "stdio_mcp"
                )
            {
                return Err("connector_registry_definition_invalid".to_owned());
            }
        }
        for (value, field) in [
            (
                &self.signature_digest,
                "connector_registry_signature_digest",
            ),
            (&self.content_digest, "connector_registry_content_digest"),
        ] {
            let Some(hex) = value.strip_prefix("sha256:") else {
                return Err(format!("{field}_invalid"));
            };
            if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.content_digest != self.digest() {
            return Err("connector_registry_content_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "registry_version": self.registry_version,
            "definitions": self.definitions,
            "signature_digest": self.signature_digest,
        }))
    }
}
