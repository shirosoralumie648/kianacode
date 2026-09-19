use kiana_domain::{
    json_digest, MemoryAdmission, MemoryClassification, MemoryCollection, MemoryModelWriteGate,
    MemoryOrigin, MemoryRecord, MemorySensitivity, MemoryState, MemoryVisibility,
};
use serde_json::json;

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

    let record = MemoryRecord {
        layer: "project".to_owned(),
        collection: "project:code".to_owned(),
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        ..MemoryRecord::default()
    };
    assert_eq!(record.visibility(), MemoryVisibility::ReviewOnly);
    assert!(!record.searchable());
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
    let record = MemoryRecord {
        layer: "instance-scratch".to_owned(),
        collection: "instance-scratch".to_owned(),
        session_id: "session-cm23".to_owned(),
        admission_state: MemoryAdmission::Ephemeral,
        state: MemoryState::Active,
        origin: MemoryOrigin::Model,
        classification: MemoryClassification::Scratch,
        sensitivity: MemorySensitivity::Internal,
        ..MemoryRecord::default()
    };
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
