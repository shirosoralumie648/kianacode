//! `kiana-query` 查询编排基础组件集合。
//!
//! 本 crate 提供查询循环所需的可组合模块：配置快照、窄范围依赖注入、文件/向量上下文
//! 索引、repo map、停止钩子、token 预算和纯状态 reducer。它是查询能力的适配层，不是
//! CompanyOS 的授权中心：任何会产生文件、进程、网络或记忆副作用的调用，都必须回到
//! `kiana-core::ControlPlane` 和受控 Broker。
//!
//! 各模块刻意把状态计算和效果边界分开：`transitions` 只返回意图；`index`/`repo_map` 的
//! 纯扫描、搜索 helper 只读取并生成上下文数据，但 `build_persistent_*`、manifest/generation
//! 写入和 ingest helper 会在调用方选择的本地路径上落盘；`stop_hooks` 负责执行已解析的 hook
//! 命令但不签发 Kiana 能力授权。项目本地 hook、plugin 和配置是否可加载，仍由组合根先完成
//! trust 检查。
//!
//! 这里的序列化结构和结果报告是当前实现的本地契约；字段存在不表示所有入口都已经接入，
//! 也不自动提供 durable、live 或 physical 证明。
//!
//! # 本 crate 在系统中的位置
//!
//! `kiana-query` 是“读取、索引、投影和少量受控 Hook 适配”的库层，不是第二个
//! `ControlPlane`，也不是可以独立决定权限的执行循环。主产品路径中，通常先由入口把请求
//! 交给 `kiana-core::ControlPlane`，通过授权后再由 `kiana-daemon::context_query` 注册的
//! handler 调用这里的索引、repo map、artifact 或 projection helper，结果再作为
//! `CapabilityResult` 回到 Core 的 finalizer，最后进入 receipt/响应视图。
//!
//! ```text
//! CLI / Workbench / Web request
//!     -> protocol normalize
//!     -> ControlPlane.authorize_and_execute
//!     -> daemon context_query handler
//!     -> kiana-query index / repo_map / projection adapter
//!     -> CapabilityResult
//!     -> Core finalizer / receipt / response
//! ```
//!
//! 源码中还存在两条需要分别理解的边界：
//!
//! 1. `kiana-entrypoints/src/cli.rs` 的 Direct Connect context 路由在 bearer 校验后，
//!    当前会直接调用部分 `build_persistent_*` 或 `ingest_context_artifacts` helper；这属于
//!    当前实现的直接 HTTP 适配路径，不能凭函数名推断它已经经过标准的
//!    `authorize_and_execute`。这也是实现与规范目标之间需要继续审计的地方，本 crate 不会
//!    自己补做缺失的授权。
//! 2. `kiana-tools/src/tool_execution.rs` 仍会通过这里重导出的 `stop_hooks` 运行遗留工具
//!    的 pre/post hook。它是兼容边界，不等于 CompanyOS 主执行链；hook 运行结果也不能替代
//!    EventLog、receipt 或 ControlPlane 的事实。
//!
//! # 模块和数据流分组
//!
//! - **事实投影与查询**：`*_projector`、`billing_api`、`connector_*`、`oauth_accounts` 等
//!   把调用方提供的已提交事实折叠成有界、可序列化的读模型。它们不因为名字带有
//!   `project` 就自动写 EventLog，也不凭空产生上游事实；输入快照、游标、授权范围和新鲜度
//!   必须由调用方提供并验证。
//! - **workspace context**：`index`、`repo_map`、`chunker`、`context_inputs`、
//!   `context_manifest`、`index_generation`、`index_invalidation`、`workspace_snapshot`、
//!   `cache_policy` 和 `unified_retrieval` 把工作区文件转换成检索候选、上下文包或持久化
//!   materialization。`write_*`、`build_persistent_*`、`ingest_context_artifacts` 会产生本地
//!   文件写入或缓存更新，不能按“query”一词把它们当成无副作用函数。
//! - **Hook 与纯状态**：`stop_hooks` 可能解析并启动外部 hook 进程；`hook_*` 模块负责
//!   取消、生命周期、结果、receipt 等边界数据；`transitions` 只根据事件和当前状态返回
//!   下一步意图。纯 reducer 不会替调用方执行该意图，外部进程也不会因此获得能力授权。
//!
//! 下方的 `pub mod` 建立模块路径，`pub use` 只是把一部分类型/函数复制到 crate 根的
//! 便捷入口。根导出是编译期 API 可见性，不是“所有功能都已上线”的证明；是否有生产调用者
//! 必须沿具体入口、handler、测试和 source guard 反向核实。

// 投影/查询模块：把已提交输入变成读模型，不承担主执行链的授权或事实写入。
pub mod allocation_projector;
pub mod audit_projector;
pub mod billing_api;
pub mod billing_ledger_projector;
pub mod cache_policy;
pub mod chunker;
pub mod config;
pub mod connector_health;
pub mod connector_quota;
pub mod context_inputs;
pub mod context_manifest;
pub mod cost_ledger_projector;
pub mod cost_projector;
pub mod data_boundary;
pub mod deps;
pub mod hook_cancellation;
pub mod hook_lifecycle_dispatch;
pub mod hook_outcome;
pub mod hook_receipt;
pub mod index;
pub mod index_generation;
pub mod index_invalidation;
pub mod oauth_accounts;
pub mod repo_map;
pub mod repo_map_task;
pub mod stop_hooks;
pub mod token_budget;
pub mod transitions;
pub mod unified_retrieval;
pub mod workspace_snapshot;

// 下面的重导出让上层可以从 `kiana_query::...` 访问稳定的 crate API。
// 它们没有改变调用方的数据流，也不会绕过各自模块中的校验和效果边界。
pub use allocation_projector::{
    project_cost_allocation, project_cost_allocation_portfolio, project_cost_allocations,
    AllocationTotals, CostAllocationPortfolio, CostAllocationPortfolioEntry,
    CostAllocationProjection, CostAllocationProjectionError, COST_ALLOCATION_PORTFOLIO_SCHEMA,
    COST_ALLOCATION_PORTFOLIO_VERSION, COST_ALLOCATION_PROJECTION_SCHEMA,
    COST_ALLOCATION_PROJECTION_VERSION,
};
pub use audit_projector::{
    append_audit_tail, audit_fold_source_event_digest, audit_projection_is_append_only,
    project_audit_from_scratch, AuditProjectionPosition, AuditProjectorAuthorization,
    AuditProjectorDisposition, AuditProjectorError, AuditProjectorFold, AuditProjectorFreshness,
    AuditProjectorQuery, AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA, AUDIT_PROJECTOR_DENY_CODES,
    AUDIT_PROJECTOR_FOLD_SCHEMA, AUDIT_PROJECTOR_FRESHNESS_SCHEMA, AUDIT_PROJECTOR_POSITION_SCHEMA,
    AUDIT_PROJECTOR_QUERY_SCHEMA, AUDIT_PROJECTOR_VERSION, MAX_AUDIT_PROJECTOR_QUERY_PAGE,
    MAX_AUDIT_PROJECTOR_TEXT,
};

pub use billing_api::{query_billing_projection, BillingQueryError, BILLING_QUERY_API_SCHEMA};
pub use billing_ledger_projector::{
    billing_source_is_rebuildable, project_billing_ledger, BillingLedgerProjectionError,
    BillingLedgerProjector, BillingSourceEvent, BILLING_LEDGER_PROJECTOR_IS_READ_ONLY,
    BILLING_LEDGER_PROJECTOR_SCHEMA, BILLING_LEDGER_PROJECTOR_VERSION,
    BILLING_PROJECTOR_NO_FACT_WRITES,
};
pub use cache_policy::{ContextCacheDecision, ContextCacheStatus, CONTEXT_CACHE_DECISION_SCHEMA};
pub use chunker::{chunk_text, ChunkingOptions};
pub use config::{is_env_truthy, QueryConfig, QueryGates};
pub use connector_health::{
    project_connector_health, ConnectorHealthProjection, ConnectorHealthProjectionEntry,
    CONNECTOR_HEALTH_PROJECTION_VERSION,
};
pub use connector_quota::{
    project_connector_quota, ConnectorQuotaProjection, ConnectorQuotaProjectionEntry,
    CONNECTOR_QUOTA_PROJECTION_SCHEMA, CONNECTOR_QUOTA_PROJECTION_VERSION,
};
pub use context_inputs::repo_map_candidate;
pub use context_manifest::{
    read_context_material_manifest, write_context_material_manifest_atomic,
    ContextMaterialManifest, CONTEXT_MATERIAL_MANIFEST_SCHEMA,
};
pub use cost_ledger_projector::{
    project_cost_ledger, CostLedgerProjection, CostLedgerProjectionError,
    COST_LEDGER_PROJECTION_SCHEMA, COST_LEDGER_PROJECTION_VERSION,
};
pub use cost_projector::{
    project_cost_receipt, project_receipt_cost, CostQueryProjectionError, CostReceiptProjection,
    COST_QUERY_PROJECTION_SCHEMA, COST_QUERY_PROJECTION_VERSION,
};
pub use data_boundary::{QueryDataBoundary, QueryDataDisposition, QUERY_DATA_BOUNDARY_SCHEMA};
pub use deps::QueryDeps;
pub use hook_cancellation::{
    cancellation_terminal, prepare_hook_invocation, retry_allowed, HookInvocationIdentity,
    HookRunRole, HookRunTerminal, HOOK_CANCELLATION_SCHEMA,
};
pub use hook_lifecycle_dispatch::{
    HookLifecycleDispatcher, HookLifecycleEvent, HookLifecycleEventKind,
    HOOK_LIFECYCLE_DISPATCH_SCHEMA,
};
pub use hook_outcome::{parse_hook_outcome, HookOutcome, HOOK_OUTCOME_SCHEMA};
pub use hook_receipt::{
    hook_receipt_digest, replay_hook_receipt, HookCleanupState, HookReceipt, HookReplayView,
    HOOK_RECEIPT_SCHEMA,
};
pub use index::{
    build_context_artifact_dependency_graph, build_context_artifact_readiness,
    build_context_artifact_store, build_context_artifacts, build_context_index, build_context_pack,
    build_persistent_context_artifact_store, build_persistent_context_artifacts,
    build_persistent_context_index, ingest_context_artifacts, search_context_index,
    search_context_vectors, ContextArtifactDependencyEdge, ContextArtifactDependencyGraph,
    ContextArtifactDependencyNode, ContextArtifactIngest, ContextArtifactIngestOptions,
    ContextArtifactIngestSyncReport, ContextArtifactItem, ContextArtifactOptions,
    ContextArtifactReadiness, ContextArtifactReadinessRole, ContextArtifactRoleSummary,
    ContextArtifactStore, ContextArtifactStoreCacheReport, ContextArtifacts,
    ContextArtifactsCacheReport, ContextIndex, ContextIndexCacheReport, ContextIndexOptions,
    ContextIndexedFile, ContextIngestedArtifact, ContextPack, ContextPackOptions,
    ContextPackSnippet, ContextSearchHit, ContextSearchOptions, ContextSearchResults,
    ContextVectorSearchHit, ContextVectorSearchOptions, ContextVectorSearchResults,
};
pub use index_generation::{
    build_context_index_generation, manifest_path, read_context_index_generation,
    read_index_manifest, write_context_index_generation_atomic, write_index_manifest_atomic,
    ContextIndexGenerationEnvelope, ContextIndexSourceManifest,
    CONTEXT_INDEX_GENERATION_ENVELOPE_SCHEMA, CONTEXT_INDEX_SOURCE_MANIFEST_SCHEMA,
};
pub use index_invalidation::{
    governed_index_invalidation, index_read_allowed, plan_index_invalidation,
};
pub use oauth_accounts::{
    project_oauth_accounts, OAuthAccountQueryPage, OAUTH_ACCOUNT_QUERY_SCHEMA,
};
pub use repo_map::{build_repo_map, RepoMap, RepoMapFile, RepoMapOptions};
pub use repo_map_task::select_repo_map_for_task;
pub use stop_hooks::{
    handle_stop_hooks, run_post_tool_use_hooks, run_pre_tool_use_hooks, run_session_start_hooks,
    run_user_prompt_submit_hooks, stop_hook_error_notification, HookInfo, PostToolUseHookContext,
    PreToolUseHookContext, SessionStartHookContext, StopHookContext, StopHookEvent, StopHookHandle,
    StopHookResult, ToolHookDecision, UserPromptSubmitHookContext, UserPromptSubmitHookResult,
};
pub use token_budget::{
    check_token_budget, get_budget_continuation_message, BudgetCompletionEvent, BudgetTracker,
    TokenBudgetDecision,
};
pub use transitions::{
    transition_query_state, QueryEffect, QueryEvent, QueryPhase, QueryState, QueryStopReason,
    QueryTransition,
};
pub use unified_retrieval::{rank_unified, UnifiedRetrievalPort};
pub use workspace_snapshot::{
    read_workspace_snapshot, snapshot_digest, WorkspaceReadContent, WorkspaceReadOutcome,
    WorkspaceSnapshotOptions,
};
