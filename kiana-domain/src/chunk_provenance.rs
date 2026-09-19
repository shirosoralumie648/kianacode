//! Deterministic chunk and offset provenance contracts.
//!
//! A ChunkSet is a lossless, non-overlapping view of one source snapshot. It carries the parser,
//! chunker and transformation digests needed to reproduce the view; it does not assert compiler
//! truth or grant access to the source.

use crate::{canonical_journal_bytes, journal_sha256, json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CHUNK_RANGE_SCHEMA: &str = "kiana.chunk-range.v1";
pub const SOURCE_CHUNK_SCHEMA: &str = "kiana.source-chunk.v1";
pub const CHUNK_SET_SCHEMA: &str = "kiana.chunk-set.v1";
pub const CHUNK_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SOURCE_CHUNKS: usize = 4_096;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    CodeSymbol,
    DocumentSection,
    Window,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkRange {
    pub schema: String,
    pub byte_start: u64,
    pub byte_end: u64,
    pub line_start: u64,
    pub line_end: u64,
}

impl ChunkRange {
    pub fn new(
        byte_start: u64,
        byte_end: u64,
        line_start: u64,
        line_end: u64,
    ) -> Result<Self, String> {
        let range = Self {
            schema: CHUNK_RANGE_SCHEMA.to_owned(),
            byte_start,
            byte_end,
            line_start,
            line_end,
        };
        range.validate(u64::MAX)?;
        Ok(range)
    }

    pub fn validate(&self, source_bytes: u64) -> Result<(), String> {
        if self.schema != CHUNK_RANGE_SCHEMA
            || self.byte_start >= self.byte_end
            || self.byte_end > source_bytes
            || self.line_start == 0
            || self.line_end < self.line_start
        {
            return Err("chunk_range_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceChunk {
    pub schema: String,
    pub chunk_id: String,
    pub kind: ChunkKind,
    pub range: ChunkRange,
    #[serde(default)]
    pub parent_chunk_id: Option<String>,
    pub parser_version: String,
    pub chunker_version: String,
    pub content: String,
    pub content_digest: String,
    pub transformation_digest: String,
}

impl SourceChunk {
    pub fn validate(&self, source_bytes: u64) -> Result<(), String> {
        if self.schema != SOURCE_CHUNK_SCHEMA {
            return Err("source_chunk_schema_invalid".to_owned());
        }
        self.range.validate(source_bytes)?;
        digest(&self.chunk_id, "source_chunk_id")?;
        required(&self.parser_version, "source_chunk_parser_version", 128)?;
        required(&self.chunker_version, "source_chunk_chunker_version", 128)?;
        digest(&self.content_digest, "source_chunk_content_digest")?;
        digest(
            &self.transformation_digest,
            "source_chunk_transformation_digest",
        )?;
        if self.content.as_bytes().len() as u64 != self.range.byte_end - self.range.byte_start {
            return Err("source_chunk_content_range_mismatch".to_owned());
        }
        if self.content_digest != format!("sha256:{}", journal_sha256(self.content.as_bytes())) {
            return Err("source_chunk_content_digest_mismatch".to_owned());
        }
        if let Some(parent) = &self.parent_chunk_id {
            digest(parent, "source_chunk_parent_id")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkSet {
    pub schema: String,
    pub version: SchemaVersion,
    pub source_path: String,
    pub source_digest: String,
    pub source_bytes: u64,
    pub source_lines: u64,
    pub parser_version: String,
    pub chunker_version: String,
    pub chunks: Vec<SourceChunk>,
    pub set_digest: String,
}

impl ChunkSet {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_path: impl Into<String>,
        source_digest: impl Into<String>,
        source_bytes: u64,
        source_lines: u64,
        parser_version: impl Into<String>,
        chunker_version: impl Into<String>,
        chunks: Vec<SourceChunk>,
    ) -> Result<Self, String> {
        let mut set = Self {
            schema: CHUNK_SET_SCHEMA.to_owned(),
            version: CHUNK_SCHEMA_VERSION,
            source_path: source_path.into(),
            source_digest: source_digest.into(),
            source_bytes,
            source_lines,
            parser_version: parser_version.into(),
            chunker_version: chunker_version.into(),
            chunks,
            set_digest: String::new(),
        };
        set.set_digest = set.digest();
        set.validate()?;
        Ok(set)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CHUNK_SET_SCHEMA
            || self.version != CHUNK_SCHEMA_VERSION
            || self.source_bytes == 0
            || self.source_lines == 0
            || self.chunks.is_empty()
            || self.chunks.len() > MAX_SOURCE_CHUNKS
            || self.source_path.trim().is_empty()
            || self.source_path.starts_with('/')
            || self
                .source_path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("chunk_set_header_invalid".to_owned());
        }
        digest(&self.source_digest, "chunk_set_source_digest")?;
        required(&self.parser_version, "chunk_set_parser_version", 128)?;
        required(&self.chunker_version, "chunk_set_chunker_version", 128)?;
        let mut ids = BTreeSet::new();
        let mut parents = BTreeSet::new();
        let mut expected_start = 0;
        for chunk in &self.chunks {
            chunk.validate(self.source_bytes)?;
            if !ids.insert(chunk.chunk_id.clone()) {
                return Err("chunk_set_duplicate_chunk_id".to_owned());
            }
            if chunk.range.byte_start != expected_start {
                return Err("chunk_set_range_gap_or_overlap".to_owned());
            }
            expected_start = chunk.range.byte_end;
            if let Some(parent) = &chunk.parent_chunk_id {
                parents.insert(parent.clone());
            }
        }
        if expected_start != self.source_bytes {
            return Err("chunk_set_source_not_reconstructed".to_owned());
        }
        if parents.contains(&self.set_digest) {
            return Err("chunk_set_parent_self_reference".to_owned());
        }
        digest(&self.set_digest, "chunk_set_digest")?;
        if self.set_digest != self.digest() {
            return Err("chunk_set_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn reconstruct(&self) -> String {
        self.chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "source_path": self.source_path,
            "source_digest": self.source_digest,
            "source_bytes": self.source_bytes,
            "source_lines": self.source_lines,
            "parser_version": self.parser_version,
            "chunker_version": self.chunker_version,
            "chunks": self.chunks,
        }))
    }
}
