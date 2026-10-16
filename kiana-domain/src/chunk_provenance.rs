//! 文本分块及其来源位置（provenance）合同，属于 `kiana-domain`。
//!
//! 【预期数据流】
//! ```text
//! Query chunker 接收 source text/path/digest 和算法版本
//!     -> 生成按 UTF-8 byte range 连续排列的 SourceChunk
//!     -> 本模块把 chunk 组装为 ChunkSet 并检查无重叠、无缺口
//!     -> reconstruct() 可按顺序拼回原始文本
//!     -> context/index 层可携带这些派生证据
//! ```
//! 当前 `chunk_text()` 在仓内只有测试调用点，尚未发现生产索引构建路径消费 ChunkSet。
//! Chunk 是源文本的派生上下文证据，不是编译器事实、权限声明或可信指令的授权结果。
//! parser/chunker/transformation 摘要用于记录处理版本和检测内容变化；它们是无密钥摘要，不认证生成者。
//! Serde DTO 可以从外部 JSON 反序列化，所以每个消费边界仍需调用 `validate()`。

use crate::{canonical_journal_bytes, journal_sha256, json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

/// byte/line range DTO 的稳定 schema 名称。
pub const CHUNK_RANGE_SCHEMA: &str = "kiana.chunk-range.v1";
/// 单个 source chunk DTO 的稳定 schema 名称。
pub const SOURCE_CHUNK_SCHEMA: &str = "kiana.source-chunk.v1";
/// chunk 集合 DTO 的稳定 schema 名称。
pub const CHUNK_SET_SCHEMA: &str = "kiana.chunk-set.v1";
/// 本批 chunk DTO 的 schema 版本。
pub const CHUNK_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 一个 ChunkSet 可包含的 chunk 行最大数。
pub const MAX_SOURCE_CHUNKS: usize = 4_096;

/// 校验版本、原因等必填文本的非空、字节上限与 NUL 字符。
fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 校验摘要的 `sha256:` + 64 位十六进制外形，不读取对应内容。
fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// Query chunker 对文本区域采用的粗粒度分类，不代表语法树节点类型。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkKind {
    /// 由语法外形规则识别的函数、类型等代码段起点。
    CodeSymbol,
    /// 由 Markdown 等文档标题行识别的段落起点。
    DocumentSection,
    /// 未识别格式或超长文本使用的固定大小窗口。
    Window,
}

/// 一个 chunk 在原始 UTF-8 source 中的 byte 和行范围。
///
/// byte 范围采用左闭右开 `[byte_start, byte_end)`，可直接切片并保证相邻范围不重复字节。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkRange {
    /// 固定 range schema 名称。
    pub schema: String,
    /// 原始 source 中的起始字节偏移，含该位置。
    pub byte_start: u64,
    /// 原始 source 中的结束字节偏移，不含该位置。
    pub byte_end: u64,
    /// chunk 涉及的第一行，按 1 起算。
    pub line_start: u64,
    /// chunk 涉及的最后一行，按 1 起算。
    pub line_end: u64,
}

impl ChunkRange {
    /// 构造并立即做基本范围校验；具体 source 长度由 ChunkSet 校验时再传入。
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

    /// 检查 schema、非空 byte 区间、source 上界和 1 起算的行区间。
    ///
    /// 失败返回 `chunk_range_invalid`。此 DTO 没有原始 source 文本，因此不能从 byte offset 重新推导行号；
    /// `source_bytes` 只限制 byte_end，不会证明 line_start/line_end 与真实换行位置一致。
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

/// 一段源文本及其切片位置、处理版本和摘要。
///
/// `content` 是原文切片；parent id 将被拆成多个窗口的语义片段归组。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceChunk {
    /// 固定 chunk schema 名称。
    pub schema: String,
    /// 依据 source path/digest/range/content/transformation 派生的稳定 chunk identity。
    pub chunk_id: String,
    /// 代码符号、文档章节或普通窗口分类。
    pub kind: ChunkKind,
    /// 该内容在整个 source 中的 byte/line 定位。
    pub range: ChunkRange,
    /// 若一个语义片段被拆成多个窗口，记录共同的父分组摘要。
    #[serde(default)]
    pub parent_chunk_id: Option<String>,
    /// 识别语义起点的 parser 规则版本。
    pub parser_version: String,
    /// 分块算法版本。
    pub chunker_version: String,
    /// 原始 source 对应的 UTF-8 子串。
    pub content: String,
    /// 当前 chunk 内容字节的 SHA-256。
    pub content_digest: String,
    /// 对 source identity、range、kind 和算法版本等转换输入计算的摘要。
    pub transformation_digest: String,
}

impl SourceChunk {
    /// 校验单个 chunk 的基本合同和内容字节数。
    ///
    /// `source_bytes` 是所属 source 的总字节数；这里会验证嵌套 range、版本文本、摘要外形和 content digest。
    /// transformation digest 只检查外形，不会在此重算；parent id 也只作为分组标签校验格式。
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

/// 一个 source 的完整、按 byte offset 排序的 chunk 序列。
///
/// 该结构把所有 chunk 绑定到同一个 source path/digest 和 parser/chunker 版本，并要求从 byte 0 连续覆盖到 source_bytes。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkSet {
    /// 固定集合 schema 名称。
    pub schema: String,
    /// ChunkSet DTO 版本。
    pub version: SchemaVersion,
    /// source 的相对路径；它是 provenance 标签，不是文件访问授权。
    pub source_path: String,
    /// 调用方提供的 source 内容摘要。
    pub source_digest: String,
    /// source 的 UTF-8 字节长度。
    pub source_bytes: u64,
    /// source 行数；按 Query chunker 的行切分定义记录。
    pub source_lines: u64,
    /// 生成 chunk 起点所用的 parser 规则版本。
    pub parser_version: String,
    /// 切分窗口策略版本。
    pub chunker_version: String,
    /// 按 byte_start 递增、应无缝覆盖整个 source 的 chunk 列表。
    pub chunks: Vec<SourceChunk>,
    /// 对集合业务字段计算的无密钥摘要。
    pub set_digest: String,
}

impl ChunkSet {
    /// 组装集合、计算自摘要，并在返回前验证整份无损范围合同。
    ///
    /// 【调用者】Query `chunk_text()`；当前仓内没有生产构建器直接调用，测试是实际使用方。
    /// 【输入/输出】source path/digest/长度/行数、算法版本和 chunk 列表，成功返回完整 ChunkSet。
    /// 【失败情况】header/摘要无效、chunk 不连续或 ID 重复时返回稳定 `chunk_set_*` 错误。
    /// 【副作用/设计原因】纯内存构造；把一个集合的版本和 source identity 放在单一 envelope，避免逐块拼接不同来源。
    /// 它不会读取原 source，因此 source_digest 与真实源文本的对应关系由构造者负责。
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

    /// 校验 header、嵌套 chunk、唯一 ID、byte 连续性和集合自摘要。
    ///
    /// 成功保证 chunk.content 按顺序拼接后的字节长度覆盖 source_bytes，且没有 byte gap/overlap；
    /// 不保证行号与原文换行匹配，也不重算 source_digest 或 transformation_digest。
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

    /// 按列表顺序拼接 chunk 内容。
    ///
    /// 调用者应先对来自外部的 ChunkSet 调用 `validate()`；本方法本身不返回错误，也不检查 gap/overlap。
    pub fn reconstruct(&self) -> String {
        self.chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect()
    }

    /// 使用统一的 journal canonical JSON 编码，供持久化或稳定比较调用。
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    /// 对集合除 `set_digest` 外的字段计算无密钥 SHA-256 摘要。
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
