//! Replayable local Connector adapter. No network request or external account mutation.

use crate::local_packages::{failed, sha256, LocalDir};
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    connector_bindings, AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult,
    ConnectorBindingSnapshot, ProviderOutcome, ProviderReceipt, RuntimeEvent,
    CONNECTOR_INVOKE_OPERATION, CONNECTOR_MANAGE_OPERATION, CONNECTOR_STREAM,
};
use kiana_ports::{EventStorePort, PortError};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FIXTURE_BYTES: usize = 1024 * 1024;

pub(crate) fn register(
    broker: &mut CapabilityBroker,
    events: Arc<dyn EventStorePort>,
) -> Result<(), PortError> {
    let registry = Arc::new(ConnectorRegistry { events });
    broker.register_static(
        CapabilityKind::Tool,
        CONNECTOR_MANAGE_OPERATION,
        registry.clone(),
    )?;
    broker.register_static(CapabilityKind::Tool, CONNECTOR_INVOKE_OPERATION, registry)
}

struct ConnectorRegistry {
    events: Arc<dyn EventStorePort>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectorFixture {
    schema: String,
    connector_id: String,
    account_id: String,
    operations: BTreeMap<String, Vec<FixtureCase>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCase {
    payload: Value,
    outcome: ProviderOutcome,
    receipt_id: String,
    result: Value,
}

impl ConnectorRegistry {
    async fn handle(&self, request: &AuthorizedCapabilityRequest) -> Result<Value, PortError> {
        let args = &request.request.arguments;
        if request.request.cell_id.is_some() || args["operator_authorized"] != true {
            return Err(failed("connector_operator_required"));
        }
        let project = string(args, "project_root")?;
        let actor = string(args, "actor_id")?;
        let history = self.events.read_stream(CONNECTOR_STREAM, project).await?;
        let (version, bindings) = connector_bindings(&history).map_err(failed)?;
        let action = if request.request.operation == CONNECTOR_INVOKE_OPERATION {
            "invoke"
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
                .map(|event| json!({"event_id":event.event_id,"receipt":event.data["receipt"]}))
                .collect::<Vec<_>>();
            return Ok(
                json!({"schema":"kiana.connector-registry.v1","registry_version":version,"bindings":bindings,"unresolved":unresolved}),
            );
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
        let key = format!("connector:{}:{idempotency}", sha256(project.as_bytes()));
        if let Some(previous) = history
            .iter()
            .find(|event| event.idempotency_key.as_deref() == Some(key.as_str()))
        {
            if previous.data["request_fingerprint"] != request_fingerprint {
                return Err(PortError::Conflict(
                    "connector_idempotency_payload_mismatch".to_owned(),
                ));
            }
            let mut output = previous.data["output"].clone();
            output["event_id"] = json!(previous.event_id);
            output["replayed"] = json!(true);
            // Unknown calls are replayed as unknown; reconciliation is a separate audited
            // command, never an implicit second invocation of the adapter.
            return Ok(output);
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
        let (kind, mut data, output) = match action {
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
                ("connector.binding", json!({"state":state}), output)
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
                ("connector.binding", json!({"state":state}), output)
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
                let payload_bytes =
                    serde_json::to_vec(&kiana_domain::canonical_json(payload.clone()))
                        .map_err(|e| failed(e.to_string()))?;
                if payload_bytes.len() > 64 * 1024 {
                    return Err(failed("connector_payload_too_large"));
                }
                let fixture = load_fixture(&snapshot).await?;
                let cases = fixture
                    .operations
                    .get(operation)
                    .ok_or_else(|| failed("connector_fixture_operation_missing"))?;
                let case = cases
                    .iter()
                    .find(|case| &case.payload == payload)
                    .ok_or_else(|| failed("connector_fixture_payload_mismatch"))?;
                let receipt = ProviderReceipt {
                    schema: "kiana.provider-receipt.v1".to_owned(),
                    connector_id: snapshot.definition.connector_id.clone(),
                    binding_id: snapshot.binding.binding_id.clone(),
                    account_id: snapshot.binding.account_id.clone(),
                    operation: operation.to_owned(),
                    idempotency_key: idempotency.to_owned(),
                    final_payload_sha256: sha256(&payload_bytes),
                    provider_receipt_id: case.receipt_id.clone(),
                    outcome: case.outcome,
                    source: "local_fixture".to_owned(),
                    result: kiana_domain::redact_value(&case.result),
                };
                let output = receipt_output(&receipt, next_version);
                (
                    "connector.invoked",
                    json!({"receipt":receipt,"binding_revision":snapshot.revision,"occurred_at_ms":now}),
                    output,
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
                let bytes = read_project_file(
                    project,
                    string(args, "receipt_path")?,
                    string(args, "receipt_sha256")?,
                )
                .await?;
                let receipt: ProviderReceipt = serde_json::from_slice(&bytes)
                    .map_err(|_| failed("connector_receipt_invalid"))?;
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
                let mut output = receipt_output(&receipt, next_version);
                output["reconciled"] = json!(true);
                output["invocation_event_id"] = json!(invocation);
                (
                    "connector.reconciled",
                    json!({"receipt":receipt,"invocation_event_id":invocation,"receipt_sha256":args["receipt_sha256"]}),
                    output,
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
        data["output"] = output;
        let event = RuntimeEvent::new(request.request.request_id, 1, kind, data)
            .map_err(|e| failed(e.to_string()))?
            .with_stream_metadata(CONNECTOR_STREAM, project, next_version)
            .with_idempotency_key(key);
        let appended = self
            .events
            .append_idempotent_expected(event, Some(version))
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
                CONNECTOR_INVOKE_OPERATION | CONNECTOR_MANAGE_OPERATION
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

async fn load_fixture(binding: &ConnectorBindingSnapshot) -> Result<ConnectorFixture, PortError> {
    let bytes = read_project_file(
        &binding.project_root,
        &binding.binding.fixture_path,
        &binding.binding.fixture_sha256,
    )
    .await?;
    let fixture: ConnectorFixture =
        serde_json::from_slice(&bytes).map_err(|_| failed("connector_fixture_invalid"))?;
    if fixture.schema != "kiana.connector-fixture.v1"
        || fixture.connector_id != binding.definition.connector_id
        || fixture.account_id != binding.binding.account_id
        || fixture.operations.len() > 32
    {
        return Err(failed("connector_fixture_identity_mismatch"));
    }
    for cases in fixture.operations.values() {
        if cases.is_empty()
            || cases.len() > 128
            || cases
                .iter()
                .any(|case| !kiana_domain::valid_extension_identifier(&case.receipt_id))
        {
            return Err(failed("connector_fixture_invalid"));
        }
        for (index, case) in cases.iter().enumerate() {
            if cases[..index]
                .iter()
                .any(|prior| prior.payload == case.payload)
            {
                return Err(failed("connector_fixture_payload_duplicate"));
            }
        }
    }
    Ok(fixture)
}

async fn read_project_file(
    project: &str,
    path: &str,
    expected_sha256: &str,
) -> Result<Vec<u8>, PortError> {
    if !kiana_domain::valid_extension_path(path) || !kiana_domain::is_sha256_hex(expected_sha256) {
        return Err(failed("connector_evidence_path_invalid"));
    }
    let project = Path::new(project)
        .canonicalize()
        .map_err(|e| failed(format!("connector_project_invalid:{e}")))?;
    let path = path.to_owned();
    let expected_sha256 = expected_sha256.to_owned();
    tokio::task::spawn_blocking(move || {
        let bytes = LocalDir::open(&project, false)?.read(&path, MAX_FIXTURE_BYTES)?;
        if sha256(&bytes) != expected_sha256 {
            return Err(failed("connector_fixture_hash_mismatch"));
        }
        Ok(bytes)
    })
    .await
    .map_err(|e| failed(format!("connector_fixture_join_failed:{e}")))?
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
