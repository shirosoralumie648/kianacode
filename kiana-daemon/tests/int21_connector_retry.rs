//! INT-21 daemon boundary fixture: retry classification is server-side only.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding.  The invoke route accepts only binding/operation/
//! payload/idempotency keys, so wire-supplied retry material (decision, class, policy, next
//! attempt, retry_after, an attempt override or an idempotency policy) is refused before any
//! effect, and a completed attempt is never auto-retried: the durable facts keep exactly one
//! attempt and carry no retry material.  No network transport, second execution loop or model
//! loop is involved.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, AccountBinding, ConnectorDefinition, ConnectorEffect,
    ConnectorOperation, RuntimeEvent, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND, CONNECTOR_STREAM,
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

const FIXTURE_PATH: &str = "fixtures/connector-retry.json";
const BINDING_ID: &str = "binding-retry";
const CONNECTOR_ID: &str = "connector-retry";
const ACCOUNT_ID: &str = "account-retry";
const OPERATION: &str = "lookup";

/// INT-21 retry material that must never be accepted from the wire.  `attempt` is included here
/// because the admitted attempt is server-owned: a caller cannot bump it by supplying a key.
const WIRE_FORGED_RETRY_KEYS: &[&str] = &[
    "attempt",
    "attempt_override",
    "connector_retry_binding",
    "connector_retry_decision",
    "connector_retry_policy",
    "idempotency_policy",
    "next_attempt",
    "retry",
    "retry_after_ms",
    "retry_class",
    "retry_decision",
    "retry_policy",
];

/// Retry material that must never appear in the durable connector facts or in the invoke output.
/// `attempt` is excluded because the dispatch lifecycle legitimately carries the server-owned
/// attempt number; the fixtures assert that number separately.
const RETRY_MATERIAL_KEYS: &[&str] = &[
    "attempt_override",
    "connector_retry_binding",
    "connector_retry_decision",
    "connector_retry_policy",
    "idempotency_policy",
    "next_attempt",
    "retry",
    "retry_after_ms",
    "retry_class",
    "retry_decision",
    "retry_policy",
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
        "kiana-int21-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-retry".to_owned(),
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
        "reason": "INT-21 retry classifier boundary behaviour",
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
        bind_arguments(&digest, 0, "bind-int21"),
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

async fn connector_event_count(host: &DaemonHost, kind: &str) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

fn assert_no_retry_material(value: &Value, context: &str) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                assert!(
                    !RETRY_MATERIAL_KEYS.contains(&key.as_str()),
                    "{context} leaked retry material key {key}"
                );
                assert_no_retry_material(nested, context);
            }
        }
        Value::Array(items) => {
            for item in items {
                assert_no_retry_material(item, context);
            }
        }
        _ => {}
    }
}

#[tokio::test]
async fn wire_supplied_retry_material_is_refused_before_any_dispatch() {
    let root = project_dir("retry-wire");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int21-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    assert_eq!(connector_stream_events(&host).await.len(), 1);

    // The invoke route accepts only binding/operation/payload/idempotency keys, so a wire-supplied
    // retry decision, class, policy, next attempt, retry_after or attempt override is refused
    // before the Broker or the adapter runs.
    for key in WIRE_FORGED_RETRY_KEYS {
        let mut arguments = invoke_arguments(json!({"id": "one"}), &format!("wire-{key}"));
        arguments[key] = json!({"schema": format!("forged.{key}.v1"), "retry": true});
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
        assert_eq!(
            connector_stream_events(&host).await.len(),
            1,
            "a wire-supplied {key} must not commit a connector fact"
        );
    }

    // The single spine still works: the compatibility command reaches the fixture adapter exactly
    // once, and neither the output nor the durable facts carry retry material.
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
    assert_eq!(invoked.output["receipt"]["outcome"], "succeeded");
    assert_no_retry_material(&invoked.output, "invoke output");
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    assert_eq!(
        connector_event_count(&host, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND).await,
        4
    );

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_completed_attempt_is_never_auto_retried_and_facts_keep_one_attempt() {
    let root = project_dir("retry-completed");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int21-completed";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;

    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "retry-attempt-one"),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "invoke failed: {:?}",
        invoked.error
    );
    assert_eq!(invoked.output["replayed"], false);
    assert_eq!(invoked.output["external_effect_performed"], false);
    assert_eq!(
        invoked.output["dispatch_lifecycle"]["stage"],
        "result_committed"
    );
    assert_eq!(invoked.output["dispatch_lifecycle"]["attempt"], 1);

    // One attempt, one connector fact, four lifecycle facts, all attempt 1: the daemon never
    // retries a completed attempt on its own and never mints a retry decision.
    let events = connector_stream_events(&host).await;
    assert_eq!(events.len(), 6, "binding + four lifecycle facts + invoked");
    for event in &events {
        assert!(
            !event.kind.contains("retry"),
            "connector stream must not contain a retry fact: {}",
            event.kind
        );
        assert_no_retry_material(&event.data, &format!("connector fact {}", event.kind));
    }
    let lifecycle_attempts: Vec<u64> = events[1..5]
        .iter()
        .map(|event| event.data["lifecycle"]["attempt"].as_u64().unwrap())
        .collect();
    assert_eq!(lifecycle_attempts, vec![1, 1, 1, 1]);

    // Replaying the same command returns the original receipt/observation/lifecycle and does not
    // start a second attempt: same event id, same stream length, same single connector fact.
    let replayed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "retry-attempt-one"),
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
