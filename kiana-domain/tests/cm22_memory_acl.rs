use kiana_domain::{
    AuthenticatedPrincipalRef, EventId, MemoryAccessPath, MemoryAclDecision, MemoryAclRequest,
    MemoryAdmission, MemoryClassification, MemoryCollection, MemoryEvidence, MemoryOrigin,
    MemoryRecord, MemoryScope, MemorySensitivity, MemoryState, MemoryValidity, ProjectIdentity,
    Purpose, RequestId,
};

fn project(root: &str) -> ProjectIdentity {
    ProjectIdentity::new(root, root, Some(1), Some(2), "trust").unwrap()
}

fn scope(collection: &str, session: &str) -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        project("/tmp/cm22-project"),
        session,
        vec![MemoryCollection::parse(collection).unwrap()],
        Purpose {
            id: "context.read".to_owned(),
            description: "CM-22 fixture".to_owned(),
        },
        false,
    )
    .unwrap()
}

fn record(
    id: &str,
    collection: &str,
    root: &str,
    admission: MemoryAdmission,
    state: MemoryState,
    sensitivity: MemorySensitivity,
    session: &str,
) -> MemoryRecord {
    let collection_value = MemoryCollection::parse(collection).unwrap();
    let qualified = admission == MemoryAdmission::Qualified;
    let mut record = MemoryRecord {
        project_root: root.to_owned(),
        schema: "kiana.memory-record.v2".to_owned(),
        id: id.to_owned(),
        layer: collection_value.layer.clone(),
        collection: collection_value.collection,
        text: format!("memory {id}"),
        source: "event:cm22".to_owned(),
        role_id: String::new(),
        department_id: String::new(),
        session_id: session.to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        evidence: if qualified {
            vec![MemoryEvidence {
                event_id: EventId::new(),
                request_id: RequestId::new(),
                run_id: None,
                quote: "fixture evidence".to_owned(),
            }]
        } else {
            Vec::new()
        },
        content_hash: String::new(),
        supersedes: None,
        origin: MemoryOrigin::User,
        admission_state: admission,
        state,
        classification: MemoryClassification::for_collection(&collection_value),
        purpose: Some(Purpose {
            id: "context.read".to_owned(),
            description: "CM-22 fixture".to_owned(),
        }),
        sensitivity,
        validity: MemoryValidity::default(),
        retention: None,
        dependencies: Vec::new(),
        import_mode: Default::default(),
        revision: 1,
        last_mutation_key: None,
        reviewed_by: if qualified {
            Some("operator".to_owned())
        } else {
            None
        },
        review_reason: if qualified {
            Some("fixture".to_owned())
        } else {
            None
        },
        reviewed_at_ms: if qualified { Some(1) } else { None },
    };
    record.validate_lifecycle().unwrap();
    record
}

fn request(
    scope: MemoryScope,
    path: MemoryAccessPath,
    ceiling: MemorySensitivity,
) -> MemoryAclRequest {
    MemoryAclRequest::new(scope, path, ceiling, 100, 2).unwrap()
}

#[test]
fn memory_acl_holds_in_search_review_and_resume() {
    let scope = scope("project:code", "session-cm22");
    let active = record(
        "active",
        "project:code",
        "/tmp/cm22-project",
        MemoryAdmission::Qualified,
        MemoryState::Active,
        MemorySensitivity::Internal,
        "session-cm22",
    );
    let search = MemoryAclDecision::evaluate(
        &request(
            scope.clone(),
            MemoryAccessPath::Search,
            MemorySensitivity::Restricted,
        ),
        &active,
    )
    .unwrap();
    assert!(search.allowed, "{}", search.reason);

    let candidate = record(
        "candidate",
        "project:code",
        "/tmp/cm22-project",
        MemoryAdmission::Candidate,
        MemoryState::Draft,
        MemorySensitivity::Internal,
        "session-cm22",
    );
    let search_candidate = MemoryAclDecision::evaluate(
        &request(
            scope.clone(),
            MemoryAccessPath::Search,
            MemorySensitivity::Restricted,
        ),
        &candidate,
    )
    .unwrap();
    assert!(!search_candidate.allowed);
    assert_eq!(search_candidate.reason, "lifecycle_denied");
    let review_candidate = MemoryAclDecision::evaluate(
        &request(
            scope.clone(),
            MemoryAccessPath::ReviewList,
            MemorySensitivity::Restricted,
        ),
        &candidate,
    )
    .unwrap();
    assert!(review_candidate.allowed);

    let resumed = MemoryAclDecision::evaluate(
        &request(
            scope,
            MemoryAccessPath::Resume,
            MemorySensitivity::Restricted,
        ),
        &active,
    )
    .unwrap();
    assert!(resumed.allowed);
}

#[test]
fn collection_label_cannot_grant_access() {
    let scope = scope("project:code", "session-cm22");
    let private = record(
        "private",
        "user-private",
        "/tmp/cm22-project",
        MemoryAdmission::Qualified,
        MemoryState::Active,
        MemorySensitivity::Internal,
        "session-cm22",
    );
    let private_decision = MemoryAclDecision::evaluate(
        &request(
            scope.clone(),
            MemoryAccessPath::Search,
            MemorySensitivity::Restricted,
        ),
        &private,
    )
    .unwrap();
    assert_eq!(private_decision.reason, "collection_denied");

    let foreign = record(
        "foreign",
        "project:code",
        "/tmp/foreign-project",
        MemoryAdmission::Qualified,
        MemoryState::Active,
        MemorySensitivity::Internal,
        "session-cm22",
    );
    let foreign_decision = MemoryAclDecision::evaluate(
        &request(
            scope.clone(),
            MemoryAccessPath::Citation,
            MemorySensitivity::Restricted,
        ),
        &foreign,
    )
    .unwrap();
    assert_eq!(foreign_decision.reason, "project_denied");

    let sensitive = record(
        "sensitive",
        "project:code",
        "/tmp/cm22-project",
        MemoryAdmission::Qualified,
        MemoryState::Active,
        MemorySensitivity::Internal,
        "session-cm22",
    );
    let sensitivity_decision = MemoryAclDecision::evaluate(
        &request(
            scope,
            MemoryAccessPath::AutoPrefetch,
            MemorySensitivity::Public,
        ),
        &sensitive,
    )
    .unwrap();
    assert_eq!(sensitivity_decision.reason, "sensitivity_denied");
}
