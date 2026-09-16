use kiana_domain::{
    AuthenticatedPrincipalRef, EvidenceStatus, Freshness, MemoryAdmission, MemoryClassification,
    MemoryCollection, MemoryOrigin, MemoryRecord, MemoryScope, MemoryState, ProjectIdentity,
    PromptAuthority, PromptSection, Purpose, SourceKind, SourceRef, SourceSnapshot,
};

fn project() -> ProjectIdentity {
    ProjectIdentity::new(
        "/tmp/project",
        "/tmp/project",
        Some(1),
        Some(2),
        "trust-revision",
    )
    .unwrap()
}

#[test]
fn source_refs_roundtrip_and_reject_missing_identity() {
    let source = SourceRef::new(
        "workspace:README.md",
        SourceKind::WorkspaceFile,
        "README.md",
        "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        Some(7),
        EvidenceStatus::Attributed,
    )
    .unwrap();
    let snapshot = SourceSnapshot::new(
        source.clone(),
        Freshness::Current,
        EvidenceStatus::Attributed,
        Some(100),
    )
    .unwrap();
    let encoded = serde_json::to_value(&snapshot).unwrap();
    let decoded: SourceSnapshot = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, snapshot);
    decoded.validate().unwrap();

    let mut missing = encoded;
    missing["source"]["source_id"] = serde_json::json!("");
    let decoded: SourceSnapshot = serde_json::from_value(missing).unwrap();
    assert!(decoded.validate().is_err());

    let mut unknown = serde_json::to_value(&source).unwrap();
    unknown["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SourceRef>(unknown).is_err());
}

#[test]
fn scope_resolution_never_uses_model_principal() {
    let scope = MemoryScope::new(
        AuthenticatedPrincipalRef::local(),
        project(),
        "session-cm01",
        vec![MemoryCollection::parse("project").unwrap()],
        Purpose {
            id: "context.read".to_owned(),
            description: "read project context".to_owned(),
        },
        false,
    )
    .unwrap();
    assert_eq!(scope.principal.principal_id, "local-user");
    assert!(scope.allows_collection(&MemoryCollection::parse("project:code").unwrap()));

    let mut forged = serde_json::to_value(&scope).unwrap();
    forged["principal"]["principal_id"] = serde_json::json!("model-claimed-user");
    let decoded: MemoryScope = serde_json::from_value(forged).unwrap();
    assert!(decoded.validate().is_err());
}

#[test]
fn prompt_and_memory_consumers_produce_source_refs_without_authority() {
    let prompt = PromptSection {
        name: "context".to_owned(),
        order: 200,
        text: "retrieved context".to_owned(),
        source: "artifact:prompt".to_owned(),
        authority: PromptAuthority::Context,
    };
    let prompt_ref = SourceRef::from_prompt_section(&prompt).unwrap();
    assert_eq!(prompt_ref.kind, SourceKind::Prompt);
    assert_eq!(prompt_ref.evidence, EvidenceStatus::Attributed);

    let record = MemoryRecord {
        schema: kiana_domain::MEMORY_RECORD_SCHEMA_V2.to_owned(),
        id: "memory-1".to_owned(),
        layer: "project".to_owned(),
        collection: "project".to_owned(),
        text: "remember".to_owned(),
        source: "model-claimed-source".to_owned(),
        origin: MemoryOrigin::Unknown,
        admission_state: MemoryAdmission::Candidate,
        state: MemoryState::Draft,
        classification: MemoryClassification::Project,
        ..MemoryRecord::default()
    };
    let memory_ref = SourceRef::from_memory_record(&record).unwrap();
    assert_eq!(memory_ref.kind, SourceKind::Memory);
    assert_eq!(memory_ref.evidence, EvidenceStatus::Unverifiable);
}
