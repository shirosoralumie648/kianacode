//! 上下文包（context pack）的查询入口。
//!
//! # 「上下文包」是什么
//!
//! 一次请求要交给模型看的所有东西的**打包结果**：相关文件、历史摘要、
//! 项目约定、检索片段。它是**给模型的一次性输入**，不是事实。
//!
//! # 为什么查询它要走控制面
//!
//! 因为「哪些东西该进上下文」是一个**授权决定**：
//! 一个文件该不该被这次请求看见，取决于这个主体有没有权限看见它。
//! 如果在入口层直接按路径读文件，这条路径上的任何 containment 检查
//! 都不存在——而它本来应该在 `ControlPlane` 里。
//!
//! 所以这个文件只有查询，没有读取。
//!
//! # 在完整调用链中的位置
//!
//! 本文件是 `ControlPlane` 内部的“意图规范化层”。它不碰文件系统，也不直接调用
//! `kiana-query`；它把入口层传入的 `context.query.v1` JSON 转换成一个闭合的
//! `CapabilityRequest`，再交给统一的 `authorize_and_execute`。真正的工作区读取、索引或
//! materialization 由 `kiana-daemon/src/context_query.rs` 注册的 Broker handler 完成。
//!
//! ```text
//! CLI / Workbench / Web
//!   -> CommandIntent(name=context.query.v1, arguments)
//!   -> handle_context_query()
//!   -> normalize_context_query_arguments()
//!   -> CapabilityRequest(operation + normalized arguments + RiskLevel)
//!   -> ControlPlane::authorize_and_execute()
//!   -> policy / gate / approval / broker / daemon handler
//!   -> kiana-query -> CapabilityResult -> receipt / response
//! ```
//!
//! 规范化是安全边界而不只是 JSON 转换：未知顶层键、未知 operation、非法输出模式、越界
//! 数值和绝对/父目录路径会在进入授权或文件访问前被拒绝。成功的结果只代表“请求被整理成
//! 允许继续评估的 capability”，不代表已经读取文件、写入 cache、生成 receipt 或证明模型
//! 看到了某些内容。
use super::*;

impl ControlPlane {
    /// 处理一次上下文包查询。
    ///
    /// 【返回的是「可以被看见的部分」】
    /// 它的职责是**收窄**而不是**取来**：把这次请求有权看到的东西
    /// 组装成一个包，把无权看到的部分**排除在外**。
    ///
    /// ⚠ 这也是为什么它必须返回可核对的内容：调用方要能回答
    /// 「模型这次看到了什么」。答不上来的话，一次信息泄漏就无法被发现。
    ///
    /// 【调用者】`commands.rs` 在识别到 `CONTEXT_QUERY_COMMAND`（当前 wire 名为
    /// `context.query.v1`）后调用。
    ///
    /// 【输入】`RequestContext` 提供 request/session/actor/project 等服务端上下文；`arguments`
    /// 是入口层的 JSON intent，使用人类易懂的 operation 名称和 `options` 对象。
    ///
    /// 【失败路径】规范化失败时仍先写入 `request.accepted` 和 `command.rejected` 两个事件，
    /// 再返回 `CoreResponse::blocked`。这表示“请求已被系统接收但命令参数不准入”，而不是
    /// 让无效输入继续进入授权器。事件追加失败会按 `CoreError` 向上返回，不能伪造一个已
    /// 记录的拒绝结果。
    ///
    /// 【成功路径】规范化后的 operation、参数和风险级别组成 `CapabilityRequest`，随后由
    /// `authorize_and_execute` 继续做权限、策略、审批、permit、Broker 和最终事实处理。
    /// 本函数不执行读取/写入、不自己重试，也不绕过 ControlPlane 建第二条查询循环。
    pub(crate) async fn handle_context_query(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let normalized = match normalize_context_query_arguments(&context, &arguments) {
            Ok(normalized) => normalized,
            Err(error) => {
                let reason = error.reason();
                self.append_event(
                    request_id,
                    1,
                    "request.accepted",
                    json!({ "command": CONTEXT_QUERY_COMMAND }),
                )
                .await?;
                self.append_event(
                    request_id,
                    2,
                    "command.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            request_id,
            CapabilityKind::Query,
            normalized.operation,
            normalized.arguments,
        )
        .with_risk(normalized.risk);
        self.authorize_and_execute(&context, request).await
    }
}

fn normalize_context_query_arguments(
    context: &RequestContext,
    arguments: &Value,
) -> Result<NormalizedContextQuery, ContextQueryIntentError> {
    // 入口使用的是面向用户的 command 形状：{operation, output, options, project_root}。
    // 这里必须重新构造 Broker 需要的参数，而不是把整个 JSON 原样透传；原样透传会让未知
    // 字段有机会穿过 Core 进入 Daemon handler，破坏 operation 各自的 allowlist。
    let arguments = arguments
        .as_object()
        .ok_or(ContextQueryIntentError::Invalid)?;
    if arguments.keys().any(|key| {
        !matches!(
            key.as_str(),
            "operation" | "output" | "options" | "project_root"
        )
    }) {
        return Err(ContextQueryIntentError::Invalid);
    }
    let operation = arguments
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    let output = arguments
        .get("output")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    if !matches!(output, "json" | "text") {
        return Err(ContextQueryIntentError::Invalid);
    }
    let options = arguments
        .get("options")
        .and_then(Value::as_object)
        .ok_or(ContextQueryIntentError::Invalid)?;

    let (broker_operation, risk, normalized) = match operation {
        // repo map 只需要项目根和可选 token 预算；u64::MAX 表示 Core 这一层没有额外的
        // 数值上限，实际输出仍由 kiana-query/下游上下文预算决定。
        "repo_map" => {
            ensure_option_keys(options, &["max_tokens"])?;
            let max_tokens = optional_bounded_u64(options, "max_tokens", u64::MAX)?;
            (
                CONTEXT_REPO_MAP_OPERATION,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "output": output,
                    "max_tokens": max_tokens,
                }),
            )
        }
        // 依赖图和 readiness 都是工作区只读视图。root 必须是项目内相对路径，单文件大小
        // 上限为 16 MiB（MAX_CONTEXT_BYTES_PER_FILE），避免一次请求把无限大文件交给解析器。
        "artifact_graph" | "artifact_readiness" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = if operation == "artifact_graph" {
                CONTEXT_ARTIFACT_GRAPH_OPERATION
            } else {
                CONTEXT_ARTIFACT_READINESS_OPERATION
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        // 这些 operation 名字看似“读取”，但它们生成完整 index/artifact store 结构；真正的
        // 文件访问和治理撤销检查在 Daemon handler 仍会再次执行。此处 risk 仍是 ReadOnly，
        // 因为当前 variants 只构造内存结果，明确的 cache 写入有独立 operation。
        "index" | "artifacts" | "artifact_store" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index" => CONTEXT_INDEX_OPERATION,
                "artifacts" => CONTEXT_ARTIFACTS_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        // cache write 明确标记为 LocalWrite，必须走写权限/审批策略。cache 是相对路径且必填，
        // 不能用空值让 Daemon 猜一个写入位置；max_bytes_per_file 仍限制扫描输入。
        "index_cache_write" | "artifacts_cache_write" | "artifact_store_cache_write" => {
            ensure_option_keys(options, &["root", "cache", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let cache = required_relative_path(options, "cache")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index_cache_write" => CONTEXT_INDEX_CACHE_OPERATION,
                "artifacts_cache_write" => CONTEXT_ARTIFACTS_CACHE_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_CACHE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "cache": cache,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        // ingest 同时指定 source 和可选 store：source 是现有输入目录，store 是项目内输出目录。
        // 这里只验证形状和相对路径；source 是否存在、是否与 store 重叠由 Daemon 的 canonical
        // path 层处理，避免 Core 在没有本地 filesystem adapter 时作出错误判断。
        "artifact_ingest_write" => {
            ensure_option_keys(options, &["root", "source", "store", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let source = required_relative_path(options, "source")?;
            let store = optional_relative_path(options, "store")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            (
                CONTEXT_ARTIFACT_INGEST_OPERATION,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "source": source,
                    "store": store,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        // 搜索/向量搜索/pack 必须有非空 query；limit=1000 和 snippet=1000 是默认上界常量，
        // max_bytes_per_file=16 MiB 是单文件读取上界。pack 比另外两种搜索多一个 snippet 行数
        // 参数，因此 allowlist 也必须不同，否则一个 operation 的未使用字段会被误接受。
        "search" | "vector_search" | "pack" => {
            let allowed = if operation == "pack" {
                &[
                    "query",
                    "root",
                    "limit",
                    "max_bytes_per_file",
                    "max_snippet_lines",
                ][..]
            } else {
                &["query", "root", "limit", "max_bytes_per_file"][..]
            };
            ensure_option_keys(options, allowed)?;
            let query = options
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .ok_or(ContextQueryIntentError::Invalid)?;
            let root = optional_relative_path(options, "root")?;
            let limit = optional_bounded_u64(options, "limit", MAX_CONTEXT_LIMIT)?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let max_snippet_lines = if operation == "pack" {
                optional_bounded_u64(options, "max_snippet_lines", MAX_CONTEXT_SNIPPET_LINES)?
            } else {
                None
            };
            let broker_operation = match operation {
                "search" => CONTEXT_SEARCH_OPERATION,
                "vector_search" => CONTEXT_VECTOR_SEARCH_OPERATION,
                _ => CONTEXT_PACK_OPERATION,
            };
            let normalized = if operation == "pack" {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                    "max_snippet_lines": max_snippet_lines,
                })
            } else {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                })
            };
            (broker_operation, RiskLevel::ReadOnly, normalized)
        }
        _ => return Err(ContextQueryIntentError::Unregistered),
    };

    Ok(NormalizedContextQuery {
        operation: broker_operation,
        risk,
        arguments: normalized,
    })
}

fn ensure_option_keys(
    options: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), ContextQueryIntentError> {
    // options 是 operation 专属的小 schema。未知键直接 Invalid，而不是忽略，原因是忽略会
    // 让调用者误以为某个权限、过滤器或路径选项已经生效，且会使签名/审计看到的请求和真正
    // 执行的请求不一致。
    if options.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

fn optional_bounded_u64(
    options: &serde_json::Map<String, Value>,
    key: &str,
    maximum: u64,
) -> Result<Option<u64>, ContextQueryIntentError> {
    // None 表示调用方没有指定该预算，后续层使用默认值；Some 必须是正整数且不超过调用者
    // 传入的 maximum。这里拒绝 0 是为了避免“零字节/零条结果”把合法请求悄悄变成空视图，
    // 也避免把 0 当成“无限制”的歧义。u64 解析失败还会覆盖负数、浮点和字符串。
    match options.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0 && *value <= maximum)
            .map(Some)
            .ok_or(ContextQueryIntentError::Invalid),
    }
}

fn optional_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ContextQueryIntentError> {
    // 可选路径的三种状态要区分：字段缺失 => None（使用项目根/下游默认）；字段存在但空或
    // 非字符串 => Invalid；字段是非空相对路径 => 复制为 owned String，供后续 request 使用。
    let Some(value) = options.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(Some(value.to_owned()))
}

fn required_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ContextQueryIntentError> {
    // cache/source 这类路径没有安全默认值，必须由调用方显式提供。先做非空字符串检查，
    // 再走同一份相对路径规则，防止“缺失”和“非法绝对路径”被混成一个下游 I/O 错误。
    let value = options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(value.to_owned())
}

fn validate_relative_path(value: &str) -> Result<(), ContextQueryIntentError> {
    // 这里只做词法层 traversal 检查：绝对路径、`..`、根组件和 Windows prefix 都拒绝。
    // 它不能证明路径当前存在，也不能发现 symlink；Daemon 访问真实 filesystem 时还必须
    // canonicalize 并做 project-root containment，这两个层次不能互相替代。
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

struct NormalizedContextQuery {
    // 这是 Core -> Broker 的内部中间表示。operation 已从用户别名映射成稳定 wire key，risk
    // 用于策略/审批决策，arguments 只包含对应 handler 允许的业务字段。
    operation: &'static str,
    risk: RiskLevel,
    arguments: Value,
}

enum ContextQueryIntentError {
    // Invalid 是形状/值/路径不符合 command 契约；Unregistered 是 operation 名称不在当前
    // 产品能力目录中。两者都在 handle_context_query 中变成结构化 blocked response，但 reason
    // 不应被压成通用 I/O 错误，否则客户端无法区分“参数错”和“能力不存在”。
    Invalid,
    Unregistered,
}

impl ContextQueryIntentError {
    fn reason(&self) -> &'static str {
        match self {
            Self::Invalid => "command_arguments_invalid",
            Self::Unregistered => "command_unregistered",
        }
    }
}
