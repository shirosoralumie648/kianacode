pub mod connector_text;
pub mod hooks;
pub mod ids;
pub mod logs;
pub mod message;
pub mod permissions;
pub mod plugin;
pub mod runtime;
pub mod simple_types;
pub mod tools;
pub mod trust;
pub mod utils;

#[cfg(test)]
pub(crate) fn process_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub use connector_text::ConnectorTextBlock;
pub use ids::{AgentId, SessionId};
pub use logs::{sort_logs, Entry, LogOption};
pub use message::{AssistantMessage, Message, SystemMessage, UserMessage};
pub use permissions::{PermissionBehavior, PermissionDecision, PermissionMode};
pub use runtime::{
    sdk_message_to_runtime_event, MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent,
    RuntimeEventPayload, RuntimePermissionRequestEvent, RuntimeResultEvent, RuntimeSessionEvent,
    RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
pub use simple_types::{FileSuggestion, MessageQueueEntry, NotebookCell, StatusLineItem};
pub use trust::{
    find_project_trust_file, has_explicit_project_trust, legacy_project_trust_file_path,
    project_trust_file_path, project_trust_from_app_state, project_trust_id, project_trust_root,
    read_project_trust, remove_project_trust, write_project_trust, ProjectTrust,
    ProjectTrustRecord, PROJECT_TRUST_SCHEMA,
};
