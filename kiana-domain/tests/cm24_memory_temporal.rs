use kiana_domain::{
    json_digest, resolve_memory_history, AuthenticatedPrincipalRef, EventId, MemoryAccessPath,
    MemoryAclRequest, MemoryAdmission, MemoryClassification, MemoryCollection, MemoryEvidence,
    MemoryOrigin, MemoryRecord, MemoryScope, MemorySensitivity, MemoryState, MemoryTemporalStatus,
    MemoryValidity, ProjectIdentity, Purpose, RequestId,
};

fn digest(value: &str) -> String {
    json_digest(&serde_json::json!({"value": value}))
}

fn scope() -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new(
            "/tmp/cm24-project",
            "/tmp/cm24-project",
            Some(1),
            Some(2),
            "trust",
        )
        .unwrap(),
        "session-cm24",
        vec![MemoryCollection::parse("project:code").unwrap()],
        Purpose {
            id: "context.read".to_owned(),
            description: "CM-24 fixture".to_owned(),
        },
        false,
    )
    .unwrap()
}

fn record(id: &str, created_at_ms: u64, kind: &str, supersedes: Option<&str>) -> MemoryRecord {
    let record = MemoryRecord {
        project_root: "/tmp/cm24-project".to_owned(),
        schema: "kiana.memory-record.v2".to_owned(),
        id: id.to_owned(),
        layer: "project".to_owned(),
        collection: "project:code".to_owned(),
        text: format!("memory {id}"),
        source: format!("event:{id}"),
        role_id: String::new(),
        department_id: String::new(),
        session_id: "session-cm24".to_owned(),
        created_at_ms,
        kind: kind.to_owned(),
        evidence: vec![MemoryEvidence {
            event_id: EventId::new(),
            request_id: RequestId::new(),
            run_id: None,
            quote: "fixture evidence".to_owned(),
        }],
        content_hash: digest(id),
        supersedes: supersedes.map(str::to_owned),
        origin: MemoryOrigin::User,
        admission_state: MemoryAdmission::Qualified,
        state: MemoryState::Active,
        classification: MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "context.read".to_owned(),
            description: "CM-24 fixture".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        validity: MemoryValidity::default(),
        retention: None,
        dependencies: Vec::new(),
        import_mode: Default::default(),
        revision: 1,
        last_mutation_key: None,
        reviewed_by: Some("operator".to_owned()),
        review_reason: Some("fixture".to_owned()),
        reviewed_at_ms: Some(1),
    };
    record.validate_lifecycle().unwrap();
    record
}

fn request() -> MemoryAclRequest {
    MemoryAclRequest::new(
        scope(),
        MemoryAccessPath::Search,
        MemorySensitivity::Restricted,
        100,
        2,
    )
    .unwrap()
}

#[test]
fn as_of_returns_only_valid_history() {
    let old = record("old", 10, "timezone", None);
    let successor = record("new", 20, "timezone", Some("old"));
    let historical =
        resolve_memory_history(&[old.clone(), successor.clone()], &request(), 15).unwrap();
    assert_eq!(historical.records.len(), 1);
    assert_eq!(historical.records[0].record_id, "old");
    assert_eq!(
        historical.records[0].status,
        MemoryTemporalStatus::Historical
    );
    assert!(historical
        .omitted
        .iter()
        .any(|item| item.record_id == "new"));

    let current = resolve_memory_history(&[old, successor], &request(), 25).unwrap();
    assert_eq!(current.records.len(), 1);
    assert_eq!(current.records[0].record_id, "new");
    assert!(current
        .omitted
        .iter()
        .any(|item| item.reason == "superseded_by:new"));
}

#[test]
fn conflicting_memories_remain_explicit() {
    let left = record("left", 10, "decision", None);
    let right = record("right", 11, "decision", None);
    let selection = resolve_memory_history(&[left, right], &request(), 20).unwrap();
    assert_eq!(selection.records.len(), 2);
    assert!(selection
        .records
        .iter()
        .all(|record| record.status == MemoryTemporalStatus::Conflict));
    assert_eq!(
        selection.conflict_sets,
        vec![vec!["left".to_owned(), "right".to_owned()]]
    );
    selection.validate().unwrap();
}
