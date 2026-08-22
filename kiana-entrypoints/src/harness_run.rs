//! Product-surface execution through the owned Kiana harness.
//!
//! Print mode and the SDK must start runs via `kiana-daemon` / `kiana-core`.
//! They must not call the legacy `runner.rs` model/tool loop.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope, RoleSpec,
    RunId,
};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub const HARNESS_ID: &str = "kiana-harness";

pub struct LocalDaemonTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for LocalDaemonTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

#[derive(Debug, Clone)]
pub struct HarnessRunResult {
    pub text: String,
    pub steps: u64,
    pub sandbox: String,
    pub output: Value,
}

pub async fn run_envelope(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let policy = sandbox_policy_from_options(options)?;
    let (client, metadata) = local_client(session_id, options)?;
    client
        .run(metadata, prompt.into(), policy.sandbox)
        .await
        .map_err(anyhow::Error::msg)
}

pub async fn run_owned_harness(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<HarnessRunResult> {
    completed_harness_result(run_envelope(session_id, prompt, options).await?)
}

fn local_client(
    session_id: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<(KianaClient<LocalDaemonTransport>, RequestMetadata)> {
    let project_root = project_root_from_options(options)?;
    let trusted = project_trusted(&project_root)?;
    let policy = sandbox_policy_from_options(options)?;
    let mut metadata = RequestMetadata::local(session_id.into(), project_root);
    metadata.actor_id = Some("local-cli".to_owned());
    metadata.project_trusted = trusted;
    metadata.permission_profile = policy.permission_profile;
    if let Some(role_id) = string_option(options, "role") {
        let role = RoleSpec::lookup(&role_id).ok_or_else(|| anyhow!("role_unknown"))?;
        metadata.assign_role(&role);
    }
    let client = KianaClient::new(LocalDaemonTransport {
        host: Arc::new(DaemonHost::local().map_err(anyhow::Error::msg)?),
    });
    Ok((client, metadata))
}

pub async fn continue_envelope(
    session_id: impl Into<String>,
    prompt: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let session_id = session_id.into();
    let policy = sandbox_policy_from_options(options)?;
    let (client, metadata) = local_client(session_id, options)?;
    client
        .continue_run(metadata, prompt.into(), policy.sandbox, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

pub async fn cancel_envelope(
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    reason: impl Into<String>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let (client, metadata) = local_client(session_id, options)?;
    client
        .cancel_run(metadata, run_id, reason.into())
        .await
        .map_err(anyhow::Error::msg)
}

pub async fn receipt_envelope(
    session_id: impl Into<String>,
    run_id: Option<RunId>,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let (client, metadata) = local_client(session_id, options)?;
    client
        .receipt(metadata, run_id)
        .await
        .map_err(anyhow::Error::msg)
}

pub fn completed_harness_result(response: ResponseEnvelope) -> Result<HarnessRunResult> {
    if response.status != ExecutionStatus::Completed {
        return Err(anyhow!(
            "{}",
            response
                .error
                .as_deref()
                .filter(|error| !error.is_empty())
                .unwrap_or("kiana_harness_failed")
        ));
    }
    let text = response.output["output"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    Ok(HarnessRunResult {
        text,
        steps: response.output["output"]["steps"].as_u64().unwrap_or(1),
        sandbox: response
            .output
            .get("sandbox")
            .and_then(Value::as_str)
            .unwrap_or("read-only")
            .to_owned(),
        output: response.output,
    })
}

pub fn project_root_from_options(options: &HashMap<String, Value>) -> Result<String> {
    if let Some(cwd) = string_option(options, "cwd") {
        return Ok(cwd);
    }
    std::env::current_dir()
        .context("failed to resolve current directory")
        .map(|path| path.to_string_lossy().into_owned())
}

pub fn project_trusted(project_root: &str) -> Result<bool> {
    match kiana_types::read_project_trust(project_root) {
        Ok(Some(trust)) => Ok(trust.as_bool()),
        Ok(None) => Ok(false),
        Err(error) => Err(anyhow!("project_trust_unavailable:{error}")),
    }
}

#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub sandbox: Option<String>,
    pub permission_profile: PermissionProfile,
}

pub fn sandbox_policy_from_options(options: &HashMap<String, Value>) -> Result<SandboxPolicy> {
    if let Some(sandbox) = string_option(options, "sandbox") {
        return sandbox_policy_from_name(&sandbox);
    }
    if let Some(profile) = string_option(options, "permission_profile")
        .or_else(|| env_nonempty("KIANA_PERMISSION_PROFILE"))
    {
        return sandbox_policy_from_permission_profile(&profile);
    }
    if let Some(mode) =
        string_option(options, "permission_mode").or_else(|| env_nonempty("KIANA_PERMISSION_MODE"))
    {
        return sandbox_policy_from_permission_mode(&mode);
    }
    Ok(read_only_policy())
}

pub fn prompt_from_session_messages(messages: &[Value], json_schema: Option<&Value>) -> String {
    let transcript = if messages.len() <= 1 {
        messages.first().map(message_text).unwrap_or_default()
    } else {
        messages
            .iter()
            .filter_map(|message| {
                let role = message
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let text = message_text(message);
                if text.is_empty() {
                    None
                } else {
                    Some(format!("{role}: {text}"))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    match json_schema {
        Some(schema) => {
            format!("{transcript}\n\nRespond with JSON matching this schema:\n{schema}")
        }
        None => transcript,
    }
}

pub fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| {
                if let Some(text) = block.as_str() {
                    return Some(text.to_string());
                }
                if block.get("type").and_then(Value::as_str) == Some("text") {
                    block
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(block)) if block.get("type").and_then(Value::as_str) == Some("text") => {
            block
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        }
        _ => String::new(),
    }
}

fn sandbox_policy_from_name(value: &str) -> Result<SandboxPolicy> {
    match normalize_token(value).as_str() {
        "read-only" | "readonly" | "read_only" => Ok(read_only_policy()),
        "workspace-write" | "workspacewrite" | "workspace_write" | "workspace" => {
            Ok(workspace_write_policy())
        }
        "danger-full-access" | "dangerfullaccess" | "dangerously-skip-permissions" | "full" => {
            Err(anyhow!("danger_full_access_rejected"))
        }
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

fn sandbox_policy_from_permission_profile(value: &str) -> Result<SandboxPolicy> {
    match normalize_token(value).as_str() {
        "read-only" | "readonly" | "read_only" | "ask" | "plan" | "safe" => Ok(read_only_policy()),
        "workspace" | "workspace-write" | "default" | "balanced" => Ok(workspace_write_policy()),
        "full" | "autonomous" | "danger-full-access" => Err(anyhow!("danger_full_access_rejected")),
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

fn sandbox_policy_from_permission_mode(value: &str) -> Result<SandboxPolicy> {
    match normalize_token(value).as_str() {
        "default" | "plan" | "ask" | "dontask" | "dont-ask" | "dont_ask" => Ok(read_only_policy()),
        "acceptedits" | "accept-edits" | "accept_edits" => Ok(workspace_write_policy()),
        "bypasspermissions" | "bypass-permissions" | "bypass_permissions" | "auto"
        | "danger-full-access" => Err(anyhow!("danger_full_access_rejected")),
        other => Err(anyhow!("sandbox_unsupported:{other}")),
    }
}

fn read_only_policy() -> SandboxPolicy {
    SandboxPolicy {
        sandbox: None,
        permission_profile: PermissionProfile::Safe,
    }
}

fn workspace_write_policy() -> SandboxPolicy {
    SandboxPolicy {
        sandbox: Some("workspace-write".to_owned()),
        permission_profile: PermissionProfile::Balanced,
    }
}

fn string_option(options: &HashMap<String, Value>, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_token(value: &str) -> String {
    value.trim().replace('_', "-").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn isolated_policy_env() -> crate::test_support::ScopedEnv<'static> {
        let env =
            crate::test_support::scoped_env(&["KIANA_PERMISSION_PROFILE", "KIANA_PERMISSION_MODE"]);
        std::env::remove_var("KIANA_PERMISSION_PROFILE");
        std::env::remove_var("KIANA_PERMISSION_MODE");
        env
    }

    #[test]
    fn default_policy_is_read_only() {
        let _env = isolated_policy_env();
        let policy = sandbox_policy_from_options(&HashMap::new()).unwrap();
        assert_eq!(policy.sandbox, None);
        assert_eq!(policy.permission_profile, PermissionProfile::Safe);
    }

    #[test]
    fn workspace_write_requires_balanced_profile() {
        let options = HashMap::from([(
            "sandbox".to_string(),
            Value::String("workspace-write".to_string()),
        )]);
        let policy = sandbox_policy_from_options(&options).unwrap();
        assert_eq!(policy.sandbox.as_deref(), Some("workspace-write"));
        assert_eq!(policy.permission_profile, PermissionProfile::Balanced);
    }

    #[test]
    fn danger_full_access_is_rejected() {
        let options = HashMap::from([(
            "sandbox".to_string(),
            Value::String("danger-full-access".to_string()),
        )]);
        let error = sandbox_policy_from_options(&options)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "danger_full_access_rejected");
    }

    #[test]
    fn bypass_permissions_mode_is_rejected() {
        let options = HashMap::from([(
            "permission_mode".to_string(),
            Value::String("bypassPermissions".to_string()),
        )]);
        let error = sandbox_policy_from_options(&options)
            .unwrap_err()
            .to_string();
        assert_eq!(error, "danger_full_access_rejected");
    }

    #[test]
    fn transcript_includes_history_and_schema() {
        let prompt = prompt_from_session_messages(
            &[
                json!({"role":"assistant","content":[{"type":"text","text":"previous"}]}),
                json!({"role":"user","content":"follow up"}),
            ],
            Some(&json!({"type":"object"})),
        );
        assert!(prompt.contains("assistant: previous"));
        assert!(prompt.contains("user: follow up"));
        assert!(prompt.contains(r#""type": "object""#) || prompt.contains(r#""type":"object""#));
    }
}
