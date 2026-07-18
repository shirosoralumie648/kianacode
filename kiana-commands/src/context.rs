use crate::local_state::{app_state_array_len, app_state_keys};
use crate::types::{Command, CommandContext, CommandResult, CommandRoute, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use serde_json::Value;

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
        if let Some(rest) = args.strip_prefix("index") {
            return materialization_query_route("index", "index_cache_write", rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifacts") {
            return materialization_query_route("artifacts", "artifacts_cache_write", rest.trim());
        }
        if let Some(rest) = args.strip_prefix("ingest") {
            return ingest_query_route(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-store") {
            return materialization_query_route(
                "artifact_store",
                "artifact_store_cache_write",
                rest.trim(),
            );
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
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifacts") {
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("ingest") {
            return migrated_query_result(rest.trim());
        }
        if let Some(rest) = args.strip_prefix("artifact-store") {
            return migrated_query_result(rest.trim());
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

fn materialization_query_route(
    read_operation: &str,
    write_operation: &str,
    args: &str,
) -> anyhow::Result<CommandRoute> {
    let mut json = false;
    let mut root = None;
    let mut cache = None;
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
            "--cache" => {
                cache = Some(
                    parts
                        .next()
                        .ok_or_else(|| anyhow!("--cache requires a file path"))?
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
                root = Some(arg.trim_start_matches("--root=").to_owned());
            }
            _ if arg.starts_with("--cache=") => {
                cache = Some(arg.trim_start_matches("--cache=").to_owned());
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                max_bytes_per_file = Some(parse_positive_usize(
                    arg.trim_start_matches("--max-bytes-per-file="),
                    "--max-bytes-per-file",
                )?);
            }
            "help" | "--help" | "-h" => return Ok(CommandRoute::Local),
            _ => return Err(anyhow!(usage())),
        }
    }
    for (value, label) in [(&root, "--root"), (&cache, "--cache")] {
        if let Some(value) = value {
            parse_path(value, label)?;
        }
    }
    let operation = if cache.is_some() {
        write_operation
    } else {
        read_operation
    };
    let mut options = serde_json::Map::new();
    if let Some(root) = root {
        options.insert("root".to_owned(), Value::String(root));
    }
    if let Some(cache) = cache {
        options.insert("cache".to_owned(), Value::String(cache));
    }
    if let Some(max_bytes_per_file) = max_bytes_per_file {
        options.insert(
            "max_bytes_per_file".to_owned(),
            Value::from(max_bytes_per_file),
        );
    }
    context_query_route(operation, json, options)
}

fn ingest_query_route(args: &str) -> anyhow::Result<CommandRoute> {
    let mut json = false;
    let mut root = None;
    let mut source = None;
    let mut store = None;
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
            "--source" => {
                source = Some(
                    parts
                        .next()
                        .ok_or_else(|| anyhow!("--source requires a directory path"))?
                        .to_owned(),
                );
            }
            "--store" => {
                store = Some(
                    parts
                        .next()
                        .ok_or_else(|| anyhow!("--store requires a directory path"))?
                        .to_owned(),
                );
            }
            _ if arg.starts_with("--root=") => {
                root = Some(arg.trim_start_matches("--root=").to_owned());
            }
            _ if arg.starts_with("--source=") => {
                source = Some(arg.trim_start_matches("--source=").to_owned());
            }
            _ if arg.starts_with("--store=") => {
                store = Some(arg.trim_start_matches("--store=").to_owned());
            }
            _ if arg.starts_with("--max-bytes-per-file=") => {
                max_bytes_per_file = Some(parse_positive_usize(
                    arg.trim_start_matches("--max-bytes-per-file="),
                    "--max-bytes-per-file",
                )?);
            }
            "--max-bytes-per-file" => {
                let value = parts
                    .next()
                    .ok_or_else(|| anyhow!("--max-bytes-per-file requires a positive integer"))?;
                max_bytes_per_file = Some(parse_positive_usize(value, "--max-bytes-per-file")?);
            }
            "help" | "--help" | "-h" => return Ok(CommandRoute::Local),
            _ => return Err(anyhow!(usage())),
        }
    }
    let source = source.ok_or_else(|| {
        anyhow!("Usage: kiana context ingest --source DIR [--json] [--root DIR] [--store DIR]")
    })?;
    for (value, label) in [
        (&root, "--root"),
        (&Some(source.clone()), "--source"),
        (&store, "--store"),
    ] {
        if let Some(value) = value {
            parse_path(value, label)?;
        }
    }
    let mut options = serde_json::Map::new();
    if let Some(root) = root {
        options.insert("root".to_owned(), Value::String(root));
    }
    options.insert("source".to_owned(), Value::String(source));
    if let Some(store) = store {
        options.insert("store".to_owned(), Value::String(store));
    }
    if let Some(max_bytes_per_file) = max_bytes_per_file {
        options.insert(
            "max_bytes_per_file".to_owned(),
            Value::from(max_bytes_per_file),
        );
    }
    context_query_route("artifact_ingest_write", json, options)
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

fn parse_path(value: &str, label: &str) -> anyhow::Result<()> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("{label} requires a path"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ContextCommand;
    use crate::{Command, CommandContext, CommandRoute};
    use serde_json::json;
    use std::collections::HashMap;

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
    async fn context_index_read_routes_without_a_write_capability() {
        let context = CommandContext {
            args: "index --json --root src --max-bytes-per-file 4096".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "index",
                    "output": "json",
                    "options": { "root": "src", "max_bytes_per_file": 4096 },
                }),
            }
        );
    }

    #[tokio::test]
    async fn context_index_cache_routes_as_a_local_write() {
        let context = CommandContext {
            args: "index --json --cache .kiana/context-index.json".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "index_cache_write",
                    "output": "json",
                    "options": { "cache": ".kiana/context-index.json" },
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
    async fn context_artifacts_read_routes_without_a_write_capability() {
        let context = CommandContext {
            args: "artifacts --json --root docs".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifacts",
                    "output": "json",
                    "options": { "root": "docs" },
                }),
            }
        );
    }

    #[tokio::test]
    async fn context_ingest_routes_source_and_store_as_local_write() {
        let context = CommandContext {
            args: "ingest --json --source docs --store .kiana/ingest".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifact_ingest_write",
                    "output": "json",
                    "options": {
                        "source": "docs",
                        "store": ".kiana/ingest",
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
    async fn context_artifacts_cache_routes_as_a_local_write() {
        let context = CommandContext {
            args: "artifacts --json --cache .kiana/context-artifacts.json".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&context).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifacts_cache_write",
                    "output": "json",
                    "options": { "cache": ".kiana/context-artifacts.json" },
                }),
            }
        );
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
    async fn context_artifact_store_read_and_cache_write_have_separate_operations() {
        let read = CommandContext {
            args: "artifact-store --json".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&read).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifact_store",
                    "output": "json",
                    "options": {},
                }),
            }
        );

        let write = CommandContext {
            args: "artifact-store --json --cache .kiana/context-artifact-store.json".to_owned(),
            app_state: HashMap::new(),
        };
        assert_eq!(
            ContextCommand.route(&write).unwrap(),
            CommandRoute::ControlPlane {
                name: "context.query.v1".to_owned(),
                arguments: json!({
                    "operation": "artifact_store_cache_write",
                    "output": "json",
                    "options": { "cache": ".kiana/context-artifact-store.json" },
                }),
            }
        );
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
}
