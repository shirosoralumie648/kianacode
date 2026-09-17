//! The server-owned authority catalog for the currently supported model-visible tools.
//!
//! JSON schemas remain in `tool_catalog`; this typed table owns the identity/alias/capability
//! mapping so Runner and policy code cannot silently grow a second model tool surface.
use crate::{
    json_digest, tool_schemas, validate_schema_contract, CapabilityKind, RiskLevel,
    TOOL_APPLY_PATCH, TOOL_MCP, TOOL_MEMORY_SEARCH, TOOL_MEMORY_WRITE, TOOL_SHELL,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const TOOL_AUTHORITY_SCHEMA: &str = "kiana.tool-authority.v1";
pub const TOOL_CATALOG_SCHEMA: &str = "kiana.tool-catalog.v1";
pub const TOOL_CATALOG_VERSION: crate::SchemaVersion = crate::SchemaVersion::new(1, 0);
pub const TOOL_DEFAULT_OUTPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolSpec {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub capability: CapabilityKind,
    pub operation: &'static str,
    pub risk_policy: RiskLevel,
    pub side_effecting: bool,
    pub schema: &'static str,
}

/// Versioned, immutable view consumed by model schema mapping, policy and Broker registration.
/// The static `ToolSpec` table remains the source for aliases/capability identity; this snapshot
/// adds the schema, output and replay metadata that must be pinned for one model step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCatalogSnapshot {
    pub schema: String,
    pub version: crate::SchemaVersion,
    pub digest: String,
    pub tools: Vec<ToolDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDescriptor {
    pub name: String,
    pub wire_name: String,
    pub aliases: Vec<String>,
    pub capability: CapabilityKind,
    pub operation: String,
    pub minimum_risk: RiskLevel,
    pub side_effecting: bool,
    pub argument_schema: Value,
    pub result_schema: Value,
    pub output_limit: u64,
    pub execution_mode: String,
    pub replay_class: String,
}

impl ToolCatalogSnapshot {
    pub fn current() -> Self {
        let schemas = tool_schemas();
        let tools = TOOL_SPECS
            .iter()
            .filter_map(|spec| {
                let argument_schema = schemas
                    .iter()
                    .find(|schema| schema["name"] == spec.name)
                    .and_then(|schema| schema.get("parameters"))
                    .cloned()?;
                Some(ToolDescriptor {
                    name: spec.name.to_owned(),
                    wire_name: spec.name.replace('.', "_"),
                    aliases: spec
                        .aliases
                        .iter()
                        .map(|alias| (*alias).to_owned())
                        .collect(),
                    capability: spec.capability.clone(),
                    operation: spec.operation.to_owned(),
                    minimum_risk: spec.risk_policy,
                    side_effecting: spec.side_effecting,
                    argument_schema,
                    result_schema: json!({
                        "type": "object",
                        "additionalProperties": true
                    }),
                    output_limit: TOOL_DEFAULT_OUTPUT_LIMIT,
                    execution_mode: "brokered".to_owned(),
                    replay_class: if spec.side_effecting {
                        "reconcile".to_owned()
                    } else {
                        "replay_safe".to_owned()
                    },
                })
            })
            .collect::<Vec<_>>();
        let mut snapshot = Self {
            schema: TOOL_CATALOG_SCHEMA.to_owned(),
            version: TOOL_CATALOG_VERSION,
            digest: String::new(),
            tools,
        };
        snapshot.digest = snapshot.unsigned_digest();
        snapshot
    }

    fn unsigned_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "tools": self.tools,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != TOOL_CATALOG_SCHEMA
            || self.version != TOOL_CATALOG_VERSION
            || self.tools.is_empty()
            || self.digest != self.unsigned_digest()
        {
            return Err("tool_catalog_snapshot_invalid".to_owned());
        }
        let mut names = std::collections::BTreeSet::new();
        let mut wires = std::collections::BTreeSet::new();
        let mut aliases = std::collections::BTreeSet::new();
        for tool in &self.tools {
            if tool.name.trim().is_empty()
                || tool.wire_name.trim().is_empty()
                || tool.operation.trim().is_empty()
                || tool.execution_mode != "brokered"
                || !matches!(tool.replay_class.as_str(), "replay_safe" | "reconcile")
                || tool.output_limit == 0
                || tool.output_limit > 16 * 1024 * 1024
                || !names.insert(tool.name.clone())
                || !wires.insert(tool.wire_name.clone())
                || tool.aliases.iter().any(|alias| {
                    alias.trim().is_empty()
                        || names.contains(alias)
                        || !aliases.insert(alias.clone())
                })
            {
                return Err("tool_catalog_descriptor_invalid".to_owned());
            }
            validate_schema_contract(&tool.argument_schema)
                .map_err(|error| format!("tool_catalog_argument_schema_invalid:{error}"))?;
            validate_schema_contract(&tool.result_schema)
                .map_err(|error| format!("tool_catalog_result_schema_invalid:{error}"))?;
            let spec =
                tool_spec(&tool.name).ok_or_else(|| "tool_catalog_tool_unadvertised".to_owned())?;
            if tool.operation != spec.operation
                || tool.capability != spec.capability.clone()
                || tool.minimum_risk != spec.risk_policy
                || tool.side_effecting != spec.side_effecting
            {
                return Err("tool_catalog_authority_drift".to_owned());
            }
        }
        if self.tools.len() != TOOL_SPECS.len()
            || self
                .tools
                .iter()
                .any(|tool| TOOL_SPECS.iter().all(|spec| spec.name != tool.name))
        {
            return Err("tool_catalog_authority_drift".to_owned());
        }
        Ok(())
    }

    pub fn descriptor(&self, name: &str) -> Option<&ToolDescriptor> {
        self.tools
            .iter()
            .find(|tool| tool.name == name || tool.aliases.iter().any(|alias| alias == name))
    }
}

pub fn current_tool_catalog() -> ToolCatalogSnapshot {
    ToolCatalogSnapshot::current()
}

pub fn tool_catalog_digest() -> String {
    ToolCatalogSnapshot::current().digest
}

/// Bind a prepared request to both the catalog snapshot and the exact model-visible list.
pub fn tool_catalog_hash(tools: &[Value]) -> String {
    json_digest(&json!({
        "catalog_schema": TOOL_CATALOG_SCHEMA,
        "catalog_version": TOOL_CATALOG_VERSION,
        "catalog_digest": tool_catalog_digest(),
        "tools": tools,
    }))
}

pub fn tool_wire_name(name: &str) -> Option<String> {
    ToolCatalogSnapshot::current()
        .descriptor(name)
        .map(|tool| tool.wire_name.clone())
}

pub const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        name: TOOL_SHELL,
        aliases: &["shell.exec", "bash", "exec", "command_execution"],
        capability: CapabilityKind::Process,
        operation: "shell.exec",
        risk_policy: RiskLevel::ReadOnly,
        side_effecting: true,
        schema: "kiana.tool.shell.v1",
    },
    ToolSpec {
        name: TOOL_APPLY_PATCH,
        aliases: &["file_change"],
        capability: CapabilityKind::Filesystem,
        operation: "apply_patch",
        risk_policy: RiskLevel::LocalWrite,
        side_effecting: true,
        schema: "kiana.tool.apply-patch.v1",
    },
    ToolSpec {
        name: TOOL_MCP,
        aliases: &["mcp.call"],
        capability: CapabilityKind::Network,
        operation: "mcp.call",
        risk_policy: RiskLevel::ExternalSideEffect,
        side_effecting: true,
        schema: "kiana.tool.mcp.v1",
    },
    ToolSpec {
        name: TOOL_MEMORY_SEARCH,
        aliases: &[],
        capability: CapabilityKind::Query,
        operation: "memory.search",
        risk_policy: RiskLevel::ReadOnly,
        side_effecting: false,
        schema: "kiana.tool.memory-search.v1",
    },
    ToolSpec {
        name: TOOL_MEMORY_WRITE,
        aliases: &[],
        capability: CapabilityKind::Filesystem,
        operation: "memory.write",
        risk_policy: RiskLevel::LocalWrite,
        side_effecting: true,
        schema: "kiana.tool.memory-write.v1",
    },
];

pub fn tool_spec(name: &str) -> Option<&'static ToolSpec> {
    TOOL_SPECS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.iter().any(|alias| *alias == name))
}

pub fn validate_tool_authority() -> Result<(), String> {
    if TOOL_SPECS.is_empty() {
        return Err("tool_authority_surface_invalid".to_owned());
    }
    ToolCatalogSnapshot::current().validate()?;
    let schemas = tool_schemas();
    let names = TOOL_SPECS
        .iter()
        .map(|spec| spec.name)
        .collect::<std::collections::BTreeSet<_>>();
    if names.len() != TOOL_SPECS.len() {
        return Err("tool_authority_spec_invalid".to_owned());
    }
    let mut aliases = std::collections::BTreeSet::new();
    for spec in TOOL_SPECS {
        if spec.operation.trim().is_empty()
            || spec.schema.trim().is_empty()
            || !schemas.iter().any(|schema| schema["name"] == spec.name)
        {
            return Err("tool_authority_spec_invalid".to_owned());
        }
        for alias in spec.aliases {
            if alias.trim().is_empty() || names.contains(alias) || !aliases.insert(*alias) {
                return Err("tool_authority_alias_conflict".to_owned());
            }
        }
    }
    if schemas
        .iter()
        .filter_map(|schema| schema["name"].as_str())
        .collect::<std::collections::BTreeSet<_>>()
        != names
    {
        return Err("tool_authority_schema_drift".to_owned());
    }
    Ok(())
}
