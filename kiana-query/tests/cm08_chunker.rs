use kiana_domain::{ChunkKind, ChunkSet};
use kiana_query::{chunk_text, ChunkingOptions};

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

#[test]
fn code_chunks_reconstruct_source_with_symbol_parent_provenance() {
    let source =
        "use crate::x;\n\npub fn first() {\n    one();\n}\n\npub fn second() {\n    two();\n}\n";
    let set = chunk_text(
        "src/lib.rs",
        digest('a'),
        source,
        ChunkingOptions {
            max_chunk_bytes: 32,
            max_window_bytes: 16,
            max_chunks: 32,
            ..ChunkingOptions::default()
        },
    )
    .unwrap();
    assert_eq!(set.reconstruct(), source);
    assert!(set
        .chunks
        .iter()
        .all(|chunk| chunk.kind == ChunkKind::CodeSymbol));
    assert!(set
        .chunks
        .iter()
        .any(|chunk| chunk.parent_chunk_id.is_some()));
    set.validate().unwrap();
}

#[test]
fn document_and_oversized_windows_keep_byte_and_line_ranges() {
    let source = "# Intro\n第一段落\n## Next\nsecond paragraph with unicode 中文\n";
    let set = chunk_text(
        "docs/guide.md",
        digest('b'),
        source,
        ChunkingOptions {
            max_chunk_bytes: 18,
            max_window_bytes: 9,
            max_chunks: 64,
            ..ChunkingOptions::default()
        },
    )
    .unwrap();
    assert_eq!(set.reconstruct(), source);
    assert!(set
        .chunks
        .iter()
        .all(|chunk| chunk.range.byte_end > chunk.range.byte_start));
    assert!(set
        .chunks
        .iter()
        .any(|chunk| chunk.kind == ChunkKind::DocumentSection));
    assert!(set
        .chunks
        .iter()
        .all(|chunk| chunk.range.line_start <= chunk.range.line_end));
    set.validate().unwrap();
}
