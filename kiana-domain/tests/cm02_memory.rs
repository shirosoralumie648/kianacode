use kiana_domain::{
    EventId, MemoryAdmission, MemoryClassification, MemoryEvidence, MemoryImportMode, MemoryOrigin,
    MemoryRecord, MemorySensitivity, MemoryState, MemoryValidity, Purpose, RequestId, RunId,
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

#[test]
fn legacy_memory_is_unverifiable_until_reviewed() {
    let record = MemoryRecord::legacy_import(legacy_row()).unwrap();
    assert_eq!(record.import_mode, MemoryImportMode::LegacyImport);
    assert_eq!(record.origin, MemoryOrigin::Unknown);
    assert_eq!(record.admission_state, MemoryAdmission::Candidate);
    assert_eq!(record.state, MemoryState::Draft);
    assert_eq!(record.sensitivity, MemorySensitivity::Unknown);
    assert!(!record.searchable());
    assert_eq!(record.hit()["verified"], false);
    assert_eq!(record.hit()["provenance"], "unverifiable");
    record.validate_lifecycle().unwrap();
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

    let mut missing_origin = record.clone();
    missing_origin.origin = MemoryOrigin::Unknown;
    assert_eq!(
        missing_origin.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

    let mut missing_evidence = record.clone();
    missing_evidence.evidence.clear();
    assert_eq!(
        missing_evidence.validate_lifecycle().unwrap_err(),
        "memory_qualified_provenance_invalid"
    );

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

    let mut missing_reviewer = record.clone();
    missing_reviewer.reviewed_by = None;
    assert_eq!(
        missing_reviewer.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );

    let mut blank_reviewer = record.clone();
    blank_reviewer.reviewed_by = Some(" \t".to_owned());
    assert_eq!(
        blank_reviewer.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );

    let mut missing_review_time = record;
    missing_review_time.reviewed_at_ms = None;
    assert_eq!(
        missing_review_time.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );

    let mut zero_review_time = record;
    zero_review_time.reviewed_at_ms = Some(0);
    assert_eq!(
        zero_review_time.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
}
