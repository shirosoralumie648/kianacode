use kiana_domain::{
    ExtensionSnapshot, ExtensionSnapshotState, ExtensionVisibilityActionKind,
    ExtensionVisibilityEntry, ExtensionVisibilityKind, ExtensionVisibilityRisk,
    ExtensionVisibilitySnapshot, ExtensionVisibilitySource, ExtensionVisibilityStatus,
    ExtensionVisibilityTrust, SnapshotId,
};
use std::collections::BTreeSet;

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn source(trust: ExtensionVisibilityTrust) -> ExtensionVisibilitySource {
    ExtensionVisibilitySource::new("source-skill", trust, digest('a'), "revision-v1").unwrap()
}

fn entry(trust: ExtensionVisibilityTrust) -> ExtensionVisibilityEntry {
    let mut actions = BTreeSet::new();
    actions.insert(ExtensionVisibilityActionKind::List);
    actions.insert(ExtensionVisibilityActionKind::Search);
    ExtensionVisibilityEntry::new(
        "review-skill",
        "review-skill-component",
        ExtensionVisibilityKind::Skill,
        "1.0.0",
        "Review code changes",
        ExtensionVisibilityStatus::Eligible,
        source(trust),
        ExtensionVisibilityRisk::ReadOnly,
        Some(digest('b')),
        actions,
    )
    .unwrap()
}

fn snapshot() -> ExtensionSnapshot {
    ExtensionSnapshot::new(
        SnapshotId::new(),
        3,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        digest('c'),
    )
    .unwrap()
}

#[test]
fn visibility_projection_omits_body_paths_and_secrets() {
    let source = snapshot();
    let projected = ExtensionVisibilitySnapshot::from_source_snapshot(
        &source,
        vec![entry(ExtensionVisibilityTrust::Trusted)],
        "review",
        32,
    )
    .unwrap();
    let encoded = serde_json::to_string(&projected).unwrap();
    assert!(encoded.contains("review-skill"));
    assert!(!encoded.contains("SKILL.md"));
    assert!(!encoded.contains("/private"));
    assert!(!encoded.contains("secret_refs"));
    assert!(!encoded.contains("internal_command"));
    assert_eq!(projected.generation, source.generation);
    assert_eq!(projected.source_snapshot_digest, source.snapshot_digest);
}

#[test]
fn untrusted_visibility_entry_is_rejected_before_projection() {
    let error = ExtensionVisibilityEntry::new(
        "review-skill",
        "review-skill-component",
        ExtensionVisibilityKind::Skill,
        "1.0.0",
        "Review code changes",
        ExtensionVisibilityStatus::Eligible,
        source(ExtensionVisibilityTrust::Untrusted),
        ExtensionVisibilityRisk::ReadOnly,
        Some(digest('b')),
        BTreeSet::new(),
    )
    .unwrap_err();
    assert_eq!(error, "extension_visibility_untrusted_entry");
}

#[test]
fn search_and_action_keep_snapshot_identity_and_generation() {
    let source = snapshot();
    let projected = ExtensionVisibilitySnapshot::from_source_snapshot(
        &source,
        vec![entry(ExtensionVisibilityTrust::Trusted)],
        "",
        32,
    )
    .unwrap();
    let action = projected
        .action(
            "review-skill",
            ExtensionVisibilityActionKind::List,
            "render catalog",
        )
        .unwrap();
    assert_eq!(action.snapshot_id, projected.snapshot_id);
    assert_eq!(action.generation, projected.generation);
    assert_eq!(action.snapshot_digest, projected.snapshot_digest);
    assert_eq!(projected.state, ExtensionSnapshotState::Prepared);
}
