use anyhow::{anyhow, Context};
use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_commands::{Command, CommandContext, CommandResult, CommandRoute};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ApprovalChallenge, ApprovalDecision, ApprovalId, ExecutionStatus, PermissionProfile,
    RequestEnvelope, RequestMetadata, ResponseEnvelope,
};
use serde_json::Value;
use std::sync::{Arc, OnceLock};

pub const APPROVE_LOCAL_WRITE_APP_STATE_KEY: &str = "approve_local_write";

static LOCAL_DAEMON: OnceLock<Arc<DaemonHost>> = OnceLock::new();

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
    let approve_local_write = context
        .app_state
        .get(APPROVE_LOCAL_WRITE_APP_STATE_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let approval_context = context.clone();
    match dispatch_command(command, context).await? {
        CommandDispatchOutcome::Completed(result) => Ok(result),
        CommandDispatchOutcome::AwaitingApproval(challenge) if approve_local_write => {
            resolve_command_approval(
                &approval_context,
                challenge.approval_id,
                ApprovalDecision::Approve,
            )
            .await
        }
        CommandDispatchOutcome::AwaitingApproval(challenge) => Err(anyhow!(
            "control_plane_command_awaiting_approval:{}",
            serde_json::to_string(&challenge)?
        )),
    }
}

#[derive(Clone, Debug)]
pub enum CommandDispatchOutcome {
    Completed(CommandResult),
    AwaitingApproval(ApprovalChallenge),
}

pub async fn dispatch_command(
    command: &dyn Command,
    context: CommandContext,
) -> anyhow::Result<CommandDispatchOutcome> {
    match command.route(&context)? {
        CommandRoute::Local => command
            .execute(context)
            .await
            .map(CommandDispatchOutcome::Completed),
        CommandRoute::ControlPlane { name, arguments } => {
            execute_control_plane_command(&context, name, arguments).await
        }
    }
}

async fn execute_control_plane_command(
    context: &CommandContext,
    name: String,
    arguments: Value,
) -> anyhow::Result<CommandDispatchOutcome> {
    let metadata = request_metadata(context)?;
    let client = KianaClient::new(LocalDaemonTransport {
        host: local_daemon()?,
    });
    let response = client
        .command(metadata, name, arguments)
        .await
        .map_err(anyhow::Error::msg)?;
    response_outcome(response)
}

pub async fn resolve_command_approval(
    context: &CommandContext,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
) -> anyhow::Result<CommandResult> {
    let response = resolve_command_approval_response(context, approval_id, decision).await?;
    match response_outcome(response)? {
        CommandDispatchOutcome::Completed(result) => Ok(result),
        CommandDispatchOutcome::AwaitingApproval(_) => {
            Err(anyhow!("approval_decision_returned_new_challenge"))
        }
    }
}

pub async fn resolve_command_approval_response(
    context: &CommandContext,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
) -> anyhow::Result<ResponseEnvelope> {
    let metadata = request_metadata(context)?;
    let client = KianaClient::new(LocalDaemonTransport {
        host: local_daemon()?,
    });
    client
        .approval_decision(metadata, approval_id, decision)
        .await
        .map_err(anyhow::Error::msg)
}

fn request_metadata(context: &CommandContext) -> anyhow::Result<RequestMetadata> {
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
    metadata.actor_id = Some(
        context
            .app_state
            .get("actor_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .unwrap_or("local-command")
            .to_owned(),
    );
    metadata.project_trusted =
        kiana_types::project_trust_from_app_state(&context.app_state).as_bool();
    metadata.permission_profile = PermissionProfile::Safe;
    Ok(metadata)
}

fn response_outcome(response: ResponseEnvelope) -> anyhow::Result<CommandDispatchOutcome> {
    if response.status == ExecutionStatus::AwaitingApproval {
        let challenge = response
            .output
            .get("approval")
            .cloned()
            .ok_or_else(|| anyhow!("control_plane_approval_challenge_missing"))?;
        let challenge = serde_json::from_value(challenge)
            .context("control_plane_approval_challenge_invalid")?;
        return Ok(CommandDispatchOutcome::AwaitingApproval(challenge));
    }
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
    serde_json::from_value(result)
        .context("control_plane_command_result_invalid")
        .map(CommandDispatchOutcome::Completed)
}

fn local_daemon() -> anyhow::Result<Arc<DaemonHost>> {
    if let Some(host) = LOCAL_DAEMON.get() {
        return Ok(host.clone());
    }
    let candidate = Arc::new(DaemonHost::local()?);
    let _ = LOCAL_DAEMON.set(candidate);
    LOCAL_DAEMON
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("local_daemon_initialization_failed"))
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
    use kiana_commands::{context::ContextCommand, CommandType};
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    #[tokio::test]
    async fn context_repo_map_uses_client_daemon_core_and_query_handler() {
        let root = fixture_root("repo-map");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub struct RoutedContext;\n").unwrap();
        let result = execute_command(
            &ContextCommand,
            CommandContext {
                args: "repo-map --json --max-tokens 1000".to_owned(),
                app_state: HashMap::from([
                    ("cwd".to_owned(), Value::String(root.display().to_string())),
                    ("project_trusted".to_owned(), Value::Bool(true)),
                ]),
            },
        )
        .await
        .unwrap();
        let map: Value = serde_json::from_str(&result.value).unwrap();
        assert_eq!(map["token_budget"], 1000);
        assert_eq!(map["files"][0]["path"], "src/lib.rs");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn context_repo_map_does_not_promote_unknown_project_trust() {
        let root = fixture_root("untrusted");
        fs::create_dir_all(&root).unwrap();
        let error = execute_command(
            &ContextCommand,
            CommandContext {
                args: "repo-map --json".to_owned(),
                app_state: HashMap::from([(
                    "cwd".to_owned(),
                    Value::String(root.display().to_string()),
                )]),
            },
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "control_plane_command_denied:project_untrusted"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn context_read_queries_use_the_same_dispatcher() {
        let root = fixture_root("read-queries");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout_flow() {}\n// checkout workflow\n",
        )
        .unwrap();
        let app_state = HashMap::from([
            ("cwd".to_owned(), Value::String(root.display().to_string())),
            ("project_trusted".to_owned(), Value::Bool(true)),
        ]);

        let search = execute_command(
            &ContextCommand,
            CommandContext {
                args: "search checkout --json --limit 1".to_owned(),
                app_state: app_state.clone(),
            },
        )
        .await
        .unwrap();
        let search: Value = serde_json::from_str(&search.value).unwrap();
        assert_eq!(search["schema"], "kiana.context-search.v1");
        assert_eq!(search["hits"][0]["path"], "src/lib.rs");

        let pack = execute_command(
            &ContextCommand,
            CommandContext {
                args: "pack checkout --json --limit 1 --max-snippet-lines 1".to_owned(),
                app_state,
            },
        )
        .await
        .unwrap();
        let pack: Value = serde_json::from_str(&pack.value).unwrap();
        assert_eq!(pack["schema"], "kiana.context-pack.v1");
        assert_eq!(pack["snippets"][0]["path"], "src/lib.rs");

        let _ = fs::remove_dir_all(root);
    }

    fn fixture_root(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-command-dispatch-{label}-{}-{nanos}",
            std::process::id()
        ))
    }
}
