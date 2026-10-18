//! INT-19 daemon behaviour fixture: Broker dispatch and adapter observation are separate facts.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding.  The journal commits `prepared` and `dispatching`
//! before the adapter is entered, then commits `observed` and the terminal lifecycle before the
//! `connector.invoked` fact, so the dispatch chain is replayable and a committed attempt is never
//! re-dispatched.  A seeded incomplete/terminal chain without a fact (the crash window between the
//! lifecycle commit and the connector fact) is reported as `result_unknown` and the adapter is not
//! entered again; wire-supplied lifecycle/observation material is refused before any effect.
//! No network transport, second execution loop or model loop is involved.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, json_digest, AccountBinding, ConnectorDefinition,
    ConnectorDispatchLifecycle, ConnectorDispatchStage, ConnectorEffect, ConnectorOperation,
    EffectObservation, InvocationId, ProviderReceipt, RequestId, RuntimeEvent,
    CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND, CONNECTOR_STREAM,
};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::EventStorePort;
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ExecutionStatus, RequestEnvelope, RequestMetadata,
    ResponseEnvelope, RiskLevel,
};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const FIXTURE_PATH: &str = "fixtures/connector-dispatch-split.json";
const BINDING_ID: &str = "binding-dispatch-split";
const CONNECTOR_ID: &str = "connector-dispatch-split";
const ACCOUNT_ID: &str = "account-dispatch-split";
const OPERATION: &str = "lookup";

/// INT-19 server-owned dispatch/observation material that must never be accepted from, or
/// persisted on behalf of, the wire.
const WIRE_FORGED_DISPATCH_KEYS: &[&str] = &[
    "dispatch_lifecycle",
    "connector_dispatch_lifecycle",
    "connector_dispatch_stage",
    "effect_observation",
    "connector_observation",
    "receipt",
];

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn project_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-int19-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-dispatch-split".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            OPERATION.to_owned(),
            ConnectorOperation {
                effect: ConnectorEffect::ReadOnly,
                required_scope: "read".to_owned(),
                data_classes: BTreeSet::new(),
            },
        )]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    }
}

fn account_binding(fixture_sha256: &str) -> AccountBinding {
    AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: BINDING_ID.to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        account_id: ACCOUNT_ID.to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::new(),
        fixture_path: FIXTURE_PATH.to_owned(),
        fixture_sha256: fixture_sha256.to_owned(),
        credential_ref: None,
    }
}

fn fixture_bytes() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "kiana.connector-fixture.v1",
        "connector_id": CONNECTOR_ID,
        "account_id": ACCOUNT_ID,
        "operations": {
            "lookup": [
                {
                    "payload": {"id": "one"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-one",
                    "result": {"value": "one"},
                },
            ],
        },
    }))
    .unwrap()
}

fn write_fixture(root: &Path, bytes: &[u8]) {
    let path = root.join(FIXTURE_PATH);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn trusted_metadata(root: &Path, session: &str) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

fn bind_arguments(fixture_sha256: &str, expected_registry_version: u64, key: &str) -> Value {
    json!({
        "action": "bind",
        "definition": definition(),
        "binding": account_binding(fixture_sha256),
        "expected_registry_version": expected_registry_version,
        "reason": "INT-19 dispatch/observation split behaviour",
        "idempotency_key": key,
    })
}

fn invoke_arguments(payload: Value, key: &str) -> Value {
    json!({
        "binding_id": BINDING_ID,
        "operation": OPERATION,
        "payload": payload,
        "idempotency_key": key,
    })
}

async fn command(
    host: &DaemonHost,
    metadata: RequestMetadata,
    name: &str,
    arguments: Value,
) -> ResponseEnvelope {
    host.handle(RequestEnvelope::command(metadata, name, arguments))
        .await
}

fn assert_denied(response: &ResponseEnvelope, marker: &str) {
    assert_ne!(
        response.status,
        ExecutionStatus::Completed,
        "deny path unexpectedly completed: {:?}",
        response.error
    );
    let error = response.error.as_deref().unwrap_or_default();
    assert!(
        error.contains(marker) || response.output.to_string().contains(marker),
        "expected {marker} in error {:?} / output {}",
        response.error,
        response.output
    );
}

async fn bind(host: &DaemonHost, root: &Path, session: &str) {
    let bytes = fixture_bytes();
    let digest = connector_fixture_sha256(&bytes);
    write_fixture(root, &bytes);

    let pending = command(
        host,
        trusted_metadata(root, session),
        "connector.manage",
        bind_arguments(&digest, 0, "bind-int19"),
    )
    .await;
    assert_eq!(
        pending.status,
        ExecutionStatus::AwaitingApproval,
        "bind was not staged: {:?}",
        pending.error
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(pending.output["approval"].clone()).expect("approval challenge");
    assert_eq!(challenge.risk, RiskLevel::ExternalSideEffect);
    let bound = host
        .handle(RequestEnvelope::approval_decision_with_proof(
            trusted_metadata(root, session),
            challenge.approval_id,
            ApprovalDecision::Approve,
            Some(challenge.request_hash.clone()),
            Some(challenge.nonce.clone()),
        ))
        .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "approved bind failed: {:?}",
        bound.error
    );
    assert_eq!(bound.output["state"], "active");
    assert_eq!(bound.output["registry_version"], 1);
}

async fn registry_list(host: &DaemonHost, root: &Path, session: &str) -> Value {
    let listed = command(
        host,
        trusted_metadata(root, session),
        "connector.manage",
        json!({"action": "list"}),
    )
    .await;
    assert_eq!(
        listed.status,
        ExecutionStatus::Completed,
        "registry list failed: {:?}",
        listed.error
    );
    listed.output
}

/// Every connector-stream event, including the dispatch lifecycle chain.  A denied or replayed
/// command must leave this sequence unchanged: the stream is the durable connector fact history.
async fn connector_stream_events(host: &DaemonHost) -> Vec<RuntimeEvent> {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .into_iter()
        .filter(|event| event.aggregate_type.as_deref() == Some(CONNECTOR_STREAM))
        .collect()
}

async fn connector_event_count(host: &DaemonHost, kind: &str) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

fn lifecycle_of(event: &RuntimeEvent) -> ConnectorDispatchLifecycle {
    serde_json::from_value(event.data["lifecycle"].clone()).expect("dispatch lifecycle")
}

/// The durable host: the seeded event file survives across the test and is reopened by a fresh
/// host instance, so the crash-window facts are read from the EventLog rather than memory.
fn disk_host(events_path: &Path) -> DaemonHost {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&json!([])).unwrap()));
    DaemonHost::with_harness_on_disk_and_project_authority(
        harness,
        events_path,
        Arc::new(TrustedProject),
    )
    .expect("disk daemon host")
}

fn seeded_prepared(command_digest: &str, attempt: u32) -> ConnectorDispatchLifecycle {
    ConnectorDispatchLifecycle::prepared(
        InvocationId::new(),
        attempt,
        command_digest.to_owned(),
        json_digest(&json!({"seed": "binding"})),
        json_digest(&json!({"seed": "payload"})),
        json_digest(&json!({"seed": "idempotency"})),
    )
    .expect("seeded prepared lifecycle")
}

/// Walk the pure transition function so the seeded chain is exactly what a crashed worker would
/// have committed at each phase.
fn seeded_chain(
    prepared: &ConnectorDispatchLifecycle,
    target: ConnectorDispatchStage,
) -> Vec<ConnectorDispatchLifecycle> {
    let dispatching = prepared
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("seeded dispatching");
    match target {
        ConnectorDispatchStage::Prepared => vec![prepared.clone()],
        ConnectorDispatchStage::Dispatching => vec![prepared.clone(), dispatching],
        ConnectorDispatchStage::Unknown => {
            let unknown = dispatching
                .advance(
                    ConnectorDispatchStage::Unknown,
                    None,
                    None,
                    None,
                    Some("seeded_unknown".to_owned()),
                )
                .expect("seeded unknown");
            vec![prepared.clone(), dispatching, unknown]
        }
        ConnectorDispatchStage::Observed | ConnectorDispatchStage::ResultCommitted => {
            let observed = dispatching
                .advance(
                    ConnectorDispatchStage::Observed,
                    Some(json_digest(&json!({"seed": "receipt"}))),
                    Some(json_digest(&json!({"seed": "observation"}))),
                    None,
                    None,
                )
                .expect("seeded observed");
            let mut chain = vec![prepared.clone(), dispatching, observed.clone()];
            if target == ConnectorDispatchStage::ResultCommitted {
                let committed = observed
                    .advance(
                        ConnectorDispatchStage::ResultCommitted,
                        observed.receipt_digest.clone(),
                        observed.observation_digest.clone(),
                        Some(json_digest(&json!({"seed": "result"}))),
                        None,
                    )
                    .expect("seeded committed");
                chain.push(committed);
            }
            chain
        }
    }
}

/// Seed a fresh EventLog with the committed binding fact plus the lifecycle chain a crashed worker
/// left behind.  The connector fact is intentionally absent: that is the crash window INT-19 must
/// recover from without a second effect.
async fn seed_events_log(
    events_path: &Path,
    project: &Path,
    bind_event: &RuntimeEvent,
    chain: &[ConnectorDispatchLifecycle],
) {
    fs::create_dir_all(events_path.parent().unwrap()).unwrap();
    let store = Arc::new(JsonlEventLog::open(events_path).expect("open seeded event log"));
    let port: Arc<dyn EventStorePort> = store;
    port.append(bind_event.clone())
        .await
        .expect("append seeded binding fact");
    for (index, lifecycle) in chain.iter().enumerate() {
        let event = RuntimeEvent::new(
            RequestId::new(),
            1,
            CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND,
            json!({
                "schema": "kiana.connector-dispatch-lifecycle-event.v1",
                "project_root": project.to_string_lossy(),
                "invocation_id": lifecycle.invocation_id,
                "attempt": lifecycle.attempt,
                "lifecycle": lifecycle,
                "proof_level": "source",
            }),
        )
        .expect("seeded lifecycle event")
        .with_stream_metadata(
            CONNECTOR_STREAM,
            project.to_string_lossy(),
            2 + index as u64,
        );
        port.append(event)
            .await
            .expect("append seeded lifecycle fact");
    }
    port.flush().await.expect("flush seeded facts");
    drop(port);
}

// Connector bind publishes and re-reads the fixture through `LocalDir`, which is implemented only
// for Unix directory handles; on Windows the platform reports `local_package_platform_unsupported`
// instead of weakening the bind contract, so these behaviour fixtures are Unix-only.
#[cfg(unix)]
#[tokio::test]
async fn dispatch_and_observation_facts_precede_the_connector_fact_and_replay_is_effect_free() {
    let root = project_dir("split-success");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int19-success";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;

    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "attempt-one"),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "invoke failed: {:?}",
        invoked.error
    );
    assert_eq!(invoked.output["replayed"], false);
    assert_eq!(invoked.output["receipt"]["outcome"], "succeeded");
    assert_eq!(invoked.output["external_effect_performed"], false);
    assert_eq!(
        invoked.output["dispatch_lifecycle"]["stage"],
        "result_committed"
    );

    // The dispatch chain is four separate committed facts, in phase order, all before the single
    // connector fact that carries the receipt and observation.
    let events = connector_stream_events(&host).await;
    assert_eq!(
        events.len(),
        6,
        "binding + four lifecycle facts + invoked fact"
    );
    assert_eq!(events[0].kind, "connector.binding");
    assert_eq!(events[5].kind, "connector.invoked");
    let stages: Vec<ConnectorDispatchStage> = events[1..5]
        .iter()
        .map(lifecycle_of)
        .map(|l| l.stage)
        .collect();
    assert_eq!(
        stages,
        vec![
            ConnectorDispatchStage::Prepared,
            ConnectorDispatchStage::Dispatching,
            ConnectorDispatchStage::Observed,
            ConnectorDispatchStage::ResultCommitted,
        ]
    );
    for (index, event) in events[1..5].iter().enumerate() {
        assert_eq!(event.kind, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND);
        assert_eq!(event.stream_version, Some(2 + index as u64));
        lifecycle_of(event).validate().expect("committed lifecycle");
    }
    let terminal = lifecycle_of(&events[4]);
    assert_eq!(terminal.attempt, 1);
    for event in &events[1..5] {
        assert_eq!(lifecycle_of(event).invocation_id, terminal.invocation_id);
        assert_eq!(lifecycle_of(event).command_digest, terminal.command_digest);
    }

    // The connector fact binds the same digests: the receipt digest is recomputed from the
    // persisted receipt, the observation digest from the persisted observation, and the result
    // digest covers receipt + observation + credential evidence.
    let fact = &events[5];
    assert_eq!(
        fact.data["dispatch_lifecycle"],
        serde_json::to_value(&terminal).unwrap()
    );
    let receipt: ProviderReceipt =
        serde_json::from_value(fact.data["receipt"].clone()).expect("persisted receipt");
    let receipt_digest = json_digest(&serde_json::to_value(&receipt).unwrap());
    assert_eq!(
        terminal.receipt_digest.as_deref(),
        Some(receipt_digest.as_str())
    );
    let observation =
        EffectObservation::from_json(&fact.data["effect_observation"]).expect("observation");
    assert_eq!(
        terminal.observation_digest.as_deref(),
        Some(observation.observation_digest.as_str())
    );
    assert_eq!(observation.invocation_id, terminal.invocation_id);
    assert_eq!(observation.attempt, terminal.attempt);
    observation
        .validate_for_receipt(
            &receipt,
            &observation.owner_digest,
            &observation.audience_digest,
        )
        .expect("observation binds the persisted receipt");
    let result_digest = json_digest(&json!({
        "receipt": receipt,
        "observation": observation,
        "credential_evidence": fact.data["credential_evidence"],
    }));
    assert_eq!(
        terminal.result_digest.as_deref(),
        Some(result_digest.as_str())
    );

    // A replayed command returns the original receipt/observation/lifecycle and never repeats the
    // effect: same event id, same stream length, one connector fact, four lifecycle facts.
    let replayed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "attempt-one"),
    )
    .await;
    assert_eq!(
        replayed.status,
        ExecutionStatus::Completed,
        "replay failed: {:?}",
        replayed.error
    );
    assert_eq!(replayed.output["replayed"], true);
    assert_eq!(replayed.output["event_id"], invoked.output["event_id"]);
    assert_eq!(replayed.output["receipt"], invoked.output["receipt"]);
    assert_eq!(
        replayed.output["effect_observation"],
        invoked.output["effect_observation"]
    );
    assert_eq!(
        replayed.output["dispatch_lifecycle"],
        invoked.output["dispatch_lifecycle"]
    );
    assert_eq!(connector_stream_events(&host).await.len(), 6);
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    assert_eq!(
        connector_event_count(&host, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND).await,
        4
    );

    let _ = fs::remove_dir_all(&root);
}

// Connector bind publishes and re-reads the fixture through `LocalDir`, which is implemented only
// for Unix directory handles; on Windows the platform reports `local_package_platform_unsupported`
// instead of weakening the bind contract, so these behaviour fixtures are Unix-only.
#[cfg(unix)]
#[tokio::test]
async fn wire_supplied_lifecycle_or_observation_material_is_refused_before_any_effect() {
    let root = project_dir("split-wire");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int19-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    assert_eq!(connector_stream_events(&host).await.len(), 1);

    // The invoke route only accepts binding/operation/payload/idempotency keys, so a wire-supplied
    // lifecycle stage, observation or receipt object is refused before the Broker or the adapter.
    for key in WIRE_FORGED_DISPATCH_KEYS {
        let mut arguments = invoke_arguments(json!({"id": "one"}), &format!("wire-{key}"));
        arguments[key] = json!({"schema": format!("forged.{key}.v1")});
        let denied = command(
            &host,
            trusted_metadata(&root, session),
            "connector.invoke",
            arguments,
        )
        .await;
        assert_denied(&denied, "connector_arguments_invalid");
        assert_eq!(
            connector_event_count(&host, "connector.invoked").await,
            0,
            "a wire-supplied {key} must not reach the adapter"
        );
        assert_eq!(connector_stream_events(&host).await.len(), 1);
    }

    // The single spine still works: the compatibility command reaches the fixture adapter exactly
    // once and commits its own dispatch chain.
    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "wire-legitimate"),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "compatibility invoke failed: {:?}",
        invoked.error
    );
    assert_eq!(invoked.output["replayed"], false);
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    assert_eq!(
        connector_event_count(&host, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND).await,
        4
    );

    let _ = fs::remove_dir_all(&root);
}

// Connector bind publishes and re-reads the fixture through `LocalDir`, which is implemented only
// for Unix directory handles; on Windows the platform reports `local_package_platform_unsupported`
// instead of weakening the bind contract, so these behaviour fixtures are Unix-only.
#[cfg(unix)]
#[tokio::test]
async fn an_incomplete_or_terminal_attempt_without_a_fact_is_reported_unknown_and_never_reexecutes()
{
    let root = project_dir("split-seeded");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int19-seeded";

    // Capture the server-owned command digest and the committed binding fact of one real attempt
    // so the seeded crash-window facts belong to exactly the same command.
    let reference = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("reference daemon host");
    bind(&reference, &root, session).await;
    let captured = command(
        &reference,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "seed-source"),
    )
    .await;
    assert_eq!(
        captured.status,
        ExecutionStatus::Completed,
        "reference invoke failed: {:?}",
        captured.error
    );
    let command_digest = captured.output["dispatch_lifecycle"]["command_digest"]
        .as_str()
        .expect("server-owned command digest")
        .to_owned();
    let bind_event = connector_stream_events(&reference)
        .await
        .into_iter()
        .find(|event| event.kind == "connector.binding")
        .expect("committed binding fact");

    // A crashed worker can only leave an incomplete chain or a terminal chain without the
    // connector fact.  Neither may be re-dispatched: the caller gets result_unknown, the seeded
    // stream is neither extended nor re-executed, and no second effect is recorded.
    for (label, target, expected) in [
        (
            "dispatching",
            ConnectorDispatchStage::Dispatching,
            "result_unknown:connector_dispatch_incomplete",
        ),
        (
            "result_committed",
            ConnectorDispatchStage::ResultCommitted,
            "result_unknown:connector_result_commit_missing",
        ),
        (
            "unknown",
            ConnectorDispatchStage::Unknown,
            "result_unknown:connector_dispatch_terminal",
        ),
    ] {
        let events_path = project_dir(&format!("split-seed-{label}")).join("events.jsonl");
        let prepared = seeded_prepared(&command_digest, 1);
        let chain = seeded_chain(&prepared, target);
        seed_events_log(&events_path, &root, &bind_event, &chain).await;
        let host = disk_host(&events_path);
        assert_eq!(connector_stream_events(&host).await.len(), 1 + chain.len());
        let listed = registry_list(&host, &root, session).await;
        assert_eq!(listed["registry_version"], json!(1 + chain.len() as u64));

        let denied = command(
            &host,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(json!({"id": "one"}), "seed-source"),
        )
        .await;
        assert_denied(&denied, expected);
        assert_eq!(
            connector_event_count(&host, "connector.invoked").await,
            0,
            "{label}: a recovered attempt must not reach the adapter again"
        );
        assert_eq!(
            connector_stream_events(&host).await.len(),
            1 + chain.len(),
            "{label}: the seeded crash-window stream must not be extended"
        );

        let _ = fs::remove_dir_all(events_path.parent().unwrap());
    }

    // A lifecycle chain for a different command is digest-bound and cannot block the current
    // attempt: the spine proceeds and commits its own chain next to the foreign one.
    let events_path = project_dir("split-seed-foreign").join("events.jsonl");
    let foreign = seeded_prepared(&json_digest(&json!({"seed": "foreign-command"})), 1);
    let chain = seeded_chain(&foreign, ConnectorDispatchStage::Dispatching);
    seed_events_log(&events_path, &root, &bind_event, &chain).await;
    let host = disk_host(&events_path);
    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "seed-source"),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "a foreign-command lifecycle must not block the current attempt: {:?}",
        invoked.error
    );
    assert_eq!(
        invoked.output["dispatch_lifecycle"]["command_digest"],
        json!(command_digest)
    );
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    assert_eq!(
        connector_stream_events(&host).await.len(),
        1 + chain.len() + 5
    );
    let _ = fs::remove_dir_all(events_path.parent().unwrap());

    let _ = fs::remove_dir_all(&root);
}
