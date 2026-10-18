//! INT-24 route fixture: a signed webhook/A2A event reaches `ControlPlane` through the production
//! daemon route, and every denial stays zero-effect.
//!
//! Deny-first ordering is asserted by counting EventStore facts: a refused event must not add a
//! single event, which proves `ControlPlane` was never entered (the route returns before
//! `DaemonHost::handle`). The durable replay store is exercised across two host instances that
//! share one replay file, so replay protection survives a daemon restart rather than living in a
//! process-local map.

use kiana_daemon::{
    ConnectorIngressRouteConfig, ConnectorIngressVerifier, DaemonHost,
    JsonlConnectorIngressReplayStore, ProjectTrustAuthority,
};
use kiana_domain::{
    connector_fixture_sha256, AccountBinding, ConnectorDefinition, ConnectorEffect,
    ConnectorIngressEvent, ConnectorIngressPolicy, ConnectorIngressProtocol, ConnectorOperation,
    RequestId, RuntimeEvent, CONNECTOR_STREAM,
};
use kiana_eventlog::JsonlEventLog;
use kiana_ports::{EventStorePort, PortError};
use kiana_protocol::ExecutionStatus;
use kiana_runner::{KianaHarness, ScriptedModel};
use ring::hmac;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const SECRET: &[u8] = b"0123456789abcdef-int24-secret";
const SOURCE_ID: &str = "source-int24";
const KEY_ID: &str = "key-int24";
const TENANT_ID: &str = "tenant-int24";
const PROJECT_ID: &str = "project-int24";
const BINDING_ID: &str = "binding-int24";
const CONNECTOR_ID: &str = "connector-int24";
const ACCOUNT_ID: &str = "account-int24";
const OPERATION: &str = "lookup";
const FIXTURE_PATH: &str = "fixtures/int24-connector.json";
/// Event time used by every signed fixture. Route calls pass a server clock near this value.
const OCCURRED_AT: u64 = 1_000;

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
        "kiana-int24-{label}-{}-{nanos}",
        std::process::id()
    ))
}

fn policy() -> ConnectorIngressPolicy {
    ConnectorIngressPolicy::new(
        ConnectorIngressProtocol::Webhook,
        SOURCE_ID,
        KEY_ID,
        TENANT_ID,
        PROJECT_ID,
        ["job.created".to_owned()],
        ["id".to_owned()],
        ["id".to_owned(), "value".to_owned()],
        100,
        4_096,
    )
    .unwrap()
}

fn sign(event: &mut ConnectorIngressEvent) {
    let key = hmac::Key::new(hmac::HMAC_SHA256, SECRET);
    let tag = hmac::sign(&key, event.signing_digest().as_bytes());
    event.signature = tag
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    event.ingress_digest = kiana_domain::json_digest(&json!({
        "schema": event.schema,
        "protocol": event.protocol,
        "source_id": event.source_id,
        "key_id": event.key_id,
        "tenant_id": event.tenant_id,
        "project_id": event.project_id,
        "event_id": event.event_id,
        "event_kind": event.event_kind,
        "occurred_at_unix_ms": event.occurred_at_unix_ms,
        "nonce": event.nonce,
        "payload": event.payload,
        "payload_digest": event.payload_digest,
        "signature": event.signature,
        "signature_algorithm": event.signature_algorithm,
    }));
}

fn event_with(event_id: &str, nonce: &str, value: i64, occurred_at: u64) -> ConnectorIngressEvent {
    let mut event = ConnectorIngressEvent::new(
        ConnectorIngressProtocol::Webhook,
        SOURCE_ID,
        KEY_ID,
        TENANT_ID,
        PROJECT_ID,
        event_id,
        "job.created",
        occurred_at,
        nonce,
        json!({"id": "job-1", "value": value}),
        "00".repeat(32),
    )
    .unwrap();
    sign(&mut event);
    event
}

fn signed_event(event_id: &str, value: i64) -> ConnectorIngressEvent {
    event_with(event_id, &format!("nonce-{event_id}"), value, OCCURRED_AT)
}

fn definition() -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: CONNECTOR_ID.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-int24".to_owned(),
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

fn fixture_bytes() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": "kiana.connector-fixture.v1",
        "connector_id": CONNECTOR_ID,
        "account_id": ACCOUNT_ID,
        "operations": {
            "lookup": [
                {
                    "payload": {"id": "job-1", "value": 1},
                    "outcome": "succeeded",
                    "receipt_id": "receipt-int24",
                    "result": {"value": "one"},
                },
            ],
        },
    }))
    .unwrap()
}

fn binding_snapshot(fixture_sha256: &str) -> AccountBinding {
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

fn seed_binding(project_root: &str) -> kiana_domain::ConnectorBindingSnapshot {
    let bytes = fixture_bytes();
    let digest = connector_fixture_sha256(&bytes);
    let path = Path::new(project_root).join(FIXTURE_PATH);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &bytes).unwrap();
    kiana_domain::ConnectorBindingSnapshot {
        definition: definition(),
        binding: binding_snapshot(&digest),
        project_root: project_root.to_owned(),
        revision: 1,
        status: "active".to_owned(),
    }
}

async fn seed_event_log(events_path: &Path, binding: &kiana_domain::ConnectorBindingSnapshot) {
    fs::create_dir_all(events_path.parent().unwrap()).unwrap();
    let store = Arc::new(JsonlEventLog::open(events_path).expect("open event log"));
    let port: Arc<dyn EventStorePort> = store;
    let event = RuntimeEvent::new(
        RequestId::new(),
        1,
        "connector.binding",
        json!({"state": binding}),
    )
    .expect("binding event")
    .with_stream_metadata(CONNECTOR_STREAM, binding.project_root.clone(), 1);
    port.append(event).await.expect("append seeded binding");
    port.flush().await.expect("flush seeded binding");
    drop(port);
}

fn disk_host(events_path: &Path) -> DaemonHost {
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&json!([])).unwrap()));
    DaemonHost::with_harness_on_disk_and_project_authority(
        harness,
        events_path,
        Arc::new(TrustedProject),
    )
    .expect("disk daemon host")
}

fn route_config(project_root: &str) -> ConnectorIngressRouteConfig {
    ConnectorIngressRouteConfig {
        source_id: SOURCE_ID.to_owned(),
        project_root: project_root.to_owned(),
        project_id: PROJECT_ID.to_owned(),
        binding_id: BINDING_ID.to_owned(),
        operation: OPERATION.to_owned(),
        session_id: "session-int24-route".to_owned(),
    }
}

fn durable_verifier(store_path: &Path) -> ConnectorIngressVerifier {
    let store = Arc::new(
        JsonlConnectorIngressReplayStore::open(store_path).expect("open durable replay store"),
    );
    ConnectorIngressVerifier::new_durable([(policy(), SECRET.to_vec())], store)
        .expect("durable verifier")
}

async fn event_count(host: &DaemonHost) -> usize {
    host.persisted_events()
        .await
        .expect("event store read")
        .expect("persistent event store")
        .len()
}

async fn setup(label: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = project_dir(label);
    fs::create_dir_all(&root).unwrap();
    let canonical = fs::canonicalize(&root).unwrap();
    let events_path = root.join("events.jsonl");
    let replay_path = root.join("replay.json");
    let binding = seed_binding(canonical.to_string_lossy().as_ref());
    seed_event_log(&events_path, &binding).await;
    (canonical, events_path, replay_path)
}

fn refusal(result: Result<impl std::fmt::Debug, PortError>) -> PortError {
    match result {
        Ok(value) => panic!("expected a refusal, got {value:?}"),
        Err(error) => error,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_occurrence_is_refused_by_the_server_clock_before_any_effect() {
    let (root, events_path, replay_path) = setup("stale").await;
    let host = disk_host(&events_path)
        .with_connector_ingress_route(
            durable_verifier(&replay_path),
            route_config(root.to_string_lossy().as_ref()),
        )
        .expect("route configured");
    let before = event_count(&host).await;
    // The event claims to be fresh at `occurred_at`, but the server clock is far past the 100ms
    // skew window. The old code derived "now" from the event and could not detect this.
    let error = refusal(
        host.handle_connector_ingress(signed_event("event-stale", 1), OCCURRED_AT + 5_000)
            .await,
    );
    assert_eq!(
        error,
        PortError::Conflict("connector_ingress_clock_skew".to_owned())
    );
    assert_eq!(
        event_count(&host).await,
        before,
        "denial must not enter ControlPlane"
    );
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn nonce_replay_and_payload_conflict_are_refused_before_any_effect() {
    let (root, events_path, replay_path) = setup("replay").await;
    let host = disk_host(&events_path)
        .with_connector_ingress_route(
            durable_verifier(&replay_path),
            route_config(root.to_string_lossy().as_ref()),
        )
        .expect("route configured");
    let before = event_count(&host).await;
    let first = event_with("event-a", "nonce-shared", 1, OCCURRED_AT);
    let replayed = event_with("event-b", "nonce-shared", 1, OCCURRED_AT);
    // Admission of the first event is expected to reach ControlPlane; the fixture only asserts the
    // *replay* refusals stay zero-effect. `handle` is allowed to return a ControlPlane response.
    let _ = host.handle_connector_ingress(first, OCCURRED_AT + 10).await;
    let after_first = event_count(&host).await;
    assert!(after_first >= before);

    let error = refusal(
        host.handle_connector_ingress(replayed, OCCURRED_AT + 20)
            .await,
    );
    assert_eq!(
        error,
        PortError::Conflict("connector_ingress_nonce_replay".to_owned())
    );
    assert_eq!(
        event_count(&host).await,
        after_first,
        "nonce replay must not enter ControlPlane"
    );

    // Same source+event ID with a different payload is a conflict, not a retry.
    let conflict = event_with("event-a", "nonce-other", 2, OCCURRED_AT);
    let error = refusal(
        host.handle_connector_ingress(conflict, OCCURRED_AT + 30)
            .await,
    );
    assert_eq!(
        error,
        PortError::Conflict("connector_ingress_dedupe_payload_conflict".to_owned())
    );
    assert_eq!(event_count(&host).await, after_first);
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn durable_replay_survives_a_daemon_restart_and_never_reaches_control_plane_twice() {
    let (root, events_path, replay_path) = setup("durable").await;
    let project = root.to_string_lossy().to_string();
    let event = signed_event("event-durable", 1);

    let first_host = disk_host(&events_path)
        .with_connector_ingress_route(durable_verifier(&replay_path), route_config(&project))
        .expect("route configured");
    let admitted = first_host
        .handle_connector_ingress(event.clone(), OCCURRED_AT + 10)
        .await
        .expect("first admission");
    assert!(!admitted.duplicate);
    assert_eq!(admitted.broker_calls, 0);
    assert!(
        admitted.response.is_some(),
        "an admitted occurrence must be submitted to ControlPlane"
    );
    let after_first = event_count(&first_host).await;
    assert!(after_first > 1, "admission must leave a ControlPlane fact");
    drop(first_host);

    // A fresh host, a fresh verifier and the same replay file model a daemon restart.
    let second_host = disk_host(&events_path)
        .with_connector_ingress_route(durable_verifier(&replay_path), route_config(&project))
        .expect("route configured");
    let replayed = second_host
        .handle_connector_ingress(event, OCCURRED_AT + 20)
        .await
        .expect("duplicate is not an error");
    assert!(replayed.duplicate, "restart must not accept the same event");
    assert_eq!(replayed.broker_calls, 0);
    assert!(
        replayed.response.is_none(),
        "duplicate must not call ControlPlane"
    );
    assert_eq!(
        event_count(&second_host).await,
        after_first,
        "duplicate must not append any fact"
    );
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn route_mismatch_and_untrusted_project_are_refused_before_any_effect() {
    let (root, events_path, replay_path) = setup("routing").await;
    let project = root.to_string_lossy().to_string();
    let host = disk_host(&events_path)
        .with_connector_ingress_route(durable_verifier(&replay_path), route_config(&project))
        .expect("route configured");
    let before = event_count(&host).await;
    // A signed event for another allowlisted-looking source cannot be delivered on this route; the
    // server-owned route, not the wire source id, selects the connector account.
    let mut foreign = signed_event("event-foreign", 1);
    foreign.source_id = "source-other".to_owned();
    let error = refusal(
        host.handle_connector_ingress(foreign, OCCURRED_AT + 10)
            .await,
    );
    assert_eq!(
        error,
        PortError::Conflict("connector_ingress_source_route_mismatch".to_owned())
    );
    assert_eq!(event_count(&host).await, before);

    // The same signed event on the same route is still refused while the project is untrusted.
    struct UntrustedProject;
    impl ProjectTrustAuthority for UntrustedProject {
        fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
            Ok(false)
        }
    }
    let harness = KianaHarness::new(Arc::new(ScriptedModel::from_json(&json!([])).unwrap()));
    let untrusted = DaemonHost::with_harness_on_disk_and_project_authority(
        harness,
        &events_path,
        Arc::new(UntrustedProject),
    )
    .expect("disk daemon host")
    .with_connector_ingress_route(durable_verifier(&replay_path), route_config(&project))
    .expect("route configured");
    let error = refusal(
        untrusted
            .handle_connector_ingress(signed_event("event-untrusted", 1), OCCURRED_AT + 10)
            .await,
    );
    assert_eq!(error, PortError::Conflict("project_untrusted".to_owned()));
    assert_eq!(event_count(&untrusted).await, before);
    let _ = fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn admitted_occurrence_enters_control_plane_and_completes_through_the_broker() {
    let (root, events_path, replay_path) = setup("success").await;
    let project = root.to_string_lossy().to_string();
    let host = disk_host(&events_path)
        .with_connector_ingress_route(durable_verifier(&replay_path), route_config(&project))
        .expect("route configured");
    let result = host
        .handle_connector_ingress(signed_event("event-success", 1), OCCURRED_AT + 10)
        .await
        .expect("admission");
    let response = result.response.expect("ControlPlane response");
    assert_eq!(
        response.status,
        ExecutionStatus::Completed,
        "route response: {:?} / {}",
        response.error,
        response.output
    );
    assert_eq!(response.output["receipt"]["outcome"], "succeeded");
    assert_eq!(response.output["external_effect_performed"], false);
    // The event digest chain points back to the verified occurrence, not to a wire-supplied intent.
    assert!(result
        .intent_digest
        .as_deref()
        .unwrap_or_default()
        .starts_with("sha256:"));
    let events = host
        .persisted_events()
        .await
        .unwrap()
        .expect("persistent event store");
    assert!(
        events.iter().any(|event| event.kind == "connector.invoked"),
        "the normal connector fact must be the only effect record"
    );
    let _ = fs::remove_dir_all(&root);
}
