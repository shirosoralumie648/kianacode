//! The server-owned authority catalog for the currently supported model-visible tools.
//!
//! `OPERATION_SPECS` owns operation identity and policy; this cached projection preserves the
//! reference-based ToolSpec API without introducing a second authority table.
use crate::{
    json_digest, model_tool_name, tool_schemas, validate_schema_contract, CapabilityKind,
    RiskLevel, OPERATION_SPECS,
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

#[derive(Clone, Copy, Debug)]
pub struct ToolSpecCollection;

pub const TOOL_SPECS: ToolSpecCollection = ToolSpecCollection;
static TOOL_SPECS_CACHE: std::sync::LazyLock<Vec<ToolSpec>> = std::sync::LazyLock::new(|| {
    OPERATION_SPECS
        .iter()
        .filter_map(tool_spec_from_operation)
        .collect()
});

impl ToolSpecCollection {
    pub fn iter(&self) -> impl Iterator<Item = &'static ToolSpec> + '_ {
        TOOL_SPECS_CACHE.iter()
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl std::ops::Deref for ToolSpecCollection {
    type Target = [ToolSpec];

    fn deref(&self) -> &Self::Target {
        TOOL_SPECS_CACHE.as_slice()
    }
}

impl AsRef<[ToolSpec]> for ToolSpecCollection {
    fn as_ref(&self) -> &[ToolSpec] {
        TOOL_SPECS_CACHE.as_slice()
    }
}

impl IntoIterator for ToolSpecCollection {
    type Item = &'static ToolSpec;
    type IntoIter = std::slice::Iter<'static, ToolSpec>;

    fn into_iter(self) -> Self::IntoIter {
        TOOL_SPECS_CACHE.iter()
    }
}

impl IntoIterator for &ToolSpecCollection {
    type Item = &'static ToolSpec;
    type IntoIter = std::slice::Iter<'static, ToolSpec>;

    fn into_iter(self) -> Self::IntoIter {
        TOOL_SPECS_CACHE.iter()
    }
}

fn tool_spec_from_operation(spec: &crate::OperationSpec) -> Option<ToolSpec> {
    let model = spec.model?;
    Some(ToolSpec {
        name: model.name,
        aliases: model.aliases,
        capability: spec.capability.clone(),
        operation: spec.operation,
        risk_policy: spec.minimum_risk,
        side_effecting: model.side_effecting,
        schema: model.schema,
    })
}

/// Versioned, immutable view consumed by model schema mapping, policy and Broker registration.
/// Every ToolDescriptor is projected from the closed operation specification.
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
    /// Deterministic scheduler class. Read-only calls may share a bounded group; every other
    /// operation is an exclusive barrier and is still re-authorized independently by Core.
    pub scheduling: String,
    /// Coarse resource claims used by the batch planner; values never grant access by themselves.
    pub resources: Vec<String>,
    pub max_parallelism: u32,
}

impl ToolCatalogSnapshot {
    pub fn current() -> Self {
        let tools = TOOL_SPECS
            .iter()
            .filter_map(|spec| {
                let model = crate::model_tool_projection_for_operation(spec.operation)?;
                let argument_schema = (model.parameters)();
                Some(ToolDescriptor {
                    name: model.name.to_owned(),
                    wire_name: model.name.replace('.', "_"),
                    aliases: model
                        .aliases
                        .iter()
                        .map(|alias| (*alias).to_owned())
                        .collect(),
                    capability: spec.capability.clone(),
                    operation: spec.operation.to_owned(),
                    minimum_risk: spec.risk_policy,
                    side_effecting: spec.side_effecting,
                    argument_schema,
                    result_schema: (model.result_schema)(),
                    output_limit: TOOL_DEFAULT_OUTPUT_LIMIT,
                    execution_mode: "brokered".to_owned(),
                    replay_class: if spec.side_effecting {
                        "reconcile".to_owned()
                    } else {
                        "replay_safe".to_owned()
                    },
                    scheduling: if spec.side_effecting {
                        "exclusive".to_owned()
                    } else {
                        "parallel_read".to_owned()
                    },
                    resources: model
                        .catalog_resources
                        .iter()
                        .map(|resource| (*resource).to_owned())
                        .collect(),
                    max_parallelism: if spec.side_effecting { 1 } else { 4 },
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
                || !matches!(tool.scheduling.as_str(), "parallel_read" | "exclusive")
                || tool.resources.is_empty()
                || tool
                    .resources
                    .iter()
                    .any(|resource| resource.trim().is_empty())
                || tool.max_parallelism == 0
                || tool.scheduling == "parallel_read" && tool.side_effecting
                || tool.scheduling == "exclusive" && tool.max_parallelism != 1
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

pub fn tool_spec(name: &str) -> Option<&'static ToolSpec> {
    TOOL_SPECS
        .iter()
        .find(|spec| spec.name == name || spec.aliases.iter().any(|alias| *alias == name))
}

pub fn validate_tool_action_binding(spec: &ToolSpec) -> Result<(), String> {
    let model_name =
        model_tool_name(spec.name).ok_or_else(|| "tool_action_tool_unknown".to_owned())?;
    if model_name != spec.name {
        return Err("tool_action_model_name_mismatch".to_owned());
    }

    let operation = crate::canonical_action_operation(model_name)
        .ok_or_else(|| "tool_action_operation_unknown".to_owned())?;
    if operation != spec.operation || !crate::ACTION_OPERATIONS.contains(&spec.operation) {
        return Err("tool_action_operation_mismatch".to_owned());
    }
    let descriptor = crate::capability_action_descriptor(operation)
        .ok_or_else(|| "tool_action_descriptor_missing".to_owned())?;
    if descriptor.operation != spec.operation {
        return Err("tool_action_operation_mismatch".to_owned());
    }
    if descriptor.capability != spec.capability || descriptor.minimum_risk != spec.risk_policy {
        return Err("tool_action_metadata_mismatch".to_owned());
    }

    for alias in spec.aliases {
        if model_tool_name(alias) != Some(spec.name)
            || crate::canonical_action_operation(alias) != Some(spec.operation)
        {
            return Err("tool_action_alias_mismatch".to_owned());
        }
    }

    let model_schema = tool_schemas()
        .into_iter()
        .find(|schema| schema["name"] == spec.name)
        .and_then(|schema| schema.get("parameters").cloned())
        .ok_or_else(|| "tool_action_model_schema_missing".to_owned())?;
    let mut compatible_action_schema = model_schema;
    compatible_action_schema["additionalProperties"] = json!(true);
    if descriptor.argument_schema != compatible_action_schema {
        return Err("tool_action_schema_mismatch".to_owned());
    }

    Ok(())
}

pub fn validate_tool_action_bindings() -> Result<(), String> {
    for spec in TOOL_SPECS {
        validate_tool_action_binding(spec)?;
    }
    Ok(())
}

pub fn validate_tool_authority() -> Result<(), String> {
    let model_tool_count = TOOL_SPECS.len();
    if model_tool_count == 0 || model_tool_count != 5 {
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
    validate_tool_action_bindings()?;
    Ok(())
}
