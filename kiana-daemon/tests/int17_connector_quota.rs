//! INT-17 daemon behaviour fixture: durable quota/rate accounting and server-owned envelopes.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path.  The
//! deny-first case proves a caller cannot smuggle the quota reservation/claim/policy envelope
//! through the wire (they are server-owned and rejected before the Broker), and the restart case
//! proves the per-binding rate accounting is rebuilt from the durable EventLog rather than from
//! an in-memory counter that resets to zero: a second host over the same event file still sees
//! the earlier invocations, refuses the over-limit one and replays the original receipt for the
//! same idempotency key.  No network transport, second execution loop or model loop is involved.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, connector_payload_sha256, AccountBinding, ConnectorDefinition,
    ConnectorEffect, ConnectorOperation, CONNECTOR_STREAM,
};
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

const FIXTURE_PATH: &str = "fixtures/connector-quota.json";
const BINDING_ID: &str = "binding-quota";
const CONNECTOR_ID: &str = "connector-quota";
const ACCOUNT_ID: &str = "account-quota";
const OPERATION: &str = "lookup";
/// Two invocations fit into the UTC window; the third must be refused from the durable facts.
const RATE_LIMIT_PER_MINUTE: u32 = 2;

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
        "kiana-int17-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-quota".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            OPERATION.to_owned(),
            ConnectorOperation {
                effect: ConnectorEffect::ReadOnly,
                required_scope: "read".to_owned(),
                data_classes: BTreeSet::new(),
            },
        )]),
        rate_limit_per_minute: RATE_LIMIT_PER_MINUTE,
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
                {
                    "payload": {"id": "two"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-two",
                    "result": {"value": "two"},
                },
                {
                    "payload": {"id": "three"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-three",
                    "result": {"value": "three"},
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
        "reason": "INT-17 durable quota accounting",
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
        bind_arguments(&digest, 0, "bind-int17"),
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

/// The durable host: the same event file survives a host restart, so accounting rebuilt from it
/// is not an in-memory counter.
fn disk_host(events_path: &Path) -> DaemonHost {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&json!([])).unwrap()));
    DaemonHost::with_harness_on_disk_and_project_authority(
        harness,
        events_path,
        Arc::new(TrustedProject),
    )
    .expect("disk daemon host")
}

#[tokio::test]
async fn wire_supplied_quota_envelope_is_refused_before_any_effect() {
    let root = project_dir("quota-wire");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int17-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    let baseline = connector_event_count(&host, "connector.invoked").await;
    assert_eq!(baseline, 0);
    let bound_stream_len = connector_stream_len(&host).await;

    // Quota reservation/claim/policy material is server-owned: the invoke route only accepts the
    // binding/operation/payload/idempotency keys, so any smuggled quota object is refused before
    // the Broker or the adapter are reached.
    let smuggled = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        json!({
            "binding_id": BINDING_ID,
            "operation": OPERATION,
            "payload": {"id": "one"},
            "idempotency_key": "smuggled-1",
            "connector_quota_reservation": {"schema": "kiana.connector-quota-reservation.v1"},
            "connector_quota_claim": {"schema": "kiana.connector-quota-claim.v1"},
            "connector_quota_policy": {"schema": "kiana.connector-quota-policy.v1"},
        }),
    )
    .await;
    assert_denied(&smuggled, "connector_arguments_invalid");
    assert_eq!(
        connector_event_count(&host, "connector.invoked").await,
        baseline,
        "a wire-supplied quota envelope must not reach the adapter"
    );
    assert_eq!(connector_stream_len(&host).await, bound_stream_len);

    // A lone policy object is refused with the same stable code.
    let lone_policy = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        json!({
            "binding_id": BINDING_ID,
            "operation": OPERATION,
            "payload": {"id": "one"},
            "idempotency_key": "smuggled-2",
            "connector_quota_policy": {"schema": "kiana.connector-quota-policy.v1"},
        }),
    )
    .await;
    assert_denied(&lone_policy, "connector_arguments_invalid");
    assert_eq!(
        connector_event_count(&host, "connector.invoked").await,
        baseline
    );
    let registry = registry_list(&host, &root, session).await;
    assert_eq!(registry["registry_version"], 1);
    assert_eq!(registry["bindings"][BINDING_ID]["revision"], 1);

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn rate_accounting_survives_a_restart_and_is_never_reset_to_zero() {
    let root = project_dir("quota-restart");
    fs::create_dir_all(&root).unwrap();
    let events_path = project_dir("quota-restart-events").join("events.jsonl");
    let session = "session-int17-restart";

    let first_event_id;
    let first_receipt;
    {
        let host = disk_host(&events_path);
        bind(&host, &root, session).await;

        let first = command(
            &host,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(json!({"id": "one"}), "quota-1"),
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
        first_event_id = first.output["event_id"].as_str().unwrap().to_owned();
        first_receipt = first.output["receipt"].clone();

        let second = command(
            &host,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(json!({"id": "two"}), "quota-2"),
        )
        .await;
        assert_eq!(
            second.status,
            ExecutionStatus::Completed,
            "second invoke failed: {:?}",
            second.error
        );

        // The third invocation in the same window exceeds the binding's durable rate ceiling.
        let third = command(
            &host,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(json!({"id": "three"}), "quota-3"),
        )
        .await;
        assert_denied(&third, "connector_rate_limit_exceeded");
        assert_eq!(
            connector_event_count(&host, "connector.invoked").await,
            2,
            "an over-limit invocation must not append a connector fact"
        );
        let stream_len = connector_stream_len(&host).await;
        let registry_version =
            registry_list(&host, &root, session).await["registry_version"].clone();
        assert_eq!(registry_version, 1);
        drop(host);

        // Restarted process: a fresh host over the same event file must rebuild the accounting
        // from the committed facts instead of starting from an empty in-memory counter.
        let restarted = disk_host(&events_path);
        assert_eq!(
            connector_event_count(&restarted, "connector.invoked").await,
            2,
            "durable connector facts survive the restart"
        );
        assert_eq!(connector_stream_len(&restarted).await, stream_len);
        let registry = registry_list(&restarted, &root, session).await;
        assert_eq!(registry["registry_version"], 1);
        assert_eq!(registry["bindings"][BINDING_ID]["revision"], 1);

        // Still over the durable window ceiling: the restart cannot bypass it.
        let after_restart = command(
            &restarted,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(json!({"id": "three"}), "quota-4"),
        )
        .await;
        assert_denied(&after_restart, "connector_rate_limit_exceeded");
        assert_eq!(
            connector_event_count(&restarted, "connector.invoked").await,
            2
        );
        assert_eq!(connector_stream_len(&restarted).await, stream_len);

        // The idempotent replay of the first command still returns the original receipt and does
        // not create a second attempt, so quota accounting stays one-effect-per-command.
        let replayed = command(
            &restarted,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(json!({"id": "one"}), "quota-1"),
        )
        .await;
        assert_eq!(
            replayed.status,
            ExecutionStatus::Completed,
            "replay failed: {:?}",
            replayed.error
        );
        assert_eq!(replayed.output["replayed"], true);
        assert_eq!(
            replayed.output["event_id"].as_str(),
            Some(first_event_id.as_str())
        );
        assert_eq!(replayed.output["receipt"], first_receipt);
        assert_eq!(
            replayed.output["receipt"]["final_payload_sha256"].as_str(),
            Some(connector_payload_sha256(&json!({"id": "one"})).as_str())
        );
        assert_eq!(
            connector_event_count(&restarted, "connector.invoked").await,
            2
        );
        assert_eq!(connector_stream_len(&restarted).await, stream_len);
    }

    let _ = fs::remove_dir_all(&root);
}
