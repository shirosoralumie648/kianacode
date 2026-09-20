//! Signed local extension metadata. Declarations can only restrict execution.

use crate::{
    json_digest, CapabilityKind, CapabilityRequest, RiskLevel, RoleSpec, SchemaVersion, ScopeLimit,
    ScopeSet,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
    pub schema: String,
    pub version: SchemaVersion,
    pub extension_id: String,
    pub publisher: String,
    pub package_sha256: String,
    pub content_hash: String,
    pub registry_generation: u64,
    pub role_id: String,
    pub effect: ExtensionEffect,
    pub required_capabilities: BTreeSet<String>,
    pub network_policy: ExtensionNetworkPolicy,
    pub supported_roles: BTreeSet<String>,
    pub capability_diff_digest: String,
    pub network_policy_digest: String,
    pub secret_policy_digest: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub scope_digest: String,
}

impl ExtensionExecutionScope {
    pub const SCHEMA: &'static str = "kiana.extension-execution-scope.v2";
    pub const VERSION: SchemaVersion = SchemaVersion::new(2, 0);

    pub fn new(
        manifest: &ExtensionManifest,
        package_sha256: impl Into<String>,
        registry_generation: u64,
        role_id: impl Into<String>,
        issued_at_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        manifest.validate().map_err(str::to_owned)?;
        let mut scope = Self {
            schema: Self::SCHEMA.to_owned(),
            version: Self::VERSION,
            extension_id: manifest.extension_id.clone(),
            publisher: manifest.publisher.clone(),
            package_sha256: package_sha256.into(),
            content_hash: manifest.content_hash.clone(),
            registry_generation,
            role_id: role_id.into(),
            effect: manifest.effect,
            required_capabilities: manifest.required_capabilities.clone(),
            network_policy: manifest.network_policy.clone(),
            supported_roles: manifest.supported_roles.clone(),
            capability_diff_digest: json_digest(&manifest.capability_diff(None)),
            network_policy_digest: json_digest(
                &serde_json::to_value(&manifest.network_policy)
                    .map_err(|_| "extension_scope_policy_serialize".to_owned())?,
            ),
            secret_policy_digest: json_digest(&json!({
                "secret_refs": manifest.secret_refs,
            })),
            issued_at_unix_ms,
            expires_at_unix_ms,
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != Self::SCHEMA
            || self.version != Self::VERSION
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.role_id)
            || self.registry_generation == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || !is_sha256_hex(&self.package_sha256)
            || !is_sha256_hex(&self.content_hash)
        {
            return Err("extension_scope_header_invalid".to_owned());
        }
        if !self.supported_roles.contains(&self.role_id) {
            return Err("extension_scope_role_not_supported".to_owned());
        }
        for (value, field) in [
            (
                &self.capability_diff_digest,
                "extension_scope_capability_diff_digest",
            ),
            (
                &self.network_policy_digest,
                "extension_scope_network_policy_digest",
            ),
            (
                &self.secret_policy_digest,
                "extension_scope_secret_policy_digest",
            ),
            (&self.scope_digest, "extension_scope_digest"),
        ] {
            if !value.starts_with("sha256:")
                || value.len() != 71
                || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!("{field}_invalid"));
            }
        }
        if self.scope_digest != self.digest() {
            return Err("extension_scope_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_at(
        &self,
        now_unix_ms: u64,
        registry_generation: u64,
        role_id: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("extension_scope_expired".to_owned());
        }
        if self.registry_generation != registry_generation {
            return Err("extension_scope_generation_stale".to_owned());
        }
        if self.role_id != role_id {
            return Err("extension_scope_role_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn matches_manifest(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        manifest.validate().map_err(str::to_owned)?;
        if self.extension_id != manifest.extension_id
            || self.publisher != manifest.publisher
            || self.content_hash != manifest.content_hash
            || self.effect != manifest.effect
            || self.required_capabilities != manifest.required_capabilities
            || self.network_policy != manifest.network_policy
            || self.supported_roles != manifest.supported_roles
            || self.capability_diff_digest != json_digest(&manifest.capability_diff(None))
            || self.network_policy_digest
                != json_digest(
                    &serde_json::to_value(&manifest.network_policy)
                        .map_err(|_| "extension_scope_policy_serialize".to_owned())?,
                )
            || self.secret_policy_digest
                != json_digest(&json!({"secret_refs": manifest.secret_refs}))
        {
            return Err("extension_scope_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn contract(&self, handler_effect: ExtensionEffect) -> ExtensionExecutionContract {
        ExtensionExecutionContract {
            extension_id: self.extension_id.clone(),
            publisher: self.publisher.clone(),
            package_sha256: self.package_sha256.clone(),
            content_hash: self.content_hash.clone(),
            registry_generation: self.registry_generation,
            role_id: self.role_id.clone(),
            effect: self.effect,
            required_capabilities: self.required_capabilities.clone(),
            network_policy: self.network_policy.clone(),
            supported_roles: self.supported_roles.clone(),
            capability_diff_digest: self.capability_diff_digest.clone(),
            network_policy_digest: self.network_policy_digest.clone(),
            secret_policy_digest: self.secret_policy_digest.clone(),
            issued_at_unix_ms: self.issued_at_unix_ms,
            expires_at_unix_ms: self.expires_at_unix_ms,
            scope_digest: self.scope_digest.clone(),
            handler_effect,
        }
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "extension_id": self.extension_id,
            "publisher": self.publisher,
            "package_sha256": self.package_sha256,
            "content_hash": self.content_hash,
            "registry_generation": self.registry_generation,
            "role_id": self.role_id,
            "effect": self.effect,
            "required_capabilities": self.required_capabilities,
            "network_policy": self.network_policy,
            "supported_roles": self.supported_roles,
            "capability_diff_digest": self.capability_diff_digest,
            "network_policy_digest": self.network_policy_digest,
            "secret_policy_digest": self.secret_policy_digest,
            "issued_at_unix_ms": self.issued_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
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
            for scope in &scopes {
                scope.validate().map_err(|_| "extension_scope_invalid")?;
            }
            Ok(scopes)
        })
        .transpose()
}

#[derive(Clone, Debug)]
pub struct ExtensionExecutionContract {
    pub extension_id: String,
    pub publisher: String,
    pub package_sha256: String,
    pub content_hash: String,
    pub registry_generation: u64,
    pub role_id: String,
    pub effect: ExtensionEffect,
    pub required_capabilities: BTreeSet<String>,
    pub network_policy: ExtensionNetworkPolicy,
    pub supported_roles: BTreeSet<String>,
    pub capability_diff_digest: String,
    pub network_policy_digest: String,
    pub secret_policy_digest: String,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub scope_digest: String,
    /// Classification of the concrete host adapter, separate from a caller's risk label.
    pub handler_effect: ExtensionEffect,
}

impl ExtensionExecutionContract {
    pub fn validate_at(
        &self,
        now_unix_ms: u64,
        registry_generation: u64,
        role_id: &str,
    ) -> Result<(), String> {
        if now_unix_ms >= self.expires_at_unix_ms {
            return Err("extension_scope_expired".to_owned());
        }
        if self.registry_generation != registry_generation {
            return Err("extension_scope_generation_stale".to_owned());
        }
        if self.role_id != role_id {
            return Err("extension_scope_role_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn matches_manifest(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        manifest.validate().map_err(str::to_owned)?;
        if self.extension_id != manifest.extension_id
            || self.publisher != manifest.publisher
            || self.content_hash != manifest.content_hash
            || self.effect != manifest.effect
            || self.required_capabilities != manifest.required_capabilities
            || self.network_policy != manifest.network_policy
            || self.supported_roles != manifest.supported_roles
            || self.capability_diff_digest != json_digest(&manifest.capability_diff(None))
            || self.network_policy_digest
                != json_digest(
                    &serde_json::to_value(&manifest.network_policy)
                        .map_err(|_| "extension_scope_policy_serialize".to_owned())?,
                )
            || self.secret_policy_digest
                != json_digest(&json!({"secret_refs": manifest.secret_refs}))
        {
            return Err("extension_scope_policy_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn check(&self, request: &CapabilityRequest) -> Result<(), &'static str> {
        if !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.publisher)
            || !valid_extension_identifier(&self.role_id)
            || self.registry_generation == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || !is_sha256_hex(&self.content_hash)
            || !is_sha256_hex(&self.package_sha256)
            || !self.scope_digest.starts_with("sha256:")
            || self.scope_digest.len() != 71
        {
            return Err("extension_execution_contract_invalid");
        }
        if request.arguments.get("role_id").and_then(Value::as_str) != Some(self.role_id.as_str()) {
            return Err("extension_scope_role_mismatch");
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

// Dependency resolution and binding are inert metadata. A declaration never grants capability;
// the ControlPlane and Broker remain the only effect-authorizing boundaries.
pub const EXTENSION_DEPENDENCY_GRAPH_SCHEMA: &str = "kiana.extension-dependency-graph.v1";
pub const EXTENSION_GRAPH_RESOLUTION_SCHEMA: &str = "kiana.extension-graph-resolution.v1";
pub const EXTENSION_BINDING_SCHEMA: &str = "kiana.extension-binding.v1";
pub const EXTENSION_BINDING_SNAPSHOT_SCHEMA: &str = "kiana.extension-binding-snapshot.v1";
pub const EXTENSION_DEPENDENCY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_EXTENSION_GRAPH_NODES: usize = 256;
pub const MAX_EXTENSION_DEPENDENCIES: usize = 64;
pub const MAX_EXTENSION_BINDINGS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionGraphKind {
    Skill,
    Hook,
    Mcp,
    Capability,
    Memory,
    Provider,
    UiComponent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionDependency {
    pub dependency_id: String,
    pub version: String,
    pub kind: ExtensionGraphKind,
    #[serde(default)]
    pub optional: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_scope: Option<ScopeSet>,
}

impl ExtensionDependency {
    fn validate(&self) -> Result<(), String> {
        if !valid_extension_identifier(&self.dependency_id)
            || !valid_extension_identifier(&self.version)
        {
            return Err("extension_dependency_identity_invalid".to_owned());
        }
        if let Some(scope) = &self.required_scope {
            scope.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionGraphNode {
    pub node_id: String,
    pub extension_id: String,
    pub version: String,
    pub kind: ExtensionGraphKind,
    pub source_digest: String,
    #[serde(default)]
    pub dependencies: Vec<ExtensionDependency>,
    #[serde(default)]
    pub supported_platforms: BTreeSet<String>,
    pub scope: ScopeSet,
}

impl ExtensionGraphNode {
    fn validate(&self) -> Result<(), String> {
        if !valid_extension_identifier(&self.node_id)
            || !valid_extension_identifier(&self.extension_id)
            || !valid_extension_identifier(&self.version)
            || !is_sha256_hex(&self.source_digest)
            || self.dependencies.len() > MAX_EXTENSION_DEPENDENCIES
            || self.supported_platforms.len() > 16
        {
            return Err("extension_graph_node_invalid".to_owned());
        }
        self.scope.validate()?;
        for platform in &self.supported_platforms {
            if !valid_extension_identifier(platform) {
                return Err("extension_graph_platform_invalid".to_owned());
            }
        }
        for dependency in &self.dependencies {
            dependency.validate()?;
        }
        if self.dependencies.windows(2).any(|pair| {
            (&pair[0].dependency_id, &pair[0].version, pair[0].kind)
                > (&pair[1].dependency_id, &pair[1].version, pair[1].kind)
        }) {
            return Err("extension_dependency_order_invalid".to_owned());
        }
        if self
            .dependencies
            .windows(2)
            .any(|pair| pair[0].dependency_id == pair[1].dependency_id)
        {
            return Err("extension_dependency_duplicate".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionDependencyGraph {
    pub schema: String,
    pub version: SchemaVersion,
    pub platform: String,
    pub nodes: BTreeMap<String, ExtensionGraphNode>,
    pub graph_digest: String,
}

impl ExtensionDependencyGraph {
    pub fn new(
        platform: impl Into<String>,
        nodes: Vec<ExtensionGraphNode>,
    ) -> Result<Self, String> {
        let mut indexed = BTreeMap::new();
        for mut node in nodes {
            node.dependencies.sort_by(|left, right| {
                (&left.dependency_id, &left.version, left.kind).cmp(&(
                    &right.dependency_id,
                    &right.version,
                    right.kind,
                ))
            });
            node.validate()?;
            if indexed.insert(node.node_id.clone(), node).is_some() {
                return Err("extension_graph_node_duplicate".to_owned());
            }
        }
        let mut graph = Self {
            schema: EXTENSION_DEPENDENCY_GRAPH_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            platform: platform.into(),
            nodes: indexed,
            graph_digest: String::new(),
        };
        graph.graph_digest = graph.digest();
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_DEPENDENCY_GRAPH_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !valid_extension_identifier(&self.platform)
            || self.nodes.len() > MAX_EXTENSION_GRAPH_NODES
            || !is_prefixed_digest(&self.graph_digest)
        {
            return Err("extension_dependency_graph_header_invalid".to_owned());
        }
        for node in self.nodes.values() {
            node.validate()?;
            if !node.supported_platforms.is_empty()
                && !node.supported_platforms.contains(&self.platform)
            {
                return Err("extension_dependency_platform_mismatch".to_owned());
            }
            for dependency in &node.dependencies {
                let Some(target) = self.nodes.get(&dependency.dependency_id) else {
                    if dependency.optional {
                        continue;
                    }
                    return Err("extension_dependency_unsatisfied".to_owned());
                };
                if target.version != dependency.version {
                    return Err("extension_dependency_version_unavailable".to_owned());
                }
                if target.kind != dependency.kind {
                    return Err("extension_dependency_kind_mismatch".to_owned());
                }
                if !target.supported_platforms.is_empty()
                    && !target.supported_platforms.contains(&self.platform)
                {
                    return Err("extension_dependency_platform_mismatch".to_owned());
                }
                node.scope
                    .intersect(&target.scope)
                    .map_err(|_| "extension_dependency_scope_disjoint".to_owned())?;
                if let Some(required_scope) = &dependency.required_scope {
                    required_scope
                        .intersect(&node.scope)
                        .map_err(|_| "extension_dependency_scope_disjoint".to_owned())?;
                    required_scope
                        .intersect(&target.scope)
                        .map_err(|_| "extension_dependency_scope_disjoint".to_owned())?;
                }
            }
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        for node_id in self.nodes.keys() {
            visit_extension_node(node_id, &self.nodes, &mut visiting, &mut visited)?;
        }
        if self.graph_digest != self.digest() {
            return Err("extension_dependency_graph_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn resolve(&self) -> Result<ExtensionGraphResolution, String> {
        self.validate()?;
        let mut resolved_dependencies = Vec::new();
        for node in self.nodes.values() {
            for dependency in &node.dependencies {
                if let Some(target) = self.nodes.get(&dependency.dependency_id) {
                    resolved_dependencies.push(ResolvedExtensionDependency {
                        source_node_id: node.node_id.clone(),
                        target_node_id: target.node_id.clone(),
                        version: target.version.clone(),
                        kind: target.kind,
                        required_scope_digest: dependency
                            .required_scope
                            .as_ref()
                            .map(ScopeSet::digest),
                    });
                }
            }
        }
        resolved_dependencies.sort_by(|left, right| {
            (
                &left.source_node_id,
                &left.target_node_id,
                &left.version,
                left.kind,
            )
                .cmp(&(
                    &right.source_node_id,
                    &right.target_node_id,
                    &right.version,
                    right.kind,
                ))
        });
        let mut resolution = ExtensionGraphResolution {
            schema: EXTENSION_GRAPH_RESOLUTION_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            graph_digest: self.graph_digest.clone(),
            platform: self.platform.clone(),
            resolved_dependencies,
            resolution_digest: String::new(),
        };
        resolution.resolution_digest = resolution.digest();
        resolution.validate()?;
        Ok(resolution)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "platform": self.platform,
            "nodes": self.nodes,
        }))
    }
}

fn visit_extension_node(
    node_id: &str,
    nodes: &BTreeMap<String, ExtensionGraphNode>,
    visiting: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), String> {
    if visited.contains(node_id) {
        return Ok(());
    }
    if !visiting.insert(node_id.to_owned()) {
        return Err("extension_dependency_cycle".to_owned());
    }
    if let Some(node) = nodes.get(node_id) {
        for dependency in &node.dependencies {
            if nodes.contains_key(&dependency.dependency_id) {
                visit_extension_node(&dependency.dependency_id, nodes, visiting, visited)?;
            }
        }
    }
    visiting.remove(node_id);
    visited.insert(node_id.to_owned());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedExtensionDependency {
    pub source_node_id: String,
    pub target_node_id: String,
    pub version: String,
    pub kind: ExtensionGraphKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_scope_digest: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionGraphResolution {
    pub schema: String,
    pub version: SchemaVersion,
    pub graph_digest: String,
    pub platform: String,
    pub resolved_dependencies: Vec<ResolvedExtensionDependency>,
    pub resolution_digest: String,
}

impl ExtensionGraphResolution {
    fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_GRAPH_RESOLUTION_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !is_prefixed_digest(&self.graph_digest)
            || !valid_extension_identifier(&self.platform)
            || !is_prefixed_digest(&self.resolution_digest)
        {
            return Err("extension_graph_resolution_invalid".to_owned());
        }
        if self.resolved_dependencies.windows(2).any(|pair| {
            (
                &pair[0].source_node_id,
                &pair[0].target_node_id,
                &pair[0].version,
                pair[0].kind,
            ) > (
                &pair[1].source_node_id,
                &pair[1].target_node_id,
                &pair[1].version,
                pair[1].kind,
            )
        }) {
            return Err("extension_graph_resolution_order_invalid".to_owned());
        }
        for dependency in &self.resolved_dependencies {
            if !valid_extension_identifier(&dependency.source_node_id)
                || !valid_extension_identifier(&dependency.target_node_id)
                || !valid_extension_identifier(&dependency.version)
                || dependency
                    .required_scope_digest
                    .as_deref()
                    .is_some_and(|digest| !is_prefixed_digest(digest))
            {
                return Err("extension_graph_resolution_dependency_invalid".to_owned());
            }
        }
        if self.resolution_digest != self.digest() {
            return Err("extension_graph_resolution_digest_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "graph_digest": self.graph_digest,
            "platform": self.platform,
            "resolved_dependencies": self.resolved_dependencies,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub binding_id: String,
    pub snapshot_id: String,
    pub source_node_id: String,
    pub destination: String,
    pub packet_id: String,
    pub role_id: String,
    pub data_classes: BTreeSet<String>,
    pub network_policy: ExtensionNetworkPolicy,
    pub secret_handles: BTreeSet<String>,
    pub budget: ScopeLimit,
    pub expires_at_unix_ms: u64,
    pub parent_scope_digest: String,
    pub scope: ScopeSet,
    pub binding_digest: String,
}

impl ExtensionBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        binding_id: impl Into<String>,
        snapshot_id: impl Into<String>,
        source_node_id: impl Into<String>,
        destination: impl Into<String>,
        packet_id: impl Into<String>,
        role_id: impl Into<String>,
        data_classes: BTreeSet<String>,
        network_policy: ExtensionNetworkPolicy,
        secret_handles: BTreeSet<String>,
        budget: ScopeLimit,
        expires_at_unix_ms: u64,
        parent_scope: &ScopeSet,
        scope: ScopeSet,
    ) -> Result<Self, String> {
        parent_scope.validate()?;
        scope.validate()?;
        if !scope.is_subset_of(parent_scope)? {
            return Err("extension_binding_scope_widened".to_owned());
        }
        let mut binding = Self {
            schema: EXTENSION_BINDING_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            binding_id: binding_id.into(),
            snapshot_id: snapshot_id.into(),
            source_node_id: source_node_id.into(),
            destination: destination.into(),
            packet_id: packet_id.into(),
            role_id: role_id.into(),
            data_classes,
            network_policy,
            secret_handles,
            budget,
            expires_at_unix_ms,
            parent_scope_digest: parent_scope.digest(),
            scope,
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_BINDING_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !valid_extension_identifier(&self.binding_id)
            || !valid_extension_identifier(&self.snapshot_id)
            || !valid_extension_identifier(&self.source_node_id)
            || !valid_extension_identifier(&self.packet_id)
            || !valid_extension_identifier(&self.role_id)
            || self.destination.trim().is_empty()
            || self.destination.len() > 512
            || self.destination.contains('\0')
            || self.data_classes.len() > 32
            || self.secret_handles.len() > 32
            || self.expires_at_unix_ms == 0
            || matches!(self.budget, ScopeLimit::NotApplicable)
            || !is_prefixed_digest(&self.parent_scope_digest)
            || !is_prefixed_digest(&self.binding_digest)
        {
            return Err("extension_binding_invalid".to_owned());
        }
        validate_string_set(&self.data_classes, "extension_binding_data_class")?;
        validate_string_set(&self.secret_handles, "extension_binding_secret_handle")?;
        validate_network_policy(&self.network_policy)?;
        self.scope.validate()?;
        if self.binding_digest != self.digest() {
            return Err("extension_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn is_valid_against(
        &self,
        current_parent_scope: &ScopeSet,
        now_ms: u64,
    ) -> Result<(), String> {
        self.validate()?;
        current_parent_scope.validate()?;
        if self.parent_scope_digest != current_parent_scope.digest() {
            return Err("extension_binding_parent_scope_changed".to_owned());
        }
        if !self.scope.is_subset_of(current_parent_scope)? {
            return Err("extension_binding_scope_widened".to_owned());
        }
        if now_ms >= self.expires_at_unix_ms {
            return Err("extension_binding_expired".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "binding_id": self.binding_id,
            "snapshot_id": self.snapshot_id,
            "source_node_id": self.source_node_id,
            "destination": self.destination,
            "packet_id": self.packet_id,
            "role_id": self.role_id,
            "data_classes": self.data_classes,
            "network_policy": self.network_policy,
            "secret_handles": self.secret_handles,
            "budget": self.budget,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "parent_scope_digest": self.parent_scope_digest,
            "scope": self.scope,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionBindingSnapshot {
    pub schema: String,
    pub version: SchemaVersion,
    pub snapshot_id: String,
    pub graph_digest: String,
    pub resolution_digest: String,
    pub authority_epoch: u64,
    pub bindings: Vec<ExtensionBinding>,
    pub snapshot_digest: String,
}

impl ExtensionBindingSnapshot {
    pub fn new(
        graph: &ExtensionDependencyGraph,
        resolution: &ExtensionGraphResolution,
        snapshot_id: impl Into<String>,
        authority_epoch: u64,
        mut bindings: Vec<ExtensionBinding>,
    ) -> Result<Self, String> {
        graph.validate()?;
        resolution.validate()?;
        if resolution.graph_digest != graph.graph_digest {
            return Err("extension_binding_graph_changed".to_owned());
        }
        let snapshot_id = snapshot_id.into();
        if !valid_extension_identifier(&snapshot_id) || authority_epoch == 0 {
            return Err("extension_binding_snapshot_header_invalid".to_owned());
        }
        bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
        for binding in &bindings {
            binding.validate()?;
            let Some(parent) = graph.nodes.get(&binding.source_node_id) else {
                return Err("extension_binding_parent_missing".to_owned());
            };
            if binding.snapshot_id != snapshot_id
                || binding.parent_scope_digest != parent.scope.digest()
                || !binding.scope.is_subset_of(&parent.scope)?
            {
                return Err("extension_binding_snapshot_mismatch".to_owned());
            }
        }
        if bindings
            .windows(2)
            .any(|pair| pair[0].binding_id == pair[1].binding_id)
        {
            return Err("extension_binding_duplicate".to_owned());
        }
        let mut snapshot = Self {
            schema: EXTENSION_BINDING_SNAPSHOT_SCHEMA.to_owned(),
            version: EXTENSION_DEPENDENCY_VERSION,
            snapshot_id,
            graph_digest: graph.graph_digest.clone(),
            resolution_digest: resolution.resolution_digest.clone(),
            authority_epoch,
            bindings,
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_BINDING_SNAPSHOT_SCHEMA
            || self.version != EXTENSION_DEPENDENCY_VERSION
            || !valid_extension_identifier(&self.snapshot_id)
            || !is_prefixed_digest(&self.graph_digest)
            || !is_prefixed_digest(&self.resolution_digest)
            || self.authority_epoch == 0
            || self.bindings.len() > MAX_EXTENSION_BINDINGS
            || !is_prefixed_digest(&self.snapshot_digest)
        {
            return Err("extension_binding_snapshot_invalid".to_owned());
        }
        for binding in &self.bindings {
            binding.validate()?;
            if binding.snapshot_id != self.snapshot_id {
                return Err("extension_binding_snapshot_identity_mismatch".to_owned());
            }
        }
        if self
            .bindings
            .windows(2)
            .any(|pair| pair[0].binding_id >= pair[1].binding_id)
        {
            return Err("extension_binding_snapshot_order_invalid".to_owned());
        }
        if self.snapshot_digest != self.digest() {
            return Err("extension_binding_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn revalidate(&self, graph: &ExtensionDependencyGraph, now_ms: u64) -> Result<(), String> {
        self.validate()?;
        graph.validate()?;
        if self.graph_digest != graph.graph_digest {
            return Err("extension_binding_graph_changed".to_owned());
        }
        for binding in &self.bindings {
            let Some(parent) = graph.nodes.get(&binding.source_node_id) else {
                return Err("extension_binding_parent_missing".to_owned());
            };
            binding.is_valid_against(&parent.scope, now_ms)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "snapshot_id": self.snapshot_id,
            "graph_digest": self.graph_digest,
            "resolution_digest": self.resolution_digest,
            "authority_epoch": self.authority_epoch,
            "bindings": self.bindings,
        }))
    }
}

fn validate_string_set(values: &BTreeSet<String>, field: &str) -> Result<(), String> {
    if values.iter().any(|value| {
        value.trim().is_empty()
            || value.len() > 256
            || value.contains('\0')
            || value.chars().any(char::is_control)
    }) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_network_policy(policy: &ExtensionNetworkPolicy) -> Result<(), String> {
    if let ExtensionNetworkPolicy::Allowlist { hosts } = policy {
        if hosts.is_empty()
            || hosts.len() > 32
            || hosts.iter().any(|host| {
                host.is_empty()
                    || host.len() > 253
                    || host.starts_with('.')
                    || host.ends_with('.')
                    || host.contains("..")
                    || !host.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'.' | b'-')
                    })
            })
        {
            return Err("extension_binding_network_policy_invalid".to_owned());
        }
    }
    Ok(())
}

fn is_prefixed_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
