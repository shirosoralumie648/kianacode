//! Strict Skill/Plugin/Hook manifest adapters.
//!
//! The parser produces inert metadata.  It never loads an entrypoint or turns a declaration into
//! a capability.  Legacy manifests are accepted only through the explicit adapter branch and are
//! marked as such in the normalized DTO.

use kiana_domain::{json_digest, ExtensionError, ExtensionErrorCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const NORMALIZED_PLUGIN_MANIFEST_SCHEMA: &str = kiana_domain::PLUGIN_MANIFEST_SCHEMA;
pub const NORMALIZED_HOOK_MANIFEST_SCHEMA: &str = kiana_domain::HOOK_MANIFEST_SCHEMA;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentKind {
    Commands,
    Agents,
    Skills,
    Hooks,
    OutputStyles,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponent {
    pub id: String,
    pub kind: PluginComponentKind,
    pub entry: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedPluginManifest {
    pub schema: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub components: Vec<PluginComponent>,
    pub legacy_adapter: bool,
    pub source_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookEntry {
    pub id: String,
    pub event: String,
    pub matcher: String,
    pub phase: String,
    pub entry: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedHookManifest {
    pub schema: String,
    pub hooks: Vec<HookEntry>,
    pub legacy_adapter: bool,
    pub source_digest: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictPluginManifest {
    schema: String,
    name: String,
    version: String,
    #[serde(default)]
    description: Option<String>,
    components: Vec<PluginComponent>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictHookManifest {
    schema: String,
    hooks: Vec<HookEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyComponent {
    #[serde(default)]
    id: Option<String>,
    entry: String,
}

fn error(code: ExtensionErrorCode, message: impl Into<String>) -> ExtensionError {
    ExtensionError::new(code, message, false)
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

fn valid_entry(value: &str) -> bool {
    kiana_domain::valid_extension_path(value)
}

fn validate_components(
    name: &str,
    version: &str,
    description: Option<&str>,
    components: &[PluginComponent],
) -> Result<(), ExtensionError> {
    if !valid_identifier(name, 128) || !valid_identifier(version, 64) {
        return Err(error(
            ExtensionErrorCode::InvalidIdentity,
            "plugin name or version is invalid",
        ));
    }
    if description.is_some_and(|value| value.len() > 4_096 || value.contains('\0')) {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "plugin description is invalid",
        ));
    }
    if components.is_empty() || components.len() > 64 {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "plugin components are required and bounded",
        ));
    }
    let mut ids = BTreeSet::new();
    for component in components {
        if !valid_identifier(&component.id, 128)
            || !valid_entry(&component.entry)
            || !ids.insert(component.id.clone())
        {
            return Err(error(
                ExtensionErrorCode::InvalidIdentity,
                "plugin component id or entry is invalid",
            ));
        }
    }
    Ok(())
}

fn validate_hooks(hooks: &[HookEntry]) -> Result<(), ExtensionError> {
    if hooks.is_empty() || hooks.len() > 64 {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "hook entries are required and bounded",
        ));
    }
    let mut ids = BTreeSet::new();
    for hook in hooks {
        if !valid_identifier(&hook.id, 128)
            || hook.event.trim().is_empty()
            || hook.event.len() > 128
            || hook.matcher.trim().is_empty()
            || hook.matcher.len() > 1_024
            || !matches!(hook.phase.as_str(), "guard" | "observer")
            || !valid_entry(&hook.entry)
            || !ids.insert(hook.id.clone())
        {
            return Err(error(
                ExtensionErrorCode::InvalidIdentity,
                "hook id, matcher, phase or entry is invalid",
            ));
        }
    }
    Ok(())
}

pub fn parse_plugin_manifest(raw: &str) -> Result<NormalizedPluginManifest, ExtensionError> {
    if raw.len() > MAX_MANIFEST_BYTES {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "plugin manifest exceeds size limit",
        ));
    }
    let value = kiana_domain::parse_bounded_json(raw.as_bytes()).map_err(|reason| {
        error(
            ExtensionErrorCode::InvalidSchema,
            format!("plugin manifest JSON invalid:{reason}"),
        )
    })?;
    let source_digest = json_digest(&value);
    let schema = value.get("schema").and_then(Value::as_str);
    if schema == Some(NORMALIZED_PLUGIN_MANIFEST_SCHEMA) {
        let strict: StrictPluginManifest = serde_json::from_value(value).map_err(|_| {
            error(
                ExtensionErrorCode::InvalidSchema,
                "strict plugin manifest fields invalid",
            )
        })?;
        if strict.schema != NORMALIZED_PLUGIN_MANIFEST_SCHEMA {
            return Err(error(
                ExtensionErrorCode::InvalidSchema,
                "plugin manifest schema unsupported",
            ));
        }
        validate_components(
            &strict.name,
            &strict.version,
            strict.description.as_deref(),
            &strict.components,
        )?;
        return Ok(NormalizedPluginManifest {
            schema: NORMALIZED_PLUGIN_MANIFEST_SCHEMA.to_owned(),
            name: strict.name,
            version: strict.version,
            description: strict.description,
            components: strict.components,
            legacy_adapter: false,
            source_digest,
        });
    }

    parse_legacy_plugin_manifest(value, source_digest)
}

fn parse_legacy_plugin_manifest(
    value: Value,
    source_digest: String,
) -> Result<NormalizedPluginManifest, ExtensionError> {
    let object = value.as_object().ok_or_else(|| {
        error(
            ExtensionErrorCode::InvalidSchema,
            "legacy plugin manifest must be an object",
        )
    })?;
    let allowed = [
        "name",
        "version",
        "description",
        "author",
        "components",
        "commands",
        "agents",
        "skills",
        "hooks",
        "outputStyles",
    ];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "legacy plugin manifest contains unknown fields",
        ));
    }
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            error(
                ExtensionErrorCode::InvalidIdentity,
                "legacy plugin name required",
            )
        })?
        .to_owned();
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("legacy")
        .to_owned();
    let description = object
        .get("description")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut components = Vec::new();
    if let Some(entries) = object.get("components") {
        components.extend(parse_legacy_components(entries, None)?);
    }
    for (field, kind) in [
        ("commands", PluginComponentKind::Commands),
        ("agents", PluginComponentKind::Agents),
        ("skills", PluginComponentKind::Skills),
        ("hooks", PluginComponentKind::Hooks),
        ("outputStyles", PluginComponentKind::OutputStyles),
    ] {
        if let Some(entries) = object.get(field) {
            components.extend(parse_legacy_components(entries, Some(kind))?);
        }
    }
    validate_components(&name, &version, description.as_deref(), &components)?;
    Ok(NormalizedPluginManifest {
        schema: NORMALIZED_PLUGIN_MANIFEST_SCHEMA.to_owned(),
        name,
        version,
        description,
        components,
        legacy_adapter: true,
        source_digest,
    })
}

fn parse_legacy_components(
    value: &Value,
    kind: Option<PluginComponentKind>,
) -> Result<Vec<PluginComponent>, ExtensionError> {
    let values = value.as_array().ok_or_else(|| {
        error(
            ExtensionErrorCode::InvalidSchema,
            "legacy component list must be an array",
        )
    })?;
    if values.len() > 64 {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "legacy component list exceeds limit",
        ));
    }
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if let Some(entry) = value.as_str() {
                let kind = kind.ok_or_else(|| {
                    error(
                        ExtensionErrorCode::InvalidSchema,
                        "legacy components require an explicit kind",
                    )
                })?;
                return Ok(PluginComponent {
                    id: format!("component-{index}"),
                    kind,
                    entry: entry.to_owned(),
                });
            }
            let raw: LegacyComponent = serde_json::from_value(value.clone()).map_err(|_| {
                error(
                    ExtensionErrorCode::InvalidSchema,
                    "legacy component object invalid",
                )
            })?;
            let kind = kind.ok_or_else(|| {
                error(
                    ExtensionErrorCode::InvalidSchema,
                    "legacy components require an explicit kind",
                )
            })?;
            Ok(PluginComponent {
                id: raw.id.unwrap_or_else(|| format!("component-{index}")),
                kind,
                entry: raw.entry,
            })
        })
        .collect()
}

pub fn parse_hook_manifest(raw: &str) -> Result<NormalizedHookManifest, ExtensionError> {
    if raw.len() > MAX_MANIFEST_BYTES {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "hook manifest exceeds size limit",
        ));
    }
    let value = kiana_domain::parse_bounded_json(raw.as_bytes()).map_err(|reason| {
        error(
            ExtensionErrorCode::InvalidSchema,
            format!("hook manifest JSON invalid:{reason}"),
        )
    })?;
    let source_digest = json_digest(&value);
    let schema = value.get("schema").and_then(Value::as_str);
    let (hooks, legacy_adapter) = if schema == Some(NORMALIZED_HOOK_MANIFEST_SCHEMA) {
        let strict: StrictHookManifest = serde_json::from_value(value).map_err(|_| {
            error(
                ExtensionErrorCode::InvalidSchema,
                "strict hook manifest fields invalid",
            )
        })?;
        if strict.schema != NORMALIZED_HOOK_MANIFEST_SCHEMA {
            return Err(error(
                ExtensionErrorCode::InvalidSchema,
                "hook manifest schema unsupported",
            ));
        }
        (strict.hooks, false)
    } else {
        let object = value.as_object().ok_or_else(|| {
            error(
                ExtensionErrorCode::InvalidSchema,
                "legacy hook manifest must be an object",
            )
        })?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "hooks" | "version"))
        {
            return Err(error(
                ExtensionErrorCode::InvalidSchema,
                "legacy hook manifest contains unknown fields",
            ));
        }
        let hooks: Vec<HookEntry> =
            serde_json::from_value(object.get("hooks").cloned().ok_or_else(|| {
                error(
                    ExtensionErrorCode::InvalidSchema,
                    "legacy hooks are required",
                )
            })?)
            .map_err(|_| {
                error(
                    ExtensionErrorCode::InvalidSchema,
                    "legacy hook entries invalid",
                )
            })?;
        (hooks, true)
    };
    validate_hooks(&hooks)?;
    Ok(NormalizedHookManifest {
        schema: NORMALIZED_HOOK_MANIFEST_SCHEMA.to_owned(),
        hooks,
        legacy_adapter,
        source_digest,
    })
}

/// Convert a normalized plugin manifest to a deterministic component index.
pub fn component_index(manifest: &NormalizedPluginManifest) -> BTreeMap<String, PluginComponent> {
    manifest
        .components
        .iter()
        .cloned()
        .map(|component| (component.id.clone(), component))
        .collect()
}
