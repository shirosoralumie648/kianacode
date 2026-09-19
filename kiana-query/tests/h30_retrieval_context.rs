use kiana_domain::{ContextMaterialClass, RetrievalSourceKind};
use kiana_query::{repo_map_candidate, RepoMapFile};

#[test]
fn repo_map_results_enter_the_shared_context_candidate_contract() {
    let file = RepoMapFile {
        content_hash: "sha256:".to_owned() + &"a".repeat(64),
        path: "src/lib.rs".to_owned(),
        language: Some("rust".to_owned()),
        bytes: 128,
        estimated_tokens: 32,
        symbols: vec!["fn main".to_owned()],
    };
    let candidate = repo_map_candidate("/repo", &file, "project:read", 4).unwrap();
    candidate.validate().unwrap();
    assert_eq!(candidate.source_kind, RetrievalSourceKind::RepoMap);
    assert_eq!(
        candidate.material_class,
        ContextMaterialClass::ProjectKnowledge
    );
    assert_eq!(
        candidate.source_snapshot.freshness,
        kiana_domain::Freshness::Current
    );
}
