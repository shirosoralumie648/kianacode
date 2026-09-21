use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, EventId, EvidenceStatus, MemoryAdmission,
    MemoryBodyRef, MemoryCollection, MemoryEvidence, MemoryJournalFact, MemoryMutation,
    MemoryMutationAuthority, MemoryMutationJournal, MemoryMutationJournalStage,
    MemoryMutationOperation, MemoryMutationTarget, MemoryOrigin, MemoryRecord, MemoryScope,
    MemorySensitivity, MemoryState, ProjectIdentity, Purpose, RequestId, RuntimeEvent, SourceKind,
    SourceRef, MEMORY_FACT_EVENT_KIND, MEMORY_RECORD_SCHEMA_V2, MEMORY_STREAM,
};
use serde_json::json;
use sha2::Digest;

fn scope() -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new("/tmp/pd17", "/tmp/pd17", Some(1), Some(2), "trust").unwrap(),
        "pd17-session",
        vec![MemoryCollection::parse("project:code").unwrap()],
        Purpose {
            id: "memory.mutation".to_owned(),
            description: "PD-17 fixture".to_owned(),
        },
        true,
    )
    .unwrap()
}

fn evidence() -> Vec<SourceRef> {
    vec![SourceRef::new(
        "event:pd17",
        SourceKind::Event,
        "event://pd17",
        "cursor:1",
        json_digest(&json!({"fixture": "pd17"})),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap()]
}

fn record(id: &str, text: &str, revision: u64, key: &str) -> MemoryRecord {
    MemoryRecord {
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: id.to_owned(),
        layer: "project".to_owned(),
        collection: "project:code".to_owned(),
        text: text.to_owned(),
        source: "event:pd17".to_owned(),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "pd17-session".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        content_hash: format!("{:x}", sha2::Sha256::digest(text.as_bytes())),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        classification: kiana_domain::MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "memory.candidate".to_owned(),
            description: "PD-17 candidate".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        revision,
        last_mutation_key: Some(key.to_owned()),
        ..MemoryRecord::default()
    }
}

fn mutation(
    operation: MemoryMutationOperation,
    expected_revision: u64,
    key: &str,
) -> MemoryMutation {
    MemoryMutation::new(
        format!("mutation:{key}"),
        operation,
        "local-user",
        scope(),
        vec![MemoryMutationTarget::new(
            "memory-1",
            "project:code",
            expected_revision,
        )],
        evidence(),
        1,
        1,
        key,
        &json!({"key": key}),
    )
    .unwrap()
}

fn event(stream: &str, version: u64, key: &str, fact: MemoryJournalFact) -> RuntimeEvent {
    RuntimeEvent::new(
        RequestId::new(),
        1,
        MEMORY_FACT_EVENT_KIND,
        serde_json::to_value(fact).unwrap(),
    )
    .unwrap()
    .with_stream_metadata(MEMORY_STREAM, stream, version)
    .with_idempotency_key(key)
}

#[test]
fn mutation_journal_rebuild_preserves_stage_and_record_state() {
    let stream = "sha256:pd17-stream";
    let candidate_key = "pd17:candidate";
    let candidate = record("memory-1", "candidate", 1, candidate_key);
    let candidate_fact = MemoryJournalFact::new(
        "write",
        candidate_key,
        candidate.clone(),
        MemoryBodyRef::new(stream, candidate.content_hash.clone()),
    )
    .with_mutation(
        mutation(MemoryMutationOperation::Add, 0, candidate_key),
        MemoryMutationJournalStage::Candidate,
    )
    .unwrap();

    let approve_key = "pd17:approve";
    let mut approved = record("memory-1", "candidate", 2, approve_key);
    approved.admission_state = MemoryAdmission::Qualified;
    approved.state = MemoryState::Active;
    approved.evidence = vec![MemoryEvidence {
        event_id: EventId::new(),
        request_id: RequestId::new(),
        run_id: None,
        quote: "human review".to_owned(),
    }];
    approved.purpose = Some(Purpose {
        id: "memory.review".to_owned(),
        description: "PD-17 approval".to_owned(),
    });
    approved.reviewed_by = Some("local-user".to_owned());
    approved.reviewed_at_ms = Some(2);
    approved.last_mutation_key = Some(approve_key.to_owned());
    let approve_mutation = mutation(MemoryMutationOperation::Approve, 1, approve_key)
        .with_authority(MemoryMutationAuthority::Human)
        .unwrap();
    let approve_fact = MemoryJournalFact::new(
        "review",
        approve_key,
        approved.clone(),
        MemoryBodyRef::new(stream, approved.content_hash.clone()),
    )
    .with_mutation(approve_mutation, MemoryMutationJournalStage::Approve)
    .unwrap();

    let projection = kiana_domain::project_memory_facts(&[
        event(stream, 1, "fact:pd17:candidate", candidate_fact),
        event(stream, 2, "fact:pd17:approve", approve_fact),
    ])
    .unwrap();
    assert_eq!(projection.source_cursor, 2);
    assert_eq!(projection.mutation_digests.len(), 2);
    assert_eq!(projection.records["memory-1"]["state"], json!("active"));
}

#[test]
fn agent_cannot_commit_approval_or_scope_superset() {
    let agent_approval = mutation(MemoryMutationOperation::Approve, 1, "pd17:agent-approve");
    assert_eq!(
        MemoryMutationJournal::new(agent_approval, MemoryMutationJournalStage::Approve)
            .unwrap_err(),
        "memory_mutation_journal_approval_invalid"
    );

    let invalid_scope = MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new("/tmp/pd17", "/tmp/pd17", Some(1), Some(2), "trust").unwrap(),
        "pd17-session",
        vec![MemoryCollection::parse("project:code").unwrap()],
        Purpose {
            id: "memory.mutation".to_owned(),
            description: "PD-17 fixture".to_owned(),
        },
        true,
    )
    .unwrap();
    let out_of_scope = MemoryMutation::new(
        "mutation:pd17:scope",
        MemoryMutationOperation::Add,
        "local-user",
        invalid_scope,
        vec![MemoryMutationTarget::new(
            "memory-1",
            "department:planning",
            0,
        )],
        evidence(),
        1,
        1,
        "pd17:scope",
        &json!({"scope": "widened"}),
    );
    assert_eq!(out_of_scope.unwrap_err(), "memory_mutation_scope_denied");
}

#[test]
fn duplicate_mutation_key_is_rejected_during_rebuild() {
    let stream = "sha256:pd17-duplicate";
    let key = "pd17:duplicate";
    let candidate = record("memory-1", "candidate", 1, key);
    let fact = MemoryJournalFact::new(
        "write",
        key,
        candidate.clone(),
        MemoryBodyRef::new(stream, candidate.content_hash.clone()),
    )
    .with_mutation(
        mutation(MemoryMutationOperation::Add, 0, key),
        MemoryMutationJournalStage::Candidate,
    )
    .unwrap();
    let first = event(stream, 1, "fact:pd17:first", fact.clone());
    let second = event(stream, 2, "fact:pd17:second", fact);
    assert_eq!(
        kiana_domain::project_memory_facts(&[first, second]).unwrap_err(),
        "memory_mutation_journal_duplicate"
    );
}
