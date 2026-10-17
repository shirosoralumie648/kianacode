//! INT-09 daemon behaviour fixture: read-only `connector.health`.
//!
//! These tests drive the production `DaemonHost → ControlPlane → Broker → EventStore` path with
//! a real hash-pinned local fixture.  They prove that a probe is read-only, that the health
//! classes stay distinguishable, that raw provider text cannot enter the fact and that a failed
//! probe never revokes the binding or the credential reference.  No network transport exists in
//! this path; a successful local read remains `connectivity_only`.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    connector_fixture_sha256, AccountBinding, ConnectorDefinition, ConnectorEffect,
    ConnectorOperation,
};
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ExecutionStatus, RequestEnvelope, RequestMetadata,
    ResponseEnvelope,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const FIXTURE_PATH: &str = "fixtures/connector.json";
const BINDING_ID: &str = "binding-demo";

struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

struct UntrustedProject;

impl ProjectTrustAuthority for UntrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(false)
    }
}

fn fixture_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-int09-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn definition(
    operation: &str,
    effect: ConnectorEffect,
    required_scope: &str,
) -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-demo".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            operation.to_owned(),
            ConnectorOperation {
                effect,
                required_scope: required_scope.to_owned(),
                data_classes: BTreeSet::new(),
            },
        )]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    }
}

fn account_binding(
    fixture_sha256: &str,
    read_scopes: BTreeSet<String>,
    write_scopes: BTreeSet<String>,
) -> AccountBinding {
    AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: BINDING_ID.to_owned(),
        connector_id: "connector-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        read_scopes,
        write_scopes,
        fixture_path: FIXTURE_PATH.to_owned(),
        fixture_sha256: fixture_sha256.to_owned(),
        credential_ref: None,
    }
}

fn fixture_bytes(operation: &str) -> Vec<u8> {
    let operations = serde_json::Map::from_iter([(
        operation.to_owned(),
        json!([{
            "payload": {"id": "one"},
            "outcome": "succeeded",
            "receipt_id": "receipt-one",
            "result": {"value": "fixture"},
        }]),
    )]);
    serde_json::to_vec(&json!({
        "schema": "kiana.connector-fixture.v1",
        "connector_id": "connector-demo",
        "account_id": "account-demo",
        "operations": serde_json::Value::Object(operations),
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

async fn bind(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    definition: ConnectorDefinition,
    binding: AccountBinding,
    expected_registry_version: u64,
    key: &str,
) -> ResponseEnvelope {
    let metadata = trusted_metadata(root, session);
    let pending = command(
        host,
        metadata.clone(),
        "connector.manage",
        json!({
            "action": "bind",
            "definition": definition,
            "binding": binding,
            "expected_registry_version": expected_registry_version,
            "reason": "INT-09 health acceptance",
            "idempotency_key": key,
        }),
    )
    .await;
    approve(host, metadata, &pending).await
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
async fn read_only_health_is_connectivity_only_redacted_and_replays_the_original_fact() {
    let root = fixture_dir("health-success");
    fs::create_dir_all(&root).unwrap();
    let bytes = fixture_bytes("lookup");
    let digest = connector_fixture_sha256(&bytes);
    write_fixture(&root, &bytes);
    let session = "session-int09-health";

    let host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject)).unwrap();
    let bound = bind(
        &host,
        &root,
        session,
        definition("lookup", ConnectorEffect::ReadOnly, "read"),
        account_binding(
            &digest,
            BTreeSet::from(["read".to_owned()]),
            BTreeSet::new(),
        ),
        0,
        "bind-health-1",
    )
    .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "bind failed: {:?}",
        bound.error
    );

    let request = RequestEnvelope::command(
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    );
    let first = host.handle(request.clone()).await;
    assert_eq!(
        first.status,
        ExecutionStatus::Completed,
        "health failed: {:?}",
        first.error
    );
    assert_eq!(first.output["schema"], "kiana.connector-health-result.v1");
    assert_eq!(first.output["health"]["status"], "connectivity_only");
    assert_eq!(first.output["health"]["probe_kind"], "read_only");
    assert_eq!(first.output["health"]["binding_id"], BINDING_ID);
    assert_eq!(first.output["health"]["connector_id"], "connector-demo");
    assert_eq!(first.output["health"]["binding_revision"], 1);
    assert!(first.output["health"].get("error_code").is_none());
    assert_eq!(
        first.output["health"]["limitations"],
        json!(["local_fixture_only", "external_provider_not_probed"])
    );
    assert_eq!(first.output["proof_level"], "source");
    assert_eq!(first.output["stale"], false);
    assert_eq!(first.output["replayed"], false);
    assert_eq!(first.output["source_cursor"], 2);
    assert!(first.output["event_id"].is_string());
    assert_eq!(
        first.output["health_projection"]["schema"],
        "kiana.connector-health-projection.v1"
    );
    assert_eq!(
        first.output["health_projection"]["entries"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        first.output["health_projection"]["entries"][0]["health"]["status"],
        "connectivity_only"
    );
    let rendered = first.output.to_string();
    for forbidden in [
        "fixture_path",
        "fixture_sha256",
        "credential_ref",
        "secret_value",
        "Authorization",
    ] {
        assert!(
            !rendered.contains(forbidden),
            "health result leaked {forbidden}: {rendered}"
        );
    }

    // The same server request id is a retry: it returns the original event and does not append a
    // second fact, even though the wall clock and fixture state could have changed.
    let replayed = host.handle(request).await;
    assert_eq!(
        replayed.status,
        ExecutionStatus::Completed,
        "replay failed: {:?}",
        replayed.error
    );
    assert_eq!(replayed.output["replayed"], true);
    assert_eq!(replayed.output["health"], first.output["health"]);
    assert_eq!(replayed.output["event_id"], first.output["event_id"]);
    assert_eq!(
        replayed.output["health_projection"],
        first.output["health_projection"]
    );

    // A new request id is a new observation and appends one more health fact.
    let second = command(
        &host,
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_eq!(
        second.status,
        ExecutionStatus::Completed,
        "second health failed: {:?}",
        second.error
    );
    assert_eq!(second.output["replayed"], false);
    assert_eq!(second.output["source_cursor"], 3);
    assert_ne!(second.output["event_id"], first.output["event_id"]);

    let listed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        json!({"action": "list"}),
    )
    .await;
    assert_eq!(listed.status, ExecutionStatus::Completed);
    assert_eq!(listed.output["registry_version"], 3);
    assert_eq!(
        listed.output["health"]["entries"][0]["source_cursor"], 3,
        "registry projection must keep the latest health fact"
    );

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn health_refuses_destructive_probe_foreign_scope_and_revoked_binding() {
    let root = fixture_dir("health-deny");
    fs::create_dir_all(&root).unwrap();
    let bytes = fixture_bytes("lookup");
    let digest = connector_fixture_sha256(&bytes);
    write_fixture(&root, &bytes);
    let session = "session-int09-deny";
    let host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject)).unwrap();

    // Untrusted projects are rejected before the binding or fixture is resolved. The daemon's
    // ProjectTrustAuthority is authoritative, not the wire bit.
    let untrusted_host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(UntrustedProject)).unwrap();
    let untrusted = command(
        &untrusted_host,
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_denied(&untrusted, "project_untrusted");

    let bound = bind(
        &host,
        &root,
        session,
        definition("lookup", ConnectorEffect::ReadOnly, "read"),
        account_binding(
            &digest,
            BTreeSet::from(["read".to_owned()]),
            BTreeSet::new(),
        ),
        0,
        "bind-health-deny",
    )
    .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "bind failed: {:?}",
        bound.error
    );

    // A missing binding is a stable lookup failure, not a health class.
    let missing = command(
        &host,
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": "binding-missing"}),
    )
    .await;
    assert_denied(&missing, "connector_binding_missing");

    // Destructive probe kinds and caller-supplied authority fields never reach the handler.
    for arguments in [
        json!({"binding_id": BINDING_ID, "probe_kind": "destructive"}),
        json!({"binding_id": BINDING_ID, "operation": "lookup"}),
        json!({"binding_id": BINDING_ID, "risk": "read_only"}),
        json!({"binding_id": BINDING_ID, "binding_snapshot": {"forged": true}}),
    ] {
        let denied = command(
            &host,
            trusted_metadata(&root, session),
            "connector.health",
            arguments,
        )
        .await;
        assert_denied(&denied, "connector_health_arguments_invalid");
    }

    // Replacing the pinned bytes is an unreachable/evidence failure. The binding and the
    // credential reference stay untouched; the fact carries only a bounded redacted code.
    let mut tampered = fixture_bytes("lookup");
    tampered.extend_from_slice(b"tampered");
    write_fixture(&root, &tampered);
    let failed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_eq!(
        failed.status,
        ExecutionStatus::Completed,
        "a probe failure is an observation, not a command failure: {:?}",
        failed.error
    );
    assert_eq!(failed.output["health"]["status"], "endpoint_unreachable");
    assert_eq!(
        failed.output["health"]["error_code"],
        "connector_fixture_hash_mismatch"
    );
    assert!(failed.output["health"]["limitations"]
        .as_array()
        .unwrap()
        .contains(&json!("probe_error_redacted")));
    let listed = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        json!({"action": "list"}),
    )
    .await;
    assert_eq!(listed.status, ExecutionStatus::Completed);
    assert_eq!(
        listed.output["bindings"][BINDING_ID]["status"], "active",
        "a failed probe must not revoke the binding"
    );
    assert_eq!(listed.output["bindings"][BINDING_ID]["revision"], 1);

    // Restoring the pinned bytes proves the failure was about evidence, not a revoked account.
    write_fixture(&root, &bytes);
    let restored = command(
        &host,
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_eq!(restored.status, ExecutionStatus::Completed);
    assert_eq!(restored.output["health"]["status"], "connectivity_only");

    // A revoked binding cannot be probed even by an operator.
    let version = restored.output["source_cursor"].as_u64().unwrap();
    let pending = command(
        &host,
        trusted_metadata(&root, session),
        "connector.manage",
        json!({
            "action": "revoke",
            "binding_id": BINDING_ID,
            "expected_registry_version": version,
            "reason": "INT-09 revoke acceptance",
            "idempotency_key": "revoke-health-1",
        }),
    )
    .await;
    let revoked = approve(&host, trusted_metadata(&root, session), &pending).await;
    assert_eq!(
        revoked.status,
        ExecutionStatus::Completed,
        "revoke failed: {:?}",
        revoked.error
    );
    let denied = command(
        &host,
        trusted_metadata(&root, session),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_denied(&denied, "connector_binding_revoked");

    let _ = fs::remove_dir_all(&root);
}

#[tokio::test]
async fn health_separates_missing_scope_from_unsupported_destructive_connectors() {
    let scope_root = fixture_dir("health-scope");
    fs::create_dir_all(&scope_root).unwrap();
    let scope_bytes = fixture_bytes("lookup");
    let scope_digest = connector_fixture_sha256(&scope_bytes);
    write_fixture(&scope_root, &scope_bytes);
    let host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject)).unwrap();

    // The definition requires `admin`, the binding grants only `read`: health must report a
    // scope problem, never `credential_invalid`.
    let bound = bind(
        &host,
        &scope_root,
        "session-int09-scope",
        definition("lookup", ConnectorEffect::ReadOnly, "admin"),
        account_binding(
            &scope_digest,
            BTreeSet::from(["read".to_owned()]),
            BTreeSet::new(),
        ),
        0,
        "bind-health-scope",
    )
    .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "bind failed: {:?}",
        bound.error
    );
    let scope_fact = command(
        &host,
        trusted_metadata(&scope_root, "session-int09-scope"),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_eq!(scope_fact.status, ExecutionStatus::Completed);
    assert_eq!(scope_fact.output["health"]["status"], "scope_insufficient");
    assert_eq!(
        scope_fact.output["health"]["error_code"],
        "connector_account_scope_denied"
    );
    assert_ne!(scope_fact.output["health"]["status"], "credential_invalid");
    assert!(scope_fact.output["health"]["limitations"]
        .as_array()
        .unwrap()
        .contains(&json!("required_scope_not_granted")));

    let write_root = fixture_dir("health-write-only");
    fs::create_dir_all(&write_root).unwrap();
    let write_bytes = fixture_bytes("mutate");
    let write_digest = connector_fixture_sha256(&write_bytes);
    write_fixture(&write_root, &write_bytes);
    let write_host =
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject)).unwrap();
    let bound = bind(
        &write_host,
        &write_root,
        "session-int09-write",
        definition("mutate", ConnectorEffect::Write, "write"),
        account_binding(
            &write_digest,
            BTreeSet::new(),
            BTreeSet::from(["write".to_owned()]),
        ),
        0,
        "bind-health-write",
    )
    .await;
    assert_eq!(
        bound.status,
        ExecutionStatus::Completed,
        "write-only bind failed: {:?}",
        bound.error
    );
    // The fixture is removed on purpose: an unsupported probe must not fall back to a
    // destructive read or attempt to open the file.
    fs::remove_file(write_root.join(FIXTURE_PATH)).unwrap();
    let unsupported = command(
        &write_host,
        trusted_metadata(&write_root, "session-int09-write"),
        "connector.health",
        json!({"binding_id": BINDING_ID}),
    )
    .await;
    assert_eq!(unsupported.status, ExecutionStatus::Completed);
    assert_eq!(unsupported.output["health"]["status"], "unsupported");
    assert_eq!(
        unsupported.output["health"]["error_code"],
        "connector_read_only_probe_unsupported"
    );
    assert!(unsupported.output["health"]["limitations"]
        .as_array()
        .unwrap()
        .contains(&json!("read_only_probe_contract_missing")));

    let _ = fs::remove_dir_all(&scope_root);
    let _ = fs::remove_dir_all(&write_root);
}
