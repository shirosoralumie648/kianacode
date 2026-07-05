use crate::local_state::{app_state_array_len, app_state_keys};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use kiana_query::{
    build_context_artifact_dependency_graph, build_context_artifact_store, build_context_artifacts,
    build_context_index, build_context_pack, build_persistent_context_artifact_store,
    build_persistent_context_artifacts, build_persistent_context_index, build_repo_map,
    search_context_index, ContextArtifactDependencyGraph, ContextArtifactOptions,
    ContextArtifactStore, ContextArtifacts, ContextIndex, ContextIndexOptions, ContextPack,
    ContextPackOptions, ContextSearchOptions, ContextSearchResults, RepoMap, RepoMapOptions,
};
use serde_json::Value;
use std::path::PathBuf;

pub struct ContextCommand;

#[async_trait]
impl Command for ContextCommand {
    fn name(&self) -> &str {
        "context"
    }

    fn description(&self) -> &str {
        "Manage context"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = context.args.trim();
        if let Some(rest) = args.strip_prefix("repo-map") {
            return repo_map_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("index") {
            return index_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifacts") {
            return artifacts_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-store") {
            return artifact_store_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-graph") {
            return artifact_graph_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("search") {
            return search_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("pack") {
            return pack_result(&context, rest.trim());
        }

        match args {
            "" | "status" => {}
            "json" => {
                return Ok(CommandResult::text(serde_json::to_string_pretty(
                    &context.app_state,
                )?));
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }

        let keys = app_state_keys(&context.app_state);
        let key_text = if keys.is_empty() {
            "none".to_string()
        } else {
            keys.join(", ")
        };
        Ok(CommandResult::text(format!(
            "Context status\nkeys: {}\ntasks: {}\nteams: {}\ntodos: {}\nmcp_invocations: {}\nusage: kiana context json",
            key_text,
            app_state_array_len(&context.app_state, "tasks"),
            app_state_array_len(&context.app_state, "teams"),
            app_state_array_len(&context.app_state, "todos"),
            app_state_array_len(&context.app_state, "mcp_invocations")
        )))
    }
}

fn usage() -> &'static str {
    "Usage: kiana context [status|json|repo-map [--json] [--max-tokens N]|index [--json] [--root DIR] [--cache PATH] [--max-bytes-per-file N]|artifacts [--json] [--root DIR] [--cache PATH] [--max-bytes-per-file N]|artifact-store [--json] [--root DIR] [--cache PATH] [--max-bytes-per-file N]|artifact-graph [--json] [--root DIR] [--max-bytes-per-file N]|search <query> [--json] [--root DIR] [--limit N] [--max-bytes-per-file N]|pack <query> [--json] [--root DIR] [--limit N] [--max-snippet-lines N] [--max-bytes-per-file N]]"
}

fn repo_map_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut max_tokens = None;
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--max-tokens" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-tokens requires a positive integer"))?;
                max_tokens = Some(parse_max_tokens(value)?);
            }
            _ if arg.starts_with("--max-tokens=") => {
                let value = arg.trim_start_matches("--max-tokens=");
                max_tokens = Some(parse_max_tokens(value)?);
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }
    }

    let map = build_repo_map(context_cwd(context), RepoMapOptions { max_tokens })?;
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&map)?));
    }
    Ok(CommandResult::text(format_repo_map_text(&map)))
}

fn index_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut cache = None;
    let mut max_bytes_per_file = None;
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--root requires a directory path"))?;
                root = Some(parse_root(value)?);
            }
            "--cache" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--cache requires a file path"))?;
                cache = Some(parse_path(value, "--cache")?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = Some(parse_root(value)?);
            }
            _ if arg.starts_with("--cache=") => {
                let value = arg.trim_start_matches("--cache=");
                cache = Some(parse_path(value, "--cache")?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }
    }

    let root = context_root(context, root);
    let options = ContextIndexOptions { max_bytes_per_file };
    let index = match cache {
        Some(cache_path) => build_persistent_context_index(root, options, cache_path)?,
        None => build_context_index(root, options)?,
    };
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&index)?));
    }
    Ok(CommandResult::text(format_context_index_text(&index)))
}

fn artifacts_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut cache = None;
    let mut max_bytes_per_file = None;
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--root requires a directory path"))?;
                root = Some(parse_root(value)?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "--cache" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--cache requires a file path"))?;
                cache = Some(parse_path(value, "--cache")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = Some(parse_root(value)?);
            }
            _ if arg.starts_with("--cache=") => {
                let value = arg.trim_start_matches("--cache=");
                cache = Some(parse_path(value, "--cache")?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }
    }

    let root = context_root(context, root);
    let options = ContextArtifactOptions { max_bytes_per_file };
    let report = match cache {
        Some(cache_path) => build_persistent_context_artifacts(root, options, cache_path)?,
        None => build_context_artifacts(root, options)?,
    };
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format_context_artifacts_text(&report)))
}

fn artifact_graph_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut max_bytes_per_file = None;
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--root requires a directory path"))?;
                root = Some(parse_root(value)?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = Some(parse_root(value)?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }
    }

    let graph = build_context_artifact_dependency_graph(
        context_root(context, root),
        ContextArtifactOptions { max_bytes_per_file },
    )?;
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&graph)?));
    }
    Ok(CommandResult::text(format_context_artifact_graph_text(
        &graph,
    )))
}

fn artifact_store_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut cache = None;
    let mut max_bytes_per_file = None;
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--root requires a directory path"))?;
                root = Some(parse_root(value)?);
            }
            "--cache" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--cache requires a file path"))?;
                cache = Some(parse_path(value, "--cache")?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = Some(parse_root(value)?);
            }
            _ if arg.starts_with("--cache=") => {
                let value = arg.trim_start_matches("--cache=");
                cache = Some(parse_path(value, "--cache")?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }
    }

    let store = match cache {
        Some(cache_path) => build_persistent_context_artifact_store(
            context_root(context, root),
            ContextArtifactOptions { max_bytes_per_file },
            cache_path,
        )?,
        None => build_context_artifact_store(
            context_root(context, root),
            ContextArtifactOptions { max_bytes_per_file },
        )?,
    };
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&store)?));
    }
    Ok(CommandResult::text(format_context_artifact_store_text(
        &store,
    )))
}

fn search_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut limit = None;
    let mut max_bytes_per_file = None;
    let mut query = Vec::new();
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--root requires a directory path"))?;
                root = Some(parse_root(value)?);
            }
            "--limit" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--limit requires a positive integer"))?;
                limit = Some(parse_positive_usize(value, "--limit")?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if arg.starts_with("--limit=") => {
                let value = arg.trim_start_matches("--limit=");
                limit = Some(parse_positive_usize(value, "--limit")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = Some(parse_root(value)?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" if query.is_empty() => {
                return Ok(CommandResult::text(usage()))
            }
            _ if arg.starts_with('-') => return Err(anyhow!(usage())),
            _ => query.push(arg.to_string()),
        }
    }
    if query.is_empty() {
        return Err(anyhow!(
            "Usage: kiana context search <query> [--json] [--limit N]"
        ));
    }

    let results = search_context_index(
        context_root(context, root),
        &query.join(" "),
        ContextSearchOptions {
            limit,
            max_bytes_per_file,
        },
    )?;
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&results)?));
    }
    Ok(CommandResult::text(format_context_search_text(&results)))
}

fn pack_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut limit = None;
    let mut max_bytes_per_file = None;
    let mut max_snippet_lines = None;
    let mut query = Vec::new();
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--root requires a directory path"))?;
                root = Some(parse_root(value)?);
            }
            "--limit" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--limit requires a positive integer"))?;
                limit = Some(parse_positive_usize(value, "--limit")?);
            }
            "--max-snippet-lines" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-snippet-lines requires a positive integer"))?;
                max_snippet_lines = Some(parse_positive_usize(value, "--max-snippet-lines")?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if arg.starts_with("--limit=") => {
                let value = arg.trim_start_matches("--limit=");
                limit = Some(parse_positive_usize(value, "--limit")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                root = Some(parse_root(value)?);
            }
            _ if arg.starts_with("--max-snippet-lines=") => {
                let value = arg.trim_start_matches("--max-snippet-lines=");
                max_snippet_lines = Some(parse_positive_usize(value, "--max-snippet-lines")?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" if query.is_empty() => {
                return Ok(CommandResult::text(usage()))
            }
            _ if arg.starts_with('-') => return Err(anyhow!(usage())),
            _ => query.push(arg.to_string()),
        }
    }
    if query.is_empty() {
        return Err(anyhow!(
            "Usage: kiana context pack <query> [--json] [--limit N]"
        ));
    }

    let pack = build_context_pack(
        context_root(context, root),
        &query.join(" "),
        ContextPackOptions {
            limit,
            max_bytes_per_file,
            max_snippet_lines,
        },
    )?;
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&pack)?));
    }
    Ok(CommandResult::text(format_context_pack_text(&pack)))
}

fn parse_max_tokens(value: &str) -> anyhow::Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow!("--max-tokens requires a positive integer"))?;
    if parsed == 0 {
        return Err(anyhow!("--max-tokens requires a positive integer"));
    }
    Ok(parsed)
}

fn parse_positive_usize(value: &str, label: &str) -> anyhow::Result<usize> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| anyhow!("{label} requires a positive integer"))?;
    if parsed == 0 {
        return Err(anyhow!("{label} requires a positive integer"));
    }
    Ok(parsed)
}

fn parse_root(value: &str) -> anyhow::Result<PathBuf> {
    parse_path(value, "--root")
}

fn parse_path(value: &str, label: &str) -> anyhow::Result<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("{label} requires a path"));
    }
    Ok(PathBuf::from(value))
}

fn context_root(context: &CommandContext, root: Option<PathBuf>) -> PathBuf {
    root.unwrap_or_else(|| context_cwd(context))
}

fn context_cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn format_repo_map_text(map: &RepoMap) -> String {
    let mut lines = vec![
        "Repo map".to_string(),
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

fn format_context_index_text(index: &ContextIndex) -> String {
    let mut lines = vec![
        "Context index".to_string(),
        format!("root: {}", index.root),
        format!(
            "files_indexed: {} skipped_files: {} total_bytes: {}",
            index.files_indexed, index.skipped_files, index.total_bytes
        ),
    ];
    for file in &index.files {
        lines.push(format!(
            "- {} [{}] bytes={} lines={} hash={}",
            file.path,
            file.language.as_deref().unwrap_or("unknown"),
            file.bytes,
            file.line_count,
            file.content_hash
        ));
    }
    if let Some(cache) = &index.cache {
        lines.push(format!(
            "cache: {} status={} reused={} added={} changed={} removed={}",
            cache.path,
            cache.status,
            cache.reused_files,
            cache.added_files,
            cache.changed_files,
            cache.removed_files
        ));
    }
    lines.join("\n")
}

fn format_context_artifacts_text(report: &ContextArtifacts) -> String {
    let mut lines = vec![
        "Context artifacts".to_string(),
        format!("root: {}", report.root),
        format!(
            "files_indexed: {} skipped_files: {} artifacts={}",
            report.files_indexed,
            report.skipped_files,
            report.artifacts.len()
        ),
    ];
    for artifact in &report.artifacts {
        lines.push(format!(
            "- {} [{}] kind={} bytes={} lines={} hash={} id={}",
            artifact.path,
            artifact.language.as_deref().unwrap_or("unknown"),
            artifact.kind,
            artifact.bytes,
            artifact.line_count,
            artifact.content_hash,
            artifact.id
        ));
    }
    if let Some(cache) = &report.cache {
        lines.push(format!(
            "cache: {} status={} reused={} added={} changed={} removed={}",
            cache.path,
            cache.status,
            cache.reused_artifacts,
            cache.added_artifacts,
            cache.changed_artifacts,
            cache.removed_artifacts
        ));
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

fn format_context_artifact_store_text(store: &ContextArtifactStore) -> String {
    let mut lines = vec![
        "Context artifact store".to_string(),
        format!("root: {}", store.root),
        format!(
            "schema: {} artifacts={} dependencies={}",
            store.schema, store.artifact_count, store.dependency_count
        ),
        format!("artifacts_schema: {}", store.artifacts_schema),
        format!("dependency_graph_schema: {}", store.dependency_graph_schema),
        format!(
            "artifact_roles: {}",
            store
                .artifact_roles
                .iter()
                .map(|role| format!("{}={}", role.role, role.count))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ];
    if let Some(cache) = &store.cache {
        lines.push(format!(
            "cache: {} status={} reused_artifacts={} added_artifacts={} changed_artifacts={} removed_artifacts={} reused_dependencies={} added_dependencies={} removed_dependencies={}",
            cache.path,
            cache.status,
            cache.reused_artifacts,
            cache.added_artifacts,
            cache.changed_artifacts,
            cache.removed_artifacts,
            cache.reused_dependencies,
            cache.added_dependencies,
            cache.removed_dependencies
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

#[cfg(test)]
mod tests {
    use super::ContextCommand;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn context_rejects_unknown_args_instead_of_returning_status() {
        let result = ContextCommand
            .execute(CommandContext {
                args: "json please".to_string(),
                app_state: HashMap::from([("tasks".to_string(), json!([{ "id": "t1" }]))]),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn context_repo_map_json_uses_cwd_and_budget() {
        let root = fixture_root("command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub struct Widget;\nfn render() {}\n",
        )
        .unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "repo-map --json --max-tokens 1000".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["token_budget"], 1000);
        assert_eq!(value["truncated"], false);
        assert_eq!(value["files"][0]["path"], "src/lib.rs");
        assert_eq!(value["files"][0]["language"], "rust");
        assert!(value["files"][0]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol == "struct Widget"));
        assert!(value["files"][0]["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .any(|symbol| symbol == "fn render"));

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_index_json_reports_file_hashes() {
        let root = fixture_root("index-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn indexed() {}\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "index --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-index.v1");
        assert_eq!(value["files_indexed"], 1);
        assert_eq!(value["files"][0]["path"], "src/lib.rs");
        assert_eq!(
            value["files"][0]["content_hash"].as_str().unwrap().len(),
            16
        );

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_index_json_uses_explicit_root() {
        let cwd = fixture_root("index-root-cwd");
        let artifact_root = fixture_root("index-root-artifact");
        fs::create_dir_all(artifact_root.join("bundle")).unwrap();
        fs::write(artifact_root.join("bundle/notes.md"), "artifact context\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "index --json --root={}",
                    artifact_root.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-index.v1");
        assert_eq!(value["files_indexed"], 1);
        assert_eq!(value["files"][0]["path"], "bundle/notes.md");

        let _ = fs::remove_dir_all(cwd);
        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_index_json_persists_incremental_cache() {
        let root = fixture_root("index-cache-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn indexed() {}\n").unwrap();
        let cache_path = root.join(".kiana").join("context-index.json");
        let cache_arg = cache_path.to_string_lossy().replace('\\', "/");

        let first = ContextCommand
            .execute(CommandContext {
                args: format!("index --json --cache {cache_arg}"),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let first_value: serde_json::Value = serde_json::from_str(&first.value).unwrap();

        assert_eq!(first_value["schema"], "kiana.context-index.v1");
        assert_eq!(first_value["cache"]["status"], "created");
        assert_eq!(first_value["cache"]["added_files"], 1);
        assert_eq!(first_value["cache"]["reused_files"], 0);
        assert!(cache_path.is_file());

        let second = ContextCommand
            .execute(CommandContext {
                args: format!("index --json --cache {cache_arg}"),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let second_value: serde_json::Value = serde_json::from_str(&second.value).unwrap();

        assert_eq!(second_value["cache"]["status"], "updated");
        assert_eq!(second_value["cache"]["reused_files"], 1);
        assert_eq!(second_value["cache"]["added_files"], 0);
        assert_eq!(second_value["cache"]["changed_files"], 0);
        assert_eq!(second_value["cache"]["removed_files"], 0);

        fs::write(root.join("src/lib.rs"), "pub fn indexed_changed() {}\n").unwrap();
        fs::write(root.join("README.md"), "new context\n").unwrap();
        let third = ContextCommand
            .execute(CommandContext {
                args: format!("index --json --cache {cache_arg}"),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let third_value: serde_json::Value = serde_json::from_str(&third.value).unwrap();

        assert_eq!(third_value["cache"]["status"], "updated");
        assert_eq!(third_value["cache"]["added_files"], 1);
        assert_eq!(third_value["cache"]["changed_files"], 1);
        assert_eq!(third_value["cache"]["removed_files"], 0);

        fs::remove_file(root.join("README.md")).unwrap();
        let fourth = ContextCommand
            .execute(CommandContext {
                args: format!("index --json --cache {cache_arg}"),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let fourth_value: serde_json::Value = serde_json::from_str(&fourth.value).unwrap();

        assert_eq!(fourth_value["cache"]["removed_files"], 1);

        let _ = fs::remove_dir_all(Path::new(fourth_value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_index_json_recovers_corrupt_incremental_cache() {
        let root = fixture_root("index-cache-corrupt-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn recoverable() {}\n").unwrap();
        let cache_path = root.join(".kiana").join("context-index.json");
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        fs::write(&cache_path, "{not valid json").unwrap();
        let cache_arg = cache_path.to_string_lossy().replace('\\', "/");

        let result = ContextCommand
            .execute(CommandContext {
                args: format!("index --json --cache {cache_arg}"),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-index.v1");
        assert_eq!(value["files_indexed"], 1);
        assert_eq!(value["cache"]["status"], "recovered");
        assert_eq!(value["cache"]["added_files"], 1);
        assert_eq!(value["cache"]["reused_files"], 0);
        assert_eq!(value["cache"]["changed_files"], 0);
        assert_eq!(value["cache"]["removed_files"], 0);
        assert!(serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(&cache_path).unwrap()
        )
        .is_ok());

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_search_json_returns_ranked_hits() {
        let root = fixture_root("search-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// checkout checkout\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "checkout guide\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "search checkout --json --limit 1".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-search.v1");
        assert_eq!(value["terms"][0], "checkout");
        assert_eq!(value["hits"].as_array().unwrap().len(), 1);
        assert_eq!(value["hits"][0]["path"], "src/lib.rs");
        assert_eq!(value["hits"][0]["line_number"], 1);

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_search_json_matches_path_only_query() {
        let root = fixture_root("search-path-command");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/path-only.md"), "first module summary\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "search docs/path-only.md --json --limit 1".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-search.v1");
        assert_eq!(value["hits"].as_array().unwrap().len(), 1);
        assert_eq!(value["hits"][0]["path"], "docs/path-only.md");
        assert_eq!(value["hits"][0]["occurrences"], 0);
        assert_eq!(value["hits"][0]["line"], "first module summary");

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_pack_json_returns_snippets() {
        let root = fixture_root("pack-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// checkout checkout\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "checkout guide\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "pack checkout --json --limit 1 --max-snippet-lines 1".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-pack.v1");
        assert_eq!(value["terms"][0], "checkout");
        assert_eq!(value["limit"], 1);
        assert_eq!(value["max_snippet_lines"], 1);
        assert_eq!(value["snippets"].as_array().unwrap().len(), 1);
        assert_eq!(value["snippets"][0]["path"], "src/lib.rs");
        assert_eq!(value["snippets"][0]["start_line"], 1);
        assert_eq!(value["snippets"][0]["end_line"], 1);
        assert!(value["snippets"][0]["excerpt"]
            .as_str()
            .unwrap()
            .contains("checkout"));

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_pack_json_uses_explicit_root() {
        let cwd = fixture_root("pack-root-cwd");
        let artifact_root = fixture_root("pack-root-artifact");
        fs::create_dir_all(artifact_root.join("bundle")).unwrap();
        fs::write(
            artifact_root.join("bundle/notes.md"),
            "first artifact line\nsecond line\n",
        )
        .unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "pack bundle/notes.md --json --root {} --limit 1 --max-snippet-lines 1",
                    artifact_root.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-pack.v1");
        assert_eq!(value["snippets"].as_array().unwrap().len(), 1);
        assert_eq!(value["snippets"][0]["path"], "bundle/notes.md");
        assert_eq!(value["snippets"][0]["occurrences"], 0);
        assert_eq!(value["snippets"][0]["excerpt"], "first artifact line");

        let _ = fs::remove_dir_all(cwd);
        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_pack_text_reports_artifact_graph_summary() {
        let root = fixture_root("pack-text-artifact-graph");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/lib.rs"),
            "pub fn checkout() {}\n// checkout workflow\n",
        )
        .unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "pack checkout --limit 1 --max-snippet-lines 1".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("artifact_graph: schema=kiana.context-artifact-graph.v1 nodes=1 edges=1"));
        assert!(result.value.contains("graph: query:checkout -> snippet:"));
        assert!(result.value.contains("relation=matched terms=checkout"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn context_artifacts_json_reports_local_inventory() {
        let root = fixture_root("artifacts-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "artifacts --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-artifacts.v1");
        assert_eq!(value["files_indexed"], 1);
        assert_eq!(value["skipped_files"], 0);
        assert_eq!(value["artifacts"].as_array().unwrap().len(), 1);
        assert_eq!(value["artifacts"][0]["kind"], "file");
        assert_eq!(value["artifacts"][0]["path"], "src/lib.rs");
        assert_eq!(value["artifacts"][0]["language"], "rust");
        assert_eq!(
            value["artifacts"][0]["content_hash"]
                .as_str()
                .unwrap()
                .len(),
            16
        );

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_artifacts_json_persists_incremental_cache() {
        let root = fixture_root("artifacts-cache-command");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        let cache_path = root.join(".kiana").join("context-artifacts.json");

        let first = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "artifacts --json --cache {}",
                    cache_path.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let first_value: serde_json::Value = serde_json::from_str(&first.value).unwrap();
        assert_eq!(first_value["schema"], "kiana.context-artifacts.v1");
        assert_eq!(first_value["cache"]["status"], "created");
        assert_eq!(first_value["cache"]["added_artifacts"], 1);
        assert!(cache_path.is_file());

        let second = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "artifacts --json --cache {}",
                    cache_path.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let second_value: serde_json::Value = serde_json::from_str(&second.value).unwrap();
        assert_eq!(second_value["cache"]["status"], "updated");
        assert_eq!(second_value["cache"]["reused_artifacts"], 1);
        assert_eq!(second_value["cache"]["added_artifacts"], 0);
        assert_eq!(second_value["cache"]["changed_artifacts"], 0);

        let _ = fs::remove_dir_all(Path::new(second_value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_artifact_graph_json_reports_test_edges() {
        let root = fixture_root("artifact-graph-command");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "artifact-graph --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(
            value["schema"],
            "kiana.context-artifact-dependency-graph.v1"
        );
        assert_eq!(value["nodes"].as_array().unwrap().len(), 3);
        assert!(value["edges"].as_array().unwrap().iter().any(|edge| {
            edge["relation"] == "test_of"
                && edge["evidence"] == "tests/lib_test.rs matches src/lib.rs"
        }));
        assert!(value["edges"].as_array().unwrap().iter().any(|edge| {
            edge["relation"] == "path_reference"
                && edge["evidence"] == "docs/design.md references src/lib.rs"
        }));

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_artifact_store_json_reports_manifest() {
        let root = fixture_root("artifact-store-command");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: "artifact-store --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-artifact-store.v1");
        assert_eq!(value["artifacts_schema"], "kiana.context-artifacts.v1");
        assert_eq!(
            value["dependency_graph_schema"],
            "kiana.context-artifact-dependency-graph.v1"
        );
        assert_eq!(value["artifact_count"], 3);
        assert_eq!(value["dependency_count"], 2);
        assert!(value["artifact_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "design" && role["count"] == 1));
        assert!(value["artifact_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "source" && role["count"] == 1));
        assert!(value["artifact_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "test" && role["count"] == 1));
        assert_eq!(value["artifacts"]["artifacts"].as_array().unwrap().len(), 3);
        assert!(value["dependency_graph"]["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["relation"] == "path_reference"
                && edge["evidence"] == "docs/design.md references src/lib.rs"));

        let _ = fs::remove_dir_all(Path::new(value["root"].as_str().unwrap()));
    }

    #[tokio::test]
    async fn context_artifact_store_json_persists_cache_report() {
        let root = fixture_root("artifact-store-cache-command");
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(
            root.join("docs/design.md"),
            "The release API is implemented in src/lib.rs.\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn release() {}\n").unwrap();
        fs::write(root.join("tests/lib_test.rs"), "use kiana::release;\n").unwrap();
        let cache_path = root.join(".kiana").join("context-artifact-store.json");

        let first = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "artifact-store --json --cache {}",
                    cache_path.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let first_value: serde_json::Value = serde_json::from_str(&first.value).unwrap();
        assert_eq!(first_value["schema"], "kiana.context-artifact-store.v1");
        assert_eq!(first_value["cache"]["status"], "created");
        assert_eq!(first_value["cache"]["added_artifacts"], 3);
        assert_eq!(first_value["cache"]["added_dependencies"], 2);
        assert!(cache_path.is_file());

        let second = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "artifact-store --json --cache {}",
                    cache_path.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let second_value: serde_json::Value = serde_json::from_str(&second.value).unwrap();
        assert_eq!(second_value["cache"]["status"], "updated");
        assert_eq!(second_value["cache"]["reused_artifacts"], 3);
        assert_eq!(second_value["cache"]["added_artifacts"], 0);
        assert_eq!(second_value["cache"]["reused_dependencies"], 2);
        assert_eq!(second_value["cache"]["added_dependencies"], 0);

        let _ = fs::remove_dir_all(Path::new(second_value["root"].as_str().unwrap()));
    }

    fn fixture_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-context-repo-map-{name}-{}-{unique}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}
