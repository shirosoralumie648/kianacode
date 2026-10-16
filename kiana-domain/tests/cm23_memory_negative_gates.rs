use kiana_domain::{
    json_digest, MemoryAdmission, MemoryCollection, MemoryModelWriteGate, MemoryOrigin,
    MemoryRecord, MemorySensitivity, MemoryState, MemoryVisibility, MEMORY_RECORD_SCHEMA,
    MEMORY_RECORD_SCHEMA_V2,
};
use serde_json::json;

fn record_from_gate(gate: &MemoryModelWriteGate, id: &str) -> MemoryRecord {
    MemoryRecord {
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: id.to_owned(),
        text: "Model proposed fixture memory".to_owned(),
        kind: "note".to_owned(),
        layer: gate.collection.layer.clone(),
        collection: gate.collection.collection.clone(),
        session_id: gate.session_id.clone(),
        origin: gate.origin,
        admission_state: gate.admission_state,
        state: gate.state,
        classification: gate.classification,
        sensitivity: MemorySensitivity::Internal,
        revision: 1,
        ..MemoryRecord::default()
    }
}

#[test]
fn candidate_never_appears_before_approval() {
    let gate = MemoryModelWriteGate::derive(
        MemoryCollection::parse("project:code").unwrap(),
        "session-cm23",
    )
    .unwrap();
    gate.validate().unwrap();
    assert_eq!(gate.origin, MemoryOrigin::Model);
    assert_eq!(gate.admission_state, MemoryAdmission::Candidate);
    assert_eq!(gate.state, MemoryState::Draft);
    assert!(gate.operator_approval_required);

    let record = record_from_gate(&gate, "memory:cm23-candidate");
    record.validate_lifecycle().unwrap();
    assert_eq!(record.visibility(), MemoryVisibility::ReviewOnly);
    assert!(!record.searchable());

    let mut legacy = record;
    legacy.schema = MEMORY_RECORD_SCHEMA.to_owned();
    assert_eq!(
        legacy.validate_lifecycle().unwrap_err(),
        "memory_record_legacy_import_required"
    );
    assert_eq!(legacy.visibility(), MemoryVisibility::Denied);
    assert!(!legacy.searchable());
}

#[test]
fn scratch_does_not_survive_session_retirement() {
    let gate = MemoryModelWriteGate::derive(
        MemoryCollection::parse("instance-scratch").unwrap(),
        "session-cm23",
    )
    .unwrap();
    assert_eq!(gate.admission_state, MemoryAdmission::Ephemeral);
    assert_eq!(gate.state, MemoryState::Active);
    let record = record_from_gate(&gate, "memory:cm23-scratch");
    record.validate_lifecycle().unwrap();
    assert!(record.visible_in_session("session-cm23"));
    assert!(!record.visible_in_session("retired-session"));
}

#[test]
fn model_cannot_self_approve_private_memory() {
    assert_eq!(
        MemoryModelWriteGate::derive(
            MemoryCollection::parse("user-private").unwrap(),
            "session-cm23",
        )
        .unwrap_err(),
        "memory_user_private_requires_operator"
    );
    for field in [
        "actor",
        "origin",
        "classification",
        "admission_state",
        "state",
        "reviewed_by",
        "operator_approval_ref",
    ] {
        let mut payload = serde_json::Map::new();
        payload.insert(
            field.to_owned(),
            if field == "actor" {
                json_digest(&json!("model")).into()
            } else {
                "forged".into()
            },
        );
        let error =
            MemoryModelWriteGate::reject_model_overrides(&serde_json::Value::Object(payload))
                .unwrap_err();
        assert!(error.contains(field), "{error}");
    }
}
