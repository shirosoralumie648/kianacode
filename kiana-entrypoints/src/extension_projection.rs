//! Shared extension visibility adapter for CLI, Workbench, Web and Desktop.
//!
//! Every surface calls this module with its `EntryPointKind`; the server returns one
//! `ExtensionVisibilitySnapshot` from `DaemonHost`.  This metadata-only module only
//! renders/forwards the projection and creates action intents. It never parses a package,
//! executes a Hook, or decides scope/approval locally.

use anyhow::{anyhow, Result};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    EntryPointKind, ExtensionCommandRequest, ExtensionVisibilityAction, ExtensionVisibilityActionKind,
    ExtensionVisibilitySnapshot, ResponseEnvelope,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn extension_visibility_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    surface: EntryPointKind,
    action: &str,
    query: Option<String>,
    extension_id: Option<String>,
    max_results: usize,
    options: &HashMap<String, Value>,
) -> Result<ExtensionVisibilitySnapshot> {
    let (client, mut metadata) = crate::harness_run::client_on_host(host, session_id, options)?;
    // Surface labels are telemetry only. The request still enters the shared DaemonHost and
    // ControlPlane route; no surface-specific policy or capability is selected here.
    metadata.entrypoint = Some(surface);
    client
        .extension_visibility(
            metadata,
            action.to_owned(),
            query,
            extension_id,
            Some(max_results),
        )
        .await
        .map_err(|error| anyhow!(error.to_string()))
}

/// Forward an extension lifecycle intent from any UI surface.  The adapter returns the shared
/// response/receipt projection and never parses packages, executes hooks or decides approval.
pub async fn extension_command_on_host(
    host: Arc<DaemonHost>,
    session_id: impl Into<String>,
    surface: EntryPointKind,
    request: ExtensionCommandRequest,
    options: &HashMap<String, Value>,
) -> Result<ResponseEnvelope> {
    let (client, mut metadata) = crate::harness_run::client_on_host(host, session_id, options)?;
    metadata.entrypoint = Some(surface);
    client
        .extension_command(metadata, request)
        .await
        .map_err(|error| anyhow!(error.to_string()))
}

pub fn visibility_action_intent(
    snapshot: &ExtensionVisibilitySnapshot,
    extension_id: &str,
    action: ExtensionVisibilityActionKind,
    reason: &str,
) -> Result<ExtensionVisibilityAction> {
    snapshot
        .action(extension_id, action, reason.to_owned())
        .map_err(|error| anyhow!(error))
}

/// Serialize a read-only projection for JSON/HTML/TTY presenters. The value contains no skill
/// body, package path, secret or internal command because those fields do not exist in the DTO.
pub fn visibility_json(snapshot: &ExtensionVisibilitySnapshot) -> Result<Value> {
    snapshot.validate().map_err(|error| anyhow!(error))?;
    serde_json::to_value(snapshot).map_err(|error| anyhow!(error))
}

pub fn visibility_action_json(action: &ExtensionVisibilityAction) -> Result<Value> {
    action.validate().map_err(|error| anyhow!(error))?;
    Ok(json!(action))
}

/// Minimal CLI presenter for the shared projection. Mutation is intentionally not performed by
/// this presenter; callers submit the returned action intent through the normal command route.
pub async fn cli_main_from_args(args: &[String], session_id: Option<&str>) -> Result<()> {
    let action = args.get(1).map(String::as_str).unwrap_or("list");
    if !matches!(action, "list" | "search" | "inspect") {
        return Err(anyhow!("extension_visibility_action_unsupported"));
    }
    let mut query = None;
    let mut extension_id = None;
    let mut json_output = false;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--json" => json_output = true,
            "--query" => {
                query = Some(
                    args.get(index + 1)
                        .ok_or_else(|| anyhow!("extension_visibility_query_required"))?
                        .clone(),
                );
                index += 1;
            }
            value if value.starts_with("--query=") => {
                query = Some(value.trim_start_matches("--query=").to_owned());
            }
            "--id" => {
                extension_id = Some(
                    args.get(index + 1)
                        .ok_or_else(|| anyhow!("extension_visibility_id_required"))?
                        .clone(),
                );
                index += 1;
            }
            value if value.starts_with("--id=") => {
                extension_id = Some(value.trim_start_matches("--id=").to_owned());
            }
            value => return Err(anyhow!("unknown extension option: {value}")),
        }
        index += 1;
    }
    let mut options = HashMap::new();
    options.insert(
        "cwd".to_owned(),
        Value::String(std::env::current_dir()?.display().to_string()),
    );
    let host = crate::harness_run::new_local_host_with_options(&options)?;
    let snapshot = extension_visibility_on_host(
        host,
        session_id.unwrap_or("extension-cli").to_owned(),
        EntryPointKind::Cli,
        action,
        query,
        extension_id,
        128,
        &options,
    )
    .await?;
    if json_output {
        println!("{}", serde_json::to_string_pretty(&visibility_json(&snapshot)?)?);
    } else {
        for entry in &snapshot.entries {
            println!("{}\t{:?}\t{:?}", entry.extension_id, entry.status, entry.risk);
        }
    }
    Ok(())
}
