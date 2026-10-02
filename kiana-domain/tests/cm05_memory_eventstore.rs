use kiana_domain::{
    project_memory_facts, MemoryAdmission, MemoryBodyRef, MemoryJournalFact, MemoryRecord,
    MemoryState, RuntimeEvent, MEMORY_FACT_EVENT_KIND, MEMORY_RECORD_SCHEMA_V2, MEMORY_STREAM,
};
use serde_json::json;
use sha2::Digest;

fn record(id: &str, text: &str, revision: u64) -> MemoryRecord {
    MemoryRecord {
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: id.to_owned(),
        layer: "project".to_owned(),
        collection: "project".to_owned(),
        text: text.to_owned(),
        source: "event:fixture".to_owned(),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "session-1".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        content_hash: format!("{:x}", sha2::Sha256::digest(text.as_bytes())),
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        classification: kiana_domain::MemoryClassification::Project,
        purpose: Some(kiana_domain::Purpose {
            id: "memory.candidate".to_owned(),
            description: "fixture".to_owned(),
        }),
        sensitivity: kiana_domain::MemorySensitivity::Internal,
        revision,
        last_mutation_key: Some(format!("memory:{id}:{revision}")),
        ..MemoryRecord::default()
    }
}

fn event(
    request_id: kiana_domain::RequestId,
    event_id: kiana_domain::EventId,
    stream_id: &str,
    version: u64,
    operation: &str,
    key: &str,
    record: MemoryRecord,
) -> RuntimeEvent {
    let fact = MemoryJournalFact::new(
        operation,
        key,
        record.clone(),
        MemoryBodyRef::new(stream_id, record.content_hash.clone()),
    );
    RuntimeEvent {
        event_id,
        ..RuntimeEvent::new(
            request_id,
            1,
            MEMORY_FACT_EVENT_KIND,
            serde_json::to_value(fact).expect("fact serializes"),
        )
        .expect("event")
        .with_stream_metadata(MEMORY_STREAM, stream_id, version)
        .with_idempotency_key(key)
    }
}

#[test]
fn memory_commit_has_no_unjournaled_visibility() {
    let request_id = kiana_domain::RequestId::new();
    let stream = "sha256:memory-fixture";
    let first = record("memory-1", "first", 1);
    let second = record("memory-1", "second", 2);
    let events = vec![
        event(
            request_id,
            kiana_domain::EventId::new(),
            stream,
            1,
            "write",
            "memory.fact:1",
            first,
        ),
        event(
            request_id,
            kiana_domain::EventId::new(),
            stream,
            2,
            "review",
            "memory.fact:2",
            second,
        ),
    ];
    let projection = project_memory_facts(&events).expect("committed stream projects");
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.records.len(), 1);
    assert_eq!(projection.records["memory-1"]["text"], json!("second"));

    let mut duplicate = events.clone();
    duplicate.push(duplicate[1].clone());
    assert_eq!(
        project_memory_facts(&duplicate).unwrap_err(),
        "memory_stream_version_invalid"
    );
}

#[test]
fn projection_rebuild_matches_committed_memory() {
    let request_id = kiana_domain::RequestId::new();
    let stream = "sha256:memory-rebuild";
    let current = record("memory-1", "current", 1);
    let events = vec![event(
        request_id,
        kiana_domain::EventId::new(),
        stream,
        1,
        "write",
        "memory.fact:rebuild",
        current.clone(),
    )];
    let projection = project_memory_facts(&events).expect("projection rebuilds");
    assert!(projection.matches_records(std::slice::from_ref(&current)));

    let stale = record("memory-1", "stale", 1);
    assert!(!projection.matches_records(std::slice::from_ref(&stale)));

    let mut forged = events[0].clone();
    forged.data["body_ref"]["content_hash"] = json!("0".repeat(64));
    assert_eq!(
        project_memory_facts(&[forged]).unwrap_err(),
        "memory_body_ref_hash_mismatch"
    );

    let mut gap = events[0].clone();
    gap.stream_version = Some(2);
    assert_eq!(
        project_memory_facts(&[gap]).unwrap_err(),
        "memory_stream_version_invalid"
    );
}

#[test]
fn memory_event_projection_rejects_schema_v1_native_qualified_record() {
    let stream = "sha256:cm02-v1-native";
    let mut record = record("memory-v1-native", "qualified legacy", 1);
    record.schema = kiana_domain::MEMORY_RECORD_SCHEMA.to_owned();
    record.evidence = vec![kiana_domain::MemoryEvidence {
        event_id: kiana_domain::EventId::new(),
        request_id: kiana_domain::RequestId::new(),
        run_id: None,
        quote: "qualified legacy".to_owned(),
    }];
    record.origin = kiana_domain::MemoryOrigin::User;
    record.admission_state = MemoryAdmission::Qualified;
    record.state = MemoryState::Active;
    record.reviewed_by = Some("operator".to_owned());
    record.reviewed_at_ms = Some(1);

    let fact = MemoryJournalFact::new(
        "write",
        "memory.fact:cm02-v1-native",
        record.clone(),
        MemoryBodyRef::new(stream, record.content_hash.clone()),
    );
    let event = RuntimeEvent::new(
        kiana_domain::RequestId::new(),
        1,
        MEMORY_FACT_EVENT_KIND,
        serde_json::to_value(fact).unwrap(),
    )
    .unwrap()
    .with_stream_metadata(MEMORY_STREAM, stream, 1)
    .with_idempotency_key("memory.fact:cm02-v1-native");

    assert_eq!(
        project_memory_facts(&[event]).unwrap_err(),
        "memory_record_legacy_import_required"
    );
}
