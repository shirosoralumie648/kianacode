use kiana_domain::{
    json_digest, CatalogCandidate, CatalogEntryKind, EvidenceStatus, ExtensionCatalog,
    HookOrderCandidate, SourceKind, SourceRef,
};

fn source(id: &str) -> SourceRef {
    SourceRef::new(
        id,
        SourceKind::UserImport,
        format!("fixture://{id}"),
        "revision:1",
        json_digest(&serde_json::json!({"source": id})),
        Some(1),
        EvidenceStatus::Attributed,
    )
    .unwrap()
}

fn candidate(source_id: &str, precedence: u16, text: &str) -> CatalogCandidate {
    CatalogCandidate {
        kind: CatalogEntryKind::Skill,
        namespace: "project".to_owned(),
        name: "same-skill".to_owned(),
        version: "1.0.0".to_owned(),
        content_digest: json_digest(&serde_json::json!({"text": text})),
        source: source(source_id),
        precedence,
    }
}

#[test]
fn skill_name_collision_is_deterministic_and_audited() {
    let first = candidate("user", 50, "user body");
    let second = candidate("project", 60, "project body");
    let left = ExtensionCatalog::new(1, vec![second.clone(), first.clone()]).unwrap();
    let right = ExtensionCatalog::new(1, vec![first, second]).unwrap();
    assert_eq!(left, right);
    assert_eq!(left.selected().next().unwrap().source.source_id, "user");
    let shadowed = left.shadowed().next().unwrap();
    assert_eq!(shadowed.reason, "duplicate_identity_shadowed");
    assert!(shadowed.shadowed_by.is_some());
    left.validate().unwrap();
}

#[test]
fn hook_matcher_order_is_replayable() {
    let hooks = vec![
        HookOrderCandidate {
            hook_id: "broad".to_owned(),
            event: "PreToolUse".to_owned(),
            matcher: "memory.*".to_owned(),
            source_id: "source-b".to_owned(),
            precedence: 20,
        },
        HookOrderCandidate {
            hook_id: "exact".to_owned(),
            event: "PreToolUse".to_owned(),
            matcher: "memory.write".to_owned(),
            source_id: "source-a".to_owned(),
            precedence: 20,
        },
    ];
    let ordered = kiana_domain::order_hooks(hooks.clone()).unwrap();
    let replayed = kiana_domain::order_hooks(hooks.into_iter().rev().collect()).unwrap();
    assert_eq!(ordered, replayed);
    assert_eq!(ordered[0].hook_id, "exact");
    assert_eq!(ordered[1].hook_id, "broad");
}
