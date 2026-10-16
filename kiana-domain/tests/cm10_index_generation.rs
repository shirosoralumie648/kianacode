//! CM-10 Domain generation 状态机的合成合同测试。
//!
//! component/source digests 都是重复字符拼出的格式 fixture，不代表磁盘上的真实索引部件。
//! 这些测试验证 Building/Ready/Failed 状态和 reader 的单一 manifest 视图，不执行 Query 文件发布。

use kiana_domain::*;
use std::collections::BTreeMap;

/// 生成格式正确的合成 SHA-256 文本，不对应真实组件或 source manifest。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 为四种组件提供同一测试摘要，只验证完整 key 集合而不模拟四个物理索引文件。
fn components(value: char) -> BTreeMap<String, String> {
    IndexComponentKind::ALL
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), digest(value)))
        .collect()
}

/// 验证 candidate 在 commit 前对 reader 不可见，且开始下一代时旧 Ready 仍可读。
#[test]
fn readers_never_mix_index_generations() {
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    assert_eq!(
        state.reader().unwrap_err(),
        "index_generation_no_ready_manifest"
    );
    let ready = state.commit(building).unwrap();
    assert_eq!(state.reader().unwrap().generation, ready.generation);

    let next = state.begin(digest('c'), components('d')).unwrap();
    assert_eq!(state.reader().unwrap().source_manifest_digest, digest('a'));
    assert_eq!(next.source_manifest_digest, digest('c'));
    state.validate().unwrap();
}

/// 验证新 generation 构建失败时保留旧 Ready reader，只更新 last_failure。
#[test]
fn failed_rebuild_keeps_last_ready_generation() {
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    let ready = state.commit(building).unwrap();
    state.begin(digest('c'), components('d')).unwrap();
    state.fail("dense component checksum mismatch").unwrap();
    let reader = state.reader().unwrap();
    assert_eq!(reader.generation, ready.generation);
    assert_eq!(reader.source_manifest_digest, digest('a'));
    assert_eq!(
        state.last_failure.as_deref(),
        Some("dense component checksum mismatch")
    );
}

/// 验证缺少 Dense key 的不完整组件 manifest 在构造阶段即被拒绝。
#[test]
fn incomplete_component_manifest_is_rejected() {
    let mut partial = components('a');
    partial.remove("dense");
    assert_eq!(
        IndexManifest::building(1, digest('b'), partial).unwrap_err(),
        "index_manifest_header_invalid"
    );
}
