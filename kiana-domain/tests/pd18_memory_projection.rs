use kiana_domain::{
    DataClass, DataPolicy, EventId, MemoryAdmission, MemoryEvidence, MemoryOrigin,
    MemoryProjectionDisposition, MemoryProjectionFence, MemoryRecord, MemorySensitivity,
    MemoryState, ProcessingGrant, Purpose, RequestId, Retention,
};

fn grant(expiry: Option<u64>) -> ProcessingGrant {
    ProcessingGrant {
        id: "grant-1".to_owned(),
        source_path: "src/input.txt".to_owned(),
        content_hash: format!("sha256:{}", "a".repeat(64)),
        class: DataClass::Internal,
        purpose: Purpose {
            id: "memory.search".to_owned(),
            description: "PD-18 projection".to_owned(),
        },
        retention: Retention {
            expires_at_ms: expiry,
            retain_audit_metadata: true,
        },
        parent_ids: Vec::new(),
        created_by: "local-user".to_owned(),
        revoked: false,
    }
}

fn active_record(project_root: &str) -> MemoryRecord {
    MemoryRecord {
        project_root: project_root.to_owned(),
        schema: kiana_domain::MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: "memory-1".to_owned(),
        layer: "project".to_owned(),
        collection: "project:code".to_owned(),
        text: "approved fact".to_owned(),
        source: "src/input.txt".to_owned(),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "pd18-session".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Qualified,
        state: MemoryState::Active,
        classification: kiana_domain::MemoryClassification::Project,
        purpose: Some(Purpose {
            id: "memory.review".to_owned(),
            description: "approved fact".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        evidence: vec![MemoryEvidence {
            event_id: EventId::new(),
            request_id: RequestId::new(),
            run_id: None,
            quote: "approved".to_owned(),
        }],
        reviewed_by: Some("local-user".to_owned()),
        reviewed_at_ms: Some(2),
        ..MemoryRecord::default()
    }
}

#[test]
fn active_memory_is_fenced_by_epoch_scope_and_policy() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();
    let fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Searchable);
    assert_eq!(fence.reason, "allowed");
}

#[test]
fn candidate_project_mismatch_revocation_and_retention_never_search() {
    let mut policy = DataPolicy::default();
    policy.register(grant(Some(10))).unwrap();

    let mut candidate = active_record("/repo");
    candidate.admission_state = MemoryAdmission::Candidate;
    candidate.state = MemoryState::Draft;
    candidate.reviewed_by = None;
    candidate.reviewed_at_ms = None;
    let candidate_fence = MemoryProjectionFence::evaluate(&candidate, &policy, 1, "/repo", 1);
    assert_eq!(candidate_fence.unwrap().reason, "candidate_not_qualified");

    let project_fence = MemoryProjectionFence::evaluate(
        &active_record("/repo-a"),
        &policy,
        policy.data_epoch,
        "/repo-b",
        1,
    )
    .unwrap();
    assert_eq!(project_fence.reason, "project_scope_denied");

    let retention_fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        policy.data_epoch,
        "/repo",
        10,
    )
    .unwrap();
    assert_eq!(
        retention_fence.reason,
        "source_revoked_or_retention_expired"
    );

    policy.revoke("grant-1").unwrap();
    let revoked_fence = MemoryProjectionFence::evaluate(
        &active_record("/repo"),
        &policy,
        policy.data_epoch,
        "/repo",
        1,
    )
    .unwrap();
    assert_eq!(revoked_fence.reason, "source_revoked_or_retention_expired");
}

#[test]
fn stale_epoch_is_visible_and_denies_projection() {
    let mut policy = DataPolicy::default();
    policy.register(grant(None)).unwrap();
    policy.revoke("grant-1").unwrap();
    let fence =
        MemoryProjectionFence::evaluate(&active_record("/repo"), &policy, 1, "/repo", 1).unwrap();
    assert_eq!(fence.disposition, MemoryProjectionDisposition::Denied);
    assert_eq!(fence.reason, "data_epoch_stale");
    assert_eq!(
        MemoryProjectionFence::validate_epoch(policy.data_epoch, 1).unwrap_err(),
        "memory_projection_data_epoch_mismatch"
    );
}
