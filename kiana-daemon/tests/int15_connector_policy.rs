//! INT-15 daemon behaviour fixture: connector operation risk → policy/gate/approval mapping.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding and the existing ControlPlane approval journal. They
//! prove the R0--R4 mapping is enforced before any dispatch: R0/R1 read-only calls are allowed
//! without an approval, the R2 data-grant and R4 default-deny classes never reach the adapter, and
//! R3 pauses for exactly one final-payload approval that is consumed once. No connector network
//! transport, no second execution loop and no model loop are involved.

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

const FIXTURE_PATH: &str = "fixtures/connector-policy.json";
const BINDING_ID: &str = "binding-policy";
const CONNECTOR_ID: &str = "connector-policy";
const ACCOUNT_ID: &str = "account-policy";

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
        "kiana-int15-{label}-{}-{nanos}",
        std::process::id()
    ))
}

/// One server-owned operation per risk class: R0/R1 read-only, R2 data grant, R3 once approval and
/// R4 default deny. The definition is the only place operation names and effects are declared.
fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-policy".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([
            (
                "inspect_schema".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
            (
                "lookup".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
            (
                "export_records".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::ReadOnly,
                    required_scope: "read".to_owned(),
                    data_classes: BTreeSet::from(["internal".to_owned()]),
                },
            ),
            (
                "send_message".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::Write,
                    required_scope: "write".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
            (
                "delete_account".to_owned(),
                ConnectorOperation {
                    effect: ConnectorEffect::Write,
                    required_scope: "write".to_owned(),
                    data_classes: BTreeSet::new(),
                },
            ),
        ]),
        rate_limit_per_minute: 20,
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
        write_scopes: BTreeSet::from(["write".to_owned()]),
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
            "inspect_schema": [{
                "payload": {},
                "outcome": "succeeded",
                "receipt_id": "receipt-inspect",
                "result": {"value": "schema"},
            }],
            "lookup": [{
                "payload": {"id": "one"},
                "outcome": "succeeded",
                "receipt_id": "receipt-lookup",
                "result": {"value": "fixture"},
            }],
            "export_records": [{
                "payload": {"scope": "internal"},
                "outcome": "succeeded",
                "receipt_id": "receipt-export",
                "result": {"rows": []},
            }],
            "send_message": [
                {
                    "payload": {"body": "final-1", "to": "recipient"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-send-1",
                    "result": {"delivered": false},
                },
                {
                    "payload": {"body": "final-2", "to": "recipient"},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-send-2",
                    "result": {"delivered": false},
                },
            ],
            "delete_account": [{
                "payload": {"id": "account-1"},
                "outcome": "succeeded",
                "receipt_id": "receipt-delete",
                "result": {},
            }],
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
        "reason": "INT-15 risk to policy/gate/approval mapping",
        "idempotency_key": key,
    })
}

fn invoke_arguments(operation: &str, payload: Value, key: &str) -> Value {
    json!({
        "binding_id": BINDING_ID,
        "operation": operation,
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

async fn decide(
    host: &DaemonHost,
    metadata: RequestMetadata,
    challenge: &ApprovalChallenge,
    request_hash: String,
    nonce: String,
) -> ResponseEnvelope {
    host.handle(RequestEnvelope::approval_decision_with_proof(
        metadata,
        challenge.approval_id.clone(),
        ApprovalDecision::Approve,
        Some(request_hash),
        Some(nonce),
    ))
    .await
}

fn challenge(response: &ResponseEnvelope) -> ApprovalChallenge {
    assert_eq!(
        response.status,
        ExecutionStatus::AwaitingApproval,
        "command was not staged for approval: {:?}",
        response.error
    );
    serde_json::from_value(response.output["approval"].clone()).expect("approval challenge")
}

async fn bind(host: &DaemonHost, root: &Path, session: &str) {
    let bytes = fixture_bytes();
    let digest = connector_fixture_sha256(&bytes);
    write_fixture(root, &bytes);

    // Binding is an operator mutation and is staged with the external-side-effect risk. It is the
    // only operation in these fixtures that is approved outside the R3 invoke path.
    let pending = command(
        host,
        trusted_metadata(root, session),
        "connector.manage",
        bind_arguments(&digest, 0, "bind-int15"),
    )
    .await;
    let staged = challenge(&pending);
    assert_eq!(staged.risk, RiskLevel::ExternalSideEffect);
    let bound = decide(
        host,
        trusted_metadata(root, session),
        &staged,
        staged.request_hash.clone(),
        staged.nonce.clone(),
    )
    .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "approved bind failed: {:?}",
        bound.error
    );
    assert_eq!(bound.output["state"], "active");
    assert_eq!(bound.output["health"], "local_fixture_parsed");
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

async fn event_count(host: &DaemonHost, kind: &str) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.kind == kind)
        .count()
}

/// The two event counters that prove "zero dispatch" for a denied command: no connector fact was
/// appended and no execution result was committed. The bind result is already included in the
/// baseline, so a denial must leave both counters unchanged.
async fn effect_counts(host: &DaemonHost) -> (usize, usize) {
    (
        event_count(host, "connector.invoked").await,
        event_count(host, "execution.result_committed").await,
    )
}

#[tokio::test]
async fn read_only_risk_is_allowed_and_denials_never_dispatch() {
    let root = project_dir("read-only-and-denials");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int15-deny";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;

    // R0/R1 read-only operations are allowed on the single spine without any approval and still
    // produce an explicit local receipt with no external effect.
    for (operation, payload, key) in [
        ("inspect_schema", json!({}), "inspect-1"),
        ("lookup", json!({"id": "one"}), "lookup-1"),
    ] {
        let allowed = command(
            &host,
            trusted_metadata(&root, session),
            "connector.invoke",
            invoke_arguments(operation, payload, key),
        )
        .await;
        assert_eq!(allowed.status, ExecutionStatus::Completed, "{allowed:?}");
        assert_eq!(allowed.output["receipt"]["operation"], json!(operation));
        assert_eq!(allowed.output["receipt"]["outcome"], "succeeded");
        assert_eq!(allowed.output["receipt"]["source"], "local_fixture");
        assert_eq!(allowed.output["external_effect_performed"], false);
        assert_eq!(allowed.output["proof_source"], "local_fixture");
    }
    let allowed_counts = effect_counts(&host).await;
    assert_eq!(allowed_counts.0, 2, "read-only invokes must still dispatch");

    // R2 requires a server-issued data grant. Without one the same read-shaped operation is
    // denied before the Broker, and a caller cannot smuggle the grant through the wire: the
    // unknown argument is refused as an invalid connector command with zero effect.
    let r2 = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments("export_records", json!({"scope": "internal"}), "export-1"),
    )
    .await;
    assert_eq!(r2.status, ExecutionStatus::Denied, "{r2:?}");
    assert!(
        r2.error
            .as_deref()
            .unwrap_or_default()
            .contains("connector_data_grant_required"),
        "R2 must fail closed with the data-grant requirement: {:?}",
        r2.error
    );

    let smuggled = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        json!({
            "binding_id": BINDING_ID,
            "operation": "export_records",
            "payload": {"scope": "internal"},
            "idempotency_key": "export-smuggled",
            "data_grant": {"schema": "kiana.connector-data-grant.v1"},
            "connector_approval": {"schema": "kiana.connector-once-approval.v1"},
        }),
    )
    .await;
    assert_eq!(smuggled.status, ExecutionStatus::Blocked, "{smuggled:?}");
    assert!(
        smuggled
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("connector_arguments_invalid"),
        "wire-supplied grant/approval must be refused: {:?}",
        smuggled.error
    );

    // R4 is default deny even for a registered, in-scope operation: no wire field can turn it
    // into an approval gate, and every attempted classification is refused outright.
    let r4 = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments("delete_account", json!({"id": "account-1"}), "delete-1"),
    )
    .await;
    assert_eq!(r4.status, ExecutionStatus::Denied, "{r4:?}");
    assert!(
        r4.error
            .as_deref()
            .unwrap_or_default()
            .contains("connector_r4_default_denied"),
        "R4 must stay default denied: {:?}",
        r4.error
    );

    // None of the denied calls produced a connector fact, a committed execution result or a
    // binding change, and no denied operation appears in the connector stream.
    assert_eq!(effect_counts(&host).await, allowed_counts);
    let registry = registry_list(&host, &root, session).await;
    assert_eq!(registry["registry_version"], 1);
    assert_eq!(registry["bindings"][BINDING_ID]["revision"], 1);
    assert!(registry["unresolved"].as_array().unwrap().is_empty());
    let events = host.persisted_events().await.unwrap().unwrap();
    assert!(!events.iter().any(|event| {
        event.kind == "connector.invoked"
            && matches!(
                event.data["receipt"]["operation"].as_str(),
                Some("export_records") | Some("delete_account")
            )
    }));

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn final_payload_approval_is_one_shot_and_payload_bound() {
    let root = project_dir("final-payload-approval");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int15-approval";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    let bind_counts = effect_counts(&host).await;

    let payload_one = json!({"body": "final-1", "to": "recipient"});
    let payload_two = json!({"body": "final-2", "to": "recipient"});
    let digest_one = connector_payload_sha256(&payload_one);
    let digest_two = connector_payload_sha256(&payload_two);
    assert_ne!(digest_one, digest_two);

    // R3 without a final-payload approval pauses instead of dispatching: the request is staged
    // for the operator and no connector fact or committed result exists yet.
    let pending_one = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments("send_message", payload_one.clone(), "send-1"),
    )
    .await;
    let challenge_one = challenge(&pending_one);
    assert_eq!(challenge_one.risk, RiskLevel::ExternalSideEffect);
    assert!(
        challenge_one
            .reason
            .contains("connector_final_payload_approval_required"),
        "R3 must stage the final-payload approval: {}",
        challenge_one.reason
    );
    assert_eq!(effect_counts(&host).await, bind_counts);

    // A different final payload gets a different approval identity and request hash, so one
    // approval can never be replayed for another payload.
    let pending_two = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments("send_message", payload_two, "send-2"),
    )
    .await;
    let challenge_two = challenge(&pending_two);
    assert_ne!(challenge_two.approval_id, challenge_one.approval_id);
    assert_ne!(challenge_two.request_hash, challenge_one.request_hash);

    // Deny first: an approval without the exact challenge proof is refused and leaves zero
    // effect, while the still-pending approval can only be decided with the matching material.
    let wrong_hash = decide(
        &host,
        trusted_metadata(&root, session),
        &challenge_two,
        format!("sha256:{}", "b".repeat(64)),
        challenge_two.nonce.clone(),
    )
    .await;
    assert_eq!(
        wrong_hash.status,
        ExecutionStatus::Blocked,
        "{wrong_hash:?}"
    );
    assert!(
        wrong_hash
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("approval_request_hash_mismatch"),
        "a drifted request hash must be refused: {:?}",
        wrong_hash.error
    );

    let wrong_nonce = decide(
        &host,
        trusted_metadata(&root, session),
        &challenge_two,
        challenge_two.request_hash.clone(),
        "not-the-issued-nonce".to_owned(),
    )
    .await;
    assert_eq!(
        wrong_nonce.status,
        ExecutionStatus::Blocked,
        "{wrong_nonce:?}"
    );
    assert!(
        wrong_nonce
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("approval_nonce_mismatch"),
        "a stale nonce must be refused: {:?}",
        wrong_nonce.error
    );
    assert_eq!(effect_counts(&host).await, bind_counts);

    // Only the exact final payload that was staged is dispatched once the operator approves.
    let approved = decide(
        &host,
        trusted_metadata(&root, session),
        &challenge_one,
        challenge_one.request_hash.clone(),
        challenge_one.nonce.clone(),
    )
    .await;
    assert_eq!(
        approved.status,
        ExecutionStatus::Completed,
        "approved R3 invoke failed: {:?}",
        approved.error
    );
    assert_eq!(approved.output["receipt"]["operation"], "send_message");
    assert_eq!(approved.output["receipt"]["outcome"], "succeeded");
    assert_eq!(
        approved.output["receipt"]["final_payload_sha256"],
        json!(digest_one)
    );
    assert_eq!(approved.output["external_effect_performed"], false);
    assert_eq!(approved.output["replayed"], false);
    assert_eq!(effect_counts(&host).await.0, bind_counts.0 + 1);

    // Re-deciding the same approval after it was consumed is refused and dispatches nothing: the
    // one-shot approval cannot authorize a second attempt.
    let replayed = decide(
        &host,
        trusted_metadata(&root, session),
        &challenge_one,
        challenge_one.request_hash.clone(),
        challenge_one.nonce.clone(),
    )
    .await;
    assert_eq!(replayed.status, ExecutionStatus::Blocked, "{replayed:?}");
    assert!(
        replayed
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("approval_not_active"),
        "a consumed approval must not reactivate: {:?}",
        replayed.error
    );
    let after_replay = effect_counts(&host).await;
    assert_eq!(
        after_replay.0,
        bind_counts.0 + 1,
        "a decided approval must not authorize a second dispatch"
    );

    // The same payload with a new idempotency key needs its own fresh approval instead of reusing
    // the consumed one, and it remains undispatched while it waits.
    let pending_again = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments("send_message", payload_one, "send-3"),
    )
    .await;
    let challenge_again = challenge(&pending_again);
    assert_ne!(challenge_again.approval_id, challenge_one.approval_id);
    assert_eq!(effect_counts(&host).await, after_replay);

    let registry = registry_list(&host, &root, session).await;
    assert_eq!(registry["bindings"][BINDING_ID]["revision"], 1);
    assert!(registry["unresolved"].as_array().unwrap().is_empty());

    let _ = fs::remove_dir_all(&root);
}
