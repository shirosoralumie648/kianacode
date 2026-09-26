//! Narrow Company CLI/Workbench user flow adapters.
//!
//! These parsers turn friendly actions into the existing versioned RequestEnvelope. They do not
//! apply Company state, approve future work or start a second execution loop.

use anyhow::{anyhow, Context, Result};
use kiana_daemon::DaemonHost;
use kiana_protocol::{PermissionProfile, RequestEnvelope, RequestMetadata, RoleSpec};
use serde_json::{json, Value};
use std::path::PathBuf;

pub const COMPANY_FLOW_USAGE: &str = "kiana company inspect|next --project ID [--session-id ID]\nkiana company inbox [--session-id ID]\nkiana company decide --card ID --option OPTION --decision-ref REF --target-revision N --target-digest DIGEST --scope-digest DIGEST [--session-id ID]\nkiana company delivery|close --arguments JSON [--session-id ID]";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompanyFlowKind {
    Inspect,
    Next,
    Inbox,
    Decide,
    Delivery,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompanyFlowRequest {
    pub kind: CompanyFlowKind,
    pub project_id: Option<String>,
    pub card_id: Option<String>,
    pub option: Option<String>,
    pub decision_ref: Option<String>,
    pub target_revision: Option<u64>,
    pub target_digest: Option<String>,
    pub scope_digest: Option<String>,
    pub arguments: Value,
    pub session_id: Option<String>,
    pub project_root: PathBuf,
    pub role: String,
    pub permission_profile: PermissionProfile,
}

fn required(value: Option<String>, field: &str) -> Result<String, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("{field}_required"))
}

fn take_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let value = args
        .get(*index + 1)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .ok_or_else(|| format!("{flag}_value_required"))?;
    *index += 2;
    Ok(value)
}

pub fn parse_args(args: &[String]) -> Result<CompanyFlowRequest, String> {
    if args.first().map(String::as_str) != Some("company") {
        return Err("company_command_required".to_owned());
    }
    let kind = match args.get(1).map(String::as_str) {
        Some("inspect") => CompanyFlowKind::Inspect,
        Some("next") => CompanyFlowKind::Next,
        Some("inbox") => CompanyFlowKind::Inbox,
        Some("decide") => CompanyFlowKind::Decide,
        Some("delivery") => CompanyFlowKind::Delivery,
        Some("close") => CompanyFlowKind::Close,
        _ => return Err(COMPANY_FLOW_USAGE.to_owned()),
    };
    let mut request = CompanyFlowRequest {
        kind,
        project_id: None,
        card_id: None,
        option: None,
        decision_ref: None,
        target_revision: None,
        target_digest: None,
        scope_digest: None,
        arguments: serde_json::json!({}),
        session_id: None,
        project_root: std::env::current_dir().map_err(|_| "project_root_unavailable".to_owned())?,
        role: "builder".to_owned(),
        permission_profile: PermissionProfile::Safe,
    };
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--project" | "--project-id" => {
                request.project_id = Some(take_value(args, &mut index, "--project")?);
            }
            "--card" | "--card-id" => {
                request.card_id = Some(take_value(args, &mut index, "--card")?);
            }
            "--option" => request.option = Some(take_value(args, &mut index, "--option")?),
            "--decision-ref" => {
                request.decision_ref = Some(take_value(args, &mut index, "--decision-ref")?);
            }
            "--target-revision" => {
                request.target_revision = Some(
                    take_value(args, &mut index, "--target-revision")?
                        .parse()
                        .map_err(|_| "target_revision_invalid".to_owned())?,
                );
            }
            "--target-digest" => {
                request.target_digest = Some(take_value(args, &mut index, "--target-digest")?);
            }
            "--scope-digest" => {
                request.scope_digest = Some(take_value(args, &mut index, "--scope-digest")?);
            }
            "--arguments" => {
                let raw = take_value(args, &mut index, "--arguments")?;
                request.arguments = serde_json::from_str(&raw)
                    .map_err(|_| "company_arguments_invalid".to_owned())?;
                if !request.arguments.is_object() {
                    return Err("company_arguments_object_required".to_owned());
                }
            }
            "--session-id" => {
                request.session_id = Some(take_value(args, &mut index, "--session-id")?);
            }
            "--project-root" | "--workdir" => {
                request.project_root = PathBuf::from(take_value(args, &mut index, "--workdir")?);
            }
            "--role" => request.role = take_value(args, &mut index, "--role")?,
            "--permission-profile" => {
                request.permission_profile =
                    match take_value(args, &mut index, "--permission-profile")?.as_str() {
                        "safe" => PermissionProfile::Safe,
                        "balanced" => PermissionProfile::Balanced,
                        _ => return Err("permission_profile_unsupported".to_owned()),
                    };
            }
            "--help" | "-h" => return Err(COMPANY_FLOW_USAGE.to_owned()),
            unknown => return Err(format!("company_option_unknown:{unknown}")),
        }
    }
    match kind {
        CompanyFlowKind::Inspect | CompanyFlowKind::Next => {
            request.project_id = Some(required(request.project_id, "project")?);
        }
        CompanyFlowKind::Decide => {
            request.card_id = Some(required(request.card_id, "card")?);
            request.option = Some(required(request.option, "option")?);
            request.decision_ref = Some(required(request.decision_ref, "decision_ref")?);
            if request.target_revision.is_none() {
                return Err("target_revision_required".to_owned());
            }
            request.target_digest = Some(required(request.target_digest, "target_digest")?);
            request.scope_digest = Some(required(request.scope_digest, "scope_digest")?);
        }
        CompanyFlowKind::Delivery | CompanyFlowKind::Close
            if !request.arguments.is_object()
                || request
                    .arguments
                    .as_object()
                    .is_none_or(|object| object.is_empty()) =>
        {
            return Err("company_arguments_required".to_owned());
        }
        CompanyFlowKind::Inbox | CompanyFlowKind::Delivery | CompanyFlowKind::Close => {}
    }
    Ok(request)
}

/// Shared Workbench command parser. It returns a normal daemon command name and JSON arguments.
pub fn workbench_command(raw: &str) -> Result<(String, Value), String> {
    let mut parts = raw.splitn(2, char::is_whitespace);
    let action = parts.next().unwrap_or_default();
    let rest = parts.next().unwrap_or_default().trim();
    match action {
        "inspect" | "next" => {
            let project_id = required(Some(rest.to_owned()), "project")?;
            Ok((
                "company.governance.v1".to_owned(),
                serde_json::json!({"project_id": project_id}),
            ))
        }
        "inbox" => Ok(("human.inbox".to_owned(), serde_json::json!({}))),
        "decide" => {
            let arguments: Value = serde_json::from_str(rest)
                .map_err(|_| "company_decide_arguments_invalid".to_owned())?;
            if !arguments.is_object() {
                return Err("company_decide_arguments_object_required".to_owned());
            }
            Ok(("human.resolve".to_owned(), arguments))
        }
        "delivery" => Ok((
            "company.business".to_owned(),
            serde_json::from_str(rest)
                .map_err(|_| "company_delivery_arguments_invalid".to_owned())?,
        )),
        "close" => Ok((
            "company.business".to_owned(),
            serde_json::from_str(rest).map_err(|_| "company_close_arguments_invalid".to_owned())?,
        )),
        _ => Err("company_workbench_action_unknown".to_owned()),
    }
}

pub async fn main_from_args(args: &[String]) -> Result<()> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        println!("{COMPANY_FLOW_USAGE}");
        return Ok(());
    }
    let request = parse_args(args).map_err(|error| anyhow!("{error}"))?;
    let session_id =
        required(request.session_id, "session_id").map_err(|error| anyhow!("{error}"))?;
    let root = request
        .project_root
        .canonicalize()
        .context("project_root_unavailable")?;
    let role = RoleSpec::lookup(&request.role).ok_or_else(|| anyhow!("role_unknown"))?;
    let mut metadata = RequestMetadata::local(session_id, root.to_string_lossy().into_owned());
    metadata.assign_role(&role);
    metadata.permission_profile = request.permission_profile;
    let envelope = match request.kind {
        CompanyFlowKind::Inspect | CompanyFlowKind::Next => RequestEnvelope::company_governance(
            metadata,
            request.project_id.expect("validated project"),
        ),
        CompanyFlowKind::Inbox => RequestEnvelope::command(metadata, "human.inbox", json!({})),
        CompanyFlowKind::Decide => RequestEnvelope::command(
            metadata,
            "human.resolve",
            json!({
                "card_id": request.card_id,
                "option": request.option,
                "decision_ref": request.decision_ref,
                "target_revision": request.target_revision,
                "target_digest": request.target_digest,
                "scope_digest": request.scope_digest,
            }),
        ),
        CompanyFlowKind::Delivery | CompanyFlowKind::Close => {
            RequestEnvelope::command(metadata, "company.business", request.arguments)
        }
    };
    let host = DaemonHost::local().map_err(anyhow::Error::msg)?;
    let response = host.handle(envelope).await;
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
