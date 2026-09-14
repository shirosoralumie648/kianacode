//! Signed local extension metadata. Declarations can only restrict execution.

use crate::{CapabilityKind, CapabilityRequest, RiskLevel, RoleSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const EXTENSION_MANIFEST_SCHEMA: &str = "kiana.extension-manifest.v1";
pub const EXTENSION_PACKAGE_SCHEMA: &str = "kiana.extension-package.v1";
pub const EXTENSION_MANAGE_OPERATION: &str = "extension.manage";
pub const EXTENSION_STREAM: &str = "extension_registry";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionEffect {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionType {
    Skill,
    Capability,
    Workflow,
    Memory,
    Provider,
    Ui,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtensionNetworkPolicy {
    Deny,
    Allowlist { hosts: BTreeSet<String> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSignature {
    pub algorithm: String,
    pub key_id: String,
    /// Lowercase hexadecimal Ed25519 signature over canonical unsigned manifest JSON.
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionRequires {
    /// Exact versions intentionally avoid accepting an unimplemented range language.
    pub kiana_version: String,
    pub protocol_version: String,
    #[serde(default)]
    pub capability_versions: BTreeMap<String, String>,
    #[serde(default)]
    pub policy_features: BTreeSet<String>,
    #[serde(default)]
    pub memory_collections: BTreeSet<String>,
    #[serde(default)]
    pub supported_platforms: BTreeSet<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionManifest {
    pub schema: String,
    pub extension_id: String,
    pub version: String,
    pub publisher: String,
    pub license: String,
    pub content_hash: String,
    pub signature: ExtensionSignature,
    pub extension_type: ExtensionType,
    pub effect: ExtensionEffect,
    #[serde(default)]
    pub provided_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    pub supported_roles: BTreeSet<String>,
    #[serde(default)]
    pub data_classes: BTreeSet<String>,
    pub network_policy: ExtensionNetworkPolicy,
    #[serde(default)]
    pub secret_refs: BTreeSet<String>,
    #[serde(default)]
    pub configuration_schema: Value,
    #[serde(default)]
    pub migration_ref: Option<String>,
    #[serde(default)]
    pub rollback_ref: Option<String>,
    pub requires: ExtensionRequires,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionPackage {
    pub schema: String,
    pub manifest: ExtensionManifest,
    /// Relative POSIX paths to lowercase hexadecimal bytes. No archive extraction occurs.
    pub files: BTreeMap<String, String>,
}

impl ExtensionManifest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != EXTENSION_MANIFEST_SCHEMA
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.version)
            || self.license.trim().is_empty()
            || self.license.len() > 256
            || !is_sha256_hex(&self.content_hash)
        {
            return Err("extension_manifest_invalid");
        }
        if self.signature.algorithm != "ed25519"
            || !valid_extension_identifier(&self.signature.key_id)
            || !is_hex_bytes(&self.signature.value, 64)
        {
            return Err("extension_signature_invalid");
        }
        if self.supported_roles.is_empty()
            || self
                .supported_roles
                .iter()
                .any(|role| RoleSpec::lookup(role).is_none())
            || self.required_capabilities.len() > 64
            || self.provided_capabilities.len() > 64
            || self.requires.extensions.len() > 64
            || self
                .required_capabilities
                .iter()
                .chain(self.provided_capabilities.iter())
                .any(|value| !valid_extension_identifier(value))
        {
            return Err("extension_capability_declaration_invalid");
        }
        if let ExtensionNetworkPolicy::Allowlist { hosts } = &self.network_policy {
            if hosts.is_empty()
                || hosts.len() > 32
                || hosts.iter().any(|host| {
                    host.is_empty()
                        || host.len() > 253
                        || host.starts_with('.')
                        || host.ends_with('.')
                        || host.contains("..")
                        || !host.bytes().all(|b| {
                            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-')
                        })
                })
            {
                return Err("extension_network_policy_invalid");
            }
        }
        for path in [&self.migration_ref, &self.rollback_ref]
            .into_iter()
            .flatten()
        {
            if !valid_extension_path(path) {
                return Err("extension_reference_invalid");
            }
        }
        Ok(())
    }

    /// This canonical representation is the only signed message. The signature object is
    /// excluded, while content hash, publisher, effects and all requirements remain bound.
    pub fn signing_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        value
            .as_object_mut()
            .expect("manifest is an object")
            .remove("signature");
        serde_json::to_vec(&canonical_json(value))
    }

    pub fn capability_diff(&self, previous: Option<&Self>) -> Value {
        let empty = BTreeSet::new();
        let old = previous.map(|p| &p.required_capabilities).unwrap_or(&empty);
        serde_json::json!({
            "added": self.required_capabilities.difference(old).collect::<Vec<_>>(),
            "removed": old.difference(&self.required_capabilities).collect::<Vec<_>>(),
            "effect_before": previous.map(|p| p.effect), "effect_after": self.effect,
            "network_before": previous.map(|p| &p.network_policy), "network_after": self.network_policy,
            "license_before": previous.map(|p| &p.license), "license_after": self.license,
            "secret_refs_before": previous.map(|p| &p.secret_refs), "secret_refs_after": self.secret_refs,
        })
    }
}

/// Host-owned registration contract, never inferred from model arguments/frontmatter.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionExecutionScope {
    pub extension_id: String,
    pub package_sha256: String,
    pub content_hash: String,
    pub effect: ExtensionEffect,
    pub required_capabilities: BTreeSet<String>,
    pub network_policy: ExtensionNetworkPolicy,
    pub supported_roles: BTreeSet<String>,
}

impl ExtensionExecutionScope {
    pub fn contract(&self, handler_effect: ExtensionEffect) -> ExtensionExecutionContract {
        ExtensionExecutionContract {
            extension_id: self.extension_id.clone(),
            package_sha256: self.package_sha256.clone(),
            content_hash: self.content_hash.clone(),
            effect: self.effect,
            required_capabilities: self.required_capabilities.clone(),
            network_policy: self.network_policy.clone(),
            supported_roles: self.supported_roles.clone(),
            handler_effect,
        }
    }
}

pub fn request_extension_scopes(
    request: &CapabilityRequest,
) -> Result<Option<Vec<ExtensionExecutionScope>>, &'static str> {
    request
        .arguments
        .get("_extension_scopes")
        .map(|value| {
            let scopes: Vec<ExtensionExecutionScope> =
                serde_json::from_value(value.clone()).map_err(|_| "extension_scope_invalid")?;
            if scopes.len() > 64 {
                return Err("extension_scope_limit_exceeded");
            }
            Ok(scopes)
        })
        .transpose()
}

#[derive(Clone, Debug)]
pub struct ExtensionExecutionContract {
    pub extension_id: String,
    pub package_sha256: String,
    pub content_hash: String,
    pub effect: ExtensionEffect,
    pub required_capabilities: BTreeSet<String>,
    pub network_policy: ExtensionNetworkPolicy,
    pub supported_roles: BTreeSet<String>,
    /// Classification of the concrete host adapter, separate from a caller's risk label.
    pub handler_effect: ExtensionEffect,
}

impl ExtensionExecutionContract {
    pub fn check(&self, request: &CapabilityRequest) -> Result<(), &'static str> {
        if !valid_extension_identifier(&self.extension_id)
            || !is_sha256_hex(&self.content_hash)
            || !is_sha256_hex(&self.package_sha256)
        {
            return Err("extension_execution_contract_invalid");
        }
        if self.effect == ExtensionEffect::ReadOnly
            && (self.handler_effect != ExtensionEffect::ReadOnly
                || request.risk != RiskLevel::ReadOnly
                || matches!(
                    request.capability,
                    CapabilityKind::Process | CapabilityKind::Secret | CapabilityKind::Computer
                ))
        {
            return Err("extension_read_only_write_denied");
        }
        if !self.required_capabilities.contains(&request.operation) {
            return Err("extension_required_capability_missing");
        }
        if request
            .arguments
            .get("role_id")
            .and_then(Value::as_str)
            .is_none_or(|role| !self.supported_roles.contains(role))
        {
            return Err("extension_role_denied");
        }
        if request.capability == CapabilityKind::Network {
            match &self.network_policy {
                ExtensionNetworkPolicy::Deny => return Err("extension_network_denied"),
                // Actual network confinement belongs to a reviewed host adapter. Until that
                // adapter is implemented, a declared host list cannot enable networking.
                ExtensionNetworkPolicy::Allowlist { .. } => {
                    return Err("extension_network_adapter_unavailable")
                }
            }
        }
        Ok(())
    }
}

pub fn canonical_json(value: Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, canonical_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(canonical_json).collect()),
        other => other,
    }
}

pub fn valid_extension_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

pub fn valid_extension_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 512
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.chars().any(char::is_control)
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

pub fn is_sha256_hex(value: &str) -> bool {
    is_hex_bytes(value, 32)
}

pub fn is_hex_bytes(value: &str, length: usize) -> bool {
    value.len() == length * 2
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
