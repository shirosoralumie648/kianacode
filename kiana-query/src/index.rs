//! 工作区文本上下文索引、搜索、向量检索和 artifact 图构建。
//!
//! 该模块把受信项目中的可读文本文件转换为带稳定 schema 的上下文数据。所有扫描都会
//! 先 canonicalize 根目录、复用 repo map 的忽略规则，并对单文件字节数和返回数量设上限；
//! 缓存和 ingest 结果只用于加速与审计展示，不是 EventLog，也不能替代 ControlPlane 的
//! trust、sandbox、路径锁或能力授权。
//!
//! “向量搜索”当前使用本地确定性 hash embedding（`kiana.deterministic-hash-embedding.v1`），
//! 不是在线模型服务。分数、token 估算和依赖边都是启发式结果，调用方应把它们当作上下文
//! 候选，而不是编译器、语义分析器或业务事实的最终结论。

use crate::repo_map::{collect_paths, language_for_path, IgnoreRules};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const DEFAULT_LIMIT: usize = 10;
const DEFAULT_MAX_BYTES_PER_FILE: usize = 128 * 1024;
const DEFAULT_MAX_SNIPPET_LINES: usize = 5;
const DEFAULT_VECTOR_DIMENSIONS: usize = 64;
const DETERMINISTIC_VECTOR_MODEL: &str = "kiana.deterministic-hash-embedding.v1";

#[derive(Debug, Clone, Copy)]
/// 构建文件索引时的单文件读取上限。
pub struct ContextIndexOptions {
    /// 最大字节数；`None` 或 0 使用 128 KiB 默认值。
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextIndexOptions {
    fn default() -> Self {
        Self {
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// 关键词搜索的返回数量和单文件读取上限。
pub struct ContextSearchOptions {
    /// 最多返回多少个命中；`None` 或 0 使用默认值 10。
    pub limit: Option<usize>,
    /// 单文件最大读取字节数。
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextSearchOptions {
    fn default() -> Self {
        Self {
            limit: Some(DEFAULT_LIMIT),
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// 向量搜索的返回数量和单文件读取上限。
pub struct ContextVectorSearchOptions {
    /// 最多返回多少个向量命中；`None` 或 0 使用默认值 10。
    pub limit: Option<usize>,
    /// 单文件最大读取字节数。
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextVectorSearchOptions {
    fn default() -> Self {
        Self {
            limit: Some(DEFAULT_LIMIT),
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// 组装上下文 pack 时的搜索、读取和摘要截断参数。
pub struct ContextPackOptions {
    /// 最多纳入多少个搜索命中。
    pub limit: Option<usize>,
    /// 搜索单文件最大读取字节数。
    pub max_bytes_per_file: Option<usize>,
    /// 每个命中最多保留多少行上下文。
    pub max_snippet_lines: Option<usize>,
}

impl Default for ContextPackOptions {
    fn default() -> Self {
        Self {
            limit: Some(DEFAULT_LIMIT),
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
            max_snippet_lines: Some(DEFAULT_MAX_SNIPPET_LINES),
        }
    }
}

#[derive(Debug, Clone, Copy)]
/// 构建 artifact 元数据时的单文件读取上限。
pub struct ContextArtifactOptions {
    /// 最大字节数；超限、二进制或非法 UTF-8 文件会被跳过。
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextArtifactOptions {
    fn default() -> Self {
        Self {
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone)]
/// 从一个源目录 ingest artifact 到受控存储目录时的参数。
pub struct ContextArtifactIngestOptions {
    /// ingest 存储目录；相对路径按目标根目录解析。
    pub store_dir: Option<PathBuf>,
    /// 源文件的最大读取字节数。
    pub max_bytes_per_file: Option<usize>,
}

impl Default for ContextArtifactIngestOptions {
    fn default() -> Self {
        Self {
            store_dir: Some(PathBuf::from(".kiana/context-ingest")),
            max_bytes_per_file: Some(DEFAULT_MAX_BYTES_PER_FILE),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 文件内容索引及缓存状态报告。
pub struct ContextIndex {
    /// 稳定 schema 标识。
    pub schema: String,
    /// canonicalize 后的索引根路径。
    pub root: String,
    /// 成功纳入的文件数量。
    pub files_indexed: usize,
    /// 因忽略、大小、二进制或编码原因跳过的文件数量。
    pub skipped_files: usize,
    /// 纳入文件的字节总数。
    pub total_bytes: u64,
    /// 按相对路径稳定排序的文件条目。
    pub files: Vec<ContextIndexedFile>,
    /// 使用持久缓存时的复用/变更统计。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<ContextIndexCacheReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 索引中的单个文本文件元数据。
pub struct ContextIndexedFile {
    /// 相对于索引根的正斜杠路径。
    pub path: String,
    /// 由扩展名推断的语言标签。
    pub language: Option<String>,
    /// 文件字节数。
    pub bytes: u64,
    /// 按换行计算的行数。
    pub line_count: usize,
    /// 内容 hash，用于缓存复用和变更比较。
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 仅包含文件级 artifact 元数据的集合。
pub struct ContextArtifacts {
    /// 稳定 schema 标识。
    pub schema: String,
    /// artifact 根目录。
    pub root: String,
    /// 成功生成的 artifact 数量。
    pub files_indexed: usize,
    /// 被过滤或读取失败而跳过的数量。
    pub skipped_files: usize,
    /// artifact 条目列表。
    pub artifacts: Vec<ContextArtifactItem>,
    /// 可选的持久缓存差异报告。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<ContextArtifactsCacheReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 一个文件 artifact 的稳定身份与内容摘要。
pub struct ContextArtifactItem {
    /// 由路径和内容 hash 组成的稳定 ID。
    pub id: String,
    /// 当前实现固定为 `file`。
    pub kind: String,
    /// 相对于根目录的文件路径。
    pub path: String,
    /// 由扩展名推断的语言。
    pub language: Option<String>,
    /// 文件字节数。
    pub bytes: u64,
    /// 文件行数。
    pub line_count: usize,
    /// 文件内容 hash。
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 将源目录文件复制到 ingest store 后的清单报告。
pub struct ContextArtifactIngest {
    /// ingest 报告 schema。
    pub schema: String,
    /// 目标项目根目录。
    pub root: String,
    /// 被扫描的源目录。
    pub source_root: String,
    /// 相对于目标根目录的存储目录。
    pub store_dir: String,
    /// 相对于目标根目录的 manifest 路径。
    pub manifest_path: String,
    /// artifact 条目的 schema。
    pub artifacts_schema: String,
    /// 成功复制并登记的文件数。
    pub ingested_files: usize,
    /// 被跳过的文件数。
    pub skipped_files: usize,
    /// 成功复制文件的总字节数。
    pub total_bytes: u64,
    /// 与上一次 manifest 的同步差异。
    #[serde(default)]
    pub sync: ContextArtifactIngestSyncReport,
    pub artifacts: Vec<ContextIngestedArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// ingest 与旧 manifest 之间的文件级同步统计。
pub struct ContextArtifactIngestSyncReport {
    /// 对比所依据的 manifest 路径。
    pub path: String,
    /// `created`、`unchanged` 或 `updated` 等当前状态标签。
    pub status: String,
    /// 内容 hash 未变且可复用的文件数。
    pub reused_files: usize,
    /// 新出现的文件数。
    pub added_files: usize,
    /// 内容发生变化的文件数。
    pub changed_files: usize,
    /// 上一次存在但本次消失的文件数。
    pub removed_files: usize,
}

impl Default for ContextArtifactIngestSyncReport {
    fn default() -> Self {
        Self {
            path: String::new(),
            status: "created".to_string(),
            reused_files: 0,
            added_files: 0,
            changed_files: 0,
            removed_files: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 被复制到 ingest store 的单个 artifact。
pub struct ContextIngestedArtifact {
    /// 稳定 artifact ID。
    pub id: String,
    /// 当前固定为 `file`。
    pub kind: String,
    /// 源目录中的相对路径。
    pub source_path: String,
    /// ingest store 中的相对路径。
    pub stored_path: String,
    /// 语言标签。
    pub language: Option<String>,
    /// 复制后的字节数。
    pub bytes: u64,
    /// 复制后的行数。
    pub line_count: usize,
    /// 内容 hash。
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// artifact 缓存的新增、复用、变更和删除统计。
pub struct ContextArtifactsCacheReport {
    /// 缓存文件路径。
    pub path: String,
    /// 当前缓存状态标签。
    pub status: String,
    /// 未变更而复用的 artifact 数量。
    pub reused_artifacts: usize,
    /// 新增 artifact 数量。
    pub added_artifacts: usize,
    /// hash 变化的 artifact 数量。
    pub changed_artifacts: usize,
    /// 本次不再存在的 artifact 数量。
    pub removed_artifacts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 包含 artifact 集合和依赖关系图的组合存储报告。
pub struct ContextArtifactStore {
    /// 存储报告 schema。
    pub schema: String,
    /// canonicalize 后的项目根。
    pub root: String,
    /// artifact 集合 schema。
    pub artifacts_schema: String,
    /// 依赖图 schema。
    pub dependency_graph_schema: String,
    /// artifact 节点数量。
    pub artifact_count: usize,
    /// 依赖边数量。
    pub dependency_count: usize,
    /// 按角色归类的 artifact 数量。
    pub artifact_roles: Vec<ContextArtifactRoleSummary>,
    /// 文件 artifact 集合。
    pub artifacts: ContextArtifacts,
    /// 依赖图。
    pub dependency_graph: ContextArtifactDependencyGraph,
    /// 可选持久缓存差异报告。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<ContextArtifactStoreCacheReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 某一 artifact 角色的计数汇总。
pub struct ContextArtifactRoleSummary {
    /// 角色名，例如 `source`、`test` 或 `prd`。
    pub role: String,
    /// 该角色的 artifact 数量。
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// artifact store 是否具备规划所需角色的就绪报告。
pub struct ContextArtifactReadiness {
    /// 就绪报告 schema。
    pub schema: String,
    /// 项目根路径。
    pub root: String,
    /// 使用的 artifact store schema。
    pub artifact_store_schema: String,
    /// `ready` 或 `incomplete`。
    pub status: String,
    /// artifact 总数。
    pub artifact_count: usize,
    /// 依赖边总数。
    pub dependency_count: usize,
    /// 每个必需角色的存在情况。
    pub required_roles: Vec<ContextArtifactReadinessRole>,
    /// 缺失的角色名。
    pub missing_roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 单个必需 artifact 角色的就绪明细。
pub struct ContextArtifactReadinessRole {
    /// 角色名。
    pub role: String,
    /// 当前实现是否要求该角色。
    pub required: bool,
    /// 是否至少找到一个该角色 artifact。
    pub present: bool,
    /// 找到的数量。
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// artifact store 持久缓存的差异报告。
pub struct ContextArtifactStoreCacheReport {
    /// 缓存文件路径。
    pub path: String,
    /// 当前缓存状态标签。
    pub status: String,
    /// 复用的 artifact 数量。
    pub reused_artifacts: usize,
    /// 新增的 artifact 数量。
    pub added_artifacts: usize,
    /// 变更的 artifact 数量。
    pub changed_artifacts: usize,
    /// 移除的 artifact 数量。
    pub removed_artifacts: usize,
    /// 复用的依赖边数量。
    pub reused_dependencies: usize,
    /// 新增的依赖边数量。
    pub added_dependencies: usize,
    /// 移除的依赖边数量。
    pub removed_dependencies: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 文件 artifact 之间的启发式依赖关系图。
pub struct ContextArtifactDependencyGraph {
    /// 依赖图 schema。
    pub schema: String,
    /// 图对应的项目根。
    pub root: String,
    /// 文件节点列表。
    pub nodes: Vec<ContextArtifactDependencyNode>,
    /// `test_of` 和 `path_reference` 等关系边。
    pub edges: Vec<ContextArtifactDependencyEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 依赖图中的一个 artifact 节点。
pub struct ContextArtifactDependencyNode {
    /// 与 artifact 集合对应的稳定 ID。
    pub id: String,
    /// 节点类型，当前通常为 `file`。
    pub kind: String,
    /// 相对文件路径。
    pub path: String,
    /// 语言标签。
    pub language: Option<String>,
    /// 内容 hash。
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 依赖图中的一条有向关系及其证据文本。
pub struct ContextArtifactDependencyEdge {
    /// 源节点 ID。
    pub source: String,
    /// 目标节点 ID。
    pub target: String,
    /// 关系类型。
    pub relation: String,
    /// 用于审计和人工理解的路径匹配说明。
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 持久化文件索引的复用和变更统计。
pub struct ContextIndexCacheReport {
    /// 缓存文件路径。
    pub path: String,
    /// 当前缓存状态标签。
    pub status: String,
    /// hash 未变并复用的文件数。
    pub reused_files: usize,
    /// 新增文件数。
    pub added_files: usize,
    /// 内容变化文件数。
    pub changed_files: usize,
    /// 被移除文件数。
    pub removed_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 关键词上下文搜索的完整结果。
pub struct ContextSearchResults {
    /// 搜索结果 schema。
    pub schema: String,
    /// 搜索根目录。
    pub root: String,
    /// 调用方提交的原始查询（去除首尾空白）。
    pub query: String,
    /// 分词后用于匹配的非空词项。
    pub terms: Vec<String>,
    /// 实际采用的返回上限。
    pub limit: usize,
    /// 成功读取并索引的文件数。
    pub files_indexed: usize,
    /// 被跳过的文件数。
    pub skipped_files: usize,
    /// 按分数、路径和行号稳定排序的命中。
    pub hits: Vec<ContextSearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 关键词搜索中的单行命中。
pub struct ContextSearchHit {
    /// 相对文件路径。
    pub path: String,
    /// 语言标签。
    pub language: Option<String>,
    /// 词项和出现次数计算出的整数分数。
    pub score: u64,
    /// 该行及文件中词项的累计出现数。
    pub occurrences: u64,
    /// 实际匹配到的词项。
    pub matched_terms: Vec<String>,
    /// 一基行号。
    pub line_number: usize,
    /// 命中的原始文本行。
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 确定性 hash embedding 向量搜索的完整结果。
pub struct ContextVectorSearchResults {
    /// 搜索结果 schema。
    pub schema: String,
    /// 搜索根目录。
    pub root: String,
    /// 原始查询文本。
    pub query: String,
    /// 分词后的查询词项。
    pub terms: Vec<String>,
    /// 当前使用的本地 embedding 模型标签。
    pub embedding_model: String,
    /// 向量维数。
    pub dimensions: usize,
    /// 实际采用的返回上限。
    pub limit: usize,
    /// 成功读取文件数。
    pub files_indexed: usize,
    /// 被过滤或读取失败的文件数。
    pub skipped_files: usize,
    /// 按相似度、词项重叠、路径和行号稳定排序的命中。
    pub hits: Vec<ContextVectorSearchHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 向量搜索中的单行命中。
pub struct ContextVectorSearchHit {
    /// 相对文件路径。
    pub path: String,
    /// 语言标签。
    pub language: Option<String>,
    /// 文件内容 hash。
    pub content_hash: String,
    /// 查询向量与该行向量的相似度。
    pub score: f64,
    /// 查询词在该行的重叠数量。
    pub token_overlap: usize,
    /// 一基行号。
    pub line_number: usize,
    /// 命中的原始文本行。
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 供模型消费的组合上下文包。
pub struct ContextPack {
    /// context pack schema。
    pub schema: String,
    /// 搜索根目录。
    pub root: String,
    /// 原始查询文本。
    pub query: String,
    /// 分词后的查询词项。
    pub terms: Vec<String>,
    /// 实际采用的命中上限。
    pub limit: usize,
    /// 每个 snippet 的最大行数。
    pub max_snippet_lines: usize,
    /// 成功读取文件数。
    pub files_indexed: usize,
    /// 被跳过文件数。
    pub skipped_files: usize,
    /// 截断后的上下文片段。
    pub snippets: Vec<ContextPackSnippet>,
    /// 与当前上下文关联的 artifact 图。
    pub artifact_graph: ContextArtifactGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// 上下文包中的一个连续代码/文本片段。
pub struct ContextPackSnippet {
    /// 相对文件路径。
    pub path: String,
    /// 语言标签。
    pub language: Option<String>,
    /// 文件内容 hash。
    pub content_hash: String,
    /// 关键词匹配分数。
    pub score: u64,
    /// 匹配出现次数。
    pub occurrences: u64,
    /// 匹配到的词项。
    pub matched_terms: Vec<String>,
    /// 片段起始一基行号。
    pub start_line: usize,
    /// 片段结束一基行号。
    pub end_line: usize,
    /// 实际发送给模型的摘录文本。
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextArtifactGraph {
    pub schema: String,
    pub nodes: Vec<ContextArtifactNode>,
    pub edges: Vec<ContextArtifactEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextArtifactNode {
    pub id: String,
    pub kind: String,
    pub path: String,
    pub language: Option<String>,
    pub content_hash: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextArtifactEdge {
    pub source: String,
    pub target: String,
    pub relation: String,
    pub matched_terms: Vec<String>,
}

/// 扫描工作区并生成当前内容的文件索引。
///
/// 每个候选文件先经过忽略规则、二进制/UTF-8 检查和 `max_bytes_per_file` 限制；成功条目
/// 保存路径、语言、行数和内容 hash。函数只读文件，不写缓存；需要持久化时调用
/// [`build_persistent_context_index`]。
pub fn build_context_index(
    root: impl AsRef<Path>,
    options: ContextIndexOptions,
) -> Result<ContextIndex> {
    let root = canonical_root(root.as_ref(), "context index")?;
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let mut files = Vec::new();
    let mut skipped_files = 0;
    let mut total_bytes = 0;

    for path in candidate_paths(&root)? {
        let Some(indexed) = index_file(&root, &path, max_bytes)? else {
            skipped_files += 1;
            continue;
        };
        total_bytes += indexed.bytes;
        files.push(indexed);
    }

    Ok(ContextIndex {
        schema: "kiana.context-index.v1".to_string(),
        root: display_path(&root),
        files_indexed: files.len(),
        skipped_files,
        total_bytes,
        files,
        cache: None,
    })
}

/// 扫描工作区并生成文件 artifact 元数据集合。
///
/// artifact ID 同时包含相对路径和内容 hash，因此同一路径内容变化会产生新身份。结果
/// 仍是内存报告，不代表 artifact 已复制到独立存储或已进入 EventLog。
pub fn build_context_artifacts(
    root: impl AsRef<Path>,
    options: ContextArtifactOptions,
) -> Result<ContextArtifacts> {
    let root = canonical_root(root.as_ref(), "context artifacts")?;
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let mut artifacts = Vec::new();
    let mut skipped_files = 0;

    for path in candidate_paths(&root)? {
        let Some(indexed) = index_file(&root, &path, max_bytes)? else {
            skipped_files += 1;
            continue;
        };
        let id = format!("file:{}:{}", indexed.path, indexed.content_hash);
        artifacts.push(ContextArtifactItem {
            id,
            kind: "file".to_string(),
            path: indexed.path,
            language: indexed.language,
            bytes: indexed.bytes,
            line_count: indexed.line_count,
            content_hash: indexed.content_hash,
        });
    }

    Ok(ContextArtifacts {
        schema: "kiana.context-artifacts.v1".to_string(),
        root: display_path(&root),
        files_indexed: artifacts.len(),
        skipped_files,
        artifacts,
        cache: None,
    })
}

/// 构建 artifact 集合并将完整报告写入指定 JSON 缓存文件。
///
/// 相对缓存路径按项目根解析，父目录会按需创建；写入失败会返回错误。缓存差异统计只
/// 比较报告内容，不能证明写入已经经过 `fsync` 或跨进程原子替换。
pub fn build_persistent_context_artifacts(
    root: impl AsRef<Path>,
    options: ContextArtifactOptions,
    cache_path: impl AsRef<Path>,
) -> Result<ContextArtifacts> {
    let mut report = build_context_artifacts(root, options)?;
    let root = PathBuf::from(&report.root);
    let cache_path = normalize_cache_path(&root, cache_path.as_ref());
    let previous = read_cached_artifacts(&cache_path)?;
    let cache = artifacts_cache_report(&cache_path, &previous, &report);

    report.cache = Some(cache);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create context artifacts cache dir {}",
                parent.display()
            )
        })?;
    }
    fs::write(&cache_path, serde_json::to_string_pretty(&report)? + "\n").with_context(|| {
        format!(
            "failed to write context artifacts cache {}",
            cache_path.display()
        )
    })?;

    Ok(report)
}

/// 将源目录中的可识别文本复制到受控 ingest store，并写出 manifest。
///
/// `source_root` 不得位于 store 内部，以免扫描到自己生成的文件。每次调用会先清空 store
/// 的 `files/` 子目录，再按稳定候选路径复制并登记文件；因此这是有明确覆盖行为的同步
/// 操作，调用方应在合适的授权上下文中调用。失败时可能已完成部分文件清理或复制，不能
/// 把函数错误解释成源目录未变化。
pub fn ingest_context_artifacts(
    root: impl AsRef<Path>,
    source_root: impl AsRef<Path>,
    options: ContextArtifactIngestOptions,
) -> Result<ContextArtifactIngest> {
    let root = canonical_root(root.as_ref(), "context artifact ingest root")?;
    let source_root = canonical_root(source_root.as_ref(), "context artifact ingest source")?;
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let store_dir = resolve_ingest_store_dir(
        &root,
        options
            .store_dir
            .as_deref()
            .unwrap_or_else(|| Path::new(".kiana/context-ingest")),
    )?;
    let manifest_path = store_dir.join("manifest.json");
    let files_dir = store_dir.join("files");
    let previous_manifest = read_cached_ingest_manifest(&manifest_path)?;
    if source_root.starts_with(&store_dir) {
        return Err(anyhow!(
            "context artifact ingest source must not be inside the ingest store"
        ));
    }
    match fs::remove_dir_all(&files_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to clear context artifact ingest files {}",
                    files_dir.display()
                )
            });
        }
    }
    let mut artifacts = Vec::new();
    let mut skipped_files = 0;
    let mut total_bytes = 0_u64;

    for path in candidate_paths(&source_root)? {
        let Some(artifact) =
            ingest_artifact_file(&root, &source_root, &path, &files_dir, max_bytes)?
        else {
            skipped_files += 1;
            continue;
        };
        total_bytes += artifact.bytes;
        artifacts.push(artifact);
    }

    let report = ContextArtifactIngest {
        schema: "kiana.context-artifact-ingest.v1".to_string(),
        root: display_path(&root),
        source_root: display_path(&source_root),
        store_dir: relative_or_display_path(&root, &store_dir),
        manifest_path: relative_or_display_path(&root, &manifest_path),
        artifacts_schema: "kiana.context-artifacts.v1".to_string(),
        ingested_files: artifacts.len(),
        skipped_files,
        total_bytes,
        sync: ingest_sync_report(&root, &manifest_path, &previous_manifest, &artifacts),
        artifacts,
    };

    fs::create_dir_all(&store_dir).with_context(|| {
        format!(
            "failed to create context artifact ingest store {}",
            store_dir.display()
        )
    })?;
    fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&report)? + "\n",
    )
    .with_context(|| {
        format!(
            "failed to write context artifact ingest manifest {}",
            manifest_path.display()
        )
    })?;

    Ok(report)
}

/// 根据 artifact 路径关系和测试命名约定构建启发式依赖图。
///
/// `test_of` 边来自测试目标路径推断，`path_reference` 边来自文件内容中出现目标路径；
/// 两者都附带 evidence 文本并稳定排序。图不是完整编译依赖或 import 图，缺边不能证明
/// 没有依赖，误边也不能直接作为执行授权依据。
pub fn build_context_artifact_dependency_graph(
    root: impl AsRef<Path>,
    options: ContextArtifactOptions,
) -> Result<ContextArtifactDependencyGraph> {
    let report = build_context_artifacts(root, options)?;
    let root = PathBuf::from(&report.root);
    let nodes = report
        .artifacts
        .iter()
        .map(|artifact| ContextArtifactDependencyNode {
            id: artifact.id.clone(),
            kind: artifact.kind.clone(),
            path: artifact.path.clone(),
            language: artifact.language.clone(),
            content_hash: artifact.content_hash.clone(),
        })
        .collect::<Vec<_>>();
    let artifact_by_path = report
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact))
        .collect::<BTreeMap<_, _>>();
    let mut edges = Vec::new();

    for artifact in &report.artifacts {
        let Some(target_path) = test_target_path(&artifact.path) else {
            continue;
        };
        let Some(target) = artifact_by_path.get(target_path.as_str()) else {
            continue;
        };
        edges.push(ContextArtifactDependencyEdge {
            source: artifact.id.clone(),
            target: target.id.clone(),
            relation: "test_of".to_string(),
            evidence: format!("{} matches {}", artifact.path, target.path),
        });
    }

    for artifact in &report.artifacts {
        let path = root.join(&artifact.path);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        for target in &report.artifacts {
            if artifact.path == target.path || !content.contains(&target.path) {
                continue;
            }
            edges.push(ContextArtifactDependencyEdge {
                source: artifact.id.clone(),
                target: target.id.clone(),
                relation: "path_reference".to_string(),
                evidence: format!("{} references {}", artifact.path, target.path),
            });
        }
    }

    edges.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then(left.target.cmp(&right.target))
            .then(left.relation.cmp(&right.relation))
            .then(left.evidence.cmp(&right.evidence))
    });

    Ok(ContextArtifactDependencyGraph {
        schema: "kiana.context-artifact-dependency-graph.v1".to_string(),
        root: display_path(&root),
        nodes,
        edges,
    })
}

/// 一次性构建 artifact 集合、依赖图和角色汇总。
///
/// 为保持同一快照，函数使用相同选项分别构建集合和图；底层文件在两次读取间发生变化
/// 时，调用方应把结果视为本地启发式报告而不是原子快照。
pub fn build_context_artifact_store(
    root: impl AsRef<Path>,
    options: ContextArtifactOptions,
) -> Result<ContextArtifactStore> {
    let root = canonical_root(root.as_ref(), "context artifact store")?;
    let artifacts = build_context_artifacts(&root, options)?;
    let dependency_graph = build_context_artifact_dependency_graph(&root, options)?;

    Ok(ContextArtifactStore {
        schema: "kiana.context-artifact-store.v1".to_string(),
        root: display_path(&root),
        artifacts_schema: artifacts.schema.clone(),
        dependency_graph_schema: dependency_graph.schema.clone(),
        artifact_count: artifacts.artifacts.len(),
        dependency_count: dependency_graph.edges.len(),
        artifact_roles: artifact_role_summary(&artifacts.artifacts),
        artifacts,
        dependency_graph,
        cache: None,
    })
}

/// 构建 artifact store 并将报告写入 JSON 缓存。
///
/// 该函数会创建缓存父目录并覆盖目标文件；缓存只用于后续差异统计，不改变源文件，也
/// 不提供跨进程锁或崩溃恢复保证。
pub fn build_persistent_context_artifact_store(
    root: impl AsRef<Path>,
    options: ContextArtifactOptions,
    cache_path: impl AsRef<Path>,
) -> Result<ContextArtifactStore> {
    let mut store = build_context_artifact_store(root, options)?;
    let root = PathBuf::from(&store.root);
    let cache_path = normalize_cache_path(&root, cache_path.as_ref());
    let previous = read_cached_artifact_store(&cache_path)?;
    let cache = artifact_store_cache_report(&cache_path, &previous, &store);

    store.cache = Some(cache);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create context artifact store cache dir {}",
                parent.display()
            )
        })?;
    }
    fs::write(&cache_path, serde_json::to_string_pretty(&store)? + "\n").with_context(|| {
        format!(
            "failed to write context artifact store cache {}",
            cache_path.display()
        )
    })?;

    Ok(store)
}

/// 根据 artifact 角色是否齐全生成规划输入就绪报告。
///
/// 当前实现固定要求 `prd`、`design`、`tasks`、`source`、`test` 五类角色各至少一个。
/// “ready”只表示扫描结果满足数量条件，不表示内容质量、验收标准或代码正确性已经审核。
pub fn build_context_artifact_readiness(
    root: impl AsRef<Path>,
    options: ContextArtifactOptions,
) -> Result<ContextArtifactReadiness> {
    let store = build_context_artifact_store(root, options)?;
    let role_counts = store
        .artifact_roles
        .iter()
        .map(|role| (role.role.as_str(), role.count))
        .collect::<BTreeMap<_, _>>();
    let mut required_roles = Vec::new();
    let mut missing_roles = Vec::new();

    for role in ["prd", "design", "tasks", "source", "test"] {
        let count = *role_counts.get(role).unwrap_or(&0);
        let present = count > 0;
        if !present {
            missing_roles.push(role.to_string());
        }
        required_roles.push(ContextArtifactReadinessRole {
            role: role.to_string(),
            required: true,
            present,
            count,
        });
    }

    Ok(ContextArtifactReadiness {
        schema: "kiana.context-artifact-readiness.v1".to_string(),
        root: store.root,
        artifact_store_schema: store.schema,
        status: if missing_roles.is_empty() {
            "ready".to_string()
        } else {
            "incomplete".to_string()
        },
        artifact_count: store.artifact_count,
        dependency_count: store.dependency_count,
        required_roles,
        missing_roles,
    })
}

/// 构建文件索引并将结果写入 JSON 缓存文件。
///
/// 函数先读取旧缓存计算复用/新增/变更/删除统计，再覆盖写入新索引；任何缓存读取或
/// 写入错误都会返回，而不会退回空索引。
pub fn build_persistent_context_index(
    root: impl AsRef<Path>,
    options: ContextIndexOptions,
    cache_path: impl AsRef<Path>,
) -> Result<ContextIndex> {
    let mut index = build_context_index(root, options)?;
    let root = PathBuf::from(&index.root);
    let cache_path = normalize_cache_path(&root, cache_path.as_ref());
    let previous = read_cached_index(&cache_path)?;
    let report = cache_report(&cache_path, &previous, &index);

    index.cache = Some(report);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create context index cache dir {}",
                parent.display()
            )
        })?;
    }
    fs::write(&cache_path, serde_json::to_string_pretty(&index)? + "\n").with_context(|| {
        format!(
            "failed to write context index cache {}",
            cache_path.display()
        )
    })?;

    Ok(index)
}

/// 对工作区文本执行确定性的大小写不敏感关键词搜索。
///
/// 空查询直接报错；命中按分数、路径和行号稳定排序后截断到 `limit`。文件读取、编码和
/// 大小限制与索引路径一致，结果中的 `skipped_files` 必须与“没有命中”区分开。
pub fn search_context_index(
    root: impl AsRef<Path>,
    query: &str,
    options: ContextSearchOptions,
) -> Result<ContextSearchResults> {
    let root = canonical_root(root.as_ref(), "context search")?;
    let terms = query_terms(query);
    if terms.is_empty() {
        return Err(anyhow!(
            "context search query must contain at least one term"
        ));
    }
    let limit = options
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let mut hits = Vec::new();
    let mut files_indexed = 0;
    let mut skipped_files = 0;

    for path in candidate_paths(&root)? {
        let Some(hit) = search_file(&root, &path, &terms, max_bytes)? else {
            skipped_files += 1;
            continue;
        };
        files_indexed += 1;
        if hit.score > 0 {
            hits.push(hit);
        }
    }

    hits.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(left.path.cmp(&right.path))
            .then(left.line_number.cmp(&right.line_number))
    });
    hits.truncate(limit);

    Ok(ContextSearchResults {
        schema: "kiana.context-search.v1".to_string(),
        root: display_path(&root),
        query: query.trim().to_string(),
        terms,
        limit,
        files_indexed,
        skipped_files,
        hits,
    })
}

/// 使用本地确定性 hash embedding 对工作区执行近似向量搜索。
///
/// 查询和每个候选文件都转成固定 64 维向量，再结合 token overlap 排序。该函数不访问
/// 网络、不调用 live provider；相似度是检索提示，不是语义正确性的证明。
pub fn search_context_vectors(
    root: impl AsRef<Path>,
    query: &str,
    options: ContextVectorSearchOptions,
) -> Result<ContextVectorSearchResults> {
    let root = canonical_root(root.as_ref(), "context vector search")?;
    let terms = query_terms(query);
    if terms.is_empty() {
        return Err(anyhow!(
            "context vector search query must contain at least one term"
        ));
    }
    let limit = options
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let query_embedding =
        normalized_hash_embedding(vector_features(query, None), DEFAULT_VECTOR_DIMENSIONS);
    let mut hits = Vec::new();
    let mut files_indexed = 0;
    let mut skipped_files = 0;

    for path in candidate_paths(&root)? {
        let Some(hit) = vector_search_file(
            &root,
            &path,
            &terms,
            &query_embedding,
            max_bytes,
            DEFAULT_VECTOR_DIMENSIONS,
        )?
        else {
            skipped_files += 1;
            continue;
        };
        files_indexed += 1;
        if hit.score > 0.0 {
            hits.push(hit);
        }
    }

    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(right.token_overlap.cmp(&left.token_overlap))
            .then(left.path.cmp(&right.path))
            .then(left.line_number.cmp(&right.line_number))
    });
    hits.truncate(limit);

    Ok(ContextVectorSearchResults {
        schema: "kiana.context-vector-search.v1".to_string(),
        root: display_path(&root),
        query: query.trim().to_string(),
        terms,
        embedding_model: DETERMINISTIC_VECTOR_MODEL.to_string(),
        dimensions: DEFAULT_VECTOR_DIMENSIONS,
        limit,
        files_indexed,
        skipped_files,
        hits,
    })
}

/// 将关键词命中截取为受行数限制的上下文片段，并附加 artifact 图。
///
/// 片段会再次读取文件以生成相邻行摘录，因此文件在搜索后被修改时，hash/行文可能来自
/// 不同瞬间；调用方应把 pack 当作短期上下文候选。`max_snippet_lines` 和 `limit` 均会
/// 归一化为正值默认值，防止无限输出。
pub fn build_context_pack(
    root: impl AsRef<Path>,
    query: &str,
    options: ContextPackOptions,
) -> Result<ContextPack> {
    let root = canonical_root(root.as_ref(), "context pack")?;
    let terms = query_terms(query);
    if terms.is_empty() {
        return Err(anyhow!("context pack query must contain at least one term"));
    }
    let limit = options
        .limit
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_LIMIT);
    let max_bytes = options
        .max_bytes_per_file
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_BYTES_PER_FILE);
    let max_snippet_lines = options
        .max_snippet_lines
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_SNIPPET_LINES);
    let mut snippets = Vec::new();
    let mut files_indexed = 0;
    let mut skipped_files = 0;

    for path in candidate_paths(&root)? {
        let Some(snippet) = pack_file(&root, &path, &terms, max_bytes, max_snippet_lines)? else {
            skipped_files += 1;
            continue;
        };
        files_indexed += 1;
        if snippet.score > 0 {
            snippets.push(snippet);
        }
    }

    snippets.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then(left.path.cmp(&right.path))
            .then(left.start_line.cmp(&right.start_line))
    });
    snippets.truncate(limit);
    let artifact_graph = context_artifact_graph(&snippets, query.trim());

    Ok(ContextPack {
        schema: "kiana.context-pack.v1".to_string(),
        root: display_path(&root),
        query: query.trim().to_string(),
        terms,
        limit,
        max_snippet_lines,
        files_indexed,
        skipped_files,
        snippets,
        artifact_graph,
    })
}

fn canonical_root(root: &Path, label: &str) -> Result<PathBuf> {
    fs::canonicalize(root)
        .with_context(|| format!("failed to resolve {label} root {}", root.display()))
}

fn normalize_cache_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

enum CachedContextIndex {
    Missing,
    Valid(ContextIndex),
    Invalid,
}

enum CachedContextArtifacts {
    Missing,
    Valid(ContextArtifacts),
    Invalid,
}

enum CachedContextArtifactStore {
    Missing,
    Valid(ContextArtifactStore),
    Invalid,
}

enum CachedContextArtifactIngest {
    Missing,
    Valid(ContextArtifactIngest),
    Invalid,
}

fn read_cached_index(path: &Path) -> Result<CachedContextIndex> {
    match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(index) => Ok(CachedContextIndex::Valid(index)),
            Err(_) => Ok(CachedContextIndex::Invalid),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CachedContextIndex::Missing)
        }
        Err(error) => Err(error)
            .with_context(|| format!("failed to read context index cache {}", path.display())),
    }
}

fn read_cached_artifacts(path: &Path) -> Result<CachedContextArtifacts> {
    match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(report) => Ok(CachedContextArtifacts::Valid(report)),
            Err(_) => Ok(CachedContextArtifacts::Invalid),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CachedContextArtifacts::Missing)
        }
        Err(error) => Err(error)
            .with_context(|| format!("failed to read context artifacts cache {}", path.display())),
    }
}

fn read_cached_artifact_store(path: &Path) -> Result<CachedContextArtifactStore> {
    match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(store) => Ok(CachedContextArtifactStore::Valid(store)),
            Err(_) => Ok(CachedContextArtifactStore::Invalid),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CachedContextArtifactStore::Missing)
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to read context artifact store cache {}",
                path.display()
            )
        }),
    }
}

fn read_cached_ingest_manifest(path: &Path) -> Result<CachedContextArtifactIngest> {
    match fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(report) => Ok(CachedContextArtifactIngest::Valid(report)),
            Err(_) => Ok(CachedContextArtifactIngest::Invalid),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CachedContextArtifactIngest::Missing)
        }
        Err(error) => Err(error).with_context(|| {
            format!(
                "failed to read context artifact ingest manifest {}",
                path.display()
            )
        }),
    }
}

fn cache_report(
    path: &Path,
    previous: &CachedContextIndex,
    current: &ContextIndex,
) -> ContextIndexCacheReport {
    let previous = match previous {
        CachedContextIndex::Valid(previous) => previous,
        CachedContextIndex::Missing | CachedContextIndex::Invalid => {
            return ContextIndexCacheReport {
                path: portable_path(path),
                status: match previous {
                    CachedContextIndex::Missing => "created",
                    CachedContextIndex::Invalid => "recovered",
                    CachedContextIndex::Valid(_) => unreachable!(),
                }
                .to_string(),
                reused_files: 0,
                added_files: current.files.len(),
                changed_files: 0,
                removed_files: 0,
            };
        }
    };

    let previous_files = previous
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let current_files = current
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut reused_files = 0;
    let mut added_files = 0;
    let mut changed_files = 0;

    for (path, hash) in &current_files {
        match previous_files.get(*path) {
            Some(previous_hash) if *previous_hash == *hash => reused_files += 1,
            Some(_) => changed_files += 1,
            None => added_files += 1,
        }
    }

    let removed_files = previous_files
        .keys()
        .filter(|path| !current_files.contains_key(*path))
        .count();

    ContextIndexCacheReport {
        path: portable_path(path),
        status: "updated".to_string(),
        reused_files,
        added_files,
        changed_files,
        removed_files,
    }
}

fn ingest_sync_report(
    root: &Path,
    path: &Path,
    previous: &CachedContextArtifactIngest,
    current_artifacts: &[ContextIngestedArtifact],
) -> ContextArtifactIngestSyncReport {
    let previous = match previous {
        CachedContextArtifactIngest::Valid(previous) => previous,
        CachedContextArtifactIngest::Missing | CachedContextArtifactIngest::Invalid => {
            return ContextArtifactIngestSyncReport {
                path: relative_or_display_path(root, path),
                status: match previous {
                    CachedContextArtifactIngest::Missing => "created",
                    CachedContextArtifactIngest::Invalid => "recovered",
                    CachedContextArtifactIngest::Valid(_) => unreachable!(),
                }
                .to_string(),
                reused_files: 0,
                added_files: current_artifacts.len(),
                changed_files: 0,
                removed_files: 0,
            };
        }
    };

    let previous_artifacts = previous
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.source_path.as_str(),
                artifact.content_hash.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let current_artifacts = current_artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.source_path.as_str(),
                artifact.content_hash.as_str(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut reused_files = 0;
    let mut added_files = 0;
    let mut changed_files = 0;

    for (path, hash) in &current_artifacts {
        match previous_artifacts.get(*path) {
            Some(previous_hash) if *previous_hash == *hash => reused_files += 1,
            Some(_) => changed_files += 1,
            None => added_files += 1,
        }
    }

    let removed_files = previous_artifacts
        .keys()
        .filter(|path| !current_artifacts.contains_key(*path))
        .count();

    ContextArtifactIngestSyncReport {
        path: relative_or_display_path(root, path),
        status: "updated".to_string(),
        reused_files,
        added_files,
        changed_files,
        removed_files,
    }
}

fn artifacts_cache_report(
    path: &Path,
    previous: &CachedContextArtifacts,
    current: &ContextArtifacts,
) -> ContextArtifactsCacheReport {
    let previous = match previous {
        CachedContextArtifacts::Valid(previous) => previous,
        CachedContextArtifacts::Missing | CachedContextArtifacts::Invalid => {
            return ContextArtifactsCacheReport {
                path: portable_path(path),
                status: match previous {
                    CachedContextArtifacts::Missing => "created",
                    CachedContextArtifacts::Invalid => "recovered",
                    CachedContextArtifacts::Valid(_) => unreachable!(),
                }
                .to_string(),
                reused_artifacts: 0,
                added_artifacts: current.artifacts.len(),
                changed_artifacts: 0,
                removed_artifacts: 0,
            };
        }
    };

    let previous_artifacts = previous
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let current_artifacts = current
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut reused_artifacts = 0;
    let mut added_artifacts = 0;
    let mut changed_artifacts = 0;

    for (path, hash) in &current_artifacts {
        match previous_artifacts.get(*path) {
            Some(previous_hash) if *previous_hash == *hash => reused_artifacts += 1,
            Some(_) => changed_artifacts += 1,
            None => added_artifacts += 1,
        }
    }

    let removed_artifacts = previous_artifacts
        .keys()
        .filter(|path| !current_artifacts.contains_key(*path))
        .count();

    ContextArtifactsCacheReport {
        path: portable_path(path),
        status: "updated".to_string(),
        reused_artifacts,
        added_artifacts,
        changed_artifacts,
        removed_artifacts,
    }
}

fn artifact_store_cache_report(
    path: &Path,
    previous: &CachedContextArtifactStore,
    current: &ContextArtifactStore,
) -> ContextArtifactStoreCacheReport {
    let previous = match previous {
        CachedContextArtifactStore::Valid(previous) => previous,
        CachedContextArtifactStore::Missing | CachedContextArtifactStore::Invalid => {
            return ContextArtifactStoreCacheReport {
                path: portable_path(path),
                status: match previous {
                    CachedContextArtifactStore::Missing => "created",
                    CachedContextArtifactStore::Invalid => "recovered",
                    CachedContextArtifactStore::Valid(_) => unreachable!(),
                }
                .to_string(),
                reused_artifacts: 0,
                added_artifacts: current.artifacts.artifacts.len(),
                changed_artifacts: 0,
                removed_artifacts: 0,
                reused_dependencies: 0,
                added_dependencies: current.dependency_graph.edges.len(),
                removed_dependencies: 0,
            };
        }
    };

    let previous_artifacts = previous
        .artifacts
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let current_artifacts = current
        .artifacts
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact.content_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut reused_artifacts = 0;
    let mut added_artifacts = 0;
    let mut changed_artifacts = 0;

    for (path, hash) in &current_artifacts {
        match previous_artifacts.get(*path) {
            Some(previous_hash) if *previous_hash == *hash => reused_artifacts += 1,
            Some(_) => changed_artifacts += 1,
            None => added_artifacts += 1,
        }
    }

    let removed_artifacts = previous_artifacts
        .keys()
        .filter(|path| !current_artifacts.contains_key(*path))
        .count();

    let previous_edges = previous
        .dependency_graph
        .edges
        .iter()
        .map(dependency_edge_key)
        .collect::<BTreeSet<_>>();
    let current_edges = current
        .dependency_graph
        .edges
        .iter()
        .map(dependency_edge_key)
        .collect::<BTreeSet<_>>();
    let reused_dependencies = current_edges
        .iter()
        .filter(|edge| previous_edges.contains(*edge))
        .count();
    let added_dependencies = current_edges
        .iter()
        .filter(|edge| !previous_edges.contains(*edge))
        .count();
    let removed_dependencies = previous_edges
        .iter()
        .filter(|edge| !current_edges.contains(*edge))
        .count();

    ContextArtifactStoreCacheReport {
        path: portable_path(path),
        status: "updated".to_string(),
        reused_artifacts,
        added_artifacts,
        changed_artifacts,
        removed_artifacts,
        reused_dependencies,
        added_dependencies,
        removed_dependencies,
    }
}

fn dependency_edge_key(edge: &ContextArtifactDependencyEdge) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        edge.source, edge.target, edge.relation, edge.evidence
    )
}

fn artifact_role_summary(artifacts: &[ContextArtifactItem]) -> Vec<ContextArtifactRoleSummary> {
    let mut counts = BTreeMap::<String, usize>::new();
    for artifact in artifacts {
        *counts
            .entry(artifact_role(&artifact.path).to_string())
            .or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(role, count)| ContextArtifactRoleSummary { role, count })
        .collect()
}

fn artifact_role(path: &str) -> &'static str {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let file_name = normalized
        .rsplit('/')
        .next()
        .unwrap_or(normalized.as_str())
        .trim_end_matches(".md")
        .trim_end_matches(".txt");

    if normalized.starts_with("tests/")
        || normalized.contains("/tests/")
        || file_name.ends_with("_test")
        || file_name.ends_with(".test")
        || file_name.ends_with("_spec")
        || file_name.ends_with(".spec")
    {
        return "test";
    }
    if normalized.starts_with("src/")
        || normalized.starts_with("crates/")
        || matches!(
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.to_ascii_lowercase())
                .as_deref(),
            Some(
                "rs" | "py"
                    | "ts"
                    | "tsx"
                    | "js"
                    | "jsx"
                    | "go"
                    | "java"
                    | "kt"
                    | "swift"
                    | "c"
                    | "cc"
                    | "cpp"
                    | "h"
                    | "hpp"
            )
        )
    {
        return "source";
    }
    if file_name.contains("prd")
        || file_name.contains("product-requirement")
        || file_name.contains("requirements")
    {
        return "prd";
    }
    if file_name.contains("design")
        || file_name.contains("architecture")
        || file_name.contains("adr")
    {
        return "design";
    }
    if file_name.contains("task")
        || file_name.contains("todo")
        || file_name.contains("roadmap")
        || file_name.contains("backlog")
    {
        return "tasks";
    }
    "artifact"
}

fn candidate_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let ignore_rules = IgnoreRules::load(root);
    let mut paths = Vec::new();
    collect_paths(root, root, &ignore_rules, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn index_file(root: &Path, path: &Path, max_bytes: usize) -> Result<Option<ContextIndexedFile>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    Ok(Some(ContextIndexedFile {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        bytes: content.len() as u64,
        line_count: content.lines().count(),
        content_hash: stable_hash(content.as_bytes()),
    }))
}

fn ingest_artifact_file(
    workspace_root: &Path,
    source_root: &Path,
    path: &Path,
    files_dir: &Path,
    max_bytes: usize,
) -> Result<Option<ContextIngestedArtifact>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes.clone()) else {
        return Ok(None);
    };
    let source_path = relative_path(source_root, path);
    let content_hash = stable_hash(content.as_bytes());
    let stored_abs = files_dir.join(&content_hash).join(Path::new(&source_path));
    if let Some(parent) = stored_abs.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create context artifact ingest dir {}",
                parent.display()
            )
        })?;
    }
    fs::write(&stored_abs, bytes).with_context(|| {
        format!(
            "failed to write context artifact ingest file {}",
            stored_abs.display()
        )
    })?;

    Ok(Some(ContextIngestedArtifact {
        id: format!("ingest:{source_path}:{content_hash}"),
        kind: artifact_role(&source_path).to_string(),
        source_path,
        stored_path: relative_or_display_path(workspace_root, &stored_abs),
        language: language_for_path(path).map(str::to_string),
        bytes: content.len() as u64,
        line_count: content.lines().count(),
        content_hash,
    }))
}

fn search_file(
    root: &Path,
    path: &Path,
    terms: &[String],
    max_bytes: usize,
) -> Result<Option<ContextSearchHit>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    let mut occurrences_by_term = BTreeMap::new();
    for token in tokenize(&content) {
        if terms.binary_search(&token).is_ok() {
            *occurrences_by_term.entry(token).or_insert(0_u64) += 1;
        }
    }
    let occurrences = occurrences_by_term.values().sum::<u64>();
    let mut matched_terms = occurrences_by_term.keys().cloned().collect::<BTreeSet<_>>();
    let path_score = add_path_matches(&rel, terms, &mut matched_terms);
    let matched_terms = matched_terms.into_iter().collect::<Vec<_>>();
    let (line_number, line) = first_matching_line_or_start(&content, terms, path_score > 0);
    let score = occurrences * 10 + matched_terms.len() as u64 * 5 + path_score * 3;

    Ok(Some(ContextSearchHit {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        score,
        occurrences,
        matched_terms,
        line_number,
        line,
    }))
}

fn pack_file(
    root: &Path,
    path: &Path,
    terms: &[String],
    max_bytes: usize,
    max_snippet_lines: usize,
) -> Result<Option<ContextPackSnippet>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    let mut occurrences_by_term = BTreeMap::new();
    for token in tokenize(&content) {
        if terms.binary_search(&token).is_ok() {
            *occurrences_by_term.entry(token).or_insert(0_u64) += 1;
        }
    }
    let occurrences = occurrences_by_term.values().sum::<u64>();
    let mut matched_terms = occurrences_by_term.keys().cloned().collect::<BTreeSet<_>>();
    let path_score = add_path_matches(&rel, terms, &mut matched_terms);
    let matched_terms = matched_terms.into_iter().collect::<Vec<_>>();
    let (line_number, _) = first_matching_line_or_start(&content, terms, path_score > 0);
    let score = occurrences * 10 + matched_terms.len() as u64 * 5 + path_score * 3;
    let (start_line, end_line, excerpt) = snippet_excerpt(&content, line_number, max_snippet_lines);

    Ok(Some(ContextPackSnippet {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        content_hash: stable_hash(content.as_bytes()),
        score,
        occurrences,
        matched_terms,
        start_line,
        end_line,
        excerpt,
    }))
}

fn vector_search_file(
    root: &Path,
    path: &Path,
    terms: &[String],
    query_embedding: &[f64],
    max_bytes: usize,
    dimensions: usize,
) -> Result<Option<ContextVectorSearchHit>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() > max_bytes || bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let rel = relative_path(root, path);
    let file_embedding =
        normalized_hash_embedding(vector_features(&content, Some(&rel)), dimensions);
    let score = cosine_similarity(query_embedding, &file_embedding);
    let content_terms = tokenize(&content).into_iter().collect::<BTreeSet<_>>();
    let path_terms = tokenize(&rel).into_iter().collect::<BTreeSet<_>>();
    let token_overlap = terms
        .iter()
        .filter(|term| content_terms.contains(*term) || path_terms.contains(*term))
        .count();
    let (line_number, line) = first_matching_line_or_start(&content, terms, token_overlap > 0);

    Ok(Some(ContextVectorSearchHit {
        path: rel,
        language: language_for_path(path).map(str::to_string),
        content_hash: stable_hash(content.as_bytes()),
        score,
        token_overlap,
        line_number,
        line,
    }))
}

fn query_terms(query: &str) -> Vec<String> {
    tokenize(query)
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn vector_features(content: &str, rel_path: Option<&str>) -> Vec<(String, f64)> {
    let tokens = tokenize(content);
    let mut features = Vec::new();
    for token in &tokens {
        features.push((format!("tok:{token}"), 1.0));
        for trigram in char_ngrams(token, 3) {
            features.push((format!("tri:{trigram}"), 0.2));
        }
    }
    for pair in tokens.windows(2) {
        features.push((format!("bi:{}:{}", pair[0], pair[1]), 0.8));
    }
    if let Some(rel_path) = rel_path {
        for token in tokenize(rel_path) {
            features.push((format!("path:{token}"), 1.5));
        }
    }
    features
}

fn char_ngrams(token: &str, n: usize) -> Vec<String> {
    let chars = token.chars().collect::<Vec<_>>();
    if n == 0 || chars.len() < n {
        return Vec::new();
    }
    chars
        .windows(n)
        .map(|window| window.iter().collect::<String>())
        .collect()
}

fn normalized_hash_embedding(features: Vec<(String, f64)>, dimensions: usize) -> Vec<f64> {
    let dimensions = dimensions.max(1);
    let mut vector = vec![0.0; dimensions];
    for (feature, weight) in features {
        let hash = stable_hash_u64(feature.as_bytes());
        let index = (hash as usize) % dimensions;
        vector[index] += weight;
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

fn cosine_similarity(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right.iter())
        .map(|(left, right)| left * right)
        .sum::<f64>()
        .max(0.0)
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn first_matching_line(content: &str, terms: &[String]) -> (usize, String) {
    for (index, line) in content.lines().enumerate() {
        let line_tokens = tokenize(line);
        if line_tokens
            .iter()
            .any(|token| terms.binary_search(token).is_ok())
        {
            return (index + 1, line.trim().chars().take(240).collect());
        }
    }
    (0, String::new())
}

fn first_matching_line_or_start(
    content: &str,
    terms: &[String],
    allow_path_only_fallback: bool,
) -> (usize, String) {
    let matched = first_matching_line(content, terms);
    if matched.0 != 0 || !allow_path_only_fallback {
        return matched;
    }
    first_non_empty_line(content)
}

fn first_non_empty_line(content: &str) -> (usize, String) {
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return (index + 1, trimmed.chars().take(240).collect());
        }
    }
    (0, String::new())
}

fn add_path_matches(rel: &str, terms: &[String], matched_terms: &mut BTreeSet<String>) -> u64 {
    let mut score = 0;
    for token in tokenize(rel) {
        if terms.binary_search(&token).is_ok() {
            score += 1;
            matched_terms.insert(token);
        }
    }
    score
}

fn snippet_excerpt(
    content: &str,
    line_number: usize,
    max_snippet_lines: usize,
) -> (usize, usize, String) {
    if line_number == 0 {
        return (0, 0, String::new());
    }
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return (0, 0, String::new());
    }
    let max_lines = max_snippet_lines.max(1);
    let match_index = line_number.saturating_sub(1).min(lines.len() - 1);
    let mut start = match_index.saturating_sub(max_lines / 2);
    let mut end = (start + max_lines).min(lines.len());
    if end.saturating_sub(start) < max_lines {
        start = end.saturating_sub(max_lines);
    }
    end = (start + max_lines).min(lines.len());
    let excerpt = lines[start..end]
        .iter()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    (start + 1, end, excerpt)
}

fn context_artifact_graph(snippets: &[ContextPackSnippet], query: &str) -> ContextArtifactGraph {
    let nodes = snippets
        .iter()
        .map(|snippet| {
            let id = context_artifact_node_id(snippet);
            ContextArtifactNode {
                id,
                kind: "snippet".to_string(),
                path: snippet.path.clone(),
                language: snippet.language.clone(),
                content_hash: snippet.content_hash.clone(),
                start_line: snippet.start_line,
                end_line: snippet.end_line,
            }
        })
        .collect::<Vec<_>>();
    let query_id = format!("query:{}", query.trim());
    let edges = nodes
        .iter()
        .zip(snippets.iter())
        .map(|(node, snippet)| ContextArtifactEdge {
            source: query_id.clone(),
            target: node.id.clone(),
            relation: "matched".to_string(),
            matched_terms: snippet.matched_terms.clone(),
        })
        .collect::<Vec<_>>();

    ContextArtifactGraph {
        schema: "kiana.context-artifact-graph.v1".to_string(),
        nodes,
        edges,
    }
}

fn context_artifact_node_id(snippet: &ContextPackSnippet) -> String {
    format!(
        "snippet:{}:{}-{}:{}",
        snippet.path, snippet.start_line, snippet.end_line, snippet.content_hash
    )
}

fn test_target_path(path: &str) -> Option<String> {
    let rest = path.strip_prefix("tests/")?;
    for suffix in ["_test.rs", "_tests.rs", ".test.rs", ".tests.rs"] {
        if let Some(stem) = rest.strip_suffix(suffix) {
            return Some(format!("src/{stem}.rs"));
        }
    }
    None
}

fn relative_path(root: &Path, path: &Path) -> String {
    portable_path(path.strip_prefix(root).unwrap_or(path))
}

fn relative_or_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(portable_path)
        .unwrap_or_else(|_| display_path(path))
}

fn resolve_ingest_store_dir(root: &Path, store_dir: &Path) -> Result<PathBuf> {
    if store_dir
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(anyhow!(
            "context artifact ingest store must stay inside the workspace root"
        ));
    }
    let resolved = if store_dir.is_absolute() {
        store_dir.to_path_buf()
    } else {
        root.join(store_dir)
    };
    if !resolved.starts_with(root) {
        return Err(anyhow!(
            "context artifact ingest store must stay inside the workspace root"
        ));
    }
    Ok(resolved)
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        value.into_owned()
    }
}

fn portable_path(path: &Path) -> String {
    display_path(path).replace('\\', "/")
}

fn stable_hash(bytes: &[u8]) -> String {
    format!("{:016x}", stable_hash_u64(bytes))
}

fn stable_hash_u64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-context-index-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn context_index_reports_stable_file_metadata() {
        let root = fixture_root("metadata");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn search() {}\n").unwrap();
        fs::write(root.join("notes.md"), "# Search Notes\n").unwrap();

        let index = build_context_index(&root, ContextIndexOptions::default()).unwrap();
        let paths = index
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(index.schema, "kiana.context-index.v1");
        assert_eq!(paths, vec!["notes.md", "src/lib.rs"]);
        assert_eq!(index.files[1].language.as_deref(), Some("rust"));
        assert_eq!(index.skipped_files, 0);
        assert_eq!(index.files[1].content_hash.len(), 16);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_context_index_recovers_from_corrupt_cache() {
        let root = fixture_root("corrupt-cache");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn recoverable() {}\n").unwrap();
        let cache_path = root.join(".kiana/context-index.json");
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        fs::write(&cache_path, "{not valid json").unwrap();

        let index =
            build_persistent_context_index(&root, ContextIndexOptions::default(), &cache_path)
                .unwrap();
        let cache = index.cache.as_ref().unwrap();

        assert_eq!(index.schema, "kiana.context-index.v1");
        assert_eq!(index.files_indexed, 1);
        assert_eq!(cache.status, "recovered");
        assert_eq!(cache.added_files, 1);
        assert_eq!(cache.reused_files, 0);
        assert_eq!(cache.changed_files, 0);
        assert_eq!(cache.removed_files, 0);
        assert!(
            serde_json::from_str::<ContextIndex>(&fs::read_to_string(&cache_path).unwrap()).is_ok()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_search_returns_ranked_hits_and_line_excerpt() {
        let root = fixture_root("search");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// checkout checkout flow\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "checkout guide\n").unwrap();

        let results = search_context_index(
            &root,
            "checkout",
            ContextSearchOptions {
                limit: Some(5),
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(results.schema, "kiana.context-search.v1");
        assert_eq!(results.terms, vec!["checkout"]);
        assert_eq!(results.hits[0].path, "src/lib.rs");
        assert_eq!(results.hits[0].occurrences, 3);
        assert_eq!(results.hits[0].line_number, 1);
        assert!(results.hits[0]
            .matched_terms
            .contains(&"checkout".to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_search_matches_path_terms_without_content_occurrences() {
        let root = fixture_root("search-path");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/path_only.rs"), "pub fn unrelated() {}\n").unwrap();

        let results = search_context_index(
            &root,
            "src/path_only.rs",
            ContextSearchOptions {
                limit: Some(5),
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.hits[0].path, "src/path_only.rs");
        assert_eq!(results.hits[0].occurrences, 0);
        assert_eq!(results.hits[0].line_number, 1);
        assert_eq!(results.hits[0].line, "pub fn unrelated() {}");
        assert!(results.hits[0]
            .matched_terms
            .contains(&"path_only".to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_vector_search_reports_deterministic_hash_embedding_hits() {
        let root = fixture_root("vector-search");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("src/checkout.rs"),
            "pub fn authorize_checkout() {}\n// payment workflow settles invoices\n",
        )
        .unwrap();
        fs::write(
            root.join("docs/refund.md"),
            "# Refunds\nrefund policy and return inventory notes\n",
        )
        .unwrap();

        let results = search_context_vectors(
            &root,
            "checkout flow",
            ContextVectorSearchOptions {
                limit: Some(1),
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(results.schema, "kiana.context-vector-search.v1");
        assert_eq!(results.embedding_model, DETERMINISTIC_VECTOR_MODEL);
        assert_eq!(results.dimensions, DEFAULT_VECTOR_DIMENSIONS);
        assert_eq!(results.limit, 1);
        assert_eq!(results.hits.len(), 1);
        assert_eq!(results.hits[0].path, "src/checkout.rs");
        assert_eq!(results.hits[0].language.as_deref(), Some("rust"));
        assert_eq!(results.hits[0].content_hash.len(), 16);
        assert!(results.hits[0].score > 0.0);
        assert!(results.hits[0].token_overlap >= 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_vector_search_rejects_empty_queries() {
        let root = fixture_root("vector-empty");
        fs::write(root.join("notes.md"), "notes\n").unwrap();

        let error = search_context_vectors(&root, "   ", ContextVectorSearchOptions::default())
            .unwrap_err();

        assert!(error.to_string().contains("query must contain"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_builds_ranked_snippets() {
        let root = fixture_root("pack");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// prepare checkout flow\n// checkout checkout done\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "checkout guide\n").unwrap();

        let pack = build_context_pack(
            &root,
            "checkout",
            ContextPackOptions {
                limit: Some(5),
                max_bytes_per_file: None,
                max_snippet_lines: Some(2),
            },
        )
        .unwrap();

        assert_eq!(pack.schema, "kiana.context-pack.v1");
        assert_eq!(pack.terms, vec!["checkout"]);
        assert_eq!(pack.max_snippet_lines, 2);
        assert_eq!(pack.snippets[0].path, "src/lib.rs");
        assert_eq!(pack.snippets[0].language.as_deref(), Some("rust"));
        assert_eq!(pack.snippets[0].occurrences, 4);
        assert_eq!(pack.snippets[0].start_line, 1);
        assert_eq!(pack.snippets[0].end_line, 2);
        assert!(pack.snippets[0].excerpt.contains("prepare checkout flow"));
        assert_eq!(pack.snippets[0].content_hash.len(), 16);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_reports_stable_artifact_graph_for_snippets() {
        let root = fixture_root("pack-artifact-graph");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn release() {}\n// release workflow\n",
        )
        .unwrap();

        let first = build_context_pack(
            &root,
            "release",
            ContextPackOptions {
                limit: Some(5),
                max_bytes_per_file: None,
                max_snippet_lines: Some(1),
            },
        )
        .unwrap();
        let second = build_context_pack(
            &root,
            "release",
            ContextPackOptions {
                limit: Some(5),
                max_bytes_per_file: None,
                max_snippet_lines: Some(1),
            },
        )
        .unwrap();

        assert_eq!(
            first.artifact_graph.schema,
            "kiana.context-artifact-graph.v1"
        );
        assert_eq!(
            serde_json::to_value(&first.artifact_graph).unwrap(),
            serde_json::to_value(&second.artifact_graph).unwrap()
        );
        assert_eq!(first.artifact_graph.nodes.len(), 1);
        assert_eq!(first.artifact_graph.nodes[0].kind, "snippet");
        assert_eq!(first.artifact_graph.nodes[0].path, "src/lib.rs");
        assert_eq!(first.artifact_graph.nodes[0].start_line, 1);
        assert_eq!(first.artifact_graph.nodes[0].end_line, 1);
        assert_eq!(first.artifact_graph.nodes[0].content_hash.len(), 16);
        assert_eq!(first.artifact_graph.edges.len(), 1);
        assert_eq!(first.artifact_graph.edges[0].source, "query:release");
        assert_eq!(
            first.artifact_graph.edges[0].target,
            first.artifact_graph.nodes[0].id
        );
        assert_eq!(first.artifact_graph.edges[0].relation, "matched");
        assert_eq!(first.artifact_graph.edges[0].matched_terms, vec!["release"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_is_deterministic_and_budgeted() {
        let root = fixture_root("pack-budget");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn lifecycle() {}\n// lifecycle lifecycle\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "lifecycle guide\n").unwrap();

        let options = ContextPackOptions {
            limit: Some(1),
            max_bytes_per_file: None,
            max_snippet_lines: Some(1),
        };
        let first = build_context_pack(&root, "lifecycle", options).unwrap();
        let second = build_context_pack(&root, "lifecycle", options).unwrap();

        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        assert_eq!(first.snippets.len(), 1);
        assert_eq!(first.snippets[0].path, "src/lib.rs");
        assert_eq!(first.snippets[0].start_line, first.snippets[0].end_line);
        assert_eq!(first.limit, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_pack_includes_path_only_match_with_file_start_excerpt() {
        let root = fixture_root("pack-path");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(
            root.join("docs/operations.md"),
            "Release owner checklist\nNo matching filename tokens here.\n",
        )
        .unwrap();

        let pack = build_context_pack(
            &root,
            "docs/operations.md",
            ContextPackOptions {
                limit: Some(5),
                max_bytes_per_file: None,
                max_snippet_lines: Some(1),
            },
        )
        .unwrap();

        assert_eq!(pack.snippets.len(), 1);
        assert_eq!(pack.snippets[0].path, "docs/operations.md");
        assert_eq!(pack.snippets[0].occurrences, 0);
        assert_eq!(pack.snippets[0].start_line, 1);
        assert_eq!(pack.snippets[0].end_line, 1);
        assert_eq!(pack.snippets[0].excerpt, "Release owner checklist");
        assert!(pack.snippets[0]
            .matched_terms
            .contains(&"operations".to_string()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_artifacts_reports_stable_local_artifact_inventory() {
        let root = fixture_root("artifacts-inventory");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();

        let report = build_context_artifacts(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(report.schema, "kiana.context-artifacts.v1");
        assert_eq!(report.files_indexed, 1);
        assert_eq!(report.skipped_files, 0);
        assert_eq!(report.artifacts.len(), 1);
        assert_eq!(report.artifacts[0].kind, "file");
        assert_eq!(report.artifacts[0].path, "src/lib.rs");
        assert_eq!(report.artifacts[0].language.as_deref(), Some("rust"));
        assert_eq!(report.artifacts[0].content_hash.len(), 16);
        assert!(report.artifacts[0].id.starts_with("file:src/lib.rs:"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_context_artifacts_reports_incremental_cache_status() {
        let root = fixture_root("artifacts-cache");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        let cache_path = root.join(".kiana").join("context-artifacts.json");

        let first = build_persistent_context_artifacts(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
            &cache_path,
        )
        .unwrap();
        let first_cache = first.cache.as_ref().unwrap();
        assert_eq!(first_cache.status, "created");
        assert_eq!(first_cache.added_artifacts, 1);
        assert_eq!(first_cache.reused_artifacts, 0);
        assert!(Path::new(&first_cache.path).ends_with(".kiana/context-artifacts.json"));

        let second = build_persistent_context_artifacts(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
            &cache_path,
        )
        .unwrap();
        let second_cache = second.cache.as_ref().unwrap();
        assert_eq!(second_cache.status, "updated");
        assert_eq!(second_cache.reused_artifacts, 1);
        assert_eq!(second_cache.added_artifacts, 0);
        assert_eq!(second_cache.changed_artifacts, 0);
        assert_eq!(second_cache.removed_artifacts, 0);

        fs::write(root.join("src/lib.rs"), "pub fn release_v2() {}\n").unwrap();
        let third = build_persistent_context_artifacts(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
            &cache_path,
        )
        .unwrap();
        let third_cache = third.cache.as_ref().unwrap();
        assert_eq!(third_cache.status, "updated");
        assert_eq!(third_cache.reused_artifacts, 0);
        assert_eq!(third_cache.added_artifacts, 0);
        assert_eq!(third_cache.changed_artifacts, 1);
        assert_eq!(third_cache.removed_artifacts, 0);

        let saved: ContextArtifacts =
            serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
        assert_eq!(saved.schema, "kiana.context-artifacts.v1");
        assert_eq!(saved.cache.as_ref().unwrap().changed_artifacts, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_artifact_dependency_graph_reports_test_relationships() {
        let root = fixture_root("artifact-dependency-graph");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(
            root.join("tests/lib_test.rs"),
            "use kiana::release;\n#[test]\nfn release_works() {}\n",
        )
        .unwrap();

        let graph = build_context_artifact_dependency_graph(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(graph.schema, "kiana.context-artifact-dependency-graph.v1");
        assert_eq!(graph.nodes.len(), 2);
        let source = graph
            .nodes
            .iter()
            .find(|node| node.path == "tests/lib_test.rs")
            .unwrap();
        let target = graph
            .nodes
            .iter()
            .find(|node| node.path == "src/lib.rs")
            .unwrap();
        assert!(graph.edges.iter().any(|edge| {
            edge.source == source.id
                && edge.target == target.id
                && edge.relation == "test_of"
                && edge.evidence == "tests/lib_test.rs matches src/lib.rs"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_artifact_dependency_graph_reports_path_references() {
        let root = fixture_root("artifact-dependency-graph-path-reference");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();

        let graph = build_context_artifact_dependency_graph(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        let source = graph
            .nodes
            .iter()
            .find(|node| node.path == "docs/design.md")
            .unwrap();
        let target = graph
            .nodes
            .iter()
            .find(|node| node.path == "src/lib.rs")
            .unwrap();
        assert!(graph.edges.iter().any(|edge| {
            edge.source == source.id
                && edge.target == target.id
                && edge.relation == "path_reference"
                && edge.evidence == "docs/design.md references src/lib.rs"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_artifact_store_reports_manifest_and_dependency_graph() {
        let root = fixture_root("artifact-store");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();

        let store = build_context_artifact_store(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(store.schema, "kiana.context-artifact-store.v1");
        assert_eq!(store.artifacts_schema, "kiana.context-artifacts.v1");
        assert_eq!(
            store.dependency_graph_schema,
            "kiana.context-artifact-dependency-graph.v1"
        );
        assert_eq!(store.artifact_count, 3);
        assert_eq!(store.dependency_count, 2);
        assert_eq!(store.artifacts.artifacts.len(), 3);
        assert!(store.dependency_graph.edges.iter().any(|edge| {
            edge.relation == "test_of" && edge.evidence == "tests/lib_test.rs matches src/lib.rs"
        }));
        assert!(store.dependency_graph.edges.iter().any(|edge| {
            edge.relation == "path_reference"
                && edge.evidence == "docs/design.md references src/lib.rs"
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_artifact_store_reports_artifact_role_summary() {
        let root = fixture_root("artifact-store-roles");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("docs/prd.md"), "# PRD\n").unwrap();
        fs::write(root.join("docs/design.md"), "# Design\n").unwrap();
        fs::write(root.join("docs/tasks.md"), "- Ship\n").unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();

        let store = build_context_artifact_store(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(store.artifact_roles.len(), 5);
        assert_eq!(store.artifact_roles[0].role, "design");
        assert_eq!(store.artifact_roles[0].count, 1);
        assert_eq!(store.artifact_roles[1].role, "prd");
        assert_eq!(store.artifact_roles[1].count, 1);
        assert_eq!(store.artifact_roles[2].role, "source");
        assert_eq!(store.artifact_roles[2].count, 1);
        assert_eq!(store.artifact_roles[3].role, "tasks");
        assert_eq!(store.artifact_roles[3].count, 1);
        assert_eq!(store.artifact_roles[4].role, "test");
        assert_eq!(store.artifact_roles[4].count, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_artifact_ingest_copies_text_artifacts_and_writes_manifest() {
        let root = fixture_root("artifact-ingest-root");
        let source = fixture_root("artifact-ingest-source");
        fs::create_dir_all(source.join("docs")).unwrap();
        fs::write(source.join("docs/prd.md"), "# PRD\nShip the release\n").unwrap();
        fs::write(
            source.join("docs/design.md"),
            "# Design\nUse local artifacts\n",
        )
        .unwrap();
        fs::write(source.join("docs/large.md"), "x".repeat(80)).unwrap();
        fs::write(source.join("raw.bin"), b"abc\0def").unwrap();

        let report = ingest_context_artifacts(
            &root,
            &source,
            ContextArtifactIngestOptions {
                store_dir: None,
                max_bytes_per_file: Some(64),
            },
        )
        .unwrap();

        assert_eq!(report.schema, "kiana.context-artifact-ingest.v1");
        assert_eq!(report.artifacts_schema, "kiana.context-artifacts.v1");
        assert_eq!(report.ingested_files, 2);
        assert_eq!(report.skipped_files, 1);
        assert_eq!(report.store_dir, ".kiana/context-ingest");
        assert_eq!(report.manifest_path, ".kiana/context-ingest/manifest.json");
        assert_eq!(report.sync.path, ".kiana/context-ingest/manifest.json");
        assert_eq!(report.sync.status, "created");
        assert_eq!(report.sync.reused_files, 0);
        assert_eq!(report.sync.added_files, 2);
        assert_eq!(report.sync.changed_files, 0);
        assert_eq!(report.sync.removed_files, 0);
        assert!(report
            .artifacts
            .iter()
            .any(|artifact| artifact.source_path == "docs/prd.md"
                && artifact.kind == "prd"
                && artifact
                    .stored_path
                    .starts_with(".kiana/context-ingest/files/")
                && artifact.content_hash.len() == 16));
        for artifact in &report.artifacts {
            assert!(root.join(&artifact.stored_path).is_file());
        }
        let saved: ContextArtifactIngest =
            serde_json::from_str(&fs::read_to_string(root.join(&report.manifest_path)).unwrap())
                .unwrap();
        assert_eq!(saved.schema, "kiana.context-artifact-ingest.v1");
        assert_eq!(saved.ingested_files, 2);
        assert_eq!(saved.sync.status, "created");

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn context_artifact_ingest_reports_sync_deltas_and_removes_stale_files() {
        let root = fixture_root("artifact-ingest-sync-root");
        let source = fixture_root("artifact-ingest-sync-source");
        fs::create_dir_all(source.join("docs")).unwrap();
        fs::write(source.join("docs/prd.md"), "# PRD\nfirst\n").unwrap();
        fs::write(source.join("docs/design.md"), "# Design\nfirst\n").unwrap();

        let first = ingest_context_artifacts(
            &root,
            &source,
            ContextArtifactIngestOptions {
                store_dir: None,
                max_bytes_per_file: None,
            },
        )
        .unwrap();
        let stale_design_path = first
            .artifacts
            .iter()
            .find(|artifact| artifact.source_path == "docs/design.md")
            .unwrap()
            .stored_path
            .clone();
        assert!(root.join(&stale_design_path).is_file());

        fs::write(source.join("docs/prd.md"), "# PRD\nchanged\n").unwrap();
        fs::remove_file(source.join("docs/design.md")).unwrap();
        fs::write(source.join("docs/tasks.md"), "- Ship\n").unwrap();

        let second = ingest_context_artifacts(
            &root,
            &source,
            ContextArtifactIngestOptions {
                store_dir: None,
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(second.sync.status, "updated");
        assert_eq!(second.sync.reused_files, 0);
        assert_eq!(second.sync.added_files, 1);
        assert_eq!(second.sync.changed_files, 1);
        assert_eq!(second.sync.removed_files, 1);
        assert!(!root.join(stale_design_path).exists());
        assert!(second
            .artifacts
            .iter()
            .any(|artifact| artifact.source_path == "docs/tasks.md"
                && artifact.kind == "tasks"
                && root.join(&artifact.stored_path).is_file()));

        fs::write(root.join(&second.manifest_path), "{not-json").unwrap();
        let recovered = ingest_context_artifacts(
            &root,
            &source,
            ContextArtifactIngestOptions {
                store_dir: None,
                max_bytes_per_file: None,
            },
        )
        .unwrap();
        assert_eq!(recovered.sync.status, "recovered");
        assert_eq!(recovered.sync.added_files, recovered.ingested_files);

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
    }

    #[test]
    fn context_artifact_ingest_rejects_store_outside_workspace() {
        let root = fixture_root("artifact-ingest-root-boundary");
        let source = fixture_root("artifact-ingest-source-boundary");
        let outside = fixture_root("artifact-ingest-outside");
        fs::write(source.join("notes.md"), "notes\n").unwrap();

        let error = ingest_context_artifacts(
            &root,
            &source,
            ContextArtifactIngestOptions {
                store_dir: Some(outside.join("store")),
                max_bytes_per_file: None,
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("inside the workspace root"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn context_artifact_readiness_reports_missing_required_roles() {
        let root = fixture_root("artifact-readiness-missing");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();

        let readiness = build_context_artifact_readiness(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
        )
        .unwrap();

        assert_eq!(readiness.schema, "kiana.context-artifact-readiness.v1");
        assert_eq!(readiness.status, "incomplete");
        assert_eq!(
            readiness.artifact_store_schema,
            "kiana.context-artifact-store.v1"
        );
        assert_eq!(readiness.artifact_count, 3);
        assert_eq!(readiness.dependency_count, 2);
        assert_eq!(readiness.missing_roles, vec!["prd", "tasks"]);
        assert!(readiness.required_roles.iter().any(|role| {
            role.role == "design" && role.required && role.present && role.count == 1
        }));
        assert!(readiness.required_roles.iter().any(|role| {
            role.role == "source" && role.required && role.present && role.count == 1
        }));
        assert!(readiness.required_roles.iter().any(|role| {
            role.role == "test" && role.required && role.present && role.count == 1
        }));
        assert!(readiness.required_roles.iter().any(|role| {
            role.role == "prd" && role.required && !role.present && role.count == 0
        }));
        assert!(readiness.required_roles.iter().any(|role| {
            role.role == "tasks" && role.required && !role.present && role.count == 0
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn persistent_context_artifact_store_reports_cache_recovery_and_deltas() {
        let root = fixture_root("artifact-store-cache");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();
        let cache_path = root.join(".kiana").join("context-artifact-store.json");
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        fs::write(&cache_path, "{not valid json").unwrap();

        let recovered = build_persistent_context_artifact_store(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
            &cache_path,
        )
        .unwrap();
        let recovered_cache = recovered.cache.as_ref().unwrap();
        assert_eq!(recovered.schema, "kiana.context-artifact-store.v1");
        assert_eq!(recovered_cache.status, "recovered");
        assert_eq!(recovered_cache.added_artifacts, 3);
        assert_eq!(recovered_cache.reused_artifacts, 0);
        assert_eq!(recovered_cache.added_dependencies, 2);
        assert_eq!(recovered_cache.reused_dependencies, 0);
        assert!(Path::new(&recovered_cache.path).ends_with(".kiana/context-artifact-store.json"));

        let second = build_persistent_context_artifact_store(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
            &cache_path,
        )
        .unwrap();
        let second_cache = second.cache.as_ref().unwrap();
        assert_eq!(second_cache.status, "updated");
        assert_eq!(second_cache.reused_artifacts, 3);
        assert_eq!(second_cache.added_artifacts, 0);
        assert_eq!(second_cache.changed_artifacts, 0);
        assert_eq!(second_cache.removed_artifacts, 0);
        assert_eq!(second_cache.reused_dependencies, 2);
        assert_eq!(second_cache.added_dependencies, 0);
        assert_eq!(second_cache.removed_dependencies, 0);

        fs::write(root.join("src/lib.rs"), "pub fn release_v2() {}\n").unwrap();
        let third = build_persistent_context_artifact_store(
            &root,
            ContextArtifactOptions {
                max_bytes_per_file: None,
            },
            &cache_path,
        )
        .unwrap();
        let third_cache = third.cache.as_ref().unwrap();
        assert_eq!(third_cache.status, "updated");
        assert_eq!(third_cache.reused_artifacts, 2);
        assert_eq!(third_cache.changed_artifacts, 1);
        assert_eq!(third_cache.reused_dependencies, 0);
        assert_eq!(third_cache.added_dependencies, 2);
        assert_eq!(third_cache.removed_dependencies, 2);

        let saved: ContextArtifactStore =
            serde_json::from_str(&fs::read_to_string(&cache_path).unwrap()).unwrap();
        assert_eq!(saved.schema, "kiana.context-artifact-store.v1");
        assert_eq!(saved.cache.as_ref().unwrap().changed_artifacts, 1);
        assert_eq!(saved.cache.as_ref().unwrap().added_dependencies, 2);

        let _ = fs::remove_dir_all(root);
    }
}
