use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, RequestId};
use kiana_ports::PortError;
use kiana_query::{
    build_context_artifact_dependency_graph, build_context_artifact_readiness, build_context_pack,
    build_repo_map, search_context_index, search_context_vectors, ContextArtifactDependencyGraph,
    ContextArtifactOptions, ContextArtifactReadiness, ContextPack, ContextPackOptions,
    ContextSearchOptions, ContextSearchResults, ContextVectorSearchOptions,
    ContextVectorSearchResults, RepoMap, RepoMapOptions,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const REPO_MAP_OPERATION: &str = "context.repo_map";
const ARTIFACT_GRAPH_OPERATION: &str = "context.artifact_graph.read";
const ARTIFACT_READINESS_OPERATION: &str = "context.artifact_readiness.read";
const SEARCH_OPERATION: &str = "context.search";
const VECTOR_SEARCH_OPERATION: &str = "context.vector_search";
const PACK_OPERATION: &str = "context.pack";

pub(crate) fn register(broker: &mut CapabilityBroker) -> Result<(), PortError> {
    broker.register_static(
        CapabilityKind::Query,
        REPO_MAP_OPERATION,
        Arc::new(RepoMapHandler),
    )?;
    for operation in [
        ReadQueryOperation::ArtifactGraph,
        ReadQueryOperation::ArtifactReadiness,
        ReadQueryOperation::Search,
        ReadQueryOperation::VectorSearch,
        ReadQueryOperation::Pack,
    ] {
        broker.register_static(
            CapabilityKind::Query,
            operation.broker_key(),
            Arc::new(ReadQueryHandler { operation }),
        )?;
    }
    Ok(())
}

struct RepoMapHandler;

#[async_trait]
impl CapabilityHandler for RepoMapHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, REPO_MAP_OPERATION)?;
        let request_id = request.request.request_id;
        let arguments = validated_repo_map_arguments(&request.request.arguments)?;
        let output = arguments.output.to_owned();
        let project_root = arguments.project_root.to_owned();
        let max_tokens = arguments.max_tokens;
        let map = tokio::task::spawn_blocking(move || {
            let project_root = canonical_project_root(&project_root)?;
            build_repo_map(project_root, RepoMapOptions { max_tokens })
                .map_err(|error| PortError::Failed(format!("context_repo_map_failed:{error}")))
        })
        .await
        .map_err(|error| PortError::Failed(format!("context_repo_map_join_failed:{error}")))??;
        command_result(request_id, &map, &output, "repo_map", format_repo_map_text)
    }
}

#[derive(Clone, Copy)]
enum ReadQueryOperation {
    ArtifactGraph,
    ArtifactReadiness,
    Search,
    VectorSearch,
    Pack,
}

impl ReadQueryOperation {
    fn broker_key(self) -> &'static str {
        match self {
            Self::ArtifactGraph => ARTIFACT_GRAPH_OPERATION,
            Self::ArtifactReadiness => ARTIFACT_READINESS_OPERATION,
            Self::Search => SEARCH_OPERATION,
            Self::VectorSearch => VECTOR_SEARCH_OPERATION,
            Self::Pack => PACK_OPERATION,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ArtifactGraph => "artifact_graph",
            Self::ArtifactReadiness => "artifact_readiness",
            Self::Search => "search",
            Self::VectorSearch => "vector_search",
            Self::Pack => "pack",
        }
    }
}

struct ReadQueryHandler {
    operation: ReadQueryOperation,
}

#[async_trait]
impl CapabilityHandler for ReadQueryHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        ensure_operation(&request, self.operation.broker_key())?;
        let request_id = request.request.request_id;
        let arguments = validated_read_arguments(self.operation, &request.request.arguments)?;
        let operation = self.operation;
        let value = tokio::task::spawn_blocking(move || execute_read_query(operation, arguments))
            .await
            .map_err(|error| {
                PortError::Failed(format!("context_{}_join_failed:{error}", operation.label()))
            })??;
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
}

fn execute_read_query(
    operation: ReadQueryOperation,
    arguments: ReadQueryArguments,
) -> Result<String, PortError> {
    let root = confined_context_root(&arguments.project_root, arguments.root.as_deref())?;
    match operation {
        ReadQueryOperation::ArtifactGraph => {
            let graph = build_context_artifact_dependency_graph(
                root,
                ContextArtifactOptions {
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_artifact_graph_failed:{error}")))?;
            render_output(
                &graph,
                &arguments.output,
                "artifact_graph",
                format_context_artifact_graph_text,
            )
        }
        ReadQueryOperation::ArtifactReadiness => {
            let readiness = build_context_artifact_readiness(
                root,
                ContextArtifactOptions {
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| {
                PortError::Failed(format!("context_artifact_readiness_failed:{error}"))
            })?;
            render_output(
                &readiness,
                &arguments.output,
                "artifact_readiness",
                format_context_artifact_readiness_text,
            )
        }
        ReadQueryOperation::Search => {
            let results = search_context_index(
                root,
                required_query(&arguments)?,
                ContextSearchOptions {
                    limit: arguments.limit,
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_search_failed:{error}")))?;
            render_output(
                &results,
                &arguments.output,
                "search",
                format_context_search_text,
            )
        }
        ReadQueryOperation::VectorSearch => {
            let results = search_context_vectors(
                root,
                required_query(&arguments)?,
                ContextVectorSearchOptions {
                    limit: arguments.limit,
                    max_bytes_per_file: arguments.max_bytes_per_file,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_vector_search_failed:{error}")))?;
            render_output(
                &results,
                &arguments.output,
                "vector_search",
                format_context_vector_search_text,
            )
        }
        ReadQueryOperation::Pack => {
            let pack = build_context_pack(
                root,
                required_query(&arguments)?,
                ContextPackOptions {
                    limit: arguments.limit,
                    max_bytes_per_file: arguments.max_bytes_per_file,
                    max_snippet_lines: arguments.max_snippet_lines,
                },
            )
            .map_err(|error| PortError::Failed(format!("context_pack_failed:{error}")))?;
            render_output(&pack, &arguments.output, "pack", format_context_pack_text)
        }
    }
}

fn required_query(arguments: &ReadQueryArguments) -> Result<&str, PortError> {
    arguments
        .query
        .as_deref()
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:query".to_owned()))
}

struct RepoMapArguments<'a> {
    project_root: &'a str,
    output: &'a str,
    max_tokens: Option<u64>,
}

fn validated_repo_map_arguments(arguments: &Value) -> Result<RepoMapArguments<'_>, PortError> {
    let arguments = arguments_object(arguments)?;
    ensure_exact_keys(arguments, &["project_root", "output", "max_tokens"])?;
    Ok(RepoMapArguments {
        project_root: required_string(arguments, "project_root")?,
        output: required_output(arguments)?,
        max_tokens: optional_u64(arguments, "max_tokens")?,
    })
}

struct ReadQueryArguments {
    project_root: String,
    root: Option<String>,
    output: String,
    query: Option<String>,
    limit: Option<usize>,
    max_bytes_per_file: Option<usize>,
    max_snippet_lines: Option<usize>,
}

fn validated_read_arguments(
    operation: ReadQueryOperation,
    arguments: &Value,
) -> Result<ReadQueryArguments, PortError> {
    let arguments = arguments_object(arguments)?;
    let expected = match operation {
        ReadQueryOperation::ArtifactGraph | ReadQueryOperation::ArtifactReadiness => {
            &["project_root", "root", "output", "max_bytes_per_file"][..]
        }
        ReadQueryOperation::Search | ReadQueryOperation::VectorSearch => &[
            "project_root",
            "root",
            "output",
            "query",
            "limit",
            "max_bytes_per_file",
        ][..],
        ReadQueryOperation::Pack => &[
            "project_root",
            "root",
            "output",
            "query",
            "limit",
            "max_bytes_per_file",
            "max_snippet_lines",
        ][..],
    };
    ensure_exact_keys(arguments, expected)?;
    let query = match operation {
        ReadQueryOperation::Search
        | ReadQueryOperation::VectorSearch
        | ReadQueryOperation::Pack => Some(required_string(arguments, "query")?.to_owned()),
        _ => None,
    };
    Ok(ReadQueryArguments {
        project_root: required_string(arguments, "project_root")?.to_owned(),
        root: optional_string(arguments, "root")?,
        output: required_output(arguments)?.to_owned(),
        query,
        limit: optional_usize(arguments, "limit")?,
        max_bytes_per_file: optional_usize(arguments, "max_bytes_per_file")?,
        max_snippet_lines: optional_usize(arguments, "max_snippet_lines")?,
    })
}

fn arguments_object(arguments: &Value) -> Result<&Map<String, Value>, PortError> {
    arguments
        .as_object()
        .ok_or_else(|| PortError::Failed("context_arguments_invalid:not_object".to_owned()))
}

fn required_string<'a>(arguments: &'a Map<String, Value>, key: &str) -> Result<&'a str, PortError> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| PortError::Failed(format!("context_arguments_invalid:{key}")))
}

fn optional_string(arguments: &Map<String, Value>, key: &str) -> Result<Option<String>, PortError> {
    match arguments.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(_) => required_string(arguments, key).map(|value| Some(value.to_owned())),
    }
}

fn required_output(arguments: &Map<String, Value>) -> Result<&str, PortError> {
    required_string(arguments, "output").and_then(|output| {
        if matches!(output, "json" | "text") {
            Ok(output)
        } else {
            Err(PortError::Failed(
                "context_arguments_invalid:output".to_owned(),
            ))
        }
    })
}

fn optional_u64(arguments: &Map<String, Value>, key: &str) -> Result<Option<u64>, PortError> {
    match arguments.get(key) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(|| PortError::Failed(format!("context_arguments_invalid:{key}"))),
    }
}

fn optional_usize(arguments: &Map<String, Value>, key: &str) -> Result<Option<usize>, PortError> {
    optional_u64(arguments, key)?
        .map(|value| {
            usize::try_from(value)
                .map_err(|_| PortError::Failed(format!("context_arguments_invalid:{key}")))
        })
        .transpose()
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

fn ensure_operation(
    request: &AuthorizedCapabilityRequest,
    expected: &str,
) -> Result<(), PortError> {
    if request.request.operation != expected {
        return Err(PortError::Failed("context_operation_mismatch".to_owned()));
    }
    Ok(())
}

fn canonical_project_root(project_root: &str) -> Result<PathBuf, PortError> {
    let project_root = PathBuf::from(project_root)
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("context_project_root_invalid:{error}")))?;
    if !project_root.is_dir() {
        return Err(PortError::Failed(
            "context_project_root_invalid:not_directory".to_owned(),
        ));
    }
    Ok(project_root)
}

fn confined_context_root(
    project_root: &str,
    requested_root: Option<&str>,
) -> Result<PathBuf, PortError> {
    let project_root = canonical_project_root(project_root)?;
    let Some(requested_root) = requested_root else {
        return Ok(project_root);
    };
    let requested_root = Path::new(requested_root);
    if requested_root.is_absolute()
        || requested_root.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PortError::Failed(
            "context_root_invalid:not_relative".to_owned(),
        ));
    }
    let root = project_root
        .join(requested_root)
        .canonicalize()
        .map_err(|error| PortError::Failed(format!("context_root_invalid:{error}")))?;
    if !root.starts_with(&project_root) {
        return Err(PortError::Failed("context_root_outside_project".to_owned()));
    }
    if !root.is_dir() {
        return Err(PortError::Failed(
            "context_root_invalid:not_directory".to_owned(),
        ));
    }
    Ok(root)
}

fn command_result<T: Serialize>(
    request_id: RequestId,
    value: &T,
    output: &str,
    label: &str,
    text_formatter: impl FnOnce(&T) -> String,
) -> Result<CapabilityResult, PortError> {
    let value = render_output(value, output, label, text_formatter)?;
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

fn render_output<T: Serialize>(
    value: &T,
    output: &str,
    label: &str,
    text_formatter: impl FnOnce(&T) -> String,
) -> Result<String, PortError> {
    if output == "json" {
        serde_json::to_string_pretty(value)
            .map_err(|error| PortError::Failed(format!("context_{label}_serialize:{error}")))
    } else {
        Ok(text_formatter(value))
    }
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

fn format_context_artifact_graph_text(graph: &ContextArtifactDependencyGraph) -> String {
    let mut lines = vec![
        "Context artifact graph".to_string(),
        format!("root: {}", graph.root),
        format!(
            "schema: {} nodes={} edges={}",
            graph.schema,
            graph.nodes.len(),
            graph.edges.len()
        ),
    ];
    for node in &graph.nodes {
        lines.push(format!(
            "- {} [{}] kind={} hash={} id={}",
            node.path,
            node.language.as_deref().unwrap_or("unknown"),
            node.kind,
            node.content_hash,
            node.id
        ));
    }
    for edge in &graph.edges {
        lines.push(format!(
            "  edge: {} -> {} relation={} evidence={}",
            edge.source, edge.target, edge.relation, edge.evidence
        ));
    }
    lines.join("\n")
}

fn format_context_artifact_readiness_text(readiness: &ContextArtifactReadiness) -> String {
    let mut lines = vec![
        "Context artifact readiness".to_string(),
        format!("root: {}", readiness.root),
        format!(
            "schema: {} status={} artifacts={} dependencies={}",
            readiness.schema,
            readiness.status,
            readiness.artifact_count,
            readiness.dependency_count
        ),
        format!("artifact_store_schema: {}", readiness.artifact_store_schema),
        format!(
            "missing_roles: {}",
            if readiness.missing_roles.is_empty() {
                "none".to_string()
            } else {
                readiness.missing_roles.join(",")
            }
        ),
    ];
    for role in &readiness.required_roles {
        lines.push(format!(
            "- {} required={} present={} count={}",
            role.role, role.required, role.present, role.count
        ));
    }
    lines.join("\n")
}

fn format_context_search_text(results: &ContextSearchResults) -> String {
    let mut lines = vec![
        "Context search".to_string(),
        format!("root: {}", results.root),
        format!(
            "query: {} terms={} files_indexed={} skipped_files={}",
            results.query,
            results.terms.join(","),
            results.files_indexed,
            results.skipped_files
        ),
    ];
    for hit in &results.hits {
        lines.push(format!(
            "- {}:{} score={} occurrences={} terms={}",
            hit.path,
            hit.line_number,
            hit.score,
            hit.occurrences,
            hit.matched_terms.join(",")
        ));
        if !hit.line.is_empty() {
            lines.push(format!("  {}", hit.line));
        }
    }
    lines.join("\n")
}

fn format_context_vector_search_text(results: &ContextVectorSearchResults) -> String {
    let mut lines = vec![
        "Context vector search".to_string(),
        format!("root: {}", results.root),
        format!(
            "query: {} terms={} model={} dimensions={} files_indexed={} skipped_files={}",
            results.query,
            results.terms.join(","),
            results.embedding_model,
            results.dimensions,
            results.files_indexed,
            results.skipped_files
        ),
    ];
    for hit in &results.hits {
        lines.push(format!(
            "- {}:{} score={:.6} token_overlap={} hash={}",
            hit.path, hit.line_number, hit.score, hit.token_overlap, hit.content_hash
        ));
        if !hit.line.is_empty() {
            lines.push(format!("  {}", hit.line));
        }
    }
    lines.join("\n")
}

fn format_context_pack_text(pack: &ContextPack) -> String {
    let mut lines = vec![
        "Context pack".to_string(),
        format!("root: {}", pack.root),
        format!(
            "query: {} terms={} files_indexed={} skipped_files={} snippets={}",
            pack.query,
            pack.terms.join(","),
            pack.files_indexed,
            pack.skipped_files,
            pack.snippets.len()
        ),
        format!(
            "artifact_graph: schema={} nodes={} edges={}",
            pack.artifact_graph.schema,
            pack.artifact_graph.nodes.len(),
            pack.artifact_graph.edges.len()
        ),
    ];
    for snippet in &pack.snippets {
        lines.push(format!(
            "- {}:{}-{} score={} occurrences={} terms={} hash={}",
            snippet.path,
            snippet.start_line,
            snippet.end_line,
            snippet.score,
            snippet.occurrences,
            snippet.matched_terms.join(","),
            snippet.content_hash
        ));
        if !snippet.excerpt.is_empty() {
            lines.push(snippet.excerpt.clone());
        }
    }
    for edge in &pack.artifact_graph.edges {
        lines.push(format!(
            "  graph: {} -> {} relation={} terms={}",
            edge.source,
            edge.target,
            edge.relation,
            edge.matched_terms.join(",")
        ));
    }
    lines.join("\n")
}
