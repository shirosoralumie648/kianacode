//! Explicit human commands sent through the versioned daemon protocol.
use anyhow::{anyhow, Context, Result};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, PermissionProfile, RequestEnvelope, RequestMetadata, RoleSpec,
};
use std::path::PathBuf;

const USAGE: &str = "kiana command <name> --arguments '<JSON>' [--session-id ID] [--project-root DIR] [--role ROLE] [--permission-profile safe|balanced]\nkiana approvals|resume --session-id ID [--project-root DIR] [--role ROLE] [--permission-profile safe|balanced]\nkiana approval <ID> --decision approve|deny --request-hash HASH --nonce NONCE --session-id ID [--project-root DIR] [--role ROLE] [--permission-profile safe|balanced]";

pub async fn main_from_args(args: &[String]) -> Result<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("{USAGE}");
        return Ok(());
    }
    let mode = args.first().map(String::as_str).unwrap_or_default();
    let named = matches!(mode, "command" | "approval");
    let name = if named {
        Some(args.get(1).ok_or_else(|| anyhow!("{USAGE}"))?.clone())
    } else {
        None
    };
    let mut arguments = serde_json::json!({});
    let mut session = None;
    let mut root = std::env::current_dir().context("project_root_unavailable")?;
    let mut role = RoleSpec::builder();
    let mut profile = PermissionProfile::Safe;
    let (mut decision, mut request_hash, mut nonce) = (None, None, None);
    let mut index = if named { 2 } else { 1 };
    while index < args.len() {
        let (flag, value, consumed) = match args[index].split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, value, 1),
            _ => (
                args[index].as_str(),
                args.get(index + 1)
                    .ok_or_else(|| anyhow!("command_option_value_required:{}", args[index]))?
                    .as_str(),
                2,
            ),
        };
        match flag {
            "--arguments" => {
                arguments = serde_json::from_str(value).context("command_arguments_invalid")?
            }
            "--session-id" => session = Some(value.to_owned()),
            "--project-root" | "--workdir" => root = PathBuf::from(value),
            "--role" => role = RoleSpec::lookup(value).ok_or_else(|| anyhow!("role_unknown"))?,
            "--permission-profile" => {
                profile = match value {
                    "safe" => PermissionProfile::Safe,
                    "balanced" => PermissionProfile::Balanced,
                    _ => return Err(anyhow!("permission_profile_unsupported")),
                }
            }
            "--decision" => {
                decision = Some(match value {
                    "approve" => ApprovalDecision::Approve,
                    "deny" => ApprovalDecision::Deny,
                    _ => return Err(anyhow!("approval_decision_invalid")),
                })
            }
            "--request-hash" => request_hash = Some(value.to_owned()),
            "--nonce" => nonce = Some(value.to_owned()),
            _ => return Err(anyhow!("command_option_unknown:{flag}")),
        }
        index += consumed;
    }
    if !arguments.is_object() {
        return Err(anyhow!("command_arguments_object_required"));
    }
    if mode != "command" && session.as_deref().is_none_or(|id| id.trim().is_empty()) {
        return Err(anyhow!("session_id_required"));
    }
    let root = root.canonicalize().context("project_root_unavailable")?;
    let mut metadata = RequestMetadata::local(
        session.unwrap_or_else(|| kiana_protocol::RunId::new().to_string()),
        root.to_string_lossy().into_owned(),
    );
    metadata.assign_role(&role);
    metadata.permission_profile = profile;
    let request = match mode {
        "command" => RequestEnvelope::command(metadata, name.unwrap(), arguments),
        "approvals" => RequestEnvelope::pending_approvals(metadata, None),
        "resume" => RequestEnvelope::resume_run(metadata, None),
        "approval" => RequestEnvelope::approval_decision_with_proof(
            metadata,
            serde_json::from_value::<ApprovalId>(serde_json::Value::String(name.unwrap()))
                .context("approval_id_invalid")?,
            decision.ok_or_else(|| anyhow!("approval_decision_required"))?,
            request_hash,
            nonce,
        ),
        _ => return Err(anyhow!("{USAGE}")),
    };
    let host = DaemonHost::local().map_err(anyhow::Error::msg)?;
    let response = host.handle(request).await;
    println!("{}", serde_json::to_string_pretty(&response)?);
    if matches!(
        response.status,
        kiana_protocol::ExecutionStatus::Denied
            | kiana_protocol::ExecutionStatus::Blocked
            | kiana_protocol::ExecutionStatus::Failed
            | kiana_protocol::ExecutionStatus::ResultUnknown
    ) {
        return Err(anyhow!(
            "{}:{}",
            response.status.as_str(),
            response.error.unwrap_or_default()
        ));
    }
    Ok(())
}
