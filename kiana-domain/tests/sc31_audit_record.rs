//! SC-31 audit record/envelope fixtures (deny-first).
//!
//! These fixtures pin the append-only audit schema, the redacted field contract and the
//! versioned wire decode. Local tests are not run in this session (CI-only); the lane is
//! `.github/workflows/sc31-audit-record.yml`.

use kiana_domain::{
    AuditActionKind, AuditDecision, AuditRecord, AuditRecordEvent, CompatibilityPolicy, DataClass,
    EventId, RequestId, SchemaLayer, SchemaVersion, AUDIT_EVENT_KIND, AUDIT_EVENT_SCHEMA,
    AUDIT_EVENT_SCHEMA_VERSION,
};
use serde_json::json;

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

fn envelope() -> AuditRecordEvent {
    AuditRecordEvent::new(record()).expect("SC-31 fixture envelope")
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
    let event = envelope();
    assert_eq!(event.schema, AUDIT_EVENT_SCHEMA);
    assert_eq!(event.version, AUDIT_EVENT_SCHEMA_VERSION);
    assert_eq!(event.event_kind, AUDIT_EVENT_KIND);
    assert!(event.append_only);
    event.validate().expect("envelope is valid");

    let encoded = serde_json::to_value(&event).expect("envelope JSON");
    assert_eq!(
        AuditRecordEvent::from_wire_value(encoded.clone()).expect("strict wire decode"),
        event
    );
    assert_eq!(
        serde_json::from_value::<AuditRecordEvent>(encoded.clone()).unwrap(),
        event
    );
    let mut unknown = encoded;
    unknown["unexpected"] = serde_json::json!(true);
    assert_eq!(
        AuditRecordEvent::from_wire_value(unknown).unwrap_err(),
        "audit_wire_unknown_field:unexpected"
    );

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

#[test]
fn audit_fact_without_actor_decision_reason_or_source_cursor_is_refused() {
    let wire = serde_json::to_value(envelope()).expect("envelope JSON");

    for (field, expected) in [
        (
            "actor_ref",
            "audit_wire_required_field_missing:record.actor_ref",
        ),
        (
            "decision",
            "audit_wire_required_field_missing:record.decision",
        ),
        ("reason", "audit_wire_required_field_missing:record.reason"),
        (
            "source_cursor",
            "audit_wire_required_field_missing:record.source_cursor",
        ),
        (
            "source_event_ids",
            "audit_wire_required_field_missing:record.source_event_ids",
        ),
    ] {
        let mut missing = wire.clone();
        missing["record"].as_object_mut().unwrap().remove(field);
        assert_eq!(
            AuditRecordEvent::from_wire_value(missing).unwrap_err(),
            expected,
            "missing record.{field} must be refused by name"
        );
        let mut null = wire.clone();
        null["record"][field] = json!(null);
        assert_eq!(
            AuditRecordEvent::from_wire_value(null).unwrap_err(),
            expected,
            "record.{field} present but null counts as missing"
        );
    }

    for (field, expected) in [
        (
            "source_cursor",
            "audit_wire_required_field_missing:source_cursor",
        ),
        (
            "source_event_ids",
            "audit_wire_required_field_missing:source_event_ids",
        ),
        ("record", "audit_wire_required_field_missing:record"),
    ] {
        let mut missing = wire.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert_eq!(
            AuditRecordEvent::from_wire_value(missing).unwrap_err(),
            expected,
            "missing envelope {field} must be refused by name"
        );
    }

    // A blank but present value is a shape error, not a silently normalized default.
    let mut blank_actor = wire.clone();
    blank_actor["record"]["actor_ref"] = json!("   ");
    assert_eq!(
        AuditRecordEvent::from_wire_value(blank_actor).unwrap_err(),
        "audit_actor_ref_required"
    );
    let mut blank_reason = wire.clone();
    blank_reason["record"]["reason"] = json!("   ");
    assert_eq!(
        AuditRecordEvent::from_wire_value(blank_reason).unwrap_err(),
        "audit_reason_required"
    );
    let mut empty_sources = wire.clone();
    empty_sources["record"]["source_event_ids"] = json!([]);
    assert_eq!(
        AuditRecordEvent::from_wire_value(empty_sources).unwrap_err(),
        "observability_source_event_ids_required"
    );
    let mut unknown_decision = wire;
    unknown_decision["record"]["decision"] = json!("maybe");
    assert_eq!(
        AuditRecordEvent::from_wire_value(unknown_decision).unwrap_err(),
        "audit_wire_payload_invalid"
    );
}

#[test]
fn raw_secret_or_prompt_text_never_enters_an_audit_record() {
    let encoded = serde_json::to_value(record()).expect("record JSON");
    for (field, expected) in [
        ("actor_ref", "audit_actor_ref_unredacted"),
        ("target_kind", "audit_target_kind_unredacted"),
        ("target_ref", "audit_target_ref_unredacted"),
        ("retention_class", "audit_retention_class_unredacted"),
        ("reason", "audit_reason_unredacted"),
        ("reason_code", "audit_reason_code_unredacted"),
        ("correlation_ref", "audit_correlation_ref_unredacted"),
        ("causation_ref", "audit_causation_ref_unredacted"),
    ] {
        let mut value = encoded.clone();
        value[field] = json!("Authorization: Bearer sc31-raw-secret");
        let mut record: AuditRecord = serde_json::from_value(value).expect("record decodes");
        record.record_digest = record.digest();
        assert_eq!(
            record.validate().unwrap_err(),
            expected,
            "{field} must refuse raw secret material"
        );
    }

    let mut key = encoded.clone();
    key["attributes"] = json!({"token=sc31-raw-secret": "ok"});
    let key: AuditRecord = serde_json::from_value(key).expect("record decodes");
    assert_eq!(
        key.validate().unwrap_err(),
        "audit_attribute_key_unredacted"
    );

    let mut value = encoded.clone();
    value["attributes"] = json!({"note": "token=sc31-raw-secret"});
    let value: AuditRecord = serde_json::from_value(value).expect("record decodes");
    assert_eq!(
        value.validate().unwrap_err(),
        "audit_attribute_value_unredacted"
    );
}

#[test]
fn prompt_and_transcript_fields_cannot_be_smuggled_into_an_audit_envelope() {
    let encoded = serde_json::to_value(envelope()).expect("envelope JSON");
    for field in ["prompt", "transcript", "messages", "content", "raw_payload"] {
        let mut envelope = encoded.clone();
        envelope[field] = json!("raw model prompt");
        assert_eq!(
            AuditRecordEvent::from_wire_value(envelope).unwrap_err(),
            format!("audit_wire_unknown_field:{field}"),
            "envelope key {field} must be refused"
        );

        let mut nested = encoded.clone();
        nested["record"][field] = json!("raw model prompt");
        assert_eq!(
            AuditRecordEvent::from_wire_value(nested).unwrap_err(),
            format!("audit_wire_unknown_field:record.{field}"),
            "record key {field} must be refused"
        );

        // The struct-level `deny_unknown_fields` fence refuses the same keys directly.
        let mut direct = encoded.clone();
        direct["record"][field] = json!("raw model prompt");
        assert!(serde_json::from_value::<AuditRecordEvent>(direct).is_err());
    }
}

#[test]
fn audit_event_versions_follow_the_registered_contract() {
    let contract = kiana_domain::schema_contract(AUDIT_EVENT_SCHEMA).expect("registered contract");
    assert_eq!(contract.version, AUDIT_EVENT_SCHEMA_VERSION);
    assert_eq!(contract.layer, SchemaLayer::RuntimeEvent);
    assert_eq!(contract.owner_crate, "kiana-domain");
    assert_eq!(
        contract.compatibility,
        CompatibilityPolicy::BackwardCompatible
    );
    assert!(!contract.allow_unknown_fields);
    assert_eq!(
        kiana_domain::schema_contract(kiana_domain::AUDIT_RECORD_SCHEMA)
            .expect("registered record contract")
            .version,
        kiana_domain::AUDIT_RECORD_SCHEMA_VERSION
    );

    let mut minor = envelope();
    minor.version = SchemaVersion::new(1, 1);
    minor.event_digest = minor.digest();
    minor.validate().expect("minor bump stays compatible");
    kiana_domain::check_schema_compatibility(AUDIT_EVENT_SCHEMA, &minor.version)
        .expect("registered contract accepts a minor bump");

    let mut major = envelope();
    major.version = SchemaVersion::new(2, 0);
    major.event_digest = major.digest();
    assert_eq!(
        major.validate().unwrap_err(),
        "audit_event_schema_version_incompatible"
    );
    assert!(kiana_domain::check_schema_compatibility(AUDIT_EVENT_SCHEMA, &major.version).is_err());

    let mut wrong_schema = envelope();
    wrong_schema.schema = "kiana.audit-event.v2".to_owned();
    wrong_schema.event_digest = wrong_schema.digest();
    assert_eq!(
        wrong_schema.validate().unwrap_err(),
        "audit_event_schema_mismatch"
    );

    let mut record_major = record();
    record_major.version = SchemaVersion::new(2, 0);
    record_major.record_digest = record_major.digest();
    assert!(record_major
        .validate()
        .unwrap_err()
        .starts_with("schema_version_incompatible:"));

    let mut wire = serde_json::to_value(envelope()).expect("envelope JSON");
    wire["version"] = json!({"major": 2, "minor": 0});
    assert_eq!(
        AuditRecordEvent::from_wire_value(wire).unwrap_err(),
        "audit_event_schema_version_incompatible"
    );
}
