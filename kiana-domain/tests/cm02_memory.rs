use kiana_domain::{
    MemoryAdmission, MemoryClassification, MemoryImportMode, MemoryOrigin, MemoryRecord,
    MemorySensitivity, MemoryState, MemoryValidity,
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
        admission_state: MemoryAdmission::Qualified,
        state: MemoryState::Active,
        classification: MemoryClassification::Project,
        ..MemoryRecord::default()
    };
    assert_eq!(
        record.validate_lifecycle().unwrap_err(),
        "memory_active_qualification_incomplete"
    );
}
