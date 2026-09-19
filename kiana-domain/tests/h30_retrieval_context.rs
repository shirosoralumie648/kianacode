use kiana_domain::*;
use serde_json::json;

fn source(id: &str, kind: SourceKind, freshness: Freshness) -> SourceSnapshot {
    let source = SourceRef::new(
        id,
        kind,
        format!("fixture://{id}"),
        "revision:1",
        json_digest(&json!({"source": id})),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap();
    SourceSnapshot::new(source, freshness, EvidenceStatus::Attributed, Some(1)).unwrap()
}

fn candidate(
    id: &str,
    kind: RetrievalSourceKind,
    class: ContextMaterialClass,
) -> RetrievalCandidate {
    RetrievalCandidate::new(
        id,
        kind,
        class,
        format!("material for {id}"),
        source(
            id,
            match kind {
                RetrievalSourceKind::Memory => SourceKind::Memory,
                RetrievalSourceKind::RepoMap | RetrievalSourceKind::CodeSearch => {
                    SourceKind::WorkspaceFile
                }
                RetrievalSourceKind::Artifact => SourceKind::Artifact,
            },
            Freshness::Current,
        ),
        "project:read",
        json_digest(&json!({"acl": id})),
        900,
        vec![format!("evidence:{id}")],
    )
    .unwrap()
}

#[test]
fn memory_repo_code_and_artifact_candidates_share_one_context_plan() {
    let candidates = vec![
        candidate(
            "memory-1",
            RetrievalSourceKind::Memory,
            ContextMaterialClass::ProjectKnowledge,
        ),
        candidate(
            "repo-1",
            RetrievalSourceKind::RepoMap,
            ContextMaterialClass::ProjectKnowledge,
        ),
        candidate(
            "code-1",
            RetrievalSourceKind::CodeSearch,
            ContextMaterialClass::ProjectKnowledge,
        ),
        candidate(
            "artifact-1",
            RetrievalSourceKind::Artifact,
            ContextMaterialClass::ProjectKnowledge,
        ),
    ];
    let pack = RetrievalPack::new(json_digest(&json!({"query":"context"})), 4, candidates).unwrap();
    let plan = pack
        .compile_context(&PromptBundle::for_role(&RoleSpec::builder()), 100_000)
        .unwrap();
    plan.validate().unwrap();
    assert!(plan
        .items
        .iter()
        .all(|item| item.authority == PromptAuthority::Context
            || item.source.kind == SourceKind::Prompt));
    assert!(plan
        .items
        .iter()
        .any(|item| item.name == "retrieval:memory-1"));
}

#[test]
fn stale_snapshot_and_acl_are_visible_and_lesson_cannot_self_promote() {
    let mut stale = candidate(
        "stale",
        RetrievalSourceKind::Memory,
        ContextMaterialClass::ProjectKnowledge,
    );
    stale.source_snapshot.freshness = Freshness::Stale;
    stale.source_snapshot.snapshot_digest = stale.source_snapshot.digest();
    stale.candidate_digest = stale.digest();
    stale.validate().unwrap();
    let context = stale.as_context_candidate().unwrap();
    assert!(context.text.contains("freshness=stale"));
    assert_eq!(context.authority, PromptAuthority::Context);

    let missing_evidence = RetrievalCandidate::new(
        "lesson",
        RetrievalSourceKind::Memory,
        ContextMaterialClass::LessonCandidate,
        "candidate lesson",
        source("lesson", SourceKind::Memory, Freshness::Current),
        "project:read",
        json_digest(&json!({"acl":"lesson"})),
        500,
        Vec::new(),
    );
    assert_eq!(
        missing_evidence.unwrap_err(),
        "retrieval_lesson_evidence_required"
    );
}

#[test]
fn forged_kind_duplicate_or_tampered_acl_is_rejected() {
    let mut forged = candidate(
        "memory",
        RetrievalSourceKind::Memory,
        ContextMaterialClass::ProjectKnowledge,
    );
    forged.source_snapshot.source.kind = SourceKind::WorkspaceFile;
    assert_eq!(
        forged.validate().unwrap_err(),
        "retrieval_candidate_invalid"
    );

    let first = candidate(
        "same",
        RetrievalSourceKind::Memory,
        ContextMaterialClass::ProjectKnowledge,
    );
    let second = first.clone();
    assert_eq!(
        RetrievalPack::new(json_digest(&json!({"q":"same"})), 1, vec![first, second]).unwrap_err(),
        "retrieval_candidate_duplicate"
    );

    let mut acl = candidate(
        "acl",
        RetrievalSourceKind::Memory,
        ContextMaterialClass::ProjectKnowledge,
    );
    acl.acl_digest = json_digest(&json!({"acl":"forged"}));
    assert_eq!(acl.validate().unwrap_err(), "retrieval_candidate_invalid");
}
