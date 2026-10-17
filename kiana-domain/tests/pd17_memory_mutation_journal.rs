use kiana_domain::{
    json_digest, AuthenticatedPrincipalRef, EventId, EvidenceStatus, MemoryAdmission,
    MemoryBodyRef, MemoryCollection, MemoryEvidence, MemoryImportMode, MemoryJournalFact,
    MemoryMutation, MemoryMutationAuthority, MemoryMutationJournal, MemoryMutationJournalStage,
    MemoryMutationLedger, MemoryMutationOperation, MemoryMutationOutcome, MemoryMutationTarget,
    MemoryOrigin, MemoryRecord, MemoryScope, MemorySensitivity, MemoryState, ProjectIdentity,
    Purpose, RequestId, RuntimeEvent, SourceKind, SourceRef, MEMORY_FACT_EVENT_KIND,
    MEMORY_RECORD_SCHEMA_V2, MEMORY_STREAM,
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

const SIX_LAYER_STREAM: &str = "sha256:pd17-six-layers";

fn six_layer_scope() -> MemoryScope {
    MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        ProjectIdentity::new("/tmp/pd17", "/tmp/pd17", Some(1), Some(2), "trust").unwrap(),
        "pd17-six",
        [
            "company",
            "department:planning",
            "role:builder",
            "project:code",
            "user-private",
            "scratch",
        ]
        .into_iter()
        .map(|collection| MemoryCollection::parse(collection).unwrap())
        .collect(),
        Purpose {
            id: "memory.mutation".to_owned(),
            description: "PD-17 six-layer fixture".to_owned(),
        },
        true,
    )
    .unwrap()
}

fn layered_record(
    id: &str,
    collection: &str,
    text: &str,
    revision: u64,
    key: &str,
) -> MemoryRecord {
    let parsed = MemoryCollection::parse(collection).unwrap();
    MemoryRecord {
        project_root: "/tmp/pd17".to_owned(),
        schema: MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: id.to_owned(),
        layer: parsed.layer.clone(),
        collection: parsed.collection.clone(),
        text: text.to_owned(),
        source: format!("event:{id}"),
        role_id: "builder".to_owned(),
        department_id: "executing".to_owned(),
        session_id: "pd17-six".to_owned(),
        created_at_ms: 1,
        kind: "fact".to_owned(),
        content_hash: format!("{:x}", sha2::Sha256::digest(text.as_bytes())),
        origin: MemoryOrigin::Model,
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        classification: kiana_domain::MemoryClassification::for_collection(&parsed),
        purpose: Some(Purpose {
            id: "memory.candidate".to_owned(),
            description: "PD-17 layered fixture".to_owned(),
        }),
        sensitivity: MemorySensitivity::Internal,
        revision,
        last_mutation_key: Some(key.to_owned()),
        ..MemoryRecord::default()
    }
}

fn approved(mut record: MemoryRecord, revision: u64, reviewer: &str, key: &str) -> MemoryRecord {
    record.revision = revision;
    record.admission_state = MemoryAdmission::Qualified;
    record.state = MemoryState::Active;
    record.evidence = vec![MemoryEvidence {
        event_id: EventId::new(),
        request_id: RequestId::new(),
        run_id: None,
        quote: record.text.clone(),
    }];
    record.purpose = Some(Purpose {
        id: "memory.review".to_owned(),
        description: "PD-17 approved fixture".to_owned(),
    });
    record.reviewed_by = Some(reviewer.to_owned());
    record.reviewed_at_ms = Some(2);
    record.last_mutation_key = Some(key.to_owned());
    record
}

fn layered_mutation(
    record: &MemoryRecord,
    operation: MemoryMutationOperation,
    authority: MemoryMutationAuthority,
    expected_revision: u64,
    key: &str,
) -> MemoryMutation {
    MemoryMutation::new(
        format!("mutation:{key}"),
        operation,
        "local-user",
        six_layer_scope(),
        vec![MemoryMutationTarget::new(
            &record.id,
            &record.collection,
            expected_revision,
        )],
        evidence(),
        1,
        1,
        key,
        &json!({"key": key}),
    )
    .unwrap()
    .with_authority(authority)
    .unwrap()
}

fn layered_fact(
    operation: &str,
    record: &MemoryRecord,
    mutation: MemoryMutation,
    stage: MemoryMutationJournalStage,
) -> MemoryJournalFact {
    MemoryJournalFact::new(
        operation,
        mutation.idempotency_key.clone(),
        record.clone(),
        MemoryBodyRef::new(SIX_LAYER_STREAM, record.content_hash.clone()),
    )
    .with_mutation(mutation, stage)
    .unwrap()
}

/// PD-17 success/regression: replaying one committed mutation key returns the original receipt
/// without advancing the revision, and reusing the key with another payload digest is a conflict.
#[test]
fn replaying_a_mutation_key_returns_the_original_receipt_without_advancing_revision() {
    let key = "pd17:replay";
    let mut ledger = MemoryMutationLedger::new();
    ledger.seed_record("memory-1", "project:code", 1).unwrap();
    assert_eq!(ledger.current_revision("memory-1", "project:code"), 1);

    let first = mutation(MemoryMutationOperation::Update, 1, key);
    let MemoryMutationOutcome::Committed { receipt } = ledger.apply(first.clone()).unwrap() else {
        panic!("the first apply commits a new receipt");
    };
    assert_eq!(ledger.current_revision("memory-1", "project:code"), 2);

    let MemoryMutationOutcome::Replayed { original } = ledger.apply(first).unwrap() else {
        panic!("the same digest must replay the original receipt");
    };
    assert_eq!(original, receipt);
    assert_eq!(ledger.current_revision("memory-1", "project:code"), 2);
    assert_eq!(ledger.receipt_for(key), Some(&receipt));

    let changed = mutation(MemoryMutationOperation::Update, 2, key);
    assert_eq!(
        ledger.apply(changed).unwrap_err(),
        "memory_mutation_idempotency_conflict"
    );
    assert_eq!(ledger.current_revision("memory-1", "project:code"), 2);
}

/// PD-17 deny-first: forged provenance, missing evidence, forged actors and unknown authority are
/// all rejected before a mutation can become an authoritative journal fact.
#[test]
fn forged_origin_import_mode_evidence_and_actor_are_rejected() {
    let key = "pd17:hardening";
    let mut forged_origin = layered_record(
        "memory-hardening",
        "department:planning",
        "forged origin",
        1,
        key,
    );
    forged_origin.origin = MemoryOrigin::User;
    let forged_mutation = layered_mutation(
        &forged_origin,
        MemoryMutationOperation::Add,
        MemoryMutationAuthority::Agent,
        0,
        key,
    );
    assert_eq!(
        MemoryJournalFact::new(
            "write",
            key,
            forged_origin.clone(),
            MemoryBodyRef::new(SIX_LAYER_STREAM, forged_origin.content_hash.clone()),
        )
        .with_mutation(forged_mutation, MemoryMutationJournalStage::Candidate)
        .unwrap_err(),
        "memory_mutation_journal_origin_forged"
    );

    let mut unresolved = layered_record(
        "memory-hardening",
        "department:planning",
        "unresolved origin",
        1,
        key,
    );
    unresolved.origin = MemoryOrigin::Unknown;
    let unresolved_mutation = layered_mutation(
        &unresolved,
        MemoryMutationOperation::Add,
        MemoryMutationAuthority::Agent,
        0,
        key,
    );
    assert_eq!(
        MemoryJournalFact::new(
            "write",
            key,
            unresolved.clone(),
            MemoryBodyRef::new(SIX_LAYER_STREAM, unresolved.content_hash.clone()),
        )
        .with_mutation(unresolved_mutation, MemoryMutationJournalStage::Candidate)
        .unwrap_err(),
        "memory_mutation_journal_origin_unresolved"
    );

    let mut legacy = layered_record(
        "memory-hardening",
        "department:planning",
        "legacy import",
        1,
        key,
    );
    legacy.import_mode = MemoryImportMode::LegacyImport;
    legacy.origin = MemoryOrigin::Unknown;
    let legacy_mutation = layered_mutation(
        &legacy,
        MemoryMutationOperation::Add,
        MemoryMutationAuthority::Service,
        0,
        key,
    );
    assert_eq!(
        MemoryJournalFact::new(
            "write",
            key,
            legacy.clone(),
            MemoryBodyRef::new(SIX_LAYER_STREAM, legacy.content_hash.clone()),
        )
        .with_mutation(legacy_mutation, MemoryMutationJournalStage::Candidate)
        .unwrap_err(),
        "memory_mutation_journal_import_mode_invalid"
    );

    let no_evidence = MemoryMutation::new(
        "mutation:pd17:no-evidence",
        MemoryMutationOperation::Add,
        "local-user",
        scope(),
        vec![MemoryMutationTarget::new("memory-1", "project:code", 0)],
        Vec::new(),
        1,
        1,
        "pd17:no-evidence",
        &json!({"key": "pd17:no-evidence"}),
    );
    assert_eq!(
        no_evidence.unwrap_err(),
        "memory_mutation_evidence_required"
    );

    let forged_actor = MemoryMutation::new(
        "mutation:pd17:forged-actor",
        MemoryMutationOperation::Add,
        "model-supplied-actor",
        scope(),
        vec![MemoryMutationTarget::new("memory-1", "project:code", 0)],
        evidence(),
        1,
        1,
        "pd17:forged-actor",
        &json!({"key": "pd17:forged-actor"}),
    );
    assert_eq!(
        forged_actor.unwrap_err(),
        "memory_mutation_actor_scope_mismatch"
    );

    let unknown_authority = mutation(MemoryMutationOperation::Add, 0, "pd17:unknown-authority")
        .with_authority(MemoryMutationAuthority::Unknown);
    assert_eq!(
        unknown_authority.unwrap_err(),
        "memory_mutation_authority_unknown"
    );
}

/// PD-17 success/regression: after all candidate/draft/qualify/approve/supersede/tombstone facts
/// commit, a projection rebuild preserves the six layers, the lifecycle state, the provenance
/// (origin/reviewer/evidence/supersedes) and the collection for every record.
#[test]
fn rebuild_preserves_six_layers_state_and_origin() {
    let company = layered_record(
        "layer-company",
        "company",
        "company memory",
        1,
        "pd17:six:company",
    );
    let mut department_draft = layered_record(
        "layer-department",
        "department:planning",
        "department memory",
        1,
        "pd17:six:department:add",
    );
    department_draft.origin = MemoryOrigin::Hook;
    let department = approved(
        department_draft.clone(),
        2,
        "operator",
        "pd17:six:department:approve",
    );

    let mut role_draft = layered_record(
        "layer-role",
        "role:builder",
        "role memory",
        1,
        "pd17:six:role:add",
    );
    role_draft.origin = MemoryOrigin::Git;
    let role = approved(role_draft.clone(), 2, "steward", "pd17:six:role:qualify");

    let project_draft = layered_record(
        "layer-project",
        "project:code",
        "project memory",
        1,
        "pd17:six:project:add",
    );
    let project = approved(
        project_draft.clone(),
        2,
        "operator",
        "pd17:six:project:approve",
    );
    let mut superseded = project.clone();
    superseded.revision = 3;
    superseded.supersedes = Some("layer-project-prior".to_owned());
    superseded.last_mutation_key = Some("pd17:six:project:supersede".to_owned());

    let mut user_draft = layered_record(
        "layer-user",
        "user-private",
        "user memory",
        1,
        "pd17:six:user:add",
    );
    user_draft.origin = MemoryOrigin::User;
    let mut user_rejected = user_draft.clone();
    user_rejected.revision = 2;
    user_rejected.admission_state = MemoryAdmission::Rejected;
    user_rejected.state = MemoryState::Rejected;
    user_rejected.reviewed_by = Some("operator".to_owned());
    user_rejected.review_reason = Some("retention revoke".to_owned());
    user_rejected.reviewed_at_ms = Some(3);
    user_rejected.last_mutation_key = Some("pd17:six:user:revoke".to_owned());

    let mut scratch = layered_record(
        "layer-scratch",
        "scratch",
        "scratch memory",
        1,
        "pd17:six:scratch",
    );
    scratch.admission_state = MemoryAdmission::Ephemeral;
    scratch.state = MemoryState::Active;

    let events = vec![
        event(
            SIX_LAYER_STREAM,
            1,
            "fact:pd17:six:company",
            layered_fact(
                "write",
                &company,
                layered_mutation(
                    &company,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Agent,
                    0,
                    "pd17:six:company",
                ),
                MemoryMutationJournalStage::Candidate,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            2,
            "fact:pd17:six:department:add",
            layered_fact(
                "write",
                &department_draft,
                layered_mutation(
                    &department_draft,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Service,
                    0,
                    "pd17:six:department:add",
                ),
                MemoryMutationJournalStage::Draft,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            3,
            "fact:pd17:six:department:approve",
            layered_fact(
                "review",
                &department,
                layered_mutation(
                    &department,
                    MemoryMutationOperation::Approve,
                    MemoryMutationAuthority::Human,
                    1,
                    "pd17:six:department:approve",
                ),
                MemoryMutationJournalStage::Approve,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            4,
            "fact:pd17:six:role:add",
            layered_fact(
                "write",
                &role_draft,
                layered_mutation(
                    &role_draft,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Service,
                    0,
                    "pd17:six:role:add",
                ),
                MemoryMutationJournalStage::Candidate,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            5,
            "fact:pd17:six:role:qualify",
            layered_fact(
                "review",
                &role,
                layered_mutation(
                    &role,
                    MemoryMutationOperation::Publish,
                    MemoryMutationAuthority::Service,
                    1,
                    "pd17:six:role:qualify",
                ),
                MemoryMutationJournalStage::Qualify,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            6,
            "fact:pd17:six:project:add",
            layered_fact(
                "write",
                &project_draft,
                layered_mutation(
                    &project_draft,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Agent,
                    0,
                    "pd17:six:project:add",
                ),
                MemoryMutationJournalStage::Candidate,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            7,
            "fact:pd17:six:project:approve",
            layered_fact(
                "review",
                &project,
                layered_mutation(
                    &project,
                    MemoryMutationOperation::Approve,
                    MemoryMutationAuthority::Human,
                    1,
                    "pd17:six:project:approve",
                ),
                MemoryMutationJournalStage::Approve,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            8,
            "fact:pd17:six:project:supersede",
            layered_fact(
                "review",
                &superseded,
                layered_mutation(
                    &superseded,
                    MemoryMutationOperation::Update,
                    MemoryMutationAuthority::Human,
                    2,
                    "pd17:six:project:supersede",
                ),
                MemoryMutationJournalStage::Supersede,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            9,
            "fact:pd17:six:user:add",
            layered_fact(
                "write",
                &user_draft,
                layered_mutation(
                    &user_draft,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Human,
                    0,
                    "pd17:six:user:add",
                ),
                MemoryMutationJournalStage::Candidate,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            10,
            "fact:pd17:six:user:revoke",
            layered_fact(
                "review",
                &user_rejected,
                layered_mutation(
                    &user_rejected,
                    MemoryMutationOperation::Revoke,
                    MemoryMutationAuthority::Human,
                    1,
                    "pd17:six:user:revoke",
                ),
                MemoryMutationJournalStage::Tombstone,
            ),
        ),
        event(
            SIX_LAYER_STREAM,
            11,
            "fact:pd17:six:scratch",
            layered_fact(
                "write",
                &scratch,
                layered_mutation(
                    &scratch,
                    MemoryMutationOperation::Add,
                    MemoryMutationAuthority::Agent,
                    0,
                    "pd17:six:scratch",
                ),
                MemoryMutationJournalStage::Ephemeral,
            ),
        ),
    ];

    let projection = kiana_domain::project_memory_facts(&events).unwrap();
    assert_eq!(projection.source_cursor, 11);
    assert_eq!(projection.mutation_digests.len(), 11);
    assert_eq!(projection.records.len(), 6);

    assert_eq!(
        projection.records["layer-company"]["layer"],
        json!("company")
    );
    assert_eq!(
        projection.records["layer-company"]["origin"],
        json!("model")
    );
    assert_eq!(
        projection.records["layer-company"]["admission_state"],
        json!("candidate")
    );
    assert_eq!(projection.records["layer-company"]["state"], json!("draft"));

    assert_eq!(
        projection.records["layer-department"]["collection"],
        json!("department:planning")
    );
    assert_eq!(
        projection.records["layer-department"]["origin"],
        json!("hook")
    );
    assert_eq!(
        projection.records["layer-department"]["admission_state"],
        json!("qualified")
    );
    assert_eq!(
        projection.records["layer-department"]["state"],
        json!("active")
    );
    assert_eq!(
        projection.records["layer-department"]["reviewed_by"],
        json!("operator")
    );
    assert_eq!(projection.records["layer-department"]["revision"], json!(2));

    assert_eq!(projection.records["layer-role"]["origin"], json!("git"));
    assert_eq!(
        projection.records["layer-role"]["admission_state"],
        json!("qualified")
    );

    assert_eq!(
        projection.records["layer-project"]["origin"],
        json!("model")
    );
    assert_eq!(
        projection.records["layer-project"]["supersedes"],
        json!("layer-project-prior")
    );
    assert_eq!(projection.records["layer-project"]["revision"], json!(3));

    assert_eq!(
        projection.records["layer-user"]["collection"],
        json!("user-private")
    );
    assert_eq!(projection.records["layer-user"]["origin"], json!("user"));
    assert_eq!(
        projection.records["layer-user"]["admission_state"],
        json!("rejected")
    );
    assert_eq!(projection.records["layer-user"]["state"], json!("rejected"));
    let user: MemoryRecord =
        serde_json::from_value(projection.records["layer-user"].clone()).unwrap();
    assert_eq!(user.admission_state, MemoryAdmission::Rejected);
    assert_eq!(user.state, MemoryState::Rejected);
    assert_eq!(user.origin, MemoryOrigin::User);

    assert_eq!(
        projection.records["layer-scratch"]["layer"],
        json!("instance-scratch")
    );
    assert_eq!(
        projection.records["layer-scratch"]["admission_state"],
        json!("ephemeral")
    );
    assert_eq!(
        projection.records["layer-scratch"]["state"],
        json!("active")
    );

    let expected = [
        company,
        department,
        role,
        superseded,
        user_rejected,
        scratch,
    ];
    assert!(projection.matches_records(&expected));
    assert_eq!(
        projection.mutation_digests["pd17:six:company"],
        layered_mutation(
            &expected[0],
            MemoryMutationOperation::Add,
            MemoryMutationAuthority::Agent,
            0,
            "pd17:six:company",
        )
        .digest()
    );
}
