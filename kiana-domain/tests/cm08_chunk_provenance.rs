//! CM-08 Domain ChunkSet provenance 合同测试。
//!
//! 测试手工构造 chunk、range 和 digest，不调用 Query 分块器，也不读取 source 文件。
//! 它验证 byte 连续性/重叠拒绝和拼接结果；不能证明 source digest 或行号来自真实源文本。

use kiana_domain::*;

/// 生成外形合法的摘要 fixture，不表示真实 SHA-256 输入内容。
fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

/// 按给定 byte 区间和父分组构造 SourceChunk 测试值。
fn chunk(start: u64, end: u64, content: &str, parent_chunk_id: Option<String>) -> SourceChunk {
    SourceChunk {
        schema: SOURCE_CHUNK_SCHEMA.to_owned(),
        chunk_id: digest(if start == 0 { 'a' } else { 'b' }),
        kind: ChunkKind::CodeSymbol,
        range: ChunkRange::new(start, end, 1, 1).unwrap(),
        parent_chunk_id,
        parser_version: "parser.v1".to_owned(),
        chunker_version: "chunker.v1".to_owned(),
        content: content.to_owned(),
        content_digest: format!("sha256:{}", journal_sha256(content.as_bytes())),
        transformation_digest: digest('d'),
    }
}

#[test]
/// 连续的两段可按顺序重建原文，并保留共享语义 parent id。
fn chunk_ranges_reconstruct_source_and_preserve_parent_adjacency() {
    let parent = digest('e');
    let set = ChunkSet::new(
        "src/lib.rs",
        digest('f'),
        6,
        1,
        "parser.v1",
        "chunker.v1",
        vec![
            chunk(0, 3, "abc", Some(parent.clone())),
            chunk(3, 6, "def", Some(parent)),
        ],
    )
    .unwrap();
    assert_eq!(set.reconstruct(), "abcdef");
    set.validate().unwrap();
    assert_eq!(
        schema_contract(CHUNK_SET_SCHEMA).unwrap().owner_crate,
        "kiana-domain"
    );
}

#[test]
/// 两个 chunk 的 byte range 重叠时，ChunkSet 必须拒绝双重计算的 source evidence。
fn overlapping_chunks_do_not_double_count_evidence() {
    let error = ChunkSet::new(
        "README.md",
        digest('f'),
        6,
        1,
        "parser.v1",
        "chunker.v1",
        vec![chunk(0, 4, "abcd", None), chunk(3, 6, "def", None)],
    )
    .unwrap_err();
    assert_eq!(error, "chunk_set_range_gap_or_overlap");
}
