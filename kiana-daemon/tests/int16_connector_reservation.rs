//! INT-16 daemon behaviour fixture: invocation reservation, command digest and idempotent replay.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding.  They prove the reachable reservation semantics at
//! the daemon boundary: every connector fact records the server-owned command digest, an
//! identical command replays the original receipt without a second attempt, the same idempotency
//! key over a different command digest is refused with zero adapter effect (the second payload is
//! a registered fixture case, so only the digest check can explain the denial), and a reservation
//! envelope cannot be supplied on the wire because ControlPlane refuses it before the Broker.
//! No network transport, second execution loop or model loop is involved.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, connector_payload_sha256, AccountBinding, ConnectorDefinition,
    ConnectorEffect, ConnectorOperation, CONNECTOR_STREAM,
};
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ExecutionStatus, RequestEnvelope, RequestMetadata,
    ResponseEnvelope, RiskLevel,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const FIXTURE_PATH: &str = "fixtures/connector-reservation.json";
const BINDING_ID: &str = "binding-reservation";
const CONNECTOR_ID: &str = "connector-reservation";
const ACCOUNT_ID: &str = "account-reservation";
const OPERATION: &str = "lookup";

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
        "kiana-int16-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-reservation".to_owned(),
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

/// Both payloads are registered fixture cases on purpose: the digest conflict below must be the
/// only reason the second payload under the first key is refused.
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
                {
                    "payload": {"id": "two"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-two",
                    "result": {"value": "two"},
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
        "reason": "INT-16 invocation reservation behaviour",
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
        bind_arguments(&digest, 0, "bind-int16"),
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
    assert_eq!(bound.output["external_transport"], "not_supported");
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

async fn connector_event_count(host: &DaemonHost, kind: &str) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

/// Every connector-stream event, including the dispatch lifecycle chain.  A denied or replayed
/// command must leave this length unchanged: the stream is the durable connector fact history.
async fn connector_stream_len(host: &DaemonHost) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.aggregate_type.as_deref() == Some(CONNECTOR_STREAM))
        .count()
}

/// The connector fact is the adapter-effect evidence.  A denied or replayed command must never
/// append another one, whatever the terminal capability status is.
async fn invoked_receipts(host: &DaemonHost) -> Vec<Value> {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.kind == "connector.invoked")
        .map(|event| event.data["receipt"].clone())
        .collect()
}

#[tokio::test]
async fn same_key_with_a_different_command_digest_is_denied_before_any_effect() {
    let root = project_dir("digest-conflict");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int16-conflict";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    let bound_stream_len = connector_stream_len(&host).await;
    let bound_version = registry_list(&host, &root, session).await["registry_version"].clone();

    let first = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "reserve-1"),
    )
    .await;
    assert_eq!(
        first.status,
        ExecutionStatus::Completed,
        "first invoke failed: {:?}",
        first.error
    );
    assert_eq!(first.output["replayed"], false);
    assert_eq!(first.output["receipt"]["outcome"], "succeeded");
    assert_eq!(first.output["external_effect_performed"], false);
    let first_event_id = first.output["event_id"].as_str().unwrap().to_owned();
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    let invoked_stream_len = connector_stream_len(&host).await;
    assert!(invoked_stream_len > bound_stream_len);
    let invoked_version = registry_list(&host, &root, session).await["registry_version"].clone();
    assert_ne!(invoked_version, bound_version);
    let facts = invoked_receipts(&host).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0]["final_payload_sha256"].as_str(),
        Some(connector_payload_sha256(&json!({"id": "one"})).as_str())
    );

    // The same idempotency key over a different canonical payload is a different command
    // digest. It is refused before the adapter and leaves the original fact untouched.
    let conflicting = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "two"}), "reserve-1"),
    )
    .await;
    assert_denied(
        &conflicting,
        "connector_idempotency_command_digest_conflict",
    );
    assert_eq!(
        conflicting.status,
        ExecutionStatus::ResultUnknown,
        "a refused dispatch must fail closed: {:?}",
        conflicting.error
    );
    assert_eq!(conflicting.output["dispatch_rejected"], true);
    assert_eq!(
        connector_event_count(&host, "connector.invoked").await,
        1,
        "a digest conflict must not append a connector fact"
    );
    assert_eq!(
        connector_stream_len(&host).await,
        invoked_stream_len,
        "a digest conflict must not append any connector-stream fact"
    );
    let facts = invoked_receipts(&host).await;
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0]["final_payload_sha256"].as_str(),
        Some(connector_payload_sha256(&json!({"id": "one"})).as_str())
    );
    assert!(facts.iter().all(|receipt| receipt["final_payload_sha256"]
        != json!(connector_payload_sha256(&json!({"id": "two"})))));
    let conflicted_version = registry_list(&host, &root, session).await["registry_version"].clone();
    assert_eq!(conflicted_version, invoked_version);

    // A different key over the same payload is a new command and dispatches as a new attempt.
    let second = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "two"}), "reserve-2"),
    )
    .await;
    assert_eq!(
        second.status,
        ExecutionStatus::Completed,
        "second key failed: {:?}",
        second.error
    );
    assert_eq!(second.output["replayed"], false);
    let second_event_id = second.output["event_id"].as_str().unwrap().to_owned();
    assert_ne!(second_event_id, first_event_id);
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 2);
    assert!(connector_stream_len(&host).await > invoked_stream_len);
    let registry = registry_list(&host, &root, session).await;
    assert_eq!(registry["bindings"][BINDING_ID]["revision"], 1);
    assert!(registry["unresolved"].as_array().unwrap().is_empty());

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn replayed_command_returns_the_original_receipt_and_keeps_one_attempt() {
    let root = project_dir("replay");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int16-replay";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;

    let payload = json!({"id": "one"});
    let key = "reserve-replay";
    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(payload.clone(), key),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "invoke failed: {:?}",
        invoked.error
    );
    assert_eq!(invoked.output["replayed"], false);
    assert_eq!(
        invoked.output["dispatch_lifecycle"]["stage"],
        "result_committed"
    );
    assert_eq!(invoked.output["effect_observation"]["attempt"], 1);
    let event_id = invoked.output["event_id"].as_str().unwrap().to_owned();
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    let invoked_stream_len = connector_stream_len(&host).await;
    let invoked_version = registry_list(&host, &root, session).await["registry_version"].clone();

    // The same key over the same canonical command replays the original receipt: the event
    // identity, receipt and committed dispatch lifecycle are the originals, not a second attempt.
    let replayed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(payload, key),
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
    assert_eq!(
        replayed.output["event_id"].as_str(),
        Some(event_id.as_str())
    );
    assert_eq!(replayed.output["receipt"], invoked.output["receipt"]);
    assert_eq!(
        replayed.output["dispatch_lifecycle"],
        invoked.output["dispatch_lifecycle"]
    );
    assert_eq!(
        replayed.output["effect_observation"],
        invoked.output["effect_observation"]
    );
    // One attempt stays one attempt: the replay appends no connector-stream fact at all.
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    assert_eq!(connector_stream_len(&host).await, invoked_stream_len);
    let replayed_version = registry_list(&host, &root, session).await["registry_version"].clone();
    assert_eq!(replayed_version, invoked_version);

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn wire_supplied_reservation_envelope_is_refused_before_any_effect() {
    let root = project_dir("wire-envelope");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int16-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    let baseline = connector_event_count(&host, "connector.invoked").await;
    assert_eq!(baseline, 0);

    // Reservation and permit material is server-owned: a caller cannot smuggle it through the
    // wire, and the unknown arguments are refused before the Broker or the adapter are reached.
    let smuggled = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        json!({
            "binding_id": BINDING_ID,
            "operation": OPERATION,
            "payload": {"id": "one"},
            "idempotency_key": "smuggled-1",
            "connector_reservation": {"schema": "kiana.connector-invocation-reservation.v1"},
            "connector_permit": {"schema": "kiana.connector-invocation-permit.v1"},
            "connector_effect_permit": {"schema": "kiana.connector-effect-permit.v1"},
        }),
    )
    .await;
    assert_denied(&smuggled, "connector_arguments_invalid");
    assert_eq!(
        connector_event_count(&host, "connector.invoked").await,
        baseline,
        "a wire-supplied reservation envelope must not reach the adapter"
    );
    assert_eq!(connector_stream_len(&host).await, 1);
    let registry = registry_list(&host, &root, session).await;
    assert_eq!(registry["registry_version"], 1);
    assert_eq!(registry["bindings"][BINDING_ID]["revision"], 1);

    let _ = fs::remove_dir_all(&root);
}
