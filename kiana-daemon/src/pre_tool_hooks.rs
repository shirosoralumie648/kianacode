use async_trait::async_trait;
use kiana_domain::{CapabilityRequest, RequestContext};
use kiana_ports::{PortError, PreToolHookDecision, PreToolHookPort};
use kiana_query::{PreToolUseHookContext, ToolHookDecision, run_pre_tool_use_hooks};
use kiana_types::ProjectTrust;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;

pub(crate) struct QueryPreToolHooks;

#[async_trait]
impl PreToolHookPort for QueryPreToolHooks {
    async fn decide(
        &self,
        context: &RequestContext,
        request: &CapabilityRequest,
    ) -> Result<PreToolHookDecision, PortError> {
        let project_trust = if context.project_trusted {
            ProjectTrust::Trusted
        } else {
            ProjectTrust::Untrusted
        };
        let decision = run_pre_tool_use_hooks(PreToolUseHookContext {
            abort_signal: Arc::new(Notify::new()),
            cwd: PathBuf::from(&context.project_root),
            project_trust,
            permission_mode: permission_mode_label(context.permission_profile),
            query_source: "kiana-harness".to_owned(),
            tool_name: hook_tool_name(&request.operation),
            tool_input: request.arguments.clone(),
            tool_use_id: request
                .arguments
                .get("call_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        })
        .await;

        Ok(match decision {
            ToolHookDecision::Allow => PreToolHookDecision::Allow,
            ToolHookDecision::UpdateInput(_) => {
                PreToolHookDecision::Block("hook_update_input_unapplied".to_owned())
            }
            ToolHookDecision::Block(reason) => PreToolHookDecision::Block(reason),
            ToolHookDecision::Ask { reason, .. } => PreToolHookDecision::Ask { reason },
        })
    }
}

fn hook_tool_name(operation: &str) -> String {
    match operation {
        "apply_patch" | "file_change" => "apply_patch".to_owned(),
        "shell.exec" | "shell" | "bash" | "exec" | "command_execution" => "shell".to_owned(),
        "mcp.call" | "mcp" => "mcp".to_owned(),
        other => other.to_owned(),
    }
}

fn permission_mode_label(profile: kiana_domain::PermissionProfile) -> String {
    match profile {
        kiana_domain::PermissionProfile::Safe => "safe".to_owned(),
        kiana_domain::PermissionProfile::Balanced => "balanced".to_owned(),
        kiana_domain::PermissionProfile::Autonomous => "autonomous".to_owned(),
    }
}
