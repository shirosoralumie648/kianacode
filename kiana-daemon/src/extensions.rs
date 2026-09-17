//! Approved, signed local packages. EventLog is the activation authority; files are cache.

use crate::local_packages::{decode_hex, failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler, ExtensionAdmission};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, ExtensionExecutionContract,
    ExtensionManifest, ExtensionPackage, ExtensionType, PromptAuthority, PromptSection, RequestId,
    RuntimeEvent, EXTENSION_MANAGE_OPERATION, EXTENSION_PACKAGE_SCHEMA, EXTENSION_STREAM,
};
use kiana_ports::{EventStorePort, PortError};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const MAX_PACKAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
const REGISTRY_SCHEMA: &str = "kiana.extension-lifecycle.v1";

#[derive(Clone)]
pub(crate) struct ExtensionRegistry {
    events: Arc<dyn EventStorePort>,
    cache_root: PathBuf,
    /// Only daemon startup configuration can add a trusted publisher/key pair.
    trusted_keys: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledExtension {
    manifest: ExtensionManifest,
    package_sha256: String,
    state: String,
    installed_by: String,
    source_request_id: RequestId,
}

#[derive(Clone)]
struct VerifiedPackage {
    package: ExtensionPackage,
    bytes: Vec<u8>,
    package_sha256: String,
}

impl ExtensionRegistry {
    pub(crate) fn from_env(events: Arc<dyn EventStorePort>) -> Result<Arc<Self>, PortError> {
        let log_path = kiana_eventlog::default_sessions_log_path()?;
        let root = log_path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| failed("extension_home_unavailable"))?
            .join("extensions")
            .join("packages");
        let keys = match std::env::var("KIANA_EXTENSION_TRUSTED_KEYS_JSON") {
            Ok(value) if value.len() <= 64 * 1024 => {
                serde_json::from_str::<BTreeMap<String, BTreeMap<String, String>>>(&value)
                    .map_err(|_| failed("extension_trusted_keys_invalid"))?
            }
            Ok(_) => return Err(failed("extension_trusted_keys_too_large")),
            Err(std::env::VarError::NotPresent) => BTreeMap::new(),
            Err(_) => return Err(failed("extension_trusted_keys_invalid")),
        };
        if keys.len() > 64
            || keys.iter().any(|(publisher, keys)| {
                !kiana_domain::valid_extension_identifier(publisher)
                    || keys.len() > 16
                    || keys.iter().any(|(id, key)| {
                        !kiana_domain::valid_extension_identifier(id)
                            || !kiana_domain::is_hex_bytes(key, 32)
                    })
            })
        {
            return Err(failed("extension_trusted_keys_invalid"));
        }
        Ok(Arc::new(Self {
            events,
            cache_root: root,
            trusted_keys: keys,
        }))
    }

    pub(crate) fn register(
        self: &Arc<Self>,
        broker: &mut CapabilityBroker,
    ) -> Result<(), PortError> {
        broker.set_extension_admission(self.clone());
        broker.register_static(
            CapabilityKind::Filesystem,
            EXTENSION_MANAGE_OPERATION,
            self.clone(),
        )
    }

    async fn stream(&self, project_root: &str) -> Result<(String, Vec<RuntimeEvent>), PortError> {
        let project_root = canonical_project(project_root)?;
        // Package/trust authority must not live under the project it controls.
        if self.cache_root.starts_with(&project_root) {
            return Err(failed("extension_home_inside_project"));
        }
        let scope = sha256(project_root.to_string_lossy().as_bytes());
        Ok((
            scope.clone(),
            self.events.read_stream(EXTENSION_STREAM, &scope).await?,
        ))
    }

    async fn manage(&self, request: &AuthorizedCapabilityRequest) -> Result<Value, PortError> {
        let arguments = &request.request.arguments;
        let action = string(arguments, "action")?;
        let project_root = string(arguments, "project_root")?;
        let actor = string(arguments, "actor_id")?;
        if request.request.cell_id.is_some() || arguments["operator_authorized"] != true {
            return Err(failed("extension_operator_required"));
        }
        let (scope, history) = self.stream(project_root).await?;
        let (version, states) = fold_registry(&history)?;
        if action == "list" {
            return Ok(
                json!({"schema":REGISTRY_SCHEMA,"project_root":project_root,"registry_version":version,"extensions":states}),
            );
        }
        if action == "inspect" {
            let package = self.load_source(arguments).await?;
            let manifest = &package.package.manifest;
            self.compatibility(manifest, &states)?;
            return Ok(
                json!({"schema":REGISTRY_SCHEMA,"registry_version":version,"manifest":manifest,
                "package_sha256":package.package_sha256,"signature_verified":true,
                "capability_diff":manifest.capability_diff(states.get(&manifest.extension_id).map(|s| &s.manifest)),
                "activation":activation_state(manifest),"safety_validation":"not_evaluated"}),
            );
        }
        if !matches!(action, "install" | "upgrade" | "revoke" | "rollback") {
            return Err(failed("extension_action_invalid"));
        }
        if !matches!(
            request.request.risk,
            kiana_domain::RiskLevel::ExternalSideEffect | kiana_domain::RiskLevel::Critical
        ) {
            return Err(failed("extension_approval_risk_required"));
        }
        let key = string(arguments, "idempotency_key")?;
        if key.len() > 128 || key.chars().any(char::is_control) {
            return Err(failed("extension_idempotency_key_invalid"));
        }
        let fingerprint = sha256(
            &serde_json::to_vec(&kiana_domain::canonical_json(arguments.clone()))
                .map_err(|e| failed(e.to_string()))?,
        );
        let event_key = format!("extension:{scope}:{key}");
        if let Some(previous) = history
            .iter()
            .find(|event| event.idempotency_key.as_deref() == Some(&event_key))
        {
            if previous.data["request_fingerprint"] != fingerprint {
                return Err(PortError::Conflict(
                    "extension_idempotency_payload_mismatch".to_owned(),
                ));
            }
            let mut output = previous.data["receipt"].clone();
            output["replayed"] = json!(true);
            output["event_id"] = json!(previous.event_id);
            return Ok(output);
        }
        let expected = arguments["expected_registry_version"]
            .as_u64()
            .ok_or_else(|| failed("extension_expected_registry_version_required"))?;
        if expected != version {
            return Err(PortError::Conflict(
                "extension_registry_version_mismatch".to_owned(),
            ));
        }
        let reason = string(arguments, "reason")?;
        if reason.len() > 4096 {
            return Err(failed("extension_reason_too_large"));
        }
        let extension_id = string(arguments, "extension_id")?;
        if !kiana_domain::valid_extension_identifier(extension_id) {
            return Err(failed("extension_id_invalid"));
        }
        let current = states.get(extension_id);
        let mut migration_receipt = json!({"kind":"none"});
        let (next, package) = match action {
            "revoke" => {
                let current = current.ok_or_else(|| failed("extension_not_installed"))?;
                if current.state == "revoked" {
                    return Err(failed("extension_already_revoked"));
                }
                let mut next = current.clone();
                next.state = "revoked".to_owned();
                next.installed_by = actor.to_owned();
                next.source_request_id = request.request.request_id;
                (next, None)
            }
            "install" | "upgrade" | "rollback" => {
                let package = if action == "rollback" {
                    if current.is_none() {
                        return Err(failed("extension_not_installed"));
                    }
                    let target = string(arguments, "package_sha256")?;
                    if !history.iter().any(|e| {
                        e.data["state"]["manifest"]["extension_id"] == extension_id
                            && e.data["state"]["package_sha256"] == target
                            && e.data["action"] != "revoke"
                    }) {
                        return Err(failed("extension_rollback_snapshot_missing"));
                    }
                    self.load_cached(target).await?
                } else {
                    self.load_source(arguments).await?
                };
                if arguments["package_sha256"] != package.package_sha256 {
                    return Err(failed("extension_final_package_hash_mismatch"));
                }
                let manifest = &package.package.manifest;
                if manifest.extension_id != extension_id {
                    return Err(failed("extension_package_identity_mismatch"));
                }
                if action == "install" && current.is_some() {
                    return Err(failed("extension_already_installed"));
                }
                if action == "upgrade" && current.is_none() {
                    return Err(failed("extension_not_installed"));
                }
                if let Some(current) = current {
                    if current.manifest.publisher != manifest.publisher {
                        return Err(failed("extension_publisher_change_denied"));
                    }
                    if current.package_sha256 == package.package_sha256 {
                        return Err(failed("extension_version_unchanged"));
                    }
                }
                if history.iter().any(|e| {
                    e.data["state"]["manifest"]["extension_id"] == extension_id
                        && e.data["state"]["manifest"]["version"] == manifest.version
                        && e.data["state"]["package_sha256"] != package.package_sha256
                }) {
                    return Err(failed("extension_version_content_conflict"));
                }
                if history.iter().any(|e| {
                    e.data["action"] == "revoke"
                        && e.data["state"]["package_sha256"] == package.package_sha256
                }) {
                    return Err(failed("extension_package_revoked"));
                }
                self.compatibility(manifest, &states)?;
                if let Some(reference) = &manifest.migration_ref {
                    let migration = read_stateless_migration(&package.package, reference)?;
                    if action != "rollback"
                        && (migration.from_version.as_deref()
                            != current.map(|state| state.manifest.version.as_str())
                            || migration.to_version != manifest.version)
                    {
                        return Err(failed("extension_migration_version_mismatch"));
                    }
                    migration_receipt = json!({"kind":"stateless","reference":reference,
                        "from_version":current.map(|state|&state.manifest.version),"to_version":manifest.version,
                        "content_hash":sha256(&decode_hex(&package.package.files[reference])?),"scripts_executed":false});
                }
                let next = InstalledExtension {
                    manifest: manifest.clone(),
                    package_sha256: package.package_sha256.clone(),
                    state: activation_state(manifest).to_owned(),
                    installed_by: actor.to_owned(),
                    source_request_id: request.request.request_id,
                };
                (next, Some(package))
            }
            _ => unreachable!(),
        };
        // Dependencies are checked against the prospective whole registry, including revoke
        // and rollback. An installed dependent never silently points at an absent version.
        let mut prospective = states.clone();
        prospective.insert(extension_id.to_owned(), next.clone());
        if prospective
            .values()
            .filter(|state| state.state == "enabled")
            .count()
            > 64
        {
            return Err(failed("extension_active_limit_exceeded"));
        }
        for state in prospective
            .values()
            .filter(|state| state.state != "revoked")
        {
            self.compatibility(&state.manifest, &prospective)?;
        }
        if let Some(package) = package {
            let root = self.cache_root.clone();
            tokio::task::spawn_blocking(move || {
                LocalDir::open(&root, true)?
                    .publish(&format!("{}.json", package.package_sha256), &package.bytes)
            })
            .await
            .map_err(|e| failed(format!("extension_cache_join_failed:{e}")))??;
        }
        let next_version = version
            .checked_add(1)
            .ok_or_else(|| failed("extension_registry_version_exhausted"))?;
        let receipt = json!({"schema":REGISTRY_SCHEMA,"action":action,"extension_id":extension_id,
            "project_root":project_root,"registry_version":next_version,"state":next.state,
            "package_sha256":next.package_sha256,"content_hash":next.manifest.content_hash,
            "version":next.manifest.version,"previous_version":current.map(|s| &s.manifest.version),
            "previous_package_sha256":current.map(|s| &s.package_sha256),"license":next.manifest.license,
            "signature":next.manifest.signature,"signature_verified":action != "revoke",
            "capability_diff":next.manifest.capability_diff(current.map(|s| &s.manifest)),
            "migration":migration_receipt,"safety_validation":"not_evaluated","reason":kiana_domain::redact_text(reason),"actor_id":actor,
            "authorization_id":request.authorization_id,"replayed":false});
        let event = RuntimeEvent::new(
            request.request.request_id,
            1,
            "extension.lifecycle",
            json!({
                "schema":REGISTRY_SCHEMA,"action":action,"project_root":project_root,"state":next,
                "request_fingerprint":fingerprint,"receipt":receipt,
            }),
        )
        .map_err(|e| failed(e.to_string()))?
        .with_stream_metadata(EXTENSION_STREAM, &scope, next_version)
        .with_idempotency_key(event_key);
        // The CAS event is the activation itself. A cache write without this event is inert.
        let appended = self
            .events
            .append_idempotent_expected(event, Some(version))
            .await
            .map_err(|e| match e {
                PortError::Conflict(_) => e,
                _ => failed(format!(
                    "result_unknown:extension_activation_event_failed:{e}"
                )),
            })?;
        let mut output = appended.event.data["receipt"].clone();
        output["event_id"] = json!(appended.event.event_id);
        Ok(output)
    }

    async fn load_source(&self, arguments: &Value) -> Result<VerifiedPackage, PortError> {
        let project = canonical_project(string(arguments, "project_root")?)?;
        let path = string(arguments, "package_path")?.to_owned();
        if !kiana_domain::valid_extension_path(&path) {
            return Err(failed("extension_package_path_must_be_project_relative"));
        }
        let registry = self.clone();
        tokio::task::spawn_blocking(move || {
            registry.verify(LocalDir::open(&project, false)?.read(&path, MAX_PACKAGE_BYTES)?)
        })
        .await
        .map_err(|e| failed(format!("extension_package_join_failed:{e}")))?
    }

    async fn load_cached(&self, digest: &str) -> Result<VerifiedPackage, PortError> {
        if !kiana_domain::is_sha256_hex(digest) {
            return Err(failed("extension_package_hash_invalid"));
        }
        let digest = digest.to_owned();
        let registry = self.clone();
        tokio::task::spawn_blocking(move || {
            let bytes = LocalDir::open(&registry.cache_root, false)?
                .read(&format!("{digest}.json"), MAX_PACKAGE_BYTES)?;
            if sha256(&bytes) != digest {
                return Err(failed("extension_cache_hash_mismatch"));
            }
            registry.verify(bytes)
        })
        .await
        .map_err(|e| failed(format!("extension_cache_join_failed:{e}")))?
    }

    fn verify(&self, bytes: Vec<u8>) -> Result<VerifiedPackage, PortError> {
        if bytes.len() > MAX_PACKAGE_BYTES {
            return Err(failed("extension_package_too_large"));
        }
        let package: ExtensionPackage =
            serde_json::from_slice(&bytes).map_err(|_| failed("extension_package_invalid"))?;
        if package.schema != EXTENSION_PACKAGE_SCHEMA
            || package.files.is_empty()
            || package.files.len() > 128
        {
            return Err(failed("extension_package_invalid"));
        }
        let manifest = &package.manifest;
        manifest.validate().map_err(failed)?;
        verify_manifest_signature(manifest, &self.trusted_keys)?;
        let mut digest = Sha256::new();
        digest.update(b"kiana.extension-content.v1\0");
        let mut size = 0usize;
        for (path, encoded) in &package.files {
            if !kiana_domain::valid_extension_path(path) {
                return Err(failed("extension_package_path_invalid"));
            }
            let content = decode_hex(encoded)?;
            size = size
                .checked_add(content.len())
                .ok_or_else(|| failed("extension_content_too_large"))?;
            if size > MAX_CONTENT_BYTES {
                return Err(failed("extension_content_too_large"));
            }
            digest.update((path.len() as u64).to_le_bytes());
            digest.update(path.as_bytes());
            digest.update((content.len() as u64).to_le_bytes());
            digest.update(&content);
        }
        if format!("{:x}", digest.finalize()) != manifest.content_hash {
            return Err(failed("extension_content_hash_mismatch"));
        }
        for path in [&manifest.migration_ref, &manifest.rollback_ref]
            .into_iter()
            .flatten()
        {
            if !package.files.contains_key(path) {
                return Err(failed("extension_reference_missing"));
            }
            read_stateless_migration(&package, path)?;
        }
        if manifest.extension_type == ExtensionType::Skill {
            let text = package
                .files
                .get("SKILL.md")
                .ok_or_else(|| failed("extension_skill_missing"))?;
            let text = decode_hex(text)?;
            if text.len() > 64 * 1024 || std::str::from_utf8(&text).is_err() {
                return Err(failed("extension_skill_invalid"));
            }
        }
        Ok(VerifiedPackage {
            package,
            package_sha256: sha256(&bytes),
            bytes,
        })
    }

    fn compatibility(
        &self,
        manifest: &ExtensionManifest,
        installed: &BTreeMap<String, InstalledExtension>,
    ) -> Result<(), PortError> {
        let requires = &manifest.requires;
        if requires.kiana_version != env!("CARGO_PKG_VERSION")
            || requires.protocol_version != kiana_protocol::PROTOCOL_SCHEMA
        {
            return Err(failed("extension_runtime_version_incompatible"));
        }
        let supported = [
            "shell.exec",
            "apply_patch",
            "mcp.call",
            "memory.search",
            "memory.write",
        ];
        if manifest
            .required_capabilities
            .iter()
            .any(|cap| !supported.contains(&cap.as_str()))
            || requires
                .capability_versions
                .iter()
                .any(|(cap, version)| !supported.contains(&cap.as_str()) || version != "1")
        {
            return Err(failed("extension_required_capability_unavailable"));
        }
        let policy_features = [
            "control_plane",
            "approval",
            "project_trust",
            "five_tools",
            "extension_effect",
            "local_only",
        ];
        if requires
            .policy_features
            .iter()
            .any(|feature| !policy_features.contains(&feature.as_str()))
        {
            return Err(failed("extension_policy_feature_unavailable"));
        }
        if !requires.supported_platforms.is_empty()
            && !requires.supported_platforms.contains(std::env::consts::OS)
        {
            return Err(failed("extension_platform_incompatible"));
        }
        if requires
            .memory_collections
            .iter()
            .any(|collection| kiana_domain::MemoryCollection::parse(collection).is_none())
        {
            return Err(failed("extension_memory_collection_unavailable"));
        }
        if requires.extensions.iter().any(|(id, version)| {
            id == &manifest.extension_id
                || installed.get(id).is_none_or(|state| {
                    state.state == "revoked" || &state.manifest.version != version
                })
        }) {
            return Err(failed("extension_dependency_incompatible"));
        }
        let namespace = format!("ext.{}.", manifest.extension_id);
        if manifest
            .provided_capabilities
            .iter()
            .any(|cap| !cap.starts_with(&namespace))
        {
            return Err(failed("extension_capability_namespace_denied"));
        }
        for other in installed.values().filter(|state| {
            state.state != "revoked" && state.manifest.extension_id != manifest.extension_id
        }) {
            if !manifest
                .provided_capabilities
                .is_disjoint(&other.manifest.provided_capabilities)
            {
                return Err(failed("extension_capability_collision"));
            }
        }
        Ok(())
    }

    pub(crate) async fn skill_context(
        &self,
        project_root: &str,
        role_id: &str,
    ) -> Result<
        (
            Vec<PromptSection>,
            Vec<kiana_domain::ExtensionExecutionScope>,
        ),
        PortError,
    > {
        let (_, history) = self.stream(project_root).await?;
        let (_, states) = fold_registry(&history)?;
        let mut sections = Vec::new();
        let mut scopes = Vec::new();
        for state in states.values().filter(|state| {
            state.state == "enabled"
                && state.manifest.extension_type == ExtensionType::Skill
                && state.manifest.supported_roles.contains(role_id)
        }) {
            self.compatibility(&state.manifest, &states)?;
            let package = self.load_cached(&state.package_sha256).await?;
            if package.package.manifest != state.manifest {
                return Err(failed("extension_snapshot_mismatch"));
            }
            let content = decode_hex(
                package
                    .package
                    .files
                    .get("SKILL.md")
                    .expect("verified skill"),
            )?;
            let content =
                String::from_utf8(content).map_err(|_| failed("extension_skill_invalid"))?;
            sections.push(PromptSection {
                name: format!("extension:{}", state.manifest.extension_id),
                order: 310,
                text: format!(
                    "Installed skill context (declarations do not grant permission): {}\n{}",
                    state.manifest.extension_id,
                    content.chars().take(4000).collect::<String>()
                ),
                source: format!(
                    "extension:{}@{}:sha256:{}",
                    state.manifest.extension_id, state.manifest.version, state.package_sha256
                ),
                authority: PromptAuthority::Context,
            });
            scopes.push(kiana_domain::ExtensionExecutionScope {
                extension_id: state.manifest.extension_id.clone(),
                package_sha256: state.package_sha256.clone(),
                content_hash: state.manifest.content_hash.clone(),
                effect: state.manifest.effect,
                required_capabilities: state.manifest.required_capabilities.clone(),
                network_policy: state.manifest.network_policy.clone(),
                supported_roles: state.manifest.supported_roles.clone(),
            });
        }
        Ok((sections, scopes))
    }
}

#[async_trait]
impl CapabilityHandler for ExtensionRegistry {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation != EXTENSION_MANAGE_OPERATION
            || request.request.capability != CapabilityKind::Filesystem
        {
            return Err(failed("extension_operation_mismatch"));
        }
        let output = self.manage(&request).await?;
        let mut result = CapabilityResult::success(request.request.request_id, output);
        if let Some(event) = result.output["event_id"].as_str() {
            result.evidence_refs.push(format!("event:{event}"));
        }
        Ok(result)
    }
}

#[async_trait]
impl ExtensionAdmission for ExtensionRegistry {
    async fn check(
        &self,
        request: &AuthorizedCapabilityRequest,
        contract: &ExtensionExecutionContract,
    ) -> Result<(), PortError> {
        let (_, history) = self
            .stream(string(&request.request.arguments, "project_root")?)
            .await?;
        let (_, states) = fold_registry(&history)?;
        let state = states
            .get(&contract.extension_id)
            .ok_or_else(|| failed("extension_not_installed"))?;
        if state.state != "enabled"
            || state.package_sha256 != contract.package_sha256
            || state.manifest.content_hash != contract.content_hash
            || state.manifest.effect != contract.effect
            || state.manifest.required_capabilities != contract.required_capabilities
            || state.manifest.network_policy != contract.network_policy
            || state.manifest.supported_roles != contract.supported_roles
        {
            return Err(failed("extension_execution_snapshot_inactive"));
        }
        let package = self.load_cached(&state.package_sha256).await?;
        if package.package.manifest != state.manifest {
            return Err(failed("extension_snapshot_mismatch"));
        }
        self.compatibility(&state.manifest, &states)
    }
}

fn fold_registry(
    events: &[RuntimeEvent],
) -> Result<(u64, BTreeMap<String, InstalledExtension>), PortError> {
    let mut version = 0;
    let mut states = BTreeMap::new();
    for event in events {
        if event.kind != "extension.lifecycle"
            || event.data["schema"] != REGISTRY_SCHEMA
            || event.stream_version != Some(version + 1)
        {
            return Err(failed("extension_registry_event_invalid"));
        }
        let state: InstalledExtension = serde_json::from_value(event.data["state"].clone())
            .map_err(|_| failed("extension_registry_event_invalid"))?;
        state.manifest.validate().map_err(failed)?;
        if !matches!(state.state.as_str(), "enabled" | "staged" | "revoked")
            || !kiana_domain::is_sha256_hex(&state.package_sha256)
        {
            return Err(failed("extension_registry_event_invalid"));
        }
        states.insert(state.manifest.extension_id.clone(), state);
        version += 1;
    }
    Ok((version, states))
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StatelessMigration {
    schema: String,
    kind: String,
    from_version: Option<String>,
    to_version: String,
}
fn read_stateless_migration(
    package: &ExtensionPackage,
    reference: &str,
) -> Result<StatelessMigration, PortError> {
    let encoded = package
        .files
        .get(reference)
        .ok_or_else(|| failed("extension_reference_missing"))?;
    let migration: StatelessMigration = serde_json::from_slice(&decode_hex(encoded)?)
        .map_err(|_| failed("extension_migration_declaration_invalid"))?;
    if migration.schema != "kiana.extension-migration.v1"
        || migration.kind != "stateless"
        || migration.to_version.trim().is_empty()
        || package.manifest.extension_type != ExtensionType::Skill
    {
        return Err(failed("extension_migration_adapter_unavailable"));
    }
    Ok(migration)
}

fn activation_state(manifest: &ExtensionManifest) -> &'static str {
    if manifest.extension_type == ExtensionType::Skill {
        "enabled"
    } else {
        "staged"
    }
}

fn verify_manifest_signature(
    manifest: &ExtensionManifest,
    trusted_keys: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<(), PortError> {
    let key = trusted_keys
        .get(&manifest.publisher)
        .and_then(|keys| keys.get(&manifest.signature.key_id))
        .ok_or_else(|| failed("extension_publisher_key_untrusted"))?;
    UnparsedPublicKey::new(&ED25519, decode_hex(key)?)
        .verify(
            &manifest
                .signing_bytes()
                .map_err(|_| failed("extension_manifest_invalid"))?,
            &decode_hex(&manifest.signature.value)?,
        )
        .map_err(|_| failed("extension_signature_verification_failed"))
}

fn canonical_project(value: &str) -> Result<PathBuf, PortError> {
    let path = Path::new(value)
        .canonicalize()
        .map_err(|e| failed(format!("extension_project_invalid:{e}")))?;
    if !path.is_dir() {
        return Err(failed("extension_project_invalid"));
    }
    Ok(path)
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, PortError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| failed(format!("extension_argument_required:{key}")))
}

#[cfg(test)]
mod tests {
    use super::verify_manifest_signature;
    use kiana_domain::{
        ExtensionEffect, ExtensionManifest, ExtensionNetworkPolicy, ExtensionRequires,
        ExtensionSignature, ExtensionType, EXTENSION_MANIFEST_SCHEMA,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use std::collections::{BTreeMap, BTreeSet};

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn manifest(signature: String) -> ExtensionManifest {
        ExtensionManifest {
            schema: EXTENSION_MANIFEST_SCHEMA.to_owned(),
            extension_id: "review-skill".to_owned(),
            version: "1.0.0".to_owned(),
            publisher: "fixture-publisher".to_owned(),
            license: "MIT".to_owned(),
            content_hash: "a".repeat(64),
            signature: ExtensionSignature {
                algorithm: "ed25519".to_owned(),
                key_id: "fixture-key".to_owned(),
                value: signature,
            },
            extension_type: ExtensionType::Skill,
            effect: ExtensionEffect::ReadOnly,
            provided_capabilities: BTreeSet::new(),
            required_capabilities: BTreeSet::from(["memory.search".to_owned()]),
            supported_roles: BTreeSet::from(["builder".to_owned()]),
            data_classes: BTreeSet::new(),
            network_policy: ExtensionNetworkPolicy::Deny,
            secret_refs: BTreeSet::new(),
            configuration_schema: serde_json::json!({"type": "object"}),
            migration_ref: None,
            rollback_ref: None,
            requires: ExtensionRequires {
                kiana_version: env!("CARGO_PKG_VERSION").to_owned(),
                protocol_version: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
                capability_versions: BTreeMap::new(),
                policy_features: BTreeSet::new(),
                memory_collections: BTreeSet::new(),
                supported_platforms: BTreeSet::new(),
                extensions: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn extension_signature_is_verified_before_install() {
        let key_pair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).expect("seed");
        let mut unsigned = manifest(String::new());
        let signature = key_pair.sign(&unsigned.signing_bytes().expect("signing bytes"));
        unsigned.signature.value = hex(signature.as_ref());
        unsigned.validate().expect("fixture manifest");

        let trusted = BTreeMap::from([(
            "fixture-publisher".to_owned(),
            BTreeMap::from([(
                "fixture-key".to_owned(),
                hex(key_pair.public_key().as_ref()),
            )]),
        )]);
        assert!(verify_manifest_signature(&unsigned, &trusted).is_ok());

        let mut forged = unsigned.clone();
        forged.version = "2.0.0".to_owned();
        assert_eq!(
            verify_manifest_signature(&forged, &trusted)
                .expect_err("manifest drift must fail closed")
                .to_string(),
            "extension_signature_verification_failed"
        );
        let mut untrusted = trusted;
        untrusted.remove("fixture-publisher");
        assert_eq!(
            verify_manifest_signature(&unsigned, &untrusted)
                .expect_err("unknown publisher must fail closed")
                .to_string(),
            "extension_publisher_key_untrusted"
        );
    }
}
