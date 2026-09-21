//! Shared request-compilation facts used by provider protocol encoders.
//!
//! The server-owned tool catalog is the only source of model-visible names.  Protocol adapters
//! may change the wire spelling, but they must preserve a reversible mapping and may not turn an
//! unknown or colliding name into an executable tool.

use crate::{json_digest, tool_catalog_hash, tool_wire_name, ModelError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const TOOL_NAME_MAP_SCHEMA: &str = "kiana.model-tool-name-map.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolNameMap {
    pub schema: String,
    pub strict: bool,
    pub catalog_hash: String,
    pub internal_to_wire: BTreeMap<String, String>,
    pub wire_to_internal: BTreeMap<String, String>,
}

impl ToolNameMap {
    /// Build the mapping from the server-owned model-visible tool list.
    pub fn from_tools(tools: &[Value], strict: bool) -> Result<Self, ModelError> {
        if tools.len() > 64 {
            return Err(ModelError::invalid("model_tool_catalog_too_large"));
        }
        let entries = tools
            .iter()
            .map(|tool| {
                let internal = tool["name"]
                    .as_str()
                    .filter(|name| !name.trim().is_empty())
                    .ok_or_else(|| ModelError::invalid("model_tool_schema_name_missing"))?;
                let wire = tool_wire_name(internal)
                    .ok_or_else(|| ModelError::invalid("model_tool_unadvertised"))?;
                Ok((internal.to_owned(), wire))
            })
            .collect::<Result<Vec<_>, ModelError>>()?;
        Self::from_entries(entries, tool_catalog_hash(tools), strict).map_err(ModelError::invalid)
    }

    /// Construct a mapping from explicit pairs.  This is used by protocol fixtures to prove
    /// collision rejection without changing the authoritative five-tool catalog.
    pub fn from_entries(
        entries: impl IntoIterator<Item = (String, String)>,
        catalog_hash: String,
        strict: bool,
    ) -> Result<Self, String> {
        let mut internal_to_wire = BTreeMap::new();
        let mut wire_to_internal = BTreeMap::new();
        for (internal, wire) in entries {
            if internal.trim().is_empty() || internal.len() > 256 {
                return Err("tool_name_map_internal_invalid".to_owned());
            }
            if wire.trim().is_empty()
                || wire.len() > 64
                || !wire
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            {
                return Err("tool_name_map_wire_invalid".to_owned());
            }
            if internal_to_wire
                .insert(internal.clone(), wire.clone())
                .is_some()
            {
                return Err("tool_name_map_internal_duplicate".to_owned());
            }
            if wire_to_internal.insert(wire, internal).is_some() {
                return Err("tool_name_map_wire_collision".to_owned());
            }
        }
        let map = Self {
            schema: TOOL_NAME_MAP_SCHEMA.to_owned(),
            strict,
            catalog_hash,
            internal_to_wire,
            wire_to_internal,
        };
        map.validate()?;
        Ok(map)
    }

    pub fn wire_name(&self, internal: &str) -> Result<&str, ModelError> {
        self.internal_to_wire
            .get(internal)
            .map(String::as_str)
            .ok_or_else(|| ModelError::invalid("provider_tool_not_in_compiled_catalog"))
    }

    pub fn internal_name(&self, wire: &str) -> Result<&str, ModelError> {
        self.wire_to_internal
            .get(wire)
            .map(String::as_str)
            .ok_or_else(|| ModelError::invalid("provider_returned_unadvertised_tool"))
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "strict": self.strict,
            "catalog_hash": self.catalog_hash,
            "internal_to_wire": self.internal_to_wire,
            "wire_to_internal": self.wire_to_internal,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_NAME_MAP_SCHEMA
            || !self.catalog_hash.starts_with("sha256:")
            || self.catalog_hash.len() != 71
            || self.internal_to_wire.len() != self.wire_to_internal.len()
            || self.internal_to_wire.iter().any(|(internal, wire)| {
                internal.trim().is_empty() || self.wire_to_internal.get(wire) != Some(internal)
            })
            || self.wire_to_internal.iter().any(|(wire, internal)| {
                wire.trim().is_empty() || self.internal_to_wire.get(internal) != Some(wire)
            })
        {
            return Err("tool_name_map_invalid".to_owned());
        }
        Ok(())
    }
}
