//! INT-18 daemon behaviour fixture: effect-time permit fencing at the real adapter edge.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with a
//! real, hash-pinned `local_fixture` binding.  The endpoint is server-owned: every attempt to
//! smuggle the INT-16 reservation/permit envelope, the INT-18 effect permit, its fence epochs or a
//! fence snapshot through the wire is refused by the ControlPlane normalization with zero
//! connector facts, and a wire-supplied envelope can never replace the adapter-effect identity of
//! the single spine.  A legitimate compatibility command still reaches the fixture adapter exactly
//! once, and no permit/fence material is persisted or echoed with the connector fact.
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

const FIXTURE_PATH: &str = "fixtures/connector-effect-permit.json";
const BINDING_ID: &str = "binding-effect-permit";
const CONNECTOR_ID: &str = "connector-effect-permit";
const ACCOUNT_ID: &str = "account-effect-permit";
const OPERATION: &str = "lookup";

/// INT-18 server-owned permit/fence material that must never be accepted from, or echoed to, the
/// wire and never persisted with a connector fact.
const EFFECT_PERMIT_MATERIAL: &[&str] = &[
    "connector_effect_permit",
    "connector_effect_fence",
    "connector_configuration_epoch",
    "connector_policy_epoch",
    "connector_credential_epoch",
    "kiana.connector-effect-permit.v1",
    "kiana.connector-effect-fence.v1",
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
        "kiana-int18-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-effect-permit".to_owned(),
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
        "reason": "INT-18 effect-time permit behaviour",
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
        bind_arguments(&digest, 0, "bind-int18"),
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

/// Every connector-stream event, including the dispatch lifecycle chain.  A denied command must
/// leave this length unchanged: the stream is the durable connector fact history.
async fn connector_stream_len(host: &DaemonHost) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .iter()
        .filter(|event| event.aggregate_type.as_deref() == Some(CONNECTOR_STREAM))
        .count()
}

/// The connector fact is the adapter-effect evidence.  A denied command must never append one.
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
async fn wire_supplied_effect_permit_is_refused_before_any_adapter_effect() {
    let root = project_dir("wire-effect-permit");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int18-wire";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;
    let baseline_stream_len = connector_stream_len(&host).await;
    let baseline_version = registry_list(&host, &root, session).await["registry_version"].clone();
    assert_eq!(baseline_stream_len, 1);
    assert_eq!(baseline_version, json!(1));

    // Every server-owned permit/fence object and epoch is rejected at the ControlPlane argument
    // boundary, before the Broker or the fixture adapter can be reached.
    let mut forged: Vec<Value> = Vec::new();
    let mut effect_only = invoke_arguments(json!({"id": "one"}), "wire-effect-only");
    effect_only["connector_effect_permit"] = json!({"schema": "kiana.connector-effect-permit.v1"});
    forged.push(effect_only);
    let mut full_envelope = invoke_arguments(json!({"id": "one"}), "wire-full-envelope");
    full_envelope["connector_reservation"] =
        json!({"schema": "kiana.connector-invocation-reservation.v1"});
    full_envelope["connector_permit"] = json!({"schema": "kiana.connector-invocation-permit.v1"});
    full_envelope["connector_effect_permit"] =
        json!({"schema": "kiana.connector-effect-permit.v1"});
    forged.push(full_envelope);
    let mut fence_only = invoke_arguments(json!({"id": "one"}), "wire-fence-only");
    fence_only["connector_effect_fence"] = json!({"schema": "kiana.connector-effect-fence.v1"});
    forged.push(fence_only);
    for key in [
        "connector_configuration_epoch",
        "connector_policy_epoch",
        "connector_credential_epoch",
    ] {
        let mut epochs = invoke_arguments(json!({"id": "one"}), "wire-epoch-only");
        epochs[key] = json!(7);
        forged.push(epochs);
    }

    for arguments in forged {
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
            "a wire-supplied permit/fence envelope must not reach the adapter"
        );
        assert_eq!(connector_stream_len(&host).await, baseline_stream_len);
        assert_eq!(
            registry_list(&host, &root, session).await["registry_version"],
            baseline_version
        );
    }

    // The compatibility command still reaches the fixture adapter exactly once: the refuse-first
    // wire fence does not disable the single spine.
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
    assert_eq!(invoked.output["external_effect_performed"], false);
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);
    assert!(connector_stream_len(&host).await > baseline_stream_len);
    let receipts = invoked_receipts(&host).await;
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        receipts[0]["final_payload_sha256"].as_str(),
        Some(connector_payload_sha256(&json!({"id": "one"})).as_str())
    );

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn effect_permit_and_fence_material_is_never_persisted_or_echoed() {
    let root = project_dir("no-permit-material");
    fs::create_dir_all(&root).unwrap();
    let session = "session-int18-material";
    let host = DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
        .expect("daemon host");
    bind(&host, &root, session).await;

    let invoked = command(
        &host,
        trusted_metadata(&root, session),
        "connector.invoke",
        invoke_arguments(json!({"id": "one"}), "material-1"),
    )
    .await;
    assert_eq!(
        invoked.status,
        ExecutionStatus::Completed,
        "invoke failed: {:?}",
        invoked.error
    );
    assert_eq!(connector_event_count(&host, "connector.invoked").await, 1);

    // The model-visible result exposes the receipt, the server-owned command digest and the
    // dispatch lifecycle, never the server-owned permit/fence identity.
    let output = invoked.output.to_string();
    let command_digest = invoked.output["dispatch_lifecycle"]["command_digest"]
        .as_str()
        .unwrap_or_default();
    assert!(
        command_digest.starts_with("sha256:") && command_digest.len() == 71,
        "the connector fact must carry the server-owned command digest"
    );
    for forbidden in EFFECT_PERMIT_MATERIAL {
        assert!(
            !output.contains(forbidden),
            "invoke output must not echo {forbidden}"
        );
    }

    // No persisted connector fact, in any stage of the dispatch lifecycle, carries that material.
    let events = host
        .persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store");
    let persisted = serde_json::to_string(
        &events
            .iter()
            .filter(|event| event.aggregate_type.as_deref() == Some(CONNECTOR_STREAM))
            .collect::<Vec<_>>(),
    )
    .expect("connector events encode");
    for forbidden in EFFECT_PERMIT_MATERIAL {
        assert!(
            !persisted.contains(forbidden),
            "persisted connector facts must not retain {forbidden}"
        );
    }

    let _ = fs::remove_dir_all(&root);
}
