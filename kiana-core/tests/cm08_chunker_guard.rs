//! CM-08 Core 静态源码标记守卫。
//!
//! `include_str!` 将 Domain provenance contract 与 Query chunker 源码作为文本检查。
//! 它不实际构造 ChunkSet、不执行分块，也不能证明生产入口使用这些类型。

#[test]
/// 固定 lossless byte/line range、算法 provenance 和禁止把 chunker 当模型/能力循环的源级标记。
fn stable_chunker_keeps_lossless_offsets_and_provenance() {
    let domain = include_str!("../../kiana-domain/src/chunk_provenance.rs");
    let query = include_str!("../../kiana-query/src/chunker.rs");
    for marker in [
        "ChunkRange",
        "SourceChunk",
        "ChunkSet",
        "byte_start",
        "byte_end",
        "line_start",
        "line_end",
        "parent_chunk_id",
        "parser_version",
        "chunker_version",
        "transformation_digest",
        "chunk_set_range_gap_or_overlap",
        "chunk_set_source_not_reconstructed",
        "CodeSymbol",
        "DocumentSection",
        "Window",
        "is_char_boundary",
        "chunk_count_limit",
    ] {
        assert!(
            domain.contains(marker) || query.contains(marker),
            "CM-08 marker missing: {marker}"
        );
    }
    assert!(query.contains("chunk_text"));
    assert!(!query.contains("ModelClient"));
    assert!(!query.contains("CapabilityBroker"));
}
