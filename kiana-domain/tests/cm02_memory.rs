use kiana_domain::{
    project_memory_facts, EventId, MemoryAdmission, MemoryBodyRef, MemoryClassification,
    MemoryEvidence, MemoryImportMode, MemoryJournalFact, MemoryOrigin, MemoryRecord,
    MemorySensitivity, MemoryState, MemoryValidity, Purpose, RequestId, RunId, RuntimeEvent,
    MEMORY_FACT_EVENT_KIND, MEMORY_RECORD_SCHEMA_V2, MEMORY_STREAM,
};
use serde_json::json;

fn legacy_row() -> serde_json::Value {
    json!({
        "schema": "kiana.memory-record.v1",
        "id": "legacy-1",
        "layer": "project",
        "collection": "project",
        "text": "legacy note",
        "source": "old-source-string",
        "role_id": "builder",
        "department_id": "executing",
        "session_id": "session-cm02",
        "created_at_ms": 10
    })
}

fn qualified_v1_native_record() -> MemoryRecord {
    let mut raw = legacy_row();
    raw["kind"] = json!("fact");
    raw["evidence"] = json!([{
        "event_id": "00000000-0000-4000-8000-000000000001",
        "request_id": "00000000-0000-4000-8000-000000000002",
        "run_id": null,
        "quote": "qualified legacy evidence"
    }]);
    raw["origin"] = json!("user");
    raw["admission_state"] = json!("qualified");
    raw["state"] = json!("active");
    raw["purpose"] = json!({
        "id": "context.read",
        "description": "CM-02 v1 native denial fixture"
    });
    raw["sensitivity"] = json!("internal");
    raw["import_mode"] = json!("native");
    raw["reviewed_by"] = json!("operator");
    raw["reviewed_at_ms"] = json!(11);
    raw["content_hash"] = json!("a".repeat(64));
    serde_json::from_value(raw).unwrap()
}

#[test]
fn legacy_memory_is_unverifiable_until_reviewed() {
    let record = MemoryRecord::legacy_import(legacy_row()).unwrap();
    assert_eq!(record.schema, MEMORY_RECORD_SCHEMA_V2);
    assert_eq!(record.import_mode, MemoryImportMode::LegacyImport);
    assert_eq!(record.origin, MemoryOrigin::Unknown);
    assert_eq!(record.admission_state, MemoryAdmission::Candidate);
    assert_eq!(record.state, MemoryState::Draft);
    assert_eq!(record.sensitivity, MemorySensitivity::Unknown);
    assert!(!record.searchable());
    assert_eq!(record.hit()["verified"], false);
    assert_eq!(record.hit()["provenance"], "unverifiable");
    record.validate_lifecycle().unwrap();

    let upcast = kiana_domain::upcast_storage_value(legacy_row(), MEMORY_RECORD_SCHEMA_V2).unwrap();
    let upcast: MemoryRecord = serde_json::from_value(upcast).unwrap();
    assert_eq!(upcast.schema, MEMORY_RECORD_SCHEMA_V2);
    assert_eq!(upcast.import_mode, MemoryImportMode::LegacyImport);
    assert_eq!(upcast.origin, MemoryOrigin::Unknown);
    assert_eq!(upcast.admission_state, MemoryAdmission::Candidate);
    assert_eq!(upcast.state, MemoryState::Draft);
    assert!(!upcast.searchable());
    upcast.validate_lifecycle().unwrap();
}

#[test]
fn schema_v1_native_qualified_record_requires_explicit_legacy_import() {
    let record = qualified_v1_native_record();
    assert_eq!(record.import_mode, MemoryImportMode::Native);
    assert_eq!(
        record.validate_lifecycle().unwrap_err(),
        "memory_record_legacy_import_required"
    );
    assert!(!record.searchable());
}

#[test]
fn memory_record_rejects_unknown_nested_lifecycle_fields() {
    let base = || {
        json!({
            "schema": kiana_domain::MEMORY_RECORD_SCHEMA_V2,
            "id": "memory-unknown-nested",
            "layer": "project",
            "collection": "project",
            "text": "candidate",
            "source": "event:3",
            "role_id": "builder",
            "department_id": "executing",
            "session_id": "session-cm02",
            "created_at_ms": 1,
            "kind": "fact",
            "purpose": {
                "id": "context.read",
                "description": "CM-02 strict fixture"
            },
            "retention": {
                "expires_at_ms": 100,
                "retain_audit_metadata": true
            },
            "evidence": [{
                "event_id": "00000000-0000-4000-8000-000000000001",
                "request_id": "00000000-0000-4000-8000-000000000002",
                "run_id": null,
                "quote": "evidence quote"
            }]
        })
    };

    assert!(serde_json::from_value::<MemoryRecord>(base()).is_ok());

    let mut purpose = base();
    purpose["purpose"]["future"] = json!(true);
    assert!(serde_json::from_value::<MemoryRecord>(purpose).is_err());

    let mut retention = base();
    retention["retention"]["future"] = json!(true);
    assert!(serde_json::from_value::<MemoryRecord>(retention).is_err());

    let mut evidence = base();
    evidence["evidence"][0]["future"] = json!(true);
    assert!(serde_json::from_value::<MemoryRecord>(evidence).is_err());

    let mut known_legacy_evidence = legacy_row();
    known_legacy_evidence["evidence"] = json!([{
        "event_id": "00000000-0000-4000-8000-000000000001",
        "request_id": "00000000-0000-4000-8000-000000000002",
        "run_id": null,
        "quote": "legacy evidence quote"
    }]);
    assert!(MemoryRecord::legacy_import(known_legacy_evidence).is_ok());

    let mut legacy_evidence = legacy_row();
    legacy_evidence["evidence"] = json!([{
        "event_id": "00000000-0000-4000-8000-000000000001",
        "request_id": "00000000-0000-4000-8000-000000000002",
        "run_id": null,
        "quote": "legacy evidence quote",
        "future": true
    }]);
    assert_eq!(
        MemoryRecord::legacy_import(legacy_evidence).unwrap_err(),
        "memory_legacy_import_invalid"
    );
}

#[test]
fn invalid_admission_state_combination_is_denied() {
    let record = MemoryRecord {
        schema: kiana_domain::MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: "memory-2".to_owned(),
        layer: "project".to_owned(),
        collection: "project".to_owned(),
        text: "candidate".to_owned(),
        source: "event:1".to_owned(),
        kind: "fact".to_owned(),
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Active,
        classification: MemoryClassification::Project,
        ..MemoryRecord::default()
    };
    assert_eq!(
        record.validate_lifecycle().unwrap_err(),
        "memory_admission_state_invalid"
    );

    let mut invalid_validity = record;
    invalid_validity.state = MemoryState::Draft;
    invalid_validity.validity = MemoryValidity {
        valid_from_ms: Some(20),
        valid_to_ms: Some(10),
    };
    assert_eq!(
        invalid_validity.validate_lifecycle().unwrap_err(),
        "memory_validity_invalid"
    );
}

#[test]
fn qualified_memory_requires_review_evidence_and_purpose() {
    let record = MemoryRecord {
        schema: kiana_domain::MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: "memory-3".to_owned(),
        layer: "project".to_owned(),
        collection: "project".to_owned(),
        text: "qualified".to_owned(),
        source: "event:2".to_owned(),
        kind: "fact".to_owned(),
        evidence: vec![MemoryEvidence {
            event_id: EventId::new(),
            request_id: RequestId::new(),
            run_id: None,
            quote: "qualified evidence".to_owned(),
        }],
        origin: MemoryOrigin::User,
        admission_state: MemoryAdmission::Qualified,
        state: MemoryState::Active,
        classification: MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "context.read".to_owned(),
            description: "CM-02 qualification fixture".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        reviewed_by: Some("operator".to_owned()),
        reviewed_at_ms: Some(1),
        ..MemoryRecord::default()
    };

    record.validate_lifecycle().unwrap();
    assert_eq!(
        record.visibility(),
        kiana_domain::MemoryVisibility::Searchable
    );
    assert!(record.searchable());
    let assert_denied = |invalid: &MemoryRecord| {
        assert_eq!(invalid.visibility(), kiana_domain::MemoryVisibility::Denied);
        assert!(!invalid.searchable());
    };

    let mut missing_origin = record.clone();
    missing_origin.origin = MemoryOrigin::Unknown;
    assert_eq!(
        missing_origin.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );
    assert_denied(&missing_origin);

    let mut missing_evidence = record.clone();
    missing_evidence.evidence.clear();
    assert_eq!(
        missing_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );
    assert_denied(&missing_evidence);

    let mut blank_evidence = record.clone();
    blank_evidence.evidence[0].quote = "  \n".to_owned();
    assert_eq!(
        blank_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

    let mut oversize_evidence = record.clone();
    oversize_evidence.evidence[0].quote =
        "x".repeat(kiana_domain::MEMORY_EXTRACTION_MAX_QUOTE_BYTES + 1);
    assert_eq!(
        oversize_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

    let mut nil_event_evidence = record.clone();
    nil_event_evidence.evidence[0].event_id =
        EventId::parse_str("00000000-0000-0000-0000-000000000000").unwrap();
    assert_eq!(
        nil_event_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

    let nil_id = "00000000-0000-0000-0000-000000000000";
    let mut nil_request_evidence = record.clone();
    nil_request_evidence.evidence[0].request_id = RequestId::parse_str(nil_id).unwrap();
    assert_eq!(
        nil_request_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

    let mut nil_run_evidence = record.clone();
    nil_run_evidence.evidence[0].run_id = Some(RunId::parse_str(nil_id).unwrap());
    assert_eq!(
        nil_run_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

    let mut missing_purpose = record.clone();
    missing_purpose.purpose = None;
    assert_eq!(
        missing_purpose.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
    assert_denied(&missing_purpose);

    let mut malformed_purpose = record.clone();
    malformed_purpose.purpose.as_mut().unwrap().id = "  ".to_owned();
    assert_eq!(
        malformed_purpose.validate_lifecycle().unwrap_err(),
        "purpose_id_invalid"
    );

    let mut missing_sensitivity = record.clone();
    missing_sensitivity.sensitivity = MemorySensitivity::Unknown;
    assert_eq!(
        missing_sensitivity.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
    assert_denied(&missing_sensitivity);

    let mut missing_reviewer = record.clone();
    missing_reviewer.reviewed_by = None;
    assert_eq!(
        missing_reviewer.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
    assert_denied(&missing_reviewer);

    let mut blank_reviewer = record.clone();
    blank_reviewer.reviewed_by = Some(" \t".to_owned());
    assert_eq!(
        blank_reviewer.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );

    let mut missing_review_time = record.clone();
    missing_review_time.reviewed_at_ms = None;
    assert_eq!(
        missing_review_time.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
    assert_denied(&missing_review_time);

    let mut zero_review_time = record;
    zero_review_time.reviewed_at_ms = Some(0);
    assert_eq!(
        zero_review_time.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
}
