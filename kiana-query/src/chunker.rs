//! 确定性文本分块 adapter，负责把完整 source 转成带 byte/line provenance 的 ChunkSet。
//!
//! 【预期数据流】
//! ```text
//! WorkspaceReadContent / 调用方 source text + path + source digest
//!     -> 按扩展名选择 heading-lite、syntax-lite 或 raw-window 规则
//!     -> 语义起点切段；超长段再切成 UTF-8 安全窗口
//!     -> 为每块计算 range、content/transformation digest 与 chunk id
//!     -> kiana-domain::ChunkSet 验证连续性并返回
//! ```
//! 当前仓内没有生产调用者连接 WorkspaceReadContent 与 `chunk_text()`；该 Query API 由 CM-08 tests 使用。
//! 这里是有界启发式（syntax-lite）分块，不构造 AST、不理解完整语言语法，也不证明编译器语义。
//! 输出 ChunkSet 是派生上下文证据，不是 authority section、权限或模型执行能力。

use kiana_domain::{
    json_digest, ChunkKind, ChunkRange, ChunkSet, SourceChunk, CHUNK_SCHEMA_VERSION,
    MAX_SOURCE_CHUNKS,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::Path;

/// 被记录到每个 ChunkSet 的分块算法标识；修改语义时应升级版本值以区分缓存。
pub const DEFAULT_CHUNKER_VERSION: &str = "kiana.chunker.v1";
/// 支持语言的语法外形识别规则标识，不代表对应语言完整 parser 版本。
pub const CODE_PARSER_VERSION: &str = "kiana.syntax-lite.v1";
/// 文档标题边界识别规则标识。
pub const DOCUMENT_PARSER_VERSION: &str = "kiana.heading-lite.v1";
/// 未识别格式的原始窗口规则标识。
pub const WINDOW_PARSER_VERSION: &str = "kiana.raw-window.v1";

/// 控制语义段最大长度、fallback 窗口大小、输出数量和 chunker identity。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkingOptions {
    /// 代码/文档语义段超过此 UTF-8 字节数时拆成多个窗口。
    pub max_chunk_bytes: usize,
    /// Window 类型文本以及超长语义段窗口的目标最大字节数。
    pub max_window_bytes: usize,
    /// 本次最多输出的 chunk 行数；超过后失败，不返回截断集合。
    pub max_chunks: usize,
    /// 记录在每个 chunk 上的 chunker 算法版本标签。
    pub chunker_version: String,
}

impl Default for ChunkingOptions {
    /// 使用 16 KiB 语义段、8 KiB 窗口和 Domain 允许的最大 chunk 行数。
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
    /// 拒绝过小窗口、零/超量 chunk 数和空版本标签。
    ///
    /// 最小 4 字节是为了让最大 UTF-8 字符（4 字节）至少能单独放进一个窗口。
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

/// 源文本中一行的 UTF-8 byte 边界、1 起算行号与借用文本切片。
///
/// 仅在本次 `chunk_text()` 调用中存在；不复制整份 source，也不暴露为持久化 DTO。
#[derive(Clone, Copy)]
struct Line<'a> {
    /// 该行在 source 中的起始 byte offset。
    start: usize,
    /// 该行结束的 byte offset；换行符包含在行文本范围中。
    end: usize,
    /// 从 1 起算的行号。
    number: u64,
    /// source 对应 byte 区间的借用字符串。
    text: &'a str,
}

/// 把一份非空 UTF-8 source 切成连续语义近似段或有界窗口。
///
/// 【调用者】当前只有 CM-08 Query tests；仓内未发现生产调用点。
/// 【输入/输出】接收路径标签、调用方 source digest、源文本和分块预算，返回完整 `ChunkSet`。
/// 【核心流程】校验选项/非空文本 → 标准化路径分隔符 → 建立行边界 → 按扩展名选分段规则 →
/// 找语义起点 → 对每段按 UTF-8 边界切窗 → 计算每块摘要和身份 → 交由 Domain 验证。
/// 【失败情况】空 source、预算非法、chunk 数超限或 Domain DTO 校验失败时返回稳定 `String` 错误，不返回半成品。
/// 【副作用】纯内存文本处理，不读取路径、不访问文件系统、不调用模型或 tokenizer。
/// 【设计原因】保留原始文本全部 byte 且不重叠，使下游可根据 range 重建上下文来源；未知格式使用统一窗口规则。
/// source path/digest 只是调用方提供的身份标签，本函数不核对它们与磁盘文件的对应关系。
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
    // 每一语义 span 从某个行起点延伸至下一个起点；首个起点始终是 source 的第一行。
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
            // 同一语义段若被切成多个窗口，所有窗口共享该 parent id，保留它们原来的分组关系。
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

/// 从 UTF-8 source 的换行字节建立行切片表；不把换行后的空尾部另算一行。
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

/// 按文件扩展名选择最简单可复现的边界识别器。
///
/// Markdown-like 扩展名使用标题起点，列出的常见代码扩展名使用符号前缀识别，其他类型退回 raw window。
/// 该选择只看扩展名，不会检查实际语言、语法是否正确或项目配置。
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

/// 返回各启发式语义段的第一行索引；没有识别出的新边界时整份文本仍由第一段覆盖。
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

/// 检查一行是否以支持的常见声明关键字开头。
///
/// 这是固定前缀表而非 parser：缩进会先由调用方去掉，但修饰符、宏、跨行声明或语言变体可能不识别。
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

/// 将一个语义 span 按字节上限切成 SourceChunk，并为每块生成稳定 provenance 字段。
///
/// parent id 在同一语义段的各窗口间复用；chunk id 还绑定 path/source/range/content/transformation。
/// 一旦超过 `max_chunks` 立即报错，调用方得到 Err 而不是部分 source 覆盖集合。
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

/// 选择不超过目标字节上限的 UTF-8 字符边界，并尽量在更早的换行处结束。
///
/// 如果目标位置落在多字节字符内部，先向前退到合法边界；若一个完整字符本身比预算长，至少推进一个字符，
/// 因此极端字符窗口可能略超目标字节数，但不会切坏 UTF-8。options 的最小 4 字节约束通常可容纳单个 Unicode scalar。
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
        // 在目标范围内优先于换行后切开，降低段落/代码行被任意切断的概率；仍保持字节连续无损。
        let line_boundary = start + newline + 1;
        if line_boundary > start {
            return line_boundary;
        }
    }
    boundary
}

/// 将 byte range 映射回首末行号；end 为右开边界，所以用 end - 1 查找最后一个实际字节所属行。
///
/// 输入通常来自 `lines()` 与 `next_boundary()`，保证落在文本内；`unwrap_or` 为空行表等边缘输入保留兜底位置。
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

/// 对 chunk 内容字节计算 `sha256:` 格式摘要。
fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
