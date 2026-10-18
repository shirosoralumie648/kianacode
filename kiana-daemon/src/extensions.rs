//! Approved, signed local packages. EventLog is the activation authority; files are cache.

use crate::local_packages::{decode_hex, failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler, ExtensionAdmission};
use kiana_domain::{
    json_digest, AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult,
    ExtensionAdapterRegistry, ExtensionComponentKind, ExtensionConfigSnapshot,
    ExtensionConfigurationSnapshot, ExtensionExecutionContract, ExtensionLifecycleMutation,
    ExtensionLifecyclePhase, ExtensionManifest, ExtensionPackage, ExtensionSnapshot,
    ExtensionSnapshotState, ExtensionStateMigrationPlan, ExtensionStateMigrationReceipt,
    ExtensionStateScope, ExtensionType, ExtensionVisibilityActionKind, ExtensionVisibilityEntry,
    ExtensionVisibilityKind, ExtensionVisibilityRisk, ExtensionVisibilitySnapshot,
    ExtensionVisibilitySource, ExtensionVisibilityStatus, ExtensionVisibilityTrust,
    PromptAuthority, PromptBudgetUsage, PromptSection, RequestId, RuntimeEvent,
    SkillPromptProvenance, SourceKind, SourceRef, VerifiedExtensionComponent,
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
        let actor = arguments
            .get("actor_id")
            .and_then(Value::as_str)
            .unwrap_or("readonly");
        if arguments["operator_authorized"] != true
            || (request.request.cell_id.is_some()
                && request.request.risk != kiana_domain::RiskLevel::ReadOnly)
        {
            return Err(failed("extension_operator_required"));
        }
        let (scope, history) = self.stream(project_root).await?;
        let (version, states) = fold_registry(&history)?;
        let role_id = arguments
            .get("role_id")
            .and_then(Value::as_str)
            .unwrap_or("builder");
        if action == "list"
            || action == "search"
            || (action == "inspect" && arguments["package_path"].is_null())
        {
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
                    "dependency_snapshot_digest": dependency_snapshot_digest(&states),
                    "trust_revision": project_trust_revision(Path::new(project_root))?,
                    "snapshot": snapshot,
                    "extension": entry,
                }));
            }
            return Ok(json!({
                "schema": REGISTRY_SCHEMA,
                "registry_version": version,
                "dependency_snapshot_digest": dependency_snapshot_digest(&states),
                "trust_revision": project_trust_revision(Path::new(project_root))?,
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
                "dependency_snapshot_digest":dependency_snapshot_digest(&states),
                "trust_revision":project_trust_revision(Path::new(project_root))?,
                "capability_diff":manifest.capability_diff(states.get(&manifest.extension_id).map(|s| &s.manifest)),
                "activation":activation_state(manifest),"state_scope_digest":state_scope.scope_contract_digest,
                "configuration_digest":configuration.as_ref().map(|snapshot| snapshot.config_digest.clone()),
                "component_adapters":component_adapters,
                "safety_validation":"not_evaluated"}),
            );
        }
        if !matches!(
            action,
            "stage"
                | "install"
                | "upgrade"
                | "enable"
                | "disable"
                | "revoke"
                | "rollback"
                | "uninstall"
        ) {
            return Err(failed("extension_action_invalid"));
        }
        if !matches!(
            request.request.risk,
            kiana_domain::RiskLevel::ExternalSideEffect | kiana_domain::RiskLevel::Critical
        ) {
            return Err(failed("extension_approval_risk_required"));
        }
        // 类型化生命周期绑定（inspect → stage → install → enable）。
        //
        // stage / install / enable 必须携带 ControlPlane 归一化之后铸造的
        // mutation 摘要，以及该 mutation 绑定的 trust revision、依赖快照摘要；
        // enable 还必须携带 approval 与类型化 config snapshot。daemon 用**当前**
        // 事件流与项目 trust 重新计算这两个快照，过期即拒绝，避免「用旧依赖集/
        // 旧信任状态批准新变更」。这些摘要全部由服务端字段一起参与计算
        // （actor/project 由 Core 注入后才铸造），客户端无法自行铸造；任何字段
        // 在传输途中被改写都会体现为摘要不一致。
        let mut lifecycle_mutation = None;
        let mut config_snapshot = None;
        if matches!(action, "stage" | "install" | "enable") {
            let mutation =
                kiana_domain::ExtensionLifecycleMutation::from_command_arguments(arguments)
                    .map_err(failed)?;
            let declared = arguments
                .get("mutation_digest")
                .and_then(Value::as_str)
                .ok_or_else(|| failed("extension_lifecycle_mutation_digest_required"))?;
            if declared != mutation.mutation_digest {
                return Err(failed("extension_lifecycle_mutation_digest_mismatch"));
            }
            if mutation.trust_revision != project_trust_revision(Path::new(project_root))? {
                return Err(failed("extension_trust_revision_stale"));
            }
            if mutation.dependency_snapshot_digest.as_deref()
                != Some(dependency_snapshot_digest(&states).as_str())
            {
                return Err(failed("extension_dependency_snapshot_stale"));
            }
            let snapshot = arguments
                .get("config_snapshot")
                .cloned()
                .map(serde_json::from_value::<kiana_domain::ExtensionConfigSnapshot>)
                .transpose()
                .map_err(|_| failed("extension_config_snapshot_invalid"))?;
            if action == "enable" && snapshot.is_none() {
                return Err(failed("extension_config_snapshot_required"));
            }
            if let Some(snapshot) = &snapshot {
                snapshot.bind_to_mutation(&mutation).map_err(failed)?;
            }
            lifecycle_mutation = Some(mutation);
            config_snapshot = snapshot;
        }
        // Resolve the target extension before the idempotency replay path so a replayed command
        // answers with the same typed receipt (which carries `extension_id`) as the original.
        let extension_id = string(arguments, "extension_id")?;
        if !kiana_domain::valid_extension_identifier(extension_id) {
            return Err(failed("extension_id_invalid"));
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
            // A replayed command must answer with the same typed receipt shape as the original
            // commit: a surface that renders receipts cannot be forced back onto the generic
            // lifecycle envelope just because the answer came from the idempotency ledger.
            output["command_receipt"] = json!({
                "schema": kiana_domain::EXTENSION_COMMAND_RECEIPT_SCHEMA,
                "command": action,
                "extension_id": extension_id,
                "actor_id": actor,
                "reason": previous.data["receipt"]["reason"],
                "idempotency_key": key,
                "expected_registry_version": arguments["expected_registry_version"],
                "registry_version": previous.data["receipt"]["registry_version"],
                "mutation_digest": lifecycle_mutation
                    .as_ref()
                    .map(|mutation| &mutation.mutation_digest),
                "approval_id": lifecycle_mutation
                    .as_ref()
                    .and_then(|mutation| mutation.approval_id.as_ref()),
                "event_id": previous.event_id,
                "replayed": true,
            });
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
            "enable" | "disable" => {
                let current = current.ok_or_else(|| failed("extension_not_installed"))?;
                if matches!(current.state.as_str(), "revoked" | "uninstalled") {
                    return Err(failed("extension_lifecycle_state_invalid"));
                }
                // `stage` 只把已验证字节放进不可变缓存并登记一条 inert 记录，
                // 它不构成安装。没有一次 install/upgrade/rollback 事件之前，
                // enable 必须被拒绝：staging 永远不能直接开通执行面。
                if action == "enable"
                    && latest_lifecycle_action(&history, extension_id) == Some("stage")
                {
                    return Err(failed("extension_not_installed"));
                }
                if action == "enable" && current.state == "enabled" {
                    return Err(failed("extension_already_enabled"));
                }
                if action == "disable" && current.state != "enabled" {
                    return Err(failed("extension_not_enabled"));
                }
                let mut next = current.clone();
                next.state = if action == "enable" {
                    "enabled".to_owned()
                } else {
                    "disabled".to_owned()
                };
                next.installed_by = actor.to_owned();
                next.source_request_id = request.request.request_id;
                (next, None)
            }
            "stage" | "install" | "upgrade" | "rollback" => {
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
                // 只有「同一份已验证包、且当前记录正是由 stage 事件建立」时的
                // stage/install 才被允许：前者是幂等重试，后者是 stage → install
                // 的推进。其余已存在的记录（包括真正安装过的）一律拒绝安装，
                // 必须走 upgrade/rollback。
                let staged_pending = current.is_some_and(|state| {
                    state.state == "staged"
                        && state.package_sha256 == package.package_sha256
                        && latest_lifecycle_action(&history, extension_id) == Some("stage")
                });
                if matches!(action, "stage" | "install")
                    && current.is_some_and(|state| state.state != "uninstalled")
                    && !staged_pending
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
                    if current.package_sha256 == package.package_sha256 && !staged_pending {
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
        // `enable` 的有效配置投影：只为 enable 携带的类型化快照才有值。它按
        // host → user → project → run 的优先级合并，只含每个字段的来源引用与
        // 值摘要（不含明文、不含密钥本体）；非 enable 动作恒为空数组。
        let config_effective = config_snapshot
            .as_ref()
            .map(kiana_domain::ExtensionConfigSnapshot::effective_entries)
            .transpose()
            .map_err(failed)?
            .unwrap_or_default();
        let lifecycle_binding = json!({
            "mutation_digest": lifecycle_mutation.as_ref().map(|mutation| &mutation.mutation_digest),
            "trust_revision": lifecycle_mutation.as_ref().map(|mutation| &mutation.trust_revision),
            "dependency_snapshot_digest": lifecycle_mutation
                .as_ref()
                .and_then(|mutation| mutation.dependency_snapshot_digest.as_ref()),
            "approval_id": lifecycle_mutation
                .as_ref()
                .and_then(|mutation| mutation.approval_id.as_ref()),
            "config_snapshot_digest": config_snapshot
                .as_ref()
                .map(|snapshot| &snapshot.snapshot_digest),
            "config_effective": config_effective,
        });
        let receipt = json!({"schema":REGISTRY_SCHEMA,"action":action,"extension_id":extension_id,
            "project_root":project_root,"registry_version":next_version,"state":next.state,
            "package_sha256":next.package_sha256,"content_hash":next.manifest.content_hash,
            "version":next.manifest.version,"previous_version":current.map(|s| &s.manifest.version),
            "previous_package_sha256":current.map(|s| &s.package_sha256),"license":next.manifest.license,
            "signature":next.manifest.signature,"signature_verified":matches!(action, "stage" | "install" | "upgrade" | "rollback"),
            "capability_diff":next.manifest.capability_diff(current.map(|s| &s.manifest)),
            "migration":migration_receipt,"safety_validation":"not_evaluated","reason":kiana_domain::redact_text(reason),"actor_id":actor,
            "state_scope":state_scope,"configuration_digest":configuration.as_ref().map(|snapshot| snapshot.config_digest.clone()),
            "lifecycle_binding":lifecycle_binding,
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
        output["command_receipt"] = json!({
            "schema": kiana_domain::EXTENSION_COMMAND_RECEIPT_SCHEMA,
            "command": action,
            "extension_id": extension_id,
            "actor_id": actor,
            "reason": kiana_domain::redact_text(reason),
            "idempotency_key": key,
            "expected_registry_version": expected,
            "registry_version": next_version,
            "mutation_digest": lifecycle_mutation
                .as_ref()
                .map(|mutation| &mutation.mutation_digest),
            "approval_id": lifecycle_mutation
                .as_ref()
                .and_then(|mutation| mutation.approval_id.as_ref()),
            "event_id": appended.event.event_id,
            "replayed": false,
        });
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
            "trust": project_trust.as_str(),
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
        let trust_revision = project_trust_revision(&project)?;
        let generation =
            kiana_skills::snapshot_generation().max(registry_generation.saturating_add(1));
        let snapshot_id = stable_snapshot_id(&material_digest)?;
        let plugins = states
            .values()
            .filter(|state| state.state != "uninstalled")
            .enumerate()
            .map(|(index, state)| kiana_domain::PluginLifecycle {
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
                    kiana_skills::SkillDisclosureStatus::Disabled => {
                        ExtensionVisibilityStatus::Disabled
                    }
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
            entries.push(
                ExtensionVisibilityEntry::new(
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
                    )
                    .map_err(failed)?,
                    extension_risk(&state.manifest),
                    Some(format!("sha256:{}", state.package_sha256)),
                    actions,
                )
                .map_err(failed)?,
            );
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
    } else if !matches!(
        manifest.network_policy,
        kiana_domain::ExtensionNetworkPolicy::Deny
    ) {
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
            "enabled" | "staged" | "disabled" | "rolled_back" | "revoked" | "uninstalled"
        ) || !kiana_domain::is_sha256_hex(&state.package_sha256)
        {
            return Err(failed("extension_registry_event_invalid"));
        }
        states.insert(state.manifest.extension_id.clone(), state);
        version += 1;
    }
    Ok((version, states))
}

/// Digest of the registry dependency surface a caller inspected before mutating.
///
/// `inspect` returns this value; `stage` / `install` / `enable` must carry it back. The digest
/// covers exactly what another extension's `requires.extensions` can depend on (identity,
/// version, verified package hash and live lifecycle state) and is folded from the committed
/// event stream, so it is stable across processes and cannot be forged from arguments.
fn dependency_snapshot_digest(states: &BTreeMap<String, InstalledExtension>) -> String {
    json_digest(&json!({
        "schema": "kiana.extension-dependency-snapshot.v1",
        "dependencies": states
            .values()
            .map(|state| {
                json!({
                    "extension_id": state.manifest.extension_id,
                    "version": state.manifest.version,
                    "package_sha256": state.package_sha256,
                    "state": state.state,
                })
            })
            .collect::<Vec<_>>(),
    }))
}

/// Lifecycle action of the most recent committed event for `extension_id`.
///
/// The folded state alone cannot distinguish "staged by `stage`" (inert bytes) from "staged by
/// `install`" (installation confirmed, enable allowed). Both write state `staged`; only the
/// append-only action history can tell them apart, which is why promotion and `enable` read it.
fn latest_lifecycle_action<'a>(history: &'a [RuntimeEvent], extension_id: &str) -> Option<&'a str> {
    history
        .iter()
        .rev()
        .find(|event| {
            event.kind == "extension.lifecycle"
                && event.data["state"]["manifest"]["extension_id"] == extension_id
        })
        .and_then(|event| event.data["action"].as_str())
}

/// Trust revision bound into lifecycle mutations and visibility snapshots.
///
/// It digests the canonical project path plus the project trust decision. The same value is
/// returned by the read-only projection, so a caller can prove "I asked for this extension
/// while the project was in this trust state" and the daemon can reject stale revisions after
/// the trust file changes.
fn project_trust_revision(project: &Path) -> Result<String, PortError> {
    let project = canonical_project(&project.to_string_lossy())?;
    let project_trust = kiana_types::read_project_trust(&project)
        .ok()
        .flatten()
        .unwrap_or(kiana_types::ProjectTrust::Unknown);
    Ok(json_digest(&json!({
        "project": project.to_string_lossy(),
        "trust": project_trust.as_str(),
    })))
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
    let _ = manifest;
    "staged"
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
    use super::{
        activation_state, canonical_project, fold_registry, verify_manifest_signature,
        ExtensionRegistry, InstalledExtension, REGISTRY_SCHEMA,
    };
    use crate::local_packages::{decode_hex, sha256, LocalDir};
    use async_trait::async_trait;
    use kiana_capability_broker::{CapabilityBroker, CapabilityHandler, ExtensionAdmission};
    use kiana_domain::{
        AuthenticatedPrincipalRef, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
        CapabilityResult, ExecutionScope, ExtensionCommand, ExtensionCommandReceipt,
        ExtensionConfigEntry, ExtensionConfigScope, ExtensionConfigSnapshot, ExtensionEffect,
        ExtensionExecutionScope, ExtensionManifest, ExtensionNetworkPolicy, ExtensionPackage,
        ExtensionRequires, ExtensionSignature, ExtensionType, ProjectIdentity, RequestId,
        RiskLevel, RuntimeEvent, ScopeDimension, ScopeLimit, ScopeSet, SessionId,
        EXTENSION_MANAGE_OPERATION, EXTENSION_MANIFEST_SCHEMA, EXTENSION_PACKAGE_SCHEMA,
        EXTENSION_STREAM,
    };
    use kiana_ports::{
        CapabilityBrokerPort, EventStorePort, ExecutionPermitVerifierPort, PortError,
    };
    use ring::signature::{Ed25519KeyPair, KeyPair};
    use serde_json::{json, Value};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    struct FixturePackage {
        manifest: ExtensionManifest,
        bytes: Vec<u8>,
        package_sha256: String,
        trusted_keys: BTreeMap<String, BTreeMap<String, String>>,
    }

    struct CountingHandler {
        calls: Arc<AtomicUsize>,
        binding_version: &'static str,
    }

    struct NoopHandler {
        binding_version: &'static str,
    }

    struct AllowPermits;

    #[async_trait]
    impl ExecutionPermitVerifierPort for AllowPermits {
        async fn verify_and_consume(
            &self,
            _request: &AuthorizedCapabilityRequest,
        ) -> Result<(), PortError> {
            Ok(())
        }
    }

    #[async_trait]
    impl CapabilityHandler for NoopHandler {
        fn binding_version(&self) -> &'static str {
            self.binding_version
        }

        async fn execute(
            &self,
            request: AuthorizedCapabilityRequest,
        ) -> Result<CapabilityResult, PortError> {
            Ok(CapabilityResult::success(
                request.request.request_id,
                serde_json::Value::Null,
            ))
        }
    }

    #[async_trait]
    impl CapabilityHandler for CountingHandler {
        fn binding_version(&self) -> &'static str {
            self.binding_version
        }

        async fn execute(
            &self,
            request: AuthorizedCapabilityRequest,
        ) -> Result<CapabilityResult, PortError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(CapabilityResult::success(
                request.request.request_id,
                json!({"fixture": "extension-admission"}),
            ))
        }
    }

    fn package_content_hash(files: &BTreeMap<String, String>) -> String {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(b"kiana.extension-content.v1\0");
        for (path, encoded) in files {
            let content = decode_hex(encoded).expect("hex fixture content");
            digest.update((path.len() as u64).to_le_bytes());
            digest.update(path.as_bytes());
            digest.update((content.len() as u64).to_le_bytes());
            digest.update(&content);
        }
        format!("{:x}", digest.finalize())
    }

    fn fixture_package() -> FixturePackage {
        let key_pair = Ed25519KeyPair::from_seed_unchecked(&[9_u8; 32]).expect("seed");
        let files = BTreeMap::from([(
            "SKILL.md".to_owned(),
            hex("Fixture skill body; declarations never grant permission.".as_bytes()),
        )]);
        let mut signed = manifest(String::new());
        signed.content_hash = package_content_hash(&files);
        let signature = key_pair.sign(&signed.signing_bytes().expect("signing bytes"));
        signed.signature.value = hex(signature.as_ref());
        signed.validate().expect("fixture manifest");
        let package = ExtensionPackage {
            schema: EXTENSION_PACKAGE_SCHEMA.to_owned(),
            manifest: signed.clone(),
            files,
        };
        let bytes = serde_json::to_vec(&package).expect("fixture package bytes");
        let package_sha256 = sha256(&bytes);
        let trusted_keys = BTreeMap::from([(
            "fixture-publisher".to_owned(),
            BTreeMap::from([(
                "fixture-key".to_owned(),
                hex(key_pair.public_key().as_ref()),
            )]),
        )]);
        FixturePackage {
            manifest: signed,
            bytes,
            package_sha256,
            trusted_keys,
        }
    }

    fn fixture_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time");
        let stamp = stamp.as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-ext25-{label}-{stamp}"));
        std::fs::create_dir_all(root.join("project")).expect("fixture project dir");
        root
    }

    fn fixture_registry(
        root: &Path,
        package: &FixturePackage,
        events: Arc<dyn EventStorePort>,
    ) -> Arc<ExtensionRegistry> {
        let cache_root = root.join("extensions").join("packages");
        LocalDir::open(&cache_root, true)
            .expect("fixture cache root")
            .publish(&format!("{}.json", package.package_sha256), &package.bytes)
            .expect("publish fixture package");
        Arc::new(ExtensionRegistry {
            events,
            cache_root,
            state_root: root.join("extensions").join("state"),
            trusted_keys: package.trusted_keys.clone(),
        })
    }

    fn installed_state(package: &FixturePackage, state: &str) -> InstalledExtension {
        InstalledExtension {
            manifest: package.manifest.clone(),
            package_sha256: package.package_sha256.clone(),
            state: state.to_owned(),
            installed_by: "fixture".to_owned(),
            source_request_id: RequestId::new(),
        }
    }

    async fn append_registry_state(
        events: &Arc<dyn EventStorePort>,
        scope_id: &str,
        version: u64,
        state: &InstalledExtension,
    ) {
        let event = RuntimeEvent::new(
            RequestId::new(),
            1,
            "extension.lifecycle",
            json!({"schema": REGISTRY_SCHEMA, "state": state}),
        )
        .expect("registry event")
        .with_stream_metadata(EXTENSION_STREAM, scope_id, version);
        events
            .append_expected(event, Some(version - 1))
            .await
            .expect("append registry event");
    }

    fn project_scope(root: &Path) -> (String, String) {
        let fixture = root.join("project");
        let project = canonical_project(fixture.to_str().expect("fixture project utf8"))
            .expect("fixture project");
        let scope_id = sha256(project.to_string_lossy().as_bytes());
        (project.to_string_lossy().to_string(), scope_id)
    }

    fn execution_scope(request: &CapabilityRequest) -> ExecutionScope {
        let permission_scope = ScopeSet::new(
            ScopeDimension::Restricted(vec![request.operation.clone()]),
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )
        .expect("fixture permission scope");
        let principal = AuthenticatedPrincipalRef::local();
        let project = ProjectIdentity::new(
            "/repo",
            "/repo",
            None,
            None,
            kiana_domain::json_digest(&json!("trusted")),
        )
        .expect("fixture project identity");
        let mut scope = ExecutionScope {
            schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
            version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
            principal,
            project,
            session_id: SessionId::new("ext25-fixture"),
            run_id: None,
            turn_id: None,
            cell_id: None,
            grant_refs: Vec::new(),
            budget_lease_id: None,
            work_packet_id: None,
            environment_id: "kiana-local".to_owned(),
            workspace_revision: None,
            permission_scope: permission_scope.clone(),
            read_roots: vec!["/repo".to_owned()],
            write_roots: Vec::new(),
            read_denies: Vec::new(),
            write_denies: Vec::new(),
            memory_scopes: Vec::new(),
            server_scopes: Vec::new(),
            network_policy: Vec::new(),
            authority_epoch: 1,
            trust_revision: kiana_domain::json_digest(&json!("trusted")),
            data_epoch: 1,
            cancellation_epoch: 1,
            deadline_unix_ms: u64::MAX,
            fencing_token: 1,
            catalog_digest: kiana_domain::capability_action_catalog_digest(),
            action_digest: kiana_domain::capability_action_digest(request),
            permission_scope_digest: permission_scope.digest(),
            scope_digest: String::new(),
        };
        scope.scope_digest = scope.digest();
        scope
    }

    fn extension_request(
        project_root: &str,
        role_id: &str,
        scope: &ExtensionExecutionScope,
    ) -> AuthorizedCapabilityRequest {
        let mut request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "memory.search",
            json!({
                "query": "ext25 admission fixture",
                "project_root": project_root,
                "role_id": role_id,
                "_extension_scopes": serde_json::to_value(scope).expect("scope json"),
            }),
        )
        .with_risk(RiskLevel::ReadOnly);
        kiana_domain::normalize_capability_action(&mut request).expect("normalize fixture request");
        request.execution_scope = Some(execution_scope(&request));
        AuthorizedCapabilityRequest::new("extension:ext25-fixture", request).expect("authorized")
    }

    fn sealed_broker(
        admission: Arc<dyn ExtensionAdmission>,
        calls: Arc<AtomicUsize>,
    ) -> CapabilityBroker {
        let mut broker = CapabilityBroker::new();
        for operation in kiana_domain::ACTION_OPERATIONS {
            let descriptor =
                kiana_domain::capability_action_descriptor(operation).expect("descriptor");
            let handler: Arc<dyn CapabilityHandler> = if *operation == "memory.search" {
                Arc::new(CountingHandler {
                    calls: calls.clone(),
                    binding_version: descriptor.binding_version,
                })
            } else {
                Arc::new(NoopHandler {
                    binding_version: descriptor.binding_version,
                })
            };
            broker
                .register_static(descriptor.capability, *operation, handler)
                .expect("register fixture handler");
        }
        broker.validate_catalog_bindings().expect("sealed catalog");
        broker.set_permit_verifier(Arc::new(AllowPermits));
        broker.set_extension_admission(admission);
        broker
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
                .expect_err("manifest drift must fail closed"),
            kiana_ports::PortError::Failed("extension_signature_verification_failed".to_owned())
        );
        let mut untrusted = trusted;
        untrusted.remove("fixture-publisher");
        assert_eq!(
            verify_manifest_signature(&unsigned, &untrusted)
                .expect_err("unknown publisher must fail closed"),
            kiana_ports::PortError::Failed("extension_publisher_key_untrusted".to_owned())
        );
    }

    #[test]
    fn extension_install_state_is_staged_before_explicit_enable() {
        assert_eq!(activation_state(&manifest(String::new())), "staged");
    }

    /// EXT-25 behavioral fixture: the host folds a signed, enabled fixture package, then the
    /// broker and the daemon admission recheck the live registry immediately before dispatch.
    /// Deny-first: role drift, expired windows, forged scope digests, stale generation, disabled,
    /// revoked and post-upgrade package-hash drift must fail with stable reason codes and zero
    /// handler calls before the passing control case runs; the enabled control dispatches once.
    #[tokio::test]
    async fn extension_admission_rechecks_live_registry_before_dispatch() {
        let package = fixture_package();
        let root = fixture_root("admission");
        let (project_root, scope_id) = project_scope(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        append_registry_state(&events, &scope_id, 1, &installed_state(&package, "enabled")).await;

        // The host issues the scope from the committed registry snapshot, never from the prompt.
        let (sections, scopes, provenance) = registry
            .skill_context(&project_root, "builder")
            .await
            .expect("enabled fixture skill context");
        assert_eq!(sections.len(), 1);
        assert_eq!(scopes.len(), 1);
        assert_eq!(provenance.len(), 1);
        let issued_scope = scopes.into_iter().next().expect("issued scope");
        assert_eq!(issued_scope.registry_generation, 1);

        let calls = Arc::new(AtomicUsize::new(0));
        let broker = sealed_broker(registry.clone(), calls.clone());

        // (a) role drift: the request role no longer matches the signed scope.
        let drift = extension_request(&project_root, "architect", &issued_scope);
        assert_eq!(
            registry
                .check(&drift, &issued_scope.contract(ExtensionEffect::ReadOnly))
                .await
                .expect_err("role drift must fail closed"),
            PortError::Failed("extension_scope_role_mismatch".to_owned())
        );
        assert_eq!(
            broker
                .execute(drift)
                .await
                .expect_err("role drift must not dispatch"),
            PortError::Failed("extension_scope_role_mismatch".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // (b) expired window: the signed issue/expiry window is part of the server scope, so a
        // scope that outlived its window must fail closed at both admission layers.
        let expired_scope = ExtensionExecutionScope::new(
            &package.manifest,
            package.package_sha256.clone(),
            1,
            "builder",
            issued_scope.issued_at_unix_ms.saturating_sub(2_000),
            issued_scope.issued_at_unix_ms.saturating_sub(1_000),
        )
        .expect("expired fixture scope");
        let expired = extension_request(&project_root, "builder", &expired_scope);
        assert_eq!(
            registry
                .check(&expired, &expired_scope.contract(ExtensionEffect::ReadOnly))
                .await
                .expect_err("expired scope must fail closed"),
            PortError::Failed("extension_scope_expired".to_owned())
        );
        assert_eq!(
            broker
                .execute(expired)
                .await
                .expect_err("expired scope must not dispatch"),
            PortError::Failed("extension_scope_expired".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // (c) forged scope digest: a client or model cannot mint `_extension_scopes`; a
        // well-formed scope whose digest does not cover its own fields is refused before any
        // handler runs. The Broker rejects the wire shape, the admission recomputes the digest.
        let mut forged_scope = issued_scope.clone();
        forged_scope.scope_digest = format!("sha256:{}", "0".repeat(64));
        let forged = extension_request(&project_root, "builder", &forged_scope);
        assert_eq!(
            registry
                .check(&forged, &forged_scope.contract(ExtensionEffect::ReadOnly))
                .await
                .expect_err("forged scope digest must fail closed"),
            PortError::Failed("extension_scope_digest_mismatch".to_owned())
        );
        assert_eq!(
            broker
                .execute(forged)
                .await
                .expect_err("forged scope must not dispatch"),
            PortError::Failed("extension_scope_invalid".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // (d) stale registry generation: disabling advances the registry to generation 2, so the
        // scope captured at generation 1 must fail closed even though role and package match.
        append_registry_state(
            &events,
            &scope_id,
            2,
            &installed_state(&package, "disabled"),
        )
        .await;
        let stale = extension_request(&project_root, "builder", &issued_scope);
        assert_eq!(
            registry
                .check(&stale, &issued_scope.contract(ExtensionEffect::ReadOnly))
                .await
                .expect_err("stale generation must fail closed"),
            PortError::Failed("extension_scope_generation_stale".to_owned())
        );
        assert_eq!(
            broker
                .execute(stale)
                .await
                .expect_err("stale generation must not dispatch"),
            PortError::Failed("extension_scope_generation_stale".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // (e) disabled package: even a scope recomputed at the current generation must not
        // authorize execution while the live registry state is not enabled.
        let disabled_scope = ExtensionExecutionScope::new(
            &package.manifest,
            package.package_sha256.clone(),
            2,
            "builder",
            issued_scope.issued_at_unix_ms,
            issued_scope.expires_at_unix_ms,
        )
        .expect("disabled fixture scope");
        let disabled = extension_request(&project_root, "builder", &disabled_scope);
        assert_eq!(
            registry
                .check(
                    &disabled,
                    &disabled_scope.contract(ExtensionEffect::ReadOnly)
                )
                .await
                .expect_err("disabled package must fail closed"),
            PortError::Failed("extension_execution_snapshot_inactive".to_owned())
        );
        assert_eq!(
            broker
                .execute(disabled)
                .await
                .expect_err("disabled package must not dispatch"),
            PortError::Failed("extension_execution_snapshot_inactive".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // (f) revoked package: the next generation keeps failing closed with the same code.
        append_registry_state(&events, &scope_id, 3, &installed_state(&package, "revoked")).await;
        let revoked_scope = ExtensionExecutionScope::new(
            &package.manifest,
            package.package_sha256.clone(),
            3,
            "builder",
            issued_scope.issued_at_unix_ms,
            issued_scope.expires_at_unix_ms,
        )
        .expect("revoked fixture scope");
        let revoked = extension_request(&project_root, "builder", &revoked_scope);
        assert_eq!(
            registry
                .check(&revoked, &revoked_scope.contract(ExtensionEffect::ReadOnly))
                .await
                .expect_err("revoked package must fail closed"),
            PortError::Failed("extension_execution_snapshot_inactive".to_owned())
        );
        assert_eq!(
            broker
                .execute(revoked)
                .await
                .expect_err("revoked package must not dispatch"),
            PortError::Failed("extension_execution_snapshot_inactive".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // Control: an enabled registry at the matching generation admits once and dispatches.
        let control_root = fixture_root("control");
        let (control_project, control_scope_id) = project_scope(&control_root);
        let control_events: Arc<dyn EventStorePort> =
            Arc::new(kiana_eventlog::MemoryEventLog::new());
        let control_registry = fixture_registry(&control_root, &package, control_events.clone());
        append_registry_state(
            &control_events,
            &control_scope_id,
            1,
            &installed_state(&package, "enabled"),
        )
        .await;
        let (_, control_scopes, _) = control_registry
            .skill_context(&control_project, "builder")
            .await
            .expect("control skill context");
        let control_scope = control_scopes.into_iter().next().expect("control scope");
        let control = extension_request(&control_project, "builder", &control_scope);
        control_registry
            .check(&control, &control_scope.contract(ExtensionEffect::ReadOnly))
            .await
            .expect("control admission passes");
        let control_broker = sealed_broker(control_registry.clone(), calls.clone());
        let result = control_broker
            .execute(control)
            .await
            .expect("control dispatch");
        assert_eq!(result.output["fixture"], json!("extension-admission"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // (g) upgrade package-hash drift: a new enabled generation carrying a different package
        // hash invalidates the pre-upgrade scope, and even a self-consistent scope re-issued at
        // the new generation with the old package hash fails the live snapshot comparison.
        let mut upgraded = installed_state(&package, "enabled");
        upgraded.package_sha256 = "e".repeat(64);
        append_registry_state(&control_events, &control_scope_id, 2, &upgraded).await;

        let stale_upgraded = extension_request(&control_project, "builder", &control_scope);
        assert_eq!(
            control_registry
                .check(
                    &stale_upgraded,
                    &control_scope.contract(ExtensionEffect::ReadOnly)
                )
                .await
                .expect_err("pre-upgrade scope must fail closed"),
            PortError::Failed("extension_scope_generation_stale".to_owned())
        );
        assert_eq!(
            control_broker
                .execute(stale_upgraded)
                .await
                .expect_err("pre-upgrade scope must not dispatch"),
            PortError::Failed("extension_scope_generation_stale".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let mut drifted_scope = control_scope.clone();
        drifted_scope.registry_generation = 2;
        drifted_scope.scope_digest = drifted_scope.digest();
        let drifted = extension_request(&control_project, "builder", &drifted_scope);
        assert_eq!(
            control_registry
                .check(&drifted, &drifted_scope.contract(ExtensionEffect::ReadOnly))
                .await
                .expect_err("old package hash must fail closed"),
            PortError::Failed("extension_execution_snapshot_inactive".to_owned())
        );
        assert_eq!(
            control_broker
                .execute(drifted)
                .await
                .expect_err("old package hash must not dispatch"),
            PortError::Failed("extension_execution_snapshot_inactive".to_owned())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    // ---- EXT-22: typed inspect → stage → install → enable lifecycle bindings ----

    fn fixture_package_file(root: &Path, package: &FixturePackage) -> String {
        let path = "extensions/review-skill.package.json";
        let file = root.join("project").join(path);
        std::fs::create_dir_all(file.parent().expect("fixture package parent"))
            .expect("fixture package directory");
        std::fs::write(&file, &package.bytes).expect("fixture package bytes");
        path.to_owned()
    }

    /// Build an authorized `extension.manage` request the way the composition root would.
    fn manage_request(
        project_root: &str,
        risk: RiskLevel,
        arguments: serde_json::Value,
    ) -> AuthorizedCapabilityRequest {
        let mut request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            EXTENSION_MANAGE_OPERATION,
            arguments,
        )
        .with_risk(risk);
        kiana_domain::normalize_capability_action(&mut request).expect("normalize fixture request");
        request.execution_scope = Some(execution_scope(&request));
        AuthorizedCapabilityRequest::new("extension:ext22-fixture", request).expect("authorized")
    }

    /// Server-owned fields the ControlPlane injects before the daemon ever sees the request.
    /// Clients cannot send these keys (the argument whitelist rejects them), so tests that want
    /// to prove forgery resistance only get them through this helper.
    fn server_owned_arguments(arguments: &mut serde_json::Value, project_root: &str) {
        let object = arguments.as_object_mut().expect("fixture arguments object");
        object.insert("operator_authorized".to_owned(), json!(true));
        object.insert("project_root".to_owned(), json!(project_root));
        object.insert("actor_id".to_owned(), json!("fixture-operator"));
        object.insert("role_id".to_owned(), json!("builder"));
        object.insert("department_id".to_owned(), json!("engineering"));
    }

    /// Mint the typed mutation digest exactly where the ControlPlane does: after the
    /// server-owned fields are present, over the fields the daemon will re-derive.
    fn bind_lifecycle_mutation(arguments: &mut serde_json::Value, project_root: &str) {
        server_owned_arguments(arguments, project_root);
        let mutation = kiana_domain::ExtensionLifecycleMutation::from_command_arguments(arguments)
            .expect("fixture lifecycle mutation");
        arguments
            .as_object_mut()
            .expect("fixture arguments object")
            .insert(
                "mutation_digest".to_owned(),
                json!(mutation.mutation_digest),
            );
    }

    fn package_mutation_arguments(
        action: &str,
        package: &FixturePackage,
        package_path: &str,
        expected_registry_version: u64,
        idempotency_key: &str,
        trust_revision: &str,
        dependency_snapshot_digest: &str,
    ) -> serde_json::Value {
        json!({
            "action": action,
            "extension_id": "review-skill",
            "package_path": package_path,
            "package_sha256": package.package_sha256,
            "expected_registry_version": expected_registry_version,
            "idempotency_key": idempotency_key,
            "reason": "EXT-22 lifecycle fixture",
            "trust_revision": trust_revision,
            "dependency_snapshot_digest": dependency_snapshot_digest,
        })
    }

    fn fixture_config_snapshot(registry_version: u64) -> ExtensionConfigSnapshot {
        ExtensionConfigSnapshot::new(
            "review-skill",
            registry_version,
            vec![
                ExtensionConfigEntry {
                    key: "greeting".to_owned(),
                    scope: ExtensionConfigScope::Host,
                    source_ref: "host:defaults".to_owned(),
                    value_digest: format!("sha256:{}", sha256(b"host-greeting")),
                    secret_handle: None,
                },
                ExtensionConfigEntry {
                    key: "greeting".to_owned(),
                    scope: ExtensionConfigScope::Run,
                    source_ref: "run:override".to_owned(),
                    value_digest: format!("sha256:{}", sha256(b"run-greeting")),
                    secret_handle: None,
                },
            ],
        )
        .expect("fixture config snapshot")
    }

    /// Ask the daemon for the read-only lifecycle binding a caller must carry into every
    /// mutation: registry generation, dependency snapshot digest and project trust revision.
    async fn inspect_lifecycle_binding(
        registry: &Arc<ExtensionRegistry>,
        project_root: &str,
        package_path: &str,
    ) -> (u64, String, String) {
        let mut arguments = json!({
            "action": "inspect",
            "extension_id": "review-skill",
            "package_path": package_path,
        });
        server_owned_arguments(&mut arguments, project_root);
        let inspected = registry
            .manage(&manage_request(
                project_root,
                RiskLevel::ReadOnly,
                arguments,
            ))
            .await
            .expect("lifecycle inspect");
        assert_eq!(inspected["signature_verified"], json!(true));
        (
            inspected["registry_version"]
                .as_u64()
                .expect("registry version"),
            inspected["dependency_snapshot_digest"]
                .as_str()
                .expect("dependency snapshot digest")
                .to_owned(),
            inspected["trust_revision"]
                .as_str()
                .expect("trust revision")
                .to_owned(),
        )
    }

    async fn folded_registry(
        registry: &Arc<ExtensionRegistry>,
        project_root: &str,
    ) -> (u64, BTreeMap<String, InstalledExtension>) {
        let (_, history) = registry.stream(project_root).await.expect("stream");
        fold_registry(&history).expect("fold registry")
    }

    /// EXT-22 deny-first fixture: forged or stale lifecycle bindings never reach the registry
    /// CAS. Each case fails with a stable reason code and appends zero events.
    #[tokio::test]
    async fn ext22_lifecycle_binding_rejects_forged_and_stale_mutations() {
        let package = fixture_package();
        let root = fixture_root("ext22-deny");
        let (project_root, _) = project_scope(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        let package_path = fixture_package_file(&root, &package);
        let (version, dependency_digest, trust_revision) =
            inspect_lifecycle_binding(&registry, &project_root, &package_path).await;
        assert_eq!(version, 0);

        // (a) no operator identity: a bare mutation never reaches argument parsing.
        let missing_operator = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-deny-operator",
            &trust_revision,
            &dependency_digest,
        );
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    missing_operator,
                ))
                .await
                .expect_err("operator identity required"),
            PortError::Failed("extension_operator_required".to_owned())
        );

        // (b) missing typed binding: the daemon refuses to infer trust/dependency state.
        let mut unbound = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-deny-unbound",
            &trust_revision,
            &dependency_digest,
        );
        server_owned_arguments(&mut unbound, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    unbound,
                ))
                .await
                .expect_err("missing mutation digest"),
            PortError::Failed("extension_lifecycle_mutation_digest_required".to_owned())
        );

        // (c) a client-minted (forged) mutation digest does not match the server derivation.
        let mut forged = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-deny-forged",
            &trust_revision,
            &dependency_digest,
        );
        bind_lifecycle_mutation(&mut forged, &project_root);
        forged["mutation_digest"] = json!(format!("sha256:{}", sha256(b"forged-mutation")));
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    forged,
                ))
                .await
                .expect_err("forged mutation digest"),
            PortError::Failed("extension_lifecycle_mutation_digest_mismatch".to_owned())
        );

        // (d) missing trust revision is a hard argument error, not an implicit "unknown".
        let mut missing_trust = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-deny-trust",
            &trust_revision,
            &dependency_digest,
        );
        missing_trust
            .as_object_mut()
            .expect("arguments object")
            .remove("trust_revision");
        server_owned_arguments(&mut missing_trust, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    missing_trust,
                ))
                .await
                .expect_err("missing trust revision"),
            PortError::Failed("extension_lifecycle_trust_revision_required".to_owned())
        );

        // (e) a digest-consistent but stale trust revision fails against the live project trust.
        let mut stale_trust = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-deny-stale-trust",
            "trust:stale-revision",
            &dependency_digest,
        );
        bind_lifecycle_mutation(&mut stale_trust, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    stale_trust,
                ))
                .await
                .expect_err("stale trust revision"),
            PortError::Failed("extension_trust_revision_stale".to_owned())
        );

        // (f) a digest-consistent dependency snapshot that no longer matches the folded registry.
        let stale_dependency = format!("sha256:{}", "d".repeat(64));
        let mut stale_snapshot = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-deny-stale-dependency",
            &trust_revision,
            &stale_dependency,
        );
        bind_lifecycle_mutation(&mut stale_snapshot, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    stale_snapshot,
                ))
                .await
                .expect_err("stale dependency snapshot"),
            PortError::Failed("extension_dependency_snapshot_stale".to_owned())
        );

        // (g) an enable carrying another extension's config snapshot is refused by the typed bind.
        let foreign_snapshot = ExtensionConfigSnapshot::new("other.extension", 1, Vec::new())
            .expect("foreign fixture snapshot");
        let mut foreign_bind = json!({
            "action": "enable",
            "extension_id": "review-skill",
            "package_sha256": package.package_sha256,
            "expected_registry_version": 1,
            "idempotency_key": "ext22-deny-config",
            "reason": "EXT-22 lifecycle fixture",
            "trust_revision": trust_revision,
            "dependency_snapshot_digest": dependency_digest,
            "approval_id": "approval.ext22-deny",
            "config_snapshot_digest": foreign_snapshot.snapshot_digest,
            "config_snapshot": foreign_snapshot,
        });
        bind_lifecycle_mutation(&mut foreign_bind, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    foreign_bind,
                ))
                .await
                .expect_err("foreign config snapshot"),
            PortError::Failed("extension_config_extension_mismatch".to_owned())
        );

        // (h) an enable with a bound digest but no snapshot body is refused before the CAS.
        let mut missing_snapshot = json!({
            "action": "enable",
            "extension_id": "review-skill",
            "package_sha256": package.package_sha256,
            "expected_registry_version": 1,
            "idempotency_key": "ext22-deny-missing-config",
            "reason": "EXT-22 lifecycle fixture",
            "trust_revision": trust_revision,
            "dependency_snapshot_digest": dependency_digest,
            "approval_id": "approval.ext22-deny",
            "config_snapshot_digest": format!("sha256:{}", sha256(b"absent-snapshot")),
        });
        bind_lifecycle_mutation(&mut missing_snapshot, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    missing_snapshot,
                ))
                .await
                .expect_err("missing config snapshot body"),
            PortError::Failed("extension_config_snapshot_required".to_owned())
        );

        // Nothing above may have appended a single lifecycle event.
        let (version, states) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 0);
        assert!(states.is_empty());
    }

    /// EXT-22 behavioral fixture: the full inspect → stage → re-stage → install → enable
    /// pipeline advances the append-only registry one CAS event at a time, keeps `stage`
    /// inert (no enable without an install), replays idempotently and rejects a stale
    /// registry generation without appending.
    #[tokio::test]
    async fn ext22_inspect_stage_install_enable_pipeline_binds_typed_lifecycle() {
        let package = fixture_package();
        let root = fixture_root("ext22-pipeline");
        let (project_root, _) = project_scope(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        let package_path = fixture_package_file(&root, &package);

        // inspect: read-only, no mutation binding required; the response hands the caller the
        // registry generation, dependency snapshot and trust revision to carry forward.
        let (version, dependency_digest, trust_revision) =
            inspect_lifecycle_binding(&registry, &project_root, &package_path).await;
        assert_eq!(version, 0);

        // stage: verified bytes enter the immutable cache and one inert `staged` fact is
        // appended. The receipt records the typed binding, not just the raw arguments.
        let mut stage = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-stage-1",
            &trust_revision,
            &dependency_digest,
        );
        bind_lifecycle_mutation(&mut stage, &project_root);
        let staged = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                stage,
            ))
            .await
            .expect("stage");
        assert_eq!(staged["action"], json!("stage"));
        assert_eq!(staged["state"], json!("staged"));
        assert_eq!(staged["signature_verified"], json!(true));
        assert_eq!(staged["registry_version"], json!(1));
        assert_eq!(staged["replayed"], json!(false));
        assert!(staged["lifecycle_binding"]["mutation_digest"].is_string());
        assert_eq!(
            staged["lifecycle_binding"]["dependency_snapshot_digest"],
            json!(&dependency_digest)
        );
        assert_eq!(
            staged["lifecycle_binding"]["trust_revision"],
            json!(&trust_revision)
        );

        // stage is inert: enable must fail closed until an install fact exists.
        let (version, dependency_digest, trust_revision) =
            inspect_lifecycle_binding(&registry, &project_root, &package_path).await;
        assert_eq!(version, 1);
        let premature_snapshot = fixture_config_snapshot(version);
        let mut premature = json!({
            "action": "enable",
            "extension_id": "review-skill",
            "package_sha256": package.package_sha256,
            "expected_registry_version": version,
            "idempotency_key": "ext22-enable-premature",
            "reason": "EXT-22 lifecycle fixture",
            "trust_revision": trust_revision,
            "dependency_snapshot_digest": dependency_digest,
            "approval_id": "approval.ext22-premature",
            "config_snapshot_digest": premature_snapshot.snapshot_digest,
            "config_snapshot": premature_snapshot,
        });
        bind_lifecycle_mutation(&mut premature, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    premature,
                ))
                .await
                .expect_err("staging must not enable"),
            PortError::Failed("extension_not_installed".to_owned())
        );

        // re-stage: same verified package, new idempotency key. Still inert, still stage-owned.
        let mut restage = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-stage-2",
            &trust_revision,
            &dependency_digest,
        );
        bind_lifecycle_mutation(&mut restage, &project_root);
        let restaged = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                restage,
            ))
            .await
            .expect("re-stage");
        assert_eq!(restaged["action"], json!("stage"));
        assert_eq!(restaged["state"], json!("staged"));
        assert_eq!(restaged["registry_version"], json!(2));

        // install: promotes the stage-owned record without re-verifying different bytes.
        let (version, dependency_digest, trust_revision) =
            inspect_lifecycle_binding(&registry, &project_root, &package_path).await;
        assert_eq!(version, 2);
        let mut install = package_mutation_arguments(
            "install",
            &package,
            &package_path,
            version,
            "ext22-install-1",
            &trust_revision,
            &dependency_digest,
        );
        bind_lifecycle_mutation(&mut install, &project_root);
        let installed = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                install,
            ))
            .await
            .expect("install");
        assert_eq!(installed["action"], json!("install"));
        assert_eq!(installed["state"], json!("staged"));
        assert_eq!(installed["registry_version"], json!(3));
        assert_eq!(installed["signature_verified"], json!(true));

        // after install the record is no longer stage-owned: another stage is refused.
        let (version, dependency_digest, trust_revision) =
            inspect_lifecycle_binding(&registry, &project_root, &package_path).await;
        assert_eq!(version, 3);
        let mut late_stage = package_mutation_arguments(
            "stage",
            &package,
            &package_path,
            version,
            "ext22-stage-late",
            &trust_revision,
            &dependency_digest,
        );
        bind_lifecycle_mutation(&mut late_stage, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    late_stage,
                ))
                .await
                .expect_err("installed packages use upgrade/rollback"),
            PortError::Failed("extension_already_installed".to_owned())
        );

        // enable: requires the operator approval and the typed host→run config snapshot.
        let committed_snapshot = fixture_config_snapshot(version);
        let committed_digest = committed_snapshot.snapshot_digest.clone();
        let mut enable = json!({
            "action": "enable",
            "extension_id": "review-skill",
            "package_sha256": package.package_sha256,
            "expected_registry_version": version,
            "idempotency_key": "ext22-enable-1",
            "reason": "EXT-22 lifecycle fixture",
            "trust_revision": trust_revision,
            "dependency_snapshot_digest": dependency_digest,
            "approval_id": "approval.ext22-enable",
            "config_snapshot_digest": committed_digest,
            "config_snapshot": committed_snapshot,
        });
        bind_lifecycle_mutation(&mut enable, &project_root);
        let replay_request = enable.clone();
        let enabled = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                enable,
            ))
            .await
            .expect("enable");
        assert_eq!(enabled["action"], json!("enable"));
        assert_eq!(enabled["state"], json!("enabled"));
        assert_eq!(enabled["registry_version"], json!(4));
        assert_eq!(
            enabled["lifecycle_binding"]["approval_id"],
            json!("approval.ext22-enable")
        );
        assert_eq!(
            enabled["lifecycle_binding"]["config_snapshot_digest"],
            json!(&committed_digest)
        );
        let effective = enabled["lifecycle_binding"]["config_effective"]
            .as_array()
            .expect("effective config projection")
            .clone();
        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0]["scope"], json!("run"));
        assert_eq!(effective[0]["source_ref"], json!("run:override"));
        assert_eq!(
            enabled["command_receipt"]["approval_id"],
            json!("approval.ext22-enable")
        );
        assert_eq!(
            enabled["command_receipt"]["mutation_digest"],
            enabled["lifecycle_binding"]["mutation_digest"]
        );

        let (version, states) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 4);
        assert_eq!(states["review-skill"].state, "enabled");

        // idempotent replay returns the previous receipt without appending a second event.
        let replayed = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                replay_request,
            ))
            .await
            .expect("replay");
        assert_eq!(replayed["replayed"], json!(true));
        assert_eq!(replayed["registry_version"], json!(4));
        assert_eq!(replayed["event_id"], enabled["event_id"]);
        let (version, _) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 4);

        // a stale registry generation is a conflict, not an overwrite.
        let mut stale = json!({
            "action": "disable",
            "extension_id": "review-skill",
            "expected_registry_version": 2,
            "idempotency_key": "ext22-disable-stale",
            "reason": "EXT-22 lifecycle fixture",
        });
        server_owned_arguments(&mut stale, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    stale,
                ))
                .await
                .expect_err("stale registry generation"),
            PortError::Conflict("extension_registry_version_mismatch".to_owned())
        );
        let (version, _) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 4);
    }

    // ---- EXT-27: one redacted, trust-filtered visibility projection for every surface ----

    /// Body/path/secret markers a public projection must never carry.
    const EXT27_HIDDEN_BODY: &str = "ext27-hidden-body-token-3f9a";
    const EXT27_HIDDEN_PATH: &str = "/opt/ext27/private/skills/secret-source.pem";
    const EXT27_HIDDEN_SECRET: &str = "ext27-hidden-secret-8811";

    /// Write a project Skill whose body carries material a naive projection would leak.
    fn ext27_project_skill(root: &Path) {
        let dir = root
            .join("project")
            .join(".claude")
            .join("skills")
            .join("ext27-hidden-skill");
        std::fs::create_dir_all(&dir).expect("ext27 fixture skill dir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\ndescription: EXT-27 fixture skill api_key={EXT27_HIDDEN_SECRET}\n---\n{EXT27_HIDDEN_BODY}\n{EXT27_HIDDEN_PATH}\n"
            ),
        )
        .expect("ext27 fixture skill body");
    }

    /// Ask for the projection through the same `extension.manage` read path a surface uses.
    async fn ext27_visibility(
        registry: &Arc<ExtensionRegistry>,
        project_root: &str,
        role_id: &str,
        action: &str,
        query: Option<&str>,
    ) -> Result<Value, PortError> {
        let mut arguments = json!({"action": action});
        if let Some(query) = query {
            arguments["query"] = json!(query);
        }
        server_owned_arguments(&mut arguments, project_root);
        arguments["role_id"] = json!(role_id);
        registry
            .manage(&manage_request(
                project_root,
                RiskLevel::ReadOnly,
                arguments,
            ))
            .await
    }

    async fn ext27_snapshot(
        registry: &Arc<ExtensionRegistry>,
        project_root: &str,
        role_id: &str,
        action: &str,
        query: Option<&str>,
    ) -> kiana_domain::ExtensionVisibilitySnapshot {
        let output = ext27_visibility(registry, project_root, role_id, action, query)
            .await
            .expect("ext27 visibility projection");
        serde_json::from_value(output["snapshot"].clone()).expect("ext27 visibility snapshot")
    }

    /// EXT-27 deny-first daemon fixture: the registry mints exactly one redacted projection per
    /// (project, role, material) and every surface reads it. An unknown-trust project exposes no
    /// project material, the registry row is downgraded to a diagnostic without an activation
    /// label, role drift removes the row, the query stays bounded and labels stay intents.
    #[tokio::test]
    async fn ext27_visibility_projection_is_redacted_bounded_and_role_scoped() {
        let package = fixture_package();
        let root = fixture_root("ext27-visibility");
        let (project_root, scope_id) = project_scope(&root);
        ext27_project_skill(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        append_registry_state(&events, &scope_id, 1, &installed_state(&package, "enabled")).await;

        // (a) unknown trust: an untrusted project's Skill is not even loaded, and the refusal to
        // activate is decided by the server, not by the label the caller happens to see.
        let snapshot = ext27_snapshot(&registry, &project_root, "builder", "list", None).await;
        assert!(
            snapshot
                .inspect("ext27-hidden-skill")
                .expect("inspect hidden skill id")
                .is_none(),
            "an untrusted project's Skill must not enter the projection"
        );
        let encoded = serde_json::to_string(&snapshot).expect("ext27 encoded snapshot");
        for marker in [
            EXT27_HIDDEN_BODY,
            EXT27_HIDDEN_PATH,
            EXT27_HIDDEN_SECRET,
            "SKILL.md",
            ".claude",
        ] {
            assert!(
                !encoded.contains(marker),
                "project material leaked into the projection: {marker}"
            );
        }
        let entry = snapshot
            .inspect("review-skill")
            .expect("inspect fixture extension")
            .expect("the registry row is projected as a diagnostic");
        assert_eq!(entry.kind, kiana_domain::ExtensionVisibilityKind::Skill);
        assert_eq!(entry.risk, kiana_domain::ExtensionVisibilityRisk::ReadOnly);
        assert_eq!(
            entry.source.trust,
            kiana_domain::ExtensionVisibilityTrust::Unknown
        );
        assert_eq!(
            entry.status,
            kiana_domain::ExtensionVisibilityStatus::Stale,
            "an untrusted source is a stale diagnostic, never an installable row"
        );
        assert!(
            !entry
                .actions
                .contains(&kiana_domain::ExtensionVisibilityActionKind::Activate),
            "an untrusted source must not offer an activation label"
        );
        assert!(
            entry
                .actions
                .contains(&kiana_domain::ExtensionVisibilityActionKind::Inspect),
            "read-only labels stay available for diagnostics"
        );
        assert_eq!(
            snapshot
                .action(
                    "review-skill",
                    kiana_domain::ExtensionVisibilityActionKind::Activate,
                    "EXT-27 activation probe",
                )
                .expect_err("an untrusted source must not mint an activation intent"),
            "extension_visibility_action_denied"
        );
        assert_eq!(
            snapshot
                .action(
                    "ext27-hidden-skill",
                    kiana_domain::ExtensionVisibilityActionKind::Inspect,
                    "EXT-27 hidden probe",
                )
                .expect_err("an unlisted extension has no intent"),
            "extension_visibility_not_found"
        );

        // (b) bounded query: bodies and locators are not searchable, and an oversized query is
        // refused before it can reach the catalog.
        for query in [EXT27_HIDDEN_BODY, EXT27_HIDDEN_PATH, EXT27_HIDDEN_SECRET] {
            let searched =
                ext27_snapshot(&registry, &project_root, "builder", "search", Some(query)).await;
            assert!(
                searched.entries.is_empty(),
                "hidden body/path/secret material must not be searchable: {query}"
            );
        }
        let too_long = "x".repeat(kiana_domain::MAX_EXTENSION_VISIBILITY_QUERY_BYTES + 1);
        assert_eq!(
            ext27_visibility(
                &registry,
                &project_root,
                "builder",
                "search",
                Some(&too_long)
            )
            .await
            .expect_err("oversized query must fail closed"),
            PortError::Failed("extension_visibility_query_invalid".to_owned())
        );

        // (c) role drift: a role the manifest was not signed for never sees the row.
        let drifted = ext27_snapshot(&registry, &project_root, "architect", "list", None).await;
        assert!(
            drifted
                .inspect("review-skill")
                .expect("inspect under role drift")
                .is_none(),
            "role drift must not project another role's extension"
        );

        // (d) an inspect for an extension that is not in the projection is a stable not-found.
        let mut arguments = json!({"action": "inspect", "extension_id": "ext27-absent-skill"});
        server_owned_arguments(&mut arguments, &project_root);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ReadOnly,
                    arguments,
                ))
                .await
                .expect_err("unknown extension id"),
            PortError::Failed("extension_visibility_not_found".to_owned())
        );
    }

    /// EXT-27 deny-first fixture: a cached projection is bound to one material identity, so a
    /// registry generation advance refuses the old id/generation pair instead of letting a stale
    /// surface reuse it as an authority. Read-only labels stay intents bound to that identity.
    #[tokio::test]
    async fn ext27_visibility_projection_identity_fences_stale_surface_caches() {
        let package = fixture_package();
        let root = fixture_root("ext27-generation");
        let (project_root, scope_id) = project_scope(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        append_registry_state(&events, &scope_id, 1, &installed_state(&package, "enabled")).await;

        // The projection a surface cached: id, generation and digest travel together.
        let cached = ext27_snapshot(&registry, &project_root, "builder", "list", None).await;
        let cached_intent = cached
            .action(
                "review-skill",
                kiana_domain::ExtensionVisibilityActionKind::Inspect,
                "EXT-27 cached read-only intent",
            )
            .expect("a read-only label mints an intent");
        assert_eq!(cached_intent.snapshot_id, cached.snapshot_id);
        assert_eq!(cached_intent.snapshot_digest, cached.snapshot_digest);
        cached_intent
            .validate()
            .expect("intent stays self-consistent");
        cached
            .require_generation(cached_intent.snapshot_id, cached_intent.generation)
            .expect("a fresh cache passes its own fence");

        // The registry advances one CAS generation: the same project folds to a different
        // material digest, so the cached identity is refused instead of being reused.
        append_registry_state(
            &events,
            &scope_id,
            2,
            &installed_state(&package, "disabled"),
        )
        .await;
        let fresh = ext27_snapshot(&registry, &project_root, "builder", "list", None).await;
        assert_ne!(
            cached.snapshot_id, fresh.snapshot_id,
            "a registry advance must change the projection identity"
        );
        assert_ne!(cached.snapshot_digest, fresh.snapshot_digest);
        assert_eq!(
            fresh
                .require_generation(cached_intent.snapshot_id, cached_intent.generation)
                .expect_err("a stale cache must be refused"),
            "extension_visibility_snapshot_stale"
        );
        assert_eq!(
            cached
                .require_generation(fresh.snapshot_id, fresh.generation)
                .expect_err("a newer projection is not a valid precondition"),
            "extension_visibility_snapshot_stale"
        );
        assert_eq!(
            fresh
                .require_generation(fresh.snapshot_id, fresh.generation.saturating_add(1))
                .expect_err("generation drift alone is stale"),
            "extension_visibility_snapshot_stale"
        );
        fresh
            .require_generation(fresh.snapshot_id, fresh.generation)
            .expect("the live pair passes");

        // The label remains an intent: it carries the snapshot identity, never an authorization.
        assert_ne!(fresh.snapshot_digest, cached_intent.snapshot_digest);
    }

    // ---- EXT-28: typed command contract, permission errors and receipt audit association ----

    /// A read-only `list` command plus the server-owned fields, the way the ControlPlane injects
    /// them after a request passes the entrypoint.
    fn ext28_list_arguments(project_root: &str) -> serde_json::Value {
        let mut arguments = json!({"action": "list", "max_results": 64});
        server_owned_arguments(&mut arguments, project_root);
        arguments
    }

    /// A `disable` mutation carrying the operator binding EXT-28 requires from every state change.
    fn ext28_disable_arguments(
        project_root: &str,
        expected_registry_version: u64,
        idempotency_key: &str,
    ) -> serde_json::Value {
        let mut arguments = json!({
            "action": "disable",
            "extension_id": "review-skill",
            "expected_registry_version": expected_registry_version,
            "idempotency_key": idempotency_key,
            "reason": "EXT-28 command fixture",
        });
        server_owned_arguments(&mut arguments, project_root);
        arguments
    }

    /// Build an authorized `extension.manage` request without the adapter's action-contract
    /// normalization. EXT-28 uses it only to prove the daemon refuses an under-declared mutation
    /// even when a hand-built request skipped the entrypoint check (defense in depth).
    fn raw_manage_request(
        risk: RiskLevel,
        arguments: serde_json::Value,
    ) -> AuthorizedCapabilityRequest {
        let mut request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            EXTENSION_MANAGE_OPERATION,
            arguments,
        )
        .with_risk(risk);
        request.execution_scope = Some(execution_scope(&request));
        AuthorizedCapabilityRequest::new("extension:ext28-fixture", request).expect("authorized")
    }

    /// EXT-28 deny-first daemon fixture. A read-only command is free and appends no fact, while
    /// every state change is refused unless the caller is an operator outside a cell and carries
    /// the expected registry generation. Only the accepted command commits one CAS fact, and the
    /// typed receipt it returns resolves to exactly that committed event.
    #[tokio::test]
    async fn ext28_commands_are_permission_bound_and_the_receipt_resolves_to_the_committed_event() {
        let package = fixture_package();
        let root = fixture_root("ext28-commands");
        let (project_root, scope_id) = project_scope(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        append_registry_state(&events, &scope_id, 1, &installed_state(&package, "enabled")).await;

        // (a) read-only list: no mutation binding, no approval, no lifecycle fact.
        let listed = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ReadOnly,
                ext28_list_arguments(&project_root),
            ))
            .await
            .expect("read-only list");
        assert_eq!(listed["registry_version"], json!(1));
        assert!(listed["snapshot"].is_object());

        // (b) a state change that declares read-only risk is refused before it can reach a
        // state transition: the daemon re-checks the declared risk even for a hand-built request
        // that never went through the adapter's action contract.
        assert_eq!(
            registry
                .manage(&raw_manage_request(
                    RiskLevel::ReadOnly,
                    ext28_disable_arguments(&project_root, 1, "ext28-downgraded"),
                ))
                .await
                .expect_err("a mutation may not declare read-only risk"),
            PortError::Failed("extension_approval_risk_required".to_owned())
        );

        // (c) no operator identity: the mutation is refused before any argument is trusted.
        let mut missing_operator = ext28_disable_arguments(&project_root, 1, "ext28-no-operator");
        missing_operator["operator_authorized"] = json!(false);
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    missing_operator,
                ))
                .await
                .expect_err("operator identity required"),
            PortError::Failed("extension_operator_required".to_owned())
        );

        // (d) a cell-scoped request never owns extension state changes.
        let mut cell_request = manage_request(
            &project_root,
            RiskLevel::ExternalSideEffect,
            ext28_disable_arguments(&project_root, 1, "ext28-cell"),
        );
        cell_request.request.cell_id = Some(kiana_domain::CellId::new());
        assert_eq!(
            registry
                .manage(&cell_request)
                .await
                .expect_err("cell context must not mutate the registry"),
            PortError::Failed("extension_operator_required".to_owned())
        );

        // (e) the expected registry generation is a required precondition, not a hint.
        let mut mut_no_version = ext28_disable_arguments(&project_root, 1, "ext28-no-version");
        mut_no_version
            .as_object_mut()
            .expect("arguments object")
            .remove("expected_registry_version");
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    mut_no_version,
                ))
                .await
                .expect_err("expected registry version required"),
            PortError::Failed("extension_expected_registry_version_required".to_owned())
        );

        // None of the refusals above may have appended a fact.
        let (version, states) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 1);
        assert_eq!(states["review-skill"].state, "enabled");

        // (f) the accepted command commits exactly one CAS fact and returns the typed receipt.
        let committed = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                ext28_disable_arguments(&project_root, 1, "ext28-disable-1"),
            ))
            .await
            .expect("disable");
        assert_eq!(committed["state"], json!("disabled"));
        assert_eq!(committed["registry_version"], json!(2));
        assert_eq!(committed["replayed"], json!(false));
        let receipt = ExtensionCommandReceipt::from_response_output(&committed)
            .expect("typed receipt projection");
        assert_eq!(receipt.command, ExtensionCommand::Disable);
        assert_eq!(receipt.extension_id, "review-skill");
        assert_eq!(receipt.expected_registry_version, 1);
        assert_eq!(receipt.registry_version, 2);
        assert!(!receipt.replayed);
        assert_eq!(receipt.event_id, committed["event_id"]);
        receipt.validate().expect("receipt self-check");

        // Audit association: the receipt resolves to the committed event, not to a parallel
        // lifecycle envelope, and the event stream grew by exactly one fact.
        let (_, history) = registry.stream(&project_root).await.expect("stream");
        assert_eq!(history.len(), 2);
        let appending = history.last().expect("committed disable fact");
        assert_eq!(appending.kind, "extension.lifecycle");
        assert_eq!(appending.stream_version, Some(2));
        assert_eq!(appending.data["action"], json!("disable"));
        assert_eq!(appending.event_id.to_string(), receipt.event_id);
        let (version, states) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 2);
        assert_eq!(states["review-skill"].state, "disabled");
    }

    /// EXT-28 deny-first daemon fixture: a replayed command returns the original typed receipt
    /// and event identity without appending a second fact, and the same idempotency key with a
    /// different payload is a conflict that leaves the registry untouched.
    #[tokio::test]
    async fn ext28_replayed_commands_return_the_same_receipt_and_event_without_a_second_fact() {
        let package = fixture_package();
        let root = fixture_root("ext28-replay");
        let (project_root, scope_id) = project_scope(&root);
        let events: Arc<dyn EventStorePort> = Arc::new(kiana_eventlog::MemoryEventLog::new());
        let registry = fixture_registry(&root, &package, events.clone());
        append_registry_state(&events, &scope_id, 1, &installed_state(&package, "enabled")).await;

        let disable = ext28_disable_arguments(&project_root, 1, "ext28-replay-1");
        let committed = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                disable.clone(),
            ))
            .await
            .expect("disable");
        let receipt =
            ExtensionCommandReceipt::from_response_output(&committed).expect("committed receipt");
        assert!(!receipt.replayed);

        let replayed = registry
            .manage(&manage_request(
                &project_root,
                RiskLevel::ExternalSideEffect,
                disable.clone(),
            ))
            .await
            .expect("idempotent replay");
        assert_eq!(replayed["replayed"], json!(true));
        assert_eq!(replayed["event_id"], committed["event_id"]);
        let replay_receipt = ExtensionCommandReceipt::from_response_output(&replayed)
            .expect("a replayed command answers with the same typed receipt");
        assert!(replay_receipt.replayed);
        assert_eq!(replay_receipt.event_id, receipt.event_id);
        assert_eq!(
            replay_receipt.expected_registry_version,
            receipt.expected_registry_version
        );
        assert_eq!(replay_receipt.registry_version, receipt.registry_version);
        assert_eq!(replayed["command_receipt"]["command"], json!("disable"));

        // Payload drift under one idempotency key is a conflict, not a second mutation.
        let mut drifted = disable;
        drifted["reason"] = json!("EXT-28 drifted replay");
        assert_eq!(
            registry
                .manage(&manage_request(
                    &project_root,
                    RiskLevel::ExternalSideEffect,
                    drifted,
                ))
                .await
                .expect_err("payload drift under one key"),
            PortError::Conflict("extension_idempotency_payload_mismatch".to_owned())
        );
        let (version, states) = folded_registry(&registry, &project_root).await;
        assert_eq!(version, 2);
        assert_eq!(states["review-skill"].state, "disabled");
    }
}
