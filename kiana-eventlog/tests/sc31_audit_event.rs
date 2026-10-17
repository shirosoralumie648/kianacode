//! SC-31 EventLog audit boundary fixtures (deny-first).
//!
//! `audit.record` is the only accepted audit kind and its envelope must be the strict,
//! digest-bound, append-only `AuditRecordEvent`. Local tests are not run in this session
//! (CI-only); the lane is `.github/workflows/sc31-audit-record.yml`.

use kiana_domain::{
    AggregateVersion, AuditActionKind, AuditDecision, AuditRecord, AuditRecordEvent, DataClass,
    EventId, RequestId, RuntimeEvent, SchemaVersion, TransitionBatch,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde_json::json;

fn envelope() -> AuditRecordEvent {
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
    AuditRecordEvent::new(record).expect("valid audit envelope")
}

fn audit_runtime_event() -> RuntimeEvent {
    envelope()
        .into_runtime_event(RequestId::new(), 1)
        .expect("valid runtime event")
}

/// Rebuild the acceptance frame with a tampered envelope payload.
fn tampered_event(mutate: impl FnOnce(&mut serde_json::Value)) -> RuntimeEvent {
    let mut event = audit_runtime_event();
    mutate(&mut event.data);
    event
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

#[tokio::test]
async fn missing_audit_identity_fields_are_refused_before_any_storage_mutation() {
    let store = MemoryEventLog::new();

    let cases: [(&str, RuntimeEvent, &str); 12] = [
        (
            "missing_actor",
            tampered_event(|data| {
                data["record"].as_object_mut().unwrap().remove("actor_ref");
            }),
            "eventlog_audit_field_missing:record.actor_ref",
        ),
        (
            "missing_decision",
            tampered_event(|data| {
                data["record"].as_object_mut().unwrap().remove("decision");
            }),
            "eventlog_audit_field_missing:record.decision",
        ),
        (
            "missing_reason",
            tampered_event(|data| {
                data["record"].as_object_mut().unwrap().remove("reason");
            }),
            "eventlog_audit_field_missing:record.reason",
        ),
        (
            "missing_record_cursor",
            tampered_event(|data| {
                data["record"]
                    .as_object_mut()
                    .unwrap()
                    .remove("source_cursor");
            }),
            "eventlog_audit_field_missing:record.source_cursor",
        ),
        (
            "missing_record_source_events",
            tampered_event(|data| {
                data["record"]
                    .as_object_mut()
                    .unwrap()
                    .remove("source_event_ids");
            }),
            "eventlog_audit_field_missing:record.source_event_ids",
        ),
        (
            "missing_envelope_cursor",
            tampered_event(|data| {
                data.as_object_mut().unwrap().remove("source_cursor");
            }),
            "eventlog_audit_field_missing:source_cursor",
        ),
        (
            "missing_record",
            tampered_event(|data| {
                data.as_object_mut().unwrap().remove("record");
            }),
            "eventlog_audit_field_missing:record",
        ),
        (
            "null_actor",
            tampered_event(|data| {
                data["record"]["actor_ref"] = json!(null);
            }),
            "eventlog_audit_field_missing:record.actor_ref",
        ),
        (
            "blank_actor",
            tampered_event(|data| {
                data["record"]["actor_ref"] = json!("   ");
            }),
            "eventlog_audit_envelope:audit_actor_ref_required",
        ),
        (
            "blank_reason",
            tampered_event(|data| {
                data["record"]["reason"] = json!("   ");
            }),
            "eventlog_audit_envelope:audit_reason_required",
        ),
        (
            "missing_append_only",
            tampered_event(|data| {
                data.as_object_mut().unwrap().remove("append_only");
            }),
            "eventlog_audit_field_missing:append_only",
        ),
        (
            "record_not_object",
            tampered_event(|data| {
                data["record"] = json!("audit:sc31");
            }),
            "eventlog_audit_field_missing:record",
        ),
    ];

    for (label, event, expected) in cases {
        let expected = expected.to_owned();
        assert_eq!(
            store.append(event.clone()).await.unwrap_err(),
            PortError::Failed(expected.clone()),
            "{label} must be refused on append"
        );
        assert_eq!(
            store.append_idempotent(event.clone()).await.unwrap_err(),
            PortError::Failed(expected.clone()),
            "{label} must be refused on idempotent replay"
        );
        let batch = TransitionBatch {
            command_id: RequestId::new(),
            command_digest: "b".repeat(64),
            expected_versions: vec![AggregateVersion::new("audit_record", "sc31", 0)],
            events: vec![event.with_stream_metadata("audit_record", "sc31", 1)],
        };
        assert_eq!(
            store.commit_transition(batch).await.unwrap_err(),
            PortError::Failed(expected),
            "{label} must be refused on the transition boundary"
        );
        assert!(
            store.read_all().await.unwrap().is_empty(),
            "{label} must not mutate storage"
        );
    }
}

#[tokio::test]
async fn secrets_prompts_and_version_drift_never_enter_the_audit_log() {
    let store = MemoryEventLog::new();

    let secret = tampered_event(|data| {
        data["record"]["reason"] = json!("Authorization: Bearer sc31-raw-secret");
    });
    assert_eq!(
        store.append(secret).await.unwrap_err(),
        PortError::Failed("eventlog_audit_envelope:audit_reason_unredacted".to_owned())
    );

    let prompt = tampered_event(|data| {
        data["prompt"] = json!("raw model prompt");
    });
    assert_eq!(
        store.append(prompt).await.unwrap_err(),
        PortError::Failed("eventlog_audit_payload_invalid".to_owned())
    );

    let transcript = tampered_event(|data| {
        data["record"]["transcript"] = json!(["raw prompt"]);
    });
    assert_eq!(
        store.append(transcript).await.unwrap_err(),
        PortError::Failed("eventlog_audit_payload_invalid".to_owned())
    );

    let major = tampered_event(|data| {
        data["version"] = json!({"major": 2, "minor": 0});
    });
    assert_eq!(
        store.append(major).await.unwrap_err(),
        PortError::Failed(
            "eventlog_audit_envelope:audit_event_schema_version_incompatible".to_owned()
        )
    );

    assert!(store.read_all().await.unwrap().is_empty());

    // A re-signed minor bump inside the registered major stays appendable.
    let mut minor = envelope();
    minor.version = SchemaVersion::new(1, 1);
    minor.event_digest = minor.digest();
    let minor = minor
        .into_runtime_event(RequestId::new(), 1)
        .expect("re-signed minor envelope");
    store.append(minor).await.expect("minor bump appends");
    assert_eq!(store.read_all().await.unwrap().len(), 1);
}
