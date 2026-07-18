use crate::local_state::{app_state_array_len, app_state_keys};
use crate::types::{Command, CommandContext, CommandResult, CommandRoute, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use kiana_query::{
    build_context_artifact_store, build_context_artifacts, build_context_index,
    build_persistent_context_artifact_store, build_persistent_context_artifacts,
    build_persistent_context_index, ingest_context_artifacts, ContextArtifactIngest,
    ContextArtifactIngestOptions, ContextArtifactOptions, ContextArtifactStore, ContextArtifacts,
    ContextIndex, ContextIndexOptions,
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

    fn route(&self, context: &CommandContext) -> anyhow::Result<CommandRoute> {
        let args = context.args.trim();
        if let Some(rest) = args.strip_prefix("repo-map") {
            return repo_map_route(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-readiness") {
            return artifact_query_route("artifact_readiness", rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-graph") {
            return artifact_query_route("artifact_graph", rest.trim());
        }
        if let Some(rest) = args.strip_prefix("search") {
            return search_query_route("search", rest.trim(), false);
        }
        if let Some(rest) = args.strip_prefix("vector-search") {
            return search_query_route("vector_search", rest.trim(), false);
        }
        if let Some(rest) = args.strip_prefix("pack") {
            return search_query_route("pack", rest.trim(), true);
        }
        Ok(CommandRoute::Local)
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = context.args.trim();
        if let Some(rest) = args.strip_prefix("repo-map") {
            if is_help_args(rest.trim()) {
                return Ok(CommandResult::text(usage()));
            }
            return Err(anyhow!("command_requires_control_plane"));
        }
        if let Some(rest) = args.strip_prefix("index") {
            return index_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifacts") {
            return artifacts_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("ingest") {
            return ingest_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-store") {
            return artifact_store_result(&context, rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-readiness") {
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-graph") {
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("search") {
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("vector-search") {
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("pack") {
            return migrated_query_result(rest.trim());
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

fn is_help_args(args: &str) -> bool {
    matches!(args, "help" | "--help" | "-h")
}

fn migrated_query_result(args: &str) -> anyhow::Result<CommandResult> {
    if is_help_args(args) {
        return Ok(CommandResult::text(usage()));
    }
    Err(anyhow!("command_requires_control_plane"))
}

fn usage() -> &'static str {
    "Usage: kiana context [status|json|repo-map [--json] [--max-tokens N]|index [--json] [--root DIR] [--cache PATH] [--max-bytes-per-file N]|artifacts [--json] [--root DIR] [--cache PATH] [--max-bytes-per-file N]|ingest --source DIR [--json] [--root DIR] [--store DIR] [--max-bytes-per-file N]|artifact-store [--json] [--root DIR] [--cache PATH] [--max-bytes-per-file N]|artifact-readiness [--json] [--root DIR] [--max-bytes-per-file N]|artifact-graph [--json] [--root DIR] [--max-bytes-per-file N]|search <query> [--json] [--root DIR] [--limit N] [--max-bytes-per-file N]|vector-search <query> [--json] [--root DIR] [--limit N] [--max-bytes-per-file N]|pack <query> [--json] [--root DIR] [--limit N] [--max-snippet-lines N] [--max-bytes-per-file N]]"
}

fn repo_map_route(args: &str) -> anyhow::Result<CommandRoute> {
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
            "help" | "--help" | "-h" => return Ok(CommandRoute::Local),
            _ => return Err(anyhow!(usage())),
        }
    }

    let mut options = serde_json::Map::new();
    if let Some(max_tokens) = max_tokens {
        options.insert("max_tokens".to_owned(), Value::from(max_tokens));
    }
    Ok(CommandRoute::ControlPlane {
        name: "context.query.v1".to_owned(),
        arguments: serde_json::json!({
            "operation": "repo_map",
            "output": if json { "json" } else { "text" },
            "options": options,
        }),
    })
}

fn artifact_query_route(operation: &str, args: &str) -> anyhow::Result<CommandRoute> {
    let mut json = false;
    let mut root = None;
    let mut max_bytes_per_file = None;
    let mut parts = args.split_whitespace();
    while let Some(arg) = parts.next() {
        match arg {
            "--json" => json = true,
            "--root" => {
                root = Some(
                    parts
                        .next()
                        .ok_or_else(|| anyhow!("--root requires a directory path"))?
                        .to_owned(),
                );
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                parse_path(value, "--root")?;
                root = Some(value.to_owned());
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandRoute::Local),
            _ => return Err(anyhow!(usage())),
        }
    }
    if let Some(value) = root.as_deref() {
        parse_path(value, "--root")?;
    }

    let mut options = serde_json::Map::new();
    if let Some(root) = root {
        options.insert("root".to_owned(), Value::String(root));
    }
    if let Some(max_bytes_per_file) = max_bytes_per_file {
        options.insert(
            "max_bytes_per_file".to_owned(),
            Value::from(max_bytes_per_file),
        );
    }
    context_query_route(operation, json, options)
}

fn search_query_route(
    operation: &str,
    args: &str,
    allow_max_snippet_lines: bool,
) -> anyhow::Result<CommandRoute> {
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
                root = Some(
                    parts
                        .next()
                        .ok_or_else(|| anyhow!("--root requires a directory path"))?
                        .to_owned(),
                );
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
            "--max-snippet-lines" if allow_max_snippet_lines => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-snippet-lines requires a positive integer"))?;
                max_snippet_lines = Some(parse_positive_usize(value, "--max-snippet-lines")?);
            }
            _ if arg.starts_with("--root=") => {
                let value = arg.trim_start_matches("--root=");
                parse_path(value, "--root")?;
                root = Some(value.to_owned());
            }
            _ if arg.starts_with("--limit=") => {
                let value = arg.trim_start_matches("--limit=");
                limit = Some(parse_positive_usize(value, "--limit")?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            _ if allow_max_snippet_lines && arg.starts_with("--max-snippet-lines=") => {
                let value = arg.trim_start_matches("--max-snippet-lines=");
                max_snippet_lines = Some(parse_positive_usize(value, "--max-snippet-lines")?);
            }
            "help" | "--help" | "-h" if query.is_empty() => return Ok(CommandRoute::Local),
            _ if arg.starts_with('-') => return Err(anyhow!(usage())),
            _ => query.push(arg.to_owned()),
        }
    }
    if query.is_empty() {
        return Err(anyhow!(match operation {
            "search" => "Usage: kiana context search <query> [--json] [--limit N]",
            "vector_search" => {
                "Usage: kiana context vector-search <query> [--json] [--limit N]"
            }
            _ => "Usage: kiana context pack <query> [--json] [--limit N]",
        }));
    }
    if let Some(value) = root.as_deref() {
        parse_path(value, "--root")?;
    }

    let mut options = serde_json::Map::new();
    options.insert("query".to_owned(), Value::String(query.join(" ")));
    if let Some(root) = root {
        options.insert("root".to_owned(), Value::String(root));
    }
    if let Some(limit) = limit {
        options.insert("limit".to_owned(), Value::from(limit));
    }
    if let Some(max_bytes_per_file) = max_bytes_per_file {
        options.insert(
            "max_bytes_per_file".to_owned(),
            Value::from(max_bytes_per_file),
        );
    }
    if let Some(max_snippet_lines) = max_snippet_lines {
        options.insert(
            "max_snippet_lines".to_owned(),
            Value::from(max_snippet_lines),
        );
    }
    context_query_route(operation, json, options)
}

fn context_query_route(
    operation: &str,
    json_output: bool,
    options: serde_json::Map<String, Value>,
) -> anyhow::Result<CommandRoute> {
    Ok(CommandRoute::ControlPlane {
        name: "context.query.v1".to_owned(),
        arguments: serde_json::json!({
            "operation": operation,
            "output": if json_output { "json" } else { "text" },
            "options": options,
        }),
    })
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

fn ingest_result(context: &CommandContext, args: &str) -> anyhow::Result<CommandResult> {
    let mut json = false;
    let mut root = None;
    let mut source = None;
    let mut store_dir = None;
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
            "--source" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--source requires a directory path"))?;
                source = Some(parse_path(value, "--source")?);
            }
            "--store" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--store requires a directory path"))?;
                store_dir = Some(parse_path(value, "--store")?);
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
            _ if arg.starts_with("--source=") => {
                let value = arg.trim_start_matches("--source=");
                source = Some(parse_path(value, "--source")?);
            }
            _ if arg.starts_with("--store=") => {
                let value = arg.trim_start_matches("--store=");
                store_dir = Some(parse_path(value, "--store")?);
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                let value = arg.trim_start_matches("--max-bytes-per-file=");
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }
    }
    let workspace = context_root(context, root);
    let source = source.ok_or_else(|| {
        anyhow!("Usage: kiana context ingest --source DIR [--json] [--store DIR]")
    })?;
    let source = if source.is_absolute() {
        source
    } else {
        workspace.join(source)
    };
    let report = ingest_context_artifacts(
        &workspace,
        source,
        ContextArtifactIngestOptions {
            store_dir,
            max_bytes_per_file,
        },
    )?;
    if json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(format_context_artifact_ingest_text(
        &report,
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

fn format_context_artifact_ingest_text(report: &ContextArtifactIngest) -> String {
    let mut lines = vec![
        "Context artifact ingest".to_string(),
        format!("root: {}", report.root),
        format!("source_root: {}", report.source_root),
        format!("store_dir: {}", report.store_dir),
        format!("manifest_path: {}", report.manifest_path),
        format!(
            "schema: {} artifacts_schema: {} ingested_files: {} skipped_files: {} total_bytes: {}",
            report.schema,
            report.artifacts_schema,
            report.ingested_files,
            report.skipped_files,
            report.total_bytes
        ),
        format!(
            "sync: {} status={} reused={} added={} changed={} removed={}",
            report.sync.path,
            report.sync.status,
            report.sync.reused_files,
            report.sync.added_files,
            report.sync.changed_files,
            report.sync.removed_files
        ),
    ];
    for artifact in &report.artifacts {
        lines.push(format!(
            "- {} -> {} [{}] kind={} bytes={} lines={} hash={} id={}",
            artifact.source_path,
            artifact.stored_path,
            artifact.language.as_deref().unwrap_or("unknown"),
            artifact.kind,
            artifact.bytes,
            artifact.line_count,
            artifact.content_hash,
            artifact.id
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

#[cfg(test)]
mod tests {
    use super::ContextCommand;
    use crate::{Command, CommandContext, CommandRoute};
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
    async fn context_repo_map_routes_to_the_control_plane() {
        let context = CommandContext {
            args: "repo-map --json --max-tokens 1000".to_string(),
            app_state: HashMap::new(),
        };
        let route = ContextCommand.route(&context).unwrap();

        assert_eq!(
            route,
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "repo_map",
                    "output": "json",
                    "options": { "max_tokens": 1000 },
                }),
            }
        );
        let error = ContextCommand.execute(context).await.unwrap_err();
        assert_eq!(error.to_string(), "command_requires_control_plane");
    }

    #[tokio::test]
    async fn context_repo_map_help_stays_local() {
        let context = CommandContext {
            args: "repo-map --help".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(ContextCommand.route(&context).unwrap(), CommandRoute::Local);
        let result = ContextCommand.execute(context).await.unwrap();
        assert!(result.value.starts_with("Usage: kiana context"));
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
    async fn context_search_queries_route_with_structured_options() {
        let context = CommandContext {
            args:
                "vector-search checkout flow --json --root src --limit 2 --max-bytes-per-file 4096"
                    .to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "vector_search",
                    "output": "json",
                    "options": {
                        "query": "checkout flow",
                        "root": "src",
                        "limit": 2,
                        "max_bytes_per_file": 4096,
                    },
                }),
            }
        );
        assert_eq!(
            ContextCommand
                .execute(context)
                .await
                .unwrap_err()
                .to_string(),
            "command_requires_control_plane"
        );
    }

    #[tokio::test]
    async fn context_pack_routes_snippet_limits_without_client_risk() {
        let context = CommandContext {
            args: "pack checkout --limit=1 --max-snippet-lines=3".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "pack",
                    "output": "text",
                    "options": {
                        "query": "checkout",
                        "limit": 1,
                        "max_snippet_lines": 3,
                    },
                }),
            }
        );
    }

    #[tokio::test]
    async fn context_search_rejects_missing_query_and_unknown_flags() {
        let missing = CommandContext {
            args: "search --json".to_owned(),
            app_state: HashMap::new(),
        };
        assert!(ContextCommand.route(&missing).is_err());

        let unknown = CommandContext {
            args: "pack checkout --risk local-write".to_owned(),
            app_state: HashMap::new(),
        };
        assert!(ContextCommand.route(&unknown).is_err());
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
    async fn context_ingest_json_copies_source_artifacts() {
        let root = fixture_root("ingest-command-root");
        let source = fixture_root("ingest-command-source");
        fs::create_dir_all(source.join("docs")).unwrap();
        fs::write(source.join("docs/prd.md"), "# PRD\nShip a local RC\n").unwrap();
        fs::write(source.join("docs/large.md"), "x".repeat(80)).unwrap();
        fs::write(source.join("raw.bin"), b"abc\0def").unwrap();

        let result = ContextCommand
            .execute(CommandContext {
                args: format!(
                    "ingest --json --source {} --max-bytes-per-file 64",
                    source.to_string_lossy().replace('\\', "/")
                ),
                app_state: HashMap::from([("cwd".to_string(), json!(root))]),
            })
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.context-artifact-ingest.v1");
        assert_eq!(value["artifacts_schema"], "kiana.context-artifacts.v1");
        assert_eq!(value["ingested_files"], 1);
        assert_eq!(value["skipped_files"], 1);
        assert_eq!(value["store_dir"], ".kiana/context-ingest");
        assert_eq!(
            value["manifest_path"],
            ".kiana/context-ingest/manifest.json"
        );
        assert_eq!(value["sync"]["path"], ".kiana/context-ingest/manifest.json");
        assert_eq!(value["sync"]["status"], "created");
        assert_eq!(value["sync"]["added_files"], 1);
        assert_eq!(value["sync"]["reused_files"], 0);
        assert_eq!(value["artifacts"][0]["source_path"], "docs/prd.md");
        assert_eq!(value["artifacts"][0]["kind"], "prd");
        assert!(value["artifacts"][0]["stored_path"]
            .as_str()
            .unwrap()
            .starts_with(".kiana/context-ingest/files/"));
        assert!(root
            .join(value["artifacts"][0]["stored_path"].as_str().unwrap())
            .is_file());
        assert!(root.join(".kiana/context-ingest/manifest.json").is_file());

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(source);
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
    async fn context_artifact_graph_routes_to_the_control_plane() {
        let context = CommandContext {
            args: "artifact-graph --json --root docs --max-bytes-per-file 8192".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifact_graph",
                    "output": "json",
                    "options": {
                        "root": "docs",
                        "max_bytes_per_file": 8192,
                    },
                }),
            }
        );
        assert_eq!(
            ContextCommand
                .execute(context)
                .await
                .unwrap_err()
                .to_string(),
            "command_requires_control_plane"
        );
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
    async fn context_artifact_readiness_routes_with_empty_options() {
        let context = CommandContext {
            args: "artifact-readiness --json".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifact_readiness",
                    "output": "json",
                    "options": {},
                }),
            }
        );
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
