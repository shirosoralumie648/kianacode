//! INT-22 daemon boundary fixtures: an Unknown connector result is quarantined, never recorded as
//! a failure and never retried automatically.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding and no network transport.  The deny paths run first:
//! wire-supplied reconciliation material is refused before any fact, and drifted manual evidence
//! cannot close a case.  The success path proves that an explicit reconcile appends a successor
//! fact, that the original Unknown event is byte-identical afterwards, and that the safe/forbidden
//! actions of the case reach the Human Inbox projection.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, AccountBinding, ConnectorDefinition, ConnectorEffect,
    ConnectorOperation, ConnectorReconciliationCase, ConnectorReconciliationState,
    ConnectorReconciliationSuccessor, RuntimeEvent, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND,
    CONNECTOR_STREAM,
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

const FIXTURE_PATH: &str = "fixtures/connector-reconciliation.json";
const BINDING_ID: &str = "binding-reconcile";
const CONNECTOR_ID: &str = "connector-reconcile";
const ACCOUNT_ID: &str = "account-reconcile";
const OPERATION: &str = "poll";

/// Reconciliation material that must never be accepted from the wire.  The case, the successor,
/// the Human Inbox item and every derived verdict are server-owned.
const WIRE_FORGED_RECONCILIATION_KEYS: &[&str] = &[
    "automatic_retry_allowed",
    "effect_observation",
    "human_inbox_item",
    "observed_outcome",
    "receipt",
    "reconciled",
    "reconciliation_case",
    "reconciliation_required",
    "reconciliation_state",
    "reconciliation_successor",
    "replaces_original_fact",
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
        "kiana-int22-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-reconcile".to_owned(),
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
                    "payload": {"id": "unknown"},
                    "outcome": "unknown",
                    "receipt_id": "receipt-unknown",
                    "result": {"detail": "provider outcome unknown"},
                },
                {
                    "payload": {"id": "known"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-known",
                    "result": {"value": "ok"},
                },
            ],
        },
    }))
    .unwrap()
}

fn write_project_file(root: &Path, path: &str, bytes: &[u8]) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, bytes).unwrap();
}

fn write_fixture(root: &Path, bytes: &[u8]) {
    write_project_file(root, FIXTURE_PATH, bytes);
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
        "reason": "INT-22 Unknown quarantine boundary behaviour",
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

fn reconcile_arguments(
    invocation_event_id: &str,
    receipt_path: &str,
    receipt_sha256: &str,
    expected_registry_version: u64,
    key: &str,
) -> Value {
    json!({
        "action": "reconcile",
        "invocation_event_id": invocation_event_id,
        "receipt_path": receipt_path,
        "receipt_sha256": receipt_sha256,
        "expected_registry_version": expected_registry_version,
        "reason": "INT-22 explicit reconciliation evidence",
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
    let digest = connector_fixture_sha256(&bytes);
    write_fixture(root, &bytes);
    let pending = command(
        host,
        trusted_metadata(root, session),
        "connector.manage",
        bind_arguments(&digest, 0, "bind-int22"),
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

async fn connector_event_count(host: &DaemonHost, kind: &str) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

/// Only a reconciliation successor may carry an append/replace claim, and that claim is always
/// `replaces_original_fact = false`: an unrelated fact (or the original Unknown invocation) can
/// never declare that it rewrote another fact.
fn assert_no_replacement_claim(value: &Value, context: &str) {
    match value {
        Value::Object(map) => {
            assert!(
                !map.contains_key("replaces_original_fact"),
                "{context} must not carry a replacement claim"
            );
            for (key, nested) in map {
                assert_no_replacement_claim(nested, &format!("{context}.{key}"));
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                assert_no_replacement_claim(item, &format!("{context}[{index}]"));
            }
        }
        _ => {}
    }
}

/// Invoke the Unknown payload and return the response plus the durable invocation event.
async fn invoke_unknown(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    key: &str,
) -> (ResponseEnvelope, RuntimeEvent) {
    let invoked = command(
        host,
        trusted_metadata(root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "unknown"}), key),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::ResultUnknown,
        "an unknown provider outcome must stay result_unknown: {:?}",
        invoked.error
    );
    assert_eq!(invoked.output["receipt"]["outcome"], "unknown");
    let event_id = invoked.output["event_id"]
        .as_str()
        .expect("event id")
        .to_owned();
    let event = connector_stream_events(host)
        .await
        .into_iter()
        .find(|event| event.event_id.to_string() == event_id)
        .expect("invocation fact");
    (invoked, event)
}

#[tokio::test]
async fn an_unknown_outcome_is_quarantined_with_a_human_inbox_item_and_no_automatic_retry() {
    let root = project_dir("quarantine");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int22-quarantine";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    assert_eq!(connector_stream_events(&host).await.len(), 1);

    let (invoked, invocation_event) = invoke_unknown(&host, &root, session, "quarantine-1").await;
    assert_eq!(invoked.output["replayed"], false);
    assert_eq!(invoked.output["external_effect_performed"], false);
    assert_eq!(
        invoked.output["dispatch_lifecycle"]["stage"], "unknown",
        "an Unknown receipt is a terminal, non-committed dispatch stage"
    );

    // The Unknown outcome is quarantined, not failed: the output carries the server-owned case
    // verdict and a Human Inbox item whose forbidden actions include every retry shortcut.
    assert_eq!(invoked.output["reconciliation_required"], true);
    assert_eq!(invoked.output["automatic_retry_allowed"], false);
    assert_eq!(invoked.output["reconciliation_state"], "pending");
    assert_eq!(invoked.output["human_inbox_item"]["state"], "pending");
    assert_eq!(invoked.output["human_inbox_item"]["attempt"], 1);
    assert_eq!(invoked.output["human_inbox_item"]["binding_id"], BINDING_ID);
    assert_eq!(invoked.output["human_inbox_item"]["operation"], OPERATION);
    assert_eq!(
        invoked.output["human_inbox_item"]["safe_actions"],
        json!(["provider_query", "submit_manual_evidence", "human_review"])
    );
    assert_eq!(
        invoked.output["human_inbox_item"]["forbidden_actions"],
        json!([
            "automatic_retry",
            "reuse_idempotency_key",
            "replace_original_receipt"
        ])
    );

    // The durable fact keeps exactly one attempt and reaches the terminal Unknown stage; no
    // reconciliation fact exists yet and no retry material is persisted.
    let events = connector_stream_events(&host).await;
    assert_eq!(events.len(), 6, "binding + four lifecycle facts + invoked");
    assert_eq!(
        connector_event_count(&host, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND).await,
        4
    );
    assert_eq!(
        connector_event_count(&host, "connector.reconciled").await,
        0
    );
    let lifecycle = &invocation_event.data["dispatch_lifecycle"];
    assert_eq!(lifecycle["stage"], "unknown");
    assert_eq!(lifecycle["attempt"], 1);
    assert!(lifecycle["reason"].as_str().is_some());
    for event in &events {
        assert!(
            !event.kind.contains("retry"),
            "connector stream must not contain a retry fact: {}",
            event.kind
        );
        assert_no_replacement_claim(&event.data, &format!("durable fact {}", event.kind));
    }

    // The registry list projects the same pending case and inbox item for the unresolved
    // invocation.
    let listed = registry_list(&host, &root, session).await;
    let unresolved = listed["unresolved"].as_array().expect("unresolved array");
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0]["event_id"], json!(invocation_event.event_id));
    assert_eq!(unresolved[0]["reconciliation_required"], true);
    assert_eq!(unresolved[0]["automatic_retry_allowed"], false);
    assert_eq!(unresolved[0]["reconciliation_state"], "pending");
    assert_eq!(unresolved[0]["reconciliation_case"]["state"], "pending");
    assert_eq!(unresolved[0]["human_inbox_item"]["state"], "pending");
    assert_eq!(
        unresolved[0]["human_inbox_item"]["forbidden_actions"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert!(unresolved[0]["reconciliation_case_digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    let registry_version = listed["registry_version"].as_u64().unwrap();

    // Replaying the same command returns the original Unknown receipt and adds no fact: an
    // Unknown result is never silently re-dispatched or turned into a second attempt.
    let replayed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "unknown"}), "quarantine-1"),
    )
    .await;
    assert_eq!(replayed.status, ExecutionStatus::ResultUnknown);
    assert_eq!(replayed.output["replayed"], true);
    assert_eq!(replayed.output["event_id"], invoked.output["event_id"]);
    assert_eq!(replayed.output["receipt"], invoked.output["receipt"]);
    assert_eq!(connector_stream_events(&host).await.len(), 6);
    assert_eq!(
        connector_event_count(&host, CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND).await,
        4
    );

    // A *new* explicit invocation is a new operator decision, not an automatic retry; the first
    // quarantine stays open until it is reconciled.
    let explicit = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "unknown"}), "quarantine-2"),
    )
    .await;
    assert_eq!(explicit.status, ExecutionStatus::ResultUnknown);
    assert_eq!(explicit.output["replayed"], false);
    assert_ne!(explicit.output["event_id"], invoked.output["event_id"]);
    let listed = registry_list(&host, &root, session).await;
    assert_eq!(listed["unresolved"].as_array().unwrap().len(), 2);
    assert!(listed["registry_version"].as_u64().unwrap() > registry_version);

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn explicit_reconcile_appends_a_successor_fact_and_keeps_the_original_event() {
    let root = project_dir("reconcile");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int22-reconcile";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;

    let (invoked, original_event) = invoke_unknown(&host, &root, session, "reconcile-1").await;
    let invocation_event_id = original_event.event_id.to_string();
    let original_data = original_event.data.clone();
    let original_stream_version = original_event.stream_version;
    let registry_version = registry_list(&host, &root, session).await["registry_version"]
        .as_u64()
        .unwrap();
    assert_eq!(connector_stream_events(&host).await.len(), 6);

    // Deny first: manual evidence whose idempotency key belongs to another attempt cannot close
    // the case, and the refusal appends no fact.
    let mut drifted = invoked.output["receipt"].clone();
    drifted["outcome"] = json!("succeeded");
    drifted["provider_receipt_id"] = json!("receipt-manual-drifted");
    drifted["idempotency_key"] = json!("some-other-key");
    let drifted_bytes = serde_json::to_vec(&drifted).unwrap();
    write_project_file(&root, "receipts/drifted.json", &drifted_bytes);
    let denied = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        reconcile_arguments(
            &invocation_event_id,
            "receipts/drifted.json",
            &connector_fixture_sha256(&drifted_bytes),
            registry_version,
            "reconcile-drifted",
        ),
    )
    .await;
    let denied = approve(&host, trusted_metadata(&root, session), &denied).await;
    assert_denied(&denied, "connector_reconciliation_binding_mismatch");
    assert_eq!(connector_stream_events(&host).await.len(), 6);
    assert_eq!(
        connector_event_count(&host, "connector.reconciled").await,
        0
    );

    // A committed reconcile needs the same invocation/attempt/binding/account/operation/key and
    // payload identity as the original Unknown receipt.
    let mut manual = invoked.output["receipt"].clone();
    manual["outcome"] = json!("succeeded");
    manual["provider_receipt_id"] = json!("receipt-manual-int22");
    let manual_bytes = serde_json::to_vec(&manual).unwrap();
    write_project_file(&root, "receipts/manual.json", &manual_bytes);
    let manual_sha256 = connector_fixture_sha256(&manual_bytes);

    let staged = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        reconcile_arguments(
            &invocation_event_id,
            "receipts/manual.json",
            &manual_sha256,
            registry_version,
            "reconcile-1",
        ),
    )
    .await;
    let reconciled = approve(&host, trusted_metadata(&root, session), &staged).await;
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
        "receipt-manual-int22"
    );
    assert_eq!(
        reconciled.output["reconciliation_case"]["state"],
        "reconciled"
    );
    assert_eq!(reconciled.output["human_inbox_item"]["state"], "reconciled");
    assert_eq!(
        reconciled.output["human_inbox_item"]["observed_outcome"],
        "succeeded"
    );
    assert_eq!(
        reconciled.output["human_inbox_item"]["forbidden_actions"],
        json!([
            "automatic_retry",
            "reuse_idempotency_key",
            "replace_original_receipt"
        ])
    );
    // The successor is append-only by contract: it never claims to rewrite the original fact.
    assert_eq!(
        reconciled.output["reconciliation_successor"]["appends_new_fact"],
        true
    );
    assert_eq!(
        reconciled.output["reconciliation_successor"]["replaces_original_fact"],
        false
    );
    assert_eq!(
        reconciled.output["reconciliation_successor"]["observed_outcome"],
        "succeeded"
    );
    let successor: ConnectorReconciliationSuccessor =
        serde_json::from_value(reconciled.output["reconciliation_successor"].clone())
            .expect("successor");
    let case: ConnectorReconciliationCase =
        serde_json::from_value(reconciled.output["reconciliation_case"].clone())
            .expect("reconciliation case");
    assert!(successor.validate().is_ok());
    successor
        .validate_for_case(&case)
        .expect("successor matches its case");
    assert_eq!(
        successor.original_invocation_id.to_string(),
        original_data["effect_observation"]["invocation_id"]
            .as_str()
            .unwrap()
    );

    // Exactly one successor fact was appended, and the original Unknown event is byte-identical.
    let events = connector_stream_events(&host).await;
    assert_eq!(events.len(), 7, "one appended connector.reconciled fact");
    assert_eq!(
        connector_event_count(&host, "connector.reconciled").await,
        1
    );
    let original_after = events
        .iter()
        .find(|event| event.event_id.to_string() == invocation_event_id)
        .expect("original event still present");
    assert_eq!(original_after.data, original_data);
    assert_eq!(original_after.stream_version, original_stream_version);
    assert_eq!(original_after.kind, "connector.invoked");
    assert_eq!(original_after.data["receipt"]["outcome"], "unknown");
    assert_no_replacement_claim(&original_after.data, "the original Unknown invocation fact");

    let reconciled_event = events
        .iter()
        .find(|event| event.kind == "connector.reconciled")
        .expect("reconciled fact");
    assert_eq!(
        reconciled_event.data["invocation_event_id"],
        invocation_event_id
    );
    let fact_case: ConnectorReconciliationCase =
        serde_json::from_value(reconciled_event.data["reconciliation_case"].clone())
            .expect("case in fact");
    let fact_successor: ConnectorReconciliationSuccessor =
        serde_json::from_value(reconciled_event.data["reconciliation_successor"].clone())
            .expect("successor in fact");
    fact_successor
        .validate_for_case(&fact_case)
        .expect("the committed successor matches the committed case");
    assert_eq!(fact_case.state, ConnectorReconciliationState::Reconciled);
    assert_eq!(
        fact_case.original_receipt_digest,
        case.original_receipt_digest
    );
    assert_eq!(fact_successor.replaces_original_fact, false);
    assert_eq!(
        fact_successor.original_receipt_digest,
        json_digest_of(&original_after.data["receipt"])
    );

    // The case is closed: the unresolved projection is empty and the binding stays listed.
    let listed = registry_list(&host, &root, session).await;
    assert!(listed["unresolved"].as_array().unwrap().is_empty());
    assert_eq!(
        listed["bindings"][BINDING_ID]["binding"]["binding_id"],
        BINDING_ID
    );

    // Replaying the same reconcile command returns the committed successor without a second fact,
    // and a distinct reconcile attempt is refused because the case is no longer pending.
    let replayed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        reconcile_arguments(
            &invocation_event_id,
            "receipts/manual.json",
            &manual_sha256,
            registry_version,
            "reconcile-1",
        ),
    )
    .await;
    let replayed = approve(&host, trusted_metadata(&root, session), &replayed).await;
    assert_eq!(
        replayed.status,
        ExecutionStatus::Completed,
        "reconcile replay failed: {:?}",
        replayed.error
    );
    assert_eq!(replayed.output["replayed"], true);
    assert_eq!(replayed.output["event_id"], reconciled.output["event_id"]);
    assert_eq!(connector_stream_events(&host).await.len(), 7);

    let after_close = registry_list(&host, &root, session).await["registry_version"]
        .as_u64()
        .unwrap();
    let second = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        reconcile_arguments(
            &invocation_event_id,
            "receipts/manual.json",
            &manual_sha256,
            after_close,
            "reconcile-2",
        ),
    )
    .await;
    let second = approve(&host, trusted_metadata(&root, session), &second).await;
    assert_denied(&second, "connector_reconciliation_not_pending");
    assert_eq!(connector_stream_events(&host).await.len(), 7);

    // A succeeded invocation is not a quarantine input either.
    let succeeded = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "known"}), "known-1"),
    )
    .await;
    assert_eq!(succeeded.status, ExecutionStatus::Completed);
    assert_eq!(succeeded.output["receipt"]["outcome"], "succeeded");
    assert!(succeeded.output.get("reconciliation_required").is_none());
    let succeeded_version = registry_list(&host, &root, session).await["registry_version"]
        .as_u64()
        .unwrap();
    let not_unknown = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        reconcile_arguments(
            succeeded.output["event_id"].as_str().unwrap(),
            "receipts/manual.json",
            &manual_sha256,
            succeeded_version,
            "reconcile-3",
        ),
    )
    .await;
    let not_unknown = approve(&host, trusted_metadata(&root, session), &not_unknown).await;
    assert_denied(&not_unknown, "connector_reconciliation_not_pending");
    assert_eq!(
        connector_event_count(&host, "connector.reconciled").await,
        1
    );

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn wire_supplied_reconciliation_material_is_refused_before_any_fact() {
    let root = project_dir("wire");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int22-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    let (invoked, original_event) = invoke_unknown(&host, &root, session, "wire-1").await;
    let stream_before = connector_stream_events(&host).await;
    assert_eq!(stream_before.len(), 6);
    let registry_version = registry_list(&host, &root, session).await["registry_version"]
        .as_u64()
        .unwrap();

    // The reconcile route accepts only invocation/receipt/reason/idempotency keys, so a
    // wire-supplied case, successor, inbox item or verdict is refused before any fact.
    for key in WIRE_FORGED_RECONCILIATION_KEYS {
        let mut arguments = reconcile_arguments(
            &original_event.event_id.to_string(),
            "receipts/manual.json",
            &"0".repeat(64),
            registry_version,
            &format!("wire-{key}"),
        );
        arguments[key] = json!({"schema": format!("forged.{key}.v1"), "value": true});
        let denied = command(
            &host,
            trusted_metadata(&root, session),
            "connector.manage",
            arguments,
        )
        .await;
        assert_denied(&denied, "connector_arguments_invalid");
        assert_eq!(
            connector_stream_events(&host).await.len(),
            6,
            "a wire-supplied {key} must not commit a connector fact"
        );
        assert_eq!(
            registry_list(&host, &root, session).await["registry_version"]
                .as_u64()
                .unwrap(),
            registry_version,
            "a wire-supplied {key} must not advance the registry version"
        );
    }

    // The invoke route keeps its own whitelist: quarantine verdicts cannot be smuggled into a
    // dispatch either.
    for key in [
        "automatic_retry_allowed",
        "human_inbox_item",
        "reconciliation_case",
        "reconciliation_required",
        "reconciliation_successor",
    ] {
        let mut arguments = invoke_arguments(json!({"id": "known"}), &format!("wire-invoke-{key}"));
        arguments[key] = json!(true);
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
            1,
            "a wire-supplied {key} must not reach the adapter"
        );
    }
    assert_eq!(connector_stream_events(&host).await.len(), 6);

    // The single spine still works: the server-owned case closes the same quarantine exactly once.
    let mut manual = invoked.output["receipt"].clone();
    manual["outcome"] = json!("succeeded");
    manual["provider_receipt_id"] = json!("receipt-manual-wire");
    let manual_bytes = serde_json::to_vec(&manual).unwrap();
    write_project_file(&root, "receipts/manual.json", &manual_bytes);
    let staged = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        reconcile_arguments(
            &original_event.event_id.to_string(),
            "receipts/manual.json",
            &connector_fixture_sha256(&manual_bytes),
            registry_version,
            "wire-legitimate",
        ),
    )
    .await;
    let reconciled = approve(&host, trusted_metadata(&root, session), &staged).await;
    assert_eq!(
        reconciled.status,
        ExecutionStatus::Completed,
        "legitimate reconcile failed: {:?}",
        reconciled.error
    );
    assert_eq!(reconciled.output["replayed"], false);
    assert_eq!(
        reconciled.output["reconciliation_successor"]["appends_new_fact"],
        true
    );
    assert_eq!(connector_stream_events(&host).await.len(), 7);
    assert_eq!(
        connector_event_count(&host, "connector.reconciled").await,
        1
    );
    assert!(registry_list(&host, &root, session).await["unresolved"]
        .as_array()
        .unwrap()
        .is_empty());

    let _ = fs::remove_dir_all(&root);
}

/// Digest of a receipt as the reconciliation contract computes it.
fn json_digest_of(value: &Value) -> String {
    kiana_domain::json_digest(value)
}
