//! `kiana-query` 查询编排基础组件集合。
//!
//! 本 crate 提供查询循环所需的可组合模块：配置快照、窄范围依赖注入、文件/向量上下文
//! 索引、repo map、停止钩子、token 预算和纯状态 reducer。它是查询能力的适配层，不是
//! CompanyOS 的授权中心：任何会产生文件、进程、网络或记忆副作用的调用，都必须回到
//! `kiana-core::ControlPlane` 和受控 Broker。
//!
//! 各模块刻意将状态与效果拆开：`transitions` 只返回意图，`index`/`repo_map` 只读取并
//! 生成上下文数据，`stop_hooks` 负责执行已解析的 hook 命令但不签发 Kiana 能力授权。项目
//! 本地 hook、plugin 和配置是否可加载，仍由组合根先完成 trust 检查。
//!
//! 这里的序列化结构和结果报告是当前实现的本地契约；字段存在不表示所有入口都已经接入，
//! 也不自动提供 durable、live 或 physical 证明。
pub mod config;
pub mod deps;
pub mod index;
pub mod repo_map;
pub mod stop_hooks;
pub mod token_budget;
pub mod transitions;

pub use config::{is_env_truthy, QueryConfig, QueryGates};
pub use deps::QueryDeps;
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
pub use repo_map::{build_repo_map, RepoMap, RepoMapFile, RepoMapOptions};
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
