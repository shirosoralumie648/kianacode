//! PD-19 ContextIndex source fingerprint 与 generation envelope 的文件集成 fixture。
//!
//! 测试在临时工作区扫描文本文件、构造 Ready envelope 并写回 JSON；它验证代码路径和字段绑定，
//! 不证明索引在崩溃、多进程竞争或真实检索质量下具备 durability。

use kiana_domain::Freshness;
use kiana_query::{
    build_context_index_generation, read_context_index_generation,
    write_context_index_generation_atomic, ContextIndexOptions,
};
use std::fs;
use std::path::PathBuf;

/// 生成符合摘要外形的合成 ignore-rules digest。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 创建当前进程共用的临时项目根和一份 Rust 文件。
///
/// ⚠ 两个测试都调用此 helper，目录名只有 process ID；若测试线程并行，其中一个调用的清理可能删除另一个的 fixture。
/// 这里仅记录共享目录约束，没有改变临时路径或执行顺序。
fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("kiana-pd19-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "pub fn one() {}\n").unwrap();
    root
}

/// 验证 source manifest、Domain Ready manifest 和整个 JSON envelope 可在临时目录往返。
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

/// 修改源文件和 ignore digest 后，source manifest 比较结果应为 Stale。
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
