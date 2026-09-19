//! Deterministic, lossless source chunking with byte/line provenance.
//!
//! This is intentionally a bounded syntax-lite adapter. It recognizes stable symbol/heading
//! starts without claiming compiler semantics, and falls back to UTF-8-safe windows for unknown
//! or oversized material. The resulting ChunkSet is contiguous and cannot double-count overlap.

use kiana_domain::{
    json_digest, ChunkKind, ChunkRange, ChunkSet, SourceChunk, CHUNK_SCHEMA_VERSION,
    MAX_SOURCE_CHUNKS,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const DEFAULT_CHUNKER_VERSION: &str = "kiana.chunker.v1";
pub const CODE_PARSER_VERSION: &str = "kiana.syntax-lite.v1";
pub const DOCUMENT_PARSER_VERSION: &str = "kiana.heading-lite.v1";
pub const WINDOW_PARSER_VERSION: &str = "kiana.raw-window.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkingOptions {
    pub max_chunk_bytes: usize,
    pub max_window_bytes: usize,
    pub max_chunks: usize,
    pub chunker_version: String,
}

impl Default for ChunkingOptions {
    fn default() -> Self {
        Self {
            max_chunk_bytes: 16 * 1024,
            max_window_bytes: 8 * 1024,
            max_chunks: MAX_SOURCE_CHUNKS,
            chunker_version: DEFAULT_CHUNKER_VERSION.to_owned(),
        }
    }
}

impl ChunkingOptions {
    fn validate(&self) -> Result<(), String> {
        if self.max_chunk_bytes < 4
            || self.max_window_bytes < 4
            || self.max_chunks == 0
            || self.max_chunks > MAX_SOURCE_CHUNKS
            || self.chunker_version.trim().is_empty()
        {
            return Err("chunking_options_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Line<'a> {
    start: usize,
    end: usize,
    number: u64,
    text: &'a str,
}

/// Chunk one UTF-8 source into contiguous semantic-ish sections or bounded windows.
pub fn chunk_text(
    source_path: impl AsRef<str>,
    source_digest: impl Into<String>,
    text: &str,
    options: ChunkingOptions,
) -> Result<ChunkSet, String> {
    options.validate()?;
    if text.is_empty() {
        return Err("chunk_source_empty".to_owned());
    }
    let source_path = source_path.as_ref().replace('\\', "/");
    let source_digest = source_digest.into();
    let lines = lines(text);
    let (kind, parser_version) = parser_for_path(&source_path);
    let semantic_starts = semantic_starts(&lines, kind);
    let mut chunks = Vec::new();
    for (index, start_line) in semantic_starts.iter().enumerate() {
        let end_line = semantic_starts
            .get(index + 1)
            .copied()
            .unwrap_or(lines.len());
        let byte_start = lines[*start_line].start;
        let byte_end = lines[end_line - 1].end;
        let parent_chunk_id = if kind == ChunkKind::Window {
            None
        } else {
            Some(json_digest(&json!({
                "source_path": source_path,
                "source_digest": source_digest,
                "start": byte_start,
                "end": byte_end,
                "kind": kind,
            })))
        };
        append_span_chunks(
            &mut chunks,
            text,
            byte_start,
            byte_end,
            kind,
            &parser_version,
            &options,
            parent_chunk_id,
            &source_path,
            &source_digest,
            &lines,
            if kind == ChunkKind::Window {
                options.max_window_bytes
            } else {
                options.max_chunk_bytes
            },
        )?;
    }
    ChunkSet::new(
        source_path,
        source_digest,
        text.len() as u64,
        lines.len() as u64,
        parser_version,
        options.chunker_version,
        chunks,
    )
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut starts = vec![0];
    for (index, character) in text.char_indices() {
        if character == '\n' && index + 1 < text.len() {
            starts.push(index + 1);
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(text.len());
            Line {
                start: *start,
                end,
                number: index as u64 + 1,
                text: &text[*start..end],
            }
        })
        .collect()
}

fn parser_for_path(path: &str) -> (ChunkKind, String) {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "mdx" | "rst" | "adoc" => (
            ChunkKind::DocumentSection,
            DOCUMENT_PARSER_VERSION.to_owned(),
        ),
        "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h" | "cpp" | "hpp"
        | "swift" | "kt" => (ChunkKind::CodeSymbol, CODE_PARSER_VERSION.to_owned()),
        _ => (ChunkKind::Window, WINDOW_PARSER_VERSION.to_owned()),
    }
}

fn semantic_starts(lines: &[Line<'_>], kind: ChunkKind) -> Vec<usize> {
    let mut starts = vec![0];
    for index in 1..lines.len() {
        let trimmed = lines[index].text.trim_start();
        let boundary = match kind {
            ChunkKind::DocumentSection => trimmed.starts_with('#'),
            ChunkKind::CodeSymbol => is_symbol_start(trimmed),
            ChunkKind::Window => false,
        };
        if boundary {
            starts.push(index);
        }
    }
    starts
}

fn is_symbol_start(line: &str) -> bool {
    [
        "fn ",
        "pub fn ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "trait ",
        "impl ",
        "def ",
        "class ",
        "function ",
        "export function ",
        "interface ",
        "type ",
    ]
    .iter()
    .any(|prefix| line.starts_with(prefix))
}

#[allow(clippy::too_many_arguments)]
fn append_span_chunks(
    chunks: &mut Vec<SourceChunk>,
    text: &str,
    byte_start: usize,
    byte_end: usize,
    kind: ChunkKind,
    parser_version: &str,
    options: &ChunkingOptions,
    parent_chunk_id: Option<String>,
    source_path: &str,
    source_digest: &str,
    lines: &[Line<'_>],
    max_bytes: usize,
) -> Result<(), String> {
    let mut cursor = byte_start;
    while cursor < byte_end {
        if chunks.len() >= options.max_chunks {
            return Err("chunk_count_limit".to_owned());
        }
        let end = next_boundary(text, cursor, byte_end, max_bytes);
        let content = &text[cursor..end];
        let (line_start, line_end) = line_range(lines, cursor, end);
        let range = ChunkRange::new(cursor as u64, end as u64, line_start, line_end)?;
        let content_digest = digest_bytes(content.as_bytes());
        let transformation_digest = json_digest(&json!({
            "schema": CHUNK_SCHEMA_VERSION,
            "source_path": source_path,
            "source_digest": source_digest,
            "kind": kind,
            "range": range,
            "parser_version": parser_version,
            "chunker_version": options.chunker_version,
            "parent_chunk_id": parent_chunk_id,
        }));
        let chunk_id = json_digest(&json!({
            "source_path": source_path,
            "source_digest": source_digest,
            "range": range,
            "content_digest": content_digest,
            "transformation_digest": transformation_digest,
        }));
        chunks.push(SourceChunk {
            schema: kiana_domain::SOURCE_CHUNK_SCHEMA.to_owned(),
            chunk_id,
            kind,
            range,
            parent_chunk_id: parent_chunk_id.clone(),
            parser_version: parser_version.to_owned(),
            chunker_version: options.chunker_version.clone(),
            content: content.to_owned(),
            content_digest,
            transformation_digest,
        });
        cursor = end;
    }
    Ok(())
}

fn next_boundary(text: &str, start: usize, end: usize, max_bytes: usize) -> usize {
    if end - start <= max_bytes {
        return end;
    }
    let target = start + max_bytes;
    let mut boundary = target;
    while boundary > start && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    if boundary == start {
        boundary = text[start..]
            .char_indices()
            .nth(1)
            .map(|(index, _)| start + index)
            .unwrap_or(end);
    }
    if let Some(newline) = text[start..boundary].rfind('\n') {
        let line_boundary = start + newline + 1;
        if line_boundary > start {
            return line_boundary;
        }
    }
    boundary
}

fn line_range(lines: &[Line<'_>], start: usize, end: usize) -> (u64, u64) {
    let first = lines
        .iter()
        .position(|line| start >= line.start && start < line.end)
        .unwrap_or(0);
    let last_offset = end.saturating_sub(1);
    let last = lines
        .iter()
        .position(|line| last_offset >= line.start && last_offset < line.end)
        .unwrap_or(lines.len().saturating_sub(1));
    (lines[first].number, lines[last].number)
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
