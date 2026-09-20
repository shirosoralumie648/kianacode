//! Strict Skill/Plugin/Hook manifest adapters.
//!
//! The parser produces inert metadata.  It never loads an entrypoint or turns a declaration into
//! a capability.  Legacy manifests are accepted only through the explicit adapter branch and are
//! marked as such in the normalized DTO.

use kiana_domain::{json_digest, ExtensionError, ExtensionErrorCode, HookPhase};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const NORMALIZED_PLUGIN_MANIFEST_SCHEMA: &str = kiana_domain::PLUGIN_MANIFEST_SCHEMA;
pub const NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA: &str = "kiana.plugin-manifest.v2";
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
pub struct PluginManifestV2Component {
    pub id: String,
    pub kind: PluginComponentKind,
    pub entry: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedPluginManifestV2 {
    pub schema: String,
    pub plugin_id: String,
    pub publisher: String,
    pub version: String,
    pub license: String,
    pub source: String,
    pub namespace: String,
    pub components: Vec<PluginManifestV2Component>,
    pub required_dependencies: BTreeMap<String, String>,
    pub optional_dependencies: BTreeMap<String, String>,
    pub configuration_schema: Value,
    pub state_schema: Value,
    pub migration_refs: Vec<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalizedHookEvent {
    SessionStart,
    UserPromptSubmit,
    BeforeModel,
    PreToolUse,
    PostToolUse,
    PostToolFailure,
    Compaction,
    Stop,
    SessionEnd,
    Terminal,
}

impl NormalizedHookEvent {
    fn parse(value: &str) -> Option<Self> {
        let normalized = value
            .chars()
            .filter(|character| !matches!(character, '_' | '-' | '.' | ' '))
            .collect::<String>()
            .to_ascii_lowercase();
        match normalized.as_str() {
            "sessionstart" => Some(Self::SessionStart),
            "userpromptsubmit" => Some(Self::UserPromptSubmit),
            "beforemodel" => Some(Self::BeforeModel),
            "pretooluse" => Some(Self::PreToolUse),
            "posttooluse" => Some(Self::PostToolUse),
            "posttoolfailure" => Some(Self::PostToolFailure),
            "compaction" => Some(Self::Compaction),
            "stop" => Some(Self::Stop),
            "sessionend" => Some(Self::SessionEnd),
            "terminal" | "taskcompleted" => Some(Self::Terminal),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedHookDescriptor {
    pub schema: String,
    pub version: String,
    pub id: String,
    pub event: NormalizedHookEvent,
    pub matcher: String,
    pub phase: HookPhase,
    pub timeout_ms: u64,
    pub input_schema: String,
    pub output_schema: String,
    pub source_digest: String,
    pub effect: String,
    pub required_scope: BTreeSet<String>,
    pub deny_sticky: bool,
    pub ask_preserved: bool,
    pub update_requires_reauthorization: bool,
    pub entry: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedHookManifest {
    pub schema: String,
    pub hooks: Vec<HookEntry>,
    #[serde(default)]
    pub descriptors: Vec<NormalizedHookDescriptor>,
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
struct StrictPluginManifestV2 {
    schema: String,
    plugin_id: String,
    publisher: String,
    version: String,
    license: String,
    source: String,
    components: Vec<PluginManifestV2Component>,
    #[serde(default)]
    required_dependencies: BTreeMap<String, String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, String>,
    #[serde(default = "empty_object")]
    configuration_schema: Value,
    #[serde(default = "empty_object")]
    state_schema: Value,
    #[serde(default)]
    migration_refs: Vec<String>,
}

fn empty_object() -> Value {
    Value::Object(Default::default())
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

fn normalize_hook_descriptors(
    hooks: &[HookEntry],
    source_digest: &str,
) -> Result<Vec<NormalizedHookDescriptor>, ExtensionError> {
    hooks
        .iter()
        .map(|hook| {
            let event = NormalizedHookEvent::parse(&hook.event).ok_or_else(|| {
                error(
                    ExtensionErrorCode::Unsupported,
                    format!(
                        "unsupported hook event '{}'; adapter is fail-closed",
                        hook.event
                    ),
                )
            })?;
            let phase = match hook.phase.as_str() {
                "guard" => HookPhase::Guard,
                "observer" => HookPhase::Observer,
                _ => {
                    return Err(error(
                        ExtensionErrorCode::InvalidSchema,
                        "hook phase is invalid",
                    ))
                }
            };
            let event_name = serde_json::to_string(&event).map_err(|_| {
                error(
                    ExtensionErrorCode::InvalidSchema,
                    "hook event encode failed",
                )
            })?;
            let event_name = event_name.trim_matches('"').to_owned();
            let mut required_scope = BTreeSet::new();
            required_scope.insert(format!("hook:{event_name}"));
            Ok(NormalizedHookDescriptor {
                schema: "kiana.normalized-hook-descriptor.v1".to_owned(),
                version: "1.0.0".to_owned(),
                id: hook.id.clone(),
                event,
                matcher: hook.matcher.clone(),
                phase,
                timeout_ms: 5_000,
                input_schema: "kiana.hook-input.v1".to_owned(),
                output_schema: "kiana.hook-output.v1".to_owned(),
                source_digest: source_digest.to_owned(),
                effect: if phase == HookPhase::Guard {
                    "guard".to_owned()
                } else {
                    "observer".to_owned()
                },
                required_scope,
                deny_sticky: true,
                ask_preserved: true,
                update_requires_reauthorization: true,
                entry: hook.entry.clone(),
            })
        })
        .collect()
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

pub fn parse_plugin_manifest_v2(raw: &str) -> Result<NormalizedPluginManifestV2, ExtensionError> {
    if raw.len() > MAX_MANIFEST_BYTES {
        return Err(error(
            ExtensionErrorCode::InvalidSchema,
            "plugin manifest v2 exceeds size limit",
        ));
    }
    let value = kiana_domain::parse_bounded_json(raw.as_bytes()).map_err(|reason| {
        error(
            ExtensionErrorCode::InvalidSchema,
            format!("plugin manifest v2 JSON invalid:{reason}"),
        )
    })?;
    let source_digest = json_digest(&value);
    let strict: StrictPluginManifestV2 = serde_json::from_value(value).map_err(|_| {
        error(
            ExtensionErrorCode::InvalidSchema,
            "plugin manifest v2 fields invalid",
        )
    })?;
    if strict.schema != NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA
        || !valid_identifier(&strict.plugin_id, 128)
        || !valid_identifier(&strict.publisher, 128)
        || !valid_identifier(&strict.version, 64)
        || strict.license.trim().is_empty()
        || strict.license.len() > 256
        || strict.source.trim().is_empty()
        || strict.source.len() > 4_096
        || strict.components.is_empty()
        || strict.components.len() > 64
        || strict.required_dependencies.len() > 64
        || strict.optional_dependencies.len() > 64
        || strict.migration_refs.len() > 32
    {
        return Err(error(
            ExtensionErrorCode::InvalidIdentity,
            "plugin manifest v2 identity or bounds invalid",
        ));
    }
    let mut component_ids = BTreeSet::new();
    for component in &strict.components {
        if !valid_identifier(&component.id, 128)
            || !valid_entry(&component.entry)
            || !component_ids.insert(component.id.clone())
        {
            return Err(error(
                ExtensionErrorCode::DuplicateIdentity,
                "plugin manifest v2 component identity invalid",
            ));
        }
    }
    if strict
        .required_dependencies
        .keys()
        .chain(strict.optional_dependencies.keys())
        .any(|key| !valid_identifier(key, 128))
    {
        return Err(error(
            ExtensionErrorCode::InvalidIdentity,
            "plugin manifest v2 dependency identity invalid",
        ));
    }
    if strict.migration_refs.iter().any(|path| !valid_entry(path)) {
        return Err(error(
            ExtensionErrorCode::InvalidIdentity,
            "plugin manifest v2 migration reference invalid",
        ));
    }
    Ok(NormalizedPluginManifestV2 {
        schema: NORMALIZED_PLUGIN_MANIFEST_V2_SCHEMA.to_owned(),
        plugin_id: strict.plugin_id.clone(),
        publisher: strict.publisher.clone(),
        version: strict.version,
        license: strict.license,
        source: strict.source,
        namespace: format!("plugin:{}:{}", strict.publisher, strict.plugin_id),
        components: strict.components,
        required_dependencies: strict.required_dependencies,
        optional_dependencies: strict.optional_dependencies,
        configuration_schema: strict.configuration_schema,
        state_schema: strict.state_schema,
        migration_refs: strict.migration_refs,
        source_digest,
    })
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
    let descriptors = normalize_hook_descriptors(&hooks, &source_digest)?;
    Ok(NormalizedHookManifest {
        schema: NORMALIZED_HOOK_MANIFEST_SCHEMA.to_owned(),
        hooks,
        descriptors,
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
