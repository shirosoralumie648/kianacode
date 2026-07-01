use crate::local_state::app_state_array_len;
use crate::types::{
    Command, CommandContext, CommandResult, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde_json::Value;
use std::path::{Path, PathBuf};

const CLAUDE_DESKTOP_CONFIG_ENV: &str = "KIANA_CLAUDE_DESKTOP_CONFIG";

pub struct McpCommand;

#[async_trait]
impl Command for McpCommand {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "Manage MCP servers"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        if let Some(argv) = command_context_argv(&context) {
            return execute_mcp_argv(&context, &argv);
        }

        let (command, rest) = split_word(context.args.trim());
        match command.unwrap_or("status") {
            "" | "status" => {}
            "list" => return list_mcp_servers(&context, rest),
            "get" | "show" => return get_mcp_server(&context, rest),
            "add" => return add_mcp_server(rest),
            "add-json" => return add_json_mcp_server(rest),
            "add-from-claude-desktop" => return add_from_claude_desktop(rest),
            "remove" | "delete" => return remove_mcp_server(rest),
            "reset-project-choices" => return reset_project_choices(rest),
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            other => return Err(anyhow!("unknown mcp command '{}'\n\n{}", other, usage())),
        }

        let invocations = app_state_array_len(&context.app_state, "mcp_invocations");
        Ok(CommandResult::text(format!(
            "MCP status\nclient_stdio: wired\nclient_http: wired\nclient_sse: wired\nclient_ws: wired\nserver_transport: wired_stdio_http_sse_ws\nprotocol_surfaces: tools,resources,resource_templates,prompts\nerror_states: auth_error\nregistered_session_invocations: {}\nnotes: local stdio, HTTP, SSE, and WebSocket MCP clients can initialize, list tools, call tools, list/read resources, list resource templates, list prompts, and get prompts; `kiana mcp serve` exposes Kiana tools/resources/resource templates/prompts over stdio and `kiana mcp-server-http`/`kiana mcp-server-ws` expose HTTP/SSE/WS.",
            invocations
        )))
    }
}

fn execute_mcp_argv(context: &CommandContext, argv: &[String]) -> anyhow::Result<CommandResult> {
    let command = argv.first().map(String::as_str).unwrap_or("status");
    let rest = argv_tail_string(argv);
    match command {
        "" | "status" => {}
        "list" => return list_mcp_servers(context, &rest),
        "get" | "show" => return get_mcp_server(context, &rest),
        "add" => return add_mcp_server_tokens(&argv[1..]),
        "add-json" => return add_json_mcp_server(&rest),
        "add-from-claude-desktop" => return add_from_claude_desktop(&rest),
        "remove" | "delete" => return remove_mcp_server(&rest),
        "reset-project-choices" => return reset_project_choices(&rest),
        "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
        other => return Err(anyhow!("unknown mcp command '{}'\n\n{}", other, usage())),
    }

    let invocations = app_state_array_len(&context.app_state, "mcp_invocations");
    Ok(CommandResult::text(format!(
        "MCP status\nclient_stdio: wired\nclient_http: wired\nclient_sse: wired\nclient_ws: wired\nserver_transport: wired_stdio_http_sse_ws\nprotocol_surfaces: tools,resources,resource_templates,prompts\nerror_states: auth_error\nregistered_session_invocations: {}\nnotes: local stdio, HTTP, SSE, and WebSocket MCP clients can initialize, list tools, call tools, list/read resources, list resource templates, list prompts, and get prompts; `kiana mcp serve` exposes Kiana tools/resources/resource templates/prompts over stdio and `kiana mcp-server-http`/`kiana mcp-server-ws` expose HTTP/SSE/WS.",
        invocations
    )))
}

fn command_context_argv(context: &CommandContext) -> Option<Vec<String>> {
    context
        .app_state
        .get(COMMAND_ARGV_APP_STATE_KEY)?
        .as_array()?
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn argv_tail_string(argv: &[String]) -> String {
    argv.get(1..).unwrap_or_default().join(" ")
}

fn usage() -> &'static str {
    "Usage: kiana mcp [status|serve|list|get <name>|add <name> -- <command> [args...]|add-json <name> <json> [--scope project]|add-from-claude-desktop [--scope project]|remove <name> [--scope project]|reset-project-choices]\n       kiana mcp serve [--debug] [--verbose]\n       kiana --mcp-config <file-or-json> mcp list"
}

fn list_mcp_servers(context: &CommandContext, query: &str) -> anyhow::Result<CommandResult> {
    let mut entries = configured_server_entries(context);
    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        entries.retain(|(name, value)| {
            name.to_lowercase().contains(&query)
                || serde_json::to_string(value)
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains(&query)
        });
    }

    if entries.is_empty() {
        let suffix = if query.is_empty() {
            "Use `kiana --mcp-config <file-or-json> mcp list` or install a plugin with mcpServers."
        } else {
            "No MCP servers matched the query."
        };
        return Ok(CommandResult::text(format!(
            "No MCP servers configured.\n{suffix}"
        )));
    }

    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = vec![format!("{} MCP server(s)", entries.len())];
    if !query.is_empty() {
        lines.push(format!("query: {}", query));
    }
    for (name, config) in entries {
        lines.push(format!(
            "- {} [{}] {}",
            name,
            server_transport(&config),
            server_summary(&config)
        ));
    }
    lines.push("usage: kiana mcp get <name>".into());
    Ok(CommandResult::text(lines.join("\n")))
}

fn get_mcp_server(context: &CommandContext, name: &str) -> anyhow::Result<CommandResult> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("usage: kiana mcp get <name>"));
    }
    let Some((entry_name, config)) = configured_server_entries(context)
        .into_iter()
        .find(|(entry_name, _)| entry_name == name)
    else {
        return Err(anyhow!("MCP server '{}' was not found", name));
    };
    Ok(CommandResult::text(serde_json::to_string_pretty(
        &server_config_with_name(&entry_name, config),
    )?))
}

fn add_mcp_server(rest: &str) -> anyhow::Result<CommandResult> {
    let parsed = parse_mcp_add_args(rest)?;
    add_mcp_server_parsed(parsed)
}

fn add_mcp_server_tokens(tokens: &[String]) -> anyhow::Result<CommandResult> {
    let parsed = parse_mcp_add_tokens(tokens)?;
    add_mcp_server_parsed(parsed)
}

fn add_mcp_server_parsed(parsed: ParsedMcpAdd) -> anyhow::Result<CommandResult> {
    validate_server_name(&parsed.name)?;

    let config = parsed.into_config()?;
    validate_server_config(
        config
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("MCP server"),
        &config,
    )?;

    let name = config
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("usage: {}", add_usage()))?;
    let transport = config
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| server_transport(&config));
    let mut stored_config = config
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow!("MCP server {name} config must be a JSON object"))?;
    stored_config.remove("name");
    let stored_config = Value::Object(stored_config);

    let mut servers = read_project_mcp_servers()?;
    if servers.contains_key(&name) {
        return Err(anyhow!("MCP server {name} already exists in .mcp.json"));
    }
    servers.insert(name.clone(), stored_config);
    write_project_mcp_servers(&servers)?;
    Ok(CommandResult::text(format!(
        "Added {} MCP server {} to project config\nFile modified: {}",
        mcp_add_transport_label(&transport),
        name,
        project_mcp_config_path()?.display()
    )))
}

fn add_json_mcp_server(rest: &str) -> anyhow::Result<CommandResult> {
    let (name, json_and_scope) = split_word(rest);
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("usage: kiana mcp add-json <name> <json>"))?;
    validate_server_name(name)?;
    let json_and_scope = json_and_scope.trim();
    if json_and_scope.is_empty() {
        return Err(anyhow!("usage: kiana mcp add-json <name> <json>"));
    }
    let (config, scope_tail) = parse_json_value_prefix(json_and_scope)
        .with_context(|| format!("MCP server {name} JSON config is invalid"))?;
    parse_project_scope_tail(scope_tail)?;
    validate_server_config(name, &config)?;

    let mut servers = read_project_mcp_servers()?;
    if servers.contains_key(name) {
        return Err(anyhow!("MCP server {name} already exists in .mcp.json"));
    }
    servers.insert(name.to_string(), config.clone());
    write_project_mcp_servers(&servers)?;
    Ok(CommandResult::text(format!(
        "Added {} MCP server {} to project config\nFile modified: {}",
        server_transport(&config),
        name,
        project_mcp_config_path()?.display()
    )))
}

fn add_from_claude_desktop(rest: &str) -> anyhow::Result<CommandResult> {
    let rest = rest.trim();
    if matches!(rest, "help" | "--help" | "-h") {
        return Ok(CommandResult::text(add_from_desktop_usage()));
    }
    parse_project_scope_tail_for(rest, "kiana mcp add-from-claude-desktop [--scope project]")?;

    let Some((source_path, desktop_servers)) = read_claude_desktop_mcp_servers()? else {
        return Ok(CommandResult::text(
            "No MCP servers found in Claude Desktop configuration or configuration file does not exist.",
        ));
    };
    if desktop_servers.is_empty() {
        return Ok(CommandResult::text(format!(
            "No MCP servers found in Claude Desktop configuration.\nSource: {}",
            source_path.display()
        )));
    }

    let mut project_servers = read_project_mcp_servers()?;
    let mut imported = 0usize;
    let mut skipped_existing = 0usize;
    for (name, config) in desktop_servers {
        validate_server_name(&name)?;
        validate_server_config(&name, &config)?;
        if project_servers.contains_key(&name) {
            skipped_existing += 1;
            continue;
        }
        project_servers.insert(name, config);
        imported += 1;
    }

    let path = if imported > 0 {
        write_project_mcp_servers(&project_servers)?
    } else {
        project_mcp_config_path()?
    };
    let mut lines = vec![format!(
        "Imported {imported} MCP server(s) from Claude Desktop to project config"
    )];
    if skipped_existing > 0 {
        lines.push(format!("Skipped {skipped_existing} existing MCP server(s)"));
    }
    lines.push(format!("Source: {}", source_path.display()));
    if imported > 0 {
        lines.push(format!("File modified: {}", path.display()));
    } else {
        lines.push(format!("File unchanged: {}", path.display()));
    }
    Ok(CommandResult::text(lines.join("\n")))
}

fn add_from_desktop_usage() -> &'static str {
    "Usage: kiana mcp add-from-claude-desktop [--scope project]\n       Import MCP servers from Claude Desktop into the current project's .mcp.json.\n       Set KIANA_CLAUDE_DESKTOP_CONFIG to import from an explicit config file."
}

#[derive(Debug, Default)]
struct ParsedMcpAdd {
    name: String,
    transport: Option<String>,
    command_or_url: Option<String>,
    args: Vec<String>,
    env: serde_json::Map<String, Value>,
    headers: serde_json::Map<String, Value>,
}

impl ParsedMcpAdd {
    fn into_config(self) -> anyhow::Result<Value> {
        let transport =
            self.transport.unwrap_or_else(|| {
                if self.command_or_url.as_deref().is_some_and(|value| {
                    value.starts_with("http://") || value.starts_with("https://")
                }) {
                    "http".to_string()
                } else {
                    "stdio".to_string()
                }
            });

        let Some(command_or_url) = self.command_or_url else {
            return Err(anyhow!("usage: {}", add_usage()));
        };

        let mut object = serde_json::Map::new();
        object.insert("name".to_string(), Value::String(self.name));
        object.insert("type".to_string(), Value::String(transport.clone()));
        match transport.as_str() {
            "stdio" => {
                if !self.headers.is_empty() {
                    return Err(anyhow!(
                        "MCP headers are only supported for http or sse servers"
                    ));
                }
                object.insert("command".to_string(), Value::String(command_or_url));
                if !self.args.is_empty() {
                    object.insert(
                        "args".to_string(),
                        Value::Array(self.args.into_iter().map(Value::String).collect()),
                    );
                }
                if !self.env.is_empty() {
                    object.insert("env".to_string(), Value::Object(self.env));
                }
            }
            "http" | "sse" => {
                if !self.args.is_empty() {
                    return Err(anyhow!(
                        "MCP {} servers do not take command arguments; use stdio transport for commands",
                        transport
                    ));
                }
                if !self.env.is_empty() {
                    return Err(anyhow!(
                        "MCP env variables are only supported for stdio servers"
                    ));
                }
                object.insert("url".to_string(), Value::String(command_or_url));
                if !self.headers.is_empty() {
                    object.insert("headers".to_string(), Value::Object(self.headers));
                }
            }
            _ => return Err(anyhow!("unsupported MCP add transport: {transport}")),
        }
        Ok(Value::Object(object))
    }
}

fn add_usage() -> &'static str {
    "kiana mcp add [--transport stdio|http|sse] [-e KEY=value] [-H Header:Value] [--scope project] <name> [--] <command-or-url> [args...]"
}

fn parse_mcp_add_args(rest: &str) -> anyhow::Result<ParsedMcpAdd> {
    let words = split_words(rest)
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    parse_mcp_add_tokens(&words)
}

fn parse_mcp_add_tokens(words: &[String]) -> anyhow::Result<ParsedMcpAdd> {
    if words.is_empty() {
        return Err(anyhow!("usage: {}", add_usage()));
    }

    let mut transport = None;
    let mut env = serde_json::Map::new();
    let mut headers = serde_json::Map::new();
    let mut positionals = Vec::new();
    let mut command_tail = Vec::new();
    let mut index = 0;

    while index < words.len() {
        let token = words[index].as_str();
        if token == "--" {
            command_tail = words[index + 1..]
                .iter()
                .map(|value| value.to_string())
                .collect();
            break;
        }

        if token == "--transport" || token == "-t" {
            index += 1;
            let value = words
                .get(index)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: {}", add_usage()))?;
            transport = Some(parse_mcp_add_transport(value)?.to_string());
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--transport=")
            .or_else(|| token.strip_prefix("-t="))
        {
            transport = Some(parse_mcp_add_transport(value)?.to_string());
            index += 1;
            continue;
        }

        if token == "--env" || token == "-e" {
            index += 1;
            let value = words
                .get(index)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: {}", add_usage()))?;
            insert_env_assignment(&mut env, value)?;
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--env=")
            .or_else(|| token.strip_prefix("-e="))
        {
            insert_env_assignment(&mut env, value)?;
            index += 1;
            continue;
        }

        if token == "--header" || token == "-H" {
            index += 1;
            let value = words
                .get(index)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: {}", add_usage()))?;
            insert_header_assignment(&mut headers, value)?;
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--header=")
            .or_else(|| token.strip_prefix("-H="))
        {
            insert_header_assignment(&mut headers, value)?;
            index += 1;
            continue;
        }

        if token == "--scope" || token == "-s" {
            index += 1;
            let value = words
                .get(index)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("usage: {}", add_usage()))?;
            ensure_project_scope(value)?;
            index += 1;
            continue;
        }
        if let Some(value) = token
            .strip_prefix("--scope=")
            .or_else(|| token.strip_prefix("-s="))
        {
            ensure_project_scope(value)?;
            index += 1;
            continue;
        }

        if is_unsupported_mcp_add_option(token) {
            return Err(anyhow!(
                "MCP add option '{}' is not implemented yet",
                token.split_once('=').map(|(flag, _)| flag).unwrap_or(token)
            ));
        }
        if token.starts_with('-') {
            return Err(anyhow!(
                "unsupported MCP add option '{}'\n\n{}",
                token,
                add_usage()
            ));
        }

        positionals.push(token.to_string());
        index += 1;
    }

    let name = positionals
        .first()
        .cloned()
        .ok_or_else(|| anyhow!("usage: {}", add_usage()))?;
    validate_server_name(&name)?;

    if !command_tail.is_empty() && positionals.len() > 1 {
        return Err(anyhow!(
            "usage: {}; put stdio command arguments after `--` or omit `--`, but not both",
            add_usage()
        ));
    }

    let (command_or_url, args) = if command_tail.is_empty() {
        let Some(command_or_url) = positionals.get(1).cloned() else {
            return Err(anyhow!("usage: {}", add_usage()));
        };
        (
            Some(command_or_url),
            positionals.into_iter().skip(2).collect::<Vec<_>>(),
        )
    } else {
        let Some(command_or_url) = command_tail.first().cloned() else {
            return Err(anyhow!("usage: {}", add_usage()));
        };
        (
            Some(command_or_url),
            command_tail.into_iter().skip(1).collect(),
        )
    };

    Ok(ParsedMcpAdd {
        name,
        transport,
        command_or_url,
        args,
        env,
        headers,
    })
}

fn parse_mcp_add_transport(value: &str) -> anyhow::Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "stdio" => Ok("stdio"),
        "http" => Ok("http"),
        "sse" => Ok("sse"),
        other => Err(anyhow!(
            "MCP add transport '{other}' is not implemented yet; supported transports: stdio, http, sse"
        )),
    }
}

fn insert_env_assignment(
    env: &mut serde_json::Map<String, Value>,
    assignment: &str,
) -> anyhow::Result<()> {
    let (key, value) = assignment
        .split_once('=')
        .ok_or_else(|| anyhow!("MCP env values must use KEY=value"))?;
    let key = key.trim();
    if key.is_empty() {
        return Err(anyhow!("MCP env key cannot be empty"));
    }
    env.insert(key.to_string(), Value::String(value.to_string()));
    Ok(())
}

fn insert_header_assignment(
    headers: &mut serde_json::Map<String, Value>,
    assignment: &str,
) -> anyhow::Result<()> {
    let (key, value) = assignment
        .split_once(':')
        .ok_or_else(|| anyhow!("MCP headers must use Header:Value"))?;
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() || value.is_empty() {
        return Err(anyhow!("MCP header name and value cannot be empty"));
    }
    headers.insert(key.to_string(), Value::String(value.to_string()));
    Ok(())
}

fn is_unsupported_mcp_add_option(token: &str) -> bool {
    matches!(
        token.split_once('=').map(|(flag, _)| flag).unwrap_or(token),
        "--client-id" | "--client-secret" | "--callback-port" | "--xaa"
    )
}

fn mcp_add_transport_label(transport: &str) -> &'static str {
    match transport {
        "http" => "HTTP",
        "sse" => "SSE",
        _ => "stdio",
    }
}

fn remove_mcp_server(rest: &str) -> anyhow::Result<CommandResult> {
    let (name, scope_tail) = split_word(rest);
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| anyhow!("usage: kiana mcp remove <name>"))?;
    parse_project_scope_tail(scope_tail)?;
    if name.is_empty() {
        return Err(anyhow!("usage: kiana mcp remove <name>"));
    }
    let mut servers = read_project_mcp_servers()?;
    if servers.remove(name).is_none() {
        return Err(anyhow!(
            "No MCP server found with name: {name} in .mcp.json"
        ));
    }
    write_project_mcp_servers(&servers)?;
    Ok(CommandResult::text(format!(
        "Removed MCP server {} from project config\nFile modified: {}",
        name,
        project_mcp_config_path()?.display()
    )))
}

fn reset_project_choices(rest: &str) -> anyhow::Result<CommandResult> {
    if !rest.trim().is_empty() {
        return Err(anyhow!("usage: kiana mcp reset-project-choices"));
    }
    let path = project_mcp_choices_path()?;
    let mut object = if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str::<Value>(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?
            .as_object()
            .cloned()
            .unwrap_or_default()
    } else {
        serde_json::Map::new()
    };
    object.insert("enabledMcpjsonServers".to_string(), Value::Array(vec![]));
    object.insert("disabledMcpjsonServers".to_string(), Value::Array(vec![]));
    object.insert("enableAllProjectMcpServers".to_string(), Value::Bool(false));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let contents = serde_json::to_string_pretty(&Value::Object(object))?;
    std::fs::write(&path, format!("{contents}\n"))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(CommandResult::text(format!(
        "All project-scoped (.mcp.json) server approvals and rejections have been reset.\nYou will be prompted for approval next time you start Kiana Code.\nFile modified: {}",
        path.display()
    )))
}

fn configured_server_entries(context: &CommandContext) -> Vec<(String, Value)> {
    let value = context
        .app_state
        .get(kiana_tools::mcp_tool::MCP_SERVERS_APP_STATE_KEY)
        .cloned()
        .or_else(kiana_tools::mcp_tool::configured_mcp_servers);
    let Some(value) = value else {
        return Vec::new();
    };
    server_entries(&value)
}

fn read_project_mcp_servers() -> anyhow::Result<serde_json::Map<String, Value>> {
    let path = project_mcp_config_path()?;
    if !path.exists() {
        return Ok(serde_json::Map::new());
    }
    let value = read_project_mcp_config_value(&path)?;
    let Some(servers) = value
        .get("mcpServers")
        .or_else(|| value.get(kiana_tools::mcp_tool::MCP_SERVERS_APP_STATE_KEY))
        .and_then(Value::as_object)
    else {
        return Ok(serde_json::Map::new());
    };
    Ok(servers.clone())
}

fn write_project_mcp_servers(servers: &serde_json::Map<String, Value>) -> anyhow::Result<PathBuf> {
    let path = project_mcp_config_path()?;
    let mut payload = if path.exists() {
        match read_project_mcp_config_value(&path)? {
            Value::Object(object) => object,
            _ => serde_json::Map::new(),
        }
    } else {
        serde_json::Map::new()
    };
    payload.insert("mcpServers".to_string(), Value::Object(servers.clone()));
    let contents = serde_json::to_string_pretty(&Value::Object(payload))?;
    std::fs::write(&path, format!("{contents}\n"))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn read_project_mcp_config_value(path: &std::path::Path) -> anyhow::Result<Value> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
}

fn project_mcp_config_path() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?.join(".mcp.json"))
}

fn project_mcp_choices_path() -> anyhow::Result<PathBuf> {
    Ok(std::env::current_dir()?
        .join(".kiana")
        .join("mcp-project-choices.json"))
}

fn read_claude_desktop_mcp_servers(
) -> anyhow::Result<Option<(PathBuf, serde_json::Map<String, Value>)>> {
    for path in claude_desktop_config_candidates() {
        if !path.exists() {
            continue;
        }
        let value = read_project_mcp_config_value(&path)?;
        let servers = value
            .get("mcpServers")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        return Ok(Some((path, servers)));
    }
    Ok(None)
}

fn claude_desktop_config_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os(CLAUDE_DESKTOP_CONFIG_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        candidates.push(path);
    }

    if cfg!(target_os = "macos") {
        if let Some(home) = home_dir() {
            candidates.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Claude")
                    .join("claude_desktop_config.json"),
            );
        }
    } else {
        if let Some(path) = windows_userprofile_claude_desktop_config() {
            candidates.push(path);
        }
        candidates.extend(wsl_user_claude_desktop_configs());
        if let Some(home) = home_dir() {
            candidates.push(
                home.join(".config")
                    .join("Claude")
                    .join("claude_desktop_config.json"),
            );
            candidates.push(
                home.join(".config")
                    .join("claude")
                    .join("claude_desktop_config.json"),
            );
        }
    }

    dedupe_paths(candidates)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

fn windows_userprofile_claude_desktop_config() -> Option<PathBuf> {
    let profile = std::env::var("USERPROFILE").ok()?;
    windows_path_to_wsl_path(&profile).map(|home| {
        home.join("AppData")
            .join("Roaming")
            .join("Claude")
            .join("claude_desktop_config.json")
    })
}

fn windows_path_to_wsl_path(path: &str) -> Option<PathBuf> {
    let normalized = path.replace('\\', "/");
    let mut chars = normalized.chars();
    let drive = chars.next()?;
    if chars.next()? != ':' {
        return None;
    }
    let rest = normalized.get(2..)?.trim_start_matches('/');
    Some(Path::new(&format!("/mnt/{}", drive.to_ascii_lowercase())).join(rest))
}

fn wsl_user_claude_desktop_configs() -> Vec<PathBuf> {
    let users_dir = Path::new("/mnt/c/Users");
    let Ok(entries) = std::fs::read_dir(users_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(
                name.as_ref(),
                "Public" | "Default" | "Default User" | "All Users"
            ) {
                return None;
            }
            Some(
                entry
                    .path()
                    .join("AppData")
                    .join("Roaming")
                    .join("Claude")
                    .join("claude_desktop_config.json"),
            )
        })
        .collect()
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

fn parse_json_value_prefix(input: &str) -> anyhow::Result<(Value, &str)> {
    let mut stream = serde_json::Deserializer::from_str(input).into_iter::<Value>();
    let value = stream
        .next()
        .ok_or_else(|| anyhow!("empty JSON config"))??;
    let consumed = stream.byte_offset();
    Ok((value, input[consumed..].trim()))
}

fn parse_project_scope_tail(tail: &str) -> anyhow::Result<()> {
    parse_project_scope_tail_for(tail, "kiana mcp add-json <name> <json> --scope project")
}

fn parse_project_scope_tail_for(tail: &str, usage_text: &str) -> anyhow::Result<()> {
    let tail = tail.trim();
    if tail.is_empty() {
        return Ok(());
    }
    let (flag, rest) = split_word(tail);
    let Some(flag) = flag else {
        return Ok(());
    };
    let (scope, rest) = match flag {
        "--scope" | "-s" => split_word(rest),
        value if value.starts_with("--scope=") => (Some(&value["--scope=".len()..]), rest),
        value if value.starts_with("-s=") => (Some(&value["-s=".len()..]), rest),
        _ => {
            return Err(anyhow!(
                "unsupported MCP scope arguments: {tail}\n\n{}",
                usage_text
            ))
        }
    };
    let scope = scope
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .ok_or_else(|| anyhow!("usage: {usage_text}"))?;
    ensure_project_scope(scope)?;
    if !rest.trim().is_empty() {
        return Err(anyhow!("unexpected MCP scope arguments: {}", rest.trim()));
    }
    Ok(())
}

fn ensure_project_scope(scope: &str) -> anyhow::Result<()> {
    if scope == "project" {
        return Ok(());
    }
    Err(anyhow!(
        "MCP config scope '{scope}' is not implemented yet; only project (.mcp.json) is supported"
    ))
}

fn validate_server_name(name: &str) -> anyhow::Result<()> {
    if name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(anyhow!("MCP server name contains unsafe path characters"));
    }
    Ok(())
}

fn validate_server_config(name: &str, config: &Value) -> anyhow::Result<()> {
    let Some(object) = config.as_object() else {
        return Err(anyhow!("MCP server {name} config must be a JSON object"));
    };
    let has_command = object
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_url = object
        .get("url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_transport = object
        .get("transport")
        .or_else(|| object.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    if !has_command && !has_url && !has_transport {
        return Err(anyhow!(
            "MCP server {name} config requires command, url, type, or transport"
        ));
    }
    Ok(())
}

fn server_entries(value: &Value) -> Vec<(String, Value)> {
    match value {
        Value::Object(object) => object
            .iter()
            .filter(|(name, _)| !name.trim().is_empty())
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())?;
                Some((name.to_string(), item.clone()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn server_config_with_name(name: &str, config: Value) -> Value {
    let Value::Object(mut object) = config else {
        return config;
    };
    object
        .entry("name".to_string())
        .or_insert_with(|| Value::String(name.to_string()));
    Value::Object(object)
}

fn server_transport(config: &Value) -> String {
    if config
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return "stdio".to_string();
    }
    let Some(url) = config.get("url").and_then(Value::as_str) else {
        return config
            .get("transport")
            .or_else(|| config.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
    };
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("ws://") || lower.starts_with("wss://") {
        "ws".to_string()
    } else if lower.contains("/sse") {
        "sse".to_string()
    } else {
        "http".to_string()
    }
}

fn server_summary(config: &Value) -> String {
    if let Some(url) = config
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return url.to_string();
    }
    if let Some(command) = config
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let args = config
            .get("args")
            .or_else(|| config.get("commandArgs"))
            .or_else(|| config.get("command_args"))
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|args| !args.is_empty());
        return match args {
            Some(args) => format!("{command} {args}"),
            None => command.to_string(),
        };
    }
    serde_json::to_string(config).unwrap_or_else(|_| "{}".to_string())
}

fn split_word(input: &str) -> (Option<&str>, &str) {
    let input = input.trim();
    if input.is_empty() {
        return (None, "");
    }
    match input.find(char::is_whitespace) {
        Some(index) => (Some(&input[..index]), input[index..].trim()),
        None => (Some(input), ""),
    }
}

fn split_words(input: &str) -> Vec<&str> {
    input.split_whitespace().collect()
}

#[cfg(test)]
mod tests {
    use super::McpCommand;
    use crate::{Command, CommandContext};
    use serde_json::json;
    use std::collections::HashMap;

    #[tokio::test]
    async fn mcp_reports_recorded_invocations() {
        let mut app_state = HashMap::new();
        app_state.insert("mcp_invocations".to_string(), json!([{ "tool_name": "x" }]));

        let result = McpCommand
            .execute(CommandContext {
                args: String::new(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result.value.contains("registered_session_invocations: 1"));
        assert!(result.value.contains("client_http: wired"));
        assert!(result.value.contains("client_sse: wired"));
        assert!(result.value.contains("client_ws: wired"));
        assert!(result
            .value
            .contains("protocol_surfaces: tools,resources,resource_templates,prompts"));
        assert!(result.value.contains("error_states: auth_error"));
    }

    #[tokio::test]
    async fn mcp_lists_configured_servers() {
        let mut app_state = HashMap::new();
        app_state.insert(
            "mcp_servers".to_string(),
            json!({
                "docs": {
                    "url": "http://127.0.0.1:9000/mcp"
                },
                "shell": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }),
        );

        let result = McpCommand
            .execute(CommandContext {
                args: "list".to_string(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result.value.contains("2 MCP server(s)"));
        assert!(result.value.contains("docs"));
        assert!(result.value.contains("http://127.0.0.1:9000/mcp"));
        assert!(result.value.contains("shell"));
        assert!(result.value.contains("node server.js"));
    }

    #[tokio::test]
    async fn mcp_gets_configured_server_json() {
        let mut app_state = HashMap::new();
        app_state.insert(
            "mcp_servers".to_string(),
            json!({
                "docs": {
                    "url": "http://127.0.0.1:9000/mcp"
                }
            }),
        );

        let result = McpCommand
            .execute(CommandContext {
                args: "get docs".to_string(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result.value.contains("\"name\": \"docs\""));
        assert!(result
            .value
            .contains("\"url\": \"http://127.0.0.1:9000/mcp\""));
    }

    #[tokio::test]
    async fn mcp_add_json_writes_project_mcp_config_and_list_reads_it() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-add-json");
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let add = McpCommand
            .execute(CommandContext {
                args: r#"add-json docs {"command":"node","args":["server.js"]}"#.to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(add.value.contains("Added stdio MCP server docs"));
        let config = std::fs::read_to_string(root.join(".mcp.json")).unwrap();
        assert!(config.contains("\"mcpServers\""));
        assert!(config.contains("\"docs\""));

        let list = McpCommand
            .execute(CommandContext {
                args: "list".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(list.value.contains("docs"));
        assert!(list.value.contains("node server.js"));

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_list_reads_parent_project_mcp_json_from_subdirectory() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-parent-project");
        let child = root.join("packages").join("app");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"docs":{"command":"node","args":["root-server.js"]}}}"#,
        )
        .unwrap();
        std::env::set_current_dir(&child).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let list = McpCommand
            .execute(CommandContext {
                args: "list".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(list.value.contains("docs"), "{}", list.value);
        assert!(list.value.contains("node root-server.js"), "{}", list.value);

        let get = McpCommand
            .execute(CommandContext {
                args: "get docs".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        assert!(get.value.contains("\"command\": \"node\""), "{}", get.value);

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_project_config_writes_stay_in_current_directory() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-current-project-writes");
        let child = root.join("packages").join("app");
        std::fs::create_dir_all(&child).unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"docs":{"command":"root-docs"}}}"#,
        )
        .unwrap();
        std::env::set_current_dir(&child).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let add = McpCommand
            .execute(CommandContext {
                args: "add docs -- node child-server.js".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        assert!(add.value.contains("Added stdio MCP server docs"));

        let child_config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(child.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(child_config["mcpServers"]["docs"]["command"], "node");

        let parent_config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(parent_config["mcpServers"]["docs"]["command"], "root-docs");

        let remove = McpCommand
            .execute(CommandContext {
                args: "remove docs".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        assert!(remove.value.contains("Removed MCP server docs"));
        let parent_config_after_remove: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(
            parent_config_after_remove["mcpServers"]["docs"]["command"],
            "root-docs"
        );

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_add_json_accepts_reference_project_scope_flag() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-add-json-scope");
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let add = McpCommand
            .execute(CommandContext {
                args: r#"add-json docs {"command":"node","args":["server.js"]} --scope project"#
                    .to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(add.value.contains("Added stdio MCP server docs"));
        assert!(add.value.contains("project config"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config["mcpServers"]["docs"]["command"], "node");

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_add_writes_stdio_project_mcp_config() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-add-stdio");
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let result = McpCommand
            .execute(CommandContext {
                args: "add -e API_KEY=abc docs -- node server.js --watch".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Added stdio MCP server docs"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config["mcpServers"]["docs"]["type"], "stdio");
        assert_eq!(config["mcpServers"]["docs"]["command"], "node");
        assert_eq!(
            config["mcpServers"]["docs"]["args"],
            json!(["server.js", "--watch"])
        );
        assert_eq!(config["mcpServers"]["docs"]["env"]["API_KEY"], "abc");

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_add_writes_http_project_mcp_config_with_headers() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-add-http");
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let result = McpCommand
            .execute(CommandContext {
                args: "add --transport http docs https://example.test/mcp --header X-Api-Key:abc --scope project".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Added HTTP MCP server docs"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config["mcpServers"]["docs"]["type"], "http");
        assert_eq!(
            config["mcpServers"]["docs"]["url"],
            "https://example.test/mcp"
        );
        assert_eq!(config["mcpServers"]["docs"]["headers"]["X-Api-Key"], "abc");

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_remove_updates_project_mcp_config() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-remove");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"docs":{"url":"http://127.0.0.1:9000/mcp"},"shell":{"command":"node"}}}"#,
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        let result = McpCommand
            .execute(CommandContext {
                args: "remove docs".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Removed MCP server docs"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert!(config["mcpServers"].get("docs").is_none());
        assert_eq!(config["mcpServers"]["shell"]["command"], "node");

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_project_config_edits_preserve_unrelated_top_level_fields() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-preserve-fields");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"metadata":{"owner":"team"},"mcpServers":{"docs":{"command":"node"}}}"#,
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);

        McpCommand
            .execute(CommandContext {
                args: "add shell -- bash server.sh".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        let config_after_add: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config_after_add["metadata"]["owner"], "team");
        assert_eq!(config_after_add["mcpServers"]["shell"]["command"], "bash");

        McpCommand
            .execute(CommandContext {
                args: "remove docs".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        let config_after_remove: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config_after_remove["metadata"]["owner"], "team");
        assert!(config_after_remove["mcpServers"].get("docs").is_none());
        assert_eq!(
            config_after_remove["mcpServers"]["shell"]["command"],
            "bash"
        );

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_reset_project_choices_clears_project_mcp_approval_state() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let root = temp_project("mcp-reset-project-choices");
        std::fs::create_dir_all(root.join(".kiana")).unwrap();
        std::fs::write(
            root.join(".kiana").join("mcp-project-choices.json"),
            r#"{"enabledMcpjsonServers":["docs"],"disabledMcpjsonServers":["shell"],"enableAllProjectMcpServers":true,"other":"kept"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"docs":{"command":"node"}}}"#,
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();

        let result = McpCommand
            .execute(CommandContext {
                args: "reset-project-choices".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("project-scoped (.mcp.json) server approvals"));
        let choices: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".kiana").join("mcp-project-choices.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(choices["enabledMcpjsonServers"], json!([]));
        assert_eq!(choices["disabledMcpjsonServers"], json!([]));
        assert_eq!(choices["enableAllProjectMcpServers"], json!(false));
        assert_eq!(choices["other"], json!("kept"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config["mcpServers"]["docs"]["command"], "node");

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn mcp_rejects_unknown_args_instead_of_returning_status() {
        let result = McpCommand
            .execute(CommandContext {
                args: "server".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn mcp_help_mentions_reference_style_serve_entrypoint() {
        let result = McpCommand
            .execute(CommandContext {
                args: "--help".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("serve"), "{}", result.value);
        assert!(
            result
                .value
                .contains("kiana mcp serve [--debug] [--verbose]"),
            "{}",
            result.value
        );
    }

    #[tokio::test]
    async fn mcp_imports_claude_desktop_servers_to_project_config() {
        let _guard = crate::local_state::env_lock().lock().unwrap();
        let previous_cwd = std::env::current_dir().unwrap();
        let previous_desktop_config = std::env::var_os("KIANA_CLAUDE_DESKTOP_CONFIG");
        let root = temp_project("mcp-add-from-desktop");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("claude_desktop_config.json"),
            r#"{"mcpServers":{"docs":{"command":"existing-docs"},"shell":{"command":"node","args":["server.js"]}}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"other":"kept","mcpServers":{"docs":{"command":"existing-docs"}}}"#,
        )
        .unwrap();
        std::env::set_current_dir(&root).unwrap();
        std::env::set_var(
            "KIANA_CLAUDE_DESKTOP_CONFIG",
            root.join("claude_desktop_config.json"),
        );

        let result = McpCommand
            .execute(CommandContext {
                args: "add-from-claude-desktop --scope project".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Imported 1 MCP server(s)"));
        assert!(result.value.contains("Skipped 1 existing MCP server(s)"));
        let config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(config["other"], json!("kept"));
        assert_eq!(config["mcpServers"]["docs"]["command"], "existing-docs");
        assert_eq!(config["mcpServers"]["shell"]["command"], "node");
        assert_eq!(config["mcpServers"]["shell"]["args"], json!(["server.js"]));

        std::env::set_current_dir(previous_cwd).unwrap();
        match previous_desktop_config {
            Some(value) => std::env::set_var("KIANA_CLAUDE_DESKTOP_CONFIG", value),
            None => std::env::remove_var("KIANA_CLAUDE_DESKTOP_CONFIG"),
        }
        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_project(label: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kiana-{label}-{}-{unique}", std::process::id()))
    }
}
