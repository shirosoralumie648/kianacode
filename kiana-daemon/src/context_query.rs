//! 受控上下文查询与工作区 materialization 的 Daemon 适配器。
//!
//! # 文件职责与系统位置
//!
//! 本文件把 `kiana-core::ControlPlane` 已经授权的 Query capability 转换成实际的工作区
//! 读取、索引、检索和 artifact 写入。它属于 `kiana-daemon` 组合根的本地 adapter：
//! `DaemonHost` 在装配 Broker 时调用 [`register`] 注册固定的 operation，Core 的
//! `authorize_and_execute()` 通过 Broker 找到这里的 handler 后，才会进入下面的执行函数。
//! 因此这里负责“怎样访问本地工作区”，不负责决定“请求者有没有权限”；授权、审批、permit
//! 和生命周期事实由上游 ControlPlane/Broker 负责。
//!
//! ```text
//! 入口层 context query intent
//!     -> kiana-core::context_query::normalize_context_query_arguments
//!     -> ControlPlane::authorize_and_execute
//!     -> CapabilityBroker 精确匹配 (CapabilityKind::Query, operation)
//!     -> 【当前文件】handler 校验已封装参数、限制路径、调用 kiana-query
//!     -> CapabilityResult::success(command_result)
//!     -> Core finalizer / receipt / HTTP 或 CLI 响应
//! ```
//!
//! 这里的 operation 分成两类。只读类（repo map、依赖图、readiness、全文/向量搜索、pack）
//! 生成工作区上下文；materialization 类（index/artifact/artifact-store 的 cache 和 ingest）
//! 可能创建或更新项目目录内的文件。两类都使用同步的 `kiana-query` helper，但在 Tokio
//! handler 中通过 `spawn_blocking` 执行，以免文件扫描、解析或落盘占用异步 executor 的
//! worker。任何错误都会返回到 Broker/Core；本文件不把错误自行变成重试、授权或最终取消
//! 事实，外层生命周期代码再根据错误和已提交证据决定响应。
//!
//! # 数据与安全边界
//!
//! 请求参数先在 Core 中从用户形状规范化为 broker operation，再由本文件按 operation 的
//! allowlist 解析。`project_root` 必须 canonicalize 成真实目录，`root`、cache、source、store
//! 等相对路径不能包含绝对路径、`..` 或跨出项目根；写入路径还要检查现有 symlink/ancestor
//! 不会把文件导向项目外。输出通过 [`render_output`] 追加本次读取发现的源文件摘要、读取时
//! freshness 和运行时版本，帮助调用方知道“结果来自哪些当前文件”；这个摘要不是授权签名，
//! 也不代表完整的原子 workspace snapshot。
//!
//! ⚠ 注意：`AuthorizedCapabilityRequest` 已经在上游封装了授权结果，但 handler 仍必须检查
//! operation、参数键和路径。不要把“调用者已经过授权”理解成“任意路径和任意字段都可以
//! 使用”；否则同一 capability 的不同操作可能被错误复用，或把工作区读取/写入带出项目边界。

use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, RequestId};
use kiana_ports::PortError;
use kiana_query::{
    build_context_artifact_dependency_graph, build_context_artifact_readiness,
    build_context_artifact_store, build_context_artifacts, build_context_index, build_context_pack,
    build_persistent_context_artifact_store, build_persistent_context_artifacts,
    build_persistent_context_index, build_repo_map, ingest_context_artifacts, search_context_index,
    search_context_vectors, ContextArtifactDependencyGraph, ContextArtifactIngest,
    ContextArtifactIngestOptions, ContextArtifactOptions, ContextArtifactReadiness,
    ContextArtifactStore, ContextArtifacts, ContextIndex, ContextIndexOptions, ContextPack,
    ContextPackOptions, ContextSearchOptions, ContextSearchResults, ContextVectorSearchOptions,
    ContextVectorSearchResults, RepoMap, RepoMapOptions,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const REPO_MAP_OPERATION: &str = "context.repo_map";
const ARTIFACT_GRAPH_OPERATION: &str = "context.artifact_graph.read";
const ARTIFACT_READINESS_OPERATION: &str = "context.artifact_readiness.read";
const SEARCH_OPERATION: &str = "context.search";
const VECTOR_SEARCH_OPERATION: &str = "context.vector_search";
const PACK_OPERATION: &str = "context.pack";
const INDEX_OPERATION: &str = "context.index.read";
const INDEX_CACHE_OPERATION: &str = "context.index.cache.write";
const ARTIFACTS_OPERATION: &str = "context.artifacts.read";
const ARTIFACTS_CACHE_OPERATION: &str = "context.artifacts.cache.write";
const ARTIFACT_STORE_OPERATION: &str = "context.artifact_store.read";
const ARTIFACT_STORE_CACHE_OPERATION: &str = "context.artifact_store.cache.write";
const ARTIFACT_INGEST_OPERATION: &str = "context.artifact_ingest.write";

/// 把本文件提供的 Query operation 一次性注册到 Daemon 的静态 Broker 目录。
///
/// 【调用者】`DaemonHost` 的组合阶段调用；注册完成后，Core/Broker 才能按稳定的
/// `(CapabilityKind::Query, operation)` 键找到对应 handler。
///
/// 【输入与输出】输入是可变的进程内 `CapabilityBroker`；成功时返回 `Ok(())`，失败时说明
/// operation 重复、目录绑定非法或 Broker 初始化失败。这里只建立 handler 映射，不读取工作区、
/// 不执行查询、不写缓存，也不发放授权。
///
/// 【为什么分成两个循环】只读 operation 和可能落盘的 materialization operation 共享 handler
/// 结构但有不同参数集合和风险含义。保留两组枚举能让后续校验按类别收窄，而不是把所有
/// context 动作压成一个“万能查询”入口。
pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Query,
        REPO_MAP_OPERATION,
        Arc::new(RepoMapHandler),
    )?;
    for operation in [
        ReadQueryOperation::ArtifactGraph,
        ReadQueryOperation::ArtifactReadiness,
        ReadQueryOperation::Search,
        ReadQueryOperation::VectorSearch,
        ReadQueryOperation::Pack,
    ] {
        broker.register_static(
            CapabilityKind::Query,
            operation.broker_key(),
            Arc::new(ReadQueryHandler { operation }),
        )?;
    }
    for operation in [
        MaterializationOperation::Index,
        MaterializationOperation::IndexCache,
        MaterializationOperation::Artifacts,
        MaterializationOperation::ArtifactsCache,
        MaterializationOperation::ArtifactStore,
        MaterializationOperation::ArtifactStoreCache,
        MaterializationOperation::Ingest,
    ] {
        broker.register_static(
            CapabilityKind::Query,
            operation.broker_key(),
            Arc::new(MaterializationHandler { operation }),
        )?;
    }
    Ok(())
}

struct RepoMapHandler;

#[async_trait]
impl CapabilityHandler for RepoMapHandler {
    /// 执行已经通过上游授权的 repo map 请求。
    ///
    /// 【核心流程】先确认请求的 operation 仍是 `context.repo_map`，再解析工作区根目录、输出
    /// 格式和可选 token 预算；随后把同步的 repo map 扫描放入 `spawn_blocking`，最后把结构化
    /// map 交给统一的 [`command_result`] 生成 JSON/text 结果。
    ///
    /// 【失败传播】参数、canonical root、治理撤销检查、扫描或 blocking task join 任一失败都会
    /// 返回 `PortError`，调用者会把它作为 capability 失败处理。这里不重试，也不把失败账号
    /// 或项目状态写成禁用；请求的 `RequestId` 只用于把结果和上游 receipt 关联起来。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, REPO_MAP_OPERATION)?;
        let request_id = request.request.request_id;
        let arguments = validated_repo_map_arguments(&request.request.arguments)?;
        let output = arguments.output.to_owned();
        let project_root = arguments.project_root.to_owned();
        let max_tokens = arguments.max_tokens;
        let map = tokio::task::spawn_blocking(move || {
            let project_root = canonical_project_root(&project_root)?;
            build_repo_map(project_root, RepoMapOptions { max_tokens })
                .map_err(|error| PortError::Failed(format!("context_repo_map_failed:{error}")))
        })
        .await
        .map_err(|error| PortError::Failed(format!("context_repo_map_join_failed:{error}")))??;
        command_result(request_id, &map, &output, "repo_map", format_repo_map_text)
    }
}

#[derive(Clone, Copy)]
enum ReadQueryOperation {
    ArtifactGraph,
    ArtifactReadiness,
    Search,
    VectorSearch,
    Pack,
}

impl ReadQueryOperation {
    /// 返回 Broker 目录使用的稳定 operation 名称。
    ///
    /// 这个映射同时被 Core 的规范化逻辑和 Daemon 的注册逻辑依赖；修改字符串会造成“上游
    /// 已授权，但 Broker 找不到 handler”的拒绝，因此不能只改其中一处或用 label 代替。
    fn broker_key(self) -> &'static str {
        match self {
            Self::ArtifactGraph => ARTIFACT_GRAPH_OPERATION,
            Self::ArtifactReadiness => ARTIFACT_READINESS_OPERATION,
            Self::Search => SEARCH_OPERATION,
            Self::VectorSearch => VECTOR_SEARCH_OPERATION,
            Self::Pack => PACK_OPERATION,
        }
    }

    /// 返回结果字段和错误前缀使用的短标签。
    ///
    /// label 只是日志/输出分类，不承担授权身份；稳定 operation key 才是 Broker 路由契约。
    fn label(self) -> &'static str {
        match self {
            Self::ArtifactGraph => "artifact_graph",
            Self::ArtifactReadiness => "artifact_readiness",
            Self::Search => "search",
            Self::VectorSearch => "vector_search",
            Self::Pack => "pack",
        }
    }
}

#[derive(Clone, Copy)]
enum MaterializationOperation {
    Index,
    IndexCache,
    Artifacts,
    ArtifactsCache,
    ArtifactStore,
    ArtifactStoreCache,
    Ingest,
}

impl MaterializationOperation {
    /// 将 materialization 变体转换成静态 Broker operation key。
    ///
    /// read、cache write 和 ingest 使用不同 key，是为了让上游 risk/approval 和本文件的
    /// 参数 allowlist 对齐；不能把 cache/ingest 隐藏在普通 read operation 的可选字段里。
    fn broker_key(self) -> &'static str {
        match self {
            Self::Index => INDEX_OPERATION,
            Self::IndexCache => INDEX_CACHE_OPERATION,
            Self::Artifacts => ARTIFACTS_OPERATION,
            Self::ArtifactsCache => ARTIFACTS_CACHE_OPERATION,
            Self::ArtifactStore => ARTIFACT_STORE_OPERATION,
            Self::ArtifactStoreCache => ARTIFACT_STORE_CACHE_OPERATION,
            Self::Ingest => ARTIFACT_INGEST_OPERATION,
        }
    }

    /// 返回落在错误码、结果标题和 provenance 标签中的短分类名。
    fn label(self) -> &'static str {
        match self {
            Self::Index | Self::IndexCache => "index",
            Self::Artifacts | Self::ArtifactsCache => "artifacts",
            Self::ArtifactStore | Self::ArtifactStoreCache => "artifact_store",
            Self::Ingest => "artifact_ingest",
        }
    }
}

struct MaterializationHandler {
    operation: MaterializationOperation,
}

#[async_trait]
impl CapabilityHandler for MaterializationHandler {
    /// 执行索引、artifact store/cache 或 artifact ingest 这类可能落盘的操作。
    ///
    /// 【调用者】由 Broker 在 permit、operation 和扩展检查通过后调用。
    /// 【数据流】`AuthorizedCapabilityRequest.arguments` -> 参数 allowlist -> 项目内路径约束
    /// -> [`execute_materialization`] -> kiana-query 的 build/persistent/ingest helper ->
    /// `CapabilityResult` 的 `command_result.value`。
    /// 【副作用】只有选择 cache 或 ingest operation 时才会更新项目内文件；路径约束失败时
    /// 必须在调用 helper 前返回，避免把坏路径交给底层文件代码。
    /// 【异常】blocking task panic/join error 和 helper 错误都转换成带 operation label 的
    /// `PortError`；本函数不把“写入成功”解释成 EventStore 事实，真正的 receipt/状态提交由
    /// Core 上层完成。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, self.operation.broker_key())?;
        let request_id = request.request.request_id;
        let arguments =
            validated_materialization_arguments(self.operation, &request.request.arguments)?;
        let operation = self.operation;
        let value =
            tokio::task::spawn_blocking(move || execute_materialization(operation, arguments))
                .await
                .map_err(|error| {
                    PortError::Failed(format!("context_{}_join_failed:{error}", operation.label()))
                })??;
        Ok(CapabilityResult::success(
            request_id,
            json!({
                "command_result": {
                    "output_type": "text",
                    "value": value,
                    "metadata": null,
                }
            }),
        ))
    }
}

struct MaterializationArguments {
    project_root: String,
    root: Option<String>,
    output: String,
    max_bytes_per_file: Option<usize>,
    cache: Option<String>,
    source: Option<String>,
    store: Option<String>,
}

fn validated_materialization_arguments(
    operation: MaterializationOperation,
    arguments: &Value,
) -> Result<MaterializationArguments, PortError> {
    // Core 规范化后可能省略 null 的可选字段，但每个 operation 仍有自己的业务键集合。
    // 这里先区分 expected（允许出现）和 required（必须出现），让“可选字段缺失”与“必填
    // 字段缺失/未知字段注入”走同一条 fail-closed 错误路径。
    let arguments = arguments_object(arguments)?;
    let expected = match operation {
        MaterializationOperation::Index
        | MaterializationOperation::Artifacts
        | MaterializationOperation::ArtifactStore => {
            &["project_root", "root", "output", "max_bytes_per_file"][..]
        }
        MaterializationOperation::IndexCache
        | MaterializationOperation::ArtifactsCache
        | MaterializationOperation::ArtifactStoreCache => &[
            "project_root",
            "root",
            "output",
            "cache",
            "max_bytes_per_file",
        ][..],
        MaterializationOperation::Ingest => &[
            "project_root",
            "root",
            "output",
            "source",
            "store",
            "max_bytes_per_file",
        ][..],
    };
    let required = match operation {
        MaterializationOperation::Index
        | MaterializationOperation::Artifacts
        | MaterializationOperation::ArtifactStore => &["project_root", "output"][..],
        MaterializationOperation::IndexCache
        | MaterializationOperation::ArtifactsCache
        | MaterializationOperation::ArtifactStoreCache => &["project_root", "output", "cache"][..],
        MaterializationOperation::Ingest => &["project_root", "output", "source"][..],
    };
    ensure_exact_keys(arguments, expected, required)?;
    let cache = match operation {
        MaterializationOperation::IndexCache
        | MaterializationOperation::ArtifactsCache
        | MaterializationOperation::ArtifactStoreCache => {
            Some(required_string(arguments, "cache")?.to_owned())
        }
        _ => None,
    };
    let source = if matches!(operation, MaterializationOperation::Ingest) {
        Some(required_string(arguments, "source")?.to_owned())
    } else {
        None
    };
    Ok(MaterializationArguments {
        project_root: required_string(arguments, "project_root")?.to_owned(),
        root: optional_string(arguments, "root")?,
        output: required_output(arguments)?.to_owned(),
        max_bytes_per_file: optional_usize(arguments, "max_bytes_per_file")?,
        cache,
        source,
        store: optional_string(arguments, "store")?,
    })
}

/// 这是所有 materialization 变体的同步执行核心。
///
/// 【为什么集中在这里】handler 只负责异步边界和结果包装；把路径约束、query helper
/// 调用和 output formatter 集中在一个 match 中，可以保证 read、cache、store、ingest
/// 的参数和错误前缀不会在多个 async handler 中漂移。
///
/// 【路径数据流】先由 [`confined_context_root`] 得到项目内 root；cache 走
/// [`confined_write_path`]，ingest 走 [`confined_existing_dir`] + [`confined_write_dir`]，
/// 再把受限路径交给 `kiana-query`。ingest 额外拒绝 source 位于 store 内，防止同步扫描
/// 自己刚写出的文件而不断扩大输入。
///
/// 【输出】各 helper 的结构化报告先进入 [`render_output`]，再根据 `output=json|text`
/// 变成字符串。任何 helper 错误都附上稳定的 context operation 前缀后返回，不会静默
/// 当成空索引或空 artifact 集合。
fn execute_materialization(
    operation: MaterializationOperation,
    arguments: MaterializationArguments,
) -> Result<String, PortError> {
    let root = confined_context_root(&arguments.project_root, arguments.root.as_deref())?;
    let options = ContextArtifactOptions {
        max_bytes_per_file: arguments.max_bytes_per_file,
    };
    match operation {
        MaterializationOperation::Index => {
            let index = build_context_index(
                &root,
                ContextIndexOptions {
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_index_failed:{error}")))?;
            render_output(
                &index,
                &arguments.output,
                "index",
                format_context_index_text,
            )
        }
        MaterializationOperation::IndexCache => {
            let cache = confined_write_path(
                &root,
                arguments.cache.as_deref().unwrap_or_default(),
                "context index cache",
            )?;
            let index = build_persistent_context_index(
                &root,
                ContextIndexOptions {
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
                cache,
            )
            .map_err(|error| PortError::Failed(format!("context_index_cache_failed:{error}")))?;
            render_output(
                &index,
                &arguments.output,
                "index",
                format_context_index_text,
            )
        }
        MaterializationOperation::Artifacts => {
            let artifacts = build_context_artifacts(&root, options)
                .map_err(|error| PortError::Failed(format!("context_artifacts_failed:{error}")))?;
            render_output(
                &artifacts,
                &arguments.output,
                "artifacts",
                format_context_artifacts_text,
            )
        }
        MaterializationOperation::ArtifactsCache => {
            let cache = confined_write_path(
                &root,
                arguments.cache.as_deref().unwrap_or_default(),
                "context artifacts cache",
            )?;
            let artifacts =
                build_persistent_context_artifacts(&root, options, cache).map_err(|error| {
                    PortError::Failed(format!("context_artifacts_cache_failed:{error}"))
                })?;
            render_output(
                &artifacts,
                &arguments.output,
                "artifacts",
                format_context_artifacts_text,
            )
        }
        MaterializationOperation::ArtifactStore => {
            let store = build_context_artifact_store(&root, options).map_err(|error| {
                PortError::Failed(format!("context_artifact_store_failed:{error}"))
            })?;
            render_output(
                &store,
                &arguments.output,
                "artifact_store",
                format_context_artifact_store_text,
            )
        }
        MaterializationOperation::ArtifactStoreCache => {
            let cache = confined_write_path(
                &root,
                arguments.cache.as_deref().unwrap_or_default(),
                "context artifact store cache",
            )?;
            let store = build_persistent_context_artifact_store(&root, options, cache).map_err(
                |error| PortError::Failed(format!("context_artifact_store_cache_failed:{error}")),
            )?;
            render_output(
                &store,
                &arguments.output,
                "artifact_store",
                format_context_artifact_store_text,
            )
        }
        MaterializationOperation::Ingest => {
            let source = confined_existing_dir(
                &root,
                arguments.source.as_deref().unwrap_or_default(),
                "context artifact ingest source",
            )?;
            let store = confined_write_dir(
                &root,
                arguments
                    .store
                    .as_deref()
                    .unwrap_or(".kiana/context-ingest"),
                "context artifact ingest store",
            )?;
            if source.starts_with(&store) {
                return Err(PortError::Failed(
                    "context_artifact_ingest_source_inside_store".to_owned(),
                ));
            }
            let ingest = ingest_context_artifacts(
                &root,
                source,
                ContextArtifactIngestOptions {
                    store_dir: Some(store),
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| {
                PortError::Failed(format!("context_artifact_ingest_failed:{error}"))
            })?;
            render_output(
                &ingest,
                &arguments.output,
                "artifact_ingest",
                format_context_artifact_ingest_text,
            )
        }
    }
}

struct ReadQueryHandler {
    operation: ReadQueryOperation,
}

#[async_trait]
impl CapabilityHandler for ReadQueryHandler {
    /// 执行不写入 materialization 的上下文读取操作。
    ///
    /// handler 与 materialization handler 共用同一条异步边界：先核对稳定 operation key，
    /// 再把用户输入解析为该变体允许的字段，最后用 `spawn_blocking` 调用同步查询 helper。
    /// 这样 `search`、`vector_search` 和 `pack` 不能通过多余字段改变路径或偷偷升级为写入，
    /// 而图/就绪度查询也不会因为缺少 query 文本而把空字符串当成真实请求。
    ///
    /// 成功结果统一放在 `command_result.value`；解析、路径、查询或 task join 错误都向上返回，
    /// 由 Core 的 capability 生命周期决定客户端看到的拒绝/失败状态。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, self.operation.broker_key())?;
        let request_id = request.request.request_id;
        let arguments = validated_read_arguments(self.operation, &request.request.arguments)?;
        let operation = self.operation;
        let value = tokio::task::spawn_blocking(move || execute_read_query(operation, arguments))
            .await
            .map_err(|error| {
                PortError::Failed(format!("context_{}_join_failed:{error}", operation.label()))
            })??;
        Ok(CapabilityResult::success(
            request_id,
            json!({
                "command_result": {
                    "output_type": "text",
                    "value": value,
                    "metadata": null,
                }
            }),
        ))
    }
}

/// 在单个同步调用中执行 read-only query，并把结构化结果交给统一输出边界。
///
/// 【调用者】[`ReadQueryHandler::execute`] 的 blocking task。
/// 【输入】已通过 `validated_read_arguments` 的 operation 和参数；这里仍重新调用
/// [`confined_context_root`]，因为真正访问文件的边界必须在同一执行线程上再次得到 canonical
/// root，不能把未经约束的用户字符串直接传给 `kiana-query`。
/// 【输出】返回 JSON 或文本字符串。artifact graph/readiness、全文搜索、向量搜索和 pack
/// 分别调用不同 helper，但都经过 [`render_output`]，因此 provenance 形状和序列化错误处理
/// 保持一致。
/// 【失败】任何底层查询错误都会保留对应的 `context_*_failed` 前缀并返回 `PortError`；空
/// 结果和查询失败不是同一件事，不能把错误吞掉后构造空数组。
fn execute_read_query(
    operation: ReadQueryOperation,
    arguments: ReadQueryArguments,
) -> Result<String, PortError> {
    let root = confined_context_root(&arguments.project_root, arguments.root.as_deref())?;
    match operation {
        ReadQueryOperation::ArtifactGraph => {
            let graph = build_context_artifact_dependency_graph(
                root,
                ContextArtifactOptions {
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_artifact_graph_failed:{error}")))?;
            render_output(
                &graph,
                &arguments.output,
                "artifact_graph",
                format_context_artifact_graph_text,
            )
        }
        ReadQueryOperation::ArtifactReadiness => {
            let readiness = build_context_artifact_readiness(
                root,
                ContextArtifactOptions {
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| {
                PortError::Failed(format!("context_artifact_readiness_failed:{error}"))
            })?;
            render_output(
                &readiness,
                &arguments.output,
                "artifact_readiness",
                format_context_artifact_readiness_text,
            )
        }
        ReadQueryOperation::Search => {
            let results = search_context_index(
                root,
                required_query(&arguments)?,
                ContextSearchOptions {
                    limit: arguments.limit,
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_search_failed:{error}")))?;
            render_output(
                &results,
                &arguments.output,
                "search",
                format_context_search_text,
            )
        }
        ReadQueryOperation::VectorSearch => {
            let results = search_context_vectors(
                root,
                required_query(&arguments)?,
                ContextVectorSearchOptions {
                    limit: arguments.limit,
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_vector_search_failed:{error}")))?;
            render_output(
                &results,
                &arguments.output,
                "vector_search",
                format_context_vector_search_text,
            )
        }
        ReadQueryOperation::Pack => {
            let pack = build_context_pack(
                root,
                required_query(&arguments)?,
                ContextPackOptions {
                    limit: arguments.limit,
                    max_bytes_per_file: arguments.max_bytes_per_file,
                    max_snippet_lines: arguments.max_snippet_lines,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_pack_failed:{error}")))?;
            render_output(&pack, &arguments.output, "pack", format_context_pack_text)
        }
    }
}

fn format_context_index_text(index: &ContextIndex) -> String {
    let mut lines = vec![
        "Context index".to_owned(),
        format!("root: {}", index.root),
        format!(
            "files_indexed: {} skipped_files: {} total_bytes: {}",
            index.files_indexed, index.skipped_files, index.total_bytes
        ),
    ];
    for file in &index.files {
        lines.push(format!(
            "- {} [{}] bytes={} lines={} hash={}",
            file.path,
            file.language.as_deref().unwrap_or("unknown"),
            file.bytes,
            file.line_count,
            file.content_hash
        ));
    }
    if let Some(cache) = &index.cache {
        lines.push(format!(
            "cache: {} status={} reused={} added={} changed={} removed={}",
            cache.path,
            cache.status,
            cache.reused_files,
            cache.added_files,
            cache.changed_files,
            cache.removed_files
        ));
    }
    lines.join("\n")
}

fn format_context_artifacts_text(report: &ContextArtifacts) -> String {
    let mut lines = vec![
        "Context artifacts".to_owned(),
        format!("root: {}", report.root),
        format!(
            "files_indexed: {} skipped_files: {} artifacts={}",
            report.files_indexed,
            report.skipped_files,
            report.artifacts.len()
        ),
    ];
    for artifact in &report.artifacts {
        lines.push(format!(
            "- {} [{}] kind={} bytes={} lines={} hash={} id={}",
            artifact.path,
            artifact.language.as_deref().unwrap_or("unknown"),
            artifact.kind,
            artifact.bytes,
            artifact.line_count,
            artifact.content_hash,
            artifact.id
        ));
    }
    if let Some(cache) = &report.cache {
        lines.push(format!(
            "cache: {} status={} reused={} added={} changed={} removed={}",
            cache.path,
            cache.status,
            cache.reused_artifacts,
            cache.added_artifacts,
            cache.changed_artifacts,
            cache.removed_artifacts
        ));
    }
    lines.join("\n")
}

fn format_context_artifact_ingest_text(report: &ContextArtifactIngest) -> String {
    let mut lines = vec![
        "Context artifact ingest".to_owned(),
        format!("root: {}", report.root),
        format!("source_root: {}", report.source_root),
        format!("store_dir: {}", report.store_dir),
        format!("manifest_path: {}", report.manifest_path),
        format!(
            "schema: {} artifacts_schema: {} ingested_files: {} skipped_files: {} total_bytes: {}",
            report.schema,
            report.artifacts_schema,
            report.ingested_files,
            report.skipped_files,
            report.total_bytes
        ),
        format!(
            "sync: {} status={} reused={} added={} changed={} removed={}",
            report.sync.path,
            report.sync.status,
            report.sync.reused_files,
            report.sync.added_files,
            report.sync.changed_files,
            report.sync.removed_files
        ),
    ];
    for artifact in &report.artifacts {
        lines.push(format!(
            "- {} -> {} [{}] kind={} bytes={} lines={} hash={} id={}",
            artifact.source_path,
            artifact.stored_path,
            artifact.language.as_deref().unwrap_or("unknown"),
            artifact.kind,
            artifact.bytes,
            artifact.line_count,
            artifact.content_hash,
            artifact.id
        ));
    }
    lines.join("\n")
}

fn format_context_artifact_store_text(store: &ContextArtifactStore) -> String {
    let mut lines = vec![
        "Context artifact store".to_owned(),
        format!("root: {}", store.root),
        format!(
            "schema: {} artifacts={} dependencies={}",
            store.schema, store.artifact_count, store.dependency_count
        ),
        format!("artifacts_schema: {}", store.artifacts_schema),
        format!("dependency_graph_schema: {}", store.dependency_graph_schema),
        format!(
            "artifact_roles: {}",
            store
                .artifact_roles
                .iter()
                .map(|role| format!("{}={}", role.role, role.count))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ];
    if let Some(cache) = &store.cache {
        lines.push(format!(
            "cache: {} status={} reused_artifacts={} added_artifacts={} changed_artifacts={} removed_artifacts={} reused_dependencies={} added_dependencies={} removed_dependencies={}",
            cache.path,
            cache.status,
            cache.reused_artifacts,
            cache.added_artifacts,
            cache.changed_artifacts,
            cache.removed_artifacts,
            cache.reused_dependencies,
            cache.added_dependencies,
            cache.removed_dependencies
        ));
    }
    lines.join("\n")
}

fn required_query(arguments: &ReadQueryArguments) -> Result<&str, PortError> {
    // `query` 只在 search/vector_search/pack 中有业务意义。把 None 拒绝在这里，可以区分
    // “调用者没有提供查询条件”与“查询成功但没有命中”，后者必须仍然返回一个合法的空结果。
    arguments
        .query
        .as_deref()
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:query".to_owned()))
}

struct RepoMapArguments<'a> {
    // repo map 只接受项目根和输出格式；max_tokens 是可选的模型上下文预算，不是授权额度。
    project_root: &'a str,
    output: &'a str,
    max_tokens: Option<u64>,
}

fn validated_repo_map_arguments(arguments: &Value) -> Result<RepoMapArguments<'_>, PortError> {
    // repo map 的参数形状比其他 read operation 更小：project_root/output 必填，max_tokens
    // 可省略。这个解析器不读取文件，也不负责检查 root 是否存在；真正的 filesystem 边界在
    // `RepoMapHandler` 的 blocking task 内通过 `canonical_project_root` 完成。
    let arguments = arguments_object(arguments)?;
    ensure_exact_keys(
        arguments,
        &["project_root", "output", "max_tokens"],
        &["project_root", "output"],
    )?;
    Ok(RepoMapArguments {
        project_root: required_string(arguments, "project_root")?,
        output: required_output(arguments)?,
        max_tokens: optional_u64(arguments, "max_tokens")?,
    })
}

struct ReadQueryArguments {
    // 这些字段对应 Core 规范化后允许进入 read handler 的业务参数。
    // `query` 只对 search/vector_search/pack 必填；limit、文件大小和 snippet 行数是结果上限，
    // 不代表服务端会无限读取或改变项目权限。
    project_root: String,
    root: Option<String>,
    output: String,
    query: Option<String>,
    limit: Option<usize>,
    max_bytes_per_file: Option<usize>,
    max_snippet_lines: Option<usize>,
}

fn validated_read_arguments(
    operation: ReadQueryOperation,
    arguments: &Value,
) -> Result<ReadQueryArguments, PortError> {
    // 先按 operation 选择字段 allowlist，再解析 required/optional 字段。不要为了复用解析器
    // 把所有字段都设成可选：例如 artifact_graph 不需要 query，而 search 缺少 query 就不应
    // 被解释成“搜索全部文件”。
    let arguments = arguments_object(arguments)?;
    let expected = match operation {
        ReadQueryOperation::ArtifactGraph | ReadQueryOperation::ArtifactReadiness => {
            &["project_root", "root", "output", "max_bytes_per_file"][..]
        }
        ReadQueryOperation::Search | ReadQueryOperation::VectorSearch => &[
            "project_root",
            "root",
            "output",
            "query",
            "limit",
            "max_bytes_per_file",
        ][..],
        ReadQueryOperation::Pack => &[
            "project_root",
            "root",
            "output",
            "query",
            "limit",
            "max_bytes_per_file",
            "max_snippet_lines",
        ][..],
    };
    let required = match operation {
        ReadQueryOperation::ArtifactGraph | ReadQueryOperation::ArtifactReadiness => {
            &["project_root", "output"][..]
        }
        ReadQueryOperation::Search
        | ReadQueryOperation::VectorSearch
        | ReadQueryOperation::Pack => &["project_root", "output", "query"][..],
    };
    ensure_exact_keys(arguments, expected, required)?;
    let query = match operation {
        ReadQueryOperation::Search
        | ReadQueryOperation::VectorSearch
        | ReadQueryOperation::Pack => Some(required_string(arguments, "query")?.to_owned()),
        _ => None,
    };
    Ok(ReadQueryArguments {
        project_root: required_string(arguments, "project_root")?.to_owned(),
        root: optional_string(arguments, "root")?,
        output: required_output(arguments)?.to_owned(),
        query,
        limit: optional_usize(arguments, "limit")?,
        max_bytes_per_file: optional_usize(arguments, "max_bytes_per_file")?,
        max_snippet_lines: optional_usize(arguments, "max_snippet_lines")?,
    })
}

fn arguments_object(arguments: &Value) -> Result<&Map<String, Value>, PortError> {
    // 只接受 JSON object 作为 handler 参数容器。
    //
    // 上游协议可以携带任意 JSON 值，但本文件的 operation 契约要求键值对象；在这里尽早
    // 拒绝数组、字符串或 null，能让后续必填字段错误保持统一的 `context_arguments_invalid`
    // 分类，而不是触发底层索引代码的意外错误。
    arguments
        .as_object()
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:not_object".to_owned()))
}

fn required_string<'a>(arguments: &'a Map<String, Value>, key: &str) -> Result<&'a str, PortError> {
    // 业务路径、operation 参数和输出名都必须是非空字符串。trim 只影响本次解析返回的借用
    // 视图，不会修改原始 JSON；调用者随后复制成 owned String，以便送入 blocking task。
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed(format!("context_arguments_invalid:{key}")))
}

fn optional_string(arguments: &Map<String, Value>, key: &str) -> Result<Option<String>, PortError> {
    // 缺失或显式 null 表示“使用该 operation 的默认值”；一旦字段存在且不是 null，就必须
    // 经过同样的非空字符串校验，避免把数字/空白文本误当成相对路径。
    match arguments.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(_) => required_string(arguments, key).map(|value| Some(value.to_owned())),
    }
}

fn required_output(arguments: &Map<String, Value>) -> Result<&str, PortError> {
    // 约束输出协议只允许 `json` 或 `text`。
    //
    // 两种模式共享同一份结构化结果：JSON 会增加 provenance 字段，文本会追加摘要行。
    // 这里拒绝任意第三种格式，避免 formatter 的默认分支把调用方误导成已经支持某种未定义
    // 的序列化协议。
    required_string(arguments, "output").and_then(|output| {
        if matches!(output, "json" | "text") {
            Ok(output)
        } else {
            Err(PortError::Failed(
                "context_arguments_invalid:output".to_owned(),
            ))
        }
    })
}

fn optional_u64(arguments: &Map<String, Value>, key: &str) -> Result<Option<u64>, PortError> {
    // 所有数值上限最终由 Core 的 normalize 或具体 query helper 解释；本层至少拒绝 0、负数
    // （JSON 无符号转换会失败）和非整数。None 代表沿用底层默认预算，不代表无限制授权。
    match arguments.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(|| PortError::Failed(format!("context_arguments_invalid:{key}"))),
    }
}

fn optional_usize(arguments: &Map<String, Value>, key: &str) -> Result<Option<usize>, PortError> {
    // usize 转换是为了匹配本地文件 helper 的索引类型；在 32 位平台上过大的 u64 必须失败，
    // 不能截断后让读取范围和调用方看到的预算不一致。
    optional_u64(arguments, key)?
        .map(|value| {
            usize::try_from(value)
                .map_err(|_| PortError::Failed(format!("context_arguments_invalid:{key}")))
        })
        .transpose()
}

fn ensure_exact_keys(
    arguments: &Map<String, Value>,
    expected: &[&str],
    required: &[&str],
) -> Result<(), PortError> {
    // 这个函数是“业务参数边界”，不是授权器。授权结果已经封装在上游 request 中；这里的
    // 任务是保证 handler 只消费 Core 规范化过的 operation 字段，并允许少量由服务端追加的
    // 身份/Hook 快照 metadata。未知字段一律拒绝，防止客户端伪造路径、权限或执行上下文。
    // Core stamps this envelope on every prepared action. Broker verifies the sealed action and
    // execution permit before dispatch; these fields are not handler authorization inputs.
    const SERVER_IDENTITY_KEYS: &[&str] = &[
        "role_id",
        "department_id",
        "session_id",
        "actor_id",
        "project_trusted",
        "path_allow",
        // QueryPreToolHooks adds this after Core normalization. It is part of the
        // server-sealed action digest, so the handler must accept it as metadata
        // while continuing to reject arbitrary caller-supplied fields.
        "hook_snapshot",
    ];
    // Core's canonical action removes null optional fields. Their absence preserves the existing
    // parser defaults; required keys and the per-operation allowlist still fail closed.
    if required.iter().any(|key| !arguments.contains_key(*key))
        || arguments.keys().any(|key| {
            !expected.contains(&key.as_str()) && !SERVER_IDENTITY_KEYS.contains(&key.as_str())
        })
    {
        return Err(PortError::Failed(
            "context_arguments_invalid:keys".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_operation(
    request: &AuthorizedCapabilityRequest,
    expected: &str,
) -> Result<(), PortError> {
    // Broker 按 operation 找到 handler 后仍做一次精确核对，防止未来复用 handler 时把一个
    // 变体的参数解释成另一个变体。失败是本地契约错误，不会触发重新授权或自动重试。
    if request.request.operation != expected {
        return Err(PortError::Failed("context_operation_mismatch".to_owned()));
    }
    Ok(())
}

fn canonical_project_root(project_root: &str) -> Result<PathBuf, PortError> {
    // 把请求提供的项目根转换成真实、存在的目录，并先执行数据治理撤销检查。
    //
    // 【输入来源】Core context 从会话/项目上下文带入的 `project_root`，最终仍来自本地入口
    // 参数；不能仅信任字符串看起来像工作区。
    // 【失败影响】canonicalize、目录检查或 `data_governance::read_policy` 失败会阻止后续
    // 任何扫描/写入。只要治理策略列出的 revoked source 仍存在，就返回拒绝，避免历史或已
    // 撤销数据重新进入 index/artifact 结果。该函数不删除数据、不写 EventLog。
    let project_root = PathBuf::from(project_root)
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("context_project_root_invalid:{error}")))?;
    if !project_root.is_dir() {
        return Err(PortError::Failed(
            "context_project_root_invalid:not_directory".to_owned(),
        ));
    }
    let policy = crate::data_governance::read_policy(&project_root)?;
    if policy
        .revoked_sources
        .iter()
        .any(|source| project_root.join(source).exists())
    {
        return Err(PortError::Failed(
            "context_processing_revoked:retained_source".to_owned(),
        ));
    }
    Ok(project_root)
}

fn confined_context_root(
    project_root: &str,
    requested_root: Option<&str>,
) -> Result<PathBuf, PortError> {
    // 在 canonical project root 内解析可选的相对查询子目录。
    //
    // 先复用 [`canonical_project_root`]，再拒绝绝对路径、父目录、根组件和 Windows prefix，
    // 最后对 join 后的真实路径做 `starts_with(project_root)`。两次检查都需要保留：字符串
    // 形式检查阻止明显的 traversal，canonical 后的 containment 检查阻止 symlink 把路径带
    // 到项目外。失败会在真正读取前返回。
    let project_root = canonical_project_root(project_root)?;
    let Some(requested_root) = requested_root else {
        return Ok(project_root);
    };
    let requested_root = Path::new(requested_root);
    if requested_root.is_absolute()
        || requested_root.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PortError::Failed(
            "context_root_invalid:not_relative".to_owned(),
        ));
    }
    let root = project_root
        .join(requested_root)
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("context_root_invalid:{error}")))?;
    if !root.starts_with(&project_root) {
        return Err(PortError::Failed("context_root_outside_project".to_owned()));
    }
    if !root.is_dir() {
        return Err(PortError::Failed(
            "context_root_invalid:not_directory".to_owned(),
        ));
    }
    Ok(root)
}

fn confined_existing_dir(root: &Path, relative: &str, label: &str) -> Result<PathBuf, PortError> {
    // 解析一个必须已经存在的、位于 root 内的相对目录（ingest source 使用）。
    //
    // `canonicalize` 会解析 symlink，随后 containment 检查确保真实目标仍在项目根；不存在、
    // 文件类型错误或越界都会返回带 label 的 `PortError`，不会交给底层 helper 猜测路径。
    let relative = validated_relative_path(relative, label)?;
    let candidate = root.join(relative);
    let resolved = candidate
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("{label}_invalid:{error}")))?;
    if !resolved.starts_with(root) {
        return Err(PortError::Failed(format!("{label}_outside_project")));
    }
    if !resolved.is_dir() {
        return Err(PortError::Failed(format!("{label}_invalid:not_directory")));
    }
    Ok(resolved)
}

fn confined_write_path(root: &Path, relative: &str, label: &str) -> Result<PathBuf, PortError> {
    // 解析允许创建的项目内写路径。
    //
    // 写目标可能尚不存在，所以不能直接对完整路径 `canonicalize`；
    // [`ensure_write_containment`] 会沿现有 ancestor 检查 symlink/文件，并在缺失部分回退
    // 到最近存在的目录。这样既允许原子创建新 cache，又不允许通过已存在的链接跳出 root。
    let relative = validated_relative_path(relative, label)?;
    let candidate = root.join(relative);
    ensure_write_containment(root, &candidate, label)?;
    Ok(candidate)
}

fn confined_write_dir(root: &Path, relative: &str, label: &str) -> Result<PathBuf, PortError> {
    // 解析 ingest store 目录，并在目标已存在时要求它确实是目录。
    //
    // 不在这里创建目录是刻意的：目录创建和 manifest/cache 的原子写入属于 query helper 的
    // 事务边界；本层只负责路径安全和已有节点类型，避免在校验阶段产生半成品副作用。
    let candidate = confined_write_path(root, relative, label)?;
    if let Ok(metadata) = fs::symlink_metadata(&candidate) {
        if !metadata.file_type().is_dir() {
            return Err(PortError::Failed(format!("{label}_invalid:not_directory")));
        }
    }
    Ok(candidate)
}

fn validated_relative_path<'a>(value: &'a str, label: &str) -> Result<&'a str, PortError> {
    // 这里只检查相对路径语法，不解析 filesystem。调用者随后还必须做 canonical containment，
    // 因为合法的相对字符串仍可能经过 symlink 指向 root 外。
    let value = value.trim();
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PortError::Failed(format!("{label}_invalid:not_relative")));
    }
    Ok(value)
}

fn ensure_write_containment(root: &Path, candidate: &Path, label: &str) -> Result<(), PortError> {
    // 在目标可能尚不存在的情况下证明写入路径不会穿出项目根。
    //
    // 已存在的 symlink/file 先整体 canonicalize；不存在的尾部逐级向上查找第一个存在的
    // ancestor，再 canonicalize ancestor 并做 containment。这个顺序不能简化成只检查文本
    // 前缀，否则 `cache-link` 等合法名称和 symlink 都可能把写入导向外部目录。
    if let Ok(metadata) = fs::symlink_metadata(candidate) {
        if metadata.file_type().is_symlink() || metadata.is_file() {
            let resolved = candidate
                .canonicalize()
                .map_err(|error| PortError::Failed(format!("{label}_invalid:{error}")))?;
            if !resolved.starts_with(root) {
                return Err(PortError::Failed(format!("{label}_outside_project")));
            }
            return Ok(());
        }
    }

    let mut ancestor = candidate;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let resolved = ancestor
                    .canonicalize()
                    .map_err(|error| PortError::Failed(format!("{label}_invalid:{error}")))?;
                if !resolved.starts_with(root) {
                    return Err(PortError::Failed(format!("{label}_outside_project")));
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or_else(|| {
                    PortError::Failed(format!("{label}_invalid:no_existing_parent"))
                })?;
            }
            Err(error) => {
                return Err(PortError::Failed(format!("{label}_invalid:{error}")));
            }
        }
    }
}

fn command_result<T: Serialize>(
    request_id: RequestId,
    value: &T,
    output: &str,
    label: &str,
    text_formatter: impl FnOnce(&T) -> String,
) -> Result<CapabilityResult, PortError> {
    // 把任意 query 结构化值包装成统一的 capability 返回 envelope。
    //
    // 【输入】上游 request id、结构化查询结果、输出模式、label 和文本 formatter。
    // 【输出】`CapabilityResult::success` 的 `command_result`，其中 `value` 始终是字符串：
    // JSON 模式是带 provenance 的 JSON 文本，text 模式是人类可读摘要。
    // 【为什么集中】Core/CLI 不需要为 repo map、搜索、artifact store 各自猜返回形状；失败
    // 的序列化错误会在这里返回 `PortError`，不会构造一个看起来成功但无法解析的结果。
    let value = render_output(value, output, label, text_formatter)?;
    Ok(CapabilityResult::success(
        request_id,
        json!({
            "command_result": {
                "output_type": "text",
                "value": value,
                "metadata": null,
            }
        }),
    ))
}

fn render_output<T: Serialize>(
    value: &T,
    output: &str,
    label: &str,
    text_formatter: impl FnOnce(&T) -> String,
) -> Result<String, PortError> {
    // 在输出边界统一完成 serde 编码和读取来源 provenance 的组装。
    //
    // `sources` 递归收集结果中形如 `path + content_hash` 的叶子，并按稳定的
    // `BTreeMap` 顺序计算 `source_snapshot`。JSON 会把 provenance 作为字段加入对象；text
    // 只能追加摘要行。`freshness=captured_at_read` 说明摘要来自本次读取，
    // `atomic_workspace_snapshot=false` 明确告诉调用者这不是一个冻结的全局快照。
    //
    // ⚠ 摘要是结果来源的可核对指纹，不是文件锁、签名或权限证明。文件在读取后变化、结果
    // 中没有 path/hash 字段、或调用者绕过这里构造响应，都不能由这个字段自动修复；真正的
    // durability/live/physical 证明仍由上层证据链提供。
    let mut serialized = serde_json::to_value(value)
        .map_err(|error| PortError::Failed(format!("context_{label}_serialize:{error}")))?;
    let mut sources = std::collections::BTreeMap::<String, String>::new();
    collect_provenance_sources(&serialized, &mut sources);
    let source_snapshot = kiana_domain::json_digest(&json!({"query_kind":label,"sources":sources}));
    let provenance = json!({"schema":"kiana.context-provenance.v1","snapshot":source_snapshot,
        "source":"local_workspace","sources":sources,"freshness":"captured_at_read",
        "atomic_workspace_snapshot":false,"runtime_version":env!("CARGO_PKG_VERSION")});
    if output == "json" {
        if let Some(object) = serialized.as_object_mut() {
            object.insert("provenance".to_owned(), provenance);
        }
        serde_json::to_string_pretty(&serialized)
            .map_err(|error| PortError::Failed(format!("context_{label}_serialize:{error}")))
    } else {
        Ok(format!(
            "{}\nsource: local_workspace; snapshot: {}; files: {}; freshness: captured_at_read",
            text_formatter(value),
            source_snapshot,
            sources.len()
        ))
    }
}

fn collect_provenance_sources(
    value: &Value,
    sources: &mut std::collections::BTreeMap<String, String>,
) {
    // 结果类型并不统一，所以这里递归遍历 JSON，而不是依赖某个具体 Context DTO。只有同时
    // 找到字符串 path 和 content_hash 才记录来源；其他字段继续递归，数组也不能跳过。
    // 同一路径最后一次写入同一个 BTreeMap 键，输出顺序稳定但不额外声称内容发生过版本合并。
    match value {
        Value::Object(object) => {
            if let (Some(path), Some(hash)) = (
                object.get("path").and_then(Value::as_str),
                object.get("content_hash").and_then(Value::as_str),
            ) {
                sources.insert(path.to_owned(), hash.to_owned());
            }
            for child in object.values() {
                collect_provenance_sources(child, sources);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_provenance_sources(child, sources);
            }
        }
        _ => {}
    }
}

// 以下 formatter 只负责把已经生成的结构化结果展示成 text。它们不重新读取工作区、不做
// 授权判断，也不改变结果集合；JSON 输出统一由 render_output 处理，避免两种格式各自实现
// 一套 provenance 或错误边界。
fn format_repo_map_text(map: &RepoMap) -> String {
    let mut lines = vec![
        "Repo map".to_owned(),
        format!("root: {}", map.root),
        format!(
            "files: {} estimated_tokens: {}/{} truncated: {} omitted_files: {}",
            map.files.len(),
            map.estimated_tokens,
            map.token_budget,
            map.truncated,
            map.omitted_files
        ),
    ];
    for file in &map.files {
        lines.push(format!(
            "- {} [{}] bytes={} tokens={}",
            file.path,
            file.language.as_deref().unwrap_or("unknown"),
            file.bytes,
            file.estimated_tokens
        ));
        if !file.symbols.is_empty() {
            lines.push(format!("  symbols: {}", file.symbols.join(", ")));
        }
    }
    lines.join("\n")
}

fn format_context_artifact_graph_text(graph: &ContextArtifactDependencyGraph) -> String {
    let mut lines = vec![
        "Context artifact graph".to_string(),
        format!("root: {}", graph.root),
        format!(
            "schema: {} nodes={} edges={}",
            graph.schema,
            graph.nodes.len(),
            graph.edges.len()
        ),
    ];
    for node in &graph.nodes {
        lines.push(format!(
            "- {} [{}] kind={} hash={} id={}",
            node.path,
            node.language.as_deref().unwrap_or("unknown"),
            node.kind,
            node.content_hash,
            node.id
        ));
    }
    for edge in &graph.edges {
        lines.push(format!(
            "  edge: {} -> {} relation={} evidence={}",
            edge.source, edge.target, edge.relation, edge.evidence
        ));
    }
    lines.join("\n")
}

fn format_context_artifact_readiness_text(readiness: &ContextArtifactReadiness) -> String {
    let mut lines = vec![
        "Context artifact readiness".to_string(),
        format!("root: {}", readiness.root),
        format!(
            "schema: {} status={} artifacts={} dependencies={}",
            readiness.schema,
            readiness.status,
            readiness.artifact_count,
            readiness.dependency_count
        ),
        format!("artifact_store_schema: {}", readiness.artifact_store_schema),
        format!(
            "missing_roles: {}",
            if readiness.missing_roles.is_empty() {
                "none".to_string()
            } else {
                readiness.missing_roles.join(",")
            }
        ),
    ];
    for role in &readiness.required_roles {
        lines.push(format!(
            "- {} required={} present={} count={}",
            role.role, role.required, role.present, role.count
        ));
    }
    lines.join("\n")
}

fn format_context_search_text(results: &ContextSearchResults) -> String {
    let mut lines = vec![
        "Context search".to_string(),
        format!("root: {}", results.root),
        format!(
            "query: {} terms={} files_indexed={} skipped_files={}",
            results.query,
            results.terms.join(","),
            results.files_indexed,
            results.skipped_files
        ),
    ];
    for hit in &results.hits {
        lines.push(format!(
            "- {}:{} score={} occurrences={} terms={}",
            hit.path,
            hit.line_number,
            hit.score,
            hit.occurrences,
            hit.matched_terms.join(",")
        ));
        if !hit.line.is_empty() {
            lines.push(format!("  {}", hit.line));
        }
    }
    lines.join("\n")
}

fn format_context_vector_search_text(results: &ContextVectorSearchResults) -> String {
    let mut lines = vec![
        "Context vector search".to_string(),
        format!("root: {}", results.root),
        format!(
            "query: {} terms={} model={} dimensions={} files_indexed={} skipped_files={}",
            results.query,
            results.terms.join(","),
            results.embedding_model,
            results.dimensions,
            results.files_indexed,
            results.skipped_files
        ),
    ];
    for hit in &results.hits {
        lines.push(format!(
            "- {}:{} score={:.6} token_overlap={} hash={}",
            hit.path, hit.line_number, hit.score, hit.token_overlap, hit.content_hash
        ));
        if !hit.line.is_empty() {
            lines.push(format!("  {}", hit.line));
        }
    }
    lines.join("\n")
}

fn format_context_pack_text(pack: &ContextPack) -> String {
    let mut lines = vec![
        "Context pack".to_string(),
        format!("root: {}", pack.root),
        format!(
            "query: {} terms={} files_indexed={} skipped_files={} snippets={}",
            pack.query,
            pack.terms.join(","),
            pack.files_indexed,
            pack.skipped_files,
            pack.snippets.len()
        ),
        format!(
            "artifact_graph: schema={} nodes={} edges={}",
            pack.artifact_graph.schema,
            pack.artifact_graph.nodes.len(),
            pack.artifact_graph.edges.len()
        ),
    ];
    for snippet in &pack.snippets {
        lines.push(format!(
            "- {}:{}-{} score={} occurrences={} terms={} hash={}",
            snippet.path,
            snippet.start_line,
            snippet.end_line,
            snippet.score,
            snippet.occurrences,
            snippet.matched_terms.join(","),
            snippet.content_hash
        ));
        if !snippet.excerpt.is_empty() {
            lines.push(snippet.excerpt.clone());
        }
    }
    for edge in &pack.artifact_graph.edges {
        lines.push(format!(
            "  graph: {} -> {} relation={} terms={}",
            edge.source,
            edge.target,
            edge.relation,
            edge.matched_terms.join(",")
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod stamped_argument_tests {
    use super::*;

    fn server_stamped(mut value: Value) -> Value {
        let object = value.as_object_mut().unwrap();
        object.extend(
            json!({
                "role_id":"builder", "department_id":"executing", "session_id":"query-fixture",
                "actor_id":"local-user", "project_trusted":true, "path_allow":["**"]
            })
            .as_object()
            .unwrap()
            .clone(),
        );
        value
    }

    #[test]
    fn context_query_stamped_arguments_keep_unknown_and_missing_required_business_keys_rejected() {
        for key in [
            "unexpected",
            "permission_profile",
            "dispatch_permit",
            "execution_context",
            "work_packet_id",
        ] {
            let mut value = server_stamped(
                json!({"project_root":"/fixture", "output":"json", "max_tokens":1000}),
            );
            value[key] = json!("forged-authority");
            assert_eq!(
                validated_repo_map_arguments(&value)
                    .err()
                    .expect("unknown field refusal"),
                PortError::Failed("context_arguments_invalid:keys".to_owned()),
                "{key}"
            );
        }
        let mut missing =
            server_stamped(json!({"project_root":"/fixture", "output":"json", "max_tokens":1000}));
        missing.as_object_mut().unwrap().remove("output");
        assert_eq!(
            validated_repo_map_arguments(&missing)
                .err()
                .expect("missing required business field refusal"),
            PortError::Failed("context_arguments_invalid:keys".to_owned())
        );
    }

    #[test]
    fn context_query_stamped_arguments_preserve_read_and_materialization_contracts() {
        let repo =
            server_stamped(json!({"project_root":"/fixture", "output":"json", "max_tokens":1000}));
        let parsed = validated_repo_map_arguments(&repo)
            .expect("server envelope is separate from business fields");
        assert_eq!(parsed.project_root, "/fixture");
        assert_eq!(parsed.output, "json");
        assert_eq!(parsed.max_tokens, Some(1000));
        let read = server_stamped(
            json!({"project_root":"/fixture", "root":null, "output":"text", "max_bytes_per_file":4096}),
        );
        let parsed = validated_read_arguments(ReadQueryOperation::ArtifactGraph, &read).unwrap();
        assert_eq!(parsed.project_root, "/fixture");
        assert_eq!(parsed.output, "text");
        assert_eq!(parsed.max_bytes_per_file, Some(4096));
        let write = server_stamped(
            json!({"project_root":"/fixture", "root":null, "output":"json",
            "cache":".kiana/context-index.json", "max_bytes_per_file":4096}),
        );
        let parsed =
            validated_materialization_arguments(MaterializationOperation::IndexCache, &write)
                .unwrap();
        assert_eq!(parsed.project_root, "/fixture");
        assert_eq!(parsed.cache.as_deref(), Some(".kiana/context-index.json"));
        assert_eq!(parsed.output, "json");
        assert_eq!(parsed.max_bytes_per_file, Some(4096));
    }

    #[test]
    fn context_query_accepts_daemon_stamped_hook_snapshot_metadata() {
        let arguments = server_stamped(json!({
            "project_root": "/fixture",
            "output": "json",
            "max_tokens": 1000,
            "hook_snapshot": {
                "schema": "kiana.hook-snapshot.v1",
                "digest": "sha256:hook-snapshot",
                "phase": "PreToolUse",
                "count": 0,
                "sandbox": "read-only"
            }
        }));
        let parsed = validated_repo_map_arguments(&arguments)
            .expect("the daemon-owned hook snapshot is sealed metadata");
        assert_eq!(parsed.max_tokens, Some(1000));
    }

    fn prepared_arguments(operation: &str, value: Value) -> Value {
        let mut request = kiana_domain::CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            operation,
            server_stamped(value),
        );
        request.risk = kiana_domain::capability_action_descriptor(operation)
            .expect("closed catalog operation")
            .minimum_risk;
        kiana_domain::normalize_capability_action(&mut request)
            .expect("use the same canonical normalization as Core and Broker");
        request.arguments
    }

    #[test]
    fn context_query_normalized_optional_repo_fields_keep_defaults() {
        let arguments = prepared_arguments(
            REPO_MAP_OPERATION,
            json!({"project_root":"/fixture", "output":"json", "max_tokens":null}),
        );
        assert!(!arguments.as_object().unwrap().contains_key("max_tokens"));
        let parsed = validated_repo_map_arguments(&arguments)
            .expect("canonical omission of an optional budget preserves its default");
        assert_eq!(parsed.project_root, "/fixture");
        assert_eq!(parsed.output, "json");
        assert_eq!(parsed.max_tokens, None);
        for required in ["project_root", "output"] {
            let mut missing = arguments.clone();
            missing.as_object_mut().unwrap().remove(required);
            assert_eq!(
                validated_repo_map_arguments(&missing).err().unwrap(),
                PortError::Failed("context_arguments_invalid:keys".to_owned())
            );
        }
    }

    #[test]
    fn context_query_normalized_optional_read_fields_keep_defaults() {
        for operation in [
            ReadQueryOperation::ArtifactGraph,
            ReadQueryOperation::ArtifactReadiness,
            ReadQueryOperation::Search,
            ReadQueryOperation::VectorSearch,
            ReadQueryOperation::Pack,
        ] {
            let needs_query = matches!(
                operation,
                ReadQueryOperation::Search
                    | ReadQueryOperation::VectorSearch
                    | ReadQueryOperation::Pack
            );
            let mut value = json!({"project_root":"/fixture", "output":"json", "root":null,
                "max_bytes_per_file":null});
            if needs_query {
                value["query"] = json!("checkout");
                value["limit"] = Value::Null;
            }
            if matches!(operation, ReadQueryOperation::Pack) {
                value["max_snippet_lines"] = Value::Null;
            }
            let arguments = prepared_arguments(operation.broker_key(), value);
            let parsed = validated_read_arguments(operation, &arguments)
                .expect("canonical optional fields keep the original read defaults");
            assert_eq!(parsed.project_root, "/fixture");
            assert_eq!(parsed.root, None);
            assert_eq!(parsed.limit, None);
            assert_eq!(parsed.max_bytes_per_file, None);
            assert_eq!(parsed.max_snippet_lines, None);
            assert_eq!(parsed.query.as_deref(), needs_query.then_some("checkout"));
            if needs_query {
                let mut missing = arguments.clone();
                missing.as_object_mut().unwrap().remove("query");
                assert_eq!(
                    validated_read_arguments(operation, &missing).err().unwrap(),
                    PortError::Failed("context_arguments_invalid:keys".to_owned())
                );
            }
            let mut injected = arguments;
            injected["store"] = json!("forged-store");
            assert_eq!(
                validated_read_arguments(operation, &injected)
                    .err()
                    .unwrap(),
                PortError::Failed("context_arguments_invalid:keys".to_owned())
            );
        }
    }

    #[test]
    fn context_query_normalized_optional_materialization_fields_keep_defaults() {
        for operation in [
            MaterializationOperation::Index,
            MaterializationOperation::IndexCache,
            MaterializationOperation::Artifacts,
            MaterializationOperation::ArtifactsCache,
            MaterializationOperation::ArtifactStore,
            MaterializationOperation::ArtifactStoreCache,
            MaterializationOperation::Ingest,
        ] {
            let needs_cache = matches!(
                operation,
                MaterializationOperation::IndexCache
                    | MaterializationOperation::ArtifactsCache
                    | MaterializationOperation::ArtifactStoreCache
            );
            let ingest = matches!(operation, MaterializationOperation::Ingest);
            let mut value = json!({"project_root":"/fixture", "output":"json", "root":null,
                "max_bytes_per_file":null});
            if needs_cache {
                value["cache"] = json!(".kiana/cache.json");
            }
            if ingest {
                value["source"] = json!("docs/source.md");
                value["store"] = Value::Null;
            }
            let arguments = prepared_arguments(operation.broker_key(), value);
            let parsed = validated_materialization_arguments(operation, &arguments)
                .expect("canonical optional fields keep the original materialization defaults");
            assert_eq!(parsed.project_root, "/fixture");
            assert_eq!(parsed.root, None);
            assert_eq!(parsed.max_bytes_per_file, None);
            assert_eq!(parsed.store, None);
            assert_eq!(
                parsed.cache.as_deref(),
                needs_cache.then_some(".kiana/cache.json")
            );
            assert_eq!(parsed.source.as_deref(), ingest.then_some("docs/source.md"));
            let required = if needs_cache {
                "cache"
            } else if ingest {
                "source"
            } else {
                "output"
            };
            let mut missing = arguments.clone();
            missing.as_object_mut().unwrap().remove(required);
            assert_eq!(
                validated_materialization_arguments(operation, &missing)
                    .err()
                    .unwrap(),
                PortError::Failed("context_arguments_invalid:keys".to_owned())
            );
            let mut injected = arguments;
            injected["query"] = json!("unregistered-read-field");
            assert_eq!(
                validated_materialization_arguments(operation, &injected)
                    .err()
                    .unwrap(),
                PortError::Failed("context_arguments_invalid:keys".to_owned())
            );
        }
    }
}
