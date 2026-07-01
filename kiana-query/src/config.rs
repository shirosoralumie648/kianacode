use kiana_types::ids::SessionId;
/// Query configuration snapshotted at query entry.
///
/// Corresponds to `config.ts` — immutable values resolved once per query() call.
/// Separating config from mutable state and tool-use context makes the query loop
/// easier to reason about: a pure `step(state, event, config)` reducer becomes
/// tractable when config is plain data.
///
/// Feature flags that are tree-shaking boundaries are intentionally excluded here
/// and must remain inline at the guarded blocks.
use serde::{Deserialize, Serialize};

/// Runtime gates resolved once at query entry (env vars / remote flags).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryGates {
    /// Whether the model supports streaming tool execution.
    pub streaming_tool_execution: bool,
    /// Whether tool-use summary messages should be emitted.
    pub emit_tool_use_summaries: bool,
    /// True when running as an Anthropic internal user (`USER_TYPE=ant`).
    pub is_ant: bool,
    /// Whether fast-mode is active (disabled by `CLAUDE_CODE_DISABLE_FAST_MODE`).
    pub fast_mode_enabled: bool,
}

/// Immutable per-query configuration snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryConfig {
    pub session_id: SessionId,
    pub gates: QueryGates,
}

impl QueryConfig {
    /// Build a `QueryConfig` from the current process environment.
    ///
    /// In production this is called once at the start of `query()`. Tests may
    /// construct `QueryConfig` directly with synthetic values.
    pub fn from_env(session_id: SessionId) -> Self {
        let streaming_tool_execution = std::env::var("CLAUDE_STREAMING_TOOL_EXECUTION")
            .map(|v| is_env_truthy(&v))
            .unwrap_or(false);

        let emit_tool_use_summaries = std::env::var("CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES")
            .map(|v| is_env_truthy(&v))
            .unwrap_or(false);

        let is_ant = std::env::var("USER_TYPE")
            .map(|v| v == "ant")
            .unwrap_or(false);

        let fast_mode_enabled = !std::env::var("CLAUDE_CODE_DISABLE_FAST_MODE")
            .map(|v| is_env_truthy(&v))
            .unwrap_or(false);

        QueryConfig {
            session_id,
            gates: QueryGates {
                streaming_tool_execution,
                emit_tool_use_summaries,
                is_ant,
                fast_mode_enabled,
            },
        }
    }
}

/// Returns `true` for values like "1", "true", "yes" (case-insensitive).
pub fn is_env_truthy(val: &str) -> bool {
    matches!(val.to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}
