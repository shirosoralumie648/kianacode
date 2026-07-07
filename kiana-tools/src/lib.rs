pub mod agent;
pub mod bash_sandbox;
pub mod bash_tool;
pub mod config_tool;
pub mod cron_create;
pub mod cron_delete;
pub mod cron_list;
pub mod discover_skills;
pub mod enter_plan_mode;
pub mod enter_worktree;
pub mod exec_policy;
pub mod exit_plan_mode;
pub mod exit_worktree;
pub mod file_delete;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod glob;
pub mod grep;
pub mod lsp_tool;
pub mod mcp_tool;
pub mod monitor_tool;
pub mod notebook_edit;
pub mod notebook_execute;
pub mod permissions;
pub mod powershell_tool;
pub mod registry;
pub mod remote_trigger;
pub mod repl_tool;
pub mod send_message;
pub mod shell;
pub mod skill_tool;
pub mod synthetic_output;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod team_create;
pub mod team_delete;
pub mod team_lifecycle;
pub mod todo_write;
pub mod tool;
pub mod tool_execution;
pub mod tool_search;
pub mod user_interaction;
pub mod utils;
pub mod web_fetch;
pub mod web_search;
pub mod workflow_tool;

pub use registry::{create_default_registry, ToolRegistry};
pub use tool::{
    PermissionDecision, Tool, ToolContext, ToolError, ToolInput, ToolOutput, ValidationResult,
};

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    pub(crate) fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }
}
