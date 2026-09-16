use kiana_domain::json_digest;
use kiana_skills::{
    invalidate_extension_snapshots, snapshot_generation, ExtensionSnapshotCache, SnapshotCacheKey,
    SnapshotCacheState, SourceResolver, SourceRootKind,
};
use serde_json::json;
use std::fs;

fn key(root_digest: &str, trust: &str, config: &str) -> SnapshotCacheKey {
    SnapshotCacheKey::new(
        "/tmp/ext05-project",
        trust,
        root_digest,
        &json_digest(&json!({"registry": 1})),
        config,
    )
    .unwrap()
}

#[test]
fn snapshot_cache_key_binds_trust_sources_registry_and_config() {
    let source = json_digest(&json!({"roots": 1}));
    let config = json_digest(&json!({"config": 1}));
    let first = key(&source, "trusted", &config);
    let trust = key(&source, "untrusted", &config);
    let sources = key(&json_digest(&json!({"roots": 2})), "trusted", &config);
    let config_changed = key(&source, "trusted", &json_digest(&json!({"config": 2})));
    assert_ne!(first.key_digest, trust.key_digest);
    assert_ne!(first.key_digest, sources.key_digest);
    assert_ne!(first.key_digest, config_changed.key_digest);
    first.validate().unwrap();
}

#[test]
fn snapshot_cache_reuses_only_current_entries_and_invalidates_monotonically() {
    let mut cache = ExtensionSnapshotCache::new();
    let cache_key = key(
        &json_digest(&json!({"roots": 1})),
        "trusted",
        &json_digest(&json!({"config": 1})),
    );
    let first = cache
        .insert(cache_key.clone(), kiana_domain::SnapshotId::new())
        .unwrap();
    assert_eq!(
        cache.get(&cache_key).unwrap().unwrap().state,
        SnapshotCacheState::Prepared
    );
    let used = cache.mark_used(&cache_key).unwrap().unwrap();
    assert_eq!(used.state, SnapshotCacheState::Used);
    let before = cache.generation();
    assert!(cache.invalidate_key(&cache_key, "trust_changed").unwrap());
    assert!(cache.get(&cache_key).unwrap().is_none());
    assert!(cache.generation() > before);
    assert_eq!(
        cache.entries().next().unwrap().state,
        SnapshotCacheState::Invalidated
    );
    let second = cache
        .insert(cache_key, kiana_domain::SnapshotId::new())
        .unwrap();
    assert!(second.generation > first.generation);
}

#[test]
fn source_root_fingerprint_changes_when_resource_content_changes() {
    let root = std::env::temp_dir().join(format!("kiana-ext05-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let resource = root.join("SKILL.md");
    fs::write(&resource, "first").unwrap();
    let first = SourceResolver::new(&root, kiana_types::ProjectTrust::Trusted)
        .with_root(SourceRootKind::Bundled, &root)
        .resolve()
        .unwrap();
    fs::write(&resource, "second").unwrap();
    let second = SourceResolver::new(&root, kiana_types::ProjectTrust::Trusted)
        .with_root(SourceRootKind::Bundled, &root)
        .resolve()
        .unwrap();
    assert_ne!(
        first.summary.root_set_digest,
        second.summary.root_set_digest
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dynamic_invalidation_advances_global_generation() {
    let before = snapshot_generation();
    let after = invalidate_extension_snapshots("test");
    assert!(after > before);
}
