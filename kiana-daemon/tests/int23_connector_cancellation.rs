//! INT-23 daemon boundary fixtures: connector cancellation is server-owned stop evidence, never a
//! wire command, and every terminal outcome is reconstructed from durable connector facts.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding and no network transport.  The deny path runs first:
//! every wire-supplied stop report, settlement, lease settlement or late-result key is refused
//! before the Broker, the adapter and the EventStore, and there is no cancel action at all.  The
//! success path reads the committed dispatch lifecycle back from the EventStore and reconstructs
//! `not_executed` / `stop_confirmed` / `unknown` from it, proving that a late provider result stays
//! fenced evidence and that the settlement never appends or rewrites a connector fact.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, connector_not_executed_stop_report, json_digest, AccountBinding,
    ConnectorCancellationRecord, ConnectorCancellationState, ConnectorDefinition,
    ConnectorDispatchLifecycle, ConnectorDispatchStage, ConnectorEffect, ConnectorLateResult,
    ConnectorLateResultDisposition, ConnectorLeaseSettlement, ConnectorOperation, InvocationId,
    ProcessGroupState, RequestId, RuntimeEvent, StopMethod, StopReport,
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

const FIXTURE_PATH: &str = "fixtures/connector-cancellation.json";
const BINDING_ID: &str = "binding-cancellation";
const CONNECTOR_ID: &str = "connector-cancellation";
const ACCOUNT_ID: &str = "account-cancellation";
const OPERATION: &str = "lookup";

/// INT-23 server-owned cancellation material that must never be accepted from, or persisted on
/// behalf of, the wire.  The stop report, the settlement, the lease settlement and the late result
/// are all derived by the server from observations it made itself.
const WIRE_FORGED_CANCELLATION_KEYS: &[&str] = &[
    "cancellation_settlement",
    "connector_cancellation",
    "connector_late_result",
    "late_provider_result",
    "late_result",
    "lease_settlement",
    "stop_report",
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
        "kiana-int23-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn digest(label: &str) -> String {
    json_digest(&json!({"label": label}))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-cancellation".to_owned(),
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
            OPERATION: [
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
        "reason": "INT-23 cancellation boundary behaviour",
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

async fn approve(
    host: &DaemonHost,
    metadata: RequestMetadata,
    response: &ResponseEnvelope,
) -> ResponseEnvelope {
    assert_eq!(
        response.status,
        ExecutionStatus::AwaitingApproval,
        "command was not staged for approval: {:?}",
        response.error
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(response.output["approval"].clone()).expect("approval challenge");
    assert_eq!(challenge.risk, RiskLevel::ExternalSideEffect);
    host.handle(RequestEnvelope::approval_decision_with_proof(
        metadata,
        challenge.approval_id,
        ApprovalDecision::Approve,
        Some(challenge.request_hash.clone()),
        Some(challenge.nonce.clone()),
    ))
    .await
}

async fn bind(host: &DaemonHost, root: &Path, session: &str) {
    let bytes = fixture_bytes();
    let sha256 = connector_fixture_sha256(&bytes);
    write_fixture(root, &bytes);
    let pending = command(
        host,
        trusted_metadata(root, session),
        "connector.manage",
        bind_arguments(&sha256, 0, "bind-int23"),
    )
    .await;
    let bound = approve(host, trusted_metadata(root, session), &pending).await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "approved bind failed: {:?}",
        bound.error
    );
    assert_eq!(bound.output["state"], "active");
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

async fn connector_stream_events(host: &DaemonHost) -> Vec<RuntimeEvent> {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .into_iter()
        .filter(|event| event.aggregate_type.as_deref() == Some(CONNECTOR_STREAM))
        .collect()
}

async fn connector_stream_len(host: &DaemonHost) -> usize {
    connector_stream_events(host).await.len()
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

fn confirmed_kill(lifecycle: &ConnectorDispatchLifecycle) -> StopReport {
    StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Kill,
        true,
        true,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .expect("confirmed kill report")
}

fn unconfirmed_stop(lifecycle: &ConnectorDispatchLifecycle) -> StopReport {
    StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::Term,
        true,
        false,
        false,
        ProcessGroupState::Present,
        false,
        25,
    )
    .expect("unconfirmed stop report")
}

/// A "confirmed" report that never recorded a stop signal: incomplete evidence about started work.
fn signal_less_confirmed(lifecycle: &ConnectorDispatchLifecycle) -> StopReport {
    StopReport::new(
        lifecycle.invocation_id.to_string(),
        Some(12),
        Some(12),
        StopMethod::None,
        false,
        false,
        true,
        ProcessGroupState::Empty,
        true,
        25,
    )
    .expect("signal-less confirmed report")
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
        digest("seed-binding"),
        digest("seed-payload"),
        digest("seed-idempotency"),
    )
    .expect("seeded prepared lifecycle")
}

/// The chain a crashed worker leaves behind when the adapter was entered but the connector fact
/// was never committed: the prepared and dispatching facts, and nothing else.
fn seeded_chain(prepared: &ConnectorDispatchLifecycle) -> Vec<ConnectorDispatchLifecycle> {
    let dispatching = prepared
        .advance(ConnectorDispatchStage::Dispatching, None, None, None, None)
        .expect("seeded dispatching");
    vec![prepared.clone(), dispatching]
}

/// Seed a fresh EventLog with the committed binding fact plus the lifecycle chain a crashed worker
/// left behind.  The connector fact is intentionally absent: that is the crash window INT-23 must
/// reconstruct a cancellation from without a second effect.
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

#[tokio::test]
async fn wire_supplied_cancellation_material_is_refused_before_any_fact() {
    let root = project_dir("wire-cancellation");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int23-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    assert_eq!(connector_stream_len(&host).await, 1);
    let baseline_version = registry_list(&host, &root, session).await["registry_version"].clone();
    assert_eq!(baseline_version, json!(1));

    // The invoke route only accepts binding/operation/payload/idempotency keys, so a wire-supplied
    // stop report, settlement, lease settlement or late result is refused before the Broker, the
    // fixture adapter and the EventStore.
    for key in WIRE_FORGED_CANCELLATION_KEYS {
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
        assert_eq!(connector_stream_len(&host).await, 1);
        assert_eq!(
            registry_list(&host, &root, session).await["registry_version"],
            baseline_version
        );
    }

    // There is no cancel action and no cancellation evidence channel: the management route only
    // knows list/bind/revoke/reconcile, so a caller cannot request a cancellation at all.
    let cancel = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        json!({"action": "cancel"}),
    )
    .await;
    assert_denied(&cancel, "connector_action_invalid");

    let mut evidence = json!({
        "action": "revoke",
        "binding_id": BINDING_ID,
        "expected_registry_version": 1,
        "reason": "INT-23 cancellation evidence boundary",
        "idempotency_key": "wire-cancel-evidence",
    });
    evidence["stop_report"] = json!({"schema": "kiana.stop-report.v1"});
    let denied = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        evidence,
    )
    .await;
    assert_denied(&denied, "connector_arguments_invalid");
    assert_eq!(connector_event_count(&host, "connector.binding").await, 1);
    assert_eq!(connector_event_count(&host, "connector.cancelled").await, 0);
    assert_eq!(connector_stream_len(&host).await, 1);

    // The single spine still works: the compatibility command reaches the fixture adapter exactly
    // once, commits its own dispatch chain, and neither the fact stream nor the output retains any
    // cancellation material.
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
    assert_eq!(connector_event_count(&host, "connector.cancelled").await, 0);
    let persisted = serde_json::to_string(&connector_stream_events(&host).await)
        .expect("encode connector facts");
    let rendered = format!("{persisted}{}", invoked.output);
    for forbidden in [
        "cancellation_settlement",
        "connector_cancellation",
        "late_provider_result",
        "late_result",
        "lease_settlement",
        "stop_report",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "no connector fact or output may retain {forbidden}"
        );
    }

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_settled_attempt_reconstructs_from_durable_facts_and_fences_late_results() {
    let root = project_dir("reconstruct-cancellation");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int23-reconstruct";

    // One real attempt on the spine gives the server-owned command digest and the committed
    // binding fact that the seeded crash-window facts belong to.
    let reference = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("reference daemon host");
    bind(&reference, &root, session).await;
    let captured = command(
        &reference,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "cancel-source"),
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

    // A completed attempt is terminal: it cannot be settled as cancelled, and the refusal is a
    // domain verdict over the durable lifecycle rather than a new fact or a rewritten one.
    let reference_stream = connector_stream_events(&reference).await;
    let terminal = reference_stream
        .iter()
        .filter(|event| event.kind == CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND)
        .map(lifecycle_of)
        .find(|lifecycle| lifecycle.stage == ConnectorDispatchStage::ResultCommitted)
        .expect("committed dispatch lifecycle");
    assert_eq!(
        ConnectorCancellationRecord::from_stop_report(
            &terminal,
            digest("permit"),
            digest("lease"),
            &confirmed_kill(&terminal),
            1_000,
            1_025,
        )
        .unwrap_err(),
        "connector_cancellation_effect_already_observed"
    );
    assert_eq!(
        connector_stream_events(&reference).await.len(),
        reference_stream.len()
    );

    // A crashed worker leaves only the lifecycle chain on disk.  The two durable stages are read
    // back from the EventLog, not from an in-process memory.
    let events_path = project_dir("cancel-seed").join("events.jsonl");
    let prepared = seeded_prepared(&command_digest, 1);
    let chain = seeded_chain(&prepared);
    seed_events_log(&events_path, &root, &bind_event, &chain).await;
    let host = disk_host(&events_path);
    let stream = connector_stream_events(&host).await;
    assert_eq!(stream.len(), 1 + chain.len());
    let durable_prepared = stream
        .iter()
        .filter(|event| event.kind == CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND)
        .map(lifecycle_of)
        .find(|lifecycle| lifecycle.stage == ConnectorDispatchStage::Prepared)
        .expect("durable prepared stage");
    let durable_dispatching = stream
        .iter()
        .filter(|event| event.kind == CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND)
        .map(lifecycle_of)
        .find(|lifecycle| lifecycle.stage == ConnectorDispatchStage::Dispatching)
        .expect("durable dispatching stage");

    // A "confirmed" report that never sent a stop signal is incomplete evidence about a started
    // effect: the durable attempt must not settle as cancelled, and the lease stays held.
    assert_eq!(
        ConnectorCancellationRecord::from_stop_report(
            &durable_dispatching,
            digest("permit"),
            digest("lease"),
            &signal_less_confirmed(&durable_dispatching),
            1_000,
            1_025,
        )
        .unwrap_err(),
        "connector_cancellation_stop_attempt_missing"
    );

    // stop_confirmed: a started effect keeps its lease held for reconciliation.
    let mut record = ConnectorCancellationRecord::from_stop_report(
        &durable_dispatching,
        digest("permit"),
        digest("lease"),
        &confirmed_kill(&durable_dispatching),
        1_000,
        1_025,
    )
    .expect("stop confirmed record");
    assert_eq!(
        record.reconstruct_outcome(&durable_dispatching).unwrap(),
        (
            ConnectorCancellationState::StopConfirmed,
            ConnectorLeaseSettlement::HeldForReconciliation
        )
    );

    // A provider result that arrives after the settlement is fenced evidence: recording it moves
    // nothing, and the durable stream is not extended by the settlement itself.
    let late = ConnectorLateResult::new(
        durable_dispatching.invocation_id,
        durable_dispatching.attempt,
        digest("late-result"),
        2_000,
    )
    .expect("late result");
    assert_eq!(
        record.record_late_result(late).expect("fenced evidence"),
        ConnectorLateResultDisposition::Fenced
    );
    assert_eq!(
        record.reconstruct_outcome(&durable_dispatching).unwrap(),
        (
            ConnectorCancellationState::StopConfirmed,
            ConnectorLeaseSettlement::HeldForReconciliation
        )
    );
    assert_eq!(connector_stream_events(&host).await.len(), 1 + chain.len());
    let listed = registry_list(&host, &root, session).await;
    assert_eq!(listed["registry_version"], json!(1 + chain.len() as u64));
    assert_eq!(
        listed["unresolved"].as_array().map(Vec::len),
        Some(0),
        "a crashed attempt without a connector fact is not an unresolved invocation fact"
    );

    // not_executed: the prepared stage proves nothing was executed, so the confirmed stop gives
    // the lease back.
    let not_executed = ConnectorCancellationRecord::from_stop_report(
        &durable_prepared,
        digest("permit"),
        digest("lease"),
        &connector_not_executed_stop_report(durable_prepared.invocation_id).expect("stop"),
        1_000,
        1_001,
    )
    .expect("not executed record");
    assert_eq!(
        not_executed.reconstruct_outcome(&durable_prepared).unwrap(),
        (
            ConnectorCancellationState::NotExecuted,
            ConnectorLeaseSettlement::Released
        )
    );

    // unknown: an unconfirmed stop is not a cancellation, in either direction.
    let unknown = ConnectorCancellationRecord::from_stop_report(
        &durable_dispatching,
        digest("permit"),
        digest("lease"),
        &unconfirmed_stop(&durable_dispatching),
        1_000,
        1_025,
    )
    .expect("unknown record");
    assert_eq!(
        unknown.reconstruct_outcome(&durable_dispatching).unwrap(),
        (
            ConnectorCancellationState::Unknown,
            ConnectorLeaseSettlement::HeldForReconciliation
        )
    );

    // The reconstructed outcome can never be turned back into a live attempt, and the durable
    // facts retain no cancellation material: the settlement is evidence about the attempt, not a
    // second execution path.
    let replayed: ConnectorCancellationRecord =
        serde_json::from_value(serde_json::to_value(&record).expect("encode cancellation record"))
            .expect("decode cancellation record");
    assert_eq!(
        replayed.reconstruct_outcome(&durable_dispatching).unwrap(),
        (
            ConnectorCancellationState::StopConfirmed,
            ConnectorLeaseSettlement::HeldForReconciliation
        )
    );
    let persisted = serde_json::to_string(&connector_stream_events(&host).await)
        .expect("encode connector facts");
    for forbidden in [
        "cancellation_settlement",
        "late_result",
        "lease_settlement",
        "stop_report",
    ] {
        assert!(
            !persisted.contains(forbidden),
            "seeded connector facts must not retain {forbidden}"
        );
    }

    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(events_path.parent().unwrap());
}
