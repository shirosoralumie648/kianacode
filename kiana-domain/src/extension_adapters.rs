//! Fail-closed component adapter metadata for signed local extensions.
//!
//! This module is deliberately a catalog boundary, not an execution engine. A daemon creates
//! [`VerifiedExtensionComponent`] values only after package, signature, trust and dependency
//! checks; the broker may then re-check the binding before dispatch. No type in this module
//! loads an entry, spawns a process, opens a socket, or grants a capability.

use crate::{
    is_hex_bytes, is_sha256_hex, json_digest, valid_extension_identifier, valid_extension_path,
    ExtensionEffect, ExtensionLifecyclePhase, ExtensionNetworkPolicy, ExtensionPackage,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const EXTENSION_ADAPTER_DESCRIPTOR_SCHEMA: &str = "kiana.extension-adapter-descriptor.v1";
pub const EXTENSION_ADAPTER_REGISTRY_SCHEMA: &str = "kiana.extension-adapter-registry.v1";
pub const EXTENSION_ADAPTER_BINDING_SCHEMA: &str = "kiana.extension-adapter-binding.v1";
pub const EXTENSION_ADAPTER_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EXTENSION_ADAPTERS: usize = 256;
pub const MAX_UNSUPPORTED_CAPABILITIES: usize = 32;

pub const REASON_SIGNATURE_UNVERIFIED: &str = "signature_unverified";
pub const REASON_SOURCE_UNTRUSTED: &str = "source_untrusted";
pub const REASON_ENTRY_MISSING: &str = "entry_missing_from_verified_package";
pub const REASON_ENTRY_NOT_DECLARATIVE: &str = "native_or_executable_entry_denied";
pub const REASON_DEPENDENCY_UNSATISFIED: &str = "dependency_unsatisfied";
pub const REASON_NETWORK_UNAVAILABLE: &str = "network_adapter_unavailable";
pub const REASON_APPROVAL_REQUIRED: &str = "approval_required";
pub const REASON_LIFECYCLE_NOT_ENABLED: &str = "lifecycle_not_enabled";
pub const REASON_LIFECYCLE_DISABLED: &str = "lifecycle_disabled";
pub const REASON_LIFECYCLE_REVOKED: &str = "lifecycle_revoked";
pub const REASON_LIFECYCLE_UNINSTALLED: &str = "lifecycle_uninstalled";
pub const REASON_BINDING_REQUIRED: &str = "verified_binding_required";
pub const REASON_COMPONENT_UNSUPPORTED: &str = "component_kind_not_supported";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionComponentKind {
    Skill,
    Hook,
    Mcp,
    Capability,
    Workflow,
    Memory,
    Provider,
    Ui,
}

impl ExtensionComponentKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "skill" | "skills" => Some(Self::Skill),
            "hook" | "hooks" => Some(Self::Hook),
            "mcp" | "connector" | "connectors" => Some(Self::Mcp),
            "capability" | "capabilities" => Some(Self::Capability),
            "workflow" | "workflows" => Some(Self::Workflow),
            "memory" | "memories" => Some(Self::Memory),
            "provider" | "providers" => Some(Self::Provider),
            "ui" | "desktop" | "web" => Some(Self::Ui),
            _ => None,
        }
    }

    pub const fn is_reviewed(self) -> bool {
        matches!(self, Self::Skill | Self::Hook | Self::Mcp)
    }

    pub const fn capability_name(self) -> &'static str {
        match self {
            Self::Skill => "skill.context.read",
            Self::Hook => "hook.controlled.invoke",
            Self::Mcp => "mcp.declarative.stdio",
            Self::Capability => "capability.adapter",
            Self::Workflow => "workflow.adapter",
            Self::Memory => "memory.adapter",
            Self::Provider => "provider.adapter",
            Self::Ui => "ui.adapter",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionAdapterStatus {
    Available,
    RequiresApproval,
    NotSupported,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionAdapterBinding {
    pub schema: String,
    pub version: SchemaVersion,
    /// The daemon's verified package bytes hash (unprefixed lowercase SHA-256).
    pub package_sha256: String,
    pub manifest_digest: String,
    pub source_digest: String,
    pub snapshot_digest: String,
    pub binding_digest: String,
    pub registry_generation: u64,
    pub lifecycle_revision: u64,
    pub phase: ExtensionLifecyclePhase,
    #[serde(default)]
    pub approval_digest: Option<String>,
}

impl ExtensionAdapterBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        package_sha256: impl Into<String>,
        manifest_digest: impl Into<String>,
        source_digest: impl Into<String>,
        snapshot_digest: impl Into<String>,
        registry_generation: u64,
        lifecycle_revision: u64,
        phase: ExtensionLifecyclePhase,
        approval_digest: Option<String>,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: EXTENSION_ADAPTER_BINDING_SCHEMA.to_owned(),
            version: EXTENSION_ADAPTER_VERSION,
            package_sha256: package_sha256.into(),
            manifest_digest: manifest_digest.into(),
            source_digest: source_digest.into(),
            snapshot_digest: snapshot_digest.into(),
            binding_digest: String::new(),
            registry_generation,
            lifecycle_revision,
            phase,
            approval_digest,
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_ADAPTER_BINDING_SCHEMA
            || self.version != EXTENSION_ADAPTER_VERSION
            || !is_sha256_hex(&self.package_sha256)
            || self.registry_generation == 0
            || self.lifecycle_revision == 0
            || !is_prefixed_digest(&self.manifest_digest)
            || !is_prefixed_digest(&self.source_digest)
            || !is_prefixed_digest(&self.snapshot_digest)
            || !is_prefixed_digest(&self.binding_digest)
            || self
                .approval_digest
                .as_deref()
                .is_some_and(|digest| !is_prefixed_digest(digest))
        {
            return Err("extension_adapter_binding_invalid".to_owned());
        }
        if self.binding_digest != self.digest() {
            return Err("extension_adapter_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "package_sha256": self.package_sha256,
            "manifest_digest": self.manifest_digest,
            "source_digest": self.source_digest,
            "snapshot_digest": self.snapshot_digest,
            "registry_generation": self.registry_generation,
            "lifecycle_revision": self.lifecycle_revision,
            "phase": self.phase,
            "approval_digest": self.approval_digest,
        }))
    }
}

fn is_prefixed_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_component_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b':'))
}

/// Evidence assembled by the daemon after the package, manifest, ProjectTrust and dependency
/// graph have been checked. The adapter never treats a manifest declaration as this evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedExtensionComponent {
    pub extension_id: String,
    pub component_id: String,
    pub kind: ExtensionComponentKind,
    pub entry: String,
    pub entry_digest: String,
    pub package_sha256: String,
    pub manifest_digest: String,
    pub source_digest: String,
    pub snapshot_digest: String,
    pub binding_digest: String,
    pub registry_generation: u64,
    pub lifecycle_revision: u64,
    pub phase: ExtensionLifecyclePhase,
    pub adapter_version: SchemaVersion,
    pub effect: ExtensionEffect,
    pub network_policy: ExtensionNetworkPolicy,
    pub required: bool,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub unsupported_capabilities: BTreeSet<String>,
    pub signature_verified: bool,
    pub source_trusted: bool,
    pub entry_present: bool,
    pub dependencies_satisfied: bool,
    #[serde(default)]
    pub approval_digest: Option<String>,
}

impl VerifiedExtensionComponent {
    #[allow(clippy::too_many_arguments)]
    pub fn from_verified_package(
        package: &ExtensionPackage,
        package_sha256: impl Into<String>,
        manifest_digest: impl Into<String>,
        source_digest: impl Into<String>,
        snapshot_digest: impl Into<String>,
        binding_digest: impl Into<String>,
        component_id: impl Into<String>,
        kind: ExtensionComponentKind,
        entry: impl Into<String>,
        registry_generation: u64,
        lifecycle_revision: u64,
        phase: ExtensionLifecyclePhase,
        effect: ExtensionEffect,
        network_policy: ExtensionNetworkPolicy,
        required: bool,
        required_capabilities: BTreeSet<String>,
        signature_verified: bool,
        source_trusted: bool,
        dependencies_satisfied: bool,
        approval_digest: Option<String>,
    ) -> Result<Self, String> {
        package.manifest.validate().map_err(str::to_owned)?;
        let entry = entry.into();
        let entry_present = package.files.contains_key(&entry);
        let entry_digest = package
            .files
            .get(&entry)
            .map(|content| json_digest(&json!({"entry": entry.clone(), "content_hex": content})))
            .unwrap_or_else(|| json_digest(&json!({"entry": entry.clone(), "content_hex": null})));
        let component = Self {
            extension_id: package.manifest.extension_id.clone(),
            component_id: component_id.into(),
            kind,
            entry,
            entry_digest,
            package_sha256: package_sha256.into(),
            manifest_digest: manifest_digest.into(),
            source_digest: source_digest.into(),
            snapshot_digest: snapshot_digest.into(),
            binding_digest: binding_digest.into(),
            registry_generation,
            lifecycle_revision,
            phase,
            adapter_version: EXTENSION_ADAPTER_VERSION,
            effect,
            network_policy,
            required,
            required_capabilities,
            unsupported_capabilities: BTreeSet::new(),
            signature_verified,
            source_trusted,
            entry_present,
            dependencies_satisfied,
            approval_digest,
        };
        component.validate()?;
        Ok(component)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !valid_extension_identifier(&self.extension_id)
            || !valid_component_identifier(&self.component_id)
            || !valid_extension_path(&self.entry)
            || self.adapter_version != EXTENSION_ADAPTER_VERSION
            || !is_sha256_hex(&self.package_sha256)
            || !is_prefixed_digest(&self.manifest_digest)
            || !is_prefixed_digest(&self.source_digest)
            || !is_prefixed_digest(&self.snapshot_digest)
            || !is_prefixed_digest(&self.binding_digest)
            || !is_prefixed_digest(&self.entry_digest)
            || self.registry_generation == 0
            || self.lifecycle_revision == 0
            || self.required_capabilities.len() > MAX_UNSUPPORTED_CAPABILITIES
            || self.unsupported_capabilities.len() > MAX_UNSUPPORTED_CAPABILITIES
        {
            return Err("verified_extension_component_invalid".to_owned());
        }
        if self
            .approval_digest
            .as_deref()
            .is_some_and(|digest| !is_prefixed_digest(digest))
        {
            return Err("verified_extension_component_approval_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionAdapterDescriptor {
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub component_id: String,
    pub kind: ExtensionComponentKind,
    pub entry: String,
    pub entry_digest: String,
    pub status: ExtensionAdapterStatus,
    pub reason: String,
    pub effect: ExtensionEffect,
    pub network_policy_digest: String,
    pub package_sha256: String,
    pub manifest_digest: String,
    pub source_digest: String,
    pub snapshot_digest: String,
    pub binding_digest: String,
    pub registry_generation: u64,
    pub lifecycle_revision: u64,
    pub adapter_version: SchemaVersion,
    pub required: bool,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub unsupported_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub approval_digest: Option<String>,
    pub descriptor_digest: String,
}

impl ExtensionAdapterDescriptor {
    /// Compatibility constructor for callers that only have declarations. Declarations are
    /// never authority, so this intentionally produces a denied descriptor.
    pub fn new(
        extension_id: impl Into<String>,
        component_id: impl Into<String>,
        kind: ExtensionComponentKind,
        entry: impl Into<String>,
        _signed: bool,
        _trusted: bool,
        effect: ExtensionEffect,
        network_policy: &ExtensionNetworkPolicy,
    ) -> Result<Self, String> {
        let extension_id = extension_id.into();
        let component_id = component_id.into();
        let entry = entry.into();
        let mut descriptor = Self {
            schema: EXTENSION_ADAPTER_DESCRIPTOR_SCHEMA.to_owned(),
            version: EXTENSION_ADAPTER_VERSION,
            extension_id,
            component_id,
            kind,
            entry: entry.clone(),
            entry_digest: json_digest(&json!({"entry": entry})),
            status: ExtensionAdapterStatus::Denied,
            reason: REASON_BINDING_REQUIRED.to_owned(),
            effect,
            network_policy_digest: json_digest(
                &serde_json::to_value(network_policy)
                    .map_err(|_| "extension_adapter_policy_serialize".to_owned())?,
            ),
            package_sha256: "00".repeat(32),
            manifest_digest: zero_digest(),
            source_digest: zero_digest(),
            snapshot_digest: zero_digest(),
            binding_digest: zero_digest(),
            registry_generation: 1,
            lifecycle_revision: 1,
            adapter_version: EXTENSION_ADAPTER_VERSION,
            required: false,
            required_capabilities: BTreeSet::new(),
            unsupported_capabilities: BTreeSet::from([kind.capability_name().to_owned()]),
            approval_digest: None,
            descriptor_digest: String::new(),
        };
        descriptor.descriptor_digest = descriptor.digest();
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn from_verified(component: VerifiedExtensionComponent) -> Result<Self, String> {
        component.validate()?;
        let mut unsupported_capabilities = component.unsupported_capabilities.clone();
        let (status, reason) = status_for(&component);
        if !component.kind.is_reviewed() {
            unsupported_capabilities.insert(component.kind.capability_name().to_owned());
        }
        let mut descriptor = Self {
            schema: EXTENSION_ADAPTER_DESCRIPTOR_SCHEMA.to_owned(),
            version: EXTENSION_ADAPTER_VERSION,
            extension_id: component.extension_id,
            component_id: component.component_id,
            kind: component.kind,
            entry: component.entry,
            entry_digest: component.entry_digest,
            status,
            reason: reason.to_owned(),
            effect: component.effect,
            network_policy_digest: json_digest(
                &serde_json::to_value(&component.network_policy)
                    .map_err(|_| "extension_adapter_policy_serialize".to_owned())?,
            ),
            package_sha256: component.package_sha256,
            manifest_digest: component.manifest_digest,
            source_digest: component.source_digest,
            snapshot_digest: component.snapshot_digest,
            binding_digest: component.binding_digest,
            registry_generation: component.registry_generation,
            lifecycle_revision: component.lifecycle_revision,
            adapter_version: component.adapter_version,
            required: component.required,
            required_capabilities: component.required_capabilities,
            unsupported_capabilities,
            approval_digest: component.approval_digest,
            descriptor_digest: String::new(),
        };
        descriptor.descriptor_digest = descriptor.digest();
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_ADAPTER_DESCRIPTOR_SCHEMA
            || self.version != EXTENSION_ADAPTER_VERSION
            || !valid_extension_identifier(&self.extension_id)
            || !valid_component_identifier(&self.component_id)
            || !valid_extension_path(&self.entry)
            || self.adapter_version != EXTENSION_ADAPTER_VERSION
            || self.reason.trim().is_empty()
            || self.reason.len() > 256
            || !is_sha256_hex(&self.package_sha256)
            || !is_prefixed_digest(&self.entry_digest)
            || !is_prefixed_digest(&self.network_policy_digest)
            || !is_prefixed_digest(&self.manifest_digest)
            || !is_prefixed_digest(&self.source_digest)
            || !is_prefixed_digest(&self.snapshot_digest)
            || !is_prefixed_digest(&self.binding_digest)
            || !is_prefixed_digest(&self.descriptor_digest)
            || self.registry_generation == 0
            || self.lifecycle_revision == 0
            || self.unsupported_capabilities.len() > MAX_UNSUPPORTED_CAPABILITIES
        {
            return Err("extension_adapter_descriptor_invalid".to_owned());
        }
        if self
            .approval_digest
            .as_deref()
            .is_some_and(|digest| !is_prefixed_digest(digest))
        {
            return Err("extension_adapter_approval_digest_invalid".to_owned());
        }
        let expected = expected_reason(self.kind, self.status, &self.reason);
        if expected.is_none() {
            return Err("extension_adapter_reason_invalid".to_owned());
        }
        if self.status == ExtensionAdapterStatus::Available
            && (!self.kind.is_reviewed() || self.reason == REASON_APPROVAL_REQUIRED)
        {
            return Err("extension_adapter_available_without_reviewed_contract".to_owned());
        }
        if self.descriptor_digest != self.digest() {
            return Err("extension_adapter_descriptor_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "component_id": self.component_id,
            "kind": self.kind,
            "entry": self.entry,
            "entry_digest": self.entry_digest,
            "status": self.status,
            "reason": self.reason,
            "effect": self.effect,
            "network_policy_digest": self.network_policy_digest,
            "package_sha256": self.package_sha256,
            "manifest_digest": self.manifest_digest,
            "source_digest": self.source_digest,
            "snapshot_digest": self.snapshot_digest,
            "binding_digest": self.binding_digest,
            "registry_generation": self.registry_generation,
            "lifecycle_revision": self.lifecycle_revision,
            "adapter_version": self.adapter_version,
            "required": self.required,
            "required_capabilities": self.required_capabilities,
            "unsupported_capabilities": self.unsupported_capabilities,
            "approval_digest": self.approval_digest,
        }))
    }
}

fn expected_reason(
    kind: ExtensionComponentKind,
    status: ExtensionAdapterStatus,
    reason: &str,
) -> Option<&'static str> {
    for (candidate, code) in [
        (REASON_SIGNATURE_UNVERIFIED, REASON_SIGNATURE_UNVERIFIED),
        (REASON_SOURCE_UNTRUSTED, REASON_SOURCE_UNTRUSTED),
        (REASON_ENTRY_MISSING, REASON_ENTRY_MISSING),
        (REASON_ENTRY_NOT_DECLARATIVE, REASON_ENTRY_NOT_DECLARATIVE),
        (REASON_DEPENDENCY_UNSATISFIED, REASON_DEPENDENCY_UNSATISFIED),
        (REASON_NETWORK_UNAVAILABLE, REASON_NETWORK_UNAVAILABLE),
        (REASON_APPROVAL_REQUIRED, REASON_APPROVAL_REQUIRED),
        (REASON_LIFECYCLE_NOT_ENABLED, REASON_LIFECYCLE_NOT_ENABLED),
        (REASON_LIFECYCLE_DISABLED, REASON_LIFECYCLE_DISABLED),
        (REASON_LIFECYCLE_REVOKED, REASON_LIFECYCLE_REVOKED),
        (REASON_LIFECYCLE_UNINSTALLED, REASON_LIFECYCLE_UNINSTALLED),
        (REASON_BINDING_REQUIRED, REASON_BINDING_REQUIRED),
    ] {
        if reason == candidate {
            return Some(code);
        }
    }
    if status == ExtensionAdapterStatus::NotSupported
        && kind.capability_name() != ""
        && reason == REASON_COMPONENT_UNSUPPORTED
    {
        return Some(REASON_COMPONENT_UNSUPPORTED);
    }
    match (kind, status, reason) {
        (
            ExtensionComponentKind::Skill,
            ExtensionAdapterStatus::Available,
            "skill_context_adapter",
        ) => Some("skill_context_adapter"),
        (
            ExtensionComponentKind::Hook,
            ExtensionAdapterStatus::Available,
            "controlled_hook_adapter",
        ) => Some("controlled_hook_adapter"),
        (
            ExtensionComponentKind::Mcp,
            ExtensionAdapterStatus::Available,
            "declarative_mcp_connector_adapter",
        ) => Some("declarative_mcp_connector_adapter"),
        (_, ExtensionAdapterStatus::RequiresApproval, REASON_LIFECYCLE_NOT_ENABLED) => {
            Some(REASON_LIFECYCLE_NOT_ENABLED)
        }
        (_, ExtensionAdapterStatus::RequiresApproval, REASON_APPROVAL_REQUIRED) => {
            Some(REASON_APPROVAL_REQUIRED)
        }
        (_, ExtensionAdapterStatus::NotSupported, REASON_COMPONENT_UNSUPPORTED) => {
            Some(REASON_COMPONENT_UNSUPPORTED)
        }
        (_, ExtensionAdapterStatus::Denied, REASON_BINDING_REQUIRED) => {
            Some(REASON_BINDING_REQUIRED)
        }
        _ => None,
    }
}

fn status_for(component: &VerifiedExtensionComponent) -> (ExtensionAdapterStatus, &'static str) {
    if !component.signature_verified {
        return (ExtensionAdapterStatus::Denied, REASON_SIGNATURE_UNVERIFIED);
    }
    if !component.source_trusted {
        return (ExtensionAdapterStatus::Denied, REASON_SOURCE_UNTRUSTED);
    }
    if !component.entry_present {
        return (ExtensionAdapterStatus::Denied, REASON_ENTRY_MISSING);
    }
    if !declarative_entry(&component.entry) {
        return (ExtensionAdapterStatus::Denied, REASON_ENTRY_NOT_DECLARATIVE);
    }
    if !component.dependencies_satisfied {
        return (
            ExtensionAdapterStatus::Denied,
            REASON_DEPENDENCY_UNSATISFIED,
        );
    }
    if !component.kind.is_reviewed() {
        return (
            ExtensionAdapterStatus::NotSupported,
            REASON_COMPONENT_UNSUPPORTED,
        );
    }
    match component.phase {
        ExtensionLifecyclePhase::Revoked | ExtensionLifecyclePhase::RolledBack => {
            return (ExtensionAdapterStatus::Denied, REASON_LIFECYCLE_REVOKED);
        }
        ExtensionLifecyclePhase::Uninstalled => {
            return (ExtensionAdapterStatus::Denied, REASON_LIFECYCLE_UNINSTALLED);
        }
        ExtensionLifecyclePhase::Disabled => {
            return (ExtensionAdapterStatus::Denied, REASON_LIFECYCLE_DISABLED);
        }
        ExtensionLifecyclePhase::Inspected | ExtensionLifecyclePhase::Staged => {
            return (
                ExtensionAdapterStatus::RequiresApproval,
                REASON_LIFECYCLE_NOT_ENABLED,
            );
        }
        ExtensionLifecyclePhase::Enabled => {}
    }
    if !matches!(component.network_policy, ExtensionNetworkPolicy::Deny) {
        return (ExtensionAdapterStatus::Denied, REASON_NETWORK_UNAVAILABLE);
    }
    if component.effect == ExtensionEffect::ReadWrite && component.approval_digest.is_none() {
        return (
            ExtensionAdapterStatus::RequiresApproval,
            REASON_APPROVAL_REQUIRED,
        );
    }
    (
        ExtensionAdapterStatus::Available,
        adapter_reason(component.kind),
    )
}

fn adapter_reason(kind: ExtensionComponentKind) -> &'static str {
    match kind {
        ExtensionComponentKind::Skill => "skill_context_adapter",
        ExtensionComponentKind::Hook => "controlled_hook_adapter",
        ExtensionComponentKind::Mcp => "declarative_mcp_connector_adapter",
        _ => REASON_COMPONENT_UNSUPPORTED,
    }
}

fn zero_digest() -> String {
    format!("sha256:{}", "00".repeat(32))
}

fn declarative_entry(entry: &str) -> bool {
    if !valid_extension_path(entry) {
        return false;
    }
    let lower = entry.to_ascii_lowercase();
    [
        ".exe", ".dll", ".dylib", ".so", ".bin", ".sh", ".bash", ".py", ".pl", ".rb",
    ]
    .iter()
    .all(|suffix| !lower.ends_with(suffix))
        && [".md", ".json", ".yaml", ".yml", ".toml"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionAdapterRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    pub generation: u64,
    pub authority_snapshot_digest: String,
    pub entries: Vec<ExtensionAdapterDescriptor>,
    pub registry_digest: String,
}

impl ExtensionAdapterRegistry {
    pub fn new(
        generation: u64,
        mut entries: Vec<ExtensionAdapterDescriptor>,
    ) -> Result<Self, String> {
        entries.sort_by(|left, right| {
            left.extension_id
                .cmp(&right.extension_id)
                .then_with(|| left.component_id.cmp(&right.component_id))
                .then_with(|| left.descriptor_digest.cmp(&right.descriptor_digest))
        });
        let authority_snapshot_digest = entries
            .first()
            .map(|entry| entry.snapshot_digest.clone())
            .unwrap_or_else(zero_digest);
        let mut registry = Self {
            schema: EXTENSION_ADAPTER_REGISTRY_SCHEMA.to_owned(),
            version: EXTENSION_ADAPTER_VERSION,
            generation,
            authority_snapshot_digest,
            entries,
            registry_digest: String::new(),
        };
        registry.registry_digest = registry.digest();
        registry.validate()?;
        Ok(registry)
    }

    pub fn find(
        &self,
        extension_id: &str,
        component_id: &str,
    ) -> Option<&ExtensionAdapterDescriptor> {
        self.entries
            .iter()
            .find(|entry| entry.extension_id == extension_id && entry.component_id == component_id)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_ADAPTER_REGISTRY_SCHEMA
            || self.version != EXTENSION_ADAPTER_VERSION
            || self.generation == 0
            || self.entries.len() > MAX_EXTENSION_ADAPTERS
            || !is_prefixed_digest(&self.authority_snapshot_digest)
        {
            return Err("extension_adapter_registry_header_invalid".to_owned());
        }
        let mut identities = BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if entry.registry_generation != self.generation {
                return Err("extension_adapter_registry_generation_mismatch".to_owned());
            }
            if entry.snapshot_digest != self.authority_snapshot_digest {
                return Err("extension_adapter_registry_snapshot_mismatch".to_owned());
            }
            if !identities.insert((entry.extension_id.clone(), entry.component_id.clone())) {
                return Err("extension_adapter_component_duplicate".to_owned());
            }
        }
        if !self.entries.windows(2).all(|pair| {
            (&pair[0].extension_id, &pair[0].component_id)
                <= (&pair[1].extension_id, &pair[1].component_id)
        }) {
            return Err("extension_adapter_registry_order_invalid".to_owned());
        }
        if !is_prefixed_digest(&self.registry_digest) || self.registry_digest != self.digest() {
            return Err("extension_adapter_registry_digest_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "generation": self.generation,
            "authority_snapshot_digest": self.authority_snapshot_digest,
            "entries": self.entries,
        }))
    }
}

/// Re-check the immutable binding immediately before an adapter request reaches the broker.
/// This is validation only; execution remains owned by `ExtensionAdmission` and the broker.
pub fn validate_adapter_binding(
    descriptor: &ExtensionAdapterDescriptor,
    package_sha256: &str,
    snapshot_digest: &str,
    registry_generation: u64,
    lifecycle_revision: u64,
) -> Result<(), String> {
    descriptor.validate()?;
    if descriptor.status != ExtensionAdapterStatus::Available {
        return Err(format!(
            "extension_adapter_not_available:{}",
            descriptor.reason
        ));
    }
    if descriptor.package_sha256 != package_sha256
        || descriptor.snapshot_digest != snapshot_digest
        || descriptor.registry_generation != registry_generation
        || descriptor.lifecycle_revision != lifecycle_revision
    {
        return Err("extension_adapter_binding_stale".to_owned());
    }
    Ok(())
}

/// Validate package entry bytes without executing them. The package format stores lowercase
/// hexadecimal bytes; this helper rejects malformed bytes and non-UTF-8 declarative resources.
pub fn validate_declarative_package_entry(
    package_files: &BTreeMap<String, String>,
    entry: &str,
) -> Result<String, String> {
    if !declarative_entry(entry) {
        return Err(REASON_ENTRY_NOT_DECLARATIVE.to_owned());
    }
    let content = package_files
        .get(entry)
        .ok_or_else(|| REASON_ENTRY_MISSING.to_owned())?;
    if content.len() % 2 != 0 || !is_hex_bytes(content, content.len() / 2) {
        return Err("entry_bytes_invalid".to_owned());
    }
    let bytes = (0..content.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&content[offset..offset + 2], 16))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "entry_bytes_invalid".to_owned())?;
    std::str::from_utf8(&bytes).map_err(|_| "entry_utf8_invalid".to_owned())?;
    Ok(json_digest(
        &json!({"entry": entry, "content_hex": content}),
    ))
}
