//! Replayable local Connector adapter. No network request or external account mutation.

use crate::local_packages::{failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_capability_broker::{
    validate_connector_quota_boundary, CapabilityBroker, CapabilityHandler,
    ConnectorCredentialLeaseFence,
};
use kiana_domain::{
    connector_bindings, connector_fixture_hash_matches, connector_fixture_hash_valid,
    connector_payload_sha256, AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult,
    ConnectorBindingSnapshot, ConnectorCredentialEvidence, ConnectorCredentialInvocation,
    ConnectorDispatchLifecycle, ConnectorDispatchStage, ConnectorFixture, ConnectorHealthFact,
    ConnectorHealthStatus, ConnectorReconciliationCase, ConnectorReconciliationReason,
    ConnectorReconciliationSource, EffectObservation, ExecutionId, InvocationId, ProviderOutcome,
    ProviderReceipt, ProviderReceiptBinding, RuntimeEvent, CONNECTOR_CREDENTIAL_MAX_TTL_MS,
    CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND, CONNECTOR_FIXTURE_MAX_BYTES,
    CONNECTOR_HEALTH_EVENT_KIND, CONNECTOR_HEALTH_OPERATION, CONNECTOR_INVOKE_OPERATION,
    CONNECTOR_MANAGE_OPERATION, CONNECTOR_MCP_HANDSHAKE_EVENT_KIND,
    CONNECTOR_MCP_HANDSHAKE_OPERATION, CONNECTOR_STREAM,
};
use kiana_ports::{
    EventAppendResult, EventStorePort, McpCapabilityHandshakeRequest, PortError,
    CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA,
};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn register(
    broker: &mut CapabilityBroker,
    events: Arc<dyn EventStorePort>,
    mcp: Arc<crate::harness_mcp::McpRegistry>,
) -> Result<(), PortError> {
    let journal = Arc::new(ConnectorEventJournal { events });
    let registry = Arc::new(ConnectorRegistry {
        journal,
        mcp,
        credential_fence: std::sync::Mutex::new(ConnectorCredentialLeaseFence::new()),
    });
    broker.register_static(
        CapabilityKind::Tool,
        CONNECTOR_MANAGE_OPERATION,
        registry.clone(),
    )?;
    broker.register_static(
        CapabilityKind::Tool,
        CONNECTOR_HEALTH_OPERATION,
        registry.clone(),
    )?;
    broker.register_static(
        CapabilityKind::Tool,
        CONNECTOR_MCP_HANDSHAKE_OPERATION,
        registry.clone(),
    )?;
    broker.register_static(CapabilityKind::Tool, CONNECTOR_INVOKE_OPERATION, registry)
}

struct ConnectorRegistry {
    journal: Arc<ConnectorEventJournal>,
    mcp: Arc<crate::harness_mcp::McpRegistry>,
    /// Server-owned one-shot credential admission ledger.  It holds lease digests only and is
    /// in-process; restart/cross-process fencing stays with INT-18/INT-29.
    credential_fence: std::sync::Mutex<ConnectorCredentialLeaseFence>,
}

/// The capability handler never receives an EventStore directly.  This narrow journal owns
/// connector fact persistence and gives the dispatch path an explicit commit boundary.
struct ConnectorEventJournal {
    events: Arc<dyn EventStorePort>,
}

struct ConnectorDispatchOutcome {
    receipt: ProviderReceipt,
    observation: EffectObservation,
    lifecycle: ConnectorDispatchLifecycle,
    version: u64,
    credential_evidence: Option<ConnectorCredentialEvidence>,
}

impl ConnectorEventJournal {
    async fn read_stream(
        &self,
        stream: &str,
        aggregate_id: &str,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        self.events.read_stream(stream, aggregate_id).await
    }

    async fn append_idempotent_expected(
        &self,
        event: RuntimeEvent,
        expected_version: Option<u64>,
    ) -> Result<EventAppendResult, PortError> {
        self.events
            .append_idempotent_expected(event, expected_version)
            .await
    }

    async fn append_lifecycle(
        &self,
        request_id: kiana_domain::RequestId,
        project: &str,
        lifecycle: &ConnectorDispatchLifecycle,
        expected_version: u64,
    ) -> Result<EventAppendResult, PortError> {
        lifecycle.validate().map_err(failed)?;
        let event = RuntimeEvent::new(
            request_id,
            1,
            CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND,
            json!({
                "schema": "kiana.connector-dispatch-lifecycle-event.v1",
                "project_root": project,
                "invocation_id": lifecycle.invocation_id,
                "attempt": lifecycle.attempt,
                "lifecycle": lifecycle,
                "proof_level": "source",
            }),
        )
        .map_err(|error| failed(error.to_string()))?
        .with_stream_metadata(
            CONNECTOR_STREAM,
            project,
            expected_version.saturating_add(1),
        );
        self.append_idempotent_expected(event, Some(expected_version))
            .await
    }
}

impl ConnectorRegistry {
    async fn handle_health(
        &self,
        request: &AuthorizedCapabilityRequest,
        args: &Value,
        project: &str,
        actor: &str,
        history: &[RuntimeEvent],
        bindings: &std::collections::BTreeMap<String, ConnectorBindingSnapshot>,
    ) -> Result<Value, PortError> {
        if request.request.cell_id.is_some()
            || args["operator_authorized"] != true
            || args["binding_authorized"] != true
        {
            return Err(failed("connector_operator_required"));
        }
        let binding_id = string(args, "binding_id")?;
        let snapshot: ConnectorBindingSnapshot =
            serde_json::from_value(args["binding_snapshot"].clone())
                .map_err(|_| failed("connector_binding_snapshot_required"))?;
        if snapshot.project_root != project
            || snapshot.binding.binding_id != binding_id
            || bindings.get(binding_id) != Some(&snapshot)
        {
            return Err(failed("connector_binding_snapshot_changed"));
        }
        if snapshot.status != "active" {
            return Err(failed("connector_binding_revoked"));
        }

        // A retry of the same request must return the original observation instead of writing a
        // second fact with a new timestamp. The key is server-derived from project, binding and
        // request id; a different request id is a new probe by design.
        let request_fingerprint = sha256(
            &serde_json::to_vec(&kiana_domain::canonical_json(args.clone()))
                .map_err(|error| failed(error.to_string()))?,
        );
        let key = format!(
            "connector-health:{}:{}:{}",
            sha256(project.as_bytes()),
            binding_id,
            request.request.request_id
        );
        if let Some(previous) = history
            .iter()
            .find(|event| event.idempotency_key.as_deref() == Some(key.as_str()))
        {
            return health_replay_result(previous, history, binding_id);
        }

        let now = now_ms()?;
        let read_only_operation = snapshot
            .definition
            .operations
            .iter()
            .find(|(_, operation)| operation.effect == kiana_domain::ConnectorEffect::ReadOnly)
            .map(|(name, _)| name.clone());
        let (status, error_code, limitations) = match read_only_operation.as_deref() {
            // A connector that only declares write operations cannot be health-checked through a
            // destructive probe. It is unsupported, not a scope failure.
            None => (
                ConnectorHealthStatus::Unsupported,
                Some("connector_read_only_probe_unsupported".to_owned()),
                vec!["read_only_probe_contract_missing".to_owned()],
            ),
            Some(operation)
                if matches!(
                    snapshot.operation(operation),
                    Err("connector_account_scope_denied")
                ) =>
            {
                (
                    ConnectorHealthStatus::ScopeInsufficient,
                    Some("connector_account_scope_denied".to_owned()),
                    vec!["required_scope_not_granted".to_owned()],
                )
            }
            Some(operation) if snapshot.operation(operation).is_err() => (
                ConnectorHealthStatus::Unsupported,
                Some("connector_read_only_probe_unsupported".to_owned()),
                vec!["read_only_probe_contract_missing".to_owned()],
            ),
            _ => match load_fixture(&snapshot).await {
                Ok(_) => (
                    ConnectorHealthStatus::ConnectivityOnly,
                    None,
                    vec![
                        "local_fixture_only".to_owned(),
                        "external_provider_not_probed".to_owned(),
                    ],
                ),
                Err(error) => {
                    let (status, code) =
                        kiana_domain::classify_connector_probe_error(&error.to_string());
                    (
                        status,
                        Some(code),
                        vec![
                            "local_fixture_only".to_owned(),
                            "probe_error_redacted".to_owned(),
                        ],
                    )
                }
            },
        };
        let mut limitations = limitations;
        limitations.truncate(kiana_domain::CONNECTOR_HEALTH_MAX_LIMITATIONS);
        let evidence_digest = Some(kiana_domain::json_digest(&json!({
            "connector_id": &snapshot.definition.connector_id,
            "binding_id": &snapshot.binding.binding_id,
            "fixture_sha256": &snapshot.binding.fixture_sha256,
            "status": status,
        })));
        let fact = ConnectorHealthFact::new(
            snapshot.definition.connector_id.clone(),
            snapshot.binding.binding_id.clone(),
            status,
            "read_only",
            now,
            evidence_digest,
            error_code,
            snapshot.revision,
            snapshot
                .binding
                .credential_ref
                .as_ref()
                .map(|reference| reference.generation),
            kiana_domain::CONNECTOR_FIXTURE_SOURCE,
            limitations,
        )
        .map_err(failed)?;
        let next_version = history
            .last()
            .and_then(|event| event.stream_version)
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| failed("connector_registry_version_exhausted"))?;
        let data = json!({
            "schema":"kiana.connector-health-event.v1",
            "request_id":request.request.request_id,
            "connector_id":&fact.connector_id,
            "binding_id":&fact.binding_id,
            "status":fact.status,
            "probe_kind":&fact.probe_kind,
            "checked_at_unix_ms":fact.checked_at_unix_ms,
            "health":fact,
            "project_root":project,
            "actor_id":actor,
            "authorization_id":request.authorization_id,
            "request_fingerprint":request_fingerprint,
        });
        let event = RuntimeEvent::new(
            request.request.request_id,
            1,
            CONNECTOR_HEALTH_EVENT_KIND,
            data,
        )
        .map_err(|error| failed(error.to_string()))?
        .with_stream_metadata(CONNECTOR_STREAM, project, next_version)
        .with_idempotency_key(key.clone());
        let appended = match self
            .journal
            .append_idempotent_expected(event, Some(next_version - 1))
            .await
        {
            Ok(result) => result,
            // A concurrent retry may have committed between our history read and append. The
            // candidate payload differs by timestamp, so the store reports a payload conflict;
            // resolve it by replaying the winner with the same server key.
            Err(PortError::Conflict(code)) if code == "event_idempotency_key_payload_mismatch" => {
                let latest = self.journal.read_stream(CONNECTOR_STREAM, project).await?;
                if let Some(previous) = latest
                    .iter()
                    .find(|event| event.idempotency_key.as_deref() == Some(key.as_str()))
                {
                    return health_replay_result(previous, &latest, binding_id);
                }
                return Err(PortError::Conflict(code));
            }
            Err(PortError::Conflict(code)) => return Err(PortError::Conflict(code)),
            Err(other) => {
                return Err(failed(format!(
                    "result_unknown:connector_health_persist:{other}"
                )))
            }
        };
        let health = appended.event.data["health"].clone();
        let replayed = appended.replayed;
        let mut projection_events = history.to_vec();
        if !replayed {
            projection_events.push(appended.event.clone());
        }
        let health_projection =
            kiana_query::project_connector_health(&projection_events).map_err(failed)?;
        Ok(json!({
            "schema":"kiana.connector-health-result.v1",
            "health":health,
            "health_projection":health_projection,
            "source_cursor":next_version,
            "projection_version":"connector-health.v1",
            "stale":false,
            "proof_level":"source",
            "event_id":appended.event.event_id,
            "replayed":replayed,
        }))
    }

    async fn handle_mcp_handshake(
        &self,
        request: &AuthorizedCapabilityRequest,
        args: &Value,
        project: &str,
        actor: &str,
        history: &[RuntimeEvent],
        bindings: &std::collections::BTreeMap<String, ConnectorBindingSnapshot>,
    ) -> Result<Value, PortError> {
        if request.request.cell_id.is_some()
            || args["operator_authorized"] != true
            || args["binding_authorized"] != true
        {
            return Err(failed("connector_operator_required"));
        }
        let binding_id = string(args, "binding_id")?;
        let server = string(args, "server")?;
        let session_ref = string(args, "session_ref")?;
        let snapshot: ConnectorBindingSnapshot =
            serde_json::from_value(args["binding_snapshot"].clone())
                .map_err(|_| failed("connector_binding_snapshot_required"))?;
        if snapshot.project_root != project
            || snapshot.binding.binding_id != binding_id
            || bindings.get(binding_id) != Some(&snapshot)
        {
            return Err(failed("connector_binding_snapshot_changed"));
        }
        if snapshot.status != "active" {
            return Err(failed("connector_binding_revoked"));
        }
        if snapshot.definition.adapter != "stdio_mcp" {
            return Err(failed("mcp_http_unsupported"));
        }
        let handshake_request = McpCapabilityHandshakeRequest {
            schema: CONNECTOR_MCP_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
            binding: snapshot.clone(),
            server: server.to_owned(),
            session_ref: session_ref.to_owned(),
        };
        let handshake = self.mcp.connector_handshake(&handshake_request).await?;
        let next_version = history
            .last()
            .and_then(|event| event.stream_version)
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| failed("connector_registry_version_exhausted"))?;
        let event = RuntimeEvent::new(
            request.request.request_id,
            1,
            CONNECTOR_MCP_HANDSHAKE_EVENT_KIND,
            json!({
                "schema": "kiana.connector-mcp-handshake-event.v1",
                "request_id": request.request.request_id,
                "connector_id": snapshot.definition.connector_id,
                "binding_id": binding_id,
                "server": server,
                "actor_id": actor,
                "project_root": project,
                "authorization_id": request.authorization_id,
                "handshake": handshake,
                "proof_level": "source",
            }),
        )
        .map_err(|error| failed(error.to_string()))?
        .with_stream_metadata(CONNECTOR_STREAM, project, next_version)
        .with_idempotency_key(format!(
            "connector-mcp-handshake:{}:{}:{}",
            sha256(project.as_bytes()),
            binding_id,
            server
        ));
        let appended = self
            .journal
            .append_idempotent_expected(event, Some(next_version - 1))
            .await
            .map_err(|error| match error {
                PortError::Conflict(_) => error,
                other => failed(format!(
                    "result_unknown:connector_mcp_handshake_persist:{other}"
                )),
            })?;
        Ok(json!({
            "schema": "kiana.connector-mcp-handshake-result.v1",
            "handshake": appended.event.data["handshake"],
            "binding_id": binding_id,
            "server": server,
            "event_id": appended.event.event_id,
            "source_cursor": next_version,
            "replayed": appended.replayed,
            "proof_level": "source",
        }))
    }

    async fn dispatch_local_fixture(
        &self,
        request: &AuthorizedCapabilityRequest,
        project: &str,
        actor: &str,
        adapter: &LocalFixtureAdapter,
        credential_evidence: Option<ConnectorCredentialEvidence>,
        command_digest: &str,
        starting_version: u64,
        now: u64,
    ) -> Result<ConnectorDispatchOutcome, PortError> {
        let attempt = adapter.attempt;
        let prepared = ConnectorDispatchLifecycle::prepared(
            InvocationId::from_uuid(request.request.request_id.as_uuid()),
            attempt,
            command_digest.to_owned(),
            kiana_domain::json_digest(&json!({"binding": &adapter.binding})),
            kiana_domain::json_digest(&kiana_domain::canonical_json(adapter.payload.clone())),
            kiana_domain::json_digest(&json!({
                "idempotency_key": &adapter.idempotency_key
            })),
        )
        .map_err(failed)?;
        self.append_lifecycle(request, project, &prepared, starting_version)
            .await?;
        let dispatching = prepared
            .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
            .map_err(failed)?;
        self.append_lifecycle(request, project, &dispatching, starting_version + 1)
            .await?;

        let receipt = match adapter.dispatch().await {
            Ok(receipt) => receipt,
            Err(error) => {
                let unknown = dispatching
                    .advance(
                        ConnectorDispatchStage::Unknown,
                        None,
                        None,
                        None,
                        Some("connector_adapter_dispatch_unknown".to_owned()),
                    )
                    .map_err(failed)?;
                self.append_lifecycle(request, project, &unknown, starting_version + 2)
                    .await?;
                return Err(failed(format!(
                    "result_unknown:connector_adapter_dispatch:{error}"
                )));
            }
        };
        let observer = LocalFixtureEffectObserver;
        let observation_binding = ProviderReceiptBinding::new(
            adapter.binding.definition.connector_id.clone(),
            adapter.binding.binding.binding_id.clone(),
            adapter.binding.binding.account_id.clone(),
            adapter.operation.clone(),
            InvocationId::from_uuid(request.request.request_id.as_uuid()),
            attempt,
            kiana_domain::json_digest(&json!({"idempotency_key": &adapter.idempotency_key})),
            Some(connector_payload_sha256(&adapter.payload)),
            Some(kiana_domain::json_digest(&json!({
                "project_root": project,
                "actor_id": actor,
            }))),
            Some(kiana_domain::json_digest(&json!({
                "connector_id": adapter.binding.definition.connector_id,
                "binding_id": adapter.binding.binding.binding_id,
                "account_id": adapter.binding.binding.account_id,
            }))),
        )
        .map_err(failed)?;
        let observation = match observer.observe(
            &receipt,
            ExecutionId::from_uuid(request.request.request_id.as_uuid()),
            &observation_binding,
            now,
        ) {
            Ok(observation) => observation,
            Err(error) => {
                let unknown = dispatching
                    .advance(
                        ConnectorDispatchStage::Unknown,
                        None,
                        None,
                        None,
                        Some("connector_effect_observation_unknown".to_owned()),
                    )
                    .map_err(failed)?;
                self.append_lifecycle(request, project, &unknown, starting_version + 2)
                    .await?;
                return Err(failed(format!(
                    "result_unknown:connector_effect_observation:{error}"
                )));
            }
        };
        let receipt_digest = kiana_domain::json_digest(
            &serde_json::to_value(&receipt)
                .map_err(|_| failed("connector_receipt_encode_failed"))?,
        );
        let observation_digest = observation.observation_digest.clone();
        let observed = dispatching
            .advance(
                ConnectorDispatchStage::Observed,
                Some(receipt_digest.clone()),
                Some(observation_digest.clone()),
                None,
                None,
            )
            .map_err(failed)?;
        self.append_lifecycle(request, project, &observed, starting_version + 2)
            .await?;
        let result_digest = kiana_domain::json_digest(&json!({
            "receipt": receipt,
            "observation": observation,
            "credential_evidence": credential_evidence,
        }));
        let lifecycle = if receipt.outcome == ProviderOutcome::Unknown {
            observed
                .advance(
                    ConnectorDispatchStage::Unknown,
                    Some(receipt_digest),
                    Some(observation_digest),
                    None,
                    Some("connector_provider_outcome_unknown".to_owned()),
                )
                .map_err(failed)?
        } else {
            observed
                .advance(
                    ConnectorDispatchStage::ResultCommitted,
                    Some(receipt_digest),
                    Some(observation_digest),
                    Some(result_digest),
                    None,
                )
                .map_err(failed)?
        };
        self.append_lifecycle(request, project, &lifecycle, starting_version + 3)
            .await?;
        Ok(ConnectorDispatchOutcome {
            receipt,
            observation,
            lifecycle,
            version: starting_version + 4,
            credential_evidence,
        })
    }

    async fn append_lifecycle(
        &self,
        request: &AuthorizedCapabilityRequest,
        project: &str,
        lifecycle: &ConnectorDispatchLifecycle,
        expected_version: u64,
    ) -> Result<(), PortError> {
        self.journal
            .append_lifecycle(
                request.request.request_id,
                project,
                lifecycle,
                expected_version,
            )
            .await
            .map(|_| ())
            .map_err(|error| match error {
                PortError::Conflict(_) => error,
                other => failed(format!(
                    "result_unknown:connector_dispatch_lifecycle_persist:{other}"
                )),
            })
    }

    async fn handle(&self, request: &AuthorizedCapabilityRequest) -> Result<Value, PortError> {
        let args = &request.request.arguments;
        if request.request.cell_id.is_some() || args["operator_authorized"] != true {
            return Err(failed("connector_operator_required"));
        }
        let project = string(args, "project_root")?;
        let actor = string(args, "actor_id")?;
        let history = self.journal.read_stream(CONNECTOR_STREAM, project).await?;
        let (version, bindings) = connector_bindings(&history).map_err(failed)?;
        let action = if request.request.operation == CONNECTOR_INVOKE_OPERATION {
            "invoke"
        } else if request.request.operation == CONNECTOR_HEALTH_OPERATION {
            "health"
        } else if request.request.operation == CONNECTOR_MCP_HANDSHAKE_OPERATION {
            "mcp_handshake"
        } else {
            string(args, "action")?
        };
        if action == "list" {
            let unresolved = history
                .iter()
                .filter(|event| {
                    event.kind == "connector.invoked"
                        && event.data["receipt"]["outcome"] == "unknown"
                        && !history.iter().any(|other| {
                            other.kind == "connector.reconciled"
                                && other.data["invocation_event_id"] == event.event_id.to_string()
                        })
                })
                .map(unresolved_invocation_entry)
                .collect::<Vec<_>>();
            let health = kiana_query::project_connector_health(&history).map_err(failed)?;
            return Ok(json!({
                "schema":"kiana.connector-registry.v1",
                "registry_version":version,
                "bindings":bindings,
                "unresolved":unresolved,
                "health":health,
            }));
        }
        if action == "health" {
            return self
                .handle_health(&request, args, project, actor, &history, &bindings)
                .await;
        }
        if action == "mcp_handshake" {
            return self
                .handle_mcp_handshake(&request, args, project, actor, &history, &bindings)
                .await;
        }
        if !matches!(action, "bind" | "revoke" | "reconcile" | "invoke") {
            return Err(failed("connector_action_invalid"));
        }
        let idempotency = string(args, "idempotency_key")?;
        if idempotency.len() > 128 || idempotency.chars().any(char::is_control) {
            return Err(failed("connector_idempotency_key_invalid"));
        }
        let request_fingerprint = sha256(
            &serde_json::to_vec(&kiana_domain::canonical_json(args.clone()))
                .map_err(|e| failed(e.to_string()))?,
        );
        // Server-owned canonical command identity used by INT-16 replay/CAS. Keep the legacy
        // request fingerprint for compatibility with pre-reservation connector events.
        let command_digest = kiana_domain::json_digest(&kiana_domain::canonical_json(args.clone()));
        let key = format!("connector:{}:{idempotency}", sha256(project.as_bytes()));
        if let Some(previous) = history.iter().find(|event| {
            event.idempotency_key.as_deref() == Some(key.as_str())
                && event.data.get("output").is_some()
        }) {
            let previous_digest = previous.data["command_digest"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    previous.data["request_fingerprint"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                });
            if previous_digest != command_digest
                && previous.data["request_fingerprint"] != request_fingerprint
            {
                return Err(PortError::Conflict(
                    "connector_idempotency_command_digest_conflict".to_owned(),
                ));
            }
            let mut output = previous.data["output"].clone();
            output["event_id"] = json!(previous.event_id);
            output["replayed"] = json!(true);
            // Unknown calls are replayed as unknown; reconciliation is a separate audited
            // command, never an implicit second invocation of the adapter.
            return Ok(output);
        }
        if let Some(lifecycle) = history.iter().rev().find_map(|event| {
            (event.kind == CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND
                && event.data["lifecycle"]["command_digest"] == command_digest)
                .then(|| {
                    serde_json::from_value::<ConnectorDispatchLifecycle>(
                        event.data["lifecycle"].clone(),
                    )
                })
                .transpose()
                .ok()
                .flatten()
        }) {
            return Err(failed(match lifecycle.stage {
                ConnectorDispatchStage::Prepared
                | ConnectorDispatchStage::Dispatching
                | ConnectorDispatchStage::Observed => {
                    "result_unknown:connector_dispatch_incomplete"
                }
                ConnectorDispatchStage::ResultCommitted => {
                    "result_unknown:connector_result_commit_missing"
                }
                ConnectorDispatchStage::Unknown => "result_unknown:connector_dispatch_terminal",
            }));
        }
        if action != "invoke" {
            if !matches!(
                request.request.risk,
                kiana_domain::RiskLevel::ExternalSideEffect | kiana_domain::RiskLevel::Critical
            ) {
                return Err(failed("connector_final_payload_approval_required"));
            }
            if args["expected_registry_version"].as_u64() != Some(version) {
                return Err(PortError::Conflict(
                    "connector_registry_version_mismatch".to_owned(),
                ));
            }
            if string(args, "reason")?.len() > 4096 {
                return Err(failed("connector_reason_too_large"));
            }
        }
        let next_version = version
            .checked_add(1)
            .ok_or_else(|| failed("connector_registry_version_exhausted"))?;
        let (kind, mut data, mut output, event_version, expected_version) = match action {
            "bind" => {
                let state = ConnectorBindingSnapshot {
                    definition: serde_json::from_value(args["definition"].clone())
                        .map_err(|_| failed("connector_definition_invalid"))?,
                    binding: serde_json::from_value(args["binding"].clone())
                        .map_err(|_| failed("connector_binding_invalid"))?,
                    project_root: project.to_owned(),
                    revision: next_version,
                    status: "active".to_owned(),
                };
                state.validate().map_err(failed)?;
                if let Some(previous) = bindings.get(&state.binding.binding_id) {
                    if previous.binding.account_id != state.binding.account_id
                        || previous.definition.connector_id != state.definition.connector_id
                        || previous.definition.provider_id != state.definition.provider_id
                    {
                        return Err(failed("connector_account_rebinding_denied"));
                    }
                }
                let fixture = load_fixture(&state).await?;
                for operation in state.definition.operations.keys() {
                    if !fixture.operations.contains_key(operation) {
                        return Err(failed("connector_fixture_operation_missing"));
                    }
                }
                let output = json!({"schema":"kiana.connector-binding-result.v1","binding_id":state.binding.binding_id,
                    "account_id":state.binding.account_id,"registry_version":next_version,"state":"active","fixture_sha256":state.binding.fixture_sha256,
                    "health":"local_fixture_parsed","external_transport":"not_supported","replayed":false});
                (
                    "connector.binding",
                    json!({"state":state}),
                    output,
                    next_version,
                    version,
                )
            }
            "revoke" => {
                let mut state = bindings
                    .get(string(args, "binding_id")?)
                    .cloned()
                    .ok_or_else(|| failed("connector_binding_missing"))?;
                if state.status != "active" {
                    return Err(failed("connector_binding_revoked"));
                }
                state.status = "revoked".to_owned();
                state.revision = next_version;
                let output = json!({"schema":"kiana.connector-binding-result.v1","binding_id":state.binding.binding_id,
                    "registry_version":next_version,"state":"revoked","external_transport":"not_supported","replayed":false});
                (
                    "connector.binding",
                    json!({"state":state}),
                    output,
                    next_version,
                    version,
                )
            }
            "invoke" => {
                let snapshot: ConnectorBindingSnapshot =
                    serde_json::from_value(args["binding_snapshot"].clone())
                        .map_err(|_| failed("connector_binding_snapshot_required"))?;
                if args["binding_authorized"] != true
                    || snapshot.project_root != project
                    || args["binding_id"] != snapshot.binding.binding_id
                {
                    return Err(failed("connector_binding_scope_mismatch"));
                }
                let current = bindings
                    .get(&snapshot.binding.binding_id)
                    .ok_or_else(|| failed("connector_binding_missing"))?;
                if current != &snapshot {
                    return Err(failed("connector_binding_snapshot_changed"));
                }
                let operation = string(args, "operation")?;
                let contract = snapshot.operation(operation).map_err(failed)?;
                if contract.risk() == kiana_domain::RiskLevel::ExternalSideEffect
                    && !matches!(
                        request.request.risk,
                        kiana_domain::RiskLevel::ExternalSideEffect
                            | kiana_domain::RiskLevel::Critical
                    )
                {
                    return Err(failed("connector_final_payload_approval_required"));
                }
                let now = now_ms()?;
                let timestamps = history
                    .iter()
                    .filter(|event| {
                        event.kind == "connector.invoked"
                            && event.data["receipt"]["binding_id"] == snapshot.binding.binding_id
                    })
                    .filter_map(|event| event.data["occurred_at_ms"].as_u64())
                    .collect::<Vec<_>>();
                if timestamps.iter().any(|at| *at > now) {
                    return Err(failed("connector_clock_regressed"));
                }
                if timestamps
                    .iter()
                    .filter(|at| now.saturating_sub(**at) < 60_000)
                    .count()
                    >= snapshot.definition.rate_limit_per_minute as usize
                {
                    return Err(failed("connector_rate_limit_exceeded"));
                }
                let payload = args
                    .get("payload")
                    .ok_or_else(|| failed("connector_final_payload_required"))?;
                if let (Some(reservation), Some(permit)) = (
                    args.get("connector_reservation"),
                    args.get("connector_permit"),
                ) {
                    let reservation: kiana_domain::ConnectorInvocationReservation =
                        serde_json::from_value(reservation.clone())
                            .map_err(|_| failed("connector_reservation_invalid"))?;
                    let permit: kiana_domain::ConnectorInvocationPermit =
                        serde_json::from_value(permit.clone())
                            .map_err(|_| failed("connector_permit_invalid"))?;
                    kiana_domain::connector_effect_admission(&reservation, &permit, now)
                        .map_err(failed)?;
                } else if args.get("connector_reservation").is_some()
                    || args.get("connector_permit").is_some()
                {
                    return Err(failed("connector_permit_required"));
                }
                if args.get("connector_quota_reservation").is_some()
                    || args.get("connector_quota_claim").is_some()
                    || args.get("connector_quota_policy").is_some()
                {
                    validate_connector_quota_boundary(request, now).map_err(
                        |error| match error {
                            PortError::Conflict(reason) | PortError::Failed(reason) => {
                                failed(reason)
                            }
                            other => other,
                        },
                    )?;
                }
                // INT-18 effect-time fence: the adapter sees no fixture bytes until the
                // server-owned permit is checked against a fresh binding/epoch snapshot.
                if args.get("connector_reservation").is_some() {
                    let reservation: kiana_domain::ConnectorInvocationReservation =
                        serde_json::from_value(args["connector_reservation"].clone())
                            .map_err(|_| failed("connector_reservation_invalid"))?;
                    let effect_permit: kiana_domain::ConnectorEffectPermit =
                        serde_json::from_value(
                            args.get("connector_effect_permit")
                                .cloned()
                                .ok_or_else(|| failed("connector_effect_permit_required"))?,
                        )
                        .map_err(|_| failed("connector_effect_permit_invalid"))?;
                    let scope = request
                        .request
                        .execution_scope
                        .as_ref()
                        .ok_or_else(|| failed("connector_effect_scope_required"))?;
                    let configuration_epoch = args
                        .get("connector_configuration_epoch")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| failed("connector_configuration_epoch_required"))?;
                    let policy_epoch = args
                        .get("connector_policy_epoch")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| failed("connector_policy_epoch_required"))?;
                    let credential_epoch = args
                        .get("connector_credential_epoch")
                        .and_then(Value::as_u64)
                        .unwrap_or_else(|| {
                            snapshot
                                .binding
                                .credential_ref
                                .as_ref()
                                .map_or(0, |reference| reference.generation)
                        });
                    let current_scope_digest = kiana_domain::connector_effect_scope_digest(
                        &snapshot,
                        &effect_permit.operation,
                    )
                    .map_err(failed)?;
                    let current = kiana_domain::ConnectorEffectFence::new(
                        current_scope_digest,
                        scope.authority_epoch,
                        configuration_epoch,
                        policy_epoch,
                        credential_epoch,
                        scope.data_epoch,
                    )
                    .map_err(failed)?;
                    effect_permit
                        .validate_for_effect(&reservation, &snapshot, &current, now)
                        .map_err(failed)?;
                }
                let payload_bytes =
                    serde_json::to_vec(&kiana_domain::canonical_json(payload.clone()))
                        .map_err(|e| failed(e.to_string()))?;
                if payload_bytes.len() > 64 * 1024 {
                    return Err(failed("connector_payload_too_large"));
                }
                let attempt = args
                    .get("connector_reservation")
                    .and_then(|value| {
                        serde_json::from_value::<kiana_domain::ConnectorInvocationReservation>(
                            value.clone(),
                        )
                        .ok()
                    })
                    .map_or(1, |reservation| reservation.command.attempt);
                let adapter = LocalFixtureAdapter {
                    binding: snapshot.clone(),
                    operation: operation.to_owned(),
                    idempotency_key: idempotency.to_owned(),
                    payload: payload.clone(),
                    attempt,
                };
                // Payload and fixture validation happen through the adapter boundary before a
                // one-shot credential lease is consumed. The handler never opens fixture bytes.
                adapter.validate_payload().await?;
                let credential_evidence = if let Some(secret_ref) = &snapshot.binding.credential_ref
                {
                    let mut credential = ConnectorCredentialInvocation::issue(
                        &snapshot,
                        operation,
                        request.request.request_id,
                        &idempotency,
                        secret_ref,
                        now,
                        CONNECTOR_CREDENTIAL_MAX_TTL_MS,
                    )
                    .map_err(failed)?;
                    // INT-06: the server-owned fence revalidates the current binding and admits
                    // each issued lease exactly once, so a cloned or retried envelope cannot
                    // consume the same lease twice. The lock is released before the adapter call
                    // below; the fence keeps digests only.
                    let mut fence = self
                        .credential_fence
                        .lock()
                        .map_err(|_| failed("connector_credential_fence_poisoned"))?;
                    Some(fence.consume(
                        &mut credential,
                        &snapshot,
                        operation,
                        request.request.request_id,
                        &idempotency,
                        now,
                    )?)
                } else {
                    None
                };
                let dispatch = self
                    .dispatch_local_fixture(
                        request,
                        project,
                        actor,
                        &adapter,
                        credential_evidence,
                        &command_digest,
                        version,
                        now,
                    )
                    .await?;
                let mut output = receipt_output(&dispatch.receipt, dispatch.version);
                if dispatch.receipt.outcome == ProviderOutcome::Unknown {
                    // INT-22: an Unknown provider outcome is quarantined, not failed and not
                    // retried.  The explicit reconcile command is the only way to close it.
                    attach_quarantine_projection(
                        &mut output,
                        &dispatch.receipt,
                        &dispatch.observation,
                    )?;
                }
                if let Some(evidence) = &dispatch.credential_evidence {
                    output["credential_evidence"] = serde_json::to_value(evidence)
                        .map_err(|_| failed("connector_credential_evidence_encode_failed"))?;
                }
                output["effect_observation"] = serde_json::to_value(&dispatch.observation)
                    .map_err(|_| failed("effect_observation_encode_failed"))?;
                output["dispatch_lifecycle"] = serde_json::to_value(&dispatch.lifecycle)
                    .map_err(|_| failed("connector_dispatch_lifecycle_encode_failed"))?;
                (
                    "connector.invoked",
                    json!({"receipt":dispatch.receipt,"credential_evidence":dispatch.credential_evidence,"effect_observation":dispatch.observation,"dispatch_lifecycle":dispatch.lifecycle,"binding_revision":snapshot.revision,"occurred_at_ms":now}),
                    output,
                    dispatch.version + 1,
                    dispatch.version,
                )
            }
            "reconcile" => {
                let invocation = string(args, "invocation_event_id")?;
                let original = history
                    .iter()
                    .find(|event| {
                        event.kind == "connector.invoked"
                            && event.event_id.to_string() == invocation
                    })
                    .ok_or_else(|| failed("connector_invocation_missing"))?;
                let prior: ProviderReceipt =
                    serde_json::from_value(original.data["receipt"].clone())
                        .map_err(|_| failed("connector_receipt_invalid"))?;
                if prior.outcome != ProviderOutcome::Unknown
                    || history.iter().any(|event| {
                        event.kind == "connector.reconciled"
                            && event.data["invocation_event_id"] == invocation
                    })
                {
                    return Err(failed("connector_reconciliation_not_pending"));
                }
                let prior_observation =
                    EffectObservation::from_json(&original.data["effect_observation"])
                        .map_err(failed)?;
                let pending_case = ConnectorReconciliationCase::from_unknown(
                    &prior,
                    &prior_observation,
                    ConnectorReconciliationReason::ProviderUnknown,
                )
                .map_err(failed)?;
                let bytes = read_project_file(
                    project,
                    string(args, "receipt_path")?,
                    string(args, "receipt_sha256")?,
                )
                .await?;
                let receipt: ProviderReceipt = serde_json::from_slice(&bytes)
                    .map_err(|_| failed("connector_receipt_invalid"))?;
                receipt.validate().map_err(failed)?;
                if receipt.schema != "kiana.provider-receipt.v1"
                    || receipt.source != "local_fixture"
                    || receipt.outcome == ProviderOutcome::Unknown
                    || receipt.connector_id != prior.connector_id
                    || receipt.binding_id != prior.binding_id
                    || receipt.account_id != prior.account_id
                    || receipt.operation != prior.operation
                    || receipt.idempotency_key != prior.idempotency_key
                    || receipt.final_payload_sha256 != prior.final_payload_sha256
                    || !kiana_domain::valid_extension_identifier(&receipt.provider_receipt_id)
                {
                    return Err(failed("connector_reconciliation_binding_mismatch"));
                }
                let receipt = ProviderReceipt {
                    result: kiana_domain::redact_value(&receipt.result),
                    ..receipt
                };
                let now = now_ms()?;
                let observation_binding = ProviderReceiptBinding::new(
                    pending_case.connector_id.clone(),
                    pending_case.binding_id.clone(),
                    pending_case.account_id.clone(),
                    pending_case.operation.clone(),
                    pending_case.invocation_id,
                    pending_case.attempt,
                    pending_case.idempotency_key_digest.clone(),
                    Some(pending_case.payload_sha256.clone()),
                    Some(pending_case.owner_digest.clone()),
                    Some(pending_case.audience_digest.clone()),
                )
                .map_err(failed)?;
                let observation = LocalFixtureEffectObserver.observe(
                    &receipt,
                    ExecutionId::from_uuid(request.request.request_id.as_uuid()),
                    &observation_binding,
                    now,
                )?;
                let evidence_ref = kiana_domain::json_digest(&json!({
                    "receipt_sha256": args["receipt_sha256"],
                }));
                let reconciliation_case = pending_case
                    .attach_evidence(
                        ConnectorReconciliationSource::ManualEvidence,
                        receipt.clone(),
                        observation.clone(),
                        vec![evidence_ref],
                    )
                    .map_err(failed)?
                    .commit_reconciled()
                    .map_err(failed)?;
                // INT-22: the reconciliation appends a successor fact.  The successor pins the
                // original invocation/receipt digests and states that the original Unknown fact
                // is never rewritten, and the Human Inbox item publishes the safe/forbidden
                // actions for the closed case.
                let reconciliation_successor = reconciliation_case.successor().map_err(failed)?;
                let human_inbox_item = reconciliation_case.human_inbox_item().map_err(failed)?;
                let mut output = receipt_output(&receipt, next_version);
                output["effect_observation"] = serde_json::to_value(&observation)
                    .map_err(|_| failed("effect_observation_encode_failed"))?;
                output["reconciliation_case"] = serde_json::to_value(&reconciliation_case)
                    .map_err(|_| failed("connector_reconciliation_case_encode_failed"))?;
                output["reconciliation_successor"] =
                    serde_json::to_value(&reconciliation_successor)
                        .map_err(|_| failed("connector_reconciliation_successor_encode_failed"))?;
                output["human_inbox_item"] = serde_json::to_value(&human_inbox_item)
                    .map_err(|_| failed("connector_human_inbox_item_encode_failed"))?;
                output["reconciled"] = json!(true);
                output["invocation_event_id"] = json!(invocation);
                (
                    "connector.reconciled",
                    json!({"receipt":receipt,"effect_observation":observation,"reconciliation_case":reconciliation_case,"reconciliation_successor":reconciliation_successor,"invocation_event_id":invocation,"receipt_sha256":args["receipt_sha256"]}),
                    output,
                    next_version,
                    version,
                )
            }
            _ => unreachable!(),
        };
        data["schema"] = json!("kiana.connector-event.v1");
        data["project_root"] = json!(project);
        data["actor_id"] = json!(actor);
        data["authorization_id"] = json!(request.authorization_id);
        data["reason"] = json!(args["reason"].as_str().map(kiana_domain::redact_text));
        data["request_fingerprint"] = json!(request_fingerprint);
        data["command_digest"] = json!(command_digest);
        data["output"] = output;
        let event = RuntimeEvent::new(request.request.request_id, 1, kind, data)
            .map_err(|e| failed(e.to_string()))?
            .with_stream_metadata(CONNECTOR_STREAM, project, event_version)
            .with_idempotency_key(key);
        let appended = self
            .journal
            .append_idempotent_expected(event, Some(expected_version))
            .await
            .map_err(|e| match e {
                PortError::Conflict(_) => e,
                _ => failed(format!(
                    "result_unknown:connector_event_persistence_failed:{e}"
                )),
            })?;
        let mut output = appended.event.data["output"].clone();
        output["event_id"] = json!(appended.event.event_id);
        Ok(output)
    }
}

#[async_trait]
impl CapabilityHandler for ConnectorRegistry {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.capability != CapabilityKind::Tool
            || !matches!(
                request.request.operation.as_str(),
                CONNECTOR_INVOKE_OPERATION
                    | CONNECTOR_MANAGE_OPERATION
                    | CONNECTOR_HEALTH_OPERATION
                    | CONNECTOR_MCP_HANDSHAKE_OPERATION
            )
        {
            return Err(failed("connector_operation_mismatch"));
        }
        let output = self.handle(&request).await?;
        let success = output.get("error").is_none();
        let evidence_refs = output["event_id"]
            .as_str()
            .map(|id| vec![format!("event:{id}")])
            .unwrap_or_default();
        Ok(CapabilityResult {
            request_id: request.request.request_id,
            success,
            output,
            evidence_refs,
        })
    }
}

/// Local fixture adapter: it only produces a deterministic provider receipt and has no journal
/// or authorization authority. The dispatch coordinator enters it only after the dispatching
/// lifecycle fact is committed.
struct LocalFixtureAdapter {
    binding: ConnectorBindingSnapshot,
    operation: String,
    idempotency_key: String,
    payload: Value,
    attempt: u32,
}

impl LocalFixtureAdapter {
    async fn validate_payload(&self) -> Result<(), PortError> {
        let fixture = load_fixture(&self.binding).await?;
        fixture
            .find_case(&self.operation, &self.payload)
            .map(|_| ())
            .map_err(failed)
    }

    async fn dispatch(&self) -> Result<ProviderReceipt, PortError> {
        let fixture = load_fixture(&self.binding).await?;
        fixture
            .provider_receipt(
                &self.binding,
                &self.operation,
                &self.idempotency_key,
                &self.payload,
            )
            .map_err(failed)
    }
}

/// Observation is a distinct phase and projection. It never writes the journal and cannot
/// authorize, retry or mutate the connector binding. It consumes a server-owned
/// `ProviderReceiptBinding` so owner/audience/invocation/attempt/key/payload are the admitted
/// expectation, not values re-derived from the receipt under observation.
struct LocalFixtureEffectObserver;

impl LocalFixtureEffectObserver {
    fn observe(
        &self,
        receipt: &ProviderReceipt,
        execution_id: ExecutionId,
        binding: &ProviderReceiptBinding,
        observed_at_unix_ms: u64,
    ) -> Result<EffectObservation, PortError> {
        EffectObservation::from_provider_receipt_for_binding(
            receipt,
            binding,
            execution_id,
            observed_at_unix_ms,
            Vec::new(),
        )
        .map_err(failed)
    }
}

fn receipt_output(receipt: &ProviderReceipt, version: u64) -> Value {
    let mut output = json!({"schema":"kiana.connector-invocation-result.v1","receipt":receipt,"registry_version":version,
        "external_effect_performed":false,"proof_source":"local_fixture","replayed":false});
    match receipt.outcome {
        ProviderOutcome::Succeeded => {}
        ProviderOutcome::Failed => output["error"] = json!("connector_provider_failed"),
        ProviderOutcome::Unknown => {
            output["error"] = json!("result_unknown:connector_provider_outcome_unknown")
        }
    }
    output
}

/// Add the INT-22 quarantine projection to an invocation output whose outcome is Unknown.  The
/// case is rebuilt from the stored receipt/observation only; the caller cannot supply it, and the
/// unsafe actions (automatic retry, key reuse, replacing the original receipt) are published as
/// forbidden so the Human Inbox shows the same contract the domain enforces.
fn attach_quarantine_projection(
    output: &mut Value,
    receipt: &ProviderReceipt,
    observation: &EffectObservation,
) -> Result<(), PortError> {
    let case = ConnectorReconciliationCase::from_unknown(
        receipt,
        observation,
        ConnectorReconciliationReason::ProviderUnknown,
    )
    .map_err(failed)?;
    output["reconciliation_state"] = json!(case.state);
    output["reconciliation_required"] = json!(case.reconciliation_required);
    output["automatic_retry_allowed"] = json!(case.automatic_retry_allowed);
    output["human_inbox_item"] = serde_json::to_value(case.human_inbox_item().map_err(failed)?)
        .map_err(|_| failed("connector_human_inbox_item_encode_failed"))?;
    Ok(())
}

/// Projection for one quarantined Unknown invocation.  Reconciliation facts are appended, never
/// overwritten, so an invocation with a committed `connector.reconciled` successor leaves the
/// unresolved list; an older fact without a stored observation keeps the plain receipt entry
/// instead of inventing an expectation.
fn unresolved_invocation_entry(event: &RuntimeEvent) -> Value {
    let mut entry = json!({"event_id":event.event_id,"receipt":event.data["receipt"]});
    let Ok(receipt) = serde_json::from_value::<ProviderReceipt>(event.data["receipt"].clone())
    else {
        return entry;
    };
    let Ok(observation) = EffectObservation::from_json(&event.data["effect_observation"]) else {
        return entry;
    };
    let Ok(case) = ConnectorReconciliationCase::from_unknown(
        &receipt,
        &observation,
        ConnectorReconciliationReason::ProviderUnknown,
    ) else {
        return entry;
    };
    entry["reconciliation_required"] = json!(case.reconciliation_required);
    entry["automatic_retry_allowed"] = json!(case.automatic_retry_allowed);
    entry["reconciliation_state"] = json!(case.state);
    entry["reconciliation_case_digest"] = json!(case.case_digest);
    if let Ok(case_value) = serde_json::to_value(&case) {
        entry["reconciliation_case"] = case_value;
    }
    if let Ok(Ok(item)) = case.human_inbox_item().map(serde_json::to_value) {
        entry["human_inbox_item"] = item;
    }
    entry
}

async fn load_fixture(binding: &ConnectorBindingSnapshot) -> Result<ConnectorFixture, PortError> {
    let bytes = read_project_file(
        &binding.project_root,
        &binding.binding.fixture_path,
        &binding.binding.fixture_sha256,
    )
    .await?;
    let fixture = ConnectorFixture::from_bytes(&bytes).map_err(failed)?;
    fixture.validate_for_binding(binding).map_err(failed)?;
    Ok(fixture)
}

async fn read_project_file(
    project: &str,
    path: &str,
    expected_sha256: &str,
) -> Result<Vec<u8>, PortError> {
    if !kiana_domain::valid_extension_path(path) || !connector_fixture_hash_valid(expected_sha256) {
        return Err(failed("connector_evidence_path_invalid"));
    }
    let project = Path::new(project)
        .canonicalize()
        .map_err(|e| failed(format!("connector_project_invalid:{e}")))?;
    let path = path.to_owned();
    let expected_sha256 = expected_sha256.to_owned();
    tokio::task::spawn_blocking(move || {
        let bytes = LocalDir::open(&project, false)?.read(&path, CONNECTOR_FIXTURE_MAX_BYTES)?;
        if !connector_fixture_hash_matches(&expected_sha256, &bytes) {
            return Err(failed("connector_fixture_hash_mismatch"));
        }
        Ok(bytes)
    })
    .await
    .map_err(|e| failed(format!("connector_fixture_join_failed:{e}")))?
}

/// Rebuild the read-only health response for an already committed request. It never appends a
/// fact and never re-reads the fixture, so a retry cannot turn a previous observation into a
/// second effect or a fresh success.
fn health_replay_result(
    previous: &RuntimeEvent,
    projection_events: &[RuntimeEvent],
    binding_id: &str,
) -> Result<Value, PortError> {
    let health = previous.data["health"].clone();
    let health_projection =
        kiana_query::project_connector_health(projection_events).map_err(failed)?;
    let stale = health_projection
        .entries
        .iter()
        .find(|entry| entry.health.binding_id == binding_id)
        .is_none_or(|entry| entry.stale);
    Ok(json!({
        "schema":"kiana.connector-health-result.v1",
        "health":health,
        "health_projection":health_projection,
        "source_cursor":previous.stream_version.unwrap_or_default(),
        "projection_version":"connector-health.v1",
        "stale":stale,
        "proof_level":"source",
        "event_id":previous.event_id,
        "replayed":true,
    }))
}

fn now_ms() -> Result<u64, PortError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| failed("connector_clock_invalid"))?
        .as_millis();
    u64::try_from(now).map_err(|_| failed("connector_clock_invalid"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, PortError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| failed(format!("connector_argument_required:{key}")))
}
