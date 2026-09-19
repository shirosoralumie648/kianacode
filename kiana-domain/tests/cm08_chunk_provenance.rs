use kiana_domain::*;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

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
