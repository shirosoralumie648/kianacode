//! SC-31 versioned audit envelope wire fixture (deny-first).
//!
//! `kiana-protocol` re-exports the append-only `AuditRecordEvent` contract and registers it as
//! `kiana.audit-event.v1`. This fixture proves the re-export identity, the registered contract
//! and the deny-first wire behaviour of the only audit fact shape the EventLog boundary accepts.
//! It does not append events, authorize decisions or interpret any payload outside the domain
//! contract. Local tests are not run in this session (CI-only); the lane is
//! `.github/workflows/sc31-audit-record.yml`.

use kiana_domain::{
    check_schema_compatibility, schema_contract, CompatibilityPolicy, DataClass, EventId,
    SchemaLayer, SchemaVersion,
};
use kiana_protocol::{
    event_kind_spec, unknown_event_policy, AuditActionKind, AuditDecision, AuditRecord,
    AuditRecordEvent, RequestId, RuntimeEvent, AUDIT_EVENT_KIND, AUDIT_EVENT_SCHEMA,
    AUDIT_EVENT_SCHEMA_VERSION,
};
use serde_json::json;

fn digest_of(event: kiana_domain::AuditRecordEvent) -> String {
    event.digest()
}

fn record() -> AuditRecord {
    AuditRecord::new(
        "audit:sc31-wire",
        AuditActionKind::Authorization,
        AuditDecision::Accepted,
        "server:control-plane",
        "run",
        "run:sc31-wire",
        9,
        vec![EventId::new()],
        2,
        6,
        DataClass::Internal,
        "audit",
    )
    .expect("SC-31 wire fixture record")
}

fn envelope() -> AuditRecordEvent {
    AuditRecordEvent::new(record()).expect("SC-31 wire fixture envelope")
}

#[test]
fn protocol_reexports_the_registered_append_only_audit_envelope() {
    let event = envelope();
    // The protocol name is the domain type: one contract, not a wire-only clone.
    assert_eq!(digest_of(event.clone()), event.digest());
    assert_eq!(event.event_kind, AUDIT_EVENT_KIND);
    assert_eq!(event.schema, AUDIT_EVENT_SCHEMA);
    assert_eq!(event.version, AUDIT_EVENT_SCHEMA_VERSION);
    assert!(event.append_only);

    let contract = schema_contract(AUDIT_EVENT_SCHEMA).expect("registered audit event contract");
    assert_eq!(contract.version, AUDIT_EVENT_SCHEMA_VERSION);
    assert_eq!(contract.layer, SchemaLayer::RuntimeEvent);
    assert_eq!(contract.owner_crate, "kiana-domain");
    assert_eq!(
        contract.compatibility,
        CompatibilityPolicy::BackwardCompatible
    );
    assert!(
        !contract.allow_unknown_fields,
        "the audit envelope is a strict contract"
    );
    assert!(check_schema_compatibility(AUDIT_EVENT_SCHEMA, &SchemaVersion::new(1, 9)).is_ok());
    assert!(check_schema_compatibility(AUDIT_EVENT_SCHEMA, &SchemaVersion::new(2, 0)).is_err());

    // The generic event registry stays opaque for audit facts: the EventLog audit boundary is
    // the single interpreter of the envelope, so `audit.record` must not gain a second payload
    // contract here.
    assert!(event_kind_spec(AUDIT_EVENT_KIND).is_none());
    unknown_event_policy(AUDIT_EVENT_KIND)
        .expect("opaque audit facts are preserved without execution");
}

#[test]
fn protocol_wire_decode_refuses_missing_identity_secret_and_prompt_fields() {
    let wire = serde_json::to_value(envelope()).expect("envelope JSON");
    assert_eq!(
        AuditRecordEvent::from_wire_value(wire.clone()).expect("strict decode of a built envelope"),
        envelope()
    );

    let mut missing_actor = wire.clone();
    missing_actor["record"]
        .as_object_mut()
        .unwrap()
        .remove("actor_ref");
    assert_eq!(
        AuditRecordEvent::from_wire_value(missing_actor).unwrap_err(),
        "audit_wire_required_field_missing:record.actor_ref"
    );

    let mut missing_decision = wire.clone();
    missing_decision["record"]
        .as_object_mut()
        .unwrap()
        .remove("decision");
    assert_eq!(
        AuditRecordEvent::from_wire_value(missing_decision).unwrap_err(),
        "audit_wire_required_field_missing:record.decision"
    );

    let mut missing_cursor = wire.clone();
    missing_cursor
        .as_object_mut()
        .unwrap()
        .remove("source_cursor");
    assert_eq!(
        AuditRecordEvent::from_wire_value(missing_cursor).unwrap_err(),
        "audit_wire_required_field_missing:source_cursor"
    );

    let mut secret = wire.clone();
    secret["record"]["reason"] = json!("Authorization: Bearer sc31-wire-raw-secret");
    assert_eq!(
        AuditRecordEvent::from_wire_value(secret).unwrap_err(),
        "audit_reason_unredacted"
    );

    let mut prompt = wire.clone();
    prompt["prompt"] = json!("raw model prompt");
    assert_eq!(
        AuditRecordEvent::from_wire_value(prompt).unwrap_err(),
        "audit_wire_unknown_field:prompt"
    );

    let mut transcript = wire;
    transcript["record"]["transcript"] = json!(["raw prompt"]);
    assert_eq!(
        AuditRecordEvent::from_wire_value(transcript).unwrap_err(),
        "audit_wire_unknown_field:record.transcript"
    );
}

#[test]
fn protocol_runtime_frame_binds_redaction_recoverability_and_idempotency() {
    let record = record();
    let event = AuditRecordEvent::new(record.clone()).expect("envelope");
    let runtime = event
        .into_runtime_event(RequestId::new(), 3)
        .expect("runtime envelope");
    assert_eq!(runtime.kind, AUDIT_EVENT_KIND);
    assert_eq!(runtime.sequence, 3);
    assert_eq!(
        runtime.redaction_profile.as_deref(),
        Some(record.redaction_profile.as_str())
    );
    assert_eq!(runtime.payload_recoverable, Some(false));
    assert_eq!(runtime.data_epoch, Some(record.data_epoch));
    assert!(
        runtime.artifact_refs.is_empty(),
        "an audit fact is self-contained and never references deletable artifacts"
    );
    assert_eq!(
        runtime.idempotency_key.as_deref(),
        Some(format!("audit-record:{}", record.audit_id).as_str())
    );
    runtime
        .validate_redaction_metadata()
        .expect("redaction metadata is valid");

    let encoded = serde_json::to_value(&runtime).expect("runtime JSON");
    assert_eq!(
        serde_json::from_value::<RuntimeEvent>(encoded).expect("runtime envelope round trip"),
        runtime
    );
}
