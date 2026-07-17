use anyhow::{anyhow, Context};
use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_commands::{Command, CommandContext, CommandResult, CommandRoute};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope,
};
use serde_json::Value;
use std::sync::Arc;

struct LocalDaemonTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for LocalDaemonTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

pub async fn execute_command(
    command: &dyn Command,
    context: CommandContext,
) -> anyhow::Result<CommandResult> {
    match command.route(&context)? {
        CommandRoute::Local => command.execute(context).await,
        CommandRoute::ControlPlane { name, arguments } => {
            execute_control_plane_command(&context, name, arguments).await
        }
    }
}

async fn execute_control_plane_command(
    context: &CommandContext,
    name: String,
    arguments: Value,
) -> anyhow::Result<CommandResult> {
    let project_root = context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("control_plane_project_root_required"))?;
    let session_id = context
        .app_state
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("local-command");
    let mut metadata = RequestMetadata::local(session_id, project_root);
    metadata.actor_id = Some("local-command".to_owned());
    metadata.project_trusted =
        kiana_types::project_trust_from_app_state(&context.app_state).as_bool();
    metadata.permission_profile = PermissionProfile::Safe;

    let client = KianaClient::new(LocalDaemonTransport {
        host: Arc::new(DaemonHost::local()),
    });
    let response = client
        .command(metadata, name, arguments)
        .await
        .map_err(anyhow::Error::msg)?;
    if response.status != ExecutionStatus::Completed {
        return Err(anyhow!(
            "control_plane_command_{}:{}",
            status_name(response.status),
            response.error.as_deref().unwrap_or("unknown")
        ));
    }
    let result = response
        .output
        .get("command_result")
        .cloned()
        .ok_or_else(|| anyhow!("control_plane_command_result_missing"))?;
    serde_json::from_value(result).context("control_plane_command_result_invalid")
}

fn status_name(status: ExecutionStatus) -> &'static str {
    match status {
        ExecutionStatus::Accepted => "accepted",
        ExecutionStatus::Denied => "denied",
        ExecutionStatus::AwaitingApproval => "awaiting_approval",
        ExecutionStatus::Running => "running",
        ExecutionStatus::Completed => "completed",
        ExecutionStatus::Failed => "failed",
        ExecutionStatus::ResultUnknown => "result_unknown",
        ExecutionStatus::Blocked => "blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_commands::CommandType;
    use std::collections::HashMap;

    struct LocalCommand;

    #[async_trait]
    impl Command for LocalCommand {
        fn name(&self) -> &str {
            "local"
        }

        fn description(&self) -> &str {
            "local test command"
        }

        fn command_type(&self) -> CommandType {
            CommandType::Local
        }

        async fn execute(&self, _context: CommandContext) -> anyhow::Result<CommandResult> {
            Ok(CommandResult::text("local-result"))
        }
    }

    struct RoutedCommand;

    #[async_trait]
    impl Command for RoutedCommand {
        fn name(&self) -> &str {
            "routed"
        }

        fn description(&self) -> &str {
            "routed test command"
        }

        fn command_type(&self) -> CommandType {
            CommandType::Local
        }

        fn route(&self, _context: &CommandContext) -> anyhow::Result<CommandRoute> {
            Ok(CommandRoute::ControlPlane {
                name: "unknown.command".to_owned(),
                arguments: Value::Null,
            })
        }

        async fn execute(&self, _context: CommandContext) -> anyhow::Result<CommandResult> {
            panic!("routed command must not execute locally")
        }
    }

    #[tokio::test]
    async fn local_commands_keep_the_existing_execution_path() {
        let result = execute_command(
            &LocalCommand,
            CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.value, "local-result");
    }

    #[tokio::test]
    async fn routed_commands_require_an_explicit_project_root() {
        let error = execute_command(
            &RoutedCommand,
            CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(error.to_string(), "control_plane_project_root_required");
    }
}
