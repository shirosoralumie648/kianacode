use super::*;

impl ControlPlane {
    pub(crate) async fn handle_context_query(
        &self,
        context: RequestContext,
        arguments: Value,
    ) -> Result<CoreResponse, CoreError> {
        let request_id = context.request_id;
        let normalized = match normalize_context_query_arguments(&context, &arguments) {
            Ok(normalized) => normalized,
            Err(error) => {
                let reason = error.reason();
                self.append_event(
                    request_id,
                    1,
                    "request.accepted",
                    json!({ "command": CONTEXT_QUERY_COMMAND }),
                )
                .await?;
                self.append_event(
                    request_id,
                    2,
                    "command.rejected",
                    json!({ "reason": reason }),
                )
                .await?;
                return Ok(CoreResponse::blocked(request_id, reason));
            }
        };
        let request = CapabilityRequest::new(
            request_id,
            CapabilityKind::Query,
            normalized.operation,
            normalized.arguments,
        )
        .with_risk(normalized.risk);
        self.authorize_and_execute(&context, request).await
    }
}

fn normalize_context_query_arguments(
    context: &RequestContext,
    arguments: &Value,
) -> Result<NormalizedContextQuery, ContextQueryIntentError> {
    let arguments = arguments
        .as_object()
        .ok_or(ContextQueryIntentError::Invalid)?;
    if arguments.keys().any(|key| {
        !matches!(
            key.as_str(),
            "operation" | "output" | "options" | "project_root"
        )
    }) {
        return Err(ContextQueryIntentError::Invalid);
    }
    let operation = arguments
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    let output = arguments
        .get("output")
        .and_then(Value::as_str)
        .ok_or(ContextQueryIntentError::Invalid)?;
    if !matches!(output, "json" | "text") {
        return Err(ContextQueryIntentError::Invalid);
    }
    let options = arguments
        .get("options")
        .and_then(Value::as_object)
        .ok_or(ContextQueryIntentError::Invalid)?;

    let (broker_operation, risk, normalized) = match operation {
        "repo_map" => {
            ensure_option_keys(options, &["max_tokens"])?;
            let max_tokens = optional_bounded_u64(options, "max_tokens", u64::MAX)?;
            (
                CONTEXT_REPO_MAP_OPERATION,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "output": output,
                    "max_tokens": max_tokens,
                }),
            )
        }
        "artifact_graph" | "artifact_readiness" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = if operation == "artifact_graph" {
                CONTEXT_ARTIFACT_GRAPH_OPERATION
            } else {
                CONTEXT_ARTIFACT_READINESS_OPERATION
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "index" | "artifacts" | "artifact_store" => {
            ensure_option_keys(options, &["root", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index" => CONTEXT_INDEX_OPERATION,
                "artifacts" => CONTEXT_ARTIFACTS_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::ReadOnly,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "index_cache_write" | "artifacts_cache_write" | "artifact_store_cache_write" => {
            ensure_option_keys(options, &["root", "cache", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let cache = required_relative_path(options, "cache")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let broker_operation = match operation {
                "index_cache_write" => CONTEXT_INDEX_CACHE_OPERATION,
                "artifacts_cache_write" => CONTEXT_ARTIFACTS_CACHE_OPERATION,
                _ => CONTEXT_ARTIFACT_STORE_CACHE_OPERATION,
            };
            (
                broker_operation,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "cache": cache,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "artifact_ingest_write" => {
            ensure_option_keys(options, &["root", "source", "store", "max_bytes_per_file"])?;
            let root = optional_relative_path(options, "root")?;
            let source = required_relative_path(options, "source")?;
            let store = optional_relative_path(options, "store")?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            (
                CONTEXT_ARTIFACT_INGEST_OPERATION,
                RiskLevel::LocalWrite,
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "source": source,
                    "store": store,
                    "output": output,
                    "max_bytes_per_file": max_bytes_per_file,
                }),
            )
        }
        "search" | "vector_search" | "pack" => {
            let allowed = if operation == "pack" {
                &[
                    "query",
                    "root",
                    "limit",
                    "max_bytes_per_file",
                    "max_snippet_lines",
                ][..]
            } else {
                &["query", "root", "limit", "max_bytes_per_file"][..]
            };
            ensure_option_keys(options, allowed)?;
            let query = options
                .get("query")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|query| !query.is_empty())
                .ok_or(ContextQueryIntentError::Invalid)?;
            let root = optional_relative_path(options, "root")?;
            let limit = optional_bounded_u64(options, "limit", MAX_CONTEXT_LIMIT)?;
            let max_bytes_per_file =
                optional_bounded_u64(options, "max_bytes_per_file", MAX_CONTEXT_BYTES_PER_FILE)?;
            let max_snippet_lines = if operation == "pack" {
                optional_bounded_u64(options, "max_snippet_lines", MAX_CONTEXT_SNIPPET_LINES)?
            } else {
                None
            };
            let broker_operation = match operation {
                "search" => CONTEXT_SEARCH_OPERATION,
                "vector_search" => CONTEXT_VECTOR_SEARCH_OPERATION,
                _ => CONTEXT_PACK_OPERATION,
            };
            let normalized = if operation == "pack" {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                    "max_snippet_lines": max_snippet_lines,
                })
            } else {
                json!({
                    "project_root": context.project_root,
                    "root": root,
                    "output": output,
                    "query": query,
                    "limit": limit,
                    "max_bytes_per_file": max_bytes_per_file,
                })
            };
            (broker_operation, RiskLevel::ReadOnly, normalized)
        }
        _ => return Err(ContextQueryIntentError::Unregistered),
    };

    Ok(NormalizedContextQuery {
        operation: broker_operation,
        risk,
        arguments: normalized,
    })
}

fn ensure_option_keys(
    options: &serde_json::Map<String, Value>,
    allowed: &[&str],
) -> Result<(), ContextQueryIntentError> {
    if options.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

fn optional_bounded_u64(
    options: &serde_json::Map<String, Value>,
    key: &str,
    maximum: u64,
) -> Result<Option<u64>, ContextQueryIntentError> {
    match options.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0 && *value <= maximum)
            .map(Some)
            .ok_or(ContextQueryIntentError::Invalid),
    }
}

fn optional_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, ContextQueryIntentError> {
    let Some(value) = options.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(Some(value.to_owned()))
}

fn required_relative_path(
    options: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<String, ContextQueryIntentError> {
    let value = options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ContextQueryIntentError::Invalid)?;
    validate_relative_path(value)?;
    Ok(value.to_owned())
}

fn validate_relative_path(value: &str) -> Result<(), ContextQueryIntentError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ContextQueryIntentError::Invalid);
    }
    Ok(())
}

struct NormalizedContextQuery {
    operation: &'static str,
    risk: RiskLevel,
    arguments: Value,
}

enum ContextQueryIntentError {
    Invalid,
    Unregistered,
}

impl ContextQueryIntentError {
    fn reason(&self) -> &'static str {
        match self {
            Self::Invalid => "command_arguments_invalid",
            Self::Unregistered => "command_unregistered",
        }
    }
}
