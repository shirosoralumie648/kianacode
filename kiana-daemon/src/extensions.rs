//! Approved, signed local packages. EventLog is the activation authority; files are cache.

use crate::local_packages::{decode_hex, failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler, ExtensionAdmission};
use kiana_domain::{
    json_digest, AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult,
    ExtensionAdapterRegistry, ExtensionComponentKind, ExtensionConfigurationSnapshot,
    ExtensionExecutionContract, ExtensionLifecyclePhase, ExtensionManifest, ExtensionPackage,
    ExtensionSnapshot, ExtensionSnapshotState, ExtensionStateMigrationPlan,
    ExtensionStateMigrationReceipt, ExtensionStateScope, ExtensionType, ExtensionVisibilityActionKind,
    ExtensionVisibilityEntry, ExtensionVisibilityKind, ExtensionVisibilityRisk,
    ExtensionVisibilitySnapshot, ExtensionVisibilitySource, ExtensionVisibilityStatus,
    ExtensionVisibilityTrust, PromptAuthority, PromptBudgetUsage, PromptSection, RequestId,
    RuntimeEvent, SkillPromptProvenance, SourceKind, SourceRef, VerifiedExtensionComponent,
    EXTENSION_MANAGE_OPERATION, EXTENSION_PACKAGE_SCHEMA, EXTENSION_STREAM,
    SKILL_PROMPT_PROVENANCE_SCHEMA,
};
use kiana_ports::{EventStorePort, PortError};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_PACKAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
const REGISTRY_SCHEMA: &str = "kiana.extension-lifecycle.v1";

#[derive(Clone)]
pub(crate) struct ExtensionRegistry {
    events: Arc<dyn EventStorePort>,
    cache_root: PathBuf,
    state_root: PathBuf,
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
        let extension_home = root
            .parent()
            .ok_or_else(|| failed("extension_home_unavailable"))?;
        let state_root = extension_home.join("state");
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
            state_root,
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
        // Mutable extension state is deliberately a different root from the immutable package
        // cache; neither root may be controlled by the extension project itself.
        if self.cache_root == self.state_root
            || self.cache_root.starts_with(&project_root)
            || self.state_root.starts_with(&project_root)
        {
            return Err(failed("extension_home_inside_project"));
        }
        let scope = sha256(project_root.to_string_lossy().as_bytes());
        Ok((
            scope.clone(),
            self.events.read_stream(EXTENSION_STREAM, &scope).await?,
        ))
    }

    fn state_scope(
        &self,
        project_root: &str,
        manifest: &ExtensionManifest,
    ) -> Result<ExtensionStateScope, PortError> {
        let project_root = canonical_project(project_root)?;
        let scope_digest = format!(
            "sha256:{}",
            sha256(project_root.to_string_lossy().as_bytes())
        );
        let namespace = format!(
            "{}/{}/{}",
            manifest.publisher,
            manifest.extension_id,
            scope_digest.trim_start_matches("sha256:")
        );
        ExtensionStateScope::new(
            &manifest.publisher,
            &manifest.extension_id,
            scope_digest,
            format!("{namespace}/state"),
            format!("{namespace}/cache"),
        )
        .map_err(failed)
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
        let role_id = arguments
            .get("role_id")
            .and_then(Value::as_str)
            .unwrap_or("builder");
        if action == "list" || action == "search" || (action == "inspect" && arguments["package_path"].is_null()) {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let max_results = arguments
                .get("max_results")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or(128);
            let snapshot = self
                .visibility_snapshot(project_root, role_id, query, max_results)
                .await?;
            if action == "inspect" {
                let extension_id = arguments
                    .get("extension_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| failed("extension_visibility_extension_id_required"))?;
                let entry = snapshot
                    .inspect(extension_id)
                    .map_err(failed)?
                    .ok_or_else(|| failed("extension_visibility_not_found"))?;
                return Ok(json!({
                    "schema": REGISTRY_SCHEMA,
                    "registry_version": version,
                    "snapshot": snapshot,
                    "extension": entry,
                }));
            }
            return Ok(json!({
                "schema": REGISTRY_SCHEMA,
                "registry_version": version,
                "snapshot": snapshot,
            }));
        }
        if action == "inspect" {
            let package = self.load_source(arguments).await?;
            let manifest = &package.package.manifest;
            self.compatibility(manifest, &states)?;
            let state_scope = self.state_scope(project_root, manifest)?;
            let configuration = parse_configuration_snapshot(
                arguments.get("configuration"),
                manifest,
                &state_scope,
            )?;
            let component_adapters = build_component_adapter_registry(&package, version.max(1))?;
            return Ok(
                json!({"schema":REGISTRY_SCHEMA,"registry_version":version,"manifest":manifest,
                "package_sha256":package.package_sha256,"signature_verified":true,
                "capability_diff":manifest.capability_diff(states.get(&manifest.extension_id).map(|s| &s.manifest)),
                "activation":activation_state(manifest),"state_scope_digest":state_scope.scope_contract_digest,
                "configuration_digest":configuration.as_ref().map(|snapshot| snapshot.config_digest.clone()),
                "component_adapters":component_adapters,
                "safety_validation":"not_evaluated"}),
            );
        }
        if !matches!(
            action,
            "install" | "upgrade" | "revoke" | "rollback" | "uninstall"
        ) {
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
            "uninstall" => {
                let current = current.ok_or_else(|| failed("extension_not_installed"))?;
                if matches!(current.state.as_str(), "revoked" | "uninstalled") {
                    return Err(failed("extension_already_uninstalled"));
                }
                let mut next = current.clone();
                // Uninstall is an append-only lifecycle fact. Cleanup is adapter-owned and is
                // represented by the receipt; the cache is not silently erased here.
                next.state = "uninstalled".to_owned();
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
                            && e.data["action"] != "uninstall"
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
                if action == "install" && current.is_some_and(|state| state.state != "uninstalled")
                {
                    return Err(failed("extension_already_installed"));
                }
                if action == "upgrade" && current.is_none_or(|state| state.state == "uninstalled") {
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
                    match read_migration_declaration(&package.package, reference)? {
                        MigrationDeclaration::Stateless(migration) => {
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
                        MigrationDeclaration::Controlled(plan) => {
                            let scope = self.state_scope(project_root, manifest)?;
                            let receipt = prepare_controlled_migration(manifest, &scope, &plan)?;
                            return Err(failed(format!(
                                "extension_state_migration_requires_controlled_action:{}",
                                receipt.reason
                            )));
                        }
                    }
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
            .filter(|state| !matches!(state.state.as_str(), "revoked" | "uninstalled"))
        {
            self.compatibility(&state.manifest, &prospective)?;
        }
        let state_scope = self.state_scope(project_root, &next.manifest)?;
        let configuration = parse_configuration_snapshot(
            arguments.get("configuration"),
            &next.manifest,
            &state_scope,
        )?;
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
            "signature":next.manifest.signature,"signature_verified":matches!(action, "install" | "upgrade" | "rollback"),
            "capability_diff":next.manifest.capability_diff(current.map(|s| &s.manifest)),
            "migration":migration_receipt,"safety_validation":"not_evaluated","reason":kiana_domain::redact_text(reason),"actor_id":actor,
            "state_scope":state_scope,"configuration_digest":configuration.as_ref().map(|snapshot| snapshot.config_digest.clone()),
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
            read_migration_declaration(&package, path)?;
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
                    matches!(state.state.as_str(), "revoked" | "uninstalled")
                        || &state.manifest.version != version
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
            !matches!(state.state.as_str(), "revoked" | "uninstalled")
                && state.manifest.extension_id != manifest.extension_id
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

    /// Build the single redacted extension projection consumed by every product surface.
    ///
    /// This method only reads the committed registry and the already trust-filtered Skill
    /// catalog. It never returns body text, package paths, secret values or internal entrypoints;
    /// action labels are intents and still route through `extension.manage` and ControlPlane.
    pub(crate) async fn visibility_snapshot(
        &self,
        project_root: &str,
        role_id: &str,
        query: &str,
        max_results: usize,
    ) -> Result<ExtensionVisibilitySnapshot, PortError> {
        let project = canonical_project(project_root)?;
        let project_trust = kiana_types::read_project_trust(&project)
            .ok()
            .flatten()
            .unwrap_or(kiana_types::ProjectTrust::Unknown);
        let skills = kiana_skills::load_all_skills_with_trust(&project, project_trust).await;
        let skill_catalog = kiana_skills::list_skill_catalog(&skills);
        let (registry_generation, states) = {
            let (_, history) = self.stream(project_root).await?;
            fold_registry(&history)?
        };
        let root_resolution = kiana_skills::SourceResolver::new(&project, project_trust)
            .resolve()
            .map_err(|error| failed(format!("extension_visibility_source_resolution:{error}")))?;
        let mut source_refs = root_resolution.source_refs();
        let material_digest = json_digest(&json!({
            "project": project.to_string_lossy(),
            "trust": project_trust,
            "role": role_id,
            "registry_generation": registry_generation,
            "skills": &skill_catalog,
            "states": &states,
        }));
        source_refs.push(
            SourceRef::new(
                "extension-registry",
                SourceKind::Event,
                "extension-snapshot",
                format!("registry:{registry_generation}"),
                material_digest.clone(),
                None,
                kiana_domain::EvidenceStatus::Verified,
            )
            .map_err(failed)?,
        );
        let trust_revision = json_digest(&json!({
            "project": project.to_string_lossy(),
            "trust": project_trust,
        }));
        let generation = kiana_skills::snapshot_generation()
            .max(registry_generation.saturating_add(1));
        let snapshot_id = stable_snapshot_id(&material_digest)?;
        let plugins = states
            .values()
            .filter(|state| state.state != "uninstalled")
            .enumerate()
            .map(|(index, state)| {
                kiana_domain::PluginLifecycle {
                    schema: kiana_domain::PLUGIN_LIFECYCLE_SCHEMA.to_owned(),
                    extension_id: kiana_domain::ExtensionId::parse_str(&stable_uuid(
                        &state.manifest.extension_id,
                    ))
                    .unwrap_or_default(),
                    version: state.manifest.version.clone(),
                    content_hash: state.manifest.content_hash.clone(),
                    state: lifecycle_state(&state.state),
                    revision: registry_generation
                        .checked_add(index as u64)
                        .and_then(|value| value.checked_add(1))
                        .unwrap_or(u64::MAX),
                    reason: None,
                }
            })
            .collect::<Vec<_>>();
        let source_snapshot = ExtensionSnapshot::new(
            snapshot_id,
            generation,
            source_refs,
            Vec::new(),
            Vec::new(),
            plugins,
            trust_revision,
        )
        .map_err(failed)?;

        let mut entries = skill_catalog
            .entries
            .into_iter()
            .map(|entry| {
                let mut actions = BTreeSet::from([
                    ExtensionVisibilityActionKind::List,
                    ExtensionVisibilityActionKind::Search,
                    ExtensionVisibilityActionKind::Inspect,
                ]);
                let status = match entry.status {
                    kiana_skills::SkillDisclosureStatus::Eligible => {
                        actions.insert(ExtensionVisibilityActionKind::Activate);
                        ExtensionVisibilityStatus::Eligible
                    }
                    kiana_skills::SkillDisclosureStatus::Disabled => ExtensionVisibilityStatus::Disabled,
                };
                ExtensionVisibilityEntry::new(
                    entry.name.clone(),
                    format!("skill:{}", entry.name),
                    ExtensionVisibilityKind::Skill,
                    "legacy",
                    kiana_domain::redact_text(&entry.description),
                    status,
                    ExtensionVisibilitySource::new(
                        entry.source.source.clone(),
                        source_trust(entry.source.trust),
                        entry.content_digest.clone(),
                        "skill-catalog:v1",
                    )?,
                    ExtensionVisibilityRisk::ReadOnly,
                    Some(entry.package_hash),
                    actions,
                )
            })
            .collect::<Result<Vec<_>, String>>()
            .map_err(failed)?;

        for state in states.values().filter(|state| state.state != "uninstalled") {
            if !state.manifest.supported_roles.contains(role_id) {
                continue;
            }
            let mut actions = BTreeSet::from([
                ExtensionVisibilityActionKind::List,
                ExtensionVisibilityActionKind::Search,
                ExtensionVisibilityActionKind::Inspect,
            ]);
            let status = lifecycle_visibility_status(&state.state);
            if matches!(
                status,
                ExtensionVisibilityStatus::Eligible | ExtensionVisibilityStatus::Disabled
            ) {
                actions.insert(ExtensionVisibilityActionKind::Activate);
            }
            if matches!(
                status,
                ExtensionVisibilityStatus::Eligible
                    | ExtensionVisibilityStatus::Active
                    | ExtensionVisibilityStatus::Disabled
            ) {
                actions.insert(ExtensionVisibilityActionKind::Revoke);
            }
            let kind = visibility_kind(state.manifest.extension_type);
            let source_digest = format!("sha256:{}", state.manifest.content_hash);
            let trust = if project_trust.allows_project_resources() {
                ExtensionVisibilityTrust::Trusted
            } else {
                ExtensionVisibilityTrust::Unknown
            };
            // Untrusted project state is intentionally visible only as a revoked/disabled
            // diagnostic. It cannot offer activation and never exposes package internals.
            let status = if trust != ExtensionVisibilityTrust::Trusted
                && status != ExtensionVisibilityStatus::Revoked
            {
                ExtensionVisibilityStatus::Stale
            } else {
                status
            };
            if trust != ExtensionVisibilityTrust::Trusted {
                actions.remove(&ExtensionVisibilityActionKind::Activate);
            }
            entries.push(ExtensionVisibilityEntry::new(
                state.manifest.extension_id.clone(),
                format!("{}:component", state.manifest.extension_id),
                kind,
                state.manifest.version.clone(),
                format!("{} extension", kind_name(kind)),
                status,
                ExtensionVisibilitySource::new(
                    format!("extension:{}", state.manifest.extension_id),
                    trust,
                    source_digest,
                    format!("registry:{registry_generation}"),
                )?,
                extension_risk(&state.manifest),
                Some(format!("sha256:{}", state.package_sha256)),
                actions,
            )?);
        }
        ExtensionVisibilitySnapshot::from_source_snapshot(
            &source_snapshot,
            entries,
            query,
            max_results,
        )
        .map_err(failed)
    }

    pub(crate) async fn skill_context(
        &self,
        project_root: &str,
        role_id: &str,
    ) -> Result<
        (
            Vec<PromptSection>,
            Vec<kiana_domain::ExtensionExecutionScope>,
            Vec<SkillPromptProvenance>,
        ),
        PortError,
    > {
        let (_, history) = self.stream(project_root).await?;
        let (registry_generation, states) = fold_registry(&history)?;
        let issued_at_unix_ms = unix_time_ms()?;
        let expires_at_unix_ms = issued_at_unix_ms
            .checked_add(60_000)
            .ok_or_else(|| failed("extension_scope_expiry_overflow"))?;
        let mut sections = Vec::new();
        let mut scopes = Vec::new();
        let mut provenance = Vec::new();
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
            let section_name = format!("extension:{}", state.manifest.extension_id);
            let (prompt_body, budget) = bounded_extension_prompt_body(&content);
            sections.push(PromptSection {
                name: section_name.clone(),
                order: 310,
                text: format!(
                    "Installed skill context (declarations do not grant permission): {}\n{}",
                    state.manifest.extension_id, prompt_body
                ),
                source: format!(
                    "extension:{}@{}:sha256:{}",
                    state.manifest.extension_id, state.manifest.version, state.package_sha256
                ),
                authority: PromptAuthority::Context,
            });
            provenance.push(SkillPromptProvenance {
                schema: SKILL_PROMPT_PROVENANCE_SCHEMA.to_owned(),
                section_name,
                skill_id: state.manifest.extension_id.clone(),
                version: state.manifest.version.clone(),
                content_hash: format!(
                    "sha256:{}",
                    state.manifest.content_hash.trim_start_matches("sha256:")
                ),
                trust: "verified_signature".to_owned(),
                activation_reason: Some("extension_enabled".to_owned()),
                budget,
                snapshot_id: format!("extension-snapshot:{}", state.package_sha256),
            });
            scopes.push(
                kiana_domain::ExtensionExecutionScope::new(
                    &state.manifest,
                    state.package_sha256.clone(),
                    registry_generation,
                    role_id,
                    issued_at_unix_ms,
                    expires_at_unix_ms,
                )
                .map_err(failed)?,
            );
        }
        Ok((sections, scopes, provenance))
    }
}

fn bounded_extension_prompt_body(content: &str) -> (String, PromptBudgetUsage) {
    const MAX_BYTES: usize = 16 * 1024;
    const MAX_TOKENS: u64 = 4 * 1024;
    let used_bytes = content.as_bytes().len();
    let estimated_tokens = used_bytes.div_ceil(4) as u64;
    if used_bytes <= MAX_BYTES && estimated_tokens <= MAX_TOKENS {
        return (
            content.to_owned(),
            PromptBudgetUsage {
                budget_bytes: MAX_BYTES,
                budget_tokens: MAX_TOKENS,
                used_bytes,
                estimated_tokens,
                truncated: false,
                omission_reason: None,
            },
        );
    }
    const MARKER: &str = "\n[truncated: prompt_budget]";
    let available = MAX_BYTES.saturating_sub(MARKER.len());
    let mut end = available.min(content.len());
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    let body = format!("{}{}", &content[..end], MARKER);
    let body_bytes = body.as_bytes().len();
    (
        body,
        PromptBudgetUsage {
            budget_bytes: MAX_BYTES,
            budget_tokens: MAX_TOKENS,
            used_bytes: body_bytes.min(MAX_BYTES),
            estimated_tokens: (body_bytes.div_ceil(4) as u64).min(MAX_TOKENS),
            truncated: true,
            omission_reason: None,
        },
    )
}

fn source_trust(trust: kiana_skills::SourceTrust) -> ExtensionVisibilityTrust {
    match trust {
        kiana_skills::SourceTrust::Trusted => ExtensionVisibilityTrust::Trusted,
        kiana_skills::SourceTrust::Untrusted => ExtensionVisibilityTrust::Untrusted,
        kiana_skills::SourceTrust::Unknown => ExtensionVisibilityTrust::Unknown,
        kiana_skills::SourceTrust::Denied => ExtensionVisibilityTrust::Denied,
    }
}

fn visibility_kind(extension_type: ExtensionType) -> ExtensionVisibilityKind {
    match extension_type {
        ExtensionType::Skill => ExtensionVisibilityKind::Skill,
        ExtensionType::Capability => ExtensionVisibilityKind::Capability,
        ExtensionType::Workflow => ExtensionVisibilityKind::Workflow,
        ExtensionType::Memory => ExtensionVisibilityKind::Memory,
        ExtensionType::Provider => ExtensionVisibilityKind::Provider,
        ExtensionType::Ui => ExtensionVisibilityKind::Ui,
    }
}

fn kind_name(kind: ExtensionVisibilityKind) -> &'static str {
    match kind {
        ExtensionVisibilityKind::Skill => "skill",
        ExtensionVisibilityKind::Hook => "hook",
        ExtensionVisibilityKind::Plugin => "plugin",
        ExtensionVisibilityKind::Mcp => "mcp",
        ExtensionVisibilityKind::Capability => "capability",
        ExtensionVisibilityKind::Workflow => "workflow",
        ExtensionVisibilityKind::Memory => "memory",
        ExtensionVisibilityKind::Provider => "provider",
        ExtensionVisibilityKind::Ui => "ui",
    }
}

fn lifecycle_visibility_status(state: &str) -> ExtensionVisibilityStatus {
    match state {
        "enabled" => ExtensionVisibilityStatus::Active,
        "disabled" => ExtensionVisibilityStatus::Disabled,
        "revoked" | "uninstalled" => ExtensionVisibilityStatus::Revoked,
        "staged" | "inspected" => ExtensionVisibilityStatus::Eligible,
        _ => ExtensionVisibilityStatus::Stale,
    }
}

fn lifecycle_state(state: &str) -> kiana_domain::PluginLifecycleState {
    match state {
        "enabled" => kiana_domain::PluginLifecycleState::Enabled,
        "disabled" => kiana_domain::PluginLifecycleState::Disabled,
        "revoked" => kiana_domain::PluginLifecycleState::Revoked,
        "rolled_back" => kiana_domain::PluginLifecycleState::RolledBack,
        _ => kiana_domain::PluginLifecycleState::Inspected,
    }
}

fn extension_risk(manifest: &ExtensionManifest) -> ExtensionVisibilityRisk {
    if !manifest.secret_refs.is_empty() {
        ExtensionVisibilityRisk::Secret
    } else if !matches!(manifest.network_policy, kiana_domain::ExtensionNetworkPolicy::Deny) {
        ExtensionVisibilityRisk::Network
    } else if manifest.effect == kiana_domain::ExtensionEffect::ReadWrite {
        ExtensionVisibilityRisk::WorkspaceWrite
    } else {
        ExtensionVisibilityRisk::ReadOnly
    }
}

fn stable_uuid(seed: &str) -> String {
    let digest = json_digest(&json!({"extension": seed}));
    let hex = digest.trim_start_matches("sha256:");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn stable_snapshot_id(material_digest: &str) -> Result<kiana_domain::SnapshotId, PortError> {
    kiana_domain::SnapshotId::parse_str(&stable_uuid(material_digest))
        .ok_or_else(|| failed("extension_visibility_snapshot_id_invalid"))
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
        let (registry_generation, states) = fold_registry(&history)?;
        let role_id = string(&request.request.arguments, "role_id")?;
        contract
            .validate_at(unix_time_ms()?, registry_generation, role_id)
            .map_err(failed)?;
        let state = states
            .get(&contract.extension_id)
            .ok_or_else(|| failed("extension_not_installed"))?;
        if state.state != "enabled" || state.package_sha256 != contract.package_sha256 {
            return Err(failed("extension_execution_snapshot_inactive"));
        }
        contract.matches_manifest(&state.manifest).map_err(failed)?;
        let package = self.load_cached(&state.package_sha256).await?;
        if package.package.manifest != state.manifest {
            return Err(failed("extension_snapshot_mismatch"));
        }
        self.compatibility(&state.manifest, &states)
    }

    async fn check_component_adapter(
        &self,
        request: &AuthorizedCapabilityRequest,
        descriptor: &kiana_domain::ExtensionAdapterDescriptor,
    ) -> Result<(), PortError> {
        let project_root = string(&request.request.arguments, "project_root")?;
        let (generation, states) = {
            let (_, history) = self.stream(project_root).await?;
            fold_registry(&history)?
        };
        let state = states
            .get(&descriptor.extension_id)
            .ok_or_else(|| failed("extension_not_installed"))?;
        if state.state != "enabled"
            || generation != descriptor.registry_generation
            || state.package_sha256 != descriptor.package_sha256
        {
            return Err(failed("extension_adapter_binding_stale"));
        }
        let package = self.load_cached(&state.package_sha256).await?;
        let registry = build_component_adapter_registry(&package, generation)?;
        let current = registry
            .find(&descriptor.extension_id, &descriptor.component_id)
            .ok_or_else(|| failed("extension_adapter_component_missing"))?;
        if current.descriptor_digest != descriptor.descriptor_digest {
            return Err(failed("extension_adapter_descriptor_stale"));
        }
        kiana_domain::validate_adapter_binding(
            current,
            &state.package_sha256,
            &current.snapshot_digest,
            generation,
            current.lifecycle_revision,
        )
        .map_err(failed)
    }
}

fn unix_time_ms() -> Result<u64, PortError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .map_err(|_| failed("extension_clock_invalid"))
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
        if !matches!(
            state.state.as_str(),
            "enabled" | "staged" | "revoked" | "uninstalled"
        ) || !kiana_domain::is_sha256_hex(&state.package_sha256)
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

enum MigrationDeclaration {
    Stateless(StatelessMigration),
    Controlled(ExtensionStateMigrationPlan),
}

fn read_migration_declaration(
    package: &ExtensionPackage,
    reference: &str,
) -> Result<MigrationDeclaration, PortError> {
    let encoded = package
        .files
        .get(reference)
        .ok_or_else(|| failed("extension_reference_missing"))?;
    let bytes = decode_hex(encoded)?;
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| failed("extension_migration_declaration_invalid"))?;
    match value.get("schema").and_then(Value::as_str) {
        Some(kiana_domain::EXTENSION_STATE_MIGRATION_PLAN_SCHEMA) => {
            let plan: ExtensionStateMigrationPlan = serde_json::from_value(value)
                .map_err(|_| failed("extension_state_migration_plan_invalid"))?;
            plan.validate().map_err(failed)?;
            Ok(MigrationDeclaration::Controlled(plan))
        }
        Some("kiana.extension-migration.v1") => Ok(MigrationDeclaration::Stateless(
            read_stateless_migration(package, reference)?,
        )),
        _ => Err(failed("extension_migration_adapter_unavailable")),
    }
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

fn prepare_controlled_migration(
    manifest: &ExtensionManifest,
    scope: &ExtensionStateScope,
    plan: &ExtensionStateMigrationPlan,
) -> Result<ExtensionStateMigrationReceipt, PortError> {
    plan.validate().map_err(failed)?;
    if plan.extension_id != manifest.extension_id
        || plan.publisher != manifest.publisher
        || plan.scope_digest != scope.scope_digest
    {
        return Err(failed("extension_state_migration_binding_mismatch"));
    }
    ExtensionStateMigrationReceipt::unknown(plan, "migration_requires_controlled_action")
        .map_err(failed)
}

fn parse_configuration_snapshot(
    value: Option<&Value>,
    manifest: &ExtensionManifest,
    scope: &ExtensionStateScope,
) -> Result<Option<ExtensionConfigurationSnapshot>, PortError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let snapshot: ExtensionConfigurationSnapshot = serde_json::from_value(value.clone())
        .map_err(|_| failed("extension_configuration_snapshot_invalid"))?;
    snapshot.validate().map_err(failed)?;
    if snapshot.extension_id != manifest.extension_id || snapshot.scope_digest != scope.scope_digest
    {
        return Err(failed("extension_configuration_scope_mismatch"));
    }
    Ok(Some(snapshot))
}

fn activation_state(manifest: &ExtensionManifest) -> &'static str {
    if manifest.extension_type == ExtensionType::Skill {
        "enabled"
    } else {
        "staged"
    }
}

/// Build the inspect-only component registry from a package that has already passed the daemon's
/// signature, content-hash, trust and dependency checks. The registry is metadata; it never
/// executes the component entry. Runtime requests still go through the existing broker admission
/// and extension execution contract.
fn build_component_adapter_registry(
    package: &VerifiedPackage,
    registry_generation: u64,
) -> Result<ExtensionAdapterRegistry, PortError> {
    let manifest = &package.package.manifest;
    let kind = match manifest.extension_type {
        ExtensionType::Skill => ExtensionComponentKind::Skill,
        ExtensionType::Capability => ExtensionComponentKind::Capability,
        ExtensionType::Workflow => ExtensionComponentKind::Workflow,
        ExtensionType::Memory => ExtensionComponentKind::Memory,
        ExtensionType::Provider => ExtensionComponentKind::Provider,
        ExtensionType::Ui => ExtensionComponentKind::Ui,
    };
    let entry = if kind == ExtensionComponentKind::Skill {
        "SKILL.md".to_owned()
    } else {
        package
            .package
            .files
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| failed("extension_adapter_entry_missing"))?
    };
    let manifest_digest = json_digest(
        &serde_json::to_value(manifest).map_err(|_| failed("extension_manifest_digest_failed"))?,
    );
    let source_digest = json_digest(&json!({
        "publisher": manifest.publisher.clone(),
        "extension_id": manifest.extension_id.clone(),
        "package_sha256": package.package_sha256.clone(),
    }));
    let snapshot_digest = json_digest(&json!({
        "registry_generation": registry_generation,
        "package_sha256": package.package_sha256.clone(),
    }));
    let binding_digest = json_digest(&json!({
        "snapshot_digest": snapshot_digest.clone(),
        "extension_id": manifest.extension_id.clone(),
        "lifecycle_revision": registry_generation,
    }));
    let phase = if manifest.extension_type == ExtensionType::Skill {
        ExtensionLifecyclePhase::Enabled
    } else {
        ExtensionLifecyclePhase::Staged
    };
    let component = VerifiedExtensionComponent::from_verified_package(
        &package.package,
        package.package_sha256.clone(),
        manifest_digest,
        source_digest,
        snapshot_digest,
        binding_digest,
        "main",
        kind,
        entry,
        registry_generation,
        registry_generation,
        phase,
        manifest.effect,
        manifest.network_policy.clone(),
        true,
        manifest.required_capabilities.clone(),
        true,
        true,
        true,
        None,
    )
    .map_err(failed)?;
    let descriptor =
        kiana_domain::ExtensionAdapterDescriptor::from_verified(component).map_err(failed)?;
    ExtensionAdapterRegistry::new(registry_generation, vec![descriptor]).map_err(failed)
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
