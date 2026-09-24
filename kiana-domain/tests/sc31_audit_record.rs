use kiana_domain::{
    AuditActionKind, AuditDecision, AuditRecord, AuditRecordEvent, DataClass, EventId, RequestId,
    SchemaVersion, AUDIT_EVENT_KIND, AUDIT_EVENT_SCHEMA, AUDIT_EVENT_SCHEMA_VERSION,
};

fn record() -> AuditRecord {
    AuditRecord::new(
        "audit:sc31",
        AuditActionKind::Authorization,
        AuditDecision::Accepted,
        "server:control-plane",
        "run",
        "run:sc31",
        7,
        vec![EventId::new()],
        3,
        4,
        DataClass::Internal,
        "audit",
    )
    .expect("SC-31 fixture record")
}

#[test]
fn audit_record_requires_reason_and_redaction_invariants() {
    let contract =
        kiana_domain::schema_contract(AUDIT_EVENT_SCHEMA).expect("SC-31 schema registration");
    assert_eq!(contract.version, SchemaVersion::new(1, 0));
    assert!(!contract.allow_unknown_fields);

    let record = record();
    assert_eq!(record.reason, "reason_unspecified");
    assert!(!record.payload_recoverable);
    assert!(record.redaction_profile.starts_with("sha256:"));
    record.validate().expect("new record is valid");

    let mut recoverable = record.clone();
    recoverable.payload_recoverable = true;
    recoverable.record_digest = recoverable.digest();
    assert_eq!(
        recoverable.validate().unwrap_err(),
        "audit_payload_recoverable"
    );

    let mut unredacted = record;
    unredacted.reason = "Authorization: Bearer raw-token".to_owned();
    unredacted.record_digest = unredacted.digest();
    assert_eq!(
        unredacted.validate().unwrap_err(),
        "audit_reason_unredacted"
    );
}

#[test]
fn audit_event_is_strict_append_only_and_binds_source_metadata() {
    let event = AuditRecordEvent::new(record()).expect("SC-31 fixture envelope");
    assert_eq!(event.schema, AUDIT_EVENT_SCHEMA);
    assert_eq!(event.version, AUDIT_EVENT_SCHEMA_VERSION);
    assert_eq!(event.event_kind, AUDIT_EVENT_KIND);
    assert!(event.append_only);
    event.validate().expect("envelope is valid");

    let encoded = serde_json::to_value(&event).expect("envelope JSON");
    assert_eq!(
        serde_json::from_value::<AuditRecordEvent>(encoded.clone()).unwrap(),
        event
    );
    let mut unknown = encoded;
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AuditRecordEvent>(unknown).is_err());

    let mut forged = event.clone();
    forged.source_cursor += 1;
    forged.event_digest = forged.digest();
    assert_eq!(
        forged.validate().unwrap_err(),
        "audit_event_source_binding_mismatch"
    );

    let runtime = event
        .into_runtime_event(RequestId::new(), 1)
        .expect("runtime envelope");
    assert_eq!(runtime.kind, AUDIT_EVENT_KIND);
    assert_eq!(runtime.payload_recoverable, Some(false));
    assert_eq!(runtime.data_epoch, Some(4));
    assert_eq!(
        runtime.idempotency_key.as_deref(),
        Some("audit-record:audit:sc31")
    );
}
