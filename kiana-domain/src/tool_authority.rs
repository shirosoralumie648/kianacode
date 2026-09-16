//! The server-owned authority catalog for the five model-visible tools.
//!
//! JSON schemas remain in `tool_catalog`; this typed table owns the identity/alias/capability
//! mapping so Runner and policy code cannot silently grow a second model tool surface.
use crate::{
    tool_schemas, CapabilityKind, RiskLevel, TOOL_APPLY_PATCH, TOOL_MCP, TOOL_MEMORY_SEARCH,
    TOOL_MEMORY_WRITE, TOOL_SHELL,
};
use serde::Serialize;

pub const TOOL_AUTHORITY_SCHEMA: &str = "kiana.tool-authority.v1";

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
    if TOOL_SPECS.len() != 5 {
        return Err("tool_authority_surface_invalid".to_owned());
    }
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
