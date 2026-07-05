/// kiana-query — Query engine orchestration layer.
///
/// Provides the building blocks for the query loop:
///
/// - `config`       — immutable per-query configuration snapshot
/// - `deps`         — dependency injection container (testable I/O interfaces)
/// - `token_budget` — token budget tracking and continuation decisions
/// - `transitions`  — pure query-loop state reducer
/// - `stop_hooks`   — post-turn stop-hook orchestration
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
    build_context_artifacts, build_context_index, build_context_pack,
    build_persistent_context_artifacts, build_persistent_context_index, search_context_index,
    ContextArtifactItem, ContextArtifactOptions, ContextArtifacts, ContextArtifactsCacheReport,
    ContextIndex, ContextIndexCacheReport, ContextIndexOptions, ContextIndexedFile, ContextPack,
    ContextPackOptions, ContextPackSnippet, ContextSearchHit, ContextSearchOptions,
    ContextSearchResults,
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
