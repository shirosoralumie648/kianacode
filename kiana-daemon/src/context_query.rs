use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, RequestId};
use kiana_ports::PortError;
use kiana_query::{build_repo_map, RepoMap, RepoMapOptions};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Arc;

const REPO_MAP_OPERATION: &str = "context.repo_map";

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Query,
        REPO_MAP_OPERATION,
        Arc::new(RepoMapHandler),
    )
}

struct RepoMapHandler;

#[async_trait]
impl CapabilityHandler for RepoMapHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let request_id = request.request.request_id;
        let arguments = validated_arguments(&request.request.arguments)?;
        let project_root = PathBuf::from(arguments.project_root);
        let max_tokens = arguments.max_tokens;
        let output = arguments.output.to_owned();
        let map = tokio::task::spawn_blocking(move || {
            let project_root = project_root.canonicalize().map_err(|error| {
                PortError::Failed(format!("context_project_root_invalid:{error}"))
            })?;
            if !project_root.is_dir() {
                return Err(PortError::Failed(
                    "context_project_root_invalid:not_directory".to_owned(),
                ));
            }
            build_repo_map(project_root, RepoMapOptions { max_tokens })
                .map_err(|error| PortError::Failed(format!("context_repo_map_failed:{error}")))
        })
        .await
        .map_err(|error| PortError::Failed(format!("context_repo_map_join_failed:{error}")))??;
        command_result(request_id, &map, &output)
    }
}

struct RepoMapArguments<'a> {
    project_root: &'a str,
    output: &'a str,
    max_tokens: Option<u64>,
}

fn validated_arguments(arguments: &Value) -> Result<RepoMapArguments<'_>, PortError> {
    let arguments = arguments
        .as_object()
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:not_object".to_owned()))?;
    ensure_exact_keys(arguments, &["project_root", "output", "max_tokens"])?;
    let project_root = arguments
        .get("project_root")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:project_root".to_owned()))?;
    let output = arguments
        .get("output")
        .and_then(Value::as_str)
        .filter(|value| matches!(*value, "json" | "text"))
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:output".to_owned()))?;
    let max_tokens = match arguments.get("max_tokens") {
        Some(Value::Null) | None => None,
        Some(value) => {
            let value = value.as_u64().filter(|value| *value > 0).ok_or_else(|| {
                PortError::Failed("context_arguments_invalid:max_tokens".to_owned())
            })?;
            Some(value)
        }
    };
    Ok(RepoMapArguments {
        project_root,
        output,
        max_tokens,
    })
}

fn ensure_exact_keys(arguments: &Map<String, Value>, expected: &[&str]) -> Result<(), PortError> {
    if arguments.len() != expected.len()
        || arguments
            .keys()
            .any(|key| !expected.contains(&key.as_str()))
    {
        return Err(PortError::Failed(
            "context_arguments_invalid:keys".to_owned(),
        ));
    }
    Ok(())
}

fn command_result(
    request_id: RequestId,
    map: &RepoMap,
    output: &str,
) -> Result<CapabilityResult, PortError> {
    let value = if output == "json" {
        serde_json::to_string_pretty(map)
            .map_err(|error| PortError::Failed(format!("context_repo_map_serialize:{error}")))?
    } else {
        format_repo_map_text(map)
    };
    Ok(CapabilityResult::success(
        request_id,
        json!({
            "command_result": {
                "output_type": "text",
                "value": value,
                "metadata": null,
            }
        }),
    ))
}

fn format_repo_map_text(map: &RepoMap) -> String {
    let mut lines = vec![
        "Repo map".to_owned(),
        format!("root: {}", map.root),
        format!(
            "files: {} estimated_tokens: {}/{} truncated: {} omitted_files: {}",
            map.files.len(),
            map.estimated_tokens,
            map.token_budget,
            map.truncated,
            map.omitted_files
        ),
    ];
    for file in &map.files {
        lines.push(format!(
            "- {} [{}] bytes={} tokens={}",
            file.path,
            file.language.as_deref().unwrap_or("unknown"),
            file.bytes,
            file.estimated_tokens
        ));
        if !file.symbols.is_empty() {
            lines.push(format!("  symbols: {}", file.symbols.join(", ")));
        }
    }
    lines.join("\n")
}
