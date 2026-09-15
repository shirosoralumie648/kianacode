use kiana_domain::{
    append_audit_records, classify_audit_event, reduce_audit_records, AuditActionKind,
    AuditDecision, DataClass, EventId, RequestId, RuntimeEvent, SERVER_AUDIT_ACTOR,
};
use serde_json::{json, Value};

fn event(kind: &str, mut data: Value) -> RuntimeEvent {
    data["run_id"] = json!("run-oa04");
    data["authority_epoch"] = json!(7);
    data["data_epoch"] = json!(11);
    data["data_class"] = json!("internal");
    data["retention_class"] = json!("audit");
    RuntimeEvent::new(RequestId::new(), 1, kind, data)
        .expect("fixture event")
        .with_stream_metadata("run", "run-oa04", 1)
}

#[test]
fn taxonomy_is_stable_and_self_reported_audit_kinds_are_untrusted() {
    let cases = [
        (
            "command.rejected",
            AuditActionKind::Command,
            AuditDecision::Denied,
        ),
        (
            "approval.requested",
            AuditActionKind::Approval,
            AuditDecision::Staged,
        ),
        (
            "approval.approved",
            AuditActionKind::Approval,
            AuditDecision::Approved,
        ),
        (
            "capability.completed",
            AuditActionKind::Capability,
            AuditDecision::Consumed,
        ),
        (
            "credential.revoked",
            AuditActionKind::Credential,
            AuditDecision::Denied,
        ),
        (
            "recovery.result_unknown",
            AuditActionKind::Recovery,
            AuditDecision::Unknown,
        ),
        (
            "query.completed",
            AuditActionKind::Query,
            AuditDecision::Queried,
        ),
        (
            "export.completed",
            AuditActionKind::Export,
            AuditDecision::Exported,
        ),
    ];
    for (kind, action, decision) in cases {
        let data = if kind == "capability.completed" {
            json!({"capability_request_id":"cap-1"})
        } else {
            json!({})
        };
        let classified = classify_audit_event(kind, &data).expect("registered kind");
        assert_eq!(classified.map(|value| value.action_kind), Some(action));
        assert_eq!(classified.map(|value| value.decision), Some(decision));
    }
    assert!(classify_audit_event("unregistered.event", &json!({}))
        .unwrap()
        .is_none());
    for kind in ["audit.approved", "audit.completed", "audit.exported"] {
        assert!(classify_audit_event(kind, &json!({})).is_err(), "{kind}");
    }
    assert_eq!(
        classify_audit_event("audit.correction", &json!({})).unwrap_err(),
        "audit_correction_requires_append"
    );
}

#[test]
fn reducer_binds_rows_to_committed_sources_and_redacts_payloads() {
    let events = vec![
        event("request.accepted", json!({"command_id": RequestId::new()})),
        event(
            "command.rejected",
            json!({"reason":"Authorization: Bearer raw-audit-token"}),
        ),
        event(
            "approval.approved",
            json!({"approval_id":"approval-1", "decision":"approve"}),
        ),
        event(
            "capability.decision",
            json!({"capability_request_id":"cap-1", "gate":{"decision":"allowed"}}),
        ),
        event(
            "credential.rotated",
            json!({"credential_id":"credential-1"}),
        ),
        event("recovery.completed", json!({"recovery_id":"recovery-1"})),
        event("query.completed", json!({"query_id":"query-1"})),
        event("export.completed", json!({"export_id":"export-1"})),
        event("opaque.event", json!({"not_an_audit":true})),
    ];
    let records = reduce_audit_records(&events, 100).expect("committed audit projection");
    assert_eq!(records.len(), 8);
    for (index, record) in records.iter().enumerate() {
        record.validate().expect("reducer emits valid records");
        assert_eq!(record.actor_ref, SERVER_AUDIT_ACTOR);
        assert_eq!(record.source_cursor, 100 + index as u64);
        assert_eq!(record.source_event_ids.len(), 1);
        assert!(record
            .action_digest
            .as_deref()
            .is_some_and(|digest| digest.starts_with("sha256:")));
        assert!(!record
            .reason_code
            .as_deref()
            .unwrap_or_default()
            .contains("raw-audit-token"));
        assert_eq!(record.data_class, DataClass::Internal);
        let encoded = serde_json::to_string(record).expect("wire record");
        assert_eq!(
            serde_json::from_str::<kiana_domain::AuditRecord>(&encoded).unwrap(),
            *record
        );
    }
    assert_eq!(records[0].action_kind, AuditActionKind::Command);
    assert_eq!(records[1].decision, AuditDecision::Denied);
    assert_eq!(records[3].decision, AuditDecision::Accepted);
}

#[test]
fn reducer_rejects_unbound_or_forged_and_never_overwrites_a_record() {
    let unbound = RuntimeEvent::new(RequestId::new(), 1, "command.rejected", json!({})).unwrap();
    assert_eq!(
        reduce_audit_records(&[unbound], 1).unwrap_err(),
        "audit_target_binding_required"
    );

    let mut missing_epoch = event("command.rejected", json!({}));
    missing_epoch
        .data
        .as_object_mut()
        .unwrap()
        .remove("data_epoch");
    assert_eq!(
        reduce_audit_records(&[missing_epoch], 1).unwrap_err(),
        "audit_data_epoch_required"
    );

    let forged = event(
        "approval.approved",
        json!({"actor_ref":"model", "approval_id":"a"}),
    );
    assert_eq!(
        reduce_audit_records(&[forged], 1).unwrap_err(),
        "audit_actor_untrusted"
    );

    let mut first = event("approval.approved", json!({"audit_id":"immutable-1"}));
    let second = event("approval.denied", json!({"audit_id":"immutable-1"}));
    assert_eq!(
        reduce_audit_records(&[first.clone(), second], 1).unwrap_err(),
        "audit_decision_conflict"
    );
    first.event_id = EventId::new();
    assert_eq!(
        reduce_audit_records(&[first.clone(), first], 1).unwrap_err(),
        "audit_source_event_duplicate"
    );

    let duplicate = event("approval.approved", json!({"audit_id":"immutable-2"}));
    assert_eq!(
        reduce_audit_records(&[duplicate.clone(), duplicate.clone()], 1).unwrap_err(),
        "audit_source_event_duplicate"
    );
    let original_event = event("approval.approved", json!({"approval_id":"original"}));
    let original = reduce_audit_records(std::slice::from_ref(&original_event), 9).unwrap();
    let later_event = event("approval.denied", json!({"approval_id":"later"}));
    let appended = append_audit_records(&original, &[later_event], 10).unwrap();
    assert_eq!(appended.len(), 2);
    assert_eq!(appended[0], original[0]);
    assert_eq!(
        append_audit_records(&original, &[original_event], 10).unwrap_err(),
        "audit_source_event_conflict"
    );
    assert_eq!(
        reduce_audit_records(&[], 0).unwrap_err(),
        "audit_source_cursor_required"
    );
}
