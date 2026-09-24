use kiana_domain::{
    AggregateVersion, AuditActionKind, AuditDecision, AuditRecord, AuditRecordEvent, DataClass,
    EventId, RequestId, RuntimeEvent, TransitionBatch,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

fn audit_runtime_event() -> RuntimeEvent {
    let record = AuditRecord::new(
        "audit:sc31-eventlog",
        AuditActionKind::Authorization,
        AuditDecision::Accepted,
        "server:control-plane",
        "run",
        "run:sc31-eventlog",
        11,
        vec![EventId::new()],
        2,
        5,
        DataClass::Internal,
        "audit",
    )
    .expect("valid audit record");
    AuditRecordEvent::new(record)
        .expect("valid audit envelope")
        .into_runtime_event(RequestId::new(), 1)
        .expect("valid runtime event")
}

#[tokio::test]
async fn eventlog_accepts_only_bound_audit_envelopes_and_replays_by_key() {
    let store = MemoryEventLog::new();
    let event = audit_runtime_event();
    store
        .append(event.clone())
        .await
        .expect("append audit fact");
    let replay = store
        .append_idempotent(event)
        .await
        .expect("same audit key replays");
    assert!(replay.replayed);
    assert_eq!(store.read_all().await.unwrap().len(), 1);

    let mut wrong_key = audit_runtime_event();
    wrong_key.idempotency_key = Some("audit-record:forged".to_owned());
    assert_eq!(
        store.append(wrong_key).await.unwrap_err(),
        PortError::Failed("eventlog_audit_idempotency_binding_mismatch".to_owned())
    );

    let forged = RuntimeEvent::new(
        RequestId::new(),
        1,
        "audit.forged",
        json!({"decision":"accepted"}),
    )
    .unwrap();
    assert_eq!(
        store.append(forged).await.unwrap_err(),
        PortError::Failed("eventlog_audit_kind_untrusted".to_owned())
    );
    assert_eq!(store.read_all().await.unwrap().len(), 1);
}

#[tokio::test]
async fn transition_boundary_rejects_tampered_audit_payload_before_commit() {
    let store = MemoryEventLog::new();
    let mut forged = audit_runtime_event().with_stream_metadata("audit_record", "sc31", 1);
    forged.data["unexpected"] = json!(true);
    let batch = TransitionBatch {
        command_id: RequestId::new(),
        command_digest: "a".repeat(64),
        expected_versions: vec![AggregateVersion::new("audit_record", "sc31", 0)],
        events: vec![forged],
    };
    assert_eq!(
        store.commit_transition(batch).await.unwrap_err(),
        PortError::Failed("eventlog_audit_payload_invalid".to_owned())
    );
    assert!(store.read_all().await.unwrap().is_empty());
}
