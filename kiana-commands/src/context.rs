use crate::local_state::{app_state_array_len, app_state_keys};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use kiana_query::{build_repo_map, RepoMap, RepoMapOptions};
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
    "Usage: kiana context [status|json|repo-map [--json] [--max-tokens N]]"
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

fn parse_max_tokens(value: &str) -> anyhow::Result<u64> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| anyhow!("--max-tokens requires a positive integer"))?;
    if parsed == 0 {
        return Err(anyhow!("--max-tokens requires a positive integer"));
    }
    Ok(parsed)
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
