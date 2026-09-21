use kiana_domain::Freshness;
use kiana_query::{
    build_context_index_generation, read_context_index_generation,
    write_context_index_generation_atomic, ContextIndexOptions,
};
use std::fs;
use std::path::PathBuf;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-pd19-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn one() {}\n").unwrap();
    root
}

#[test]
fn context_generation_binds_source_fingerprint_and_publishes_ready_envelope() {
    let root = root();
    let state = kiana_domain::IndexGenerationState::new();
    let (envelope, next) = build_context_index_generation(
        &root,
        ContextIndexOptions::default(),
        &state,
        digest('a'),
        "kiana.deterministic-hash-embedding.v1",
    )
    .unwrap();
    assert_eq!(envelope.source.freshness, Freshness::Current);
    assert_eq!(
        envelope.index_manifest.status,
        kiana_domain::IndexGenerationStatus::Ready
    );
    assert!(next.reader().is_ok());

    let path = root.join(".kiana/context-index-generation.json");
    write_context_index_generation_atomic(&path, &envelope).unwrap();
    let loaded = read_context_index_generation(&path).unwrap();
    assert_eq!(
        loaded.source.source_fingerprint,
        envelope.source.source_fingerprint
    );
    assert_eq!(loaded.index_manifest.generation, 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn changed_source_or_ignore_rules_is_stale_not_fresh() {
    let root = root();
    let state = kiana_domain::IndexGenerationState::new();
    let (first, _) = build_context_index_generation(
        &root,
        ContextIndexOptions::default(),
        &state,
        digest('a'),
        "kiana.deterministic-hash-embedding.v1",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn two() {}\n").unwrap();
    let (second, _) = build_context_index_generation(
        &root,
        ContextIndexOptions::default(),
        &state,
        digest('b'),
        "kiana.deterministic-hash-embedding.v1",
    )
    .unwrap();
    assert_eq!(
        first.source.compare(&second.source).unwrap(),
        Freshness::Stale
    );
    let _ = fs::remove_dir_all(root);
}
