use kiana_domain::{AggregateVersion, EventId, RequestId, RuntimeEvent, TransitionBatch};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

#[tokio::test]
async fn event_id_reuse_is_denied() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let first = RuntimeEvent::new(request_id, 1, "run.started", json!({"run_id":"run-1"}))
        .unwrap()
        .with_stream_metadata("run", "run-1", 1);
    let mut duplicate = first.clone();
    duplicate.sequence = 2;
    assert!(store.append(first).await.is_ok());
    assert!(matches!(
        store.append(duplicate).await,
        Err(PortError::Conflict(reason)) if reason == "event_id_duplicate"
    ));
}

#[tokio::test]
async fn same_request_different_command_digest_conflicts() {
    let store = MemoryEventLog::new();
    let command_id = RequestId::new();
    let first = transition(command_id, 'a');
    store.commit_transition(first).await.unwrap();
    let error = store
        .commit_transition(transition(command_id, 'b'))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        PortError::Conflict(reason) if reason == "event_store_command_digest_mismatch"
    ));
}

#[tokio::test]
async fn malformed_identity_links_are_denied_before_append() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let mut malformed = RuntimeEvent::new(request_id, 1, "run.started", json!({})).unwrap();
    malformed.causation_event_id = Some(malformed.event_id);
    assert!(matches!(
        store.append(malformed).await,
        Err(PortError::Failed(reason)) if reason == "event_identity_links_invalid:event_causation_self"
    ));

    let mut command_without_correlation =
        RuntimeEvent::new(request_id, 1, "run.started", json!({})).unwrap();
    command_without_correlation.command_id = Some(RequestId::new());
    command_without_correlation.correlation_id = None;
    assert!(matches!(
        store.append(command_without_correlation).await,
        Err(PortError::Failed(reason))
            if reason == "event_identity_links_invalid:event_command_requires_correlation"
    ));
}

#[tokio::test]
async fn nil_identity_links_are_denied_before_append() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let nil_request = RequestId::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    let mut malformed = RuntimeEvent::new(request_id, 1, "run.started", json!({})).unwrap();
    malformed.command_id = Some(nil_request);
    assert!(matches!(
        store.append(malformed).await,
        Err(PortError::Failed(reason))
            if reason == "event_identity_links_invalid:event_command_id_invalid"
    ));
}

#[tokio::test]
async fn event_store_enforces_registered_payload_ids_and_unknown_fields() {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let run_id = kiana_domain::RunId::new();

    let mut missing_run_id =
        RuntimeEvent::new(request_id, 1, "run.started", json!({"unexpected": true}))
            .unwrap()
            .with_stream_metadata("run", run_id.to_string(), 1);
    assert!(matches!(
        store.append(missing_run_id.clone()).await,
        Err(PortError::Failed(reason)) if reason == "event_contract_event_required_id_missing:run_id"
    ));

    missing_run_id.data = json!({"run_id": run_id, "unexpected": true});
    assert!(matches!(
        store.append(missing_run_id).await,
        Err(PortError::Failed(reason)) if reason == "event_contract_event_payload_unknown_field"
    ));

    let valid = RuntimeEvent::new(request_id, 1, "run.started", json!({"run_id": run_id}))
        .unwrap()
        .with_stream_metadata("run", run_id.to_string(), 1);
    store.append(valid).await.unwrap();
}

#[tokio::test]
async fn idempotent_replay_rejects_command_id_drift() {
    assert_identity_link_drift_denied(IdentityLink::CommandId).await;
}

#[tokio::test]
async fn idempotent_replay_rejects_correlation_id_drift() {
    assert_identity_link_drift_denied(IdentityLink::CorrelationId).await;
}

#[tokio::test]
async fn idempotent_replay_rejects_causation_event_id_drift() {
    assert_identity_link_drift_denied(IdentityLink::CausationEventId).await;
}

#[tokio::test]
async fn idempotent_replay_rejects_parent_event_id_drift() {
    assert_identity_link_drift_denied(IdentityLink::ParentEventId).await;
}

#[derive(Clone, Copy)]
enum IdentityLink {
    CommandId,
    CorrelationId,
    CausationEventId,
    ParentEventId,
}

async fn assert_identity_link_drift_denied(link: IdentityLink) {
    let store = MemoryEventLog::new();
    let request_id = RequestId::new();
    let command_id = distinct_request_id(&[request_id]);
    let correlation_id = distinct_request_id(&[request_id]);
    let first_base = RuntimeEvent::new(request_id, 1, "run.started", json!({})).unwrap();
    let causation_event_id = distinct_event_id(&[first_base.event_id]);
    let parent_event_id = distinct_event_id(&[first_base.event_id, causation_event_id]);
    let first = first_base
        .with_identity_links(
            Some(command_id),
            Some(correlation_id),
            Some(causation_event_id),
            Some(parent_event_id),
        )
        .with_idempotency_key("er02-identity-key");
    store.append_idempotent(first).await.unwrap();

    let drifted_base = RuntimeEvent::new(request_id, 1, "run.started", json!({})).unwrap();
    let (drifted_command_id, drifted_correlation_id, drifted_causation_id, drifted_parent_id) =
        match link {
            IdentityLink::CommandId => (
                Some(distinct_request_id(&[command_id, request_id])),
                Some(correlation_id),
                Some(causation_event_id),
                Some(parent_event_id),
            ),
            IdentityLink::CorrelationId => (
                Some(command_id),
                Some(distinct_request_id(&[correlation_id, request_id])),
                Some(causation_event_id),
                Some(parent_event_id),
            ),
            IdentityLink::CausationEventId => (
                Some(command_id),
                Some(correlation_id),
                Some(distinct_event_id(&[
                    causation_event_id,
                    drifted_base.event_id,
                ])),
                Some(parent_event_id),
            ),
            IdentityLink::ParentEventId => (
                Some(command_id),
                Some(correlation_id),
                Some(causation_event_id),
                Some(distinct_event_id(&[parent_event_id, drifted_base.event_id])),
            ),
        };
    let drifted = drifted_base
        .with_identity_links(
            drifted_command_id,
            drifted_correlation_id,
            drifted_causation_id,
            drifted_parent_id,
        )
        .with_idempotency_key("er02-identity-key");
    assert!(matches!(
        store.append_idempotent(drifted).await,
        Err(PortError::Conflict(reason)) if reason == "event_idempotency_key_payload_mismatch"
    ));
}

fn distinct_request_id(excluded: &[RequestId]) -> RequestId {
    loop {
        let candidate = RequestId::new();
        if !excluded.contains(&candidate) {
            return candidate;
        }
    }
}

fn distinct_event_id(excluded: &[EventId]) -> EventId {
    loop {
        let candidate = EventId::new();
        if !excluded.contains(&candidate) {
            return candidate;
        }
    }
}

fn transition(command_id: RequestId, digest: char) -> TransitionBatch {
    let event = RuntimeEvent::new(
        command_id,
        1,
        "run.started",
        json!({"run_id":"run-identity"}),
    )
    .unwrap()
    .with_stream_metadata("run", "run-identity", 1);
    TransitionBatch {
        command_id,
        command_digest: std::iter::repeat(digest).take(64).collect(),
        expected_versions: vec![AggregateVersion::new("run", "run-identity", 0)],
        events: vec![event],
    }
}

#[allow(dead_code)]
fn _event_id_is_a_stable_uuid() {
    let _ = EventId::new();
}
