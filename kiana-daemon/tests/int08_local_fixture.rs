//! INT-08 daemon behaviour fixture: local fixture bind / invoke / replay / reconcile.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned fixture file on disk.  The fixture adapter is the only adapter exercised and
//! no network transport exists.  Every deny (replaced bytes, unknown operation, unregistered
//! operation) must leave the connector registry unchanged and produce no provider receipt.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, connector_payload_sha256, AccountBinding, ConnectorDefinition,
    ConnectorEffect, ConnectorOperation,
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

const FIXTURE_PATH: &str = "fixtures/connector.json";

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn fixture_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-int08-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-demo".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([
            (
                "lookup".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
            (
                "poll".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
        ]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    }
}

fn account_binding(fixture_sha256: &str) -> AccountBinding {
    AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: "binding-demo".to_owned(),
        connector_id: "connector-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::new(),
        fixture_path: FIXTURE_PATH.to_owned(),
        fixture_sha256: fixture_sha256.to_owned(),
        credential_ref: None,
    }
}

fn default_operations() -> Value {
    json!({
        "lookup": [{
            "payload": {"id": "one"},
            "outcome": "succeeded",
            "receipt_id": "receipt-one",
            "result": {"value": "fixture"},
        }],
        "poll": [{
            "payload": {"id": "unknown"},
            "outcome": "unknown",
            "receipt_id": "receipt-unknown",
            "result": {"detail": "pending"},
        }],
    })
}

fn fixture_bytes(operations: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "kiana.connector-fixture.v1",
        "connector_id": "connector-demo",
        "account_id": "account-demo",
        "operations": operations,
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
        "reason": "INT-08 local fixture acceptance",
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
        serde_json::from_value(response.output["approval"].clone()).unwrap();
    host.handle(RequestEnvelope::approval_decision_with_proof(
        metadata,
        challenge.approval_id,
        ApprovalDecision::Approve,
        Some(challenge.request_hash.clone()),
        Some(challenge.nonce.clone()),
    ))
    .await
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

#[tokio::test]
async fn fixture_bind_invoke_replay_and_reconcile_are_deterministic() {
    let root = fixture_dir("bind-invoke-replay-reconcile");
    fs::create_dir_all(&root).unwrap();
    let bytes = fixture_bytes(default_operations());
    let digest = connector_fixture_sha256(&bytes);
    write_fixture(&root, &bytes);
    let session = "session-int08-e2e";

    let host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject)).unwrap();

    // Bind is an operator action: it must be approved before the fixture is parsed, and the
    // approval material must declare the external-side-effect risk.
    let pending = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        bind_arguments(&digest, 0, "bind-1"),
    )
    .await;
    assert_eq!(
        pending.status,
        ExecutionStatus::AwaitingApproval,
        "bind was not staged: {:?}",
        pending.error
    );
    let challenge: ApprovalChallenge =
        serde_json::from_value(pending.output["approval"].clone()).unwrap();
    assert_eq!(challenge.risk, RiskLevel::ExternalSideEffect);
    let bound = host
        .handle(RequestEnvelope::approval_decision_with_proof(
            trusted_metadata(&root, session),
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
    assert_eq!(bound.output["schema"], "kiana.connector-binding-result.v1");
    assert_eq!(bound.output["state"], "active");
    assert_eq!(bound.output["health"], "local_fixture_parsed");
    assert_eq!(bound.output["external_transport"], "not_supported");
    assert_eq!(bound.output["registry_version"], 1);
    assert_eq!(
        bound.output["fixture_sha256"].as_str(),
        Some(digest.as_str())
    );

    // A known operation returns a deterministic local receipt with no external effect.
    let invoke_arguments = json!({
        "binding_id": "binding-demo",
        "operation": "lookup",
        "payload": {"id": "one"},
        "idempotency_key": "lookup-1",
    });
    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments.clone(),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "fixture invoke failed: {:?}",
        invoked.error
    );
    assert_eq!(
        invoked.output["schema"],
        "kiana.connector-invocation-result.v1"
    );
    assert_eq!(invoked.output["receipt"]["outcome"], "succeeded");
    assert_eq!(invoked.output["receipt"]["source"], "local_fixture");
    assert_eq!(invoked.output["receipt"]["connector_id"], "connector-demo");
    assert_eq!(invoked.output["receipt"]["binding_id"], "binding-demo");
    assert_eq!(invoked.output["receipt"]["account_id"], "account-demo");
    assert_eq!(invoked.output["receipt"]["operation"], "lookup");
    assert_eq!(invoked.output["receipt"]["idempotency_key"], "lookup-1");
    assert_eq!(
        invoked.output["receipt"]["final_payload_sha256"].as_str(),
        Some(connector_payload_sha256(&json!({"id": "one"})).as_str())
    );
    assert_eq!(invoked.output["external_effect_performed"], false);
    assert_eq!(invoked.output["proof_source"], "local_fixture");
    assert_eq!(invoked.output["replayed"], false);
    assert_eq!(
        invoked.output["dispatch_lifecycle"]["stage"],
        "result_committed"
    );
    assert_eq!(invoked.output["effect_observation"]["attempt"], 1);
    assert!(invoked.output["event_id"].is_string());
    let invocation_event_id = invoked.output["event_id"].as_str().unwrap().to_owned();

    // The same idempotency key and canonical payload replay the original receipt instead of
    // dispatching a second time.
    let replayed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments,
    )
    .await;
    assert_eq!(
        replayed.status,
        ExecutionStatus::Completed,
        "replay failed: {:?}",
        replayed.error
    );
    assert_eq!(replayed.output["replayed"], true);
    assert_eq!(replayed.output["receipt"], invoked.output["receipt"]);
    assert_eq!(replayed.output["event_id"], invoked.output["event_id"]);
    assert_eq!(
        replayed.output["event_id"].as_str(),
        Some(invocation_event_id.as_str())
    );

    // An unknown provider outcome stays unknown and is not retried automatically.
    let unknown = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        json!({
            "binding_id": "binding-demo",
            "operation": "poll",
            "payload": {"id": "unknown"},
            "idempotency_key": "poll-1",
        }),
    )
    .await;
    assert_eq!(
        unknown.status,
        ExecutionStatus::ResultUnknown,
        "unknown outcome changed status: {:?}",
        unknown.error
    );
    assert_eq!(unknown.output["receipt"]["outcome"], "unknown");
    assert_eq!(unknown.output["dispatch_lifecycle"]["stage"], "unknown");
    let unknown_invocation_event_id = unknown.output["event_id"].as_str().unwrap().to_owned();
    let unresolved_before = registry_list(&host, &root, session).await;
    assert_eq!(unresolved_before["unresolved"].as_array().unwrap().len(), 1);

    // Explicit reconciliation consumes hash-pinned manual evidence and appends a fact instead of
    // overwriting the original unknown invocation.
    let mut manual_receipt = unknown.output["receipt"].clone();
    manual_receipt["outcome"] = json!("succeeded");
    manual_receipt["provider_receipt_id"] = json!("receipt-manual-one");
    let manual_bytes = serde_json::to_vec(&manual_receipt).unwrap();
    let manual_path = root.join("receipts/manual-receipt.json");
    fs::create_dir_all(manual_path.parent().unwrap()).unwrap();
    fs::write(&manual_path, &manual_bytes).unwrap();
    let manual_digest = connector_fixture_sha256(&manual_bytes);
    let registry_version = unresolved_before["registry_version"].as_u64().unwrap();

    let reconcile = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        json!({
            "action": "reconcile",
            "invocation_event_id": json!(unknown_invocation_event_id.as_str()),
            "receipt_path": "receipts/manual-receipt.json",
            "receipt_sha256": manual_digest,
            "expected_registry_version": registry_version,
            "reason": "INT-08 manual reconciliation evidence",
            "idempotency_key": "reconcile-1",
        }),
    )
    .await;
    let reconciled = approve(&host, trusted_metadata(&root, session), &reconcile).await;
    assert_eq!(
        reconciled.status,
        ExecutionStatus::Completed,
        "reconcile failed: {:?}",
        reconciled.error
    );
    assert_eq!(reconciled.output["reconciled"], true);
    assert_eq!(reconciled.output["receipt"]["outcome"], "succeeded");
    assert_eq!(
        reconciled.output["receipt"]["provider_receipt_id"],
        "receipt-manual-one"
    );
    assert_eq!(
        reconciled.output["reconciliation_case"]["state"],
        "reconciled"
    );
    assert_eq!(reconciled.output["reconciliation_case"]["attempt"], 1);
    assert!(reconciled.output["reconciliation_case"]["original_receipt_digest"].is_string());
    assert_eq!(
        reconciled.output["invocation_event_id"],
        json!(unknown_invocation_event_id.as_str())
    );

    let unresolved_after = registry_list(&host, &root, session).await;
    assert!(unresolved_after["unresolved"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        unresolved_after["bindings"]["binding-demo"]["binding"]["binding_id"],
        "binding-demo"
    );

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn fixture_identity_replacement_and_unknown_operation_are_refused_before_any_effect() {
    let root = fixture_dir("local-fixture-deny");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int08-deny";
    let host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject)).unwrap();

    // The definition registers `lookup` and `poll`; a fixture that omits one of them is refused
    // before a binding exists.
    let missing_operation = fixture_bytes(json!({
        "lookup": [{
            "payload": {"id": "one"},
            "outcome": "succeeded",
            "receipt_id": "receipt-one",
            "result": {"value": "fixture"},
        }],
    }));
    let missing_digest = connector_fixture_sha256(&missing_operation);
    write_fixture(&root, &missing_operation);
    let pending = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        bind_arguments(&missing_digest, 0, "bind-missing"),
    )
    .await;
    let denied = approve(&host, trusted_metadata(&root, session), &pending).await;
    assert_denied(&denied, "connector_fixture_operation_missing");
    let listed = registry_list(&host, &root, session).await;
    assert_eq!(listed["registry_version"], 0);
    assert!(listed["bindings"].as_object().unwrap().is_empty());

    // An operation the definition does not register cannot be injected through the fixture.
    let mut unregistered_operations = default_operations();
    unregistered_operations["extra"] = json!([{
        "payload": {"id": "one"},
        "outcome": "succeeded",
        "receipt_id": "receipt-extra",
        "result": {"value": "extra"},
    }]);
    let unregistered = fixture_bytes(unregistered_operations);
    let unregistered_digest = connector_fixture_sha256(&unregistered);
    write_fixture(&root, &unregistered);
    let pending = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        bind_arguments(&unregistered_digest, 0, "bind-unregistered"),
    )
    .await;
    let denied = approve(&host, trusted_metadata(&root, session), &pending).await;
    assert_denied(&denied, "connector_fixture_operation_unregistered");
    let listed = registry_list(&host, &root, session).await;
    assert_eq!(listed["registry_version"], 0);
    assert!(listed["bindings"].as_object().unwrap().is_empty());

    // Replacing the bytes after the digest was pinned is refused before the fixture is parsed.
    let original = fixture_bytes(default_operations());
    let original_digest = connector_fixture_sha256(&original);
    let mut tampered_operations = default_operations();
    tampered_operations["lookup"][0]["result"] = json!({"value": "tampered"});
    let tampered = fixture_bytes(tampered_operations);
    write_fixture(&root, &tampered);
    let pending = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        bind_arguments(&original_digest, 0, "bind-replaced"),
    )
    .await;
    let denied = approve(&host, trusted_metadata(&root, session), &pending).await;
    assert_denied(&denied, "connector_fixture_hash_mismatch");
    let listed = registry_list(&host, &root, session).await;
    assert_eq!(listed["registry_version"], 0);
    assert!(listed["bindings"].as_object().unwrap().is_empty());

    // Restoring the pinned bytes binds successfully, so the earlier denials were about the
    // tampered content and not about the fixture contract itself.
    write_fixture(&root, &original);
    let pending = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        bind_arguments(&original_digest, 0, "bind-ok"),
    )
    .await;
    let bound = approve(&host, trusted_metadata(&root, session), &pending).await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "restored fixture failed to bind: {:?}",
        bound.error
    );

    // Replacing the file again makes the next invoke result_unknown with zero dispatch: the
    // binding stays at revision 1 and no new receipt is produced.
    write_fixture(&root, &tampered);
    let denied = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        json!({
            "binding_id": "binding-demo",
            "operation": "lookup",
            "payload": {"id": "one"},
            "idempotency_key": "lookup-replaced",
        }),
    )
    .await;
    assert_eq!(
        denied.status,
        ExecutionStatus::ResultUnknown,
        "replaced fixture did not fail closed: {:?}",
        denied.error
    );
    assert_eq!(denied.output["dispatch_rejected"], true);
    assert_denied(&denied, "connector_fixture_hash_mismatch");
    assert!(denied.output.get("receipt").is_none());

    let listed = registry_list(&host, &root, session).await;
    assert_eq!(
        listed["bindings"]["binding-demo"]["revision"], 1,
        "a refused dispatch must not advance the binding revision"
    );
    assert!(listed["unresolved"].as_array().unwrap().is_empty());

    let _ = fs::remove_dir_all(&root);
}
