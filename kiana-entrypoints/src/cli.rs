use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_bridge::{
    BridgeApiClient, BridgeAuthProvider, BridgeConfig, CommandBridgeSessionRunner, SessionManager,
    SpawnMode, WorkPollLoop,
};
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_commands::{
    create_default_command_registry, CommandContext, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};
use kiana_daemon::DaemonHost;
use kiana_protocol::{
    ApprovalDecision, ApprovalId, ExecutionStatus as ControlPlaneStatus,
    RequestEnvelope as ControlPlaneRequest, RequestMetadata as ControlPlaneMetadata,
    ResponseEnvelope as ControlPlaneResponse,
};
use kiana_query::{
    build_context_artifact_dependency_graph, build_context_artifact_readiness,
    build_context_artifact_store, build_context_artifacts, build_context_index, build_context_pack,
    build_persistent_context_artifact_store, build_persistent_context_artifacts,
    build_persistent_context_index, build_repo_map, ingest_context_artifacts, search_context_index,
    search_context_vectors, ContextArtifactIngestOptions, ContextArtifactOptions,
    ContextIndexOptions, ContextPackOptions, ContextSearchOptions, ContextVectorSearchOptions,
    RepoMapOptions,
};
use kiana_screens::{history::HistoryEntry, settings::SettingsSection};
use kiana_tools::permissions::effective_tool_permissions;
use kiana_tools::tool_execution::{
    PermissionPromptDecision, PermissionPromptHandler, PermissionPromptRequest,
};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, http::header::AUTHORIZATION, http::HeaderValue, Message,
};

pub async fn main() -> Result<()> {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    main_with_args(raw_args).await
}

async fn main_with_args(raw_args: Vec<String>) -> Result<()> {
    let (args, runtime_flags) = extract_runtime_flags(raw_args)?;
    apply_runtime_flags(&runtime_flags)?;

    if args.len() == 1 && matches!(args[0].as_str(), "--version" | "-v" | "-V" | "version") {
        println!("{} (Kiana Code)", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.is_empty() {
        return cli_main_with_terminal(
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
        )
        .await;
    }

    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h" | "help") {
        print_help();
        return Ok(());
    }

    if args.first().map(String::as_str) == Some("architecture") {
        return architecture_main(&args).await;
    }

    if is_print_mode_help(&args) {
        print_print_help();
        return Ok(());
    }

    if is_resume_mode_help(&args) {
        print_resume_help();
        return Ok(());
    }

    if let Some(resume_args) = parse_resume_cli_args(&args)? {
        return resume_cli_main(resume_args, &runtime_flags).await;
    }

    if let Some(print_args) = parse_print_args(&args)? {
        return print_main(print_args, &runtime_flags).await;
    }

    if runtime_flags.no_session_persistence {
        return Err(anyhow!(
            "--no-session-persistence can only be used with --print"
        ));
    }

    if args.first().map(|s| s.as_str()) == Some("--dump-system-prompt") {
        return dump_system_prompt(&args).await;
    }

    if args.first().map(|s| s.as_str()) == Some("--daemon-worker") {
        return run_daemon_worker(args.get(1).map(String::as_str), &args[2..]).await;
    }

    if args.first().map(|s| s.as_str()) == Some("--claude-in-chrome-mcp") {
        return kiana_chrome_mcp::run_stdio().await;
    }

    if args.first().map(|s| s.as_str()) == Some("--chrome-native-host") {
        return kiana_chrome_mcp::run_native_host_stdio().await;
    }

    if is_computer_mcp_stdio_command(&args) {
        if is_help_at(&args, 1) {
            print_computer_mcp_help();
            return Ok(());
        }
        return kiana_computer_mcp::run_stdio().await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some("chrome" | "chrome-native-host")
    ) {
        return chrome_main(&args).await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some("url" | "deeplink" | "deep-link")
    ) {
        return url_main(&args).await;
    }

    if is_direct_connect_open_command(&args) {
        return direct_connect_open_main(&args).await;
    }

    if args.first().map(|s| s.as_str()) == Some("server") {
        return direct_connect_server_main(&args).await;
    }

    if args.first().map(|s| s.as_str()) == Some("agents") {
        return agents_main(&args, &runtime_flags).await;
    }

    if args.first().map(|s| s.as_str()) == Some("tui") {
        return crate::tui::run_tui().await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some("remote-session" | "remote-sessions" | "rsession")
    ) {
        return remote_session_main(&args).await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some(
            "mcp-server"
                | "mcp-server-stdio"
                | "mcp-server-http"
                | "mcp-server-sse"
                | "mcp-server-ws"
                | "--mcp-server"
        )
    ) {
        return mcp_server_main(&args).await;
    }

    if is_mcp_serve_command(&args) {
        return mcp_serve_main(&args).await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some("remote-control" | "rc" | "remote" | "sync" | "bridge")
    ) {
        return bridge_main(&args).await;
    }

    if args.first().map(|s| s.as_str()) == Some("daemon") {
        return daemon_main(&args).await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some("ps" | "logs" | "attach" | "kill")
    ) || args.contains(&"--bg".to_string())
        || args.contains(&"--background".to_string())
    {
        return bg_handler(&args).await;
    }

    if matches!(
        args.first().map(|s| s.as_str()),
        Some("new" | "list" | "reply" | "show" | "rename" | "tag" | "fork")
    ) {
        return session_main(&args).await;
    }

    if args.first().map(|s| s.as_str()) == Some("session") {
        return session_main(&args[1..]).await;
    }

    if let Some(result) = run_local_command(&args, &runtime_flags).await? {
        if !result.value.is_empty() {
            println!("{}", result.value);
        }
        return Ok(());
    }

    Err(anyhow!(
        "unknown command or option: {}\n\nRun `kiana --help` for usage.",
        args[0]
    ))
}

fn is_computer_mcp_stdio_command(args: &[String]) -> bool {
    matches!(
        args.first().map(|s| s.as_str()),
        Some("computer-mcp" | "computer-use-mcp" | "--computer-mcp" | "--computer-use-mcp")
    )
}

struct LocalDaemonTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for LocalDaemonTransport {
    async fn send(
        &self,
        request: ControlPlaneRequest,
    ) -> Result<ControlPlaneResponse, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

async fn architecture_main(args: &[String]) -> Result<()> {
    if args.len() == 2 && matches!(args[1].as_str(), "help" | "--help" | "-h") {
        println!("Usage: kiana architecture status [--json]");
        return Ok(());
    }
    if args.len() < 2 || args[1] != "status" {
        return Err(anyhow!("usage: kiana architecture status [--json]"));
    }
    let mut json_output = false;
    for argument in &args[2..] {
        match argument.as_str() {
            "--json" => json_output = true,
            "--help" | "-h" => {
                println!("Usage: kiana architecture status [--json]");
                return Ok(());
            }
            _ => return Err(anyhow!("unknown architecture status option: {argument}")),
        }
    }

    let project_root = std::env::current_dir()
        .context("failed to resolve current directory")?
        .to_string_lossy()
        .into_owned();
    let mut metadata = ControlPlaneMetadata::local("architecture-status", project_root);
    metadata.project_trusted = true;
    metadata.actor_id = Some("local-cli".to_owned());
    let client = KianaClient::new(LocalDaemonTransport {
        host: Arc::new(DaemonHost::local()?),
    });
    let response = client
        .command(metadata, "system.architecture", Value::Null)
        .await
        .map_err(anyhow::Error::msg)?;
    if response.status != ControlPlaneStatus::Completed {
        return Err(anyhow!(
            "architecture status blocked: {}",
            response.error.as_deref().unwrap_or("unknown")
        ));
    }
    if json_output {
        println!("{}", serde_json::to_string_pretty(&response.output)?);
    } else {
        println!(
            "Control plane: {}",
            response.output["control_plane"]
                .as_str()
                .unwrap_or("unknown")
        );
        println!(
            "Composition root: {}",
            response.output["composition_root"]
                .as_str()
                .unwrap_or("unknown")
        );
        println!(
            "Legacy edges remaining: {}",
            response.output["legacy_edges_remaining"]
                .as_u64()
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn is_direct_connect_open_command(args: &[String]) -> bool {
    matches!(args.first().map(|s| s.as_str()), Some("open"))
        || args
            .first()
            .is_some_and(|arg| arg.starts_with("cc://") || arg.starts_with("cc+unix://"))
}

fn is_print_mode_help(args: &[String]) -> bool {
    let mut index = 0;
    let mut saw_print = false;

    while let Some(arg) = args.get(index).map(String::as_str) {
        if is_help_arg(arg) {
            return saw_print;
        }

        match arg {
            "-p" | "--print" => {
                saw_print = true;
                index += 1;
            }
            _ if arg.starts_with("--print=") => {
                return arg.strip_prefix("--print=").is_some_and(is_help_arg);
            }
            "--" => return false,
            _ if is_print_value_flag(arg) => {
                index += 2;
            }
            _ if is_print_assignment_flag(arg) || is_print_boolean_flag(arg) => {
                index += 1;
            }
            _ => return false,
        }
    }

    false
}

fn is_resume_mode_help(args: &[String]) -> bool {
    let mut index = 0;
    let mut saw_resume_mode = false;

    while let Some(arg) = args.get(index).map(String::as_str) {
        if is_help_arg(arg) {
            return saw_resume_mode;
        }

        match arg {
            "-p" | "--print" => {
                index += 1;
            }
            _ if arg.starts_with("--print=") => return false,
            "-c" | "--continue" => {
                saw_resume_mode = true;
                index += 1;
            }
            "-r" | "--resume" => {
                saw_resume_mode = true;
                let has_session_id = args
                    .get(index + 1)
                    .is_some_and(|value| !value.starts_with('-'));
                index += if has_session_id { 2 } else { 1 };
            }
            _ if arg.starts_with("--resume=") => {
                saw_resume_mode = true;
                index += 1;
            }
            "--" => return false,
            _ if is_resume_value_flag(arg) => {
                index += 2;
            }
            _ if is_resume_assignment_flag(arg) || is_resume_boolean_flag(arg) => {
                index += 1;
            }
            _ => return false,
        }
    }

    false
}

fn is_print_value_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--input-format" | "--output-format" | "--json-schema" | "--sdk-url" | "--debug-file"
    )
}

fn is_print_assignment_flag(arg: &str) -> bool {
    [
        "--input-format=",
        "--output-format=",
        "--json-schema=",
        "--sdk-url=",
        "--debug-file=",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
}

fn is_print_boolean_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--record-only"
            | "--execute"
            | "--resident-teammate"
            | "--replay-user-messages"
            | "--include-partial-messages"
    )
}

fn is_resume_value_flag(arg: &str) -> bool {
    matches!(arg, "--output-format" | "--json-schema")
}

fn is_resume_assignment_flag(arg: &str) -> bool {
    ["--output-format=", "--json-schema="]
        .iter()
        .any(|prefix| arg.starts_with(prefix))
}

fn is_resume_boolean_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--fork-session" | "--record-only" | "--execute" | "--include-partial-messages"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrintOutputFormat {
    Text,
    Json,
    StreamJson,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrintInputFormat {
    Text,
    StreamJson,
}

#[derive(Debug, PartialEq, Eq)]
struct PrintArgs {
    message: String,
    input_format: PrintInputFormat,
    output_format: PrintOutputFormat,
    execute: bool,
    resident_teammate: bool,
    json_schema: Option<Value>,
    sdk_url: Option<String>,
    replay_user_messages: bool,
    include_partial_messages: bool,
}

fn parse_print_args(args: &[String]) -> Result<Option<PrintArgs>> {
    if args.is_empty() {
        return Ok(None);
    };

    let mut input_format = PrintInputFormat::Text;
    let mut output_format = PrintOutputFormat::Text;
    let mut execute = true;
    let mut resident_teammate = false;
    let mut json_schema = None;
    let mut sdk_url = None;
    let mut replay_user_messages = false;
    let mut include_partial_messages = false;
    let mut initial_prompt = None;
    let mut index = 0;

    loop {
        let Some(arg) = args.get(index).map(String::as_str) else {
            return Ok(None);
        };
        match arg {
            "-p" | "--print" => {
                index += 1;
                break;
            }
            _ if arg.starts_with("--print=") => {
                initial_prompt = arg.strip_prefix("--print=").map(str::to_string);
                index += 1;
                break;
            }
            _ => {
                let before = index;
                parse_print_flags(
                    args,
                    &mut index,
                    &mut input_format,
                    &mut output_format,
                    &mut execute,
                    &mut resident_teammate,
                    &mut json_schema,
                    &mut sdk_url,
                    &mut replay_user_messages,
                    &mut include_partial_messages,
                )?;
                if index == before {
                    return Ok(None);
                }
            }
        }
    }

    parse_print_flags(
        args,
        &mut index,
        &mut input_format,
        &mut output_format,
        &mut execute,
        &mut resident_teammate,
        &mut json_schema,
        &mut sdk_url,
        &mut replay_user_messages,
        &mut include_partial_messages,
    )?;
    if args.get(index).map(String::as_str) == Some("--") {
        index += 1;
    }

    let mut message_parts = Vec::new();
    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        message_parts.push(prompt);
    }
    message_parts.extend(args.get(index..).unwrap_or_default().iter().cloned());
    let message = message_parts.join(" ");
    if message.trim().is_empty() && matches!(input_format, PrintInputFormat::Text) {
        return Err(anyhow!("usage: kiana -p [--json-schema <schema>] <prompt>"));
    }

    validate_print_io_flags(
        &input_format,
        &output_format,
        sdk_url.as_deref(),
        replay_user_messages,
        include_partial_messages,
    )?;

    Ok(Some(PrintArgs {
        message,
        input_format,
        output_format,
        execute,
        resident_teammate,
        json_schema,
        sdk_url,
        replay_user_messages,
        include_partial_messages,
    }))
}

fn parse_print_flags(
    args: &[String],
    index: &mut usize,
    input_format: &mut PrintInputFormat,
    output_format: &mut PrintOutputFormat,
    execute: &mut bool,
    resident_teammate: &mut bool,
    json_schema: &mut Option<Value>,
    sdk_url: &mut Option<String>,
    replay_user_messages: &mut bool,
    include_partial_messages: &mut bool,
) -> Result<()> {
    while let Some(arg) = args.get(*index).map(String::as_str) {
        match arg {
            "--input-format" => {
                let value = args
                    .get(*index + 1)
                    .ok_or_else(|| anyhow!("--input-format requires a value"))?;
                *input_format = parse_print_input_format(value)?;
                *index += 2;
                continue;
            }
            _ if arg.starts_with("--input-format=") => {
                let value = arg
                    .strip_prefix("--input-format=")
                    .ok_or_else(|| anyhow!("--input-format requires a value"))?;
                *input_format = parse_print_input_format(value)?;
            }
            "--output-format" => {
                let value = args
                    .get(*index + 1)
                    .ok_or_else(|| anyhow!("--output-format requires a value"))?;
                *output_format = parse_print_output_format(value)?;
                *index += 2;
                continue;
            }
            _ if arg.starts_with("--output-format=") => {
                let value = arg
                    .strip_prefix("--output-format=")
                    .ok_or_else(|| anyhow!("--output-format requires a value"))?;
                *output_format = parse_print_output_format(value)?;
            }
            "--json-schema" => {
                let value = args
                    .get(*index + 1)
                    .ok_or_else(|| anyhow!("--json-schema requires a JSON schema value"))?;
                *json_schema = Some(parse_json_schema_flag(value)?);
                *index += 2;
                continue;
            }
            _ if arg.starts_with("--json-schema=") => {
                let value = arg
                    .strip_prefix("--json-schema=")
                    .ok_or_else(|| anyhow!("--json-schema requires a JSON schema value"))?;
                *json_schema = Some(parse_json_schema_flag(value)?);
            }
            "--sdk-url" => {
                let value = args
                    .get(*index + 1)
                    .ok_or_else(|| anyhow!("--sdk-url requires a value"))?;
                *sdk_url = Some(nonempty_flag_value("--sdk-url", value)?);
                *index += 2;
                continue;
            }
            _ if arg.starts_with("--sdk-url=") => {
                let value = arg
                    .strip_prefix("--sdk-url=")
                    .ok_or_else(|| anyhow!("--sdk-url requires a value"))?;
                *sdk_url = Some(nonempty_flag_value("--sdk-url", value)?);
            }
            "--debug-file" => {
                let value = args
                    .get(*index + 1)
                    .ok_or_else(|| anyhow!("--debug-file requires a value"))?;
                let _ = nonempty_flag_value("--debug-file", value)?;
                *index += 2;
                continue;
            }
            _ if arg.starts_with("--debug-file=") => {
                let value = arg
                    .strip_prefix("--debug-file=")
                    .ok_or_else(|| anyhow!("--debug-file requires a value"))?;
                let _ = nonempty_flag_value("--debug-file", value)?;
            }
            "--record-only" => *execute = false,
            "--execute" => *execute = true,
            "--resident-teammate" => *resident_teammate = true,
            "--replay-user-messages" => *replay_user_messages = true,
            "--include-partial-messages" => *include_partial_messages = true,
            _ => break,
        }
        *index += 1;
    }
    Ok(())
}

fn parse_print_input_format(value: &str) -> Result<PrintInputFormat> {
    match value {
        "text" => Ok(PrintInputFormat::Text),
        "stream-json" => Ok(PrintInputFormat::StreamJson),
        _ => Err(anyhow!("--input-format must be text or stream-json")),
    }
}

fn parse_print_output_format(value: &str) -> Result<PrintOutputFormat> {
    match value {
        "text" => Ok(PrintOutputFormat::Text),
        "json" => Ok(PrintOutputFormat::Json),
        "stream-json" => Ok(PrintOutputFormat::StreamJson),
        _ => Err(anyhow!(
            "--output-format must be text, json, or stream-json"
        )),
    }
}

fn validate_print_io_flags(
    input_format: &PrintInputFormat,
    output_format: &PrintOutputFormat,
    sdk_url: Option<&str>,
    replay_user_messages: bool,
    include_partial_messages: bool,
) -> Result<()> {
    if sdk_url.is_some()
        && (!matches!(input_format, PrintInputFormat::StreamJson)
            || !matches!(output_format, PrintOutputFormat::StreamJson))
    {
        return Err(anyhow!(
            "--sdk-url requires both --input-format=stream-json and --output-format=stream-json"
        ));
    }
    if matches!(input_format, PrintInputFormat::StreamJson)
        && !matches!(output_format, PrintOutputFormat::StreamJson)
    {
        return Err(anyhow!(
            "--input-format=stream-json requires --output-format=stream-json"
        ));
    }
    if replay_user_messages
        && (!matches!(input_format, PrintInputFormat::StreamJson)
            || !matches!(output_format, PrintOutputFormat::StreamJson))
    {
        return Err(anyhow!(
            "--replay-user-messages requires both --input-format=stream-json and --output-format=stream-json"
        ));
    }
    if include_partial_messages && !matches!(output_format, PrintOutputFormat::StreamJson) {
        return Err(anyhow!(
            "--include-partial-messages requires --output-format=stream-json"
        ));
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
struct StreamJsonInput {
    prompt: String,
    replay_events: Vec<Value>,
    history_messages: Vec<Value>,
}

async fn read_stream_json_input<R>(
    reader: R,
    cli_prompt: &str,
    replay_user_messages: bool,
) -> Result<StreamJsonInput>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut lines = reader.lines();
    let mut raw = String::new();
    while let Some(line) = lines.next_line().await? {
        raw.push_str(&line);
        raw.push('\n');
    }
    parse_stream_json_input(&raw, cli_prompt, replay_user_messages)
}

fn parse_stream_json_input(
    raw: &str,
    cli_prompt: &str,
    replay_user_messages: bool,
) -> Result<StreamJsonInput> {
    let mut prompt_parts = Vec::new();
    if !cli_prompt.trim().is_empty() {
        prompt_parts.push(cli_prompt.trim().to_string());
    }

    let mut replay_events = Vec::new();
    let mut history_messages = Vec::new();
    for (line_index, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: Value = serde_json::from_str(line).map_err(|error| {
            anyhow!(
                "--input-format=stream-json line {} is not valid JSON: {}",
                line_index + 1,
                error
            )
        })?;
        match event.get("type").and_then(Value::as_str) {
            Some("user") => {
                if replay_user_messages {
                    replay_events.push(event.clone());
                }
                if is_human_user_event(&event) {
                    if let Some(text) = user_event_text(&event).filter(|text| !text.is_empty()) {
                        prompt_parts.push(text);
                    }
                }
            }
            Some("assistant") => {
                if replay_user_messages {
                    replay_events.push(event.clone());
                }
                if let Some(message) = stream_json_history_message(&event) {
                    history_messages.push(message);
                }
            }
            Some("control_response") => {
                if replay_user_messages {
                    replay_events.push(event.clone());
                }
            }
            Some("control_request") => {
                if is_end_session_control_request(&event) {
                    break;
                }
            }
            Some(
                "system" | "control_cancel_request" | "keep_alive" | "update_environment_variables",
            ) => {}
            Some(_) | None => {}
        }
    }

    let prompt = prompt_parts.join("\n").trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!(
            "--input-format=stream-json requires at least one user message or prompt"
        ));
    }

    Ok(StreamJsonInput {
        prompt,
        replay_events,
        history_messages,
    })
}

fn is_end_session_control_request(event: &Value) -> bool {
    event
        .get("request")
        .and_then(|request| request.get("subtype"))
        .and_then(Value::as_str)
        == Some("end_session")
}

fn stream_json_input_error_payload(message: &str) -> Value {
    serde_json::json!({
        "schema": "kiana.stream-json-input-error.v1",
        "code": stream_json_input_error_code(message),
        "line": stream_json_input_error_line(message),
        "message": message,
    })
}

fn stream_json_input_error_code(message: &str) -> &'static str {
    if message.contains("is not valid JSON") {
        "invalid_json"
    } else if message.contains("requires at least one user message or prompt") {
        "missing_prompt"
    } else {
        "invalid_input"
    }
}

fn stream_json_input_error_line(message: &str) -> Option<usize> {
    let marker = " line ";
    let start = message.find(marker)? + marker.len();
    let digits = message[start..]
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    digits.parse::<usize>().ok()
}

fn stream_json_history_message(event: &Value) -> Option<Value> {
    if event.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let message = event.get("message")?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let content = message.get("content")?.clone();
    Some(serde_json::json!({
        "role": "assistant",
        "content": content,
    }))
}

fn is_human_user_event(event: &Value) -> bool {
    if event
        .get("parent_tool_use_id")
        .is_some_and(|value| !value.is_null())
    {
        return false;
    }
    if event
        .get("isSynthetic")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    event
        .get("message")
        .and_then(|message| message.get("role"))
        .and_then(Value::as_str)
        == Some("user")
}

fn user_event_text(event: &Value) -> Option<String> {
    let content = event.get("message")?.get("content")?;
    match content {
        Value::String(text) => Some(text.trim().to_string()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        Value::Object(_) if content.get("type").and_then(Value::as_str) == Some("text") => content
            .get("text")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

async fn print_main(print_args: PrintArgs, runtime_flags: &RuntimeFlags) -> Result<()> {
    let mut options = HashMap::new();
    options.insert("execute".to_string(), Value::Bool(print_args.execute));
    let initial_prompt = apply_prompt_runtime_options(&mut options, runtime_flags)?;
    if runtime_flags.session_id.is_some() {
        options.insert("create_session_if_missing".to_string(), Value::Bool(true));
    }
    if runtime_flags.no_session_persistence {
        options.insert("no_session_persistence".to_string(), Value::Bool(true));
    }
    if let Some(json_schema) = print_args.json_schema {
        options.insert("json_schema".to_string(), json_schema);
    }

    if print_args.sdk_url.is_some()
        && matches!(print_args.input_format, PrintInputFormat::StreamJson)
        && matches!(print_args.output_format, PrintOutputFormat::StreamJson)
    {
        options.insert(
            "sdk_url".to_string(),
            Value::String(print_args.sdk_url.clone().unwrap()),
        );
        return print_bridge_stream_json_loop(print_args.message, options, initial_prompt).await;
    }

    let (message, replay_events) = match print_args.input_format {
        PrintInputFormat::Text => (print_args.message, Vec::new()),
        PrintInputFormat::StreamJson => {
            let stdin = tokio::io::BufReader::new(tokio::io::stdin());
            let input = match read_stream_json_input(
                stdin,
                &print_args.message,
                print_args.replay_user_messages,
            )
            .await
            {
                Ok(input) => input,
                Err(error) if matches!(print_args.output_format, PrintOutputFormat::StreamJson) => {
                    println!(
                        "{}",
                        serde_json::to_string(&stream_json_input_error_payload(
                            &error.to_string()
                        ))?
                    );
                    return Err(error);
                }
                Err(error) => return Err(error),
            };
            if !input.history_messages.is_empty() {
                options.insert(
                    crate::sdk::STREAM_JSON_HISTORY_MESSAGES_OPTION.to_string(),
                    Value::Array(input.history_messages),
                );
            }
            (input.prompt, input.replay_events)
        }
    };
    let message = prepend_initial_prompt(message, initial_prompt.as_deref());

    if print_args.resident_teammate {
        if !print_args.execute {
            return Err(anyhow!(
                "--resident-teammate requires model execution; remove --record-only"
            ));
        }
        if print_args.include_partial_messages {
            return Err(anyhow!(
                "--resident-teammate does not support --include-partial-messages"
            ));
        }
        let started = Instant::now();
        let result = crate::runner::run_resident_teammate_loop_with(
            message,
            options,
            crate::runner::ResidentTeammateLoopConfig::from_env(),
            crate::sdk::unstable_v2_prompt,
        )
        .await?;
        let output = format_resident_teammate_result_with_duration(
            &result,
            &print_args.output_format,
            started.elapsed().as_millis() as u64,
        )?;
        if !output.is_empty() {
            println!("{}", output);
        }
        return Ok(());
    }

    if print_args.include_partial_messages {
        return print_streaming_partial_result(message, options, replay_events).await;
    }

    let started = Instant::now();
    let result = crate::sdk::unstable_v2_prompt(message, options).await?;
    let output = format_print_result_with_duration(
        &result,
        &print_args.output_format,
        started.elapsed().as_millis() as u64,
        &replay_events,
    )
    .await?;
    if !output.is_empty() {
        println!("{}", output);
    }
    Ok(())
}

async fn print_bridge_stream_json_loop(
    cli_prompt: String,
    options: HashMap<String, Value>,
    initial_prompt: Option<String>,
) -> Result<()> {
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let stdout = std::io::stdout();
    print_bridge_stream_json_loop_with_io(cli_prompt, options, initial_prompt, stdin, stdout).await
}

async fn print_bridge_stream_json_loop_with_io<R, W>(
    cli_prompt: String,
    mut options: HashMap<String, Value>,
    initial_prompt: Option<String>,
    reader: R,
    writer: W,
) -> Result<()>
where
    R: AsyncBufRead + Unpin + Send,
    W: Write + Send,
{
    options.insert(
        "permission_prompt_tool".to_string(),
        Value::String("stdio".to_string()),
    );
    let options = Arc::new(AsyncMutex::new(options));
    let io = Arc::new(AsyncMutex::new(BridgeStreamJsonIo::new(reader, writer)));
    let permission_handler = BridgeStreamJsonPermissionHandler {
        io: io.clone(),
        options: options.clone(),
    };

    loop {
        let Some(event) = ({
            let mut io = io.lock().await;
            io.next_event().await?
        }) else {
            break;
        };
        match event.get("type").and_then(Value::as_str) {
            Some("user") => {
                let mut prompt_parts = Vec::new();
                if !cli_prompt.trim().is_empty() {
                    prompt_parts.push(cli_prompt.trim().to_string());
                }
                if let Some(text) = user_event_text(&event).filter(|text| !text.is_empty()) {
                    prompt_parts.push(text);
                }
                let prompt = prompt_parts.join("\n");
                if prompt.trim().is_empty() {
                    continue;
                }
                let prompt = prepend_initial_prompt(prompt, initial_prompt.as_deref());
                let prompt_options = options.lock().await.clone();
                let started = Instant::now();
                let result = crate::sdk::unstable_v2_prompt_with_permission_handler(
                    prompt,
                    prompt_options,
                    &permission_handler,
                )
                .await?;
                let mut io = io.lock().await;
                io.write_event(&stream_json_result_event(
                    &result,
                    started.elapsed().as_millis() as u64,
                ))?;
                io.write_event(&bridge_assistant_event(&result))?;
            }
            Some("update_environment_variables") => {
                apply_environment_variable_update(&event)?;
            }
            Some("control_request") => {
                let outcome = {
                    let mut options = options.lock().await;
                    handle_bridge_control_request_outcome(&event, &mut options)
                };
                let mut io = io.lock().await;
                io.write_event(&outcome.response)?;
                if outcome.end_session {
                    break;
                }
            }
            Some(
                "control_response"
                | "control_cancel_request"
                | "keep_alive"
                | "assistant"
                | "system",
            ) => {}
            Some(other) => {
                return Err(anyhow!(
                    "--input-format=stream-json bridge loop does not support type '{}'",
                    other
                ));
            }
            None => {
                return Err(anyhow!(
                    "--input-format=stream-json bridge loop message is missing type"
                ));
            }
        }
    }
    Ok(())
}

struct BridgeStreamJsonIo<R, W> {
    lines: tokio::io::Lines<R>,
    writer: W,
}

impl<R, W> BridgeStreamJsonIo<R, W>
where
    R: AsyncBufRead + Unpin,
    W: Write,
{
    fn new(reader: R, writer: W) -> Self {
        Self {
            lines: reader.lines(),
            writer,
        }
    }

    async fn next_event(&mut self) -> Result<Option<Value>> {
        loop {
            let Some(line) = self.lines.next_line().await? else {
                return Ok(None);
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: Value = serde_json::from_str(line)
                .with_context(|| "--input-format=stream-json line is not valid JSON")?;
            return Ok(Some(event));
        }
    }

    fn write_event(&mut self, event: &Value) -> Result<()> {
        write_json_line(&mut self.writer, event)?;
        self.writer.flush()?;
        Ok(())
    }
}

struct BridgeStreamJsonPermissionHandler<R, W> {
    io: Arc<AsyncMutex<BridgeStreamJsonIo<R, W>>>,
    options: Arc<AsyncMutex<HashMap<String, Value>>>,
}

#[async_trait::async_trait]
impl<R, W> PermissionPromptHandler for BridgeStreamJsonPermissionHandler<R, W>
where
    R: AsyncBufRead + Unpin + Send,
    W: Write + Send,
{
    async fn prompt(
        &self,
        request: PermissionPromptRequest,
    ) -> std::result::Result<PermissionPromptDecision, String> {
        let mut io = self.io.lock().await;
        io.write_event(&bridge_permission_request_event(&request))
            .map_err(|error| error.to_string())?;

        loop {
            let Some(event) = io.next_event().await.map_err(|error| error.to_string())? else {
                return Err(format!(
                    "Tool permission stream closed before response for {}",
                    request.request_id
                ));
            };
            match event.get("type").and_then(Value::as_str) {
                Some("control_response") => {
                    if !bridge_control_response_matches(&event, &request.request_id) {
                        continue;
                    }
                    io.write_event(&serde_json::json!({
                        "type": "control_cancel_request",
                        "request_id": request.request_id,
                    }))
                    .map_err(|error| error.to_string())?;
                    return bridge_permission_decision_from_control_response(&event);
                }
                Some("control_cancel_request") => {
                    if event.get("request_id").and_then(Value::as_str)
                        == Some(request.request_id.as_str())
                    {
                        return Ok(PermissionPromptDecision::Deny(
                            "Permission request cancelled".to_string(),
                        ));
                    }
                }
                Some("control_request") => {
                    let outcome = {
                        let mut options = self.options.lock().await;
                        handle_bridge_control_request_outcome(&event, &mut options)
                    };
                    io.write_event(&outcome.response)
                        .map_err(|error| error.to_string())?;
                    if outcome.end_session {
                        return Err("Session ended by remote control request".to_string());
                    }
                }
                Some("update_environment_variables") => {
                    apply_environment_variable_update(&event).map_err(|error| error.to_string())?;
                }
                Some("keep_alive" | "assistant" | "system" | "user") => {}
                Some(other) => {
                    return Err(format!(
                        "Permission prompt received unsupported stream-json type '{other}'"
                    ));
                }
                None => {
                    return Err(
                        "Permission prompt received stream-json message without type".to_string(),
                    );
                }
            }
        }
    }
}

fn bridge_permission_request_event(request: &PermissionPromptRequest) -> Value {
    let mut event = serde_json::json!({
        "type": "control_request",
        "request_id": request.request_id,
        "request": {
            "subtype": "can_use_tool",
            "tool_name": request.tool_name,
            "input": request.input,
            "tool_use_id": request.tool_use_id,
            "permission_suggestions": request.permission_suggestions,
            "decision_reason": request.decision_reason,
        }
    });

    if let Some(request_object) = event
        .get_mut("request")
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(blocked_path) = &request.blocked_path {
            request_object.insert(
                "blocked_path".to_string(),
                Value::String(blocked_path.clone()),
            );
        }
        if let Some(agent_id) = &request.agent_id {
            request_object.insert("agent_id".to_string(), Value::String(agent_id.clone()));
        }
    }

    event
}

fn bridge_control_response_matches(event: &Value, request_id: &str) -> bool {
    event
        .get("response")
        .and_then(|response| response.get("request_id"))
        .and_then(Value::as_str)
        == Some(request_id)
}

fn bridge_permission_decision_from_control_response(
    event: &Value,
) -> std::result::Result<PermissionPromptDecision, String> {
    let response = event
        .get("response")
        .ok_or_else(|| "control_response missing response object".to_string())?;
    match response.get("subtype").and_then(Value::as_str) {
        Some("error") => Ok(PermissionPromptDecision::Deny(
            response
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Permission request failed")
                .to_string(),
        )),
        Some("success") => {
            let payload = response.get("response").unwrap_or(&Value::Null);
            bridge_permission_decision_from_payload(payload)
        }
        Some(subtype) => Err(format!(
            "control_response subtype '{subtype}' is not supported for permission prompts"
        )),
        None => Err("control_response missing response subtype".to_string()),
    }
}

fn bridge_permission_decision_from_payload(
    payload: &Value,
) -> std::result::Result<PermissionPromptDecision, String> {
    if let Some(allowed) = payload.get("allowed").and_then(Value::as_bool) {
        return if allowed {
            Ok(PermissionPromptDecision::Allow)
        } else {
            Ok(PermissionPromptDecision::Deny(
                bridge_permission_denial_message(payload),
            ))
        };
    }
    let decision = payload
        .get("behavior")
        .or_else(|| payload.get("decision"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_ascii_lowercase());
    match decision.as_deref() {
        Some("allow" | "allowed" | "approve" | "approved") => Ok(PermissionPromptDecision::Allow),
        Some("deny" | "denied" | "reject" | "rejected") => Ok(PermissionPromptDecision::Deny(
            bridge_permission_denial_message(payload),
        )),
        Some(other) => Err(format!("unsupported permission decision '{other}'")),
        None => Err("permission response missing behavior, decision, or allowed".to_string()),
    }
}

fn bridge_permission_denial_message(payload: &Value) -> String {
    payload
        .get("message")
        .or_else(|| payload.get("reason"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Permission denied")
        .to_string()
}

fn bridge_assistant_event(result: &Value) -> Value {
    let text = result
        .get("assistant_text")
        .and_then(Value::as_str)
        .or_else(|| result.get("status").and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            result
                .get("structured_output")
                .map(|output| output.to_string())
        })
        .unwrap_or_default();
    let session_id = result
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    serde_json::json!({
        "type": "assistant",
        "uuid": uuid::Uuid::new_v4().to_string(),
        "parent_tool_use_id": null,
        "session_id": session_id,
        "message": {
            "role": "assistant",
            "content": [{
                "type": "text",
                "text": text
            }]
        }
    })
}

fn apply_environment_variable_update(event: &Value) -> Result<()> {
    let variables = event
        .get("variables")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("update_environment_variables requires variables object"))?;
    if let Some(key) = variables.keys().find(|key| {
        let key = key.to_ascii_uppercase();
        key == "HOME" || key == "USERPROFILE" || key.starts_with("KIANA_")
    }) {
        return Err(anyhow!(
            "update_environment_variables rejected trust-authority key '{key}'"
        ));
    }
    for (key, value) in variables {
        if let Some(value) = value.as_str() {
            std::env::set_var(key, value);
        }
    }
    Ok(())
}

struct BridgeControlRequestOutcome {
    response: Value,
    end_session: bool,
}

#[cfg(test)]
fn handle_bridge_control_request(event: &Value, options: &mut HashMap<String, Value>) -> Value {
    handle_bridge_control_request_outcome(event, options).response
}

fn handle_bridge_control_request_outcome(
    event: &Value,
    options: &mut HashMap<String, Value>,
) -> BridgeControlRequestOutcome {
    let request_id = event
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(request) = event.get("request").and_then(Value::as_object) else {
        return bridge_control_outcome(
            bridge_control_error(request_id, "control_request requires request object"),
            false,
        );
    };
    let response = match request.get("subtype").and_then(Value::as_str) {
        Some("initialize") => bridge_control_success(
            request_id,
            Some(serde_json::json!({
                "commands": [],
                "output_style": "normal",
                "available_output_styles": ["normal"],
                "models": [],
                "account": {},
                "pid": std::process::id(),
            })),
        ),
        Some("set_model") => {
            match request.get("model") {
                Some(Value::String(model)) if !model.trim().is_empty() => {
                    options.insert("model".to_string(), Value::String(model.trim().to_string()));
                    std::env::set_var("ANTHROPIC_MODEL", model.trim());
                }
                Some(Value::Null) | None => {
                    options.remove("model");
                    std::env::remove_var("ANTHROPIC_MODEL");
                }
                _ => {
                    return bridge_control_outcome(
                        bridge_control_error(request_id, "set_model requires string or null model"),
                        false,
                    );
                }
            }
            bridge_control_success(request_id, None)
        }
        Some("set_max_thinking_tokens") => {
            let value = request
                .get("max_thinking_tokens")
                .cloned()
                .unwrap_or(Value::Null);
            match value {
                Value::Number(number) if number.as_u64().is_some() => {
                    std::env::set_var("KIANA_MAX_THINKING_TOKENS", number.to_string());
                    options.insert("max_thinking_tokens".to_string(), Value::Number(number));
                }
                Value::Null => {
                    options.remove("max_thinking_tokens");
                    std::env::remove_var("KIANA_MAX_THINKING_TOKENS");
                }
                _ => {
                    return bridge_control_outcome(
                        bridge_control_error(
                            request_id,
                            "set_max_thinking_tokens requires a non-negative integer or null",
                        ),
                        false,
                    );
                }
            }
            bridge_control_success(request_id, None)
        }
        Some("set_permission_mode") => {
            let Some(mode) = request
                .get("mode")
                .and_then(Value::as_str)
                .and_then(normalize_stream_json_permission_mode)
            else {
                return bridge_control_outcome(
                    bridge_control_error(
                        request_id,
                        "set_permission_mode requires a supported mode",
                    ),
                    false,
                );
            };
            options.insert(
                "permission_mode".to_string(),
                Value::String(mode.to_string()),
            );
            options.insert(
                "permissionMode".to_string(),
                Value::String(mode.to_string()),
            );
            std::env::set_var("KIANA_PERMISSION_MODE", mode);
            bridge_control_success(
                request_id,
                Some(serde_json::json!({
                    "mode": mode,
                    "permission_mode": mode,
                    "permissionMode": mode,
                })),
            )
        }
        Some("end_session") => {
            return bridge_control_outcome(bridge_control_success(request_id, None), true);
        }
        Some("interrupt") => bridge_control_success(
            request_id,
            Some(serde_json::json!({
                "interrupted": true,
            })),
        ),
        Some("mcp_status") => bridge_control_success(
            request_id,
            Some(serde_json::json!({
                "mcpServers": [],
            })),
        ),
        Some("can_use_tool") => bridge_control_error(
            request_id,
            "bridge child does not handle server can_use_tool control requests",
        ),
        Some(subtype) => bridge_control_error(
            request_id,
            format!("REPL bridge does not handle control_request subtype: {subtype}"),
        ),
        None => bridge_control_error(request_id, "control_request missing request subtype"),
    };
    bridge_control_outcome(response, false)
}

fn bridge_control_outcome(response: Value, end_session: bool) -> BridgeControlRequestOutcome {
    BridgeControlRequestOutcome {
        response,
        end_session,
    }
}

fn bridge_control_success(request_id: String, response: Option<Value>) -> Value {
    let mut body = serde_json::json!({
        "subtype": "success",
        "request_id": request_id,
    });
    if let Some(response) = response {
        body["response"] = response;
    }
    serde_json::json!({
        "type": "control_response",
        "response": body,
    })
}

fn bridge_control_error(request_id: String, error: impl Into<String>) -> Value {
    serde_json::json!({
        "type": "control_response",
        "response": {
            "subtype": "error",
            "request_id": request_id,
            "error": error.into(),
        }
    })
}

fn format_resident_teammate_result_with_duration(
    result: &crate::runner::ResidentTeammateLoopResult,
    output_format: &PrintOutputFormat,
    duration_ms: u64,
) -> Result<String> {
    let value = serde_json::json!({
        "type": "resident_teammate_completed",
        "session_id": result.session_id,
        "turns": result.turns,
        "idle_polls": result.idle_polls,
        "stop_reason": result.stop_reason,
        "duration_ms": duration_ms
    });
    match output_format {
        PrintOutputFormat::Text => Ok(format!(
            "resident teammate stopped\tturns={}\tidle_polls={}\tstop_reason={}\tsession_id={}",
            result.turns, result.idle_polls, result.stop_reason, result.session_id
        )),
        PrintOutputFormat::Json | PrintOutputFormat::StreamJson => {
            Ok(serde_json::to_string(&value)?)
        }
    }
}

async fn print_streaming_partial_result(
    message: String,
    mut options: HashMap<String, Value>,
    replay_events: Vec<Value>,
) -> Result<()> {
    let session_id = ensure_prompt_session_id(&mut options);
    let started = Instant::now();
    let mut stdout = std::io::stdout();
    write_json_line(&mut stdout, &stream_json_init_event(&session_id).await?)?;
    for event in replay_events {
        write_json_line(
            &mut stdout,
            &normalize_stream_json_replay_event(event, &session_id),
        )?;
    }

    let result =
        crate::sdk::unstable_v2_prompt_streaming_with_local_events(message, options, |event| {
            write_json_line(&mut stdout, &stream_json_runner_event(event, &session_id)?)
        })
        .await?;
    write_json_line(
        &mut stdout,
        &stream_json_result_event(&result, started.elapsed().as_millis() as u64),
    )?;
    Ok(())
}

fn ensure_prompt_session_id(options: &mut HashMap<String, Value>) -> String {
    if let Some(session_id) = options
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.trim().is_empty())
    {
        return session_id.to_string();
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    options.insert("session_id".to_string(), Value::String(session_id.clone()));
    if !options
        .get("no_session_persistence")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        options.insert("create_session_if_missing".to_string(), Value::Bool(true));
    }
    session_id
}

#[cfg(test)]
fn stream_json_partial_event(
    event: kiana_services::api::streaming::StreamEvent,
    session_id: &str,
) -> Result<Value> {
    stream_json_runner_event(crate::runner::RunnerStreamEvent::Model(event), session_id)
}

fn stream_json_runner_event(
    event: crate::runner::RunnerStreamEvent,
    session_id: &str,
) -> Result<Value> {
    let event_value = match &event {
        crate::runner::RunnerStreamEvent::Model(event) => serde_json::to_value(event)?,
        crate::runner::RunnerStreamEvent::ToolResult {
            id,
            name,
            is_error,
            content,
            changed_files,
            error,
        } => {
            let mut value = serde_json::json!({
                "type": "tool_result",
                "tool_use_id": id,
                "name": name,
                "is_error": is_error,
                "content": content,
            });
            if let Some(error) = error {
                value["error"] = error.clone();
            }
            if let Some(changed_files) = changed_files {
                value["changed_files"] = changed_files.clone();
            }
            value
        }
    };
    let runtime_events = crate::runner::runtime_events_from_runner_stream_event(
        session_id,
        "stream-json",
        None,
        0,
        "0",
        event,
    );
    Ok(serde_json::json!({
        "type": "stream_event",
        "event": event_value,
        "runtime_events": runtime_events,
        "parent_tool_use_id": null,
        "uuid": uuid::Uuid::new_v4().to_string(),
        "session_id": session_id,
    }))
}

fn write_json_line<W: Write>(writer: &mut W, event: &Value) -> Result<()> {
    writeln!(writer, "{}", serde_json::to_string(event)?)?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
async fn format_print_result(result: &Value, output_format: &PrintOutputFormat) -> Result<String> {
    format_print_result_with_duration(result, output_format, 0, &[]).await
}

async fn format_print_result_with_duration(
    result: &Value,
    output_format: &PrintOutputFormat,
    duration_ms: u64,
    replay_events: &[Value],
) -> Result<String> {
    if matches!(output_format, PrintOutputFormat::Json) {
        return Ok(serde_json::to_string_pretty(result)?);
    }

    if matches!(output_format, PrintOutputFormat::StreamJson) {
        return format_stream_json_result(result, duration_ms, replay_events).await;
    }

    if let Some(structured_output) = result.get("structured_output") {
        return Ok(serde_json::to_string_pretty(structured_output)?);
    }

    if let Some(text) = result.get("assistant_text").and_then(Value::as_str) {
        return Ok(text.to_string());
    }

    Ok(serde_json::to_string_pretty(result)?)
}

async fn format_stream_json_result(
    result: &Value,
    duration_ms: u64,
    replay_events: &[Value],
) -> Result<String> {
    let session_id = result
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let assistant_text = result
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut events = vec![stream_json_init_event(&session_id).await?];
    events.extend(
        replay_events
            .iter()
            .cloned()
            .map(|event| normalize_stream_json_replay_event(event, &session_id)),
    );

    if !assistant_text.is_empty() {
        events.push(serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": assistant_text
                }]
            },
            "parent_tool_use_id": null,
            "uuid": uuid::Uuid::new_v4().to_string(),
            "session_id": session_id,
        }));
    }

    let mut result_event = stream_json_result_event(result, duration_ms);
    if let Some(structured_output) = result.get("structured_output") {
        result_event["structured_output"] = structured_output.clone();
    }
    events.push(result_event);

    events
        .into_iter()
        .map(|event| serde_json::to_string(&event).map_err(Into::into))
        .collect::<Result<Vec<_>>>()
        .map(|lines| lines.join("\n"))
}

fn stream_json_result_event(result: &Value, duration_ms: u64) -> Value {
    let session_id = result
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let assistant_text = result
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut result_event = serde_json::json!({
        "type": "result",
        "subtype": "success",
        "is_error": false,
        "duration_ms": duration_ms,
        "duration_api_ms": duration_ms,
        "num_turns": result
            .get("iterations")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| if assistant_text.is_empty() { 0 } else { 1 }),
        "result": assistant_text,
        "stop_reason": null,
        "session_id": session_id,
        "total_cost_usd": 0,
        "usage": {},
        "modelUsage": {},
        "permission_denials": [],
        "fast_mode_state": "off",
        "uuid": uuid::Uuid::new_v4().to_string(),
    });
    if let Some(structured_output) = result.get("structured_output") {
        result_event["structured_output"] = structured_output.clone();
    }
    result_event
}

async fn stream_json_init_event(session_id: &str) -> Result<Value> {
    let mut tool_names: Vec<String> = kiana_tools::create_default_registry()
        .list_tools()
        .into_iter()
        .map(|tool| tool.name().to_string())
        .collect();
    tool_names.sort();

    let mut slash_commands: Vec<String> = create_default_command_registry()
        .list()
        .into_iter()
        .map(|command| command.name().to_string())
        .collect();
    slash_commands.sort();
    let cwd = std::env::current_dir()?;
    let app_state = HashMap::from([("cwd".to_string(), Value::String(cwd.display().to_string()))]);
    let project_trust = kiana_types::project_trust_from_app_state(&app_state);
    let output_style =
        kiana_commands::output_style::selected_output_style_name_with_trust(&cwd, project_trust);
    let available_output_styles =
        kiana_commands::output_style::available_output_style_names_with_trust(&cwd, project_trust);
    let skills = stream_json_skill_summaries(&cwd).await;
    let plugins = stream_json_plugin_summaries(&cwd)?;

    Ok(serde_json::json!({
        "type": "system",
        "subtype": "init",
        "apiKeySource": "temporary",
        "claude_code_version": env!("CARGO_PKG_VERSION"),
        "cwd": cwd.display().to_string(),
        "tools": tool_names,
        "mcp_servers": stream_json_mcp_servers(),
        "model": std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "default".to_string()),
        "permissionMode": stream_json_permission_mode(),
        "slash_commands": slash_commands,
        "output_style": output_style,
        "available_output_styles": available_output_styles,
        "skills": skills,
        "plugins": plugins,
        "fast_mode_state": "off",
        "uuid": uuid::Uuid::new_v4().to_string(),
        "session_id": session_id,
    }))
}

async fn stream_json_skill_summaries(cwd: &Path) -> Value {
    kiana_skills::clear_caches();
    let app_state = HashMap::from([("cwd".to_string(), Value::String(cwd.display().to_string()))]);
    let project_trust = kiana_types::project_trust_from_app_state(&app_state);
    let mut skills = kiana_skills::load_all_skills_with_trust(cwd, project_trust).await;
    skills.retain(|skill| skill.user_invocable);
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Value::Array(
        skills
            .into_iter()
            .map(|skill| {
                serde_json::json!({
                    "name": skill.name,
                    "display_name": skill.display_name,
                    "description": skill.description,
                    "when_to_use": skill.when_to_use,
                    "argument_hint": skill.argument_hint,
                    "allowed_tools": skill.allowed_tools,
                    "model": skill.model,
                    "source": skill.source,
                    "loaded_from": skill.loaded_from,
                    "root": skill.skill_root.map(|path| path.display().to_string()),
                })
            })
            .collect::<Vec<_>>(),
    )
}

fn stream_json_plugin_summaries(cwd: &Path) -> Result<Value> {
    let context = CommandContext {
        args: String::new(),
        app_state: HashMap::from([("cwd".to_string(), Value::String(cwd.display().to_string()))]),
    };
    kiana_commands::plugin::installed_plugin_summaries(&context)
}

fn normalize_stream_json_replay_event(mut event: Value, session_id: &str) -> Value {
    if let Some(object) = event.as_object_mut() {
        object
            .entry("uuid".to_string())
            .or_insert_with(|| Value::String(uuid::Uuid::new_v4().to_string()));
        match object.get("session_id") {
            Some(Value::String(value)) if !value.trim().is_empty() => {}
            _ => {
                object.insert(
                    "session_id".to_string(),
                    Value::String(session_id.to_string()),
                );
            }
        }
        object
            .entry("parent_tool_use_id".to_string())
            .or_insert(Value::Null);
    }
    event
}

fn stream_json_mcp_servers() -> Value {
    let Ok(raw) = std::env::var(kiana_tools::mcp_tool::MCP_SERVERS_ENV) else {
        return serde_json::json!([]);
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return serde_json::json!([]);
    };
    value
}

fn stream_json_permission_mode() -> &'static str {
    std::env::var("KIANA_PERMISSION_MODE")
        .ok()
        .as_deref()
        .and_then(normalize_stream_json_permission_mode)
        .unwrap_or("default")
}

fn normalize_stream_json_permission_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "acceptEdits" | "accept-edits" | "accept_edits" => Some("acceptEdits"),
        "bypassPermissions" | "bypass-permissions" | "bypass_permissions" | "auto" => {
            Some("bypassPermissions")
        }
        "dontAsk" | "dont-ask" | "dont_ask" => Some("dontAsk"),
        "default" => Some("default"),
        "plan" => Some("plan"),
        "ask" => Some("ask"),
        _ => None,
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct RuntimeFlags {
    permission_mode: Option<String>,
    permission_profile: Option<String>,
    allowed_tools: Option<String>,
    disallowed_tools: Option<String>,
    model: Option<String>,
    fallback_model: Option<String>,
    permission_prompt_tool: Option<String>,
    agent: Option<String>,
    agents_json: Option<String>,
    base_url: Option<String>,
    api_timeout_ms: Option<String>,
    max_turns: Option<String>,
    max_thinking_tokens: Option<String>,
    repair_checks: bool,
    repair_check_attempts: Option<String>,
    tools: Option<String>,
    settings: Option<String>,
    session_id: Option<String>,
    session_name: Option<String>,
    no_session_persistence: bool,
    approve_local_write: bool,
    strict_mcp_config: bool,
    bare: bool,
    add_dirs: Vec<String>,
    editable_files: Vec<String>,
    read_only_files: Vec<String>,
    mcp_configs: Vec<String>,
    system_prompt: Option<String>,
    system_prompt_file: Option<String>,
    append_system_prompt: Option<String>,
    append_system_prompt_file: Option<String>,
}

fn extract_runtime_flags(args: Vec<String>) -> Result<(Vec<String>, RuntimeFlags)> {
    let mut kept = Vec::new();
    let mut flags = RuntimeFlags::default();
    let mut index = 0;
    let mut command_seen = false;
    let mut positional_started = false;

    while index < args.len() {
        let arg = &args[index];
        let can_parse_global_flag = !positional_started;

        if can_parse_global_flag {
            if let Some(value) = arg.strip_prefix("--settings=") {
                flags.settings = Some(nonempty_flag_value("--settings", value)?);
                index += 1;
                continue;
            }
            if arg == "--settings" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--settings requires a value"))?;
                flags.settings = Some(nonempty_flag_value("--settings", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--session-id=") {
                flags.session_id = Some(nonempty_flag_value("--session-id", value)?);
                index += 1;
                continue;
            }
            if arg == "--session-id" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--session-id requires a value"))?;
                flags.session_id = Some(nonempty_flag_value("--session-id", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--name=")
                .or_else(|| arg.strip_prefix("-n="))
            {
                flags.session_name = Some(nonempty_flag_value("--name", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--name" | "-n") {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--name requires a value"))?;
                flags.session_name = Some(nonempty_flag_value("--name", value.as_str())?);
                index += 2;
                continue;
            }
            if arg == "--no-session-persistence" {
                flags.no_session_persistence = true;
                index += 1;
                continue;
            }
            if arg == "--approve-local-write" {
                flags.approve_local_write = true;
                index += 1;
                continue;
            }
            if arg == "--strict-mcp-config" {
                flags.strict_mcp_config = true;
                index += 1;
                continue;
            }
            if arg == "--bare" {
                flags.bare = true;
                index += 1;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--add-dir=")
                .or_else(|| arg.strip_prefix("--addDir="))
            {
                flags
                    .add_dirs
                    .push(nonempty_flag_value("--add-dir", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--add-dir" | "--addDir") {
                let mut values = Vec::new();
                let mut cursor = index + 1;
                while let Some(value) = args.get(cursor) {
                    if value.starts_with('-') {
                        break;
                    }
                    values.push(nonempty_flag_value("--add-dir", value)?);
                    cursor += 1;
                }
                if values.is_empty() {
                    return Err(anyhow!("--add-dir requires at least one directory"));
                }
                flags.add_dirs.extend(values);
                index = cursor;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--editable-file=")
                .or_else(|| arg.strip_prefix("--editableFile="))
            {
                flags
                    .editable_files
                    .push(nonempty_flag_value("--editable-file", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--editable-file" | "--editableFile") {
                let mut values = Vec::new();
                let mut cursor = index + 1;
                while let Some(value) = args.get(cursor) {
                    if value.starts_with('-') {
                        break;
                    }
                    values.push(nonempty_flag_value("--editable-file", value)?);
                    cursor += 1;
                }
                if values.is_empty() {
                    return Err(anyhow!("--editable-file requires at least one file"));
                }
                flags.editable_files.extend(values);
                index = cursor;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--read-only-file=")
                .or_else(|| arg.strip_prefix("--readOnlyFile="))
                .or_else(|| arg.strip_prefix("--readonly-file="))
                .or_else(|| arg.strip_prefix("--readonlyFile="))
            {
                flags
                    .read_only_files
                    .push(nonempty_flag_value("--read-only-file", value)?);
                index += 1;
                continue;
            }
            if matches!(
                arg.as_str(),
                "--read-only-file" | "--readOnlyFile" | "--readonly-file" | "--readonlyFile"
            ) {
                let mut values = Vec::new();
                let mut cursor = index + 1;
                while let Some(value) = args.get(cursor) {
                    if value.starts_with('-') {
                        break;
                    }
                    values.push(nonempty_flag_value("--read-only-file", value)?);
                    cursor += 1;
                }
                if values.is_empty() {
                    return Err(anyhow!("--read-only-file requires at least one file"));
                }
                flags.read_only_files.extend(values);
                index = cursor;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--mcp-config=") {
                flags
                    .mcp_configs
                    .push(nonempty_flag_value("--mcp-config", value)?);
                index += 1;
                continue;
            }
            if arg == "--mcp-config" {
                let mut values = Vec::new();
                let mut cursor = index + 1;
                while let Some(value) = args.get(cursor) {
                    if value.starts_with('-') {
                        break;
                    }
                    values.push(nonempty_flag_value("--mcp-config", value)?);
                    cursor += 1;
                }
                if values.is_empty() {
                    return Err(anyhow!("--mcp-config requires at least one config"));
                }
                flags.mcp_configs.extend(values);
                index = cursor;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--permission-mode=") {
                flags.permission_mode = Some(nonempty_flag_value("--permission-mode", value)?);
                index += 1;
                continue;
            }
            if arg == "--permission-mode" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--permission-mode requires a value"))?;
                flags.permission_mode =
                    Some(nonempty_flag_value("--permission-mode", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--permission-profile=")
                .or_else(|| arg.strip_prefix("--permissionProfile="))
            {
                flags.permission_profile = Some(permission_profile_flag_value(
                    "--permission-profile",
                    value,
                )?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--permission-profile" | "--permissionProfile") {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--permission-profile requires a value"))?;
                flags.permission_profile = Some(permission_profile_flag_value(
                    "--permission-profile",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if matches!(arg.as_str(), "--dangerously-skip-permissions") {
                flags.permission_mode = Some("bypassPermissions".to_string());
                index += 1;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--permission-prompt-tool=")
                .or_else(|| arg.strip_prefix("--permissionPromptTool="))
            {
                flags.permission_prompt_tool =
                    Some(nonempty_flag_value("--permission-prompt-tool", value)?);
                index += 1;
                continue;
            }
            if matches!(
                arg.as_str(),
                "--permission-prompt-tool" | "--permissionPromptTool"
            ) {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--permission-prompt-tool requires a value"))?;
                flags.permission_prompt_tool = Some(nonempty_flag_value(
                    "--permission-prompt-tool",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--system-prompt=") {
                flags.system_prompt = Some(nonempty_flag_value("--system-prompt", value)?);
                index += 1;
                continue;
            }
            if arg == "--system-prompt" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--system-prompt requires a value"))?;
                flags.system_prompt = Some(nonempty_flag_value("--system-prompt", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--system-prompt-file=") {
                flags.system_prompt_file =
                    Some(nonempty_flag_value("--system-prompt-file", value)?);
                index += 1;
                continue;
            }
            if arg == "--system-prompt-file" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--system-prompt-file requires a value"))?;
                flags.system_prompt_file =
                    Some(nonempty_flag_value("--system-prompt-file", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--append-system-prompt=") {
                flags.append_system_prompt =
                    Some(nonempty_flag_value("--append-system-prompt", value)?);
                index += 1;
                continue;
            }
            if arg == "--append-system-prompt" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--append-system-prompt requires a value"))?;
                flags.append_system_prompt = Some(nonempty_flag_value(
                    "--append-system-prompt",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--append-system-prompt-file=") {
                flags.append_system_prompt_file =
                    Some(nonempty_flag_value("--append-system-prompt-file", value)?);
                index += 1;
                continue;
            }
            if arg == "--append-system-prompt-file" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--append-system-prompt-file requires a value"))?;
                flags.append_system_prompt_file = Some(nonempty_flag_value(
                    "--append-system-prompt-file",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--model=") {
                flags.model = Some(nonempty_flag_value("--model", value)?);
                index += 1;
                continue;
            }
            if arg == "--model" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--model requires a value"))?;
                flags.model = Some(nonempty_flag_value("--model", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--fallback-model=") {
                flags.fallback_model = Some(nonempty_flag_value("--fallback-model", value)?);
                index += 1;
                continue;
            }
            if arg == "--fallback-model" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--fallback-model requires a value"))?;
                flags.fallback_model =
                    Some(nonempty_flag_value("--fallback-model", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--agent=") {
                flags.agent = Some(nonempty_flag_value("--agent", value)?);
                index += 1;
                continue;
            }
            if arg == "--agent" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--agent requires a value"))?;
                flags.agent = Some(nonempty_flag_value("--agent", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--agents=") {
                flags.agents_json = Some(nonempty_flag_value("--agents", value)?);
                index += 1;
                continue;
            }
            if arg == "--agents" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--agents requires a JSON object"))?;
                flags.agents_json = Some(nonempty_flag_value("--agents", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--base-url=") {
                flags.base_url = Some(nonempty_flag_value("--base-url", value)?);
                index += 1;
                continue;
            }
            if arg == "--base-url" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--base-url requires a value"))?;
                flags.base_url = Some(nonempty_flag_value("--base-url", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--api-timeout-ms=") {
                flags.api_timeout_ms =
                    Some(positive_integer_flag_value("--api-timeout-ms", value)?);
                index += 1;
                continue;
            }
            if arg == "--api-timeout-ms" {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--api-timeout-ms requires a value"))?;
                flags.api_timeout_ms = Some(positive_integer_flag_value(
                    "--api-timeout-ms",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--api-timeout=")
                .or_else(|| arg.strip_prefix("--api-timeout-seconds="))
            {
                flags.api_timeout_ms = Some(timeout_seconds_flag_to_ms("--api-timeout", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--api-timeout" | "--api-timeout-seconds") {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--api-timeout requires a value"))?;
                flags.api_timeout_ms =
                    Some(timeout_seconds_flag_to_ms("--api-timeout", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--max-turns=")
                .or_else(|| arg.strip_prefix("--max-iterations="))
            {
                flags.max_turns = Some(positive_integer_flag_value("--max-turns", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--max-turns" | "--max-iterations") {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--max-turns requires a value"))?;
                flags.max_turns = Some(positive_integer_flag_value("--max-turns", value.as_str())?);
                index += 2;
                continue;
            }
            if matches!(arg.as_str(), "--repair-checks" | "--repairChecks") {
                flags.repair_checks = true;
                index += 1;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--repair-check-attempts=")
                .or_else(|| arg.strip_prefix("--repairCheckAttempts="))
                .or_else(|| arg.strip_prefix("--max-repair-attempts="))
                .or_else(|| arg.strip_prefix("--maxRepairAttempts="))
            {
                flags.repair_check_attempts = Some(positive_integer_flag_value(
                    "--repair-check-attempts",
                    value,
                )?);
                index += 1;
                continue;
            }
            if matches!(
                arg.as_str(),
                "--repair-check-attempts"
                    | "--repairCheckAttempts"
                    | "--max-repair-attempts"
                    | "--maxRepairAttempts"
            ) {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--repair-check-attempts requires a value"))?;
                flags.repair_check_attempts = Some(positive_integer_flag_value(
                    "--repair-check-attempts",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--max-thinking-tokens=")
                .or_else(|| arg.strip_prefix("--maxThinkingTokens="))
            {
                flags.max_thinking_tokens =
                    Some(positive_integer_flag_value("--max-thinking-tokens", value)?);
                index += 1;
                continue;
            }
            if matches!(
                arg.as_str(),
                "--max-thinking-tokens" | "--maxThinkingTokens"
            ) {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--max-thinking-tokens requires a value"))?;
                flags.max_thinking_tokens = Some(positive_integer_flag_value(
                    "--max-thinking-tokens",
                    value.as_str(),
                )?);
                index += 2;
                continue;
            }
            if let Some(value) = arg.strip_prefix("--tools=") {
                flags.tools = Some(value.trim().to_string());
                index += 1;
                continue;
            }
            if arg == "--tools" {
                let mut values = Vec::new();
                let mut cursor = index + 1;
                while let Some(value) = args.get(cursor) {
                    if value.starts_with('-') {
                        break;
                    }
                    values.push(value.clone());
                    cursor += 1;
                }
                if values.is_empty() {
                    return Err(anyhow!("--tools requires a value"));
                }
                flags.tools = Some(values.join(" ").trim().to_string());
                index = cursor;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--allowed-tools=")
                .or_else(|| arg.strip_prefix("--allowedTools="))
            {
                flags.allowed_tools = Some(nonempty_flag_value("--allowed-tools", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--allowed-tools" | "--allowedTools") {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--allowed-tools requires a value"))?;
                flags.allowed_tools = Some(nonempty_flag_value("--allowed-tools", value.as_str())?);
                index += 2;
                continue;
            }
            if let Some(value) = arg
                .strip_prefix("--disallowed-tools=")
                .or_else(|| arg.strip_prefix("--disallowedTools="))
            {
                flags.disallowed_tools = Some(nonempty_flag_value("--disallowed-tools", value)?);
                index += 1;
                continue;
            }
            if matches!(arg.as_str(), "--disallowed-tools" | "--disallowedTools") {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--disallowed-tools requires a value"))?;
                flags.disallowed_tools =
                    Some(nonempty_flag_value("--disallowed-tools", value.as_str())?);
                index += 2;
                continue;
            }
        }

        if is_prompt_command_token(arg) {
            command_seen = true;
            if prompt_command_contains_text(arg) {
                positional_started = true;
            }
        } else if !arg.starts_with('-') {
            if command_seen {
                positional_started = true;
            } else {
                command_seen = true;
            }
        }
        kept.push(arg.clone());
        index += 1;
    }

    validate_runtime_flags(&flags)?;
    Ok((kept, flags))
}

fn validate_runtime_flags(flags: &RuntimeFlags) -> Result<()> {
    if flags
        .model
        .as_ref()
        .zip(flags.fallback_model.as_ref())
        .is_some_and(|(model, fallback)| model == fallback)
    {
        return Err(anyhow!(
            "Fallback model cannot be the same as the main model"
        ));
    }
    if flags.system_prompt.is_some() && flags.system_prompt_file.is_some() {
        return Err(anyhow!(
            "cannot use both --system-prompt and --system-prompt-file"
        ));
    }
    if flags.append_system_prompt.is_some() && flags.append_system_prompt_file.is_some() {
        return Err(anyhow!(
            "cannot use both --append-system-prompt and --append-system-prompt-file"
        ));
    }
    Ok(())
}

fn is_prompt_command_token(arg: &str) -> bool {
    matches!(arg, "-p" | "--print" | "--bg" | "--background") || arg.starts_with("--print=")
}

fn prompt_command_contains_text(arg: &str) -> bool {
    arg.strip_prefix("--print=")
        .map(|prompt| !prompt.trim().is_empty())
        .unwrap_or(false)
}

fn nonempty_flag_value(flag: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{flag} requires a non-empty value"))
    } else {
        Ok(value.to_string())
    }
}

fn positive_integer_flag_value(flag: &str, value: &str) -> Result<String> {
    let value = nonempty_flag_value(flag, value)?;
    match value.parse::<usize>() {
        Ok(parsed) if parsed > 0 => Ok(value),
        _ => Err(anyhow!("{flag} requires a positive integer")),
    }
}

fn timeout_seconds_flag_to_ms(flag: &str, value: &str) -> Result<String> {
    let value = nonempty_flag_value(flag, value)?;
    match value
        .parse::<u64>()
        .ok()
        .filter(|parsed| *parsed > 0)
        .and_then(|parsed| parsed.checked_mul(1000))
    {
        Some(milliseconds) => Ok(milliseconds.to_string()),
        None => Err(anyhow!("{flag} requires a positive integer")),
    }
}

fn permission_profile_flag_value(flag: &str, value: &str) -> Result<String> {
    let value = nonempty_flag_value(flag, value)?;
    normalize_permission_profile_text(&value).ok_or_else(|| {
        anyhow!(
            "invalid {flag} '{}'; expected read-only, workspace, full, ask, or plan",
            value
        )
    })
}

fn normalize_permission_profile_text(value: &str) -> Option<String> {
    Some(
        match value.trim() {
            "read-only" | "readonly" | "read_only" => "read-only",
            "workspace" | "default" => "workspace",
            "full" => "full",
            "ask" => "ask",
            "plan" => "plan",
            _ => return None,
        }
        .to_string(),
    )
}

fn apply_runtime_flags(flags: &RuntimeFlags) -> Result<()> {
    if flags.bare {
        std::env::set_var("CLAUDE_CODE_SIMPLE", "1");
        std::env::set_var("KIANA_CODE_SIMPLE", "1");
    }
    if let Some(settings) = &flags.settings {
        apply_settings_flag(settings)?;
    }
    if !flags.add_dirs.is_empty() {
        apply_add_dirs_flag(&flags.add_dirs)?;
    }
    if !flags.mcp_configs.is_empty() {
        apply_mcp_config_flags(&flags.mcp_configs)?;
    } else if flags.strict_mcp_config || flags.bare {
        std::env::set_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV, "{}");
    }
    if let Some(mode) = &flags.permission_mode {
        std::env::set_var("KIANA_PERMISSION_MODE", mode);
    }
    if let Some(profile) = &flags.permission_profile {
        std::env::set_var("KIANA_PERMISSION_PROFILE", profile);
    }
    if let Some(allowed_tools) = &flags.allowed_tools {
        std::env::set_var("KIANA_ALLOWED_TOOLS", allowed_tools);
    }
    if let Some(disallowed_tools) = &flags.disallowed_tools {
        std::env::set_var("KIANA_DISALLOWED_TOOLS", disallowed_tools);
    }
    if let Some(permission_prompt_tool) = &flags.permission_prompt_tool {
        std::env::set_var(
            kiana_tools::tool_execution::PERMISSION_PROMPT_TOOL_ENV,
            permission_prompt_tool,
        );
    }
    if let Some(model) = &flags.model {
        std::env::set_var("ANTHROPIC_MODEL", model);
    }
    if let Some(base_url) = &flags.base_url {
        std::env::set_var("ANTHROPIC_BASE_URL", base_url);
    }
    if let Some(api_timeout_ms) = &flags.api_timeout_ms {
        std::env::set_var("KIANA_API_TIMEOUT_MS", api_timeout_ms);
    }
    if let Some(max_turns) = &flags.max_turns {
        std::env::set_var("KIANA_MAX_ITERATIONS", max_turns);
    }
    if let Some(max_thinking_tokens) = &flags.max_thinking_tokens {
        std::env::set_var("KIANA_MAX_THINKING_TOKENS", max_thinking_tokens);
    }
    if let Some(tools) = &flags.tools {
        std::env::set_var("KIANA_TOOLS", tools);
    }
    if let Some(system_prompt) = runtime_text_flag(
        "--system-prompt",
        flags.system_prompt.as_deref(),
        "--system-prompt-file",
        flags.system_prompt_file.as_deref(),
    )? {
        std::env::set_var("KIANA_SYSTEM_PROMPT", system_prompt);
    }
    if let Some(append_system_prompt) = runtime_text_flag(
        "--append-system-prompt",
        flags.append_system_prompt.as_deref(),
        "--append-system-prompt-file",
        flags.append_system_prompt_file.as_deref(),
    )? {
        std::env::set_var("KIANA_APPEND_SYSTEM_PROMPT", append_system_prompt);
    }
    Ok(())
}

fn apply_session_runtime_options(options: &mut HashMap<String, Value>, flags: &RuntimeFlags) {
    if let Some(session_id) = &flags.session_id {
        options.insert("session_id".to_string(), Value::String(session_id.clone()));
    }
    if let Some(session_name) = &flags.session_name {
        options.insert("title".to_string(), Value::String(session_name.clone()));
    }
}

fn apply_prompt_runtime_options(
    options: &mut HashMap<String, Value>,
    flags: &RuntimeFlags,
) -> Result<Option<String>> {
    apply_session_runtime_options(options, flags);
    if let Some(fallback_model) = &flags.fallback_model {
        options.insert(
            "fallback_model".to_string(),
            Value::String(fallback_model.clone()),
        );
    }
    if let Some(api_timeout_ms) = &flags.api_timeout_ms {
        options.insert(
            "api_timeout_ms".to_string(),
            Value::String(api_timeout_ms.clone()),
        );
    }
    if let Some(max_thinking_tokens) = &flags.max_thinking_tokens {
        options.insert(
            "max_thinking_tokens".to_string(),
            Value::String(max_thinking_tokens.clone()),
        );
    }
    if flags.repair_checks || flags.repair_check_attempts.is_some() {
        options.insert("repairChecks".to_string(), Value::Bool(true));
    }
    if let Some(repair_check_attempts) = &flags.repair_check_attempts {
        options.insert(
            "repairCheckAttempts".to_string(),
            Value::String(repair_check_attempts.clone()),
        );
    }
    if let Some(permission_prompt_tool) = &flags.permission_prompt_tool {
        options.insert(
            "permission_prompt_tool".to_string(),
            Value::String(permission_prompt_tool.clone()),
        );
    }
    if !flags.editable_files.is_empty() {
        options.insert(
            "editable_files".to_string(),
            Value::Array(
                flags
                    .editable_files
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !flags.read_only_files.is_empty() {
        options.insert(
            "read_only_files".to_string(),
            Value::Array(
                flags
                    .read_only_files
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if let Some(agent) = selected_cli_agent(flags)? {
        options.insert("agent".to_string(), Value::String(agent.name.clone()));
        if flags.model.is_none() {
            if let Some(model) = agent.model.filter(|model| model != "inherit") {
                options.insert("model".to_string(), Value::String(model));
            }
        }
        if flags.system_prompt.is_none() && flags.system_prompt_file.is_none() {
            options.insert("system_prompt".to_string(), Value::String(agent.prompt));
        }
        if flags.tools.is_none() {
            if let Some(tools) = agent.tools {
                options.insert(
                    "tools".to_string(),
                    Value::Array(tools.into_iter().map(Value::String).collect()),
                );
            }
        }
        if flags.disallowed_tools.is_none() {
            if let Some(disallowed_tools) = agent.disallowed_tools {
                options.insert(
                    "disallowed_tools".to_string(),
                    Value::Array(disallowed_tools.into_iter().map(Value::String).collect()),
                );
            }
        }
        if flags.permission_mode.is_none() {
            if let Some(permission_mode) = agent.permission_mode {
                options.insert(
                    "permission_mode".to_string(),
                    Value::String(permission_mode),
                );
            }
        }
        if flags.max_turns.is_none() {
            if let Some(max_turns) = agent.max_turns {
                options.insert(
                    "max_iterations".to_string(),
                    Value::Number(serde_json::Number::from(max_turns)),
                );
            }
        }
        return Ok(agent.initial_prompt);
    }
    Ok(None)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedCliAgent {
    name: String,
    prompt: String,
    initial_prompt: Option<String>,
    model: Option<String>,
    tools: Option<Vec<String>>,
    disallowed_tools: Option<Vec<String>>,
    permission_mode: Option<String>,
    max_turns: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawCliAgent {
    #[serde(default, alias = "system_prompt", alias = "systemPrompt")]
    prompt: Option<String>,
    #[serde(default, alias = "initial_prompt", alias = "initialPrompt")]
    initial_prompt: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tools: Option<Value>,
    #[serde(
        default,
        rename = "disallowedTools",
        alias = "disallowed_tools",
        alias = "disallowed-tools"
    )]
    disallowed_tools: Option<Value>,
    #[serde(
        default,
        rename = "permissionMode",
        alias = "permission_mode",
        alias = "permission-mode"
    )]
    permission_mode: Option<String>,
    #[serde(default, rename = "maxTurns", alias = "max_turns", alias = "max-turns")]
    max_turns: Option<Value>,
}

fn selected_cli_agent(flags: &RuntimeFlags) -> Result<Option<SelectedCliAgent>> {
    let flag_agents = if let Some(agents_json) = &flags.agents_json {
        Some(parse_cli_agents(agents_json)?)
    } else {
        None
    };
    let Some(agent_name) = flags.agent.as_deref() else {
        return Ok(None);
    };
    if let Some(agent) = flag_agents
        .as_ref()
        .and_then(|agents| agents.iter().find(|agent| agent.name == agent_name))
    {
        return Ok(Some(agent.clone()));
    }

    let agents = discover_agents(flags)?;
    let active_agents = agents
        .iter()
        .filter(|agent| agent.overridden_by.is_none())
        .collect::<Vec<_>>();
    if let Some(agent) = active_agents
        .iter()
        .copied()
        .find(|agent| agent.name == agent_name)
    {
        return selected_cli_agent_from_discovered(agent).map(Some);
    }

    let available = active_agents
        .into_iter()
        .map(|agent| agent.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    if available.is_empty() {
        Err(anyhow!("--agent '{}' was not found", agent_name))
    } else {
        Err(anyhow!(
            "--agent '{}' was not found; available agents: {}",
            agent_name,
            available
        ))
    }
}

fn selected_cli_agent_from_discovered(agent: &DiscoveredAgent) -> Result<SelectedCliAgent> {
    let prompt = agent
        .prompt
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "--agent '{}' does not define an executable prompt",
                agent.name
            )
        })?;
    Ok(SelectedCliAgent {
        name: agent.name.clone(),
        prompt,
        initial_prompt: agent.initial_prompt.clone(),
        model: agent.model.clone(),
        tools: agent.tools.clone(),
        disallowed_tools: agent.disallowed_tools.clone(),
        permission_mode: agent.permission_mode.clone(),
        max_turns: agent.max_turns,
    })
}

fn parse_cli_agents(raw: &str) -> Result<Vec<SelectedCliAgent>> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|error| anyhow!("--agents must be valid JSON: {}", error))?;
    let Some(object) = value.as_object() else {
        return Err(anyhow!(
            "--agents must be a JSON object keyed by agent name"
        ));
    };

    let mut agents = Vec::new();
    for (name, value) in object {
        let name = nonempty_flag_value("--agents agent name", name)?;
        let raw_agent: RawCliAgent = serde_json::from_value(value.clone())
            .map_err(|error| anyhow!("--agents '{}' is invalid: {}", name, error))?;
        let prompt = raw_agent
            .prompt
            .as_deref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("--agents '{}' requires a prompt", name))?;
        agents.push(SelectedCliAgent {
            name,
            prompt,
            initial_prompt: normalize_optional_text(raw_agent.initial_prompt),
            model: normalize_optional_text(raw_agent.model),
            tools: parse_agent_tools_json(raw_agent.tools.as_ref()),
            disallowed_tools: parse_agent_tools_json(raw_agent.disallowed_tools.as_ref()),
            permission_mode: normalize_permission_mode_text(raw_agent.permission_mode),
            max_turns: parse_positive_u64_json(raw_agent.max_turns.as_ref()),
        });
    }

    if agents.is_empty() {
        return Err(anyhow!("--agents must define at least one agent"));
    }
    Ok(agents)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_permission_mode_text(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_string();
    if value.is_empty() {
        return None;
    }
    Some(
        match value.as_str() {
            "accept-edits" | "accept_edits" | "acceptEdits" => "acceptEdits",
            "bypass-permissions" | "bypass_permissions" | "bypassPermissions" => {
                "bypassPermissions"
            }
            "dont-ask" | "dont_ask" | "dontAsk" => "dontAsk",
            "default" => "default",
            "plan" => "plan",
            "auto" => "auto",
            "ask" => "ask",
            _ => return None,
        }
        .to_string(),
    )
}

fn parse_agent_tools_json(value: Option<&Value>) -> Option<Vec<String>> {
    let tools = parse_tool_list_json(value)?;
    if tools.iter().any(|tool| tool == "*") {
        None
    } else {
        Some(tools)
    }
}

fn parse_tool_list_json(value: Option<&Value>) -> Option<Vec<String>> {
    let value = value?;
    let mut tools = Vec::new();
    match value {
        Value::Null => {}
        Value::String(value) => extend_tool_tokens(&mut tools, value),
        Value::Array(values) => {
            for value in values {
                if let Some(value) = value.as_str() {
                    extend_tool_tokens(&mut tools, value);
                }
            }
        }
        _ => {}
    }
    Some(tools)
}

fn extend_tool_tokens(target: &mut Vec<String>, value: &str) {
    for token in value
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(token))
        {
            target.push(token.to_string());
        }
    }
}

fn parse_positive_u64_json(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::Number(number) => number.as_u64().filter(|value| *value > 0),
        Value::String(text) => text.trim().parse::<u64>().ok().filter(|value| *value > 0),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveredAgent {
    name: String,
    source: AgentSource,
    description: Option<String>,
    prompt: Option<String>,
    initial_prompt: Option<String>,
    model: Option<String>,
    memory: Option<String>,
    tools: Option<Vec<String>>,
    disallowed_tools: Option<Vec<String>>,
    permission_mode: Option<String>,
    max_turns: Option<u64>,
    path: Option<PathBuf>,
    plugin: Option<String>,
    overridden_by: Option<AgentSource>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AgentSource {
    User,
    Project,
    Local,
    Plugin,
    Flag,
    BuiltIn,
}

impl AgentSource {
    fn label(self) -> &'static str {
        match self {
            AgentSource::User => "User agents",
            AgentSource::Project => "Project agents",
            AgentSource::Local => "Local agents",
            AgentSource::Plugin => "Plugin agents",
            AgentSource::Flag => "CLI arg agents",
            AgentSource::BuiltIn => "Built-in agents",
        }
    }

    fn name(self) -> &'static str {
        match self {
            AgentSource::User => "user",
            AgentSource::Project => "project",
            AgentSource::Local => "local",
            AgentSource::Plugin => "plugin",
            AgentSource::Flag => "flag",
            AgentSource::BuiltIn => "built-in",
        }
    }

    fn precedence(self) -> u8 {
        match self {
            AgentSource::BuiltIn => 10,
            AgentSource::Plugin => 20,
            AgentSource::User => 30,
            AgentSource::Project => 40,
            AgentSource::Local => 50,
            AgentSource::Flag => 60,
        }
    }
}

async fn agents_main(args: &[String], runtime_flags: &RuntimeFlags) -> Result<()> {
    if is_help_at(args, 1) {
        print_agents_help();
        return Ok(());
    }

    let parsed = parse_agents_args(args)?;
    let mut agents = discover_agents(runtime_flags)?;
    if let Some(sources) = &parsed.setting_sources {
        agents.retain(|agent| agent_source_matches_filter(agent.source, sources));
    }
    if parsed.json {
        println!("{}", serde_json::to_string_pretty(&agents_json(&agents))?);
    } else {
        println!("{}", format_agents_text(&agents));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentsArgs {
    json: bool,
    setting_sources: Option<Vec<String>>,
}

fn parse_agents_args(args: &[String]) -> Result<AgentsArgs> {
    let mut parsed = AgentsArgs {
        json: false,
        setting_sources: None,
    };
    let mut index = usize::from(args.first().map(String::as_str) == Some("agents"));
    while let Some(arg) = args.get(index).map(String::as_str) {
        match arg {
            "--json" => {
                parsed.json = true;
                index += 1;
            }
            "--setting-sources" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--setting-sources requires a value"))?;
                parsed.setting_sources = Some(parse_setting_sources(value));
                index += 2;
            }
            _ if arg.starts_with("--setting-sources=") => {
                parsed.setting_sources = Some(parse_setting_sources(
                    arg.trim_start_matches("--setting-sources="),
                ));
                index += 1;
            }
            other if is_help_arg(other) => {
                return Err(anyhow!(agents_usage()));
            }
            other => {
                return Err(anyhow!(
                    "unknown kiana agents option '{}'\n\n{}",
                    other,
                    agents_usage()
                ));
            }
        }
    }
    Ok(parsed)
}

fn parse_setting_sources(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn agent_source_matches_filter(source: AgentSource, filters: &[String]) -> bool {
    filters
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .any(|value| {
            matches!(
                (source, value.as_str()),
                (AgentSource::User, "user" | "usersettings" | "user-settings")
                    | (
                        AgentSource::Project,
                        "project" | "projectsettings" | "project-settings"
                    )
                    | (
                        AgentSource::Local,
                        "local" | "localsettings" | "local-settings"
                    )
                    | (AgentSource::Plugin, "plugin" | "plugins")
                    | (
                        AgentSource::Flag,
                        "flag" | "cli" | "flagsettings" | "flag-settings"
                    )
                    | (AgentSource::BuiltIn, "built-in" | "builtin" | "builtins")
            )
        })
}

fn discover_agents(runtime_flags: &RuntimeFlags) -> Result<Vec<DiscoveredAgent>> {
    let cwd = std::env::current_dir()?;
    let app_state = HashMap::from([("cwd".to_string(), Value::String(cwd.display().to_string()))]);
    let project_trust = kiana_types::project_trust_from_app_state(&app_state);
    let mut agents = Vec::new();
    agents.extend(discover_builtin_agents());
    agents.extend(discover_agents_from_settings_env()?);
    agents.extend(discover_agents_from_dirs(
        AgentSource::User,
        user_agent_dirs(),
        None,
    )?);
    if project_trust.allows_project_resources() {
        agents.extend(discover_agents_from_dirs(
            AgentSource::Project,
            vec![
                cwd.join(".kiana").join("agents"),
                cwd.join(".claude").join("agents"),
            ],
            None,
        )?);
        agents.extend(discover_agents_from_dirs(
            AgentSource::Local,
            vec![
                cwd.join(".kiana").join("agents-local"),
                cwd.join(".claude").join("agents-local"),
            ],
            None,
        )?);
    }
    agents.extend(discover_plugin_agents()?);
    if let Some(raw) = &runtime_flags.agents_json {
        agents.extend(discover_flag_agents(raw)?);
    }
    annotate_agent_overrides(&mut agents);
    agents.sort_by(|a, b| {
        source_group_order(a.source)
            .cmp(&source_group_order(b.source))
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
    });
    Ok(agents)
}

fn source_group_order(source: AgentSource) -> u8 {
    match source {
        AgentSource::User => 0,
        AgentSource::Project => 1,
        AgentSource::Local => 2,
        AgentSource::Plugin => 3,
        AgentSource::Flag => 4,
        AgentSource::BuiltIn => 5,
    }
}

fn discover_builtin_agents() -> Vec<DiscoveredAgent> {
    [
        (
            "general-purpose",
            "General-purpose agent for research, search, and multi-step tasks.",
            "You are a general-purpose coding subagent. Complete the delegated task in the given repository context and report concise, verifiable results.",
            None,
            None,
            None,
        ),
        (
            "statusline-setup",
            "Agent for setting up statusline configuration.",
            "You are a status line setup agent for Claude Code. Create or update the statusLine command in the user's Claude Code settings, preserve existing settings, and summarize what was configured.",
            Some("sonnet"),
            Some(vec!["Read", "Edit"]),
            None,
        ),
        (
            "claude-code-guide",
            "Agent that helps explain Claude Code usage and workflows.",
            "You are the Claude guide agent. Help users understand Claude Code, the Claude Agent SDK, and the Claude API. Prefer official documentation, use the docs maps when needed, and provide concise actionable guidance.",
            Some("haiku"),
            Some(vec!["Glob", "Grep", "Read", "WebFetch", "WebSearch"]),
            Some("dontAsk"),
        ),
    ]
    .into_iter()
    .map(|(name, description, prompt, model, tools, permission_mode)| DiscoveredAgent {
        name: name.to_string(),
        source: AgentSource::BuiltIn,
        description: Some(description.to_string()),
        prompt: Some(prompt.to_string()),
        initial_prompt: None,
        model: model.map(str::to_string),
        memory: None,
        tools: tools.map(|tools| tools.into_iter().map(str::to_string).collect()),
        disallowed_tools: None,
        permission_mode: permission_mode.map(str::to_string),
        max_turns: None,
        path: None,
        plugin: None,
        overridden_by: None,
    })
    .collect()
}

fn discover_agents_from_settings_env() -> Result<Vec<DiscoveredAgent>> {
    let mut agents = Vec::new();
    for key in [
        "KIANA_REMOTE_SETTINGS_FILE",
        "KIANA_SETTINGS_FILE",
        "KIANA_CONFIG_FILE",
    ] {
        if let Ok(path) = std::env::var(key) {
            agents.extend(discover_agents_from_settings_file(
                Path::new(&path),
                AgentSource::User,
            )?);
        }
    }
    if let Some(path) = kiana_bootstrap::config::config_path() {
        agents.extend(discover_agents_from_settings_file(
            &path,
            AgentSource::User,
        )?);
    }
    if let Ok(raw) = std::env::var("KIANA_SETTINGS_JSON") {
        agents.extend(discover_agents_from_settings_value(
            &parse_settings_value(&raw, "KIANA_SETTINGS_JSON")?,
            AgentSource::Flag,
            None,
        )?);
    }
    Ok(agents)
}

fn discover_agents_from_settings_file(
    path: &Path,
    source: AgentSource,
) -> Result<Vec<DiscoveredAgent>> {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let value = if path.extension().and_then(|ext| ext.to_str()) == Some("json")
        || contents.trim_start().starts_with('{')
    {
        parse_settings_value(&contents, &path.display().to_string())?
    } else {
        toml_value_to_json(
            toml::from_str::<toml::Value>(&contents)
                .with_context(|| format!("failed to parse settings file {}", path.display()))?,
        )
    };
    discover_agents_from_settings_value(&value, source, Some(path.to_path_buf()))
}

fn discover_agents_from_settings_value(
    value: &Value,
    source: AgentSource,
    path: Option<PathBuf>,
) -> Result<Vec<DiscoveredAgent>> {
    let Some(agents) = value.get("agents").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    parse_agent_map(agents, source, path, None)
}

fn parse_settings_value(raw: &str, label: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(|error| anyhow!("{label} is not valid JSON: {error}"))
}

fn user_agent_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("KIANA_HOME") {
        dirs.push(PathBuf::from(home).join("agents"));
    }
    if let Some(home) = dirs_next_home() {
        dirs.push(home.join(".kiana").join("agents"));
        dirs.push(home.join(".claude").join("agents"));
    }
    dirs
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
}

fn discover_plugin_agents() -> Result<Vec<DiscoveredAgent>> {
    let mut agents = Vec::new();
    for root in kiana_types::plugin::installed_plugin_roots() {
        let plugin_name = root
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("plugin")
            .to_string();
        agents.extend(discover_agents_from_dirs(
            AgentSource::Plugin,
            vec![root.join("agents")],
            Some(plugin_name),
        )?);
    }
    Ok(agents)
}

fn discover_flag_agents(raw: &str) -> Result<Vec<DiscoveredAgent>> {
    let agents = parse_cli_agents(raw)?;
    Ok(agents
        .into_iter()
        .map(|agent| DiscoveredAgent {
            name: agent.name,
            source: AgentSource::Flag,
            description: None,
            prompt: Some(agent.prompt),
            initial_prompt: agent.initial_prompt,
            model: agent.model,
            memory: None,
            tools: agent.tools,
            disallowed_tools: agent.disallowed_tools,
            permission_mode: agent.permission_mode,
            max_turns: agent.max_turns,
            path: None,
            plugin: None,
            overridden_by: None,
        })
        .collect())
}

fn discover_agents_from_dirs(
    source: AgentSource,
    dirs: Vec<PathBuf>,
    plugin: Option<String>,
) -> Result<Vec<DiscoveredAgent>> {
    let mut agents = Vec::new();
    let mut seen_paths = HashSet::new();
    for dir in dirs {
        if !dir.is_dir() || !seen_paths.insert(dir.clone()) {
            continue;
        }
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read agents directory {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if let Some(agent) = parse_agent_file(&path, source, plugin.clone())? {
                agents.push(agent);
            }
        }
    }
    Ok(agents)
}

fn parse_agent_file(
    path: &Path,
    source: AgentSource,
    plugin: Option<String>,
) -> Result<Option<DiscoveredAgent>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    match extension {
        "md" | "markdown" => parse_markdown_agent_file(path, source, plugin).map(Some),
        "json" => {
            let value: Value = serde_json::from_str(&std::fs::read_to_string(path)?)
                .with_context(|| format!("failed to parse agent JSON {}", path.display()))?;
            agent_from_value(
                &value,
                source,
                Some(path.to_path_buf()),
                plugin,
                path.file_stem().and_then(|value| value.to_str()),
            )
            .map(Some)
        }
        "toml" => {
            let value = toml_value_to_json(
                toml::from_str::<toml::Value>(&std::fs::read_to_string(path)?)
                    .with_context(|| format!("failed to parse agent TOML {}", path.display()))?,
            );
            agent_from_value(
                &value,
                source,
                Some(path.to_path_buf()),
                plugin,
                path.file_stem().and_then(|value| value.to_str()),
            )
            .map(Some)
        }
        _ => Ok(None),
    }
}

fn parse_markdown_agent_file(
    path: &Path,
    source: AgentSource,
    plugin: Option<String>,
) -> Result<DiscoveredAgent> {
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read agent file {}", path.display()))?;
    let (frontmatter, body) = split_markdown_frontmatter(&contents);
    let frontmatter = parse_simple_frontmatter(frontmatter.unwrap_or_default());
    let fallback_name = path.file_stem().and_then(|value| value.to_str());
    let name = frontmatter
        .get("name")
        .or_else(|| frontmatter.get("agentType"))
        .or_else(|| frontmatter.get("agent_type"))
        .and_then(Value::as_str)
        .or(fallback_name)
        .ok_or_else(|| anyhow!("agent file {} has no usable name", path.display()))?
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(anyhow!("agent file {} has an empty name", path.display()));
    }
    Ok(DiscoveredAgent {
        name,
        source,
        description: frontmatter_text(
            &frontmatter,
            &["description", "whenToUse", "when_to_use", "when-to-use"],
        ),
        prompt: Some(body.trim().to_string()).filter(|value| !value.is_empty()),
        initial_prompt: frontmatter_text(
            &frontmatter,
            &["initialPrompt", "initial_prompt", "initial-prompt"],
        ),
        model: frontmatter
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        tools: parse_agent_tools_json(frontmatter.get("tools")),
        disallowed_tools: parse_agent_tools_json(frontmatter_value(
            &frontmatter,
            &["disallowedTools", "disallowed_tools", "disallowed-tools"],
        )),
        permission_mode: normalize_permission_mode_text(frontmatter_text(
            &frontmatter,
            &["permissionMode", "permission_mode", "permission-mode"],
        )),
        max_turns: parse_positive_u64_json(frontmatter_value(
            &frontmatter,
            &["maxTurns", "max_turns", "max-turns"],
        )),
        memory: frontmatter
            .get("memory")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        path: Some(path.to_path_buf()),
        plugin,
        overridden_by: None,
    })
}

fn split_markdown_frontmatter(contents: &str) -> (Option<&str>, &str) {
    let trimmed = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"));
    let Some(rest) = trimmed else {
        return (None, contents);
    };
    if let Some(index) = rest.find("\n---\n") {
        return (Some(&rest[..index]), &rest[index + 5..]);
    }
    if let Some(index) = rest.find("\r\n---\r\n") {
        return (Some(&rest[..index]), &rest[index + 7..]);
    }
    (None, contents)
}

fn parse_simple_frontmatter(raw: &str) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        map.insert(
            key.trim().to_string(),
            parse_frontmatter_scalar(value.trim()),
        );
    }
    map
}

fn frontmatter_value<'a>(
    frontmatter: &'a serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<&'a Value> {
    keys.iter().find_map(|key| frontmatter.get(*key))
}

fn frontmatter_text(frontmatter: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    frontmatter_value(frontmatter, keys)
        .and_then(Value::as_str)
        .and_then(|value| normalize_optional_text(Some(value.to_string())))
}

fn parse_frontmatter_scalar(value: &str) -> Value {
    let value = value.trim().trim_matches('"').trim_matches('\'');
    if value.starts_with('[') && value.ends_with(']') {
        let items = value
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|item| item.trim().trim_matches('"').trim_matches('\''))
            .filter(|item| !item.is_empty())
            .map(|item| Value::String(item.to_string()))
            .collect::<Vec<_>>();
        return Value::Array(items);
    }
    match value {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(value.to_string()),
    }
}

fn parse_agent_map(
    agents: &serde_json::Map<String, Value>,
    source: AgentSource,
    path: Option<PathBuf>,
    plugin: Option<String>,
) -> Result<Vec<DiscoveredAgent>> {
    agents
        .iter()
        .map(|(name, value)| {
            agent_from_value(
                value,
                source,
                path.clone(),
                plugin.clone(),
                Some(name.as_str()),
            )
        })
        .collect()
}

fn agent_from_value(
    value: &Value,
    source: AgentSource,
    path: Option<PathBuf>,
    plugin: Option<String>,
    fallback_name: Option<&str>,
) -> Result<DiscoveredAgent> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("agent definition must be an object"))?;
    let name = object
        .get("name")
        .or_else(|| object.get("agentType"))
        .or_else(|| object.get("agent_type"))
        .and_then(Value::as_str)
        .or(fallback_name)
        .ok_or_else(|| anyhow!("agent definition is missing a name"))?
        .trim()
        .to_string();
    if name.is_empty() {
        return Err(anyhow!("agent definition has an empty name"));
    }
    let prompt = object
        .get("prompt")
        .or_else(|| object.get("system_prompt"))
        .or_else(|| object.get("systemPrompt"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Ok(DiscoveredAgent {
        name,
        source,
        description: object
            .get("description")
            .or_else(|| object.get("whenToUse"))
            .or_else(|| object.get("when_to_use"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        prompt,
        initial_prompt: object
            .get("initialPrompt")
            .or_else(|| object.get("initial_prompt"))
            .or_else(|| object.get("initial-prompt"))
            .and_then(Value::as_str)
            .and_then(|value| normalize_optional_text(Some(value.to_string()))),
        model: object
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        tools: parse_agent_tools_json(object.get("tools")),
        disallowed_tools: parse_agent_tools_json(
            object
                .get("disallowedTools")
                .or_else(|| object.get("disallowed_tools"))
                .or_else(|| object.get("disallowed-tools")),
        ),
        permission_mode: normalize_permission_mode_text(
            object
                .get("permissionMode")
                .or_else(|| object.get("permission_mode"))
                .or_else(|| object.get("permission-mode"))
                .and_then(Value::as_str)
                .map(str::to_string),
        ),
        max_turns: parse_positive_u64_json(
            object
                .get("maxTurns")
                .or_else(|| object.get("max_turns"))
                .or_else(|| object.get("max-turns")),
        ),
        memory: object
            .get("memory")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        path,
        plugin,
        overridden_by: None,
    })
}

fn toml_value_to_json(value: toml::Value) -> Value {
    match value {
        toml::Value::String(value) => Value::String(value),
        toml::Value::Integer(value) => Value::Number(value.into()),
        toml::Value::Float(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        toml::Value::Boolean(value) => Value::Bool(value),
        toml::Value::Datetime(value) => Value::String(value.to_string()),
        toml::Value::Array(values) => {
            Value::Array(values.into_iter().map(toml_value_to_json).collect())
        }
        toml::Value::Table(table) => Value::Object(
            table
                .into_iter()
                .map(|(key, value)| (key, toml_value_to_json(value)))
                .collect(),
        ),
    }
}

fn annotate_agent_overrides(agents: &mut [DiscoveredAgent]) {
    let mut winners: HashMap<String, AgentSource> = HashMap::new();
    for agent in agents.iter() {
        let entry = winners.entry(agent.name.clone()).or_insert(agent.source);
        if agent.source.precedence() > entry.precedence() {
            *entry = agent.source;
        }
    }
    for agent in agents {
        if let Some(winner) = winners.get(&agent.name) {
            if *winner != agent.source {
                agent.overridden_by = Some(*winner);
            }
        }
    }
}

fn active_agent_count(agents: &[DiscoveredAgent]) -> usize {
    agents
        .iter()
        .filter(|agent| agent.overridden_by.is_none())
        .count()
}

fn format_agents_text(agents: &[DiscoveredAgent]) -> String {
    if agents.is_empty() {
        return "No agents found.".to_string();
    }
    let mut lines = vec![
        format!("{} active agents", active_agent_count(agents)),
        String::new(),
    ];
    for source in [
        AgentSource::User,
        AgentSource::Project,
        AgentSource::Local,
        AgentSource::Plugin,
        AgentSource::Flag,
        AgentSource::BuiltIn,
    ] {
        let group = agents
            .iter()
            .filter(|agent| agent.source == source)
            .collect::<Vec<_>>();
        if group.is_empty() {
            continue;
        }
        lines.push(format!("{}:", source.label()));
        for agent in group {
            let mut prefix = "  ".to_string();
            if let Some(overridden_by) = agent.overridden_by {
                prefix.push_str(&format!("(shadowed by {}) ", overridden_by.name()));
            }
            lines.push(format!("{prefix}{}", format_agent_line(agent)));
        }
        lines.push(String::new());
    }
    lines.join("\n").trim_end().to_string()
}

fn format_agent_line(agent: &DiscoveredAgent) -> String {
    let mut parts = vec![agent.name.clone()];
    if let Some(model) = &agent.model {
        parts.push(model.clone());
    }
    if let Some(memory) = &agent.memory {
        parts.push(format!("{memory} memory"));
    }
    if let Some(plugin) = &agent.plugin {
        parts.push(format!("plugin: {plugin}"));
    }
    parts.join(" · ")
}

fn agents_json(agents: &[DiscoveredAgent]) -> Value {
    let values = agents
        .iter()
        .map(|agent| {
            serde_json::json!({
                "name": agent.name,
                "source": agent.source.name(),
                "active": agent.overridden_by.is_none(),
                "overridden_by": agent.overridden_by.map(AgentSource::name),
                "description": agent.description,
                "model": agent.model,
                "memory": agent.memory,
                "tools": agent.tools,
                "disallowed_tools": agent.disallowed_tools,
                "permission_mode": agent.permission_mode,
                "max_turns": agent.max_turns,
                "path": agent.path.as_ref().map(|path| path.display().to_string()),
                "plugin": agent.plugin,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "active_count": active_agent_count(agents),
        "agents": values,
    })
}

fn agents_usage() -> &'static str {
    "Usage: kiana agents [--json] [--setting-sources <sources>]"
}

fn print_agents_help() {
    println!("{}", agents_usage());
    println!();
    println!("List configured user, project, plugin, CLI, and built-in agents.");
    println!();
    println!("Options:");
    println!("  --json                         Output machine-readable JSON");
    println!("  --setting-sources <sources>    Accepted for reference CLI compatibility");
}

fn prepend_initial_prompt(message: String, initial_prompt: Option<&str>) -> String {
    let Some(initial_prompt) = initial_prompt
        .map(str::trim)
        .filter(|initial_prompt| !initial_prompt.is_empty())
    else {
        return message;
    };
    if message.trim().is_empty() {
        return initial_prompt.to_string();
    }
    format!("{initial_prompt}\n\n{message}")
}

fn apply_mcp_config_flags(values: &[String]) -> Result<()> {
    let servers = merge_mcp_config_values(values)?;
    std::env::set_var(
        kiana_tools::mcp_tool::MCP_SERVERS_ENV,
        serde_json::to_string(&servers)?,
    );
    Ok(())
}

fn merge_mcp_config_values(values: &[String]) -> Result<Value> {
    let mut merged = serde_json::Map::new();
    for value in values {
        let servers = parse_mcp_config_value(value)?;
        let Some(object) = servers.as_object() else {
            return Err(anyhow!("--mcp-config must resolve to an object of servers"));
        };
        for (name, config) in object {
            if name.trim().is_empty() {
                return Err(anyhow!("--mcp-config contains an empty MCP server name"));
            }
            merged.insert(name.clone(), config.clone());
        }
    }
    if merged.is_empty() {
        return Err(anyhow!("--mcp-config did not define any MCP servers"));
    }
    Ok(Value::Object(merged))
}

fn parse_mcp_config_value(value: &str) -> Result<Value> {
    let value = nonempty_flag_value("--mcp-config", value)?;
    let config = match serde_json::from_str::<Value>(&value) {
        Ok(config) => config,
        Err(json_error) => {
            let path = Path::new(&value);
            let contents = std::fs::read_to_string(path).map_err(|file_error| {
                anyhow!(
                    "failed to parse --mcp-config as JSON ({}) or read path '{}': {}",
                    json_error,
                    value,
                    file_error
                )
            })?;
            serde_json::from_str::<Value>(&contents).map_err(|error| {
                anyhow!(
                    "failed to parse --mcp-config path '{}' as JSON: {}",
                    value,
                    error
                )
            })?
        }
    };
    extract_mcp_servers(config)
}

fn extract_mcp_servers(config: Value) -> Result<Value> {
    let Some(object) = config.as_object() else {
        return Err(anyhow!("--mcp-config JSON must be an object"));
    };
    if let Some(servers) = object
        .get("mcpServers")
        .or_else(|| object.get("mcp_servers"))
    {
        if servers.as_object().is_none() {
            return Err(anyhow!("--mcp-config mcpServers must be an object"));
        }
        return Ok(servers.clone());
    }

    if object.values().any(|value| {
        value
            .as_object()
            .is_some_and(|server| server.contains_key("command") || server.contains_key("url"))
    }) {
        return Ok(Value::Object(object.clone()));
    }

    Err(anyhow!(
        "--mcp-config must contain mcpServers or be an object keyed by server name"
    ))
}

fn apply_add_dirs_flag(values: &[String]) -> Result<()> {
    let mut roots = Vec::new();
    for value in values {
        roots.push(normalize_add_dir(value)?);
    }
    let joined = std::env::join_paths(&roots)
        .map_err(|error| anyhow!("failed to encode --add-dir values: {}", error))?;
    std::env::set_var(kiana_tools::tool::ACCESS_ROOTS_ENV, joined);
    Ok(())
}

fn normalize_add_dir(value: &str) -> Result<PathBuf> {
    let value = nonempty_flag_value("--add-dir", value)?;
    let path = PathBuf::from(&value);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    let path = std::fs::canonicalize(&path)
        .map_err(|error| anyhow!("failed to resolve --add-dir '{}': {}", value, error))?;
    if !path.is_dir() {
        return Err(anyhow!(
            "--add-dir path is not a directory: {}",
            path.display()
        ));
    }
    Ok(path)
}

fn apply_settings_flag(value: &str) -> Result<()> {
    let value = nonempty_flag_value("--settings", value)?;
    if value.trim_start().starts_with('{') {
        validate_inline_settings_json(&value)?;
        std::env::set_var("KIANA_SETTINGS_JSON", value);
        return Ok(());
    }

    let path = Path::new(&value);
    let contents = std::fs::read_to_string(path)
        .map_err(|error| anyhow!("failed to read --settings path '{}': {}", value, error))?;
    if path.extension().and_then(|ext| ext.to_str()) == Some("json")
        || contents.trim_start().starts_with('{')
    {
        validate_inline_settings_json(&contents)?;
    }
    std::env::set_var("KIANA_SETTINGS_FILE", path);
    Ok(())
}

fn validate_inline_settings_json(contents: &str) -> Result<()> {
    let value: Value = serde_json::from_str(contents)
        .map_err(|error| anyhow!("--settings JSON is invalid: {}", error))?;
    if !value.is_object() {
        return Err(anyhow!("--settings JSON must be an object"));
    }
    Ok(())
}

fn runtime_text_flag(
    value_flag: &str,
    value: Option<&str>,
    file_flag: &str,
    file: Option<&str>,
) -> Result<Option<String>> {
    if let Some(value) = value {
        return Ok(Some(nonempty_flag_value(value_flag, value)?));
    }
    let Some(path) = file else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(path)
        .map_err(|error| anyhow!("failed to read {file_flag} path '{}': {}", path, error))?;
    Ok(Some(nonempty_flag_value(file_flag, &text)?))
}

#[derive(Debug, PartialEq, Eq)]
struct ChromeNativeHostInstallArgs {
    dry_run: bool,
    json: bool,
    home_dir: Option<std::path::PathBuf>,
    binary_path: Option<std::path::PathBuf>,
    platform: Option<kiana_chrome_mcp::native_install::NativeHostPlatform>,
    include_dev_origins: bool,
}

async fn chrome_main(args: &[String]) -> Result<()> {
    let command_index = if matches!(
        args.first().map(String::as_str),
        Some("chrome" | "chrome-native-host")
    ) {
        1
    } else {
        0
    };
    let command = args
        .get(command_index)
        .map(String::as_str)
        .unwrap_or("help");

    match command {
        "help" | "--help" | "-h" => {
            print_chrome_help();
            Ok(())
        }
        "install-native-host" | "install" | "setup" => {
            if args
                .get(command_index + 1)
                .map(String::as_str)
                .is_some_and(|arg| matches!(arg, "--help" | "-h"))
            {
                print_chrome_help();
                return Ok(());
            }
            let parsed = parse_chrome_native_host_install_args(args, command_index + 1)?;
            let mut options = kiana_chrome_mcp::native_install::default_install_options(
                parsed
                    .binary_path
                    .clone()
                    .unwrap_or(std::env::current_exe()?),
            );
            if let Some(platform) = parsed.platform {
                options.platform = platform;
            }
            if let Some(home_dir) = parsed.home_dir {
                options.home_dir = home_dir;
            }
            if let Some(binary_path) = parsed.binary_path {
                options.binary_path = binary_path;
            }
            if parsed.include_dev_origins {
                options.include_dev_origins = true;
            }

            if parsed.dry_run {
                let plan = kiana_chrome_mcp::native_install::plan_native_host_install(&options)?;
                if parsed.json {
                    println!("{}", serde_json::to_string_pretty(&plan)?);
                } else {
                    print_native_host_install_plan(&plan);
                }
                return Ok(());
            }

            let report = kiana_chrome_mcp::native_install::install_native_host(&options)?;
            if parsed.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_native_host_install_report(&report);
            }
            Ok(())
        }
        "plan-native-host" | "plan" => {
            let mut parsed = parse_chrome_native_host_install_args(args, command_index + 1)?;
            parsed.dry_run = true;
            let mut options = kiana_chrome_mcp::native_install::default_install_options(
                parsed
                    .binary_path
                    .clone()
                    .unwrap_or(std::env::current_exe()?),
            );
            if let Some(platform) = parsed.platform {
                options.platform = platform;
            }
            if let Some(home_dir) = parsed.home_dir {
                options.home_dir = home_dir;
            }
            if let Some(binary_path) = parsed.binary_path {
                options.binary_path = binary_path;
            }
            if parsed.include_dev_origins {
                options.include_dev_origins = true;
            }
            let plan = kiana_chrome_mcp::native_install::plan_native_host_install(&options)?;
            if parsed.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                print_native_host_install_plan(&plan);
            }
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown chrome command '{}'\n\nUsage: kiana chrome install-native-host [--dry-run] [--json]",
            command
        )),
    }
}

fn parse_chrome_native_host_install_args(
    args: &[String],
    mut index: usize,
) -> Result<ChromeNativeHostInstallArgs> {
    let mut parsed = ChromeNativeHostInstallArgs {
        dry_run: false,
        json: false,
        home_dir: None,
        binary_path: None,
        platform: None,
        include_dev_origins: false,
    };

    while let Some(arg) = args.get(index).map(String::as_str) {
        match arg {
            "--dry-run" | "--plan" => parsed.dry_run = true,
            "--json" => parsed.json = true,
            "--include-dev-origins" => parsed.include_dev_origins = true,
            "--home" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--home requires a path"))?;
                parsed.home_dir = Some(std::path::PathBuf::from(value));
                index += 2;
                continue;
            }
            _ if arg.starts_with("--home=") => {
                parsed.home_dir = Some(std::path::PathBuf::from(
                    arg.strip_prefix("--home=").unwrap_or_default(),
                ));
            }
            "--binary" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--binary requires a path"))?;
                parsed.binary_path = Some(std::path::PathBuf::from(value));
                index += 2;
                continue;
            }
            _ if arg.starts_with("--binary=") => {
                parsed.binary_path = Some(std::path::PathBuf::from(
                    arg.strip_prefix("--binary=").unwrap_or_default(),
                ));
            }
            "--platform" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--platform requires linux, macos, or windows"))?;
                parsed.platform = Some(parse_native_host_platform(value)?);
                index += 2;
                continue;
            }
            _ if arg.starts_with("--platform=") => {
                parsed.platform = Some(parse_native_host_platform(
                    arg.strip_prefix("--platform=").unwrap_or_default(),
                )?);
            }
            "--help" | "-h" => {
                return Err(anyhow!("usage: kiana chrome install-native-host [options]"))
            }
            _ => {
                return Err(anyhow!(
                    "unknown chrome install option '{}'\n\nUsage: kiana chrome install-native-host [--dry-run] [--json] [--home <path>] [--binary <path>] [--platform linux|macos|windows]",
                    arg
                ));
            }
        }
        index += 1;
    }

    Ok(parsed)
}

fn parse_native_host_platform(
    value: &str,
) -> Result<kiana_chrome_mcp::native_install::NativeHostPlatform> {
    match value {
        "linux" => Ok(kiana_chrome_mcp::native_install::NativeHostPlatform::Linux),
        "macos" | "darwin" => Ok(kiana_chrome_mcp::native_install::NativeHostPlatform::Macos),
        "windows" | "win32" => Ok(kiana_chrome_mcp::native_install::NativeHostPlatform::Windows),
        _ => Err(anyhow!("--platform must be linux, macos, or windows")),
    }
}

fn print_native_host_install_plan(plan: &kiana_chrome_mcp::native_install::NativeHostInstallPlan) {
    println!("Chrome native host install plan");
    println!("identifier: {}", plan.identifier);
    println!("wrapper: {}", plan.wrapper_path.display());
    println!("manifests:");
    for path in &plan.manifest_paths {
        println!("  {}", path.display());
    }
    if !plan.windows_registry_plans.is_empty() {
        println!("windows_registry:");
        for plan in &plan.windows_registry_plans {
            println!("  {}: {}", plan.browser, plan.add.join(" "));
        }
    }
}

fn print_native_host_install_report(
    report: &kiana_chrome_mcp::native_install::NativeHostInstallReport,
) {
    print_native_host_install_plan(&report.plan);
    println!("written_files:");
    for path in &report.written_files {
        println!("  {}", path.display());
    }
    println!("skipped_files:");
    for path in &report.skipped_files {
        println!("  {}", path.display());
    }
}

fn print_chrome_help() {
    println!("Usage: kiana chrome install-native-host [options]");
    println!();
    println!("Options:");
    println!("  --dry-run                  Print the install plan without writing files");
    println!("  --json                     Print JSON plan/report");
    println!("  --home <path>              Override home directory for manifest paths");
    println!("  --binary <path>            Binary path the wrapper should execute");
    println!("  --platform <platform>      linux, macos, or windows");
    println!("  --include-dev-origins      Include dev Chrome extension IDs");
}

async fn url_main(args: &[String]) -> Result<()> {
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    if is_help_at(args, 2) {
        print_url_help();
        return Ok(());
    }

    match command {
        "handle" | "open" => {
            let raw_url = args
                .get(2)
                .ok_or_else(|| anyhow!("usage: kiana url handle <url>"))?;
            let parsed =
                url::Url::parse(raw_url).map_err(|error| anyhow!("invalid URL: {}", error))?;
            print_handled_url(&parsed);
            Ok(())
        }
        "plan" => {
            let scheme = args.get(2).map(String::as_str).unwrap_or("kiana");
            let scheme = kiana_url_handler::UrlHandler::validate_scheme(scheme)
                .map_err(|error| anyhow!(error))?;
            let command = main_kiana_url_handler_command(std::env::current_exe()?);
            let mut plan = serde_json::Map::new();
            plan.insert("scheme".to_string(), serde_json::json!(scheme));
            plan.insert("command".to_string(), serde_json::json!(command));
            plan.insert(
                "platform".to_string(),
                serde_json::json!(std::env::consts::OS),
            );
            plan.insert(
                "registration_supported".to_string(),
                serde_json::json!(cfg!(target_os = "linux")),
            );
            #[cfg(target_os = "linux")]
            {
                plan.insert(
                    "desktop_entry_path".to_string(),
                    serde_json::json!(kiana_url_handler::UrlHandler::linux_desktop_entry_path(
                        &scheme
                    )
                    .map_err(|error| anyhow!(error))?),
                );
            }
            let plan = serde_json::Value::Object(plan);
            println!("{}", serde_json::to_string_pretty(&plan)?);
            Ok(())
        }
        "register" | "install" => {
            let scheme = args.get(2).map(String::as_str).unwrap_or("kiana");
            if is_help_arg(scheme) {
                print_url_help();
                return Ok(());
            }
            register_main_url_scheme(scheme)
        }
        "status" => {
            let scheme = args.get(2).map(String::as_str).unwrap_or("kiana");
            print_main_url_status(scheme)
        }
        "help" | "--help" | "-h" => {
            print_url_help();
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown url command '{}'\n\nUsage: kiana url <handle|plan|register|status>",
            command
        )),
    }
}

#[cfg(target_os = "linux")]
fn register_main_url_scheme(scheme: &str) -> Result<()> {
    let command = main_kiana_url_handler_command(std::env::current_exe()?);
    let path = kiana_url_handler::UrlHandler::register_linux_scheme_with_command(scheme, &command)
        .map_err(|error| anyhow!(error))?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn register_main_url_scheme(_scheme: &str) -> Result<()> {
    Err(anyhow!(
        "URL scheme registration from the main kiana binary is currently implemented on Linux"
    ))
}

#[cfg(target_os = "linux")]
fn print_main_url_status(scheme: &str) -> Result<()> {
    println!(
        "{}",
        kiana_url_handler::UrlHandler::linux_registration_status(scheme)
            .map_err(|error| anyhow!(error))?
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn print_main_url_status(_scheme: &str) -> Result<()> {
    Err(anyhow!(
        "URL scheme status from the main kiana binary is currently implemented on Linux"
    ))
}

fn main_kiana_url_handler_command(binary_path: impl AsRef<std::path::Path>) -> String {
    format!(
        "{} url handle %u",
        kiana_url_handler::UrlHandler::quote_exec_arg(&binary_path.as_ref().to_string_lossy())
    )
}

fn print_handled_url(parsed: &url::Url) {
    println!("{}", handled_url_json(parsed));
}

fn handled_url_json(parsed: &url::Url) -> Value {
    serde_json::json!({
        "handled": true,
        "url": parsed.as_str(),
        "scheme": parsed.scheme(),
        "action": parsed.host_str().unwrap_or_default(),
        "path": parsed.path(),
        "query": parsed.query().unwrap_or_default(),
    })
}

fn print_url_help() {
    println!("Usage: kiana url <handle|plan|register|status>");
    println!();
    println!("Commands:");
    println!("  handle <url>       Parse and report an incoming deep link");
    println!("  plan [scheme]      Show OS registration plan for this kiana binary");
    println!("  register [scheme]  Register this kiana binary as the URL handler");
    println!("  status [scheme]    Show current URL handler status");
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DirectConnectTransport {
    Http,
    Unix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectConnectTarget {
    server_url: String,
    auth_token: Option<String>,
    transport: DirectConnectTransport,
    unix_socket: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectConnectOpenArgs {
    raw_url: String,
    target: DirectConnectTarget,
    prompt: String,
    output_format: PrintOutputFormat,
    dangerously_skip_permissions: bool,
}

#[derive(Debug, Deserialize)]
struct DirectConnectSessionResponse {
    session_id: String,
    ws_url: String,
    #[serde(default)]
    work_dir: Option<String>,
}

async fn direct_connect_open_main(args: &[String]) -> Result<()> {
    if direct_connect_open_help_requested(args) {
        print_direct_connect_open_help();
        return Ok(());
    }
    let open_args = parse_direct_connect_open_args(args)?;
    let mut stdout = std::io::stdout();
    direct_connect_open_with_writer(&open_args, &mut stdout).await
}

fn direct_connect_open_help_requested(args: &[String]) -> bool {
    let offset = usize::from(args.first().map(String::as_str) == Some("open"));
    args.get(offset).is_none_or(|arg| is_help_arg(arg))
}

fn parse_direct_connect_open_args(args: &[String]) -> Result<DirectConnectOpenArgs> {
    let offset = usize::from(args.first().map(String::as_str) == Some("open"));
    let raw_url = args
        .get(offset)
        .ok_or_else(|| anyhow!("Usage: kiana open <cc-url> -p <prompt>"))?
        .to_string();
    let target = parse_direct_connect_url(&raw_url)?;
    let mut output_format = PrintOutputFormat::Text;
    let mut dangerously_skip_permissions = false;
    let mut saw_print = false;
    let mut prompt_parts = Vec::new();
    let mut index = offset + 1;

    while let Some(arg) = args.get(index).map(String::as_str) {
        match arg {
            "-p" | "--print" => {
                saw_print = true;
                index += 1;
                if let Some(value) = args.get(index).filter(|value| !value.starts_with('-')) {
                    prompt_parts.push(value.clone());
                    index += 1;
                }
            }
            _ if arg.starts_with("--print=") => {
                saw_print = true;
                if let Some(value) = arg.strip_prefix("--print=") {
                    if !value.trim().is_empty() {
                        prompt_parts.push(value.to_string());
                    }
                }
                index += 1;
            }
            "--output-format" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--output-format requires a value"))?;
                output_format = parse_print_output_format(value)?;
                index += 2;
            }
            _ if arg.starts_with("--output-format=") => {
                let value = arg
                    .strip_prefix("--output-format=")
                    .ok_or_else(|| anyhow!("--output-format requires a value"))?;
                output_format = parse_print_output_format(value)?;
                index += 1;
            }
            "--dangerously-skip-permissions" => {
                dangerously_skip_permissions = true;
                index += 1;
            }
            "--" => {
                prompt_parts.extend(args.get(index + 1..).unwrap_or_default().iter().cloned());
                break;
            }
            _ if arg.starts_with('-') => {
                return Err(anyhow!(
                    "unknown kiana open option '{}'\n\n{}",
                    arg,
                    direct_connect_open_usage()
                ));
            }
            _ => {
                prompt_parts.push(arg.to_string());
                index += 1;
            }
        }
    }

    if !saw_print {
        return Err(anyhow!(
            "kiana open currently supports headless direct-connect mode; use `kiana open <cc-url> -p <prompt>`"
        ));
    }
    let prompt = prompt_parts.join(" ").trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!("kiana open -p requires a prompt"));
    }

    Ok(DirectConnectOpenArgs {
        raw_url,
        target,
        prompt,
        output_format,
        dangerously_skip_permissions,
    })
}

fn parse_direct_connect_url(raw_url: &str) -> Result<DirectConnectTarget> {
    let parsed =
        url::Url::parse(raw_url).map_err(|error| anyhow!("invalid direct-connect URL: {error}"))?;
    match parsed.scheme() {
        "cc" => parse_http_direct_connect_url(parsed),
        "cc+unix" => {
            let socket_path = parsed.path().trim();
            if socket_path.is_empty() || socket_path == "/" {
                return Err(anyhow!("cc+unix URL requires a socket path"));
            }
            Ok(DirectConnectTarget {
                server_url: format!("unix:{socket_path}"),
                auth_token: direct_connect_auth_token(&parsed),
                transport: DirectConnectTransport::Unix,
                unix_socket: Some(PathBuf::from(socket_path)),
            })
        }
        "http" | "https" => Ok(DirectConnectTarget {
            server_url: raw_url.trim_end_matches('/').to_string(),
            auth_token: direct_connect_auth_token(&parsed),
            transport: DirectConnectTransport::Http,
            unix_socket: None,
        }),
        other => Err(anyhow!(
            "unsupported direct-connect URL scheme '{}'; expected cc://, cc+unix://, http://, or https://",
            other
        )),
    }
}

fn parse_http_direct_connect_url(parsed: url::Url) -> Result<DirectConnectTarget> {
    let host = parsed
        .host_str()
        .filter(|host| !host.trim().is_empty())
        .ok_or_else(|| anyhow!("cc:// URL requires a host"))?;
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let base_path = parsed.path().trim_end_matches('/');
    let base_path = if base_path.is_empty() || base_path == "/" {
        ""
    } else {
        base_path
    };
    let server_scheme = parsed
        .query_pairs()
        .find_map(|(key, value)| {
            (key == "scheme" || key == "protocol")
                .then(|| value.to_string())
                .filter(|value| value == "http" || value == "https")
        })
        .unwrap_or_else(|| "http".to_string());
    Ok(DirectConnectTarget {
        server_url: format!("{server_scheme}://{host}{port}{base_path}"),
        auth_token: direct_connect_auth_token(&parsed),
        transport: DirectConnectTransport::Http,
        unix_socket: None,
    })
}

fn direct_connect_auth_token(parsed: &url::Url) -> Option<String> {
    parsed
        .query_pairs()
        .find_map(|(key, value)| {
            matches!(
                key.as_ref(),
                "token" | "authToken" | "auth_token" | "access_token"
            )
            .then(|| value.trim().to_string())
            .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            parsed
                .password()
                .map(str::to_string)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            let username = parsed.username();
            (!username.is_empty()).then(|| username.to_string())
        })
}

async fn direct_connect_open_with_writer<W: Write>(
    open_args: &DirectConnectOpenArgs,
    writer: &mut W,
) -> Result<()> {
    let session = create_direct_connect_session(open_args).await?;
    run_direct_connect_headless(open_args, &session, writer).await
}

async fn create_direct_connect_session(
    open_args: &DirectConnectOpenArgs,
) -> Result<DirectConnectSessionResponse> {
    if matches!(open_args.target.transport, DirectConnectTransport::Unix) {
        return create_direct_connect_unix_session(open_args).await;
    }

    let url = format!(
        "{}/sessions",
        open_args.target.server_url.trim_end_matches('/')
    );
    let cwd = std::env::current_dir()?;
    let mut body = serde_json::json!({
        "cwd": cwd.to_string_lossy(),
    });
    if open_args.dangerously_skip_permissions {
        body["dangerously_skip_permissions"] = Value::Bool(true);
    }

    let client = reqwest::Client::new();
    let mut request = client.post(&url).json(&body);
    if let Some(token) = &open_args.target.auth_token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|error| {
        anyhow!(
            "failed to connect to direct-connect server at {}: {}",
            open_args.target.server_url,
            error
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("failed to create direct-connect session: {status}"));
    }
    response
        .json::<DirectConnectSessionResponse>()
        .await
        .context("direct-connect session response was not valid JSON")
}

#[cfg(unix)]
async fn create_direct_connect_unix_session(
    open_args: &DirectConnectOpenArgs,
) -> Result<DirectConnectSessionResponse> {
    let cwd = std::env::current_dir()?;
    let mut body = serde_json::json!({
        "cwd": cwd.to_string_lossy(),
    });
    if open_args.dangerously_skip_permissions {
        body["dangerously_skip_permissions"] = Value::Bool(true);
    }
    let body = serde_json::to_vec(&body)?;
    let response = direct_connect_unix_http_json(
        direct_connect_unix_socket_path(&open_args.target)?,
        "POST",
        "/sessions",
        open_args.target.auth_token.as_deref(),
        &body,
    )
    .await?;
    serde_json::from_slice::<DirectConnectSessionResponse>(&response)
        .context("direct-connect Unix session response was not valid JSON")
}

#[cfg(not(unix))]
async fn create_direct_connect_unix_session(
    _open_args: &DirectConnectOpenArgs,
) -> Result<DirectConnectSessionResponse> {
    Err(anyhow!(
        "cc+unix direct-connect URLs require Unix socket support on this platform"
    ))
}

#[cfg(unix)]
async fn direct_connect_unix_http_json(
    socket_path: &Path,
    method: &str,
    path: &str,
    auth_token: Option<&str>,
    body: &[u8],
) -> Result<Vec<u8>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::UnixStream::connect(socket_path)
        .await
        .with_context(|| {
            format!(
                "failed to connect to direct-connect Unix socket at {}",
                socket_path.display()
            )
        })?;
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: kiana.local\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(token) = auth_token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.shutdown().await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    direct_connect_parse_http_response(&response)
}

#[cfg(unix)]
fn direct_connect_parse_http_response(response: &[u8]) -> Result<Vec<u8>> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| anyhow!("direct-connect Unix HTTP response was missing headers"))?;
    let headers = std::str::from_utf8(&response[..header_end])
        .context("direct-connect Unix HTTP response headers were not UTF-8")?;
    let mut lines = headers.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| anyhow!("direct-connect Unix HTTP response was empty"))?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("direct-connect Unix HTTP response had invalid status line"))?;
    let body = response[(header_end + 4)..].to_vec();
    if !(200..300).contains(&status_code) {
        let text = String::from_utf8_lossy(&body);
        return Err(anyhow!(
            "failed direct-connect Unix HTTP request: status={} body={}",
            status_code,
            text.trim()
        ));
    }
    Ok(body)
}

async fn run_direct_connect_headless<W: Write>(
    open_args: &DirectConnectOpenArgs,
    session: &DirectConnectSessionResponse,
    writer: &mut W,
) -> Result<()> {
    if matches!(open_args.target.transport, DirectConnectTransport::Unix) {
        return run_direct_connect_unix_headless(open_args, session, writer).await;
    }

    let mut request = session
        .ws_url
        .as_str()
        .into_client_request()
        .map_err(|error| anyhow!("invalid direct-connect websocket URL: {error}"))?;
    if let Some(token) = &open_args.target.auth_token {
        let header = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|error| anyhow!("invalid direct-connect auth token: {error}"))?;
        request.headers_mut().insert(AUTHORIZATION, header);
    }
    let (mut websocket, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|error| anyhow!("failed to connect direct-connect websocket: {error}"))?;
    run_direct_connect_websocket(open_args, session, &mut websocket, writer).await
}

#[cfg(unix)]
async fn run_direct_connect_unix_headless<W: Write>(
    open_args: &DirectConnectOpenArgs,
    session: &DirectConnectSessionResponse,
    writer: &mut W,
) -> Result<()> {
    let path = format!("/sessions/{}/ws", session.session_id);
    let mut request = format!("ws://kiana.local{path}")
        .into_client_request()
        .map_err(|error| anyhow!("invalid direct-connect Unix websocket path: {error}"))?;
    if let Some(token) = &open_args.target.auth_token {
        let header = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|error| anyhow!("invalid direct-connect auth token: {error}"))?;
        request.headers_mut().insert(AUTHORIZATION, header);
    }
    let stream =
        tokio::net::UnixStream::connect(direct_connect_unix_socket_path(&open_args.target)?)
            .await
            .with_context(|| {
                format!(
                    "failed to connect direct-connect Unix websocket at {}",
                    open_args
                        .target
                        .unix_socket
                        .as_deref()
                        .unwrap_or_else(|| Path::new("<missing>"))
                        .display()
                )
            })?;
    let (mut websocket, _) = tokio_tungstenite::client_async(request, stream)
        .await
        .map_err(|error| anyhow!("failed to connect direct-connect Unix websocket: {error}"))?;
    run_direct_connect_websocket(open_args, session, &mut websocket, writer).await
}

#[cfg(not(unix))]
async fn run_direct_connect_unix_headless<W: Write>(
    _open_args: &DirectConnectOpenArgs,
    _session: &DirectConnectSessionResponse,
    _writer: &mut W,
) -> Result<()> {
    Err(anyhow!(
        "cc+unix direct-connect URLs require Unix socket support on this platform"
    ))
}

async fn run_direct_connect_websocket<W, S>(
    open_args: &DirectConnectOpenArgs,
    session: &DirectConnectSessionResponse,
    websocket: &mut tokio_tungstenite::WebSocketStream<S>,
    writer: &mut W,
) -> Result<()>
where
    W: Write,
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use futures_util::{SinkExt, StreamExt};

    let user_message = serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": open_args.prompt,
        },
        "parent_tool_use_id": null,
        "session_id": session.session_id,
    });
    websocket
        .send(Message::Text(user_message.to_string().into()))
        .await
        .map_err(|error| anyhow!("failed to send direct-connect prompt: {error}"))?;

    let mut assistant_text = Vec::new();
    let mut final_event = None;
    while let Some(message) = websocket.next().await {
        match message.map_err(|error| anyhow!("direct-connect websocket read failed: {error}"))? {
            Message::Text(text) => {
                for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                    let event: Value = serde_json::from_str(line).with_context(|| {
                        format!("direct-connect websocket message was not JSON: {line}")
                    })?;
                    if matches!(open_args.output_format, PrintOutputFormat::StreamJson) {
                        write_json_line(writer, &event)?;
                    }
                    if let Some(response) = direct_connect_control_response(&event) {
                        websocket
                            .send(Message::Text(response.to_string().into()))
                            .await
                            .map_err(|error| {
                                anyhow!("failed to send direct-connect control response: {error}")
                            })?;
                        continue;
                    }
                    match event.get("type").and_then(Value::as_str) {
                        Some("assistant") => {
                            if let Some(text) = direct_connect_event_text(&event) {
                                assistant_text.push(text);
                            }
                        }
                        Some("result") => {
                            final_event = Some(event);
                            break;
                        }
                        _ => {}
                    }
                }
                if final_event.is_some() {
                    break;
                }
            }
            Message::Ping(payload) => {
                websocket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| anyhow!("failed to answer direct-connect ping: {error}"))?;
            }
            Message::Close(_) => break,
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }

    if matches!(open_args.output_format, PrintOutputFormat::StreamJson) {
        return Ok(());
    }

    let result = final_event.unwrap_or_else(|| {
        serde_json::json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "session_id": session.session_id,
            "work_dir": session.work_dir,
            "result": assistant_text.join("\n"),
        })
    });
    match open_args.output_format {
        PrintOutputFormat::Json => {
            writeln!(writer, "{}", serde_json::to_string_pretty(&result)?)?;
        }
        PrintOutputFormat::Text => {
            if let Some(text) = direct_connect_result_text(&result) {
                writeln!(writer, "{text}")?;
            } else {
                writeln!(writer, "{}", serde_json::to_string_pretty(&result)?)?;
            }
        }
        PrintOutputFormat::StreamJson => {}
    }
    writer.flush()?;
    Ok(())
}

#[cfg(unix)]
fn direct_connect_unix_socket_path(target: &DirectConnectTarget) -> Result<&Path> {
    target
        .unix_socket
        .as_deref()
        .ok_or_else(|| anyhow!("cc+unix direct-connect target is missing a socket path"))
}

fn direct_connect_control_response(event: &Value) -> Option<Value> {
    if event.get("type").and_then(Value::as_str) != Some("control_request") {
        return None;
    }
    let request_id = event
        .get("request_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let subtype = event
        .get("request")
        .and_then(|request| request.get("subtype"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if subtype == "can_use_tool" {
        Some(serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {
                    "behavior": "deny",
                    "message": "Kiana direct-connect headless mode does not grant tool permissions"
                }
            }
        }))
    } else {
        Some(serde_json::json!({
            "type": "control_response",
            "response": {
                "subtype": "error",
                "request_id": request_id,
                "error": format!("Unsupported control request subtype: {subtype}")
            }
        }))
    }
}

fn direct_connect_event_text(event: &Value) -> Option<String> {
    let content = event.get("message")?.get("content")?;
    match content {
        Value::String(text) => Some(text.trim().to_string()).filter(|text| !text.is_empty()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn direct_connect_result_text(event: &Value) -> Option<String> {
    event
        .get("result")
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
        .or_else(|| direct_connect_event_text(event))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DirectConnectServerArgs {
    host: String,
    port: u16,
    auth_token: Option<String>,
    unix_socket: Option<PathBuf>,
    workspace: Option<PathBuf>,
    idle_timeout_ms: u64,
    max_sessions: usize,
}

#[derive(Debug, Clone)]
struct DirectConnectServerSession {
    session_id: String,
    work_dir: PathBuf,
    dangerously_skip_permissions: bool,
}

#[derive(Debug, Clone)]
struct DirectConnectServerState {
    public_addr: SocketAddr,
    unix_socket: Option<PathBuf>,
    auth_token: Option<String>,
    workspace: PathBuf,
    idle_timeout_ms: u64,
    max_sessions: usize,
    sessions: Arc<AsyncMutex<HashMap<String, DirectConnectServerSession>>>,
    base_options: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectCreateSessionRequest {
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    dangerously_skip_permissions: bool,
}

async fn direct_connect_server_main(args: &[String]) -> Result<()> {
    if direct_connect_server_help_requested(args) {
        print_direct_connect_server_help();
        return Ok(());
    }
    let parsed = parse_direct_connect_server_args(args)?;
    if let Some(path) = parsed.unix_socket.clone() {
        return direct_connect_unix_server_main(parsed, path).await;
    }

    let bind_addr: SocketAddr = format!("{}:{}", parsed.host, parsed.port)
        .parse()
        .with_context(|| {
            format!(
                "invalid server bind address {}:{}",
                parsed.host, parsed.port
            )
        })?;
    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind direct-connect server at {bind_addr}"))?;
    let public_addr = listener.local_addr()?;
    let auth_token = parsed
        .auth_token
        .clone()
        .or_else(|| Some(format!("sk-kiana-cc-{}", uuid::Uuid::new_v4().simple())));
    let state = direct_connect_server_state(parsed, public_addr, auth_token, HashMap::new())?;

    eprintln!(
        "Kiana direct-connect server listening at {}",
        direct_connect_server_http_url(public_addr)
    );
    if let Some(token) = &state.auth_token {
        eprintln!(
            "Connect with: kiana open {} -p <prompt>",
            direct_connect_server_cc_url(public_addr, token)
        );
    }

    axum::serve(listener, direct_connect_server_router(state)).await?;
    Ok(())
}

#[cfg(unix)]
async fn direct_connect_unix_server_main(
    parsed: DirectConnectServerArgs,
    socket_path: PathBuf,
) -> Result<()> {
    prepare_direct_connect_unix_socket(&socket_path)?;
    let listener = tokio::net::UnixListener::bind(&socket_path).with_context(|| {
        format!(
            "failed to bind direct-connect Unix socket at {}",
            socket_path.display()
        )
    })?;
    let auth_token = parsed
        .auth_token
        .clone()
        .or_else(|| Some(format!("sk-kiana-cc-{}", uuid::Uuid::new_v4().simple())));
    let state = direct_connect_server_state(
        parsed,
        SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0)),
        auth_token,
        HashMap::new(),
    )?;

    eprintln!(
        "Kiana direct-connect server listening at unix:{}",
        socket_path.display()
    );
    if let Some(token) = &state.auth_token {
        eprintln!(
            "Connect with: kiana open {} -p <prompt>",
            direct_connect_server_cc_unix_url(&socket_path, token)
        );
    }

    axum::serve(listener, direct_connect_server_router(state)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn direct_connect_unix_server_main(
    _parsed: DirectConnectServerArgs,
    socket_path: PathBuf,
) -> Result<()> {
    Err(anyhow!(
        "kiana server --unix {} requires Unix socket support on this platform",
        socket_path.display()
    ))
}

#[cfg(unix)]
fn prepare_direct_connect_unix_socket(socket_path: &Path) -> Result<()> {
    use std::os::unix::fs::FileTypeExt;

    if let Some(parent) = socket_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create direct-connect socket directory {}",
                parent.display()
            )
        })?;
    }
    match std::fs::symlink_metadata(socket_path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            std::fs::remove_file(socket_path).with_context(|| {
                format!(
                    "failed to remove stale direct-connect socket {}",
                    socket_path.display()
                )
            })?;
        }
        Ok(_) => {
            return Err(anyhow!(
                "refusing to overwrite non-socket path for direct-connect Unix socket: {}",
                socket_path.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(anyhow!(
                "failed to inspect direct-connect Unix socket path {}: {}",
                socket_path.display(),
                error
            ));
        }
    }
    Ok(())
}

fn direct_connect_server_help_requested(args: &[String]) -> bool {
    is_help_at(args, 1)
}

fn parse_direct_connect_server_args(args: &[String]) -> Result<DirectConnectServerArgs> {
    let mut parsed = DirectConnectServerArgs {
        host: "0.0.0.0".to_string(),
        port: 0,
        auth_token: None,
        unix_socket: None,
        workspace: None,
        idle_timeout_ms: 600_000,
        max_sessions: 32,
    };
    let mut index = usize::from(args.first().map(String::as_str) == Some("server"));
    while let Some(arg) = args.get(index).map(String::as_str) {
        match arg {
            "--host" => {
                parsed.host = direct_connect_required_arg(args, &mut index, "--host")?;
            }
            _ if arg.starts_with("--host=") => {
                parsed.host = nonempty_flag_value("--host", arg.trim_start_matches("--host="))?;
                index += 1;
            }
            "--port" => {
                let value = direct_connect_required_arg(args, &mut index, "--port")?;
                parsed.port = parse_direct_connect_u16("--port", &value)?;
            }
            _ if arg.starts_with("--port=") => {
                parsed.port =
                    parse_direct_connect_u16("--port", arg.trim_start_matches("--port="))?;
                index += 1;
            }
            "--auth-token" => {
                parsed.auth_token = Some(direct_connect_required_arg(
                    args,
                    &mut index,
                    "--auth-token",
                )?);
            }
            _ if arg.starts_with("--auth-token=") => {
                parsed.auth_token = Some(nonempty_flag_value(
                    "--auth-token",
                    arg.trim_start_matches("--auth-token="),
                )?);
                index += 1;
            }
            "--unix" => {
                parsed.unix_socket = Some(PathBuf::from(direct_connect_required_arg(
                    args, &mut index, "--unix",
                )?));
            }
            _ if arg.starts_with("--unix=") => {
                parsed.unix_socket = Some(PathBuf::from(nonempty_flag_value(
                    "--unix",
                    arg.trim_start_matches("--unix="),
                )?));
                index += 1;
            }
            "--workspace" => {
                parsed.workspace = Some(PathBuf::from(direct_connect_required_arg(
                    args,
                    &mut index,
                    "--workspace",
                )?));
            }
            _ if arg.starts_with("--workspace=") => {
                parsed.workspace = Some(PathBuf::from(nonempty_flag_value(
                    "--workspace",
                    arg.trim_start_matches("--workspace="),
                )?));
                index += 1;
            }
            "--idle-timeout" => {
                let value = direct_connect_required_arg(args, &mut index, "--idle-timeout")?;
                parsed.idle_timeout_ms = parse_direct_connect_u64("--idle-timeout", &value)?;
            }
            _ if arg.starts_with("--idle-timeout=") => {
                parsed.idle_timeout_ms = parse_direct_connect_u64(
                    "--idle-timeout",
                    arg.trim_start_matches("--idle-timeout="),
                )?;
                index += 1;
            }
            "--max-sessions" => {
                let value = direct_connect_required_arg(args, &mut index, "--max-sessions")?;
                parsed.max_sessions = parse_direct_connect_usize("--max-sessions", &value)?;
            }
            _ if arg.starts_with("--max-sessions=") => {
                parsed.max_sessions = parse_direct_connect_usize(
                    "--max-sessions",
                    arg.trim_start_matches("--max-sessions="),
                )?;
                index += 1;
            }
            other if is_help_arg(other) => {
                return Err(anyhow!(direct_connect_server_usage()));
            }
            other => {
                return Err(anyhow!(
                    "unknown kiana server option '{}'\n\n{}",
                    other,
                    direct_connect_server_usage()
                ));
            }
        }
    }
    Ok(parsed)
}

fn direct_connect_required_arg(args: &[String], index: &mut usize, flag: &str) -> Result<String> {
    let value = args
        .get(*index + 1)
        .ok_or_else(|| anyhow!("{flag} requires a value"))?;
    if value.starts_with('-') {
        return Err(anyhow!("{flag} requires a value"));
    }
    *index += 2;
    nonempty_flag_value(flag, value)
}

fn parse_direct_connect_u16(flag: &str, value: &str) -> Result<u16> {
    let value = nonempty_flag_value(flag, value)?;
    value
        .parse::<u16>()
        .map_err(|_| anyhow!("{flag} requires a port between 0 and 65535"))
}

fn parse_direct_connect_u64(flag: &str, value: &str) -> Result<u64> {
    let value = nonempty_flag_value(flag, value)?;
    value
        .parse::<u64>()
        .map_err(|_| anyhow!("{flag} requires a non-negative integer"))
}

fn parse_direct_connect_usize(flag: &str, value: &str) -> Result<usize> {
    let value = nonempty_flag_value(flag, value)?;
    value
        .parse::<usize>()
        .map_err(|_| anyhow!("{flag} requires a non-negative integer"))
}

fn direct_connect_server_state(
    args: DirectConnectServerArgs,
    public_addr: SocketAddr,
    auth_token: Option<String>,
    base_options: HashMap<String, Value>,
) -> Result<DirectConnectServerState> {
    let workspace = args.workspace.unwrap_or(std::env::current_dir()?);
    Ok(DirectConnectServerState {
        public_addr,
        unix_socket: args.unix_socket,
        auth_token,
        workspace,
        idle_timeout_ms: args.idle_timeout_ms,
        max_sessions: args.max_sessions,
        sessions: Arc::new(AsyncMutex::new(HashMap::new())),
        base_options,
    })
}

fn direct_connect_server_router(state: DirectConnectServerState) -> axum::Router {
    axum::Router::new()
        .route(
            "/app",
            axum::routing::get(direct_connect_app_contract_handler),
        )
        .route(
            "/app/conversations",
            axum::routing::get(direct_connect_app_conversations_handler),
        )
        .route(
            "/app/conversations/{session_id}/events",
            axum::routing::get(direct_connect_app_conversation_events_handler),
        )
        .route(
            "/app/conversations/{session_id}/files",
            axum::routing::get(direct_connect_app_conversation_files_get_handler)
                .post(direct_connect_app_conversation_files_post_handler),
        )
        .route(
            "/app/settings",
            axum::routing::get(direct_connect_app_settings_handler),
        )
        .route(
            "/app/prompt-history",
            axum::routing::get(direct_connect_app_prompt_history_handler),
        )
        .route(
            "/app/team/status",
            axum::routing::get(direct_connect_app_team_status_handler),
        )
        .route(
            "/app/team/plan",
            axum::routing::get(direct_connect_app_team_plan_handler),
        )
        .route(
            "/app/commands",
            axum::routing::get(direct_connect_app_commands_handler),
        )
        .route(
            "/app/commands/run",
            axum::routing::post(direct_connect_app_command_run_handler),
        )
        .route(
            "/app/approvals/decision",
            axum::routing::post(direct_connect_app_approval_decision_handler),
        )
        .route(
            "/app/config/resolved",
            axum::routing::get(direct_connect_app_config_resolved_handler),
        )
        .route(
            "/app/doctor",
            axum::routing::get(direct_connect_app_doctor_handler),
        )
        .route(
            "/app/release/blockers",
            axum::routing::get(direct_connect_app_release_blockers_handler),
        )
        .route(
            "/app/release/local-rc-evidence",
            axum::routing::get(direct_connect_app_release_local_rc_evidence_handler),
        )
        .route(
            "/app/release/product-acceptance",
            axum::routing::get(direct_connect_app_release_product_acceptance_handler),
        )
        .route(
            "/app/release/entitlement",
            axum::routing::get(direct_connect_app_release_entitlement_handler),
        )
        .route(
            "/app/release/ops",
            axum::routing::get(direct_connect_app_release_ops_handler),
        )
        .route(
            "/app/release/platform-security",
            axum::routing::get(direct_connect_app_release_platform_security_handler),
        )
        .route(
            "/app/release/source-control",
            axum::routing::get(direct_connect_app_release_source_control_handler),
        )
        .route(
            "/app/release/signature",
            axum::routing::get(direct_connect_app_release_signature_handler),
        )
        .route(
            "/app/release/enterprise-offline-manifest",
            axum::routing::get(direct_connect_app_release_enterprise_offline_manifest_handler),
        )
        .route(
            "/app/release/proof-manifest",
            axum::routing::get(direct_connect_app_release_proof_manifest_handler),
        )
        .route(
            "/app/release/live-provider-smoke",
            axum::routing::get(direct_connect_app_release_live_provider_smoke_handler),
        )
        .route(
            "/app/release/remote-code-session-smoke",
            axum::routing::get(direct_connect_app_release_remote_code_session_smoke_handler),
        )
        .route(
            "/app/release/distribution",
            axum::routing::get(direct_connect_app_release_distribution_handler),
        )
        .route(
            "/app/secrets",
            axum::routing::get(direct_connect_app_secrets_handler),
        )
        .route(
            "/app/sandbox",
            axum::routing::get(direct_connect_app_sandbox_handler),
        )
        .route(
            "/app/permissions/status",
            axum::routing::get(direct_connect_app_permissions_status_handler),
        )
        .route(
            "/app/trust/status",
            axum::routing::get(direct_connect_app_trust_status_handler),
        )
        .route(
            "/app/plugins",
            axum::routing::get(direct_connect_app_plugins_handler),
        )
        .route(
            "/app/auth/status",
            axum::routing::get(direct_connect_app_auth_status_handler),
        )
        .route(
            "/app/license/status",
            axum::routing::get(direct_connect_app_license_status_handler),
        )
        .route(
            "/app/models/catalog",
            axum::routing::get(direct_connect_app_model_catalog_handler),
        )
        .route(
            "/app/models/list",
            axum::routing::get(direct_connect_app_model_list_handler),
        )
        .route(
            "/app/models/current",
            axum::routing::get(direct_connect_app_model_current_handler)
                .post(direct_connect_app_model_current_post_handler),
        )
        .route(
            "/app/models/smoke",
            axum::routing::get(direct_connect_app_model_smoke_handler),
        )
        .route(
            "/app/git/status",
            axum::routing::get(direct_connect_app_git_status_handler),
        )
        .route(
            "/app/diff",
            axum::routing::get(direct_connect_app_diff_handler),
        )
        .route(
            "/app/checkpoints",
            axum::routing::post(direct_connect_app_checkpoint_create_handler),
        )
        .route(
            "/app/checks/dry-run",
            axum::routing::get(direct_connect_app_checks_dry_run_handler),
        )
        .route(
            "/app/checks",
            axum::routing::get(direct_connect_app_checks_run_handler),
        )
        .route(
            "/app/review/dry-run",
            axum::routing::get(direct_connect_app_review_dry_run_handler),
        )
        .route(
            "/app/review",
            axum::routing::get(direct_connect_app_review_run_handler),
        )
        .route(
            "/app/context/index",
            axum::routing::get(direct_connect_app_context_index_handler),
        )
        .route(
            "/app/context/artifacts",
            axum::routing::get(direct_connect_app_context_artifacts_handler),
        )
        .route(
            "/app/context/ingest",
            axum::routing::post(direct_connect_app_context_ingest_handler),
        )
        .route(
            "/app/context/artifact-graph",
            axum::routing::get(direct_connect_app_context_artifact_graph_handler),
        )
        .route(
            "/app/context/artifact-store",
            axum::routing::get(direct_connect_app_context_artifact_store_handler),
        )
        .route(
            "/app/context/artifact-readiness",
            axum::routing::get(direct_connect_app_context_artifact_readiness_handler),
        )
        .route(
            "/app/context/repo-map",
            axum::routing::get(direct_connect_app_context_repo_map_handler),
        )
        .route(
            "/app/context/search",
            axum::routing::get(direct_connect_app_context_search_handler),
        )
        .route(
            "/app/context/vector-search",
            axum::routing::get(direct_connect_app_context_vector_search_handler),
        )
        .route(
            "/app/context/pack",
            axum::routing::get(direct_connect_app_context_pack_handler),
        )
        .route(
            "/sessions",
            axum::routing::post(direct_connect_create_session_handler),
        )
        .route(
            "/sessions/{session_id}/ws",
            axum::routing::get(direct_connect_ws_handler),
        )
        .with_state(state)
}

async fn direct_connect_app_contract_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let active_sessions = state.sessions.lock().await.len();
    axum::Json(direct_connect_app_contract(&state, active_sessions)).into_response()
}

async fn direct_connect_app_conversations_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let active_sessions = state.sessions.lock().await;
    let active_session_ids = active_sessions
        .values()
        .map(|session| session.session_id.clone())
        .collect::<HashSet<_>>();
    let mut conversations = active_sessions
        .values()
        .map(direct_connect_active_conversation_json)
        .collect::<Vec<_>>();
    drop(active_sessions);

    let persisted_sessions = match crate::sdk::list_sessions().await {
        Ok(sessions) => sessions,
        Err(error) => {
            return direct_connect_json_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load SDK sessions: {error}"),
            )
        }
    };
    conversations.extend(
        persisted_sessions
            .into_iter()
            .filter(|session| !active_session_ids.contains(&session.session_id))
            .map(direct_connect_persisted_conversation_json),
    );
    conversations.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(right.get("id").and_then(Value::as_str).unwrap_or_default())
    });

    axum::Json(serde_json::json!({
        "schema": "kiana.app-server.conversations.v1",
        "count": conversations.len(),
        "conversations": conversations,
    }))
    .into_response()
}

fn direct_connect_active_conversation_json(session: &DirectConnectServerSession) -> Value {
    serde_json::json!({
        "id": session.session_id,
        "session_id": session.session_id,
        "source": "direct-connect",
        "active": true,
        "work_dir": session.work_dir.display().to_string(),
        "dangerously_skip_permissions": session.dangerously_skip_permissions,
        "events_url": format!("/sessions/{}/ws", session.session_id),
        "events_snapshot_url": format!("/app/conversations/{}/events", session.session_id),
    })
}

fn direct_connect_persisted_conversation_json(session: crate::sdk::SdkSessionInfo) -> Value {
    serde_json::json!({
        "id": session.session_id,
        "session_id": session.session_id,
        "source": "sdk-session-store",
        "active": false,
        "title": session.title,
        "tag": session.tag,
        "parent_session_id": session.parent_session_id,
        "work_dir": session.cwd,
        "created_at": session.created_at,
        "updated_at": session.updated_at,
        "message_count": session.message_count,
        "assistant_message_count": session.assistant_message_count,
        "last_role": session.last_role,
        "events_url": Value::Null,
        "events_snapshot_url": format!("/app/conversations/{}/events", session.session_id),
    })
}

#[derive(Debug, Deserialize)]
struct DirectConnectConversationFilesRequest {
    #[serde(default)]
    editable_files: Option<Vec<String>>,
    #[serde(
        default,
        alias = "readOnlyFiles",
        alias = "readonly_files",
        alias = "readonlyFiles"
    )]
    read_only_files: Option<Vec<String>>,
    #[serde(default)]
    clear_editable: bool,
    #[serde(default, alias = "clearReadonly")]
    clear_read_only: bool,
}

async fn direct_connect_app_conversation_files_get_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    direct_connect_app_conversation_files_handler(state, session_id, headers, None).await
}

async fn direct_connect_app_conversation_files_post_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DirectConnectConversationFilesRequest>,
) -> axum::response::Response {
    direct_connect_app_conversation_files_handler(state, session_id, headers, Some(body)).await
}

async fn direct_connect_app_conversation_files_handler(
    state: DirectConnectServerState,
    session_id: String,
    headers: axum::http::HeaderMap,
    body: Option<DirectConnectConversationFilesRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }
    if let Err(error) = direct_connect_validate_session_id(&session_id) {
        return direct_connect_json_error(axum::http::StatusCode::BAD_REQUEST, error.to_string());
    }

    match direct_connect_conversation_files_report(&session_id, body) {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) if error.to_string().contains("was not found") => direct_connect_json_error(
            axum::http::StatusCode::NOT_FOUND,
            format!("direct-connect conversation files not found: {error}"),
        ),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to update conversation files: {error}"),
        ),
    }
}

fn direct_connect_conversation_files_report(
    session_id: &str,
    request: Option<DirectConnectConversationFilesRequest>,
) -> Result<Value> {
    let mut session = direct_connect_read_session_json(session_id)?;
    let mut changed = false;

    if let Some(request) = request {
        if request.clear_editable {
            direct_connect_remove_object_key(&mut session, "editable_files");
            changed = true;
        }
        if request.clear_read_only {
            direct_connect_remove_object_key(&mut session, "read_only_files");
            changed = true;
        }
        if let Some(editable_files) = request.editable_files {
            direct_connect_validate_file_set("editable_files", &editable_files)?;
            session["editable_files"] = Value::Array(
                editable_files
                    .into_iter()
                    .map(Value::String)
                    .collect::<Vec<_>>(),
            );
            changed = true;
        }
        if let Some(read_only_files) = request.read_only_files {
            direct_connect_validate_file_set("read_only_files", &read_only_files)?;
            session["read_only_files"] = Value::Array(
                read_only_files
                    .into_iter()
                    .map(Value::String)
                    .collect::<Vec<_>>(),
            );
            changed = true;
        }
        if changed {
            session["updated_at"] = serde_json::json!(direct_connect_now_unix_seconds());
            direct_connect_write_session_json(session_id, &session)?;
        }
    }

    Ok(serde_json::json!({
        "schema": "kiana.app-server.conversation-files.v1",
        "session_id": session_id,
        "changed": changed,
        "editable_files": direct_connect_session_file_list(&session, "editable_files"),
        "read_only_files": direct_connect_session_file_list(&session, "read_only_files"),
    }))
}

fn direct_connect_read_session_json(session_id: &str) -> Result<Value> {
    direct_connect_validate_session_id(session_id)?;
    let path = direct_connect_sdk_sessions_dir().join(format!("{session_id}.json"));
    if path.exists() {
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("session '{}' was not found", session_id))?;
        return serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse session file {}", path.display()));
    }

    let events = direct_connect_read_runtime_event_values(session_id)?
        .ok_or_else(|| anyhow!("session '{}' was not found", session_id))?;
    let messages = events
        .iter()
        .filter_map(direct_connect_runtime_event_message)
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "session_id": session_id,
        "title": serde_json::Value::Null,
        "tag": serde_json::Value::Null,
        "parent_session_id": serde_json::Value::Null,
        "cwd": serde_json::Value::Null,
        "created_at": 0,
        "updated_at": 0,
        "messages": messages,
    }))
}

fn direct_connect_write_session_json(session_id: &str, session: &Value) -> Result<()> {
    direct_connect_validate_session_id(session_id)?;
    let dir = direct_connect_sdk_sessions_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let path = dir.join(format!("{session_id}.json"));
    let tmp = dir.join(format!(".{session_id}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(session)?)
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn direct_connect_runtime_event_message(event: &Value) -> Option<Value> {
    match event.get("type").and_then(Value::as_str)? {
        "user_message" | "assistant_message" => event.get("message").cloned(),
        _ => None,
    }
}

fn direct_connect_validate_file_set(label: &str, files: &[String]) -> Result<()> {
    for file in files {
        if file.trim().is_empty() {
            return Err(anyhow!("{label} contains an empty file path"));
        }
    }
    Ok(())
}

fn direct_connect_session_file_list(session: &Value, key: &str) -> Vec<String> {
    session
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn direct_connect_remove_object_key(value: &mut Value, key: &str) {
    if let Some(object) = value.as_object_mut() {
        object.remove(key);
    }
}

fn direct_connect_now_unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn direct_connect_app_conversation_events_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }
    if let Err(error) = direct_connect_validate_session_id(&session_id) {
        return direct_connect_json_error(axum::http::StatusCode::BAD_REQUEST, error.to_string());
    }

    let active_session = {
        let sessions = state.sessions.lock().await;
        sessions.get(&session_id).cloned()
    };
    let report =
        match direct_connect_app_conversation_events_report(&session_id, active_session.as_ref()) {
            Ok(report) => report,
            Err(error) => {
                return direct_connect_json_error(
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to load session events: {error}"),
                )
            }
        };
    if active_session.is_none()
        && !report
            .get("event_source")
            .and_then(|source| source.get("available"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return direct_connect_json_error(
            axum::http::StatusCode::NOT_FOUND,
            "direct-connect conversation events not found",
        );
    }

    axum::Json(report).into_response()
}

async fn direct_connect_app_settings_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let mut option_keys = state.base_options.keys().cloned().collect::<Vec<_>>();
    option_keys.sort();
    let mut settings_app_state = state.base_options.clone();
    settings_app_state.insert(
        "cwd".to_string(),
        Value::String(state.workspace.display().to_string()),
    );
    let registry = create_default_command_registry();
    let sections = crate::tui::load_settings_sections(
        registry.get("auth").cloned(),
        registry.get("model").cloned(),
        registry.get("permissions").cloned(),
        registry.get("mcp").cloned(),
        registry.get("doctor").cloned(),
        settings_app_state,
    )
    .await
    .unwrap_or_else(|error| {
        vec![SettingsSection::new(
            "Readiness",
            vec![
                kiana_screens::settings::SettingsRow::new("status", "error"),
                kiana_screens::settings::SettingsRow::new("error", error),
            ],
        )]
    });
    axum::Json(serde_json::json!({
        "schema": "kiana.app-server.settings.v1",
        "workspace": state.workspace.display().to_string(),
        "idle_timeout_ms": state.idle_timeout_ms,
        "max_sessions": state.max_sessions,
        "auth": {
            "type": if state.auth_token.is_some() { "bearer" } else { "none" },
            "required": state.auth_token.is_some(),
        },
        "base_option_keys": option_keys,
        "sections": app_settings_sections_json(sections),
    }))
    .into_response()
}

async fn direct_connect_app_prompt_history_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let history_path = crate::tui::prompt_history_path();
    match crate::tui::load_prompt_history_entries_from_path(&history_path) {
        Ok(entries) => axum::Json(direct_connect_app_prompt_history_payload(
            &state,
            &history_path,
            entries,
        ))
        .into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            error.to_string(),
        ),
    }
}

fn direct_connect_app_prompt_history_payload(
    state: &DirectConnectServerState,
    history_path: &Path,
    entries: Vec<HistoryEntry>,
) -> Value {
    let entries_json = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "prompt": &entry.prompt,
                "timestamp": &entry.timestamp,
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema": "kiana.app-server.prompt-history.v1",
        "workspace": state.workspace.display().to_string(),
        "storage": {
            "kind": "kiana_home",
            "file": history_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(crate::tui::TUI_PROMPT_HISTORY_FILE),
        },
        "limit": crate::tui::TUI_PROMPT_HISTORY_LIMIT,
        "count": entries_json.len(),
        "entries": entries_json,
    })
}

async fn direct_connect_app_team_status_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_app_team_status_payload(&state) {
        Ok(payload) => axum::Json(payload).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build team status report: {error}"),
        ),
    }
}

async fn direct_connect_app_team_plan_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_app_team_plan_payload(&state) {
        Ok(payload) => axum::Json(payload).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build team plan report: {error}"),
        ),
    }
}

fn direct_connect_app_team_status_payload(state: &DirectConnectServerState) -> Result<Value> {
    let app_state = direct_connect_app_state_with_workspace(state);
    let context = CommandContext {
        args: String::new(),
        app_state: app_state.clone(),
    };
    let tasks = kiana_commands::tasks::tasks_report(&context, None)?;
    let task_list_id = tasks["task_list_id"].as_str().unwrap_or("default");
    Ok(serde_json::json!({
        "schema": "kiana.app-server.team-status.v1",
        "workspace": state.workspace.display().to_string(),
        "team": direct_connect_app_team_metadata(&app_state, task_list_id),
        "task_list": {
            "id": tasks["task_list_id"].clone(),
            "tasks_dir": tasks["tasks_dir"].clone(),
            "count": tasks["count"].clone(),
            "status_counts": tasks["status_counts"].clone(),
        },
        "tasks": tasks,
    }))
}

fn direct_connect_app_team_plan_payload(state: &DirectConnectServerState) -> Result<Value> {
    let app_state = direct_connect_app_state_with_workspace(state);
    let context = CommandContext {
        args: String::new(),
        app_state,
    };
    kiana_commands::tasks::team_plan_report(&context, None)
}

fn direct_connect_app_team_metadata(
    app_state: &HashMap<String, Value>,
    task_list_id: &str,
) -> Value {
    let team_name = app_state
        .get("team_context")
        .or_else(|| app_state.get("teamContext"))
        .and_then(Value::as_object)
        .and_then(|team| {
            team.get("team_name")
                .or_else(|| team.get("teamName"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let source = if team_name.is_some() {
        "team_context"
    } else if app_state.contains_key("task_list_id") || app_state.contains_key("taskListId") {
        "task_list_id"
    } else if app_state.contains_key("session_id") || app_state.contains_key("sessionId") {
        "session_id"
    } else {
        "default_task_list"
    };
    serde_json::json!({
        "name": team_name.unwrap_or(task_list_id),
        "source": source,
    })
}

async fn direct_connect_app_commands_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(direct_connect_app_commands_payload(&state)).into_response()
}

fn direct_connect_app_commands_payload(state: &DirectConnectServerState) -> Value {
    let registry = create_default_command_registry();
    let mut commands = registry
        .list()
        .into_iter()
        .map(|command| {
            let name = command.name();
            let (source_kind, plugin_name) = direct_connect_app_command_source(name);
            serde_json::json!({
                "name": name,
                "slash": format!("/{name}"),
                "description": command.description(),
                "command_type": direct_connect_app_command_type_name(command.command_type()),
                "enabled": command.is_enabled(),
                "supports_non_interactive": command.supports_non_interactive(),
                "routes_to": direct_connect_app_command_route(command.command_type()),
                "source": {
                    "kind": source_kind,
                    "plugin": plugin_name,
                },
            })
        })
        .collect::<Vec<_>>();
    commands.sort_by(|left, right| {
        left["name"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["name"].as_str().unwrap_or_default())
    });
    serde_json::json!({
        "schema": "kiana.app-server.commands.v1",
        "workspace": state.workspace.display().to_string(),
        "count": commands.len(),
        "commands": commands,
    })
}

fn direct_connect_app_command_type_name(command_type: CommandType) -> &'static str {
    match command_type {
        CommandType::Local => "local",
        CommandType::LocalJsx => "local_jsx",
        CommandType::Prompt => "prompt",
    }
}

fn direct_connect_app_command_route(command_type: CommandType) -> &'static str {
    match command_type {
        CommandType::Local | CommandType::LocalJsx => "local_command",
        CommandType::Prompt => "assistant_prompt",
    }
}

fn direct_connect_app_command_source(name: &str) -> (&'static str, Value) {
    if let Some((plugin, _)) = name.split_once(':') {
        ("plugin", Value::String(plugin.to_string()))
    } else {
        ("core", Value::Null)
    }
}

const DIRECT_CONNECT_COMMAND_RUN_MAX_ARGS: usize = 64;
const DIRECT_CONNECT_COMMAND_RUN_MAX_ARG_LEN: usize = 2048;

#[derive(Debug, Deserialize)]
struct DirectConnectCommandRunRequest {
    #[serde(default, alias = "command")]
    name: Option<String>,
    #[serde(default, alias = "argv")]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectApprovalDecisionRequest {
    approval_id: ApprovalId,
    decision: ApprovalDecision,
}

async fn direct_connect_app_command_run_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DirectConnectCommandRunRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_app_command_run_payload(&state, body).await {
        Ok(payload) => axum::Json(payload).into_response(),
        Err((status, message)) => direct_connect_json_error(status, message),
    }
}

async fn direct_connect_app_command_run_payload(
    state: &DirectConnectServerState,
    request: DirectConnectCommandRunRequest,
) -> std::result::Result<Value, (axum::http::StatusCode, String)> {
    let name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                "command name is required".to_string(),
            )
        })?;
    if request.args.len() > DIRECT_CONNECT_COMMAND_RUN_MAX_ARGS {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            format!("commands/run accepts at most {DIRECT_CONNECT_COMMAND_RUN_MAX_ARGS} arguments"),
        ));
    }
    if let Some((index, _)) = request
        .args
        .iter()
        .enumerate()
        .find(|(_, value)| value.len() > DIRECT_CONNECT_COMMAND_RUN_MAX_ARG_LEN)
    {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            format!("argument {index} exceeds {DIRECT_CONNECT_COMMAND_RUN_MAX_ARG_LEN} bytes"),
        ));
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get(&name) else {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("command '{name}' is not registered"),
        ));
    };
    if command.is_hidden() {
        return Err((
            axum::http::StatusCode::NOT_FOUND,
            format!("command '{name}' is not registered"),
        ));
    }
    if !command.is_enabled() {
        return Err((
            axum::http::StatusCode::CONFLICT,
            format!("command '{name}' is disabled"),
        ));
    }
    if !command.supports_non_interactive() {
        return Err((
            axum::http::StatusCode::BAD_REQUEST,
            format!("command '{name}' does not support non-interactive app-server execution"),
        ));
    }

    let command_type = command.command_type();
    let command_args = request.args.join(" ");
    let command_context = CommandContext {
        args: command_args,
        app_state: HashMap::from([
            (
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            ),
            (
                "session_id".to_string(),
                Value::String("app-server-command".to_owned()),
            ),
            (
                "actor_id".to_string(),
                Value::String("app-server".to_owned()),
            ),
            (
                COMMAND_ARGV_APP_STATE_KEY.to_string(),
                Value::Array(request.args.iter().cloned().map(Value::String).collect()),
            ),
        ]),
    };
    let outcome = crate::command_dispatch::dispatch_command(command.as_ref(), command_context)
        .await
        .map_err(|error| {
            (
                axum::http::StatusCode::BAD_REQUEST,
                format!("failed to run command '{name}': {error}"),
            )
        })?;
    let (status, executed, output) = match outcome {
        crate::command_dispatch::CommandDispatchOutcome::Completed(result) => (
            "ok",
            !matches!(command_type, CommandType::Prompt),
            serde_json::to_value(result).map_err(|error| {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    format!("failed to serialize command '{name}' result: {error}"),
                )
            })?,
        ),
        crate::command_dispatch::CommandDispatchOutcome::AwaitingApproval(challenge) => (
            "awaiting_approval",
            false,
            serde_json::json!({ "approval": challenge }),
        ),
    };
    let (source_kind, plugin_name) = direct_connect_app_command_source(&name);
    let arg_count = request.args.len();
    let args = request.args;
    Ok(serde_json::json!({
        "schema": "kiana.app-server.command-run.v1",
        "workspace": state.workspace.display().to_string(),
        "command": {
            "name": name.clone(),
            "slash": format!("/{name}"),
            "description": command.description(),
            "command_type": direct_connect_app_command_type_name(command_type.clone()),
            "enabled": command.is_enabled(),
            "supports_non_interactive": command.supports_non_interactive(),
            "routes_to": direct_connect_app_command_route(command_type.clone()),
            "source": {
                "kind": source_kind,
                "plugin": plugin_name,
            },
        },
        "request": {
            "args": args,
            "arg_count": arg_count,
        },
        "status": status,
        "executed": executed,
        "output": output,
    }))
}

async fn direct_connect_app_approval_decision_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DirectConnectApprovalDecisionRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let context = CommandContext {
        args: String::new(),
        app_state: HashMap::from([
            (
                "cwd".to_owned(),
                Value::String(state.workspace.display().to_string()),
            ),
            (
                "session_id".to_owned(),
                Value::String("app-server-command".to_owned()),
            ),
            (
                "actor_id".to_owned(),
                Value::String("app-server".to_owned()),
            ),
        ]),
    };
    match crate::command_dispatch::resolve_command_approval_response(
        &context,
        body.approval_id,
        body.decision,
    )
    .await
    {
        Ok(response) => axum::Json(serde_json::json!({
            "schema": "kiana.app-server.approval-decision.v1",
            "status": direct_connect_approval_status(response.status),
            "request_id": response.request_id,
            "output": response.output,
            "error": response.error,
        }))
        .into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to resolve approval: {error}"),
        ),
    }
}

fn direct_connect_approval_status(status: ControlPlaneStatus) -> &'static str {
    match status {
        ControlPlaneStatus::Accepted => "accepted",
        ControlPlaneStatus::Denied => "denied",
        ControlPlaneStatus::AwaitingApproval => "awaiting_approval",
        ControlPlaneStatus::Running => "running",
        ControlPlaneStatus::Completed => "completed",
        ControlPlaneStatus::Failed => "failed",
        ControlPlaneStatus::ResultUnknown => "result_unknown",
        ControlPlaneStatus::Blocked => "blocked",
    }
}

fn app_settings_sections_json(sections: Vec<SettingsSection>) -> Value {
    Value::Array(
        sections
            .into_iter()
            .map(|section| {
                serde_json::json!({
                    "title": section.title,
                    "rows": section
                        .rows
                        .into_iter()
                        .map(|row| serde_json::json!({
                            "label": row.label,
                            "value": row.value,
                        }))
                        .collect::<Vec<_>>(),
                })
            })
            .collect(),
    )
}

async fn direct_connect_app_config_resolved_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("config") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "config command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "resolved --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(mut config) => {
            apply_app_state_resolved_config_overrides(&mut config, &state.base_options);
            axum::Json(serde_json::json!({
                "schema": "kiana.app-server.config-resolved.v1",
                "workspace": state.workspace.display().to_string(),
                "config": config
            }))
            .into_response()
        }
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build resolved config report: {error}"),
        ),
    }
}

fn apply_app_state_resolved_config_overrides(
    config: &mut Value,
    base_options: &HashMap<String, Value>,
) {
    let Some(values) = config.get_mut("values").and_then(Value::as_object_mut) else {
        return;
    };

    if direct_connect_nonempty_base_option(base_options, "api_key").is_some() {
        values.insert(
            "api_key".to_string(),
            serde_json::json!({
                "value": "redacted",
                "source": "app_state"
            }),
        );
    }
    if let Some(value) = direct_connect_nonempty_base_option(base_options, "base_url") {
        values.insert(
            "base_url".to_string(),
            serde_json::json!({
                "value": value,
                "source": "app_state"
            }),
        );
    }
    if let Some(value) = direct_connect_nonempty_base_option(base_options, "model") {
        values.insert(
            "model".to_string(),
            serde_json::json!({
                "value": value,
                "source": "app_state"
            }),
        );
    }
}

fn direct_connect_nonempty_base_option<'a>(
    base_options: &'a HashMap<String, Value>,
    key: &str,
) -> Option<&'a str> {
    base_options
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn direct_connect_app_doctor_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("doctor") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "doctor command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "--json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(doctor) => axum::Json(serde_json::json!({
            "schema": "kiana.app-server.doctor.v1",
            "workspace": state.workspace.display().to_string(),
            "doctor": doctor,
        }))
        .into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build doctor readiness report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_blockers_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_commercial_release_blockers_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build commercial release blockers report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_local_rc_evidence_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_local_rc_evidence_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read local RC evidence report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_product_acceptance_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_product_acceptance_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read product acceptance report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_entitlement_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_entitlement_proof_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read entitlement proof report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_ops_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_release_ops_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read release ops report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_platform_security_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_platform_security_proof_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read platform security proof report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_source_control_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_source_control_proof_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read source control proof report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_signature_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_release_signature_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read release signature proof report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_enterprise_offline_manifest_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_enterprise_offline_manifest_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read enterprise offline manifest report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_proof_manifest_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_commercial_proof_manifest_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read commercial proof manifest report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_live_provider_smoke_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_live_provider_smoke_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read live provider smoke report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_remote_code_session_smoke_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_remote_code_session_smoke_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to read remote code-session smoke report: {error}"),
        ),
    }
}

async fn direct_connect_app_release_distribution_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_distribution_review_report() {
        Ok(report) => axum::Json(report).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build distribution review report: {error}"),
        ),
    }
}

fn direct_connect_commercial_release_blockers_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let mut failures = Vec::new();
    for bash in direct_connect_bash_candidates() {
        match std::process::Command::new(&bash)
            .arg("./scripts/commercial-release-blockers-report.sh")
            .arg("--json")
            .current_dir(root)
            .output()
        {
            Ok(output) if output.status.success() => {
                return serde_json::from_slice::<Value>(&output.stdout).with_context(|| {
                    format!(
                        "failed to parse commercial release blockers JSON report from {}",
                        bash.display()
                    )
                });
            }
            Ok(output) => failures.push(format!(
                "{} exited with status {}: {}",
                bash.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => failures.push(format!("{} failed to start: {error}", bash.display())),
        }
    }
    Err(anyhow!(
        "failed to run scripts/commercial-release-blockers-report.sh --json: {}",
        failures.join(" | ")
    ))
}

fn direct_connect_bash_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("KIANA_BASH") {
        candidates.push(PathBuf::from(path));
    }
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"));
        candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
        ));
    }
    candidates.push(PathBuf::from("bash"));
    candidates
}

fn direct_connect_local_rc_evidence_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let evidence_path = std::env::var_os("KIANA_LOCAL_RC_EVIDENCE_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let dist_dir = std::env::var_os("DIST_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("dist"));
            dist_dir.join("proofs").join("local-rc-evidence.json")
        });
    let evidence_path = if evidence_path.is_absolute() {
        evidence_path
    } else {
        root.join(evidence_path)
    };
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read local RC evidence file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse local RC evidence JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.local-rc-evidence.v1") {
        return Err(anyhow!(
            "local RC evidence file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_product_acceptance_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let candidates = [
        std::env::var_os("KIANA_PRODUCT_ACCEPTANCE_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_PRODUCT_ACCEPTANCE_OUT").map(PathBuf::from),
        Some(
            dist_dir
                .join("proofs")
                .join("product")
                .join("product-acceptance.json"),
        ),
        Some(
            root.join("target")
                .join("product-acceptance")
                .join("product-acceptance.json"),
        ),
        Some(
            root.join("docs")
                .join("product-acceptance")
                .join(format!("{version}.json")),
        ),
    ];
    let evidence_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no product acceptance proof found in KIANA_PRODUCT_ACCEPTANCE_FILE, KIANA_PRODUCT_ACCEPTANCE_OUT, dist/proofs/product/product-acceptance.json, target/product-acceptance/product-acceptance.json, or docs/product-acceptance/{version}.json"
            )
        })?;
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read product acceptance file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse product acceptance JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.product-acceptance.v1") {
        return Err(anyhow!(
            "product acceptance file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_entitlement_proof_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let candidates = [
        std::env::var_os("KIANA_ENTITLEMENT_PROOF_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_ENTITLEMENT_PROOF_OUT").map(PathBuf::from),
        Some(
            dist_dir
                .join("proofs")
                .join("entitlement")
                .join("entitlement-proof.json"),
        ),
        Some(
            root.join("target")
                .join("entitlement-proof")
                .join("entitlement-proof.json"),
        ),
        Some(
            root.join("docs")
                .join("entitlements")
                .join(format!("{version}.json")),
        ),
    ];
    let evidence_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no entitlement proof found in KIANA_ENTITLEMENT_PROOF_FILE, KIANA_ENTITLEMENT_PROOF_OUT, dist/proofs/entitlement/entitlement-proof.json, target/entitlement-proof/entitlement-proof.json, or docs/entitlements/{version}.json"
            )
        })?;
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read entitlement proof file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse entitlement proof JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.entitlement-proof.v1") {
        return Err(anyhow!(
            "entitlement proof file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_release_ops_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let candidates = [
        std::env::var_os("KIANA_RELEASE_OPS_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_RELEASE_OPS_OUT").map(PathBuf::from),
        Some(
            dist_dir
                .join("proofs")
                .join("release-ops")
                .join("release-ops.json"),
        ),
        Some(
            root.join("target")
                .join("release-ops")
                .join("release-ops.json"),
        ),
        Some(
            root.join("docs")
                .join("release-ops")
                .join(format!("{version}.json")),
        ),
    ];
    let evidence_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no release ops proof found in KIANA_RELEASE_OPS_FILE, KIANA_RELEASE_OPS_OUT, dist/proofs/release-ops/release-ops.json, target/release-ops/release-ops.json, or docs/release-ops/{version}.json"
            )
        })?;
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read release ops file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse release ops JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.release-ops.v1") {
        return Err(anyhow!(
            "release ops file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_platform_security_proof_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    };
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let proof_dir = std::env::var_os("KIANA_PLATFORM_SECURITY_PROOF_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("docs").join("platform-security"));
    let candidates = [
        std::env::var_os("KIANA_PLATFORM_SECURITY_PROOF_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_PLATFORM_SECURITY_PROOF_OUT").map(PathBuf::from),
        Some(
            dist_dir
                .join("proofs")
                .join("platform-security")
                .join(format!("platform-security-{platform}.json")),
        ),
        Some(
            root.join("target")
                .join("platform-security")
                .join(format!("platform-security-{platform}.json")),
        ),
        Some(proof_dir.join(format!("{version}-{platform}.json"))),
    ];
    let evidence_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no platform security proof found in KIANA_PLATFORM_SECURITY_PROOF_FILE, KIANA_PLATFORM_SECURITY_PROOF_OUT, dist/proofs/platform-security/platform-security-{platform}.json, target/platform-security/platform-security-{platform}.json, or docs/platform-security/{version}-{platform}.json"
            )
        })?;
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read platform security proof file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse platform security proof JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.platform-security-proof.v1") {
        return Err(anyhow!(
            "platform security proof file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_source_control_proof_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let candidates = [
        std::env::var_os("KIANA_SOURCE_CONTROL_PROOF_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_SOURCE_CONTROL_PROOF_OUT").map(PathBuf::from),
        Some(
            dist_dir
                .join("proofs")
                .join("source-control")
                .join("source-control.json"),
        ),
        Some(
            root.join("docs")
                .join("source-control")
                .join(format!("{version}.json")),
        ),
    ];
    let evidence_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no source control proof found in KIANA_SOURCE_CONTROL_PROOF_FILE, KIANA_SOURCE_CONTROL_PROOF_OUT, dist/proofs/source-control/source-control.json, or docs/source-control/{version}.json"
            )
        })?;
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read source control proof file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse source control proof JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.source-control-proof.v1") {
        return Err(anyhow!(
            "source control proof file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_release_signature_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let dist_dir = if dist_dir.is_absolute() {
        dist_dir
    } else {
        root.join(dist_dir)
    };
    let explicit_candidates = [
        std::env::var_os("KIANA_RELEASE_SIGNATURE_PROOF_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_RELEASE_SIGNATURE_PROOF_OUT").map(PathBuf::from),
    ];
    let evidence_path = explicit_candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .or_else(|| {
            let mut packaged = std::fs::read_dir(&dist_dir)
                .ok()?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            name.starts_with(&format!("kiana-{version}-"))
                                && name.ends_with(".signature.json")
                        })
                })
                .collect::<Vec<_>>();
            packaged.sort();
            packaged.into_iter().next()
        })
        .ok_or_else(|| {
            anyhow!(
                "no release signature proof found in KIANA_RELEASE_SIGNATURE_PROOF_FILE, KIANA_RELEASE_SIGNATURE_PROOF_OUT, or DIST_DIR/kiana-{version}-*.signature.json"
            )
        })?;
    let report = std::fs::read_to_string(&evidence_path).with_context(|| {
        format!(
            "failed to read release signature proof file {}",
            evidence_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse release signature proof JSON {}",
            evidence_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.release-signature.v1") {
        return Err(anyhow!(
            "release signature proof file {} has unexpected schema",
            evidence_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_enterprise_offline_manifest_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let dist_dir = if dist_dir.is_absolute() {
        dist_dir
    } else {
        root.join(dist_dir)
    };
    let candidates = [
        std::env::var_os("KIANA_ENTERPRISE_OFFLINE_MANIFEST_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_ENTERPRISE_OFFLINE_MANIFEST_OUT").map(PathBuf::from),
        Some(
            dist_dir
                .join("manifests")
                .join("enterprise")
                .join("offline-manifest.json"),
        ),
    ];
    let manifest_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no enterprise offline manifest found in KIANA_ENTERPRISE_OFFLINE_MANIFEST_FILE, KIANA_ENTERPRISE_OFFLINE_MANIFEST_OUT, or DIST_DIR/manifests/enterprise/offline-manifest.json"
            )
        })?;
    let report = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read enterprise offline manifest {}",
            manifest_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse enterprise offline manifest JSON {}",
            manifest_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.enterprise.offline-manifest.v1") {
        return Err(anyhow!(
            "enterprise offline manifest {} has unexpected schema",
            manifest_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_commercial_proof_manifest_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let dist_dir = if dist_dir.is_absolute() {
        dist_dir
    } else {
        root.join(dist_dir)
    };
    let candidates = [
        std::env::var_os("KIANA_COMMERCIAL_PROOF_MANIFEST_FILE").map(PathBuf::from),
        std::env::var_os("KIANA_COMMERCIAL_PROOF_MANIFEST_OUT").map(PathBuf::from),
        Some(dist_dir.join("proofs").join("PROOF-MANIFEST.json")),
    ];
    let manifest_path = candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
        .ok_or_else(|| {
            anyhow!(
                "no commercial proof manifest found in KIANA_COMMERCIAL_PROOF_MANIFEST_FILE, KIANA_COMMERCIAL_PROOF_MANIFEST_OUT, or DIST_DIR/proofs/PROOF-MANIFEST.json"
            )
        })?;
    let report = std::fs::read_to_string(&manifest_path).with_context(|| {
        format!(
            "failed to read commercial proof manifest {}",
            manifest_path.display()
        )
    })?;
    let value: Value = serde_json::from_str(&report).with_context(|| {
        format!(
            "failed to parse commercial proof manifest JSON {}",
            manifest_path.display()
        )
    })?;
    if value.get("schema").and_then(Value::as_str) != Some("kiana.commercial-proof-manifest.v1") {
        return Err(anyhow!(
            "commercial proof manifest {} has unexpected schema",
            manifest_path.display()
        ));
    }
    Ok(value)
}

fn direct_connect_live_provider_smoke_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let dist_dir = if dist_dir.is_absolute() {
        dist_dir
    } else {
        root.join(dist_dir)
    };
    let target_provider_dir = root.join("target").join("live-smoke").join("provider");
    let catalog_path = direct_connect_first_existing_path(
        root,
        [
            std::env::var_os("KIANA_PROVIDER_LIVE_CATALOG_FILE").map(PathBuf::from),
            std::env::var_os("KIANA_PROVIDER_LIVE_CATALOG_OUT").map(PathBuf::from),
            Some(
                dist_dir
                    .join("proofs")
                    .join("live-smoke")
                    .join("provider")
                    .join("model-catalog-live.json"),
            ),
            Some(target_provider_dir.join("model-catalog-live.json")),
        ],
    )
    .ok_or_else(|| {
        anyhow!(
            "no live provider catalog proof found in KIANA_PROVIDER_LIVE_CATALOG_FILE, KIANA_PROVIDER_LIVE_CATALOG_OUT, DIST_DIR/proofs/live-smoke/provider/model-catalog-live.json, or target/live-smoke/provider/model-catalog-live.json"
        )
    })?;
    let smoke_path = direct_connect_first_existing_path(
        root,
        [
            std::env::var_os("KIANA_PROVIDER_LIVE_SMOKE_FILE").map(PathBuf::from),
            std::env::var_os("KIANA_PROVIDER_LIVE_SMOKE_OUT").map(PathBuf::from),
            Some(
                dist_dir
                    .join("proofs")
                    .join("live-smoke")
                    .join("provider")
                    .join("model-smoke-live-tools.json"),
            ),
            Some(target_provider_dir.join("model-smoke-live-tools.json")),
        ],
    )
    .ok_or_else(|| {
        anyhow!(
            "no live provider smoke proof found in KIANA_PROVIDER_LIVE_SMOKE_FILE, KIANA_PROVIDER_LIVE_SMOKE_OUT, DIST_DIR/proofs/live-smoke/provider/model-smoke-live-tools.json, or target/live-smoke/provider/model-smoke-live-tools.json"
        )
    })?;
    let catalog = direct_connect_read_json_schema_file(
        &catalog_path,
        "live provider catalog proof",
        "kiana.model-catalog.v1",
    )?;
    let smoke = direct_connect_read_json_schema_file(
        &smoke_path,
        "live provider smoke proof",
        "kiana.model-smoke.v1",
    )?;
    Ok(serde_json::json!({
        "schema": "kiana.app-server.live-provider-smoke.v1",
        "catalog_path": catalog_path.display().to_string(),
        "smoke_path": smoke_path.display().to_string(),
        "catalog": catalog,
        "smoke": smoke,
    }))
}

fn direct_connect_remote_code_session_smoke_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let dist_dir = std::env::var_os("DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let dist_dir = if dist_dir.is_absolute() {
        dist_dir
    } else {
        root.join(dist_dir)
    };
    let live_root = root.join("target").join("live-smoke");
    let remote_dir = std::env::var_os("KIANA_REMOTE_LIVE_SMOKE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| live_root.join("remote"));
    let remote_dir = if remote_dir.is_absolute() {
        remote_dir
    } else {
        root.join(remote_dir)
    };
    let proof_path = direct_connect_first_existing_path(
        root,
        [
            std::env::var_os("KIANA_REMOTE_SMOKE_PROOF_FILE").map(PathBuf::from),
            std::env::var_os("KIANA_REMOTE_SMOKE_PROOF_OUT").map(PathBuf::from),
            Some(
                dist_dir
                    .join("proofs")
                    .join("live-smoke")
                    .join("remote")
                    .join("code-session-smoke.json"),
            ),
            Some(remote_dir.join("code-session-smoke.json")),
        ],
    )
    .ok_or_else(|| {
        anyhow!(
            "no remote code-session smoke proof found in KIANA_REMOTE_SMOKE_PROOF_FILE, KIANA_REMOTE_SMOKE_PROOF_OUT, DIST_DIR/proofs/live-smoke/remote/code-session-smoke.json, or target/live-smoke/remote/code-session-smoke.json"
        )
    })?;
    direct_connect_read_json_schema_file(
        &proof_path,
        "remote code-session smoke proof",
        "kiana.remote-code-session-smoke.v1",
    )
}

fn direct_connect_distribution_review_report() -> Result<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))?;
    let dist_dir = std::env::var_os("KIANA_DISTRIBUTION_REVIEW_DIST_DIR")
        .or_else(|| std::env::var_os("DIST_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("dist"));
    let dist_dir = if dist_dir.is_absolute() {
        dist_dir
    } else {
        root.join(dist_dir)
    };
    let version = std::fs::read_to_string(root.join("VERSION"))
        .unwrap_or_else(|_| "0.1.0".to_string())
        .trim()
        .to_string();

    let mut artifacts = direct_connect_distribution_artifacts(&dist_dir)?;
    artifacts.sort_by(|left, right| {
        left.get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(
                right
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            )
    });
    let targets = artifacts
        .iter()
        .filter_map(|artifact| artifact.get("target").and_then(Value::as_str))
        .collect::<HashSet<_>>();
    let homebrew_formulae = direct_connect_distribution_manifest_files(
        &dist_dir,
        &["manifests", "homebrew"],
        "rb",
        false,
    )?;
    let homebrew_blocked_path = dist_dir
        .join("manifests")
        .join("homebrew")
        .join("BLOCKED.md");
    let winget_manifests = direct_connect_distribution_manifest_files(
        &dist_dir,
        &["manifests", "winget"],
        "yaml",
        true,
    )?;
    let winget_blocked_path = dist_dir.join("manifests").join("winget").join("BLOCKED.md");
    let enterprise_manifest_path = dist_dir
        .join("manifests")
        .join("enterprise")
        .join("offline-manifest.json");
    let enterprise_offline_manifest = if enterprise_manifest_path.is_file() {
        match direct_connect_read_json_schema_file(
            &enterprise_manifest_path,
            "enterprise offline manifest",
            "kiana.enterprise.offline-manifest.v1",
        ) {
            Ok(manifest) => serde_json::json!({
                "present": true,
                "path": direct_connect_relative_path(&dist_dir, &enterprise_manifest_path),
                "valid": true,
                "schema": manifest["schema"].clone(),
                "artifact_count": manifest
                    .get("artifacts")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or_default(),
                "channels": manifest.get("channels").cloned().unwrap_or(Value::Null),
            }),
            Err(error) => serde_json::json!({
                "present": true,
                "path": direct_connect_relative_path(&dist_dir, &enterprise_manifest_path),
                "valid": false,
                "error": error.to_string(),
            }),
        }
    } else {
        serde_json::json!({
            "present": false,
            "path": "manifests/enterprise/offline-manifest.json",
            "valid": false,
        })
    };

    let missing_platforms = ["linux", "macos", "windows"]
        .into_iter()
        .filter(|platform| !targets.iter().any(|target| target.contains(*platform)))
        .collect::<Vec<_>>();
    let blockers = direct_connect_distribution_review_blockers(
        artifacts.len(),
        &homebrew_formulae,
        homebrew_blocked_path.is_file(),
        &winget_manifests,
        winget_blocked_path.is_file(),
        enterprise_offline_manifest
            .get("valid")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        &missing_platforms,
    );
    let blocking_count = blockers
        .iter()
        .filter(|blocker| blocker.get("blocking").and_then(Value::as_bool) == Some(true))
        .count();
    let channel_status = |ready: bool, blocked: bool| {
        if blocked {
            "blocked"
        } else if ready {
            "ready"
        } else {
            "missing"
        }
    };
    let artifact_count = artifacts.len();
    let linux_present = targets.iter().any(|target| target.contains("linux"));
    let macos_present = targets.iter().any(|target| target.contains("macos"));
    let windows_present = targets.iter().any(|target| target.contains("windows"));
    let homebrew_ready = !homebrew_formulae.is_empty();
    let winget_ready = !winget_manifests.is_empty();
    let winget_blocked = winget_blocked_path.is_file();
    let enterprise_manifest_ready = enterprise_offline_manifest
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let channels_ready = [homebrew_ready, winget_ready, enterprise_manifest_ready]
        .into_iter()
        .filter(|ready| *ready)
        .count();
    let github_release_status = channel_status(!targets.is_empty(), false);
    let homebrew_status = channel_status(homebrew_ready, homebrew_blocked_path.is_file());
    let winget_status = channel_status(winget_ready, winget_blocked);
    let winget_blocked_path_value = if winget_blocked {
        Value::String(direct_connect_relative_path(
            &dist_dir,
            &winget_blocked_path,
        ))
    } else {
        Value::Null
    };

    Ok(serde_json::json!({
        "schema": "kiana.app-server.distribution-review.v1",
        "version": version,
        "dist_dir": dist_dir.display().to_string(),
        "summary": {
            "artifacts": artifact_count,
            "platforms": {
                "linux": linux_present,
                "macos": macos_present,
                "windows": windows_present,
            },
            "channels_ready": channels_ready,
            "blocking": blocking_count,
        },
        "artifacts": artifacts,
        "channels": {
            "github_releases": {
                "status": github_release_status,
                "artifact_count": artifact_count,
            },
            "homebrew": {
                "status": homebrew_status,
                "formulae": homebrew_formulae,
            },
            "winget": {
                "status": winget_status,
                "manifests": winget_manifests,
                "blocked_path": winget_blocked_path_value,
            },
        },
        "enterprise_offline_manifest": enterprise_offline_manifest,
        "blockers": blockers,
    }))
}

fn direct_connect_distribution_artifacts(dist_dir: &Path) -> Result<Vec<Value>> {
    if !dist_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut artifacts = Vec::new();
    for entry in std::fs::read_dir(dist_dir)
        .with_context(|| {
            format!(
                "failed to read distribution directory {}",
                dist_dir.display()
            )
        })?
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() || !direct_connect_distribution_is_archive(&path) {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let target = direct_connect_distribution_target_from_archive(file_name);
        let checksum_path = dist_dir.join(format!("{file_name}.sha256"));
        let package_name = direct_connect_distribution_package_stem(file_name);
        let binary_checksum_path = dist_dir.join(format!("{package_name}.binary.sha256"));
        artifacts.push(serde_json::json!({
            "target": target,
            "archive": file_name,
            "path": direct_connect_relative_path(dist_dir, &path),
            "sha256_file": if checksum_path.is_file() {
                Value::String(direct_connect_relative_path(dist_dir, &checksum_path))
            } else {
                Value::Null
            },
            "binary_sha256_file": if binary_checksum_path.is_file() {
                Value::String(direct_connect_relative_path(dist_dir, &binary_checksum_path))
            } else {
                Value::Null
            },
        }));
    }
    Ok(artifacts)
}

fn direct_connect_distribution_manifest_files(
    dist_dir: &Path,
    segments: &[&str],
    extension: &str,
    recursive: bool,
) -> Result<Vec<String>> {
    let mut dir = dist_dir.to_path_buf();
    for segment in segments {
        dir.push(segment);
    }
    let mut files = Vec::new();
    direct_connect_collect_distribution_manifest_files(
        dist_dir, &dir, extension, recursive, &mut files,
    )?;
    files.sort();
    Ok(files)
}

fn direct_connect_collect_distribution_manifest_files(
    dist_dir: &Path,
    dir: &Path,
    extension: &str,
    recursive: bool,
    files: &mut Vec<String>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read manifest directory {}", dir.display()))?
    {
        let path = entry?.path();
        if recursive && path.is_dir() {
            direct_connect_collect_distribution_manifest_files(
                dist_dir, &path, extension, recursive, files,
            )?;
            continue;
        }
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            files.push(direct_connect_relative_path(dist_dir, &path));
        }
    }
    Ok(())
}

fn direct_connect_distribution_review_blockers(
    artifact_count: usize,
    homebrew_formulae: &[String],
    homebrew_blocked: bool,
    winget_manifests: &[String],
    winget_blocked: bool,
    enterprise_manifest_valid: bool,
    missing_platforms: &[&str],
) -> Vec<Value> {
    let mut blockers = Vec::new();
    if artifact_count == 0 {
        blockers.push(serde_json::json!({
            "id": "distribution.artifacts",
            "blocking": true,
            "message": "no packaged release artifacts were found in the reviewed dist directory",
        }));
    }
    if !missing_platforms.is_empty() {
        blockers.push(serde_json::json!({
            "id": "distribution.platform-artifacts",
            "blocking": true,
            "message": format!("missing platform artifacts: {}", missing_platforms.join(", ")),
        }));
    }
    if homebrew_blocked || homebrew_formulae.is_empty() {
        blockers.push(serde_json::json!({
            "id": "distribution.homebrew",
            "blocking": true,
            "message": if homebrew_blocked {
                "Homebrew channel is explicitly blocked"
            } else {
                "Homebrew formula is not present"
            },
        }));
    }
    if winget_blocked || winget_manifests.is_empty() {
        blockers.push(serde_json::json!({
            "id": "distribution.winget",
            "blocking": true,
            "message": if winget_blocked {
                "winget channel is explicitly blocked"
            } else {
                "winget manifest is not present"
            },
        }));
    }
    if !enterprise_manifest_valid {
        blockers.push(serde_json::json!({
            "id": "distribution.enterprise-offline-manifest",
            "blocking": true,
            "message": "enterprise offline manifest is missing or invalid",
        }));
    }
    blockers
}

fn direct_connect_distribution_is_archive(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".tar.gz") || name.ends_with(".zip") || name.ends_with(".tgz")
        })
}

fn direct_connect_distribution_target_from_archive(file_name: &str) -> String {
    let trimmed = direct_connect_distribution_package_stem(file_name);
    trimmed
        .strip_prefix("kiana-")
        .and_then(|rest| rest.split_once('-').map(|(_, target)| target.to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn direct_connect_distribution_package_stem(file_name: &str) -> &str {
    file_name
        .strip_suffix(".tar.gz")
        .or_else(|| file_name.strip_suffix(".zip"))
        .or_else(|| file_name.strip_suffix(".tgz"))
        .unwrap_or(file_name)
}

fn direct_connect_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn direct_connect_first_existing_path<const N: usize>(
    root: &Path,
    candidates: [Option<PathBuf>; N],
) -> Option<PathBuf> {
    candidates
        .into_iter()
        .flatten()
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .find(|path| path.is_file())
}

fn direct_connect_read_json_schema_file(
    path: &Path,
    label: &str,
    expected_schema: &str,
) -> Result<Value> {
    let report = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {label} file {}", path.display()))?;
    let value: Value = serde_json::from_str(&report)
        .with_context(|| format!("failed to parse {label} JSON {}", path.display()))?;
    if value.get("schema").and_then(Value::as_str) != Some(expected_schema) {
        return Err(anyhow!(
            "{label} file {} has unexpected schema",
            path.display()
        ));
    }
    Ok(value)
}

async fn direct_connect_app_secrets_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(direct_connect_app_secrets_payload(&state.base_options)).into_response()
}

fn direct_connect_app_secrets_payload(base_options: &HashMap<String, Value>) -> Value {
    let anthropic_api_key =
        direct_connect_secret_presence("anthropic_api_key", "Anthropic API key", || {
            if direct_connect_nonempty_base_option(base_options, "api_key").is_some() {
                Some("app_state".to_string())
            } else if std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
            {
                Some("ANTHROPIC_API_KEY".to_string())
            } else {
                None
            }
        });
    let anthropic_oauth_token = direct_connect_oauth_secret_presence();
    let openai_compatible_api_key = direct_connect_secret_presence(
        "openai_compatible_api_key",
        "OpenAI-compatible API key",
        || {
            for key in ["KIANA_OPENAI_API_KEY", "OPENAI_API_KEY"] {
                if std::env::var(key)
                    .ok()
                    .is_some_and(|value| !value.trim().is_empty())
                {
                    return Some(key.to_string());
                }
            }
            None
        },
    );
    let enterprise_license_key =
        direct_connect_secret_presence("enterprise_license_key", "Enterprise license key", || {
            if std::env::var("KIANA_LICENSE_KEY")
                .ok()
                .is_some_and(|value| !value.trim().is_empty())
            {
                Some("KIANA_LICENSE_KEY".to_string())
            } else {
                None
            }
        });
    let values = vec![
        anthropic_api_key,
        anthropic_oauth_token,
        openai_compatible_api_key,
        enterprise_license_key,
    ];
    let set = values
        .iter()
        .filter(|value| value.get("status").and_then(Value::as_str) == Some("set"))
        .count();
    let invalid = values
        .iter()
        .filter(|value| value.get("status").and_then(Value::as_str) == Some("invalid"))
        .count();
    let missing = values
        .iter()
        .filter(|value| value.get("status").and_then(Value::as_str) == Some("missing"))
        .count();

    serde_json::json!({
        "schema": "kiana.app-server.secrets.v1",
        "metadata_supported": true,
        "values": values,
        "summary": {
            "total": set + invalid + missing,
            "set": set,
            "missing": missing,
            "invalid": invalid
        },
        "read_supported": false,
        "write_supported": false,
        "policy": "secret values are never returned by the local app-server contract",
    })
}

fn direct_connect_secret_presence<F>(id: &str, label: &str, source: F) -> Value
where
    F: FnOnce() -> Option<String>,
{
    match source() {
        Some(source) => serde_json::json!({
            "id": id,
            "label": label,
            "status": "set",
            "source": source,
            "value": "redacted",
            "readable": false,
            "writable": false
        }),
        None => serde_json::json!({
            "id": id,
            "label": label,
            "status": "missing",
            "source": "none",
            "value": "missing",
            "readable": false,
            "writable": false
        }),
    }
}

fn direct_connect_oauth_secret_presence() -> Value {
    let inspection = kiana_services::oauth::inspect_oauth_tokens(
        kiana_services::oauth::DEFAULT_OAUTH_EXPIRY_SKEW,
    );
    let (status, value) = match inspection.status {
        kiana_services::oauth::OAuthTokenFileStatus::Valid => ("set", "redacted"),
        kiana_services::oauth::OAuthTokenFileStatus::Missing => ("missing", "missing"),
        kiana_services::oauth::OAuthTokenFileStatus::Invalid => ("invalid", "invalid"),
    };
    serde_json::json!({
        "id": "anthropic_oauth_token",
        "label": "Anthropic OAuth token",
        "status": status,
        "source": "oauth_file",
        "value": value,
        "readable": false,
        "writable": false,
        "file": inspection.file,
        "store": inspection.store,
        "refreshable": inspection.refreshable,
        "expired": inspection.expired,
        "expiring": inspection.expiring,
        "error": inspection.error
    })
}

async fn direct_connect_app_sandbox_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(serde_json::json!({
        "schema": "kiana.app-server.sandbox.v1",
        "workspace": state.workspace.display().to_string(),
        "default_permission_mode": state
            .base_options
            .get("permission_mode")
            .or_else(|| state.base_options.get("permissionMode"))
            .and_then(Value::as_str)
            .unwrap_or("prompt"),
        "controls": [
            "bearer_auth",
            "permission_prompt",
            "exec_policy"
        ],
    }))
    .into_response()
}

async fn direct_connect_app_permissions_status_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(direct_connect_app_permissions_status_payload(&state)).into_response()
}

fn direct_connect_app_permissions_status_payload(state: &DirectConnectServerState) -> Value {
    let app_state = direct_connect_app_state_with_workspace(state);
    let permissions = effective_tool_permissions(&app_state);
    let file_status = if permissions.file_error.is_some() {
        "error"
    } else if permissions.file_loaded {
        "loaded"
    } else {
        "missing"
    };
    let managed_policy_status = if permissions.managed_policy_loaded {
        "loaded"
    } else if permissions.managed_policy_error.is_some() {
        "error"
    } else if permissions.managed_policy_path.is_some() {
        "missing"
    } else {
        "none"
    };

    serde_json::json!({
        "schema": "kiana.app-server.permissions-status.v1",
        "workspace": state.workspace.display().to_string(),
        "profile": permissions.profile,
        "mode": permissions.mode,
        "sources": permissions.sources,
        "file": {
            "path": permissions.file_path.display().to_string(),
            "status": file_status,
            "loaded": permissions.file_loaded,
            "error": permissions.file_error,
        },
        "managed_policy": {
            "path": permissions
                .managed_policy_path
                .as_ref()
                .map(|path| path.display().to_string()),
            "status": managed_policy_status,
            "loaded": permissions.managed_policy_loaded,
            "error": permissions.managed_policy_error,
        },
        "rules": {
            "allowed_tools": permissions.allowed_tools,
            "disallowed_tools": permissions.disallowed_tools,
            "ask_tools": permissions.ask_tools,
            "managed_allowed_tools": permissions.managed_allowed_tools,
            "managed_disallowed_tools": permissions.managed_disallowed_tools,
            "managed_ask_tools": permissions.managed_ask_tools,
        },
        "interactive_prompts": {
            "supported": true,
            "non_interactive_requires_allow_rule": true,
        },
    })
}

async fn direct_connect_app_trust_status_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(direct_connect_app_trust_status_payload(&state)).into_response()
}

fn direct_connect_app_trust_status_payload(state: &DirectConnectServerState) -> Value {
    kiana_commands::trust::trust_status_payload(&CommandContext {
        args: "json".to_string(),
        app_state: direct_connect_app_state_with_workspace(state),
    })
}

fn direct_connect_app_state_with_workspace(
    state: &DirectConnectServerState,
) -> HashMap<String, Value> {
    let mut app_state = state.base_options.clone();
    app_state.insert(
        "cwd".to_string(),
        Value::String(state.workspace.display().to_string()),
    );
    app_state
}

async fn direct_connect_app_plugins_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let context = CommandContext {
        args: String::new(),
        app_state: HashMap::from([(
            "cwd".to_string(),
            Value::String(state.workspace.display().to_string()),
        )]),
    };
    let plugins = match kiana_commands::plugin::installed_plugin_summaries(&context) {
        Ok(Value::Array(plugins)) => plugins,
        Ok(_) => Vec::new(),
        Err(error) => {
            return direct_connect_json_error(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("failed to load plugin summaries: {error}"),
            )
        }
    };
    axum::Json(serde_json::json!({
        "schema": "kiana.app-server.plugins.v1",
        "count": plugins.len(),
        "plugins": plugins,
    }))
    .into_response()
}

async fn direct_connect_app_auth_status_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("auth") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "auth command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "status --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| {
        let mut value = serde_json::from_str::<Value>(&result.value)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "schema".to_string(),
                Value::String("kiana.auth-status.v1".to_string()),
            );
        }
        Ok(value)
    }) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build auth status report: {error}"),
        ),
    }
}

async fn direct_connect_app_license_status_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("license") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "license command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "status --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build license status report: {error}"),
        ),
    }
}

async fn direct_connect_app_model_catalog_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("model") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "model command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "catalog --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build model catalog report: {error}"),
        ),
    }
}

async fn direct_connect_app_model_list_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("model") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "model command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "list --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(profiles) => {
            let profile_count = profiles.as_array().map_or(0, Vec::len);
            axum::Json(serde_json::json!({
                "schema": "kiana.model-list.v1",
                "summary": {
                    "profiles": profile_count
                },
                "profiles": profiles
            }))
            .into_response()
        }
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build model list report: {error}"),
        ),
    }
}

async fn direct_connect_app_model_current_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(direct_connect_app_model_current_payload(&state)).into_response()
}

#[derive(Debug, Deserialize)]
struct DirectConnectModelCurrentRequest {
    #[serde(default, alias = "modelId", alias = "model")]
    model_id: Option<String>,
    #[serde(default, alias = "providerId")]
    provider_id: Option<String>,
}

async fn direct_connect_app_model_current_post_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DirectConnectModelCurrentRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }
    if let Some(source) = direct_connect_active_model_override_source(&state.base_options) {
        return direct_connect_json_error(
            axum::http::StatusCode::CONFLICT,
            format!("current model is controlled by {source}; remove the runtime override before writing model config"),
        );
    }

    match direct_connect_update_current_model(&state, body).await {
        Ok(payload) => axum::Json(payload).into_response(),
        Err(error) => {
            direct_connect_json_error(axum::http::StatusCode::BAD_REQUEST, error.to_string())
        }
    }
}

async fn direct_connect_update_current_model(
    state: &DirectConnectServerState,
    request: DirectConnectModelCurrentRequest,
) -> Result<Value> {
    let model_id = request
        .model_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("model_id is required"))?;
    let profile = kiana_services::api::provider::built_in_model_profiles()
        .into_iter()
        .find(|profile| profile.model_id == model_id)
        .ok_or_else(|| anyhow!("model_id '{}' is not in the built-in model list", model_id))?;
    if let Some(provider_id) = request
        .provider_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if provider_id != profile.provider_id {
            return Err(anyhow!(
                "provider_id '{}' does not match model_id '{}' provider '{}'",
                provider_id,
                model_id,
                profile.provider_id
            ));
        }
    }

    let registry = create_default_command_registry();
    let command = registry
        .get("model")
        .ok_or_else(|| anyhow!("model command is not registered"))?;
    command
        .execute(CommandContext {
            args: model_id.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await?;

    Ok(direct_connect_app_model_current_payload(state))
}

fn direct_connect_app_model_current_payload(state: &DirectConnectServerState) -> Value {
    let (model_id, source) =
        if let Some(model_id) = direct_connect_nonempty_base_option(&state.base_options, "model") {
            (model_id.to_string(), "app_state".to_string())
        } else if let Ok(model_id) = std::env::var("ANTHROPIC_MODEL") {
            let model_id = model_id.trim().to_string();
            if !model_id.is_empty() {
                (model_id, "ANTHROPIC_MODEL".to_string())
            } else {
                let config = kiana_bootstrap::config::load_config();
                (config.model, "config".to_string())
            }
        } else {
            let config = kiana_bootstrap::config::load_config();
            (config.model, "config".to_string())
        };
    let provider_id = direct_connect_current_model_provider_id(&model_id);
    let profile = kiana_services::api::provider::model_profile(provider_id, &model_id)
        .map(|profile| serde_json::to_value(profile).unwrap_or_else(|_| Value::Null))
        .unwrap_or(Value::Null);

    serde_json::json!({
        "schema": "kiana.app-server.model-current.v1",
        "workspace": state.workspace.display().to_string(),
        "configured": !model_id.trim().is_empty(),
        "source": source,
        "provider_id": provider_id,
        "model_id": model_id,
        "profile": profile
    })
}

fn direct_connect_active_model_override_source(
    base_options: &HashMap<String, Value>,
) -> Option<&'static str> {
    if direct_connect_nonempty_base_option(base_options, "model").is_some() {
        return Some("app_state");
    }
    if std::env::var("ANTHROPIC_MODEL")
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
    {
        return Some("ANTHROPIC_MODEL");
    }
    None
}

fn direct_connect_current_model_provider_id(model_id: &str) -> &'static str {
    let profiles = kiana_services::api::provider::built_in_model_profiles();
    if let Some(profile) = profiles.iter().find(|profile| profile.model_id == model_id) {
        return match profile.provider_id.as_str() {
            kiana_services::api::provider::OPENAI_COMPATIBLE_PROVIDER_ID => {
                kiana_services::api::provider::OPENAI_COMPATIBLE_PROVIDER_ID
            }
            kiana_services::api::provider::OLLAMA_PROVIDER_ID => {
                kiana_services::api::provider::OLLAMA_PROVIDER_ID
            }
            kiana_services::api::provider::FAKE_PROVIDER_ID => {
                kiana_services::api::provider::FAKE_PROVIDER_ID
            }
            _ => kiana_services::api::provider::ANTHROPIC_PROVIDER_ID,
        };
    }
    if model_id.starts_with("gpt-") || model_id.starts_with("o1") || model_id.starts_with("o3") {
        kiana_services::api::provider::OPENAI_COMPATIBLE_PROVIDER_ID
    } else if model_id.starts_with("llama")
        || model_id.starts_with("mistral")
        || model_id.starts_with("qwen")
    {
        kiana_services::api::provider::OLLAMA_PROVIDER_ID
    } else if model_id.starts_with("fake-") {
        kiana_services::api::provider::FAKE_PROVIDER_ID
    } else {
        kiana_services::api::provider::ANTHROPIC_PROVIDER_ID
    }
}

async fn direct_connect_app_model_smoke_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("model") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "model command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "smoke --json --tools".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build model smoke report: {error}"),
        ),
    }
}

async fn direct_connect_app_git_status_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    axum::Json(direct_connect_git_status_report(&state.workspace)).into_response()
}

async fn direct_connect_app_diff_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("diff") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "diff command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "--json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| {
        let mut value = serde_json::from_str::<Value>(&result.value)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "schema".to_string(),
                Value::String("kiana.diff.v1".to_string()),
            );
        }
        Ok(value)
    }) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build diff report: {error}"),
        ),
    }
}

async fn direct_connect_app_checkpoint_create_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("checkpoint") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "checkpoint command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "--json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| {
        let mut value = serde_json::from_str::<Value>(&result.value)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "schema".to_string(),
                Value::String("kiana.checkpoint.v1".to_string()),
            );
        }
        Ok(value)
    }) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to create checkpoint: {error}"),
        ),
    }
}

async fn direct_connect_app_checks_dry_run_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("checks") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "checks command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "--dry-run --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build checks dry-run report: {error}"),
        ),
    }
}

async fn direct_connect_app_checks_run_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    direct_connect_app_checks_handler(state, headers, "--json", "run").await
}

async fn direct_connect_app_checks_handler(
    state: DirectConnectServerState,
    headers: axum::http::HeaderMap,
    args: &str,
    report_kind: &str,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("checks") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "checks command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build checks {report_kind} report: {error}"),
        ),
    }
}

async fn direct_connect_app_review_dry_run_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("review") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "review command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: "--dry-run --json".to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build review dry-run report: {error}"),
        ),
    }
}

async fn direct_connect_app_review_run_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    direct_connect_app_review_handler(state, headers, "--json", "run").await
}

async fn direct_connect_app_review_handler(
    state: DirectConnectServerState,
    headers: axum::http::HeaderMap,
    args: &str,
    report_kind: &str,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let registry = create_default_command_registry();
    let Some(command) = registry.get("review") else {
        return direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "review command is not registered",
        );
    };
    let result = command
        .execute(CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                Value::String(state.workspace.display().to_string()),
            )]),
        })
        .await;

    match result.and_then(|result| serde_json::from_str::<Value>(&result.value).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to build review {report_kind} report: {error}"),
        ),
    }
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextQuery {
    q: String,
    limit: Option<usize>,
    max_snippet_lines: Option<usize>,
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextIndexQuery {
    cache: Option<bool>,
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextArtifactsQuery {
    cache: Option<bool>,
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextIngestRequest {
    source: String,
    store: Option<String>,
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextArtifactGraphQuery {
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextArtifactStoreQuery {
    cache: Option<bool>,
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectContextArtifactReadinessQuery {
    max_bytes_per_file: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct DirectConnectRepoMapQuery {
    max_tokens: Option<u64>,
}

async fn direct_connect_app_context_index_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextIndexQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let options = ContextIndexOptions {
        max_bytes_per_file: query.max_bytes_per_file,
    };
    let index = if query.cache.unwrap_or(false) {
        build_persistent_context_index(
            &state.workspace,
            options,
            state.workspace.join(".kiana").join("context-index.json"),
        )
    } else {
        build_context_index(&state.workspace, options)
    };

    match index.and_then(|index| serde_json::to_value(index).map_err(Into::into)) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build context index: {error}"),
        ),
    }
}

async fn direct_connect_app_context_artifacts_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextArtifactsQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let options = ContextArtifactOptions {
        max_bytes_per_file: query.max_bytes_per_file,
    };
    let report = if query.cache.unwrap_or(false) {
        build_persistent_context_artifacts(
            &state.workspace,
            options,
            state
                .workspace
                .join(".kiana")
                .join("context-artifacts.json"),
        )
    } else {
        build_context_artifacts(&state.workspace, options)
    };

    match report.and_then(|report| serde_json::to_value(report).map_err(Into::into)) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build context artifacts: {error}"),
        ),
    }
}

async fn direct_connect_app_context_ingest_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DirectConnectContextIngestRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match direct_connect_app_context_ingest_report(&state, body)
        .and_then(|report| serde_json::to_value(report).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to ingest context artifacts: {error}"),
        ),
    }
}

fn direct_connect_app_context_ingest_report(
    state: &DirectConnectServerState,
    body: DirectConnectContextIngestRequest,
) -> Result<kiana_query::ContextArtifactIngest> {
    let source = body.source.trim();
    if source.is_empty() {
        return Err(anyhow!("source is required"));
    }
    let source = PathBuf::from(source);
    let source = if source.is_absolute() {
        source
    } else {
        state.workspace.join(source)
    };
    let workspace = state
        .workspace
        .canonicalize()
        .with_context(|| format!("failed to resolve workspace {}", state.workspace.display()))?;
    let source = source.canonicalize().with_context(|| {
        format!(
            "failed to resolve context ingest source {}",
            source.display()
        )
    })?;
    if !source.starts_with(&workspace) {
        return Err(anyhow!(
            "context ingest source must stay inside the workspace root"
        ));
    }

    let store_dir = body
        .store
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    ingest_context_artifacts(
        &workspace,
        source,
        ContextArtifactIngestOptions {
            store_dir,
            max_bytes_per_file: body.max_bytes_per_file,
        },
    )
}

async fn direct_connect_app_context_artifact_graph_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextArtifactGraphQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let options = ContextArtifactOptions {
        max_bytes_per_file: query.max_bytes_per_file,
    };
    match build_context_artifact_dependency_graph(&state.workspace, options)
        .and_then(|graph| serde_json::to_value(graph).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build context artifact graph: {error}"),
        ),
    }
}

async fn direct_connect_app_context_artifact_store_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextArtifactStoreQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let options = ContextArtifactOptions {
        max_bytes_per_file: query.max_bytes_per_file,
    };
    let store = if query.cache.unwrap_or(false) {
        build_persistent_context_artifact_store(
            &state.workspace,
            options,
            state
                .workspace
                .join(".kiana")
                .join("context-artifact-store.json"),
        )
    } else {
        build_context_artifact_store(&state.workspace, options)
    };

    match store.and_then(|store| serde_json::to_value(store).map_err(Into::into)) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build context artifact store: {error}"),
        ),
    }
}

async fn direct_connect_app_context_artifact_readiness_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextArtifactReadinessQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let options = ContextArtifactOptions {
        max_bytes_per_file: query.max_bytes_per_file,
    };
    match build_context_artifact_readiness(&state.workspace, options)
        .and_then(|readiness| serde_json::to_value(readiness).map_err(Into::into))
    {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build context artifact readiness: {error}"),
        ),
    }
}

async fn direct_connect_app_context_repo_map_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectRepoMapQuery>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    match build_repo_map(
        &state.workspace,
        RepoMapOptions {
            max_tokens: query.max_tokens,
        },
    )
    .and_then(|repo_map| {
        let mut value = serde_json::to_value(repo_map)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "schema".to_string(),
                Value::String("kiana.repo-map.v1".to_string()),
            );
        }
        Ok(value)
    }) {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build repo map: {error}"),
        ),
    }
}

async fn direct_connect_app_context_search_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextQuery>,
) -> axum::response::Response {
    direct_connect_app_context_handler(state, headers, query, "search").await
}

async fn direct_connect_app_context_vector_search_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextQuery>,
) -> axum::response::Response {
    direct_connect_app_context_handler(state, headers, query, "vector-search").await
}

async fn direct_connect_app_context_pack_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::extract::Query(query): axum::extract::Query<DirectConnectContextQuery>,
) -> axum::response::Response {
    direct_connect_app_context_handler(state, headers, query, "pack").await
}

async fn direct_connect_app_context_handler(
    state: DirectConnectServerState,
    headers: axum::http::HeaderMap,
    query: DirectConnectContextQuery,
    mode: &str,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }
    if query.q.trim().is_empty() {
        return direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            "q query parameter is required",
        );
    }

    match direct_connect_context_query_report(&state, query, mode).await {
        Ok(value) => axum::Json(value).into_response(),
        Err(error) => direct_connect_json_error(
            axum::http::StatusCode::BAD_REQUEST,
            format!("failed to build context {mode}: {error}"),
        ),
    }
}

async fn direct_connect_context_query_report(
    state: &DirectConnectServerState,
    query: DirectConnectContextQuery,
    mode: &str,
) -> Result<Value> {
    match mode {
        "search" => Ok(serde_json::to_value(search_context_index(
            &state.workspace,
            &query.q,
            ContextSearchOptions {
                limit: query.limit,
                max_bytes_per_file: query.max_bytes_per_file,
            },
        )?)?),
        "vector-search" => Ok(serde_json::to_value(search_context_vectors(
            &state.workspace,
            &query.q,
            ContextVectorSearchOptions {
                limit: query.limit,
                max_bytes_per_file: query.max_bytes_per_file,
            },
        )?)?),
        "pack" => Ok(serde_json::to_value(build_context_pack(
            &state.workspace,
            &query.q,
            ContextPackOptions {
                limit: query.limit,
                max_bytes_per_file: query.max_bytes_per_file,
                max_snippet_lines: query.max_snippet_lines,
            },
        )?)?),
        _ => Err(anyhow!("unsupported context mode: {mode}")),
    }
}

async fn direct_connect_create_session_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<DirectConnectCreateSessionRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let work_dir = match direct_connect_session_work_dir(&state.workspace, body.cwd.as_deref()) {
        Ok(work_dir) => work_dir,
        Err(error) => {
            return direct_connect_json_error(
                axum::http::StatusCode::BAD_REQUEST,
                error.to_string(),
            )
        }
    };
    let mut sessions = state.sessions.lock().await;
    if state.max_sessions > 0 && sessions.len() >= state.max_sessions {
        return direct_connect_json_error(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "maximum direct-connect sessions reached",
        );
    }

    let session_id = format!("kiana-direct-{}", uuid::Uuid::new_v4());
    sessions.insert(
        session_id.clone(),
        DirectConnectServerSession {
            session_id: session_id.clone(),
            work_dir: work_dir.clone(),
            dangerously_skip_permissions: body.dangerously_skip_permissions,
        },
    );

    axum::Json(serde_json::json!({
        "session_id": session_id,
        "ws_url": direct_connect_server_ws_url_for_state(&state, &session_id),
        "work_dir": work_dir.display().to_string(),
    }))
    .into_response()
}

async fn direct_connect_ws_handler(
    axum::extract::State(state): axum::extract::State<DirectConnectServerState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    ws: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if !direct_connect_authorized(&state.auth_token, &headers) {
        return direct_connect_json_error(
            axum::http::StatusCode::UNAUTHORIZED,
            "missing or invalid bearer token",
        );
    }

    let session = {
        let sessions = state.sessions.lock().await;
        sessions.get(&session_id).cloned()
    };
    let Some(session) = session else {
        return direct_connect_json_error(
            axum::http::StatusCode::NOT_FOUND,
            "direct-connect session not found",
        );
    };

    ws.on_upgrade(move |socket| direct_connect_ws_loop(socket, state, session))
        .into_response()
}

async fn direct_connect_ws_loop(
    socket: axum::extract::ws::WebSocket,
    state: DirectConnectServerState,
    session: DirectConnectServerSession,
) {
    use axum::extract::ws::Message as AxumWsMessage;

    let socket = Arc::new(AsyncMutex::new(socket));
    let mut options = direct_connect_server_prompt_options(&state, &session);
    let session_id = session.session_id.clone();
    loop {
        let next = {
            let mut socket = socket.lock().await;
            if state.idle_timeout_ms == 0 {
                socket.recv().await
            } else {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(state.idle_timeout_ms),
                    socket.recv(),
                )
                .await
                {
                    Ok(value) => value,
                    Err(_) => break,
                }
            }
        };

        let Some(message) = next else {
            break;
        };
        let Ok(message) = message else {
            break;
        };
        match message {
            AxumWsMessage::Text(text) => {
                for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                    let event = match serde_json::from_str::<Value>(line) {
                        Ok(event) => event,
                        Err(error) => {
                            if direct_connect_send_event(
                                &socket,
                                &direct_connect_error_result_event(
                                    &session_id,
                                    format!("direct-connect message was not valid JSON: {error}"),
                                    0,
                                ),
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    };

                    match event.get("type").and_then(Value::as_str) {
                        Some("user") => {
                            if direct_connect_run_user_event(
                                socket.clone(),
                                &mut options,
                                &session_id,
                                &event,
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                        Some("control_request") => {
                            let outcome =
                                handle_bridge_control_request_outcome(&event, &mut options);
                            if direct_connect_send_event(&socket, &outcome.response)
                                .await
                                .is_err()
                            {
                                break;
                            }
                            if outcome.end_session {
                                break;
                            }
                        }
                        Some("update_environment_variables") => {
                            let response = match apply_environment_variable_update(&event) {
                                Ok(()) => bridge_control_success(String::new(), None),
                                Err(error) => {
                                    bridge_control_error(String::new(), error.to_string())
                                }
                            };
                            if direct_connect_send_event(&socket, &response).await.is_err() {
                                break;
                            }
                        }
                        Some(
                            "assistant"
                            | "result"
                            | "control_response"
                            | "control_cancel_request"
                            | "keep_alive"
                            | "system",
                        )
                        | None => {}
                        Some(other) => {
                            if direct_connect_send_event(
                                &socket,
                                &direct_connect_error_result_event(
                                    &session_id,
                                    format!("unsupported direct-connect message type: {other}"),
                                    0,
                                ),
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            }
            AxumWsMessage::Ping(payload) => {
                if socket
                    .lock()
                    .await
                    .send(AxumWsMessage::Pong(payload))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            AxumWsMessage::Close(_) => break,
            AxumWsMessage::Binary(_) | AxumWsMessage::Pong(_) => {}
        }
    }
    state.sessions.lock().await.remove(&session_id);
}

fn direct_connect_server_prompt_options(
    state: &DirectConnectServerState,
    session: &DirectConnectServerSession,
) -> HashMap<String, Value> {
    let mut options = state.base_options.clone();
    options.insert("execute".to_string(), Value::Bool(true));
    options.insert(
        "session_id".to_string(),
        Value::String(session.session_id.clone()),
    );
    options.insert("create_session_if_missing".to_string(), Value::Bool(true));
    options.insert(
        "cwd".to_string(),
        Value::String(session.work_dir.display().to_string()),
    );
    if session.dangerously_skip_permissions {
        options.insert(
            "permission_mode".to_string(),
            Value::String("bypassPermissions".to_string()),
        );
        options.insert(
            "permissionMode".to_string(),
            Value::String("bypassPermissions".to_string()),
        );
    } else {
        options
            .entry("permission_prompt_tool".to_string())
            .or_insert_with(|| Value::String("stdio".to_string()));
    }
    options
}

async fn direct_connect_run_user_event(
    socket: Arc<AsyncMutex<axum::extract::ws::WebSocket>>,
    options: &mut HashMap<String, Value>,
    session_id: &str,
    event: &Value,
) -> Result<()> {
    let Some(prompt) = user_event_text(event).filter(|text| !text.trim().is_empty()) else {
        let error = direct_connect_error_result_event(session_id, "user message has no text", 0);
        direct_connect_send_event(&socket, &error).await?;
        return Ok(());
    };

    let started = Instant::now();
    let permission_handler = DirectConnectPermissionPromptHandler {
        socket: socket.clone(),
    };
    match crate::sdk::unstable_v2_prompt_with_permission_handler(
        prompt,
        options.clone(),
        &permission_handler,
    )
    .await
    {
        Ok(result) => {
            let assistant = bridge_assistant_event(&result);
            let result = stream_json_result_event(&result, started.elapsed().as_millis() as u64);
            direct_connect_send_events(&socket, &[assistant, result]).await?;
        }
        Err(error) => {
            let event = direct_connect_error_result_event(
                session_id,
                error.to_string(),
                started.elapsed().as_millis() as u64,
            );
            direct_connect_send_event(&socket, &event).await?;
        }
    }
    Ok(())
}

struct DirectConnectPermissionPromptHandler {
    socket: Arc<AsyncMutex<axum::extract::ws::WebSocket>>,
}

#[async_trait::async_trait]
impl PermissionPromptHandler for DirectConnectPermissionPromptHandler {
    async fn prompt(
        &self,
        request: PermissionPromptRequest,
    ) -> std::result::Result<PermissionPromptDecision, String> {
        let event = bridge_permission_request_event(&request);
        direct_connect_send_event(&self.socket, &event)
            .await
            .map_err(|error| error.to_string())?;

        loop {
            let next = {
                let mut socket = self.socket.lock().await;
                socket.recv().await
            };
            let Some(message) = next else {
                return Err(format!(
                    "direct-connect websocket closed before permission response for {}",
                    request.request_id
                ));
            };
            let message = message.map_err(|error| error.to_string())?;
            match message {
                axum::extract::ws::Message::Text(text) => {
                    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                        let event: Value = serde_json::from_str(line).map_err(|error| {
                            format!("permission response was not JSON: {error}")
                        })?;
                        match event.get("type").and_then(Value::as_str) {
                            Some("control_response") => {
                                if !bridge_control_response_matches(&event, &request.request_id) {
                                    continue;
                                }
                                direct_connect_send_event(
                                    &self.socket,
                                    &serde_json::json!({
                                        "type": "control_cancel_request",
                                        "request_id": request.request_id,
                                    }),
                                )
                                .await
                                .map_err(|error| error.to_string())?;
                                return bridge_permission_decision_from_control_response(&event);
                            }
                            Some("control_cancel_request") => {
                                if event.get("request_id").and_then(Value::as_str)
                                    == Some(request.request_id.as_str())
                                {
                                    return Ok(PermissionPromptDecision::Deny(
                                        "Permission request cancelled".to_string(),
                                    ));
                                }
                            }
                            Some("update_environment_variables") => {
                                apply_environment_variable_update(&event)
                                    .map_err(|error| error.to_string())?;
                            }
                            Some("keep_alive" | "assistant" | "system" | "user") => {}
                            Some(other) => {
                                return Err(format!(
                                    "permission prompt received unsupported direct-connect type '{other}'"
                                ));
                            }
                            None => {
                                return Err(
                                    "permission prompt received message without type".to_string()
                                );
                            }
                        }
                    }
                }
                axum::extract::ws::Message::Ping(payload) => {
                    let mut socket = self.socket.lock().await;
                    socket
                        .send(axum::extract::ws::Message::Pong(payload))
                        .await
                        .map_err(|error| error.to_string())?;
                }
                axum::extract::ws::Message::Close(_) => {
                    return Err("direct-connect websocket closed".to_string());
                }
                axum::extract::ws::Message::Binary(_) | axum::extract::ws::Message::Pong(_) => {}
            }
        }
    }
}

async fn direct_connect_send_event(
    socket: &Arc<AsyncMutex<axum::extract::ws::WebSocket>>,
    event: &Value,
) -> Result<()> {
    direct_connect_send_events(socket, std::slice::from_ref(event)).await
}

async fn direct_connect_send_events(
    socket: &Arc<AsyncMutex<axum::extract::ws::WebSocket>>,
    events: &[Value],
) -> Result<()> {
    let mut socket = socket.lock().await;
    direct_connect_send_events_locked(&mut socket, events).await
}

async fn direct_connect_send_events_locked(
    socket: &mut axum::extract::ws::WebSocket,
    events: &[Value],
) -> Result<()> {
    let payload = events
        .iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("\n");
    socket
        .send(axum::extract::ws::Message::Text(payload.into()))
        .await
        .map_err(|error| anyhow!("failed to send direct-connect websocket event: {error}"))
}

fn direct_connect_error_result_event(
    session_id: &str,
    error: impl Into<String>,
    duration_ms: u64,
) -> Value {
    serde_json::json!({
        "type": "result",
        "subtype": "error",
        "is_error": true,
        "duration_ms": duration_ms,
        "duration_api_ms": duration_ms,
        "num_turns": 0,
        "result": error.into(),
        "stop_reason": "error",
        "session_id": session_id,
        "total_cost_usd": 0,
        "usage": {},
        "modelUsage": {},
        "permission_denials": [],
        "fast_mode_state": "off",
        "uuid": uuid::Uuid::new_v4().to_string(),
    })
}

fn direct_connect_authorized(auth_token: &Option<String>, headers: &axum::http::HeaderMap) -> bool {
    let Some(expected) = auth_token.as_deref().filter(|value| !value.is_empty()) else {
        return true;
    };
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|value| value == expected)
}

fn direct_connect_json_error(
    status: axum::http::StatusCode,
    message: impl Into<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        status,
        axum::Json(serde_json::json!({
            "error": message.into(),
        })),
    )
        .into_response()
}

fn direct_connect_session_work_dir(
    workspace: &Path,
    requested_cwd: Option<&str>,
) -> Result<PathBuf> {
    let workspace = workspace
        .canonicalize()
        .map_err(|error| anyhow!("failed to resolve direct-connect workspace: {error}"))?;
    let path = requested_cwd
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.clone());
    let path = if path.is_absolute() {
        path
    } else {
        workspace.join(path)
    };
    let path = path
        .canonicalize()
        .map_err(|error| anyhow!("failed to resolve direct-connect session cwd: {error}"))?;
    if !path.starts_with(&workspace) {
        return Err(anyhow!(
            "direct-connect session cwd is outside workspace: {}",
            path.display()
        ));
    }
    Ok(path)
}

const DIRECT_CONNECT_APP_EVENTS_LIMIT: usize = 200;

fn direct_connect_app_conversation_events_report(
    session_id: &str,
    active_session: Option<&DirectConnectServerSession>,
) -> Result<Value> {
    direct_connect_validate_session_id(session_id)?;
    let event_values = direct_connect_read_runtime_event_values(session_id)?;
    let source_available = event_values.is_some();
    let mut events = event_values.unwrap_or_default();
    let total_events = events.len();
    let truncated = total_events > DIRECT_CONNECT_APP_EVENTS_LIMIT;
    if truncated {
        events = events.split_off(total_events - DIRECT_CONNECT_APP_EVENTS_LIMIT);
    }
    let summary = direct_connect_app_events_summary(&events);
    let view = direct_connect_app_events_view(&events);

    Ok(serde_json::json!({
        "schema": "kiana.app-server.events.v1",
        "session_id": session_id,
        "active": active_session.is_some(),
        "work_dir": active_session
            .map(|session| session.work_dir.display().to_string()),
        "live_url": format!("/sessions/{session_id}/ws"),
        "event_source": {
            "type": "sdk-session-tree",
            "available": source_available,
        },
        "limit": DIRECT_CONNECT_APP_EVENTS_LIMIT,
        "total_events": total_events,
        "count": events.len(),
        "truncated": truncated,
        "summary": summary,
        "view": view,
        "events": events,
    }))
}

fn direct_connect_app_events_view(events: &[Value]) -> Value {
    let mut messages = Vec::new();
    for event in events {
        match event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
        {
            "user_message" | "assistant_message" => {
                if let Some(message) = event.get("message") {
                    messages.extend(direct_connect_app_message_view_messages(event, message));
                }
            }
            "stream_delta" => {
                let content =
                    direct_connect_app_runtime_text(event.get("delta").unwrap_or(&Value::Null));
                if !content.trim().is_empty() {
                    messages.push(direct_connect_app_view_message(
                        event,
                        "assistant",
                        content,
                        serde_json::json!({
                            "kind": "stream_delta",
                        }),
                    ));
                }
            }
            "tool_call" => {
                let name = event
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let mut metadata = serde_json::json!({
                    "kind": "tool_call",
                    "tool_call_id": event.get("tool_call_id").cloned().unwrap_or(Value::Null),
                    "name": name,
                    "workbench": event.get("workbench").cloned().unwrap_or(Value::Null),
                    "input": event.get("input").cloned().unwrap_or(Value::Null),
                });
                messages.push(direct_connect_app_view_message(
                    event,
                    "tool",
                    format!("Tool requested: {name}"),
                    metadata.take(),
                ));
            }
            "tool_result" => {
                let is_error = event
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut metadata = serde_json::json!({
                    "kind": "tool_result",
                    "tool_call_id": event.get("tool_call_id").cloned().unwrap_or(Value::Null),
                    "name": event.get("name").cloned().unwrap_or(Value::Null),
                    "workbench": event.get("workbench").cloned().unwrap_or(Value::Null),
                    "is_error": is_error,
                });
                if let Some(changed_files) = event.get("changed_files") {
                    metadata["changed_files"] = changed_files.clone();
                }
                if let Some(error) = event.get("error") {
                    metadata["error"] = error.clone();
                }
                let mut message = direct_connect_app_view_message(
                    event,
                    "tool",
                    direct_connect_app_runtime_text(event.get("content").unwrap_or(&Value::Null)),
                    metadata,
                );
                if let Some(changed_files) = event.get("changed_files") {
                    message["changed_files"] = changed_files.clone();
                }
                messages.push(message);
            }
            "permission_request" => {
                messages.push(direct_connect_app_view_message(
                    event,
                    "system",
                    format!(
                        "Permission requested for {}.",
                        event
                            .get("tool_name")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                    ),
                    serde_json::json!({
                        "kind": "permission_request",
                        "request_id": event.get("request_id").cloned().unwrap_or(Value::Null),
                        "tool_name": event.get("tool_name").cloned().unwrap_or(Value::Null),
                        "action": event.get("action").cloned().unwrap_or(Value::Null),
                        "reason": event.get("reason").cloned().unwrap_or(Value::Null),
                        "input": event.get("input").cloned().unwrap_or(Value::Null),
                    }),
                ));
            }
            "session_event" => {
                let subtype = event
                    .get("subtype")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                messages.push(direct_connect_app_view_message(
                    event,
                    "system",
                    event
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("Session event: {subtype}")),
                    serde_json::json!({
                        "kind": "session_event",
                        "subtype": subtype,
                        "metadata": event.get("metadata").cloned().unwrap_or(Value::Null),
                    }),
                ));
            }
            "error" => {
                messages.push(direct_connect_app_view_message(
                    event,
                    "system",
                    format!(
                        "Error: {}",
                        event
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error")
                    ),
                    serde_json::json!({
                        "kind": "error",
                        "code": event.get("code").cloned().unwrap_or(Value::Null),
                        "details": event.get("details").cloned().unwrap_or(Value::Null),
                    }),
                ));
            }
            "result" => {
                if let Some(text) = event
                    .get("assistant_text")
                    .and_then(Value::as_str)
                    .filter(|text| !text.trim().is_empty())
                    .filter(|text| {
                        !messages.iter().rev().any(|message| {
                            message.get("role").and_then(Value::as_str) == Some("assistant")
                                && message.get("content").and_then(Value::as_str) == Some(*text)
                        })
                    })
                {
                    messages.push(direct_connect_app_view_message(
                        event,
                        "assistant",
                        text.to_string(),
                        serde_json::json!({
                            "kind": "result",
                            "status": event.get("status").cloned().unwrap_or(Value::Null),
                            "stop_reason": event.get("stop_reason").cloned().unwrap_or(Value::Null),
                        }),
                    ));
                }
            }
            _ => {}
        }
    }

    serde_json::json!({
        "schema": "kiana.app-server.events-view.v1",
        "message_count": messages.len(),
        "messages": messages,
    })
}

fn direct_connect_app_message_view_messages(event: &Value, message: &Value) -> Vec<Value> {
    let role = match message.get("role").and_then(Value::as_str) {
        Some("assistant") => "assistant",
        Some("tool") => "tool",
        Some("user")
            if message
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block.get("type").and_then(Value::as_str) == Some("tool_result")
                    })
                }) =>
        {
            "tool"
        }
        Some("user") => "user",
        _ => "system",
    };

    let content = message.get("content").unwrap_or(&Value::Null);
    let Some(blocks) = content.as_array() else {
        return vec![direct_connect_app_view_message(
            event,
            role,
            direct_connect_app_runtime_text(content),
            serde_json::json!({
                "kind": "message",
            }),
        )];
    };

    let mut messages = Vec::new();
    for block in blocks {
        let block_type = block.get("type").and_then(Value::as_str);
        let block_role = match block_type {
            Some("tool_use") | Some("tool_result") => "tool",
            _ => role,
        };
        let content = direct_connect_app_content_block_text(block);
        if content.trim().is_empty() {
            continue;
        }
        messages.push(direct_connect_app_view_message(
            event,
            block_role,
            content,
            serde_json::json!({
                "kind": "message_block",
                "block_type": block_type.unwrap_or("text"),
            }),
        ));
    }

    if messages.is_empty() {
        messages.push(direct_connect_app_view_message(
            event,
            role,
            direct_connect_app_runtime_text(content),
            serde_json::json!({
                "kind": "message",
            }),
        ));
    }
    messages
}

fn direct_connect_app_content_block_text(block: &Value) -> String {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => block
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Some("tool_use") => format!(
            "Tool requested: {}",
            block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ),
        Some("tool_result") => {
            direct_connect_app_runtime_text(block.get("content").unwrap_or(&Value::Null))
        }
        _ => direct_connect_app_runtime_text(block),
    }
}

fn direct_connect_app_runtime_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    if let Some(blocks) = value.as_array() {
        return blocks
            .iter()
            .map(direct_connect_app_content_block_text)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn direct_connect_app_view_message(
    event: &Value,
    role: &str,
    content: String,
    metadata: Value,
) -> Value {
    serde_json::json!({
        "event_id": event.get("event_id").cloned().unwrap_or(Value::Null),
        "turn_id": event.get("turn_id").cloned().unwrap_or(Value::Null),
        "sequence": event.get("sequence").cloned().unwrap_or(Value::Null),
        "timestamp": event.get("timestamp").cloned().unwrap_or(Value::Null),
        "source_type": event.get("type").cloned().unwrap_or(Value::Null),
        "role": role,
        "content": content,
        "metadata": metadata,
    })
}

fn direct_connect_app_events_summary(events: &[Value]) -> Value {
    let mut event_types = serde_json::Map::new();
    let mut turn_ids = HashSet::new();
    let mut tool_result_total = 0usize;
    let mut tool_result_errors = 0usize;
    let mut file_change_paths = Vec::<String>::new();
    let mut terminal = serde_json::json!({
        "present": false,
        "status": Value::Null,
        "stop_reason": Value::Null,
    });

    for event in events {
        if let Some(turn_id) = event.get("turn_id").and_then(Value::as_str) {
            turn_ids.insert(turn_id.to_string());
        }
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let count = event_types
            .get(event_type)
            .and_then(Value::as_u64)
            .unwrap_or(0)
            + 1;
        event_types.insert(event_type.to_string(), Value::from(count));

        if event_type == "tool_result" {
            tool_result_total += 1;
            if event
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                tool_result_errors += 1;
            }
            if let Some(changed_files) = event.get("changed_files").and_then(Value::as_array) {
                for changed_file in changed_files {
                    if let Some(path) = changed_file.get("path").and_then(Value::as_str) {
                        if !file_change_paths.iter().any(|existing| existing == path) {
                            file_change_paths.push(path.to_string());
                        }
                    }
                }
            }
        }

        if event_type == "result" {
            terminal = serde_json::json!({
                "present": true,
                "status": event.get("status").cloned().unwrap_or(Value::Null),
                "stop_reason": event.get("stop_reason").cloned().unwrap_or(Value::Null),
            });
        }
    }
    file_change_paths.sort();

    serde_json::json!({
        "turns": turn_ids.len(),
        "event_types": Value::Object(event_types),
        "tool_results": {
            "total": tool_result_total,
            "errors": tool_result_errors,
        },
        "file_changes": {
            "count": file_change_paths.len(),
            "paths": file_change_paths,
        },
        "terminal": terminal,
    })
}

fn direct_connect_read_runtime_event_values(session_id: &str) -> Result<Option<Vec<Value>>> {
    direct_connect_validate_session_id(session_id)?;
    let path = direct_connect_sdk_sessions_dir()
        .join(session_id)
        .join("events.jsonl");
    if !path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut events = Vec::new();
    for (line_index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let event: kiana_types::RuntimeEvent = serde_json::from_str(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                line_index + 1,
                path.display()
            )
        })?;
        if event.session_id != session_id {
            return Err(anyhow!(
                "runtime event {} belongs to another session",
                line_index + 1
            ));
        }
        events.push(serde_json::to_value(event)?);
    }
    Ok(Some(events))
}

fn direct_connect_sdk_sessions_dir() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_SDK_SESSIONS_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path).join("sdk-sessions");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".kiana").join("sdk-sessions");
    }
    PathBuf::from(".kiana").join("sdk-sessions")
}

fn direct_connect_validate_session_id(session_id: &str) -> Result<()> {
    if session_id.trim().is_empty()
        || session_id.chars().any(char::is_whitespace)
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id.contains("..")
    {
        return Err(anyhow!("session id contains invalid path characters"));
    }
    Ok(())
}

fn direct_connect_app_contract(state: &DirectConnectServerState, active_sessions: usize) -> Value {
    serde_json::json!({
        "schema": "kiana.app-server.contract.v1",
        "transport": if state.unix_socket.is_some() { "unix" } else { "http" },
        "workspace": state.workspace.display().to_string(),
        "auth": {
            "type": if state.auth_token.is_some() { "bearer" } else { "none" },
            "required": state.auth_token.is_some(),
        },
        "capabilities": direct_connect_app_capabilities(),
        "endpoints": direct_connect_app_endpoints(),
        "active_sessions": active_sessions,
    })
}

fn direct_connect_app_capabilities() -> Vec<&'static str> {
    vec![
        "conversations.read",
        "events.snapshot.read",
        "events.websocket",
        "conversation.files.read",
        "conversation.files.write",
        "settings.read",
        "prompt.history.read",
        "team.status.read",
        "tasks.read",
        "commands.read",
        "commands.run",
        "approvals.decide",
        "config.resolved.read",
        "doctor.read",
        "release.blockers.read",
        "release.local_rc_evidence.read",
        "release.product_acceptance.read",
        "release.entitlement.read",
        "release.ops.read",
        "release.platform_security.read",
        "release.source_control.read",
        "release.signature.read",
        "release.enterprise_offline_manifest.read",
        "release.proof_manifest.read",
        "release.live_provider_smoke.read",
        "release.remote_code_session_smoke.read",
        "release.distribution_review.read",
        "secrets.redacted",
        "sandbox.read",
        "permissions.status.read",
        "trust.status.read",
        "plugins.read",
        "auth.status.read",
        "license.status.read",
        "model.catalog.read",
        "model.list.read",
        "model.current.read",
        "model.current.write",
        "model.smoke.read",
        "git.status.read",
        "diff.read",
        "checkpoint.create",
        "checks.dry_run.read",
        "checks.run.read",
        "review.dry_run.read",
        "review.run.read",
        "context.index.read",
        "context.index.cache.write",
        "context.artifacts.read",
        "context.artifacts.cache.write",
        "context.artifact_ingest.write",
        "context.artifact_graph.read",
        "context.artifact_store.read",
        "context.artifact_readiness.read",
        "context.repo_map.read",
        "context.search.read",
        "context.vector_search.read",
        "context.pack.read",
    ]
}

fn direct_connect_app_endpoint(method: &str, path: &str, schema: &str) -> Value {
    serde_json::json!({
        "method": method,
        "path": path,
        "schema": schema,
    })
}

fn direct_connect_app_endpoint_with_query(
    method: &str,
    path: &str,
    schema: &str,
    query: Value,
) -> Value {
    let mut endpoint = direct_connect_app_endpoint(method, path, schema);
    if let Some(object) = endpoint.as_object_mut() {
        object.insert("query".to_string(), query);
    }
    endpoint
}

fn direct_connect_app_endpoints() -> Vec<Value> {
    vec![
        direct_connect_app_endpoint("GET", "/app", "kiana.app-server.contract.v1"),
        direct_connect_app_endpoint("GET", "/app/conversations", "kiana.app-server.conversations.v1"),
        direct_connect_app_endpoint("GET", "/app/conversations/{session_id}/events", "kiana.app-server.events.v1"),
        direct_connect_app_endpoint("GET", "/app/conversations/{session_id}/files", "kiana.app-server.conversation-files.v1"),
        direct_connect_app_endpoint("POST", "/app/conversations/{session_id}/files", "kiana.app-server.conversation-files.v1"),
        direct_connect_app_endpoint("GET", "/app/settings", "kiana.app-server.settings.v1"),
        direct_connect_app_endpoint("GET", "/app/prompt-history", "kiana.app-server.prompt-history.v1"),
        direct_connect_app_endpoint("GET", "/app/team/status", "kiana.app-server.team-status.v1"),
        direct_connect_app_endpoint("GET", "/app/team/plan", "kiana.team-plan.v1"),
        direct_connect_app_endpoint("GET", "/app/commands", "kiana.app-server.commands.v1"),
        direct_connect_app_endpoint("POST", "/app/commands/run", "kiana.app-server.command-run.v1"),
        direct_connect_app_endpoint("POST", "/app/approvals/decision", "kiana.app-server.approval-decision.v1"),
        direct_connect_app_endpoint("GET", "/app/config/resolved", "kiana.app-server.config-resolved.v1"),
        direct_connect_app_endpoint("GET", "/app/doctor", "kiana.app-server.doctor.v1"),
        direct_connect_app_endpoint("GET", "/app/release/blockers", "kiana.commercial-release-blockers.v1"),
        direct_connect_app_endpoint("GET", "/app/release/local-rc-evidence", "kiana.local-rc-evidence.v1"),
        direct_connect_app_endpoint("GET", "/app/release/product-acceptance", "kiana.product-acceptance.v1"),
        direct_connect_app_endpoint("GET", "/app/release/entitlement", "kiana.entitlement-proof.v1"),
        direct_connect_app_endpoint("GET", "/app/release/ops", "kiana.release-ops.v1"),
        direct_connect_app_endpoint("GET", "/app/release/platform-security", "kiana.platform-security-proof.v1"),
        direct_connect_app_endpoint("GET", "/app/release/source-control", "kiana.source-control-proof.v1"),
        direct_connect_app_endpoint("GET", "/app/release/signature", "kiana.release-signature.v1"),
        direct_connect_app_endpoint("GET", "/app/release/enterprise-offline-manifest", "kiana.enterprise.offline-manifest.v1"),
        direct_connect_app_endpoint("GET", "/app/release/proof-manifest", "kiana.commercial-proof-manifest.v1"),
        direct_connect_app_endpoint("GET", "/app/release/live-provider-smoke", "kiana.app-server.live-provider-smoke.v1"),
        direct_connect_app_endpoint("GET", "/app/release/remote-code-session-smoke", "kiana.remote-code-session-smoke.v1"),
        direct_connect_app_endpoint("GET", "/app/release/distribution", "kiana.app-server.distribution-review.v1"),
        direct_connect_app_endpoint("GET", "/app/secrets", "kiana.app-server.secrets.v1"),
        direct_connect_app_endpoint("GET", "/app/sandbox", "kiana.app-server.sandbox.v1"),
        direct_connect_app_endpoint("GET", "/app/permissions/status", "kiana.app-server.permissions-status.v1"),
        direct_connect_app_endpoint("GET", "/app/trust/status", "kiana.app-server.trust-status.v1"),
        direct_connect_app_endpoint("GET", "/app/plugins", "kiana.app-server.plugins.v1"),
        direct_connect_app_endpoint("GET", "/app/auth/status", "kiana.auth-status.v1"),
        direct_connect_app_endpoint("GET", "/app/license/status", "kiana.license-status.v1"),
        direct_connect_app_endpoint("GET", "/app/models/catalog", "kiana.model-catalog.v1"),
        direct_connect_app_endpoint("GET", "/app/models/list", "kiana.model-list.v1"),
        direct_connect_app_endpoint("GET", "/app/models/current", "kiana.app-server.model-current.v1"),
        direct_connect_app_endpoint("POST", "/app/models/current", "kiana.app-server.model-current.v1"),
        direct_connect_app_endpoint("GET", "/app/models/smoke", "kiana.model-smoke.v1"),
        direct_connect_app_endpoint("GET", "/app/git/status", "kiana.app-server.git-status.v1"),
        direct_connect_app_endpoint("GET", "/app/diff", "kiana.diff.v1"),
        direct_connect_app_endpoint("POST", "/app/checkpoints", "kiana.checkpoint.v1"),
        direct_connect_app_endpoint("GET", "/app/checks/dry-run", "kiana.checks.dry_run.v1"),
        direct_connect_app_endpoint("GET", "/app/checks", "kiana.checks.run.v1"),
        direct_connect_app_endpoint("GET", "/app/review/dry-run", "kiana.review.dry_run.v1"),
        direct_connect_app_endpoint("GET", "/app/review", "kiana.review.run.v1"),
        direct_connect_app_endpoint_with_query(
            "GET",
            "/app/context/index",
            "kiana.context-index.v1",
            serde_json::json!({
                "cache": "optional boolean; when true writes .kiana/context-index.json in the active workspace"
            }),
        ),
        direct_connect_app_endpoint_with_query(
            "GET",
            "/app/context/artifacts",
            "kiana.context-artifacts.v1",
            Value::String("optional cache boolean writes .kiana/context-artifacts.json; optional max_bytes_per_file caps indexed file bytes".to_string()),
        ),
        direct_connect_app_endpoint_with_query(
            "POST",
            "/app/context/ingest",
            "kiana.context-artifact-ingest.v1",
            serde_json::json!({
                "body": {
                    "source": "required workspace-relative source directory to ingest",
                    "store": "optional workspace-relative store directory; defaults to .kiana/context-ingest",
                    "max_bytes_per_file": "optional positive byte cap for source files"
                }
            }),
        ),
        direct_connect_app_endpoint_with_query(
            "GET",
            "/app/context/artifact-graph",
            "kiana.context-artifact-dependency-graph.v1",
            Value::String("optional max_bytes_per_file caps indexed file bytes".to_string()),
        ),
        direct_connect_app_endpoint_with_query(
            "GET",
            "/app/context/artifact-store",
            "kiana.context-artifact-store.v1",
            Value::String("optional max_bytes_per_file caps indexed file bytes".to_string()),
        ),
        direct_connect_app_endpoint_with_query(
            "GET",
            "/app/context/artifact-readiness",
            "kiana.context-artifact-readiness.v1",
            Value::String("optional max_bytes_per_file caps indexed file bytes".to_string()),
        ),
        direct_connect_app_endpoint("GET", "/app/context/repo-map", "kiana.repo-map.v1"),
        direct_connect_app_endpoint("GET", "/app/context/search", "kiana.context-search.v1"),
        direct_connect_app_endpoint(
            "GET",
            "/app/context/vector-search",
            "kiana.context-vector-search.v1",
        ),
        direct_connect_app_endpoint("GET", "/app/context/pack", "kiana.context-pack.v1"),
        direct_connect_app_endpoint("POST", "/sessions", "kiana.direct-connect.session-create.v1"),
        direct_connect_app_endpoint("GET", "/sessions/{session_id}/ws", "kiana.direct-connect.events.websocket.v1"),
    ]
}

fn direct_connect_git_status_report(workspace: &Path) -> Value {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(workspace)
        .arg("status")
        .arg("--porcelain=v1")
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let files = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(|line| Value::String(line.to_string()))
                .collect::<Vec<_>>();
            serde_json::json!({
                "schema": "kiana.app-server.git-status.v1",
                "workspace": workspace.display().to_string(),
                "repository": {
                    "detected": true,
                    "source": "git status --porcelain=v1"
                },
                "dirty": !files.is_empty(),
                "files": files,
            })
        }
        Ok(output) => serde_json::json!({
            "schema": "kiana.app-server.git-status.v1",
            "workspace": workspace.display().to_string(),
            "repository": {
                "detected": false,
                "source": "git status --porcelain=v1"
            },
            "dirty": null,
            "files": [],
            "error": String::from_utf8_lossy(&output.stderr).trim(),
        }),
        Err(error) => serde_json::json!({
            "schema": "kiana.app-server.git-status.v1",
            "workspace": workspace.display().to_string(),
            "repository": {
                "detected": false,
                "source": "git status --porcelain=v1"
            },
            "dirty": null,
            "files": [],
            "error": error.to_string(),
        }),
    }
}

fn direct_connect_server_http_url(addr: SocketAddr) -> String {
    format!("http://{}", direct_connect_display_addr(addr))
}

fn direct_connect_server_cc_url(addr: SocketAddr, token: &str) -> String {
    format!("cc://{}?token={}", direct_connect_display_addr(addr), token)
}

#[cfg(unix)]
fn direct_connect_server_cc_unix_url(socket_path: &Path, token: &str) -> String {
    format!("cc+unix://{}?token={}", socket_path.display(), token)
}

fn direct_connect_server_ws_url_for_state(
    state: &DirectConnectServerState,
    session_id: &str,
) -> String {
    if let Some(socket_path) = &state.unix_socket {
        return format!("unix:{}:/sessions/{}/ws", socket_path.display(), session_id);
    }
    direct_connect_server_ws_url(state.public_addr, session_id)
}

fn direct_connect_server_ws_url(addr: SocketAddr, session_id: &str) -> String {
    format!(
        "ws://{}/sessions/{}/ws",
        direct_connect_display_addr(addr),
        session_id
    )
}

fn direct_connect_display_addr(addr: SocketAddr) -> String {
    let ip = if addr.ip().is_unspecified() {
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    } else {
        addr.ip()
    };
    SocketAddr::new(ip, addr.port()).to_string()
}

fn direct_connect_open_usage() -> &'static str {
    "Usage: kiana open <cc-url> -p <prompt> [--output-format text|json|stream-json]"
}

fn print_direct_connect_open_help() {
    println!("{}", direct_connect_open_usage());
    println!();
    println!("Connect to a direct-connect server in headless direct-connect mode.");
    println!();
    println!("Options:");
    println!("  -p, --print [prompt]       Run a headless prompt against a direct-connect server");
    println!("  --output-format <format>   text, json, or stream-json");
    println!(
        "  --dangerously-skip-permissions  Request skipped permissions when creating the session"
    );
}

fn direct_connect_server_usage() -> &'static str {
    "Usage: kiana server [--host <host>] [--port <port>] [--auth-token <token>] [--unix <path>] [--workspace <dir>] [--idle-timeout <ms>] [--max-sessions <n>]"
}

fn print_direct_connect_server_help() {
    println!("{}", direct_connect_server_usage());
    println!();
    println!("Start a local direct-connect server for kiana open <cc-url>.");
    println!();
    println!("Options:");
    println!("  --host <host>          Bind address (default: 0.0.0.0)");
    println!("  --port <port>          HTTP port (default: 0)");
    println!("  --auth-token <token>   Bearer token for HTTP and websocket auth");
    println!("  --unix <path>          Bind a Unix domain socket instead of TCP (Unix only)");
    println!("  --workspace <dir>      Default working directory for sessions");
    println!("  --idle-timeout <ms>    Websocket idle timeout in milliseconds (default: 600000, 0 disables)");
    println!("  --max-sessions <n>     Maximum concurrent sessions (default: 32, 0 unlimited)");
}

async fn dump_system_prompt(_args: &[String]) -> Result<()> {
    println!(
        "You are Kiana Code, an interactive coding assistant. Help the user inspect, edit, and verify code in the current working directory."
    );
    Ok(())
}

async fn run_daemon_worker(kind: Option<&str>, args: &[String]) -> Result<()> {
    crate::bg::run_worker(kind, args).await
}

async fn mcp_server_main(args: &[String]) -> Result<()> {
    if is_help_at(args, 1) {
        print_mcp_server_help();
        return Ok(());
    }

    let debug = args.iter().any(|arg| arg == "--debug");
    let verbose = args.iter().any(|arg| arg == "--verbose" || arg == "-v");
    let cwd = std::env::current_dir()?;
    let cwd = cwd.to_string_lossy().to_string();
    if matches!(
        args.first().map(String::as_str),
        Some("mcp-server-http" | "mcp-server-sse" | "mcp-server-ws")
    ) || args.iter().any(|arg| arg == "--http")
        || args.iter().any(|arg| arg == "--sse")
        || args.iter().any(|arg| arg == "--ws")
    {
        let host = option_or_env(args, "--host", "KIANA_MCP_HTTP_HOST")
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let port = option_or_env(args, "--port", "KIANA_MCP_HTTP_PORT")
            .unwrap_or_else(|| "8765".to_string())
            .parse::<u16>()?;
        let addr: SocketAddr = format!("{}:{}", host, port).parse()?;
        println!(
            "Starting Kiana MCP HTTP/SSE/WS server at http://{}/mcp, http://{}/sse, and ws://{}/ws",
            addr, addr, addr
        );
        return crate::mcp::start_mcp_http_server(&cwd, addr, debug, verbose).await;
    }
    crate::mcp::start_mcp_server(&cwd, debug, verbose).await
}

async fn mcp_serve_main(args: &[String]) -> Result<()> {
    if is_help_at(args, 2) {
        print_mcp_serve_help();
        return Ok(());
    }

    let debug = args.iter().any(|arg| arg == "--debug");
    let verbose = args.iter().any(|arg| arg == "--verbose" || arg == "-v");
    let cwd = std::env::current_dir()?;
    let cwd = cwd.to_string_lossy().to_string();
    crate::mcp::start_mcp_server(&cwd, debug, verbose).await
}

fn print_mcp_server_help() {
    println!("Usage: kiana mcp-server [--debug] [--verbose]");
    println!("       kiana mcp-server-http [--host <host>] [--port <port>] [--debug] [--verbose]");
    println!("       kiana mcp-server-sse [--host <host>] [--port <port>] [--debug] [--verbose]");
    println!("       kiana mcp-server-ws [--host <host>] [--port <port>] [--debug] [--verbose]");
    println!();
    println!("Commands:");
    println!("  mcp-server          Run the Kiana MCP server over stdio");
    println!("  mcp-server-http     Run HTTP, SSE, and WebSocket MCP endpoints");
    println!("  mcp-server-sse      Alias for the HTTP/SSE/WS server");
    println!("  mcp-server-ws       Alias for the HTTP/SSE/WS server");
    println!();
    println!("Options:");
    println!("  --host <host>       HTTP/SSE/WS bind host (default: 127.0.0.1)");
    println!("  --port <port>       HTTP/SSE/WS bind port (default: 8765)");
    println!("  --debug             Enable MCP debug logging");
    println!("  --verbose, -v       Enable verbose MCP logging");
}

fn print_mcp_serve_help() {
    println!("Usage: kiana mcp serve [--debug] [--verbose]");
    println!();
    println!("Commands:");
    println!("  mcp serve           Run the Kiana MCP server over stdio");
    println!();
    println!("Options:");
    println!("  --debug             Enable MCP debug logging");
    println!("  --verbose, -v       Enable verbose MCP logging");
}

fn print_computer_mcp_help() {
    println!("Usage: kiana computer-mcp");
    println!("       kiana computer-use-mcp");
    println!();
    println!("Commands:");
    println!("  computer-mcp        Run the computer-use MCP server over stdio");
    println!("  computer-use-mcp    Alias for computer-mcp");
    println!();
    println!("Transport:");
    println!("  stdio               The server reads JSON-RPC from stdin and writes to stdout");
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteSessionCliConfig {
    session_id: Option<String>,
    org_uuid: Option<String>,
    api_base_url: String,
    token_configured: bool,
    token_status: RemoteSessionTokenStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteSessionTokenStatus {
    Configured(&'static str),
    Missing,
    AnthropicApiKeyMisuse,
}

impl RemoteSessionTokenStatus {
    fn is_configured(self) -> bool {
        matches!(self, RemoteSessionTokenStatus::Configured(_))
    }

    fn label(self) -> String {
        match self {
            RemoteSessionTokenStatus::Configured(source) => format!("ready ({source})"),
            RemoteSessionTokenStatus::Missing => "missing".to_string(),
            RemoteSessionTokenStatus::AnthropicApiKeyMisuse => {
                "invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)".to_string()
            }
        }
    }

    fn fix(self) -> Option<&'static str> {
        match self {
            RemoteSessionTokenStatus::Configured(_) => None,
            RemoteSessionTokenStatus::Missing | RemoteSessionTokenStatus::AnthropicApiKeyMisuse => {
                Some("set KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN to the remote bearer token")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteSessionPermissionMode {
    Deny,
    Allow,
    Manual,
}

async fn remote_session_main(args: &[String]) -> Result<()> {
    let command = args.get(1).map(String::as_str).unwrap_or("status");
    if remote_session_command_uses_top_level_help(command) && is_help_at(args, 2) {
        print_remote_session_help();
        return Ok(());
    }

    match command {
        "status" => {
            if args.iter().skip(2).any(|arg| is_help_arg(arg)) {
                print_remote_session_help();
                return Ok(());
            }
            let config = build_remote_session_cli_config(args)?;
            print_remote_session_status(&config);
            Ok(())
        }
        "url" | "ws-url" | "websocket-url" => {
            let config = build_remote_session_cli_config(args)?;
            let session_id = config.session_id.as_deref().ok_or_else(|| {
                anyhow!("remote-session url requires --session-id or KIANA_REMOTE_SESSION_ID")
            })?;
            let org_uuid = config.org_uuid.as_deref().ok_or_else(|| {
                anyhow!("remote-session url requires --org-uuid or KIANA_REMOTE_ORG_UUID")
            })?;
            println!(
                "{}",
                kiana_remote::sessions_websocket_url(&config.api_base_url, session_id, org_uuid,)?
            );
            Ok(())
        }
        "list" | "ls" => remote_session_list(args).await,
        "show" | "get" | "inspect" => remote_session_show(args).await,
        "rename" | "title" => remote_session_rename(args).await,
        "create" | "new" | "start" => remote_session_create(args).await,
        "archive" | "delete" | "close" => remote_session_archive(args).await,
        "env" | "environment" | "environments" => remote_session_environments(args).await,
        "code-session" | "code-sessions" | "code" | "ccr-v2" => {
            remote_session_code_session(args).await
        }
        "listen" | "connect" | "tail" => remote_session_listen(args).await,
        "send" | "message" => remote_session_send(args).await,
        "help" | "--help" | "-h" => {
            print_remote_session_help();
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown remote-session command '{}'\n\nUsage: kiana remote-session [status|list|show|rename|create|archive|environments|code-session|url|listen|send]",
            command
        )),
    }
}

fn build_remote_session_cli_config(args: &[String]) -> Result<RemoteSessionCliConfig> {
    let token_status = remote_session_token_status();
    Ok(RemoteSessionCliConfig {
        session_id: remote_session_option_or_env(
            args,
            "--session-id",
            &["KIANA_REMOTE_SESSION_ID", "CLAUDE_REMOTE_SESSION_ID"],
        ),
        org_uuid: remote_session_option_or_env(
            args,
            "--org-uuid",
            &[
                "KIANA_REMOTE_ORG_UUID",
                "CLAUDE_ORG_UUID",
                "ANTHROPIC_ORGANIZATION_ID",
            ],
        ),
        api_base_url: remote_session_option_or_env(
            args,
            "--api-base-url",
            &["KIANA_REMOTE_API_BASE_URL", "ANTHROPIC_API_BASE_URL"],
        )
        .unwrap_or_else(|| kiana_remote::DEFAULT_API_BASE_URL.to_string()),
        token_configured: token_status.is_configured(),
        token_status,
    })
}

fn remote_session_option_or_env(args: &[String], name: &str, envs: &[&str]) -> Option<String> {
    let inline_prefix = format!("{}=", name);
    args.iter()
        .find_map(|arg| arg.strip_prefix(&inline_prefix).map(str::to_string))
        .or_else(|| {
            args.windows(2).find_map(|window| {
                if window[0] == name {
                    Some(window[1].clone())
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            envs.iter().find_map(|env| {
                std::env::var(env)
                    .ok()
                    .map(|value| value.trim().to_string())
            })
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone)]
struct RemoteSessionAccessToken {
    value: String,
    source: &'static str,
}

fn oauth_proactive_refresh_skew() -> std::time::Duration {
    kiana_services::oauth::DEFAULT_OAUTH_EXPIRY_SKEW
}

fn remote_session_access_token_details() -> Option<RemoteSessionAccessToken> {
    [
        "KIANA_REMOTE_ACCESS_TOKEN",
        "CLAUDE_ACCESS_TOKEN",
        "ANTHROPIC_AUTH_TOKEN",
    ]
    .iter()
    .find_map(|env| {
        std::env::var(env).ok().and_then(|value| {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(RemoteSessionAccessToken { value, source: env })
            }
        })
    })
    .or_else(|| {
        kiana_services::oauth::load_oauth_tokens()
            .ok()
            .flatten()
            .and_then(|tokens| {
                let value = tokens.access_token.trim().to_string();
                if value.is_empty() {
                    None
                } else {
                    Some(RemoteSessionAccessToken {
                        value,
                        source: "oauth_file",
                    })
                }
            })
    })
}

async fn remote_session_access_token_details_refreshing_if_expiring(
) -> Result<Option<RemoteSessionAccessToken>> {
    if let Some(access_token) = [
        "KIANA_REMOTE_ACCESS_TOKEN",
        "CLAUDE_ACCESS_TOKEN",
        "ANTHROPIC_AUTH_TOKEN",
    ]
    .iter()
    .find_map(|env| {
        std::env::var(env).ok().and_then(|value| {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(RemoteSessionAccessToken { value, source: env })
            }
        })
    }) {
        return Ok(Some(access_token));
    }

    let Some(tokens) = kiana_services::oauth::load_oauth_tokens_refreshing_if_expiring(
        oauth_proactive_refresh_skew(),
    )
    .await?
    else {
        return Ok(None);
    };
    let value = tokens.access_token.trim().to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(RemoteSessionAccessToken {
            value,
            source: "oauth_file",
        }))
    }
}

async fn remote_session_live_access_token(command: &str) -> Result<RemoteSessionAccessToken> {
    let access_token = remote_session_access_token_details_refreshing_if_expiring()
        .await?
        .ok_or_else(|| {
        anyhow!(
            "remote-session {command} requires KIANA_REMOTE_ACCESS_TOKEN, CLAUDE_ACCESS_TOKEN, or ANTHROPIC_AUTH_TOKEN"
        )
    })?;
    if access_token.source == "ANTHROPIC_AUTH_TOKEN"
        && remote_session_token_looks_like_anthropic_api_key(&access_token.value)
    {
        return Err(remote_session_api_key_misuse_error(None));
    }
    Ok(access_token)
}

fn remote_session_token_status() -> RemoteSessionTokenStatus {
    let Some(access_token) = remote_session_access_token_details() else {
        return RemoteSessionTokenStatus::Missing;
    };
    if access_token.source == "ANTHROPIC_AUTH_TOKEN"
        && remote_session_token_looks_like_anthropic_api_key(&access_token.value)
    {
        return RemoteSessionTokenStatus::AnthropicApiKeyMisuse;
    }
    RemoteSessionTokenStatus::Configured(access_token.source)
}

async fn refresh_remote_session_access_token(stale_access_token: &str) -> Result<Option<String>> {
    if let Some(command_line) = std::env::var("KIANA_REMOTE_REFRESH_COMMAND")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        let mut command = if cfg!(windows) {
            let mut command = tokio::process::Command::new("cmd");
            command.args(["/C", &command_line]);
            command
        } else {
            let mut command = tokio::process::Command::new("sh");
            command.arg("-lc").arg(&command_line);
            command
        };
        let output = command
            .env("KIANA_REMOTE_STALE_ACCESS_TOKEN", stale_access_token)
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!(
                "remote refresh command failed with status {}{}",
                output.status,
                if stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", stderr.trim())
                }
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let token = parse_bridge_refresh_token(&stdout)
            .ok_or_else(|| anyhow!("remote refresh command did not output an access token"))?;
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", &token);
        return Ok(Some(token));
    }

    if let Some(tokens) = kiana_services::oauth::refresh_stored_oauth_tokens().await? {
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", &tokens.access_token);
        Ok(Some(tokens.access_token))
    } else {
        Ok(None)
    }
}

async fn remote_session_api_credentials(
    args: &[String],
    command: &str,
) -> Result<(RemoteSessionCliConfig, String, String)> {
    let config = build_remote_session_cli_config(args)?;
    let org_uuid = config.org_uuid.clone().ok_or_else(|| {
        anyhow!("remote-session {command} requires --org-uuid or KIANA_REMOTE_ORG_UUID")
    })?;
    let access_token = remote_session_live_access_token(command).await?;
    Ok((config, org_uuid, access_token.value))
}

async fn remote_session_code_session_credentials(
    args: &[String],
    command: &str,
) -> Result<(RemoteSessionCliConfig, RemoteSessionAccessToken)> {
    let config = build_remote_session_cli_config(args)?;
    let access_token = remote_session_live_access_token(command).await?;
    Ok((config, access_token))
}

fn remote_session_code_session_auth_error(
    error: kiana_remote::CodeSessionApiError,
    access_token: &RemoteSessionAccessToken,
) -> anyhow::Error {
    let message = error.to_string();
    if access_token.source == "ANTHROPIC_AUTH_TOKEN"
        && remote_session_token_looks_like_anthropic_api_key(&access_token.value)
        && remote_session_error_looks_like_auth_failure(&message)
    {
        remote_session_api_key_misuse_error(Some(&message))
    } else {
        anyhow!(error)
    }
}

async fn remote_session_create_code_session_with_auth_retry(
    config: &RemoteSessionCliConfig,
    access_token: &mut RemoteSessionAccessToken,
    title: &str,
    tags: &[String],
) -> Result<String> {
    match kiana_remote::create_code_session(&config.api_base_url, &access_token.value, title, tags)
        .await
    {
        Ok(session_id) => Ok(session_id),
        Err(error) if code_session_error_should_refresh(&error) => {
            if remote_session_refresh_access_token_for_retry(access_token).await? {
                kiana_remote::create_code_session(
                    &config.api_base_url,
                    &access_token.value,
                    title,
                    tags,
                )
                .await
                .map_err(|error| remote_session_code_session_auth_error(error, access_token))
            } else {
                Err(remote_session_code_session_auth_error(error, access_token))
            }
        }
        Err(error) => Err(remote_session_code_session_auth_error(error, access_token)),
    }
}

async fn remote_session_fetch_credentials_with_auth_retry(
    config: &RemoteSessionCliConfig,
    access_token: &mut RemoteSessionAccessToken,
    session_id: &str,
    trusted_device_token: Option<&str>,
) -> Result<kiana_remote::RemoteCredentials> {
    match kiana_remote::fetch_remote_credentials(
        &config.api_base_url,
        session_id,
        &access_token.value,
        trusted_device_token,
    )
    .await
    {
        Ok(credentials) => Ok(credentials),
        Err(error) if code_session_error_should_refresh(&error) => {
            if remote_session_refresh_access_token_for_retry(access_token).await? {
                kiana_remote::fetch_remote_credentials(
                    &config.api_base_url,
                    session_id,
                    &access_token.value,
                    trusted_device_token,
                )
                .await
                .map_err(|error| remote_session_code_session_auth_error(error, access_token))
            } else {
                Err(remote_session_code_session_auth_error(error, access_token))
            }
        }
        Err(error) => Err(remote_session_code_session_auth_error(error, access_token)),
    }
}

async fn remote_session_refresh_access_token_for_retry(
    access_token: &mut RemoteSessionAccessToken,
) -> Result<bool> {
    let Some(fresh_token) = refresh_remote_session_access_token(&access_token.value).await? else {
        return Ok(false);
    };
    access_token.value = fresh_token;
    access_token.source = "refreshed";
    Ok(true)
}

fn code_session_error_should_refresh(error: &kiana_remote::CodeSessionApiError) -> bool {
    error.is_auth_failure_status()
        || remote_session_error_looks_like_auth_failure(&error.to_string())
}

fn remote_session_api_key_misuse_error(prefix: Option<&str>) -> anyhow::Error {
    let hint = "ANTHROPIC_AUTH_TOKEN looks like an Anthropic API key, but remote-session live calls need a Claude/remote access token. Set KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN to the remote bearer token, or unset ANTHROPIC_AUTH_TOKEN before running live smoke.";
    match prefix {
        Some(prefix) if !prefix.trim().is_empty() => anyhow!("{prefix}\n\n{hint}"),
        _ => anyhow!("{hint}"),
    }
}

fn remote_session_token_looks_like_anthropic_api_key(token: &str) -> bool {
    token.trim().starts_with("sk-")
}

fn remote_session_error_looks_like_auth_failure(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("authentication")
        || message.contains("unauthorized")
        || message.contains("401")
        || message.contains("invalid or missing api key")
}

fn remote_session_session_id_from_args(
    args: &[String],
    config: &RemoteSessionCliConfig,
) -> Option<String> {
    config
        .session_id
        .clone()
        .or_else(|| remote_session_positional_args(args).into_iter().next())
}

#[derive(Debug)]
enum RemoteSessionCliEvent {
    Message,
    PermissionRequest {
        request_id: String,
        request: kiana_remote::SDKControlPermissionRequest,
    },
    Closed,
    Error(String),
}

struct RemoteSessionCliCallbacks {
    events: tokio::sync::mpsc::UnboundedSender<RemoteSessionCliEvent>,
    access_token: Arc<StdMutex<String>>,
}

#[async_trait::async_trait]
impl kiana_remote::RemoteSessionCallbacks for RemoteSessionCliCallbacks {
    async fn on_message(&self, message: kiana_remote::SDKMessage) {
        match serde_json::to_string(&message) {
            Ok(line) => println!("{}", line),
            Err(error) => eprintln!("remote session message serialization error: {}", error),
        }
        let _ = self.events.send(RemoteSessionCliEvent::Message);
    }

    async fn on_permission_request(
        &self,
        request: kiana_remote::SDKControlPermissionRequest,
        request_id: String,
    ) {
        let event = serde_json::json!({
            "type": "control_request",
            "request_id": request_id.clone(),
            "request": request.clone()
        });
        println!("{}", event);
        let _ = self.events.send(RemoteSessionCliEvent::PermissionRequest {
            request_id,
            request,
        });
    }

    async fn on_permission_cancelled(&self, request_id: String, tool_use_id: Option<String>) {
        let event = serde_json::json!({
            "type": "control_cancel_request",
            "request_id": request_id,
            "tool_use_id": tool_use_id
        });
        println!("{}", event);
    }

    async fn refresh_after_unauthorized(&self, stale_access_token: String) -> bool {
        match refresh_remote_session_access_token(&stale_access_token).await {
            Ok(Some(token)) => {
                *self.access_token.lock().unwrap() = token;
                true
            }
            Ok(None) => false,
            Err(error) => {
                eprintln!("Remote session token refresh failed: {}", error);
                false
            }
        }
    }

    async fn on_disconnected(&self) {
        eprintln!("Remote session disconnected.");
        let _ = self.events.send(RemoteSessionCliEvent::Closed);
    }

    async fn on_error(&self, error: kiana_remote::WebSocketError) {
        eprintln!("Remote session error: {}", error);
        let _ = self
            .events
            .send(RemoteSessionCliEvent::Error(error.to_string()));
    }

    async fn on_connected(&self) {
        eprintln!("Remote session connected.");
    }
}

async fn remote_session_list(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) = remote_session_api_credentials(args, "list").await?;
    let sessions = kiana_remote::fetch_code_sessions_from_sessions_api(
        &config.api_base_url,
        &org_uuid,
        &access_token,
    )
    .await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
        return Ok(());
    }

    if sessions.is_empty() {
        println!("No remote sessions.");
        return Ok(());
    }

    for session in sessions {
        let repo = session
            .repo
            .as_ref()
            .map(|repo| format!("{}/{}", repo.owner.login, repo.name))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{}\t{}\t{}\t{}\t{}",
            session.id, session.status, session.updated_at, repo, session.title
        );
    }

    Ok(())
}

async fn remote_session_show(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) = remote_session_api_credentials(args, "show").await?;
    let session_id = remote_session_session_id_from_args(args, &config).ok_or_else(|| {
        anyhow!("remote-session show requires --session-id, KIANA_REMOTE_SESSION_ID, or a session ID argument")
    })?;
    let session =
        kiana_remote::fetch_session(&config.api_base_url, &session_id, &org_uuid, &access_token)
            .await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&session)?);
        return Ok(());
    }

    println!("id: {}", session.id);
    println!("title: {}", session.title.as_deref().unwrap_or("Untitled"));
    println!("status: {}", session.session_status);
    println!(
        "environment_id: {}",
        session.environment_id.as_deref().unwrap_or("-")
    );
    println!(
        "created_at: {}",
        session.created_at.as_deref().unwrap_or("-")
    );
    println!(
        "updated_at: {}",
        session.updated_at.as_deref().unwrap_or("-")
    );
    println!(
        "cwd: {}",
        session.session_context.cwd.as_deref().unwrap_or("-")
    );
    println!(
        "branch: {}",
        kiana_remote::get_branch_from_session(&session)
            .as_deref()
            .unwrap_or("-")
    );

    Ok(())
}

async fn remote_session_rename(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) = remote_session_api_credentials(args, "rename").await?;
    let positionals = remote_session_positional_args(args);
    let session_id = config
        .session_id
        .clone()
        .or_else(|| positionals.first().cloned())
        .ok_or_else(|| {
            anyhow!("remote-session rename requires --session-id, KIANA_REMOTE_SESSION_ID, or a session ID argument")
        })?;
    let title = remote_session_option_or_env(args, "--title", &[])
        .or_else(|| {
            let title_parts = if config.session_id.is_some() {
                positionals
            } else {
                positionals.into_iter().skip(1).collect()
            };
            remote_session_join_positionals(title_parts)
        })
        .ok_or_else(|| {
            anyhow!("remote-session rename requires --title <text> or a title argument")
        })?;

    let updated = kiana_remote::update_session_title(
        &config.api_base_url,
        &session_id,
        &org_uuid,
        &access_token,
        &title,
    )
    .await;

    if !updated {
        return Err(anyhow!("remote-session rename failed"));
    }

    println!("Remote session title updated.");
    Ok(())
}

async fn remote_session_create(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) = remote_session_api_credentials(args, "create").await?;
    let environment_id = remote_session_create_environment_id(args).ok_or_else(|| {
        anyhow!("remote-session create requires --environment-id or KIANA_REMOTE_ENVIRONMENT_ID")
    })?;
    let permission_mode = remote_session_create_permission_mode(args)?;
    let mut session_context = remote_session_create_context(args)?;
    if session_context.seed_bundle_file_id.is_none()
        && remote_session_create_should_seed_bundle(args)
    {
        let bundle = kiana_remote::create_and_upload_git_bundle(
            &kiana_remote::FilesApiConfig {
                oauth_token: access_token.clone(),
                base_url: config.api_base_url.clone(),
                session_id: config
                    .session_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            },
            kiana_remote::GitBundleOptions {
                cwd: remote_session_seed_bundle_dir(args).map(PathBuf::from),
                max_bytes: remote_session_seed_bundle_max_bytes(args)?,
            },
        )
        .await?;
        match bundle {
            kiana_remote::BundleUploadResult::Success { file_id, .. } => {
                session_context.seed_bundle_file_id = Some(file_id);
            }
            kiana_remote::BundleUploadResult::Failure { error, fail_reason } => {
                let reason = fail_reason
                    .map(|reason| format!(" ({})", reason.as_str()))
                    .unwrap_or_default();
                return Err(anyhow!(
                    "remote-session create seed bundle failed{reason}: {error}"
                ));
            }
        }
    }
    let session = kiana_remote::create_remote_session(
        &config.api_base_url,
        &org_uuid,
        &access_token,
        kiana_remote::CreateRemoteSessionOptions {
            title: remote_session_option_or_env(args, "--title", &[]),
            environment_id,
            initial_message: remote_session_create_message_from_args(args)
                .map(kiana_remote::RemoteMessageContent::Text),
            permission_mode,
            permission_request_id: remote_session_option_or_env(args, "--request-id", &[]),
            event_uuid: remote_session_option_or_env(args, "--event-uuid", &[])
                .or_else(|| remote_session_option_or_env(args, "--uuid", &[])),
            session_context,
            source: remote_session_option_or_env(
                args,
                "--source",
                &["KIANA_REMOTE_SESSION_SOURCE"],
            )
            .or_else(|| Some("remote-control".to_string())),
        },
    )
    .await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&session)?);
    } else {
        println!("{}", session.id);
    }

    Ok(())
}

async fn remote_session_archive(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) = remote_session_api_credentials(args, "archive").await?;
    let session_id = remote_session_session_id_from_args(args, &config).ok_or_else(|| {
        anyhow!("remote-session archive requires --session-id, KIANA_REMOTE_SESSION_ID, or a session ID argument")
    })?;

    let archived = kiana_remote::archive_remote_session(
        &config.api_base_url,
        &session_id,
        &org_uuid,
        &access_token,
    )
    .await;
    if !archived {
        return Err(anyhow!("remote-session archive failed"));
    }

    println!("Remote session archived.");
    Ok(())
}

async fn remote_session_code_session(args: &[String]) -> Result<()> {
    let subcommand = args
        .get(2)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .unwrap_or("help");
    if remote_session_code_session_command_uses_help(subcommand) && is_help_at(args, 3) {
        print_remote_session_code_session_help();
        return Ok(());
    }

    match subcommand {
        "create" | "new" | "start" => remote_session_code_session_create(args).await,
        "bridge" | "credentials" | "creds" => remote_session_code_session_bridge(args).await,
        "smoke" | "live-smoke" => remote_session_code_session_smoke(args).await,
        "hydrate" | "restore" => remote_session_code_session_hydrate(args).await,
        "sdk-url" | "url" => remote_session_code_session_sdk_url(args),
        "help" | "--help" | "-h" => {
            print_remote_session_code_session_help();
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown remote-session code-session command '{}'\n\nUsage: kiana remote-session code-session [create|bridge|smoke|hydrate|sdk-url]",
            subcommand
        )),
    }
}

async fn remote_session_code_session_create(args: &[String]) -> Result<()> {
    let (config, mut access_token) =
        remote_session_code_session_credentials(args, "code-session create").await?;
    let title = remote_session_option_or_env(args, "--title", &["KIANA_REMOTE_CODE_SESSION_TITLE"])
        .or_else(|| remote_session_join_positionals(remote_session_code_session_positionals(args)))
        .unwrap_or_else(|| "Kiana remote code session".to_string());
    let tags = remote_session_code_session_tags(args);
    let session_id = remote_session_create_code_session_with_auth_retry(
        &config,
        &mut access_token,
        &title,
        &tags,
    )
    .await?;

    if remote_session_has_flag(args, "--json") {
        let sdk_url = kiana_remote::build_ccr_v2_sdk_url(&config.api_base_url, &session_id)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": session_id,
                "sdk_url": sdk_url,
            }))?
        );
    } else {
        println!("{}", session_id);
    }

    Ok(())
}

async fn remote_session_code_session_bridge(args: &[String]) -> Result<()> {
    let (config, mut access_token) =
        remote_session_code_session_credentials(args, "code-session bridge").await?;
    let session_id = remote_session_code_session_id_from_args(args, &config).ok_or_else(|| {
        anyhow!("remote-session code-session bridge requires --session-id, KIANA_REMOTE_SESSION_ID, or a cse_* session ID argument")
    })?;
    let trusted_device_token = remote_session_option_or_env(
        args,
        "--trusted-device-token",
        &[
            "KIANA_REMOTE_TRUSTED_DEVICE_TOKEN",
            "CLAUDE_TRUSTED_DEVICE_TOKEN",
        ],
    );
    let credentials = remote_session_fetch_credentials_with_auth_retry(
        &config,
        &mut access_token,
        &session_id,
        trusted_device_token.as_deref(),
    )
    .await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&credentials)?);
    } else {
        println!("worker_jwt: {}", credentials.worker_jwt);
        println!("api_base_url: {}", credentials.api_base_url);
        println!("expires_in: {}", credentials.expires_in);
        println!("worker_epoch: {}", credentials.worker_epoch);
    }

    Ok(())
}

async fn remote_session_code_session_smoke(args: &[String]) -> Result<()> {
    let (config, mut access_token) =
        remote_session_code_session_credentials(args, "code-session smoke").await?;
    let title = remote_session_option_or_env(args, "--title", &["KIANA_REMOTE_CODE_SESSION_TITLE"])
        .or_else(|| remote_session_join_positionals(remote_session_code_session_positionals(args)))
        .unwrap_or_else(|| "Kiana CCR v2 live smoke".to_string());
    let tags = remote_session_code_session_tags(args);
    let session_id = remote_session_create_code_session_with_auth_retry(
        &config,
        &mut access_token,
        &title,
        &tags,
    )
    .await?;
    let trusted_device_token = remote_session_option_or_env(
        args,
        "--trusted-device-token",
        &[
            "KIANA_REMOTE_TRUSTED_DEVICE_TOKEN",
            "CLAUDE_TRUSTED_DEVICE_TOKEN",
        ],
    );
    let credentials = remote_session_fetch_credentials_with_auth_retry(
        &config,
        &mut access_token,
        &session_id,
        trusted_device_token.as_deref(),
    )
    .await?;
    let sdk_url = kiana_remote::build_ccr_v2_sdk_url(&credentials.api_base_url, &session_id)?;

    if remote_session_has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "ok",
                "session_id": session_id,
                "api_base_url": credentials.api_base_url,
                "sdk_url": sdk_url,
                "expires_in": credentials.expires_in,
                "worker_epoch": credentials.worker_epoch,
            }))?
        );
    } else {
        println!("CCR v2 live smoke ok");
        println!("session_id: {}", session_id);
        println!("api_base_url: {}", credentials.api_base_url);
        println!("sdk_url: {}", sdk_url);
        println!("expires_in: {}", credentials.expires_in);
        println!("worker_epoch: {}", credentials.worker_epoch);
    }

    Ok(())
}

async fn remote_session_code_session_hydrate(args: &[String]) -> Result<()> {
    let (config, mut access_token) =
        remote_session_code_session_credentials(args, "code-session hydrate").await?;
    let session_id = remote_session_code_session_id_from_args(args, &config).ok_or_else(|| {
        anyhow!("remote-session code-session hydrate requires --session-id, KIANA_REMOTE_SESSION_ID, or a cse_* session ID argument")
    })?;
    let trusted_device_token = remote_session_option_or_env(
        args,
        "--trusted-device-token",
        &[
            "KIANA_REMOTE_TRUSTED_DEVICE_TOKEN",
            "CLAUDE_TRUSTED_DEVICE_TOKEN",
        ],
    );
    let credentials = remote_session_fetch_credentials_with_auth_retry(
        &config,
        &mut access_token,
        &session_id,
        trusted_device_token.as_deref(),
    )
    .await?;
    let session_url = kiana_remote::build_ccr_v2_sdk_url(&credentials.api_base_url, &session_id)?;
    let client = kiana_remote::CcrV2WorkerClient::new(
        session_url,
        session_id.clone(),
        credentials.worker_jwt,
        credentials.worker_epoch,
    )?;
    let report = crate::sdk::hydrate_ccr_v2_session_from_worker(session_id, &client).await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Hydrated CCR v2 session {}", report.session_id);
        println!("messages: {}", report.message_count);
        println!("internal_events: {}", report.internal_event_count);
        println!("subagent_events: {}", report.subagent_event_count);
        println!("skipped_events: {}", report.skipped_event_count);
    }

    Ok(())
}

fn remote_session_code_session_sdk_url(args: &[String]) -> Result<()> {
    let config = build_remote_session_cli_config(args)?;
    let session_id = remote_session_code_session_id_from_args(args, &config).ok_or_else(|| {
        anyhow!("remote-session code-session sdk-url requires --session-id, KIANA_REMOTE_SESSION_ID, or a cse_* session ID argument")
    })?;
    println!(
        "{}",
        kiana_remote::build_ccr_v2_sdk_url(&config.api_base_url, &session_id)?
    );
    Ok(())
}

fn remote_session_code_session_id_from_args(
    args: &[String],
    config: &RemoteSessionCliConfig,
) -> Option<String> {
    config.session_id.clone().or_else(|| {
        remote_session_code_session_positionals(args)
            .into_iter()
            .next()
    })
}

fn remote_session_code_session_positionals(args: &[String]) -> Vec<String> {
    let mut positionals = remote_session_positional_args(args);
    if positionals.first().is_some_and(|value| {
        matches!(
            value.as_str(),
            "create"
                | "new"
                | "start"
                | "bridge"
                | "credentials"
                | "creds"
                | "smoke"
                | "live-smoke"
                | "hydrate"
                | "restore"
                | "sdk-url"
                | "url"
        )
    }) {
        positionals.remove(0);
    }
    positionals
}

fn remote_session_code_session_tags(args: &[String]) -> Vec<String> {
    remote_session_option_values(args, "--tag", &["KIANA_REMOTE_CODE_SESSION_TAGS"])
}

fn remote_session_option_values(args: &[String], name: &str, envs: &[&str]) -> Vec<String> {
    let inline_prefix = format!("{}=", name);
    let mut values = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix(&inline_prefix) {
            values.push(value.to_string());
        } else if arg == name {
            if let Some(value) = args.get(index + 1).filter(|value| !value.starts_with('-')) {
                values.push(value.clone());
            }
        }
    }
    if values.is_empty() {
        for env in envs {
            if let Ok(value) = std::env::var(env) {
                values.extend(value.split(',').map(str::to_string));
            }
        }
    }
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn remote_session_create_environment_id(args: &[String]) -> Option<String> {
    remote_session_option_or_env(
        args,
        "--environment-id",
        &[
            "KIANA_REMOTE_ENVIRONMENT_ID",
            "CLAUDE_ENVIRONMENT_ID",
            "KIANA_REMOTE_DEFAULT_ENVIRONMENT_ID",
            "CLAUDE_DEFAULT_ENVIRONMENT_ID",
        ],
    )
    .or_else(|| remote_session_default_environment_id(args))
}

fn remote_session_create_message_from_args(args: &[String]) -> Option<String> {
    if let Some(message) = remote_session_option_or_env(args, "--message", &[]) {
        return Some(message);
    }

    remote_session_join_positionals(remote_session_positional_args(args))
}

fn remote_session_create_permission_mode(args: &[String]) -> Result<Option<String>> {
    let Some(mode) = remote_session_option_or_env(
        args,
        "--permission-mode",
        &[
            "KIANA_REMOTE_CREATE_PERMISSION_MODE",
            "KIANA_PERMISSION_MODE",
        ],
    ) else {
        return Ok(None);
    };

    normalize_stream_json_permission_mode(&mode)
        .map(str::to_string)
        .map(Some)
        .ok_or_else(|| {
            anyhow!(
                "invalid remote-session create --permission-mode '{}'; expected default, ask, plan, acceptEdits, bypassPermissions, or dontAsk",
                mode
            )
        })
}

fn remote_session_create_context(args: &[String]) -> Result<kiana_remote::SessionContext> {
    let git_url = remote_session_option_or_env(args, "--git-url", &[]);
    let git_revision = remote_session_option_or_env(args, "--git-revision", &[]);
    let sources = git_url
        .as_ref()
        .map(|url| {
            vec![kiana_remote::SessionContextSource::GitRepository {
                url: url.clone(),
                revision: git_revision,
                allow_unrestricted_git_push: None,
            }]
        })
        .unwrap_or_default();

    let outcome_repo = remote_session_option_or_env(args, "--outcome-repo", &[]).or_else(|| {
        git_url
            .as_deref()
            .and_then(remote_session_github_repo_from_url)
    });
    let outcome_branch = remote_session_option_or_env(args, "--outcome-branch", &[]);
    let outcomes = match (outcome_repo, outcome_branch) {
        (Some(repo), Some(branch)) => Some(vec![serde_json::json!({
            "type": "git_repository",
            "git_info": {
                "type": "github",
                "repo": repo,
                "branches": [branch],
            }
        })]),
        (None, Some(_)) => {
            return Err(anyhow!(
                "remote-session create --outcome-branch requires --outcome-repo or a parseable GitHub --git-url"
            ));
        }
        _ => None,
    };

    let cwd = remote_session_option_or_env(args, "--cwd", &[]).or_else(|| {
        std::env::current_dir()
            .ok()
            .map(|path| path.to_string_lossy().to_string())
            .filter(|path| !path.trim().is_empty())
    });
    let reuse_outcome_branches = if remote_session_has_flag(args, "--no-reuse-outcome-branches") {
        Some(false)
    } else if outcomes.is_some() || remote_session_has_flag(args, "--reuse-outcome-branches") {
        Some(true)
    } else {
        None
    };

    Ok(kiana_remote::SessionContext {
        sources,
        cwd,
        outcomes,
        model: remote_session_option_or_env(args, "--model", &["KIANA_REMOTE_MODEL"]),
        seed_bundle_file_id: remote_session_option_or_env(args, "--seed-bundle-file-id", &[]),
        reuse_outcome_branches,
        ..Default::default()
    })
}

fn remote_session_create_should_seed_bundle(args: &[String]) -> bool {
    remote_session_has_flag(args, "--seed-bundle")
        || remote_session_has_flag(args, "--use-bundle")
        || env_truthy("KIANA_REMOTE_SEED_BUNDLE")
        || env_truthy("CCR_FORCE_BUNDLE")
}

fn remote_session_seed_bundle_dir(args: &[String]) -> Option<String> {
    remote_session_option_or_env(args, "--seed-bundle-dir", &["KIANA_REMOTE_SEED_BUNDLE_DIR"])
}

fn remote_session_seed_bundle_max_bytes(args: &[String]) -> Result<Option<u64>> {
    let Some(value) = remote_session_option_or_env(
        args,
        "--seed-bundle-max-bytes",
        &["KIANA_REMOTE_SEED_BUNDLE_MAX_BYTES"],
    ) else {
        return Ok(None);
    };
    value.parse::<u64>().map(Some).with_context(|| {
        format!("--seed-bundle-max-bytes requires an integer byte limit, got '{value}'")
    })
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn remote_session_github_repo_from_url(url: &str) -> Option<String> {
    let path = if let Some(path) = url.strip_prefix("git@github.com:") {
        path
    } else {
        let marker = "github.com/";
        let index = url.find(marker)?;
        &url[index + marker.len()..]
    };
    let path = path
        .trim()
        .trim_matches('/')
        .trim_end_matches(".git")
        .trim_matches('/');
    let mut parts = path.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() {
        None
    } else {
        Some(format!("{owner}/{repo}"))
    }
}

async fn remote_session_environments(args: &[String]) -> Result<()> {
    if args.iter().skip(2).any(|arg| is_help_arg(arg)) {
        print_remote_session_environment_help();
        return Ok(());
    }

    let subcommand = args
        .get(2)
        .map(String::as_str)
        .filter(|value| !value.starts_with('-'))
        .unwrap_or("list");

    match subcommand {
        "list" | "ls" | "status" => remote_session_environment_list(args).await,
        "selected" | "select" | "current" => remote_session_environment_selected(args).await,
        "create-default" | "create-default-cloud" | "create-cloud" => {
            remote_session_environment_create_default(args).await
        }
        "help" | "--help" | "-h" => {
            print_remote_session_environment_help();
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown remote-session environments command '{}'\n\nUsage: kiana remote-session environments [list|selected|create-default]",
            subcommand
        )),
    }
}

async fn remote_session_environment_list(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) =
        remote_session_api_credentials(args, "environments").await?;
    let environments =
        kiana_remote::fetch_environments(&config.api_base_url, &org_uuid, &access_token).await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&environments)?);
        return Ok(());
    }

    if environments.is_empty() {
        println!("No remote environments.");
        return Ok(());
    }

    for environment in environments {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            environment.environment_id,
            environment.kind,
            environment.state,
            environment.created_at,
            environment.name
        );
    }

    Ok(())
}

async fn remote_session_environment_selected(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) =
        remote_session_api_credentials(args, "environments").await?;
    let environments =
        kiana_remote::fetch_environments(&config.api_base_url, &org_uuid, &access_token).await?;
    let default_environment_id = remote_session_default_environment_id(args);
    let selection =
        kiana_remote::select_environment(&environments, default_environment_id.as_deref());

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&selection)?);
        return Ok(());
    }

    match selection.selected_environment {
        Some(environment) => println!(
            "{}\t{}\t{}\t{}",
            environment.environment_id, environment.kind, environment.state, environment.name
        ),
        None => println!("No remote environments."),
    }

    Ok(())
}

async fn remote_session_environment_create_default(args: &[String]) -> Result<()> {
    let (config, org_uuid, access_token) =
        remote_session_api_credentials(args, "environments").await?;
    let name = remote_session_option_or_env(args, "--name", &[])
        .or_else(|| remote_session_join_positionals(remote_session_environment_positionals(args)))
        .unwrap_or_else(|| "Default Cloud Environment".to_string());
    let environment = kiana_remote::create_default_cloud_environment(
        &config.api_base_url,
        &org_uuid,
        &access_token,
        &name,
    )
    .await?;

    if remote_session_has_flag(args, "--json") {
        println!("{}", serde_json::to_string_pretty(&environment)?);
        return Ok(());
    }

    println!(
        "{}\t{}\t{}\t{}",
        environment.environment_id, environment.kind, environment.state, environment.name
    );

    Ok(())
}

async fn remote_session_listen(args: &[String]) -> Result<()> {
    let config = build_remote_session_cli_config(args)?;
    let session_id = config.session_id.clone().ok_or_else(|| {
        anyhow!("remote-session listen requires --session-id or KIANA_REMOTE_SESSION_ID")
    })?;
    let org_uuid = config.org_uuid.clone().ok_or_else(|| {
        anyhow!("remote-session listen requires --org-uuid or KIANA_REMOTE_ORG_UUID")
    })?;
    let access_token = remote_session_live_access_token("listen").await?.value;
    let once = remote_session_has_flag(args, "--once");
    let permission_mode = remote_session_permission_mode(args)?;
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let access_token = Arc::new(StdMutex::new(access_token));
    let token_provider = {
        let access_token = access_token.clone();
        Arc::new(move || access_token.lock().unwrap().clone())
    };
    let manager = Arc::new(kiana_remote::RemoteSessionManager::new(
        kiana_remote::create_remote_session_config_with_api_base_url(
            session_id.clone(),
            token_provider,
            org_uuid,
            false,
            false,
            config.api_base_url,
        ),
        Arc::new(RemoteSessionCliCallbacks {
            events: event_tx,
            access_token,
        }),
    ));

    manager.connect().await?;

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(RemoteSessionCliEvent::Message) if once => break,
                    Some(RemoteSessionCliEvent::Message) => {}
                    Some(RemoteSessionCliEvent::PermissionRequest { request_id, request }) => {
                        handle_remote_session_permission_request(
                            manager.as_ref(),
                            permission_mode,
                            request_id,
                            request,
                        )
                        .await?;
                    }
                    Some(RemoteSessionCliEvent::Closed) | None => break,
                    Some(RemoteSessionCliEvent::Error(error)) => {
                        manager.disconnect().await;
                        return Err(anyhow!("remote session stream error: {}", error));
                    }
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal?;
                break;
            }
        }
    }

    manager.disconnect().await;
    Ok(())
}

async fn remote_session_send(args: &[String]) -> Result<()> {
    let config = build_remote_session_cli_config(args)?;
    let session_id = config.session_id.clone().ok_or_else(|| {
        anyhow!("remote-session send requires --session-id or KIANA_REMOTE_SESSION_ID")
    })?;
    let org_uuid = config.org_uuid.clone().ok_or_else(|| {
        anyhow!("remote-session send requires --org-uuid or KIANA_REMOTE_ORG_UUID")
    })?;
    let access_token = remote_session_live_access_token("send").await?.value;
    let message = remote_session_send_message_from_args(args).ok_or_else(|| {
        anyhow!("remote-session send requires a message argument or --message <text>")
    })?;
    let uuid = remote_session_option_or_env(args, "--uuid", &[]);

    let sent = kiana_remote::send_event_to_remote_session(
        &config.api_base_url,
        &session_id,
        &org_uuid,
        &access_token,
        kiana_remote::RemoteMessageContent::Text(message),
        kiana_remote::SendRemoteMessageOptions { uuid },
    )
    .await;

    if !sent {
        return Err(anyhow!("remote-session send failed"));
    }

    println!("Remote session message sent.");
    Ok(())
}

fn remote_session_send_message_from_args(args: &[String]) -> Option<String> {
    if let Some(message) = remote_session_option_or_env(args, "--message", &[]) {
        return Some(message);
    }

    remote_session_join_positionals(remote_session_positional_args(args))
}

fn remote_session_positional_args(args: &[String]) -> Vec<String> {
    let mut parts = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if index < 2
            || remote_session_send_skip_inline_option(arg)
            || remote_session_send_skip_flag(arg)
        {
            index += 1;
            continue;
        }
        if remote_session_send_skip_option_value(arg) {
            index += 2;
            continue;
        }
        parts.push(arg.clone());
        index += 1;
    }

    parts
}

fn remote_session_environment_positionals(args: &[String]) -> Vec<String> {
    let mut positionals = remote_session_positional_args(args);
    if positionals.first().is_some_and(|value| {
        matches!(
            value.as_str(),
            "create-default" | "create-default-cloud" | "create-cloud"
        )
    }) {
        positionals.remove(0);
    }
    positionals
}

fn remote_session_default_environment_id(args: &[String]) -> Option<String> {
    remote_session_option_or_env(
        args,
        "--default-environment-id",
        &[
            "KIANA_REMOTE_DEFAULT_ENVIRONMENT_ID",
            "CLAUDE_DEFAULT_ENVIRONMENT_ID",
        ],
    )
}

fn remote_session_join_positionals(parts: Vec<String>) -> Option<String> {
    let message = parts.join(" ").trim().to_string();
    if message.is_empty() {
        None
    } else {
        Some(message)
    }
}

fn remote_session_send_skip_inline_option(arg: &str) -> bool {
    [
        "--session-id=",
        "--org-uuid=",
        "--api-base-url=",
        "--uuid=",
        "--event-uuid=",
        "--request-id=",
        "--message=",
        "--title=",
        "--tag=",
        "--trusted-device-token=",
        "--name=",
        "--default-environment-id=",
        "--permission-mode=",
        "--environment-id=",
        "--source=",
        "--model=",
        "--git-url=",
        "--git-revision=",
        "--outcome-repo=",
        "--outcome-branch=",
        "--seed-bundle-file-id=",
        "--seed-bundle-dir=",
        "--seed-bundle-max-bytes=",
        "--cwd=",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
}

fn remote_session_send_skip_option_value(arg: &str) -> bool {
    matches!(
        arg,
        "--session-id"
            | "--org-uuid"
            | "--api-base-url"
            | "--uuid"
            | "--event-uuid"
            | "--request-id"
            | "--message"
            | "--title"
            | "--tag"
            | "--trusted-device-token"
            | "--name"
            | "--default-environment-id"
            | "--permission-mode"
            | "--environment-id"
            | "--source"
            | "--model"
            | "--git-url"
            | "--git-revision"
            | "--outcome-repo"
            | "--outcome-branch"
            | "--seed-bundle-file-id"
            | "--seed-bundle-dir"
            | "--seed-bundle-max-bytes"
            | "--cwd"
    )
}

fn remote_session_send_skip_flag(arg: &str) -> bool {
    matches!(
        arg,
        "--once"
            | "--auto-allow-permissions"
            | "--auto-deny-permissions"
            | "--reuse-outcome-branches"
            | "--no-reuse-outcome-branches"
            | "--seed-bundle"
            | "--use-bundle"
            | "--json"
            | "--help"
            | "-h"
    )
}

async fn handle_remote_session_permission_request(
    manager: &kiana_remote::RemoteSessionManager,
    mode: RemoteSessionPermissionMode,
    request_id: String,
    request: kiana_remote::SDKControlPermissionRequest,
) -> Result<()> {
    match mode {
        RemoteSessionPermissionMode::Deny => {
            manager
                .respond_to_permission_request(
                    request_id,
                    kiana_remote::RemotePermissionResponse::Deny {
                        message: "Denied by kiana remote-session listen --permission-mode deny"
                            .to_string(),
                    },
                )
                .await?;
            Ok(())
        }
        RemoteSessionPermissionMode::Allow => {
            manager
                .respond_to_permission_request(
                    request_id,
                    kiana_remote::RemotePermissionResponse::Allow {
                        updated_input: request.input,
                    },
                )
                .await?;
            Ok(())
        }
        RemoteSessionPermissionMode::Manual => {
            eprintln!("Remote permission request received; --permission-mode manual does not send an automatic response.");
            Ok(())
        }
    }
}

fn remote_session_permission_mode(args: &[String]) -> Result<RemoteSessionPermissionMode> {
    if remote_session_has_flag(args, "--auto-allow-permissions") {
        return Ok(RemoteSessionPermissionMode::Allow);
    }
    if remote_session_has_flag(args, "--auto-deny-permissions") {
        return Ok(RemoteSessionPermissionMode::Deny);
    }

    let value =
        remote_session_option_or_env(args, "--permission-mode", &["KIANA_REMOTE_PERMISSION_MODE"])
            .unwrap_or_else(|| "deny".to_string());
    match value.as_str() {
        "deny" | "reject" | "refuse" => Ok(RemoteSessionPermissionMode::Deny),
        "allow" | "approve" => Ok(RemoteSessionPermissionMode::Allow),
        "manual" | "none" | "observe" => Ok(RemoteSessionPermissionMode::Manual),
        _ => Err(anyhow!(
            "invalid remote-session --permission-mode '{}'; expected deny, allow, or manual",
            value
        )),
    }
}

fn remote_session_has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn print_remote_session_status(config: &RemoteSessionCliConfig) {
    println!("{}", remote_session_status_text(config));
}

fn remote_session_status_text(config: &RemoteSessionCliConfig) -> String {
    let mut lines = vec![
        "Remote session status".to_string(),
        format!(
            "session_id: {}",
            config.session_id.as_deref().unwrap_or("-")
        ),
        format!("org_uuid: {}", config.org_uuid.as_deref().unwrap_or("-")),
        format!("api_base_url: {}", config.api_base_url),
        format!("token_configured: {}", config.token_configured),
        format!("token_status: {}", config.token_status.label()),
    ];
    if let Some(fix) = config.token_status.fix() {
        lines.push(format!("token_fix: {fix}"));
    }
    lines.extend([
        "list: kiana remote-session list --org-uuid <uuid>".to_string(),
        "show: kiana remote-session show <id> --org-uuid <uuid>".to_string(),
        "rename: kiana remote-session rename <id> <title> --org-uuid <uuid>".to_string(),
        "create: kiana remote-session create --environment-id <id> --org-uuid <uuid> <message>"
            .to_string(),
        "archive: kiana remote-session archive <id> --org-uuid <uuid>".to_string(),
        "env: kiana remote-session environments --org-uuid <uuid>".to_string(),
        "code-session: kiana remote-session code-session create --title <title>".to_string(),
        "url: kiana remote-session url --session-id <id> --org-uuid <uuid>".to_string(),
        "send: kiana remote-session send --session-id <id> --org-uuid <uuid> <message>".to_string(),
    ]);
    lines.join("\n")
}

fn print_remote_session_help() {
    println!("Usage: kiana remote-session [status|list|show|rename|create|archive|environments|code-session|url|listen|send] [options]");
    println!();
    println!("Commands:");
    println!("  status                  Inspect remote session configuration");
    println!("  list                    List remote sessions through the Sessions API");
    println!("  show                    Fetch and print one remote session");
    println!("  rename                  Update a remote session title");
    println!("  create                  Create a remote session through the Sessions API");
    println!("  archive                 Archive a remote session");
    println!("  environments            List, select, or create remote environments");
    println!("  code-session            Create env-less CCR v2 sessions and credentials");
    println!("  url                     Print the WebSocket subscription URL");
    println!("  listen                  Connect and print remote session messages as JSONL");
    println!("  send                    Send a user message event to a remote session");
    println!();
    println!("Options:");
    println!("  --session-id <id>       Remote session ID");
    println!("  --org-uuid <uuid>       Organization UUID");
    println!("  --api-base-url <url>    API base URL, e.g. https://api.anthropic.com");
    println!("  --message <text>        Message text for remote-session send");
    println!("  --title <text>          Title text for remote-session rename");
    println!("  --tag <tag>             Tag for code-session create; may be repeated");
    println!("  --trusted-device-token <token>  Trusted device token for code-session bridge");
    println!("  --environment-id <id>   Environment for remote-session create");
    println!("  --model <name>          Model for remote-session create context");
    println!("  --git-url <url>         Git source URL for remote-session create context");
    println!("  --outcome-branch <name> Git outcome branch for remote-session create context");
    println!("  --seed-bundle-file-id <id>  Seed bundle file ID for remote-session create");
    println!("  --seed-bundle           Create and upload a local git bundle before create");
    println!("  --seed-bundle-dir <dir> Directory to bundle; defaults to current directory");
    println!("  --name <text>           Name for environments create-default");
    println!("  --default-environment-id <id>  Preferred environment for selected");
    println!("  --uuid <uuid>           Event UUID for remote-session send");
    println!("  --json                  Print list/show responses as JSON");
    println!("  --once                  Exit after the first remote message");
    println!("  --permission-mode <m>   listen: deny/allow/manual; create: default/ask/plan/acceptEdits/bypassPermissions/dontAsk");
    println!();
    println!("Environment:");
    println!("  KIANA_REMOTE_SESSION_ID or CLAUDE_REMOTE_SESSION_ID");
    println!("  KIANA_REMOTE_ORG_UUID, CLAUDE_ORG_UUID, or ANTHROPIC_ORGANIZATION_ID");
    println!("  KIANA_REMOTE_API_BASE_URL or ANTHROPIC_API_BASE_URL");
    println!("  KIANA_REMOTE_ACCESS_TOKEN, CLAUDE_ACCESS_TOKEN, or ANTHROPIC_AUTH_TOKEN");
    println!("  KIANA_REMOTE_REFRESH_COMMAND may output a replacement token after WS 4003.");
    println!("  KIANA_REMOTE_ENVIRONMENT_ID or KIANA_REMOTE_DEFAULT_ENVIRONMENT_ID");
    println!("  KIANA_REMOTE_SEED_BUNDLE=1 to upload a local git bundle during create");
    println!("  KIANA_REMOTE_PERMISSION_MODE=deny|allow|manual");
    println!("  KIANA_REMOTE_CODE_SESSION_TAGS=tag1,tag2 for code-session create");
}

fn print_remote_session_environment_help() {
    println!("Usage: kiana remote-session environments [list|selected|create-default] [options]");
    println!();
    println!("Commands:");
    println!("  list                    List remote environments");
    println!("  selected                Print selected environment using reference fallback rules");
    println!("  create-default [name]   Create an anthropic_cloud default environment");
}

fn print_remote_session_code_session_help() {
    println!(
        "Usage: kiana remote-session code-session [create|bridge|smoke|hydrate|sdk-url] [options]"
    );
    println!();
    println!("Commands:");
    println!("  create [title]          POST /v1/code/sessions and print the cse_* ID");
    println!("  bridge <cse_id>         POST /v1/code/sessions/<id>/bridge and print credentials");
    println!("  smoke [title]           Create a CCR v2 session and fetch bridge credentials");
    println!("  hydrate <cse_id>        Restore foreground and subagent CCR v2 transcripts");
    println!("  sdk-url <cse_id>        Print /v1/code/sessions/<id> SDK URL");
    println!();
    println!("Options:");
    println!("  --api-base-url <url>    API base URL, e.g. https://api.anthropic.com");
    println!("  --session-id <id>       Code session ID for bridge/hydrate/sdk-url");
    println!("  --title <text>          Title for code-session create");
    println!("  --tag <tag>             Tag for code-session create; may be repeated");
    println!("  --trusted-device-token <token>  Trusted device token for bridge credentials");
    println!("  --json                  Print create/bridge responses as JSON");
    println!();
    println!("Environment:");
    println!("  KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN is required for live calls");
    println!("  ANTHROPIC_AUTH_TOKEN is accepted only when it is a remote access token, not a sk-* API key");
}

async fn bridge_main(args: &[String]) -> Result<()> {
    let command = args.get(1).map(String::as_str).unwrap_or("status");
    if is_help_at(args, 2) {
        print_bridge_help();
        return Ok(());
    }

    match command {
        "status" => {
            let config = build_bridge_config(args)?;
            print_bridge_status(&config, bridge_access_token().is_some());
            Ok(())
        }
        "start" | "run" => start_bridge(args).await,
        "help" | "--help" | "-h" => {
            print_bridge_help();
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown bridge command '{}'\n\nUsage: kiana bridge [status|start]",
            command
        )),
    }
}

async fn start_bridge(args: &[String]) -> Result<()> {
    let config = build_bridge_config(args)?;
    bridge_access_token_refreshing_if_expiring()
        .await?
        .ok_or_else(|| {
            anyhow!("remote bridge requires KIANA_BRIDGE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN")
        })?;

    let api = Arc::new(BridgeApiClient::with_auth_provider(
        config.api_base_url.clone(),
        Arc::new(CliBridgeAuthProvider),
    ));
    println!("Registering bridge environment...");
    let (environment_id, environment_secret) = api.register_environment(&config).await?;
    println!("Bridge environment registered: {}", environment_id);
    println!("Polling for remote work. Press Ctrl+C to stop.");

    let session_manager = Arc::new(SessionManager::new());
    let runner = CommandBridgeSessionRunner::for_config(&config)?;
    let work_loop = Arc::new(WorkPollLoop::with_runner(
        api.clone(),
        session_manager,
        config,
        Arc::new(runner),
    ));
    let run_result = tokio::select! {
        result = work_loop.run(environment_id.clone(), environment_secret) => result,
        signal = tokio::signal::ctrl_c() => {
            signal?;
            if let Err(error) = work_loop.shutdown_active_work(&environment_id).await {
                eprintln!("Bridge active work shutdown error: {}", error);
            }
            Ok(())
        }
    };

    if let Err(error) = api.deregister_environment(&environment_id).await {
        eprintln!("Bridge deregistration error: {}", error);
    }

    run_result
}

fn build_bridge_config(args: &[String]) -> Result<BridgeConfig> {
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let api_base_url = option_or_env(args, "--api-base-url", "KIANA_BRIDGE_API_BASE_URL")
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    let session_ingress_url = option_or_env(
        args,
        "--session-ingress-url",
        "KIANA_BRIDGE_SESSION_INGRESS_URL",
    )
    .unwrap_or_else(|| api_base_url.clone());
    let dir = option_or_env(args, "--dir", "KIANA_BRIDGE_DIR").unwrap_or(cwd);
    let max_sessions = option_or_env(args, "--max-sessions", "KIANA_BRIDGE_MAX_SESSIONS")
        .as_deref()
        .unwrap_or("1")
        .parse::<usize>()?;
    let heartbeat_interval_ms = option_or_env(
        args,
        "--heartbeat-interval-ms",
        "KIANA_BRIDGE_HEARTBEAT_INTERVAL_MS",
    )
    .as_deref()
    .unwrap_or("60000")
    .parse::<u64>()?;
    let session_timeout_ms = bridge_session_timeout_ms(args)?;
    let ccr_v2_sse_reconnect_give_up_ms = option_or_env(
        args,
        "--ccr-v2-sse-reconnect-give-up-ms",
        "KIANA_BRIDGE_CCR_V2_SSE_RECONNECT_GIVE_UP_MS",
    )
    .map(|value| {
        value.parse::<u64>().with_context(|| {
            format!(
                "--ccr-v2-sse-reconnect-give-up-ms/KIANA_BRIDGE_CCR_V2_SSE_RECONNECT_GIVE_UP_MS requires an integer, got '{}'",
                value
            )
        })
    })
    .transpose()?;
    let ccr_v2_sse_liveness_timeout_ms = option_or_env(
        args,
        "--ccr-v2-sse-liveness-timeout-ms",
        "KIANA_BRIDGE_CCR_V2_SSE_LIVENESS_TIMEOUT_MS",
    )
    .map(|value| {
        value.parse::<u64>().with_context(|| {
            format!(
                "--ccr-v2-sse-liveness-timeout-ms/KIANA_BRIDGE_CCR_V2_SSE_LIVENESS_TIMEOUT_MS requires an integer, got '{}'",
                value
            )
        })
    })
    .transpose()?;

    Ok(BridgeConfig {
        dir: dir.clone(),
        machine_name: option_or_env(args, "--machine-name", "KIANA_BRIDGE_MACHINE_NAME")
            .unwrap_or_else(local_machine_name),
        branch: option_or_env(args, "--branch", "KIANA_BRIDGE_BRANCH")
            .or_else(|| current_git_branch_for(&dir))
            .unwrap_or_else(|| "unknown".to_string()),
        git_repo_url: option_or_env(args, "--git-repo-url", "KIANA_BRIDGE_GIT_REPO_URL")
            .or_else(|| current_git_remote_url_for(&dir)),
        max_sessions,
        spawn_mode: parse_spawn_mode(
            option_or_env(args, "--spawn-mode", "KIANA_BRIDGE_SPAWN_MODE")
                .as_deref()
                .unwrap_or("single-session"),
        )?,
        bridge_id: option_or_env(args, "--bridge-id", "KIANA_BRIDGE_ID")
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        worker_type: option_or_env(args, "--worker-type", "KIANA_BRIDGE_WORKER_TYPE")
            .unwrap_or_else(|| "kiana_code".to_string()),
        environment_id: option_or_env(args, "--environment-id", "KIANA_BRIDGE_ENVIRONMENT_ID")
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        api_base_url,
        session_ingress_url,
        heartbeat_interval_ms,
        session_timeout_ms,
        ccr_v2_sse_reconnect_give_up_ms,
        ccr_v2_sse_liveness_timeout_ms,
        debug_file: option_or_env(args, "--debug-file", "KIANA_BRIDGE_DEBUG_FILE"),
        permission_mode: bridge_permission_mode(args)?,
    })
}

fn bridge_permission_mode(args: &[String]) -> Result<Option<String>> {
    let Some(mode) = option_or_env(args, "--permission-mode", "KIANA_BRIDGE_PERMISSION_MODE")
        .or_else(|| std::env::var("KIANA_PERMISSION_MODE").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    normalize_stream_json_permission_mode(&mode)
        .map(str::to_string)
        .map(Some)
        .ok_or_else(|| anyhow!("invalid bridge --permission-mode '{}'", mode))
}

fn bridge_session_timeout_ms(args: &[String]) -> Result<u64> {
    if let Some(value) = option_or_env(
        args,
        "--session-timeout-ms",
        "KIANA_BRIDGE_SESSION_TIMEOUT_MS",
    ) {
        return value.parse::<u64>().with_context(|| {
            format!(
                "--session-timeout-ms/KIANA_BRIDGE_SESSION_TIMEOUT_MS requires an integer, got '{}'",
                value
            )
        });
    }
    if let Some(value) = option_or_env(args, "--session-timeout", "KIANA_BRIDGE_SESSION_TIMEOUT") {
        let seconds = value.parse::<u64>().with_context(|| {
            format!(
                "--session-timeout/KIANA_BRIDGE_SESSION_TIMEOUT requires seconds as an integer, got '{}'",
                value
            )
        })?;
        return Ok(seconds.saturating_mul(1000));
    }
    Ok(24 * 60 * 60 * 1000)
}

fn option_or_env(args: &[String], name: &str, env: &str) -> Option<String> {
    let inline_prefix = format!("{}=", name);
    args.iter()
        .find_map(|arg| arg.strip_prefix(&inline_prefix).map(str::to_string))
        .or_else(|| {
            args.windows(2).find_map(|window| {
                if window[0] == name {
                    Some(window[1].clone())
                } else {
                    None
                }
            })
        })
        .or_else(|| std::env::var(env).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn bridge_access_token() -> Option<String> {
    std::env::var("KIANA_BRIDGE_ACCESS_TOKEN")
        .or_else(|_| std::env::var("CLAUDE_ACCESS_TOKEN"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            kiana_services::oauth::load_oauth_tokens()
                .ok()
                .flatten()
                .map(|tokens| tokens.access_token.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

async fn bridge_access_token_refreshing_if_expiring() -> Result<Option<String>> {
    if let Some(token) = std::env::var("KIANA_BRIDGE_ACCESS_TOKEN")
        .or_else(|_| std::env::var("CLAUDE_ACCESS_TOKEN"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(token));
    }

    Ok(
        kiana_services::oauth::load_oauth_tokens_refreshing_if_expiring(
            oauth_proactive_refresh_skew(),
        )
        .await?
        .map(|tokens| tokens.access_token.trim().to_string())
        .filter(|value| !value.is_empty()),
    )
}

struct CliBridgeAuthProvider;

#[async_trait]
impl BridgeAuthProvider for CliBridgeAuthProvider {
    fn access_token(&self) -> Option<String> {
        bridge_access_token()
    }

    async fn refresh_after_unauthorized(&self, stale_access_token: &str) -> Result<bool> {
        if let Some(command_line) = std::env::var("KIANA_BRIDGE_REFRESH_COMMAND")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            let mut command = if cfg!(windows) {
                let mut command = tokio::process::Command::new("cmd");
                command.args(["/C", &command_line]);
                command
            } else {
                let mut command = tokio::process::Command::new("sh");
                command.arg("-lc").arg(&command_line);
                command
            };
            let output = command
                .env("KIANA_BRIDGE_STALE_ACCESS_TOKEN", stale_access_token)
                .output()
                .await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow!(
                    "bridge refresh command failed with status {}{}",
                    output.status,
                    if stderr.trim().is_empty() {
                        String::new()
                    } else {
                        format!(": {}", stderr.trim())
                    }
                ));
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let token = parse_bridge_refresh_token(&stdout)
                .ok_or_else(|| anyhow!("bridge refresh command did not output an access token"))?;
            std::env::set_var("KIANA_BRIDGE_ACCESS_TOKEN", token);
            return Ok(true);
        }

        if let Some(tokens) = kiana_services::oauth::refresh_stored_oauth_tokens().await? {
            std::env::set_var("KIANA_BRIDGE_ACCESS_TOKEN", tokens.access_token);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn parse_bridge_refresh_token(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        for key in ["access_token", "accessToken", "token"] {
            if let Some(token) = value
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|token| !token.is_empty())
            {
                return Some(token.to_string());
            }
        }
    }
    trimmed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn parse_spawn_mode(value: &str) -> Result<SpawnMode> {
    match value {
        "single-session" | "single_session" | "single" => Ok(SpawnMode::SingleSession),
        "same-dir" | "same_dir" => Ok(SpawnMode::SameDir),
        "worktree" => Ok(SpawnMode::Worktree),
        _ => Err(anyhow!(
            "invalid spawn mode '{}'; expected single-session, same-dir, or worktree",
            value
        )),
    }
}

fn local_machine_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn current_git_branch_for(dir: &str) -> Option<String> {
    git_output_in_dir(dir, ["branch", "--show-current"])
}

fn current_git_remote_url_for(dir: &str) -> Option<String> {
    git_output_in_dir(dir, ["config", "--get", "remote.origin.url"])
}

fn git_output_in_dir<const N: usize>(dir: &str, args: [&str; N]) -> Option<String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn print_bridge_status(config: &BridgeConfig, token_configured: bool) {
    println!("Remote bridge status");
    println!("dir: {}", config.dir);
    println!("machine_name: {}", config.machine_name);
    println!("branch: {}", config.branch);
    println!(
        "git_repo_url: {}",
        config.git_repo_url.as_deref().unwrap_or("-")
    );
    println!("api_base_url: {}", config.api_base_url);
    println!("session_ingress_url: {}", config.session_ingress_url);
    println!("max_sessions: {}", config.max_sessions);
    println!("spawn_mode: {}", spawn_mode_label(config.spawn_mode));
    println!("heartbeat_interval_ms: {}", config.heartbeat_interval_ms);
    println!("session_timeout_ms: {}", config.session_timeout_ms);
    println!(
        "ccr_v2_sse_reconnect_give_up_ms: {}",
        config
            .ccr_v2_sse_reconnect_give_up_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "ccr_v2_sse_liveness_timeout_ms: {}",
        config
            .ccr_v2_sse_liveness_timeout_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "debug_file: {}",
        config.debug_file.as_deref().unwrap_or("-")
    );
    println!("token_configured: {}", token_configured);
    println!("run: kiana bridge start");
}

fn print_bridge_help() {
    println!("Usage: kiana bridge [status|start] [options]");
    println!();
    println!("Options:");
    println!("  --api-base-url <url>          Bridge API base URL");
    println!("  --session-ingress-url <url>   Session ingress base URL");
    println!("  --dir <path>                  Working directory to register");
    println!("  --branch <name>               Git branch metadata");
    println!("  --max-sessions <n>            Maximum concurrent sessions");
    println!("  --spawn-mode <mode>           single-session, same-dir, or worktree");
    println!("  --heartbeat-interval-ms <n>   Work lease heartbeat interval; 0 disables it");
    println!("  --session-timeout <seconds>   Per-session timeout; 0 disables it");
    println!("  --session-timeout-ms <n>      Per-session timeout in milliseconds");
    println!("  --ccr-v2-sse-reconnect-give-up-ms <n>");
    println!("                                  CCR v2 SSE reconnect budget in milliseconds");
    println!("  --ccr-v2-sse-liveness-timeout-ms <n>");
    println!("                                  CCR v2 SSE idle liveness timeout; 0 disables it");
    println!("  --debug-file <path>           Write per-session bridge debug logs");
    println!();
    println!("Environment:");
    println!("  KIANA_BRIDGE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN is required for start.");
    println!("  KIANA_BRIDGE_REFRESH_COMMAND may output a replacement token after HTTP 401.");
    println!("  KIANA_BRIDGE_DEBUG_FILE enables per-session debug and transcript logs.");
}

fn spawn_mode_label(spawn_mode: SpawnMode) -> &'static str {
    match spawn_mode {
        SpawnMode::SingleSession => "single-session",
        SpawnMode::SameDir => "same-dir",
        SpawnMode::Worktree => "worktree",
    }
}

async fn daemon_main(args: &[String]) -> Result<()> {
    let command = args.get(1).map(String::as_str).unwrap_or("status");
    if is_help_at(args, 2) {
        print_daemon_help();
        return Ok(());
    }

    match command {
        "status" => {
            print_daemon_status(crate::bg::resident_daemon_status()?);
            Ok(())
        }
        "start" | "run" => {
            let state = crate::bg::start_resident_daemon()?;
            println!(
                "resident daemon {:?}\tpid={}\troot={}",
                state.status,
                state
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                state.root
            );
            Ok(())
        }
        "stop" => {
            let state = crate::bg::stop_resident_daemon()?;
            println!(
                "resident daemon {:?}\tpid={}\troot={}",
                state.status,
                state
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                state.root
            );
            Ok(())
        }
        "enqueue" | "queue" => {
            let prompt = daemon_prompt(args);
            if prompt.trim().is_empty() {
                return Err(anyhow!("usage: kiana daemon enqueue <prompt>"));
            }
            let task = crate::bg::create_task(prompt, std::env::current_dir()?)?;
            println!("queued {}\t{:?}\t{}", task.id, task.status, task.prompt);
            Ok(())
        }
        "ps" | "tasks" => {
            print_background_tasks(&crate::bg::list_tasks()?)?;
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_daemon_help();
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown daemon command '{}'\n\nUsage: kiana daemon [status|start|stop|enqueue|ps]",
            command
        )),
    }
}

async fn bg_handler(args: &[String]) -> Result<()> {
    if args.iter().any(|arg| is_help_arg(arg)) {
        print_background_help();
        return Ok(());
    }

    match args.first().map(String::as_str) {
        Some("ps") => {
            print_background_tasks(&crate::bg::list_tasks()?)?;
        }
        Some("logs") => {
            let task_id = args
                .get(1)
                .ok_or_else(|| anyhow!("usage: kiana logs <task_id>"))?;
            print!("{}", crate::bg::read_logs(task_id)?);
        }
        Some("attach") => {
            let task_id = args
                .get(1)
                .ok_or_else(|| anyhow!("usage: kiana attach <task_id>"))?;
            let task = crate::bg::read_task(&crate::bg::default_root(), task_id)?;
            println!("{}", serde_json::to_string_pretty(&task)?);
            println!();
            print!("{}", crate::bg::read_logs(task_id).unwrap_or_default());
        }
        Some("kill") => {
            let task_id = args
                .get(1)
                .ok_or_else(|| anyhow!("usage: kiana kill <task_id>"))?;
            let task = crate::bg::kill_task(task_id)?;
            println!("killed {}\t{:?}", task.id, task.status);
        }
        _ if args.contains(&"--bg".to_string()) || args.contains(&"--background".to_string()) => {
            let prompt = background_prompt(args);
            let task = crate::bg::create_task(prompt, std::env::current_dir()?)?;
            let task = crate::bg::spawn_task(&task.id)?;
            println!(
                "{}\t{:?}\tpid={}",
                task.id,
                task.status,
                task.pid.unwrap_or(0)
            );
        }
        _ => {
            return Err(anyhow!(
                "usage: kiana --bg <prompt> | kiana ps | kiana logs <task_id> | kiana attach <task_id> | kiana kill <task_id>"
            ));
        }
    }

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct ResumeCliArgs {
    mode: ResumeCliMode,
    message: Option<String>,
    execute: bool,
    fork_session: bool,
    output_format: PrintOutputFormat,
    json_schema: Option<Value>,
    include_partial_messages: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum ResumeCliMode {
    ContinueLatest,
    ResumeSession(String),
}

fn parse_resume_cli_args(args: &[String]) -> Result<Option<ResumeCliArgs>> {
    if args.is_empty() {
        return Ok(None);
    }

    let mut index = 0;
    let mut mode = None;
    let mut initial_prompt = None;
    let mut execute = true;
    let mut fork_session = false;
    let mut output_format = PrintOutputFormat::Text;
    let mut json_schema = None;
    let mut include_partial_messages = false;
    let mut message_start = None;

    while let Some(arg) = args.get(index).map(String::as_str) {
        match arg {
            "-p" | "--print" => {
                index += 1;
                continue;
            }
            _ if arg.starts_with("--print=") => {
                initial_prompt = arg.strip_prefix("--print=").map(str::to_string);
            }
            "-c" | "--continue" => {
                set_resume_mode(&mut mode, ResumeCliMode::ContinueLatest)?;
            }
            "-r" | "--resume" => {
                let session_id = args
                    .get(index + 1)
                    .filter(|value| !value.trim().is_empty())
                    .filter(|value| !value.starts_with('-'))
                    .ok_or_else(resume_usage)?
                    .clone();
                set_resume_mode(&mut mode, ResumeCliMode::ResumeSession(session_id))?;
                index += 2;
                continue;
            }
            _ if arg.starts_with("--resume=") => {
                let session_id = arg
                    .strip_prefix("--resume=")
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(resume_usage)?;
                set_resume_mode(
                    &mut mode,
                    ResumeCliMode::ResumeSession(session_id.to_string()),
                )?;
            }
            "--fork-session" => fork_session = true,
            "--record-only" => execute = false,
            "--execute" => execute = true,
            "--include-partial-messages" => include_partial_messages = true,
            "--output-format" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--output-format requires a value"))?;
                output_format = parse_print_output_format(value)?;
                index += 2;
                continue;
            }
            _ if arg.starts_with("--output-format=") => {
                let value = arg
                    .strip_prefix("--output-format=")
                    .ok_or_else(|| anyhow!("--output-format requires a value"))?;
                output_format = parse_print_output_format(value)?;
            }
            "--json-schema" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| anyhow!("--json-schema requires a JSON schema value"))?;
                json_schema = Some(parse_json_schema_flag(value)?);
                index += 2;
                continue;
            }
            _ if arg.starts_with("--json-schema=") => {
                let value = arg
                    .strip_prefix("--json-schema=")
                    .ok_or_else(|| anyhow!("--json-schema requires a JSON schema value"))?;
                json_schema = Some(parse_json_schema_flag(value)?);
            }
            "--" => {
                index += 1;
                message_start = Some(index);
                break;
            }
            _ if arg.starts_with('-') && mode.is_none() => return Ok(None),
            _ => {
                if mode.is_none() {
                    return Ok(None);
                }
                message_start = Some(index);
                break;
            }
        }
        index += 1;
    }

    let Some(mode) = mode else {
        return Ok(None);
    };

    let mut message_parts = Vec::new();
    if let Some(prompt) = initial_prompt.filter(|prompt| !prompt.trim().is_empty()) {
        message_parts.push(prompt);
    }
    message_parts.extend(
        args.get(message_start.unwrap_or(index)..)
            .unwrap_or_default()
            .iter()
            .cloned(),
    );
    let message = (!message_parts.is_empty())
        .then(|| message_parts.join(" "))
        .filter(|message| !message.trim().is_empty());
    if include_partial_messages && !matches!(output_format, PrintOutputFormat::StreamJson) {
        return Err(anyhow!(
            "--include-partial-messages requires --output-format=stream-json"
        ));
    }

    Ok(Some(ResumeCliArgs {
        mode,
        message,
        execute,
        fork_session,
        output_format,
        json_schema,
        include_partial_messages,
    }))
}

fn set_resume_mode(mode: &mut Option<ResumeCliMode>, next: ResumeCliMode) -> Result<()> {
    if mode.replace(next).is_some() {
        return Err(anyhow!("use only one of --continue or --resume"));
    }
    Ok(())
}

fn resume_usage() -> anyhow::Error {
    anyhow!("{}", resume_usage_text())
}

fn resume_usage_text() -> &'static str {
    "Usage: kiana --continue [prompt]\n       kiana --resume <session_id> [prompt]"
}

async fn resume_cli_main(args: ResumeCliArgs, runtime_flags: &RuntimeFlags) -> Result<()> {
    if runtime_flags.no_session_persistence {
        return Err(anyhow!(
            "--no-session-persistence cannot be used with --continue or --resume"
        ));
    }

    let mut session_id = match args.mode {
        ResumeCliMode::ContinueLatest => latest_session_id().await?,
        ResumeCliMode::ResumeSession(session_id) => session_id,
    };

    if args.fork_session {
        let mut fork_options = HashMap::new();
        apply_session_runtime_options(&mut fork_options, runtime_flags);
        session_id = crate::sdk::fork_session_with_options(session_id, fork_options).await?;
    } else {
        if runtime_flags.session_id.is_some() {
            return Err(anyhow!(
                "--session-id can only be used with --continue or --resume when --fork-session is set"
            ));
        }
        if let Some(session_name) = &runtime_flags.session_name {
            crate::sdk::rename_session(session_id.clone(), session_name.clone()).await?;
        }
    }

    if let Some(message) = args.message {
        let mut options = HashMap::new();
        options.insert("session_id".to_string(), Value::String(session_id));
        options.insert("execute".to_string(), Value::Bool(args.execute));
        let initial_prompt = apply_prompt_runtime_options(&mut options, runtime_flags)?;
        if let Some(json_schema) = args.json_schema {
            options.insert("json_schema".to_string(), json_schema);
        }

        let started = Instant::now();
        let message = prepend_initial_prompt(message, initial_prompt.as_deref());
        if args.include_partial_messages {
            return print_streaming_partial_result(message, options, Vec::new()).await;
        }
        let result = crate::sdk::unstable_v2_prompt(message, options).await?;
        let output = format_print_result_with_duration(
            &result,
            &args.output_format,
            started.elapsed().as_millis() as u64,
            &[],
        )
        .await?;
        if !output.is_empty() {
            println!("{}", output);
        }
        return Ok(());
    }

    print_shared_session_show(&session_id).await
}

async fn latest_session_id() -> Result<String> {
    let sessions = crate::sdk::list_sessions().await?;
    if sessions.is_empty() {
        return Err(anyhow!("no local SDK sessions found"));
    }

    let current_cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let has_cwd_metadata = sessions.iter().any(|session| session.cwd.is_some());
    let cwd_sessions = sessions
        .iter()
        .filter(|session| session.cwd.as_deref() == Some(current_cwd.as_str()))
        .collect::<Vec<_>>();
    let candidates = if !cwd_sessions.is_empty() {
        cwd_sessions
    } else if has_cwd_metadata {
        return Err(anyhow!(
            "No conversation found to continue in current directory: {}",
            current_cwd
        ));
    } else {
        sessions.iter().collect::<Vec<_>>()
    };

    candidates
        .iter()
        .find(|session| session.assistant_message_count > 0)
        .copied()
        .or_else(|| candidates.first().copied())
        .map(|session| session.session_id.clone())
        .ok_or_else(|| anyhow!("no local SDK sessions found"))
}

async fn print_shared_session_show(session_id: &str) -> Result<()> {
    let result = run_local_session_command(&["show".to_string(), session_id.to_string()]).await?;
    if !result.value.is_empty() {
        println!("{}", result.value);
    }
    Ok(())
}

async fn session_main(args: &[String]) -> Result<()> {
    let command = args.first().map(String::as_str).unwrap_or("list");

    match command {
        "help" | "--help" | "-h" => {
            println!("{}", session_usage());
        }
        "list" | "status" | "path" | "current" | "show" | "rename" | "tag" | "files"
        | "file-set" | "file-sets" | "fork" | "delete" | "rm" | "import" | "export" | "compact" => {
            if is_session_subcommand_help(args) {
                println!("{}", session_usage());
                return Ok(());
            }
            let result = run_local_session_command(args).await?;
            if !result.value.is_empty() {
                println!("{}", result.value);
            }
        }
        "new" if is_session_subcommand_help(args) => {
            println!("{}", session_new_usage());
        }
        "new" => {
            let mut options = HashMap::new();
            let title = args.get(1..).unwrap_or_default().join(" ");
            if !title.trim().is_empty() {
                options.insert("title".to_string(), Value::String(title));
            }
            let session = crate::sdk::unstable_v2_create_session(options).await?;
            println!("{}", session.session_id);
        }
        "reply" if is_session_subcommand_help(args) => {
            println!("{}", session_reply_usage_text());
        }
        "reply" => {
            let reply = parse_reply_args(args)?;
            if !reply.execute && reply.json_schema.is_none() {
                let result = run_local_session_command(args).await?;
                if !result.value.is_empty() {
                    println!("{}", result.value);
                }
                return Ok(());
            }
            let mut options = HashMap::new();
            options.insert("session_id".to_string(), Value::String(reply.session_id));
            options.insert("execute".to_string(), Value::Bool(reply.execute));
            if let Some(json_schema) = reply.json_schema {
                options.insert("json_schema".to_string(), json_schema);
            }
            let started = Instant::now();
            let result = crate::sdk::unstable_v2_prompt(reply.message, options).await?;
            let output = format_print_result_with_duration(
                &result,
                &PrintOutputFormat::Text,
                started.elapsed().as_millis() as u64,
                &[],
            )
            .await?;
            if !output.is_empty() {
                println!("{}", output);
            }
        }
        _ => {
            return Err(anyhow!(
                "unknown session command '{}'\n\n{}",
                command,
                session_usage()
            ));
        }
    }

    Ok(())
}

async fn run_local_session_command(args: &[String]) -> Result<kiana_commands::CommandResult> {
    let registry = create_default_command_registry();
    let command = registry
        .get("session")
        .ok_or_else(|| anyhow!("session command is not registered"))?;
    command
        .execute(CommandContext {
            args: args.join(" "),
            app_state: HashMap::new(),
        })
        .await
}

fn session_usage() -> &'static str {
    "Usage: kiana session <new|list|status|path|current|reply|show|rename|tag|files|fork|delete|import|export|compact>\n\
     Commands:\n\
       new [title]                 Create a local SDK session\n\
       list                        List local SDK sessions\n\
       status                      List local SDK sessions with current marker when available\n\
       path                        Print the SDK sessions directory\n\
       current                     Show the current REPL session when available\n\
       reply <id> [options] <msg>  Run or record a prompt in a session\n\
       show <id>                   Show a session and its messages\n\
       rename <id> <title>         Rename a session\n\
       tag <id> [tag]              Set or clear a session tag\n\
       files <id>                  Show or update editable/read-only file sets\n\
       fork <id>                   Fork a session\n\
       delete <id>                 Delete a local SDK session\n\
       import <path> [--force]     Import a local SDK session JSON file\n\
       export <id> [options]       Export a local SDK session\n\
       compact <id> [options]      Compact a local SDK session"
}

fn is_session_subcommand_help(args: &[String]) -> bool {
    args.len() == 2 && matches!(args[1].as_str(), "help" | "--help" | "-h")
}

fn is_help_arg(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h")
}

fn is_help_at(args: &[String], index: usize) -> bool {
    args.get(index).is_some_and(|arg| is_help_arg(arg))
}

fn is_mcp_serve_command(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg == "mcp") && args.get(1).is_some_and(|arg| arg == "serve")
}

fn remote_session_command_uses_top_level_help(command: &str) -> bool {
    matches!(
        command,
        "status"
            | "url"
            | "ws-url"
            | "websocket-url"
            | "list"
            | "ls"
            | "show"
            | "get"
            | "inspect"
            | "rename"
            | "title"
            | "create"
            | "new"
            | "start"
            | "archive"
            | "delete"
            | "close"
            | "listen"
            | "connect"
            | "tail"
            | "send"
            | "message"
    )
}

fn remote_session_code_session_command_uses_help(command: &str) -> bool {
    matches!(
        command,
        "create"
            | "new"
            | "start"
            | "bridge"
            | "credentials"
            | "creds"
            | "smoke"
            | "live-smoke"
            | "hydrate"
            | "restore"
            | "sdk-url"
            | "url"
    )
}

fn session_new_usage() -> &'static str {
    "Usage: kiana new [title]\n       kiana session new [title]"
}

fn session_reply_usage_text() -> &'static str {
    "Usage: kiana reply <session_id> [--record-only] [--json-schema <schema>] <message>\n       kiana session reply <session_id> [--record-only] [--json-schema <schema>] <message>"
}

#[derive(Debug, PartialEq, Eq)]
struct ReplyArgs {
    session_id: String,
    message: String,
    execute: bool,
    json_schema: Option<Value>,
}

fn parse_reply_args(args: &[String]) -> Result<ReplyArgs> {
    let mut record_only = false;
    let mut json_schema = None;
    let mut index = 1;

    parse_reply_flags(args, &mut index, &mut record_only, &mut json_schema)?;
    let session_id = args.get(index).cloned().ok_or_else(reply_usage)?;
    index += 1;
    parse_reply_flags(args, &mut index, &mut record_only, &mut json_schema)?;
    if args.get(index).map(String::as_str) == Some("--") {
        index += 1;
    }

    let message = args.get(index..).unwrap_or_default().join(" ");
    if message.trim().is_empty() {
        return Err(reply_usage());
    }

    Ok(ReplyArgs {
        session_id,
        message,
        execute: !record_only,
        json_schema,
    })
}

fn reply_usage() -> anyhow::Error {
    anyhow!("{}", session_reply_usage_text())
}

fn parse_reply_flags(
    args: &[String],
    index: &mut usize,
    record_only: &mut bool,
    json_schema: &mut Option<Value>,
) -> Result<()> {
    while let Some(arg) = args.get(*index).map(String::as_str) {
        match arg {
            "--record-only" => *record_only = true,
            "--execute" => *record_only = false,
            "--json-schema" => {
                let value = args
                    .get(*index + 1)
                    .ok_or_else(|| anyhow!("--json-schema requires a JSON schema value"))?;
                *json_schema = Some(parse_json_schema_flag(value)?);
                *index += 2;
                continue;
            }
            _ if arg.starts_with("--json-schema=") => {
                let value = arg
                    .strip_prefix("--json-schema=")
                    .ok_or_else(|| anyhow!("--json-schema requires a JSON schema value"))?;
                *json_schema = Some(parse_json_schema_flag(value)?);
            }
            _ => break,
        }
        *index += 1;
    }
    Ok(())
}

fn parse_json_schema_flag(value: &str) -> Result<Value> {
    let schema: Value = serde_json::from_str(value)
        .map_err(|error| anyhow!("--json-schema must be valid JSON: {}", error))?;
    if !schema.is_object() {
        return Err(anyhow!("--json-schema must be a JSON object"));
    }
    Ok(schema)
}

async fn cli_main_with_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> Result<()> {
    ensure_repl_terminal(stdin_is_terminal, stdout_is_terminal)?;
    cli_main().await
}

fn ensure_repl_terminal(stdin_is_terminal: bool, stdout_is_terminal: bool) -> Result<()> {
    let guidance =
        "Use `kiana -p <prompt>` for scripts or `kiana tui` from a real terminal for the TUI.";
    match (stdin_is_terminal, stdout_is_terminal) {
        (true, true) => Ok(()),
        (false, false) => Err(anyhow!(
            "kiana requires an interactive terminal on stdin and stdout when no command is provided. {guidance}"
        )),
        (false, true) => Err(anyhow!(
            "kiana requires interactive stdin when no command is provided. {guidance}"
        )),
        (true, false) => Err(anyhow!(
            "kiana requires interactive stdout when no command is provided. {guidance}"
        )),
    }
}

async fn cli_main() -> Result<()> {
    crate::repl::run_repl().await
}

async fn run_local_command(
    args: &[String],
    runtime_flags: &RuntimeFlags,
) -> Result<Option<kiana_commands::CommandResult>> {
    let name = args[0].trim_start_matches('/');
    let command_args = args[1..].join(" ");
    let registry = create_default_command_registry();
    let Some(command) = registry.get(name) else {
        return Ok(None);
    };

    if matches!(command.command_type(), CommandType::Prompt)
        && !command.supports_non_interactive()
        && !is_local_help_args(&command_args)
    {
        return Err(anyhow!(
            "`{}` is an interactive prompt command; run it from the REPL as `/{}`.",
            name,
            name
        ));
    }

    let mut app_state = HashMap::new();
    app_state.insert(
        "cwd".to_string(),
        Value::String(std::env::current_dir()?.display().to_string()),
    );
    app_state.insert(
        COMMAND_ARGV_APP_STATE_KEY.to_string(),
        Value::Array(args[1..].iter().cloned().map(Value::String).collect()),
    );
    app_state.insert(
        crate::command_dispatch::APPROVE_LOCAL_WRITE_APP_STATE_KEY.to_owned(),
        Value::Bool(runtime_flags.approve_local_write),
    );
    let result = crate::command_dispatch::execute_command(
        command.as_ref(),
        CommandContext {
            args: command_args,
            app_state,
        },
    )
    .await?;
    Ok(Some(result))
}

fn is_local_help_args(args: &str) -> bool {
    matches!(args.trim(), "help" | "--help" | "-h")
}

fn print_help() {
    let registry = create_default_command_registry();
    let mut commands = registry.list();
    commands.sort_by(|a, b| a.name().cmp(b.name()));

    println!("Kiana Code {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("USAGE:");
    println!("  kiana                 Start the interactive REPL (requires a terminal)");
    println!("  kiana -p <prompt>     Run a non-interactive prompt and print the result");
    println!("  kiana -c [prompt]     Continue the most recent local SDK session");
    println!("  kiana -r <id> [prompt]  Resume a local SDK session");
    println!("  kiana --model <name> -p <prompt>  Override the model for this run");
    println!("  kiana --fallback-model <name> -p <prompt>  Retry overloads on another model");
    println!("  kiana --permission-profile <profile> -p <prompt>  Use read-only/workspace/full/ask/plan permissions");
    println!(
        "  kiana --approve-local-write <command>  Approve one daemon-issued local-write challenge"
    );
    println!("  kiana --permission-prompt-tool <tool> -p <prompt>  Use MCP approval in ask mode");
    println!("  kiana --agent <name> -p <prompt>  Use a configured custom agent prompt");
    println!("  kiana --agents '{{...}}' --agent <name> -p <prompt>  Use inline agent JSON");
    println!("  kiana --api-timeout <seconds> -p <prompt>  Override the API request timeout");
    println!("  kiana --max-turns <n> -p <prompt>  Limit agentic model/tool iterations");
    println!("  kiana --repair-checks -p <prompt>  Run isolated review/checks and ask the model to repair failures");
    println!("  kiana --tools <list> -p <prompt>  Limit tools; use core for common tools or \"\" to disable");
    println!("  kiana --settings <file-or-json>  Load extra config for this run");
    println!("  kiana --session-id <id> -p <prompt>  Use a specific local SDK session ID");
    println!("  kiana -p --resident-teammate <prompt>  Keep a team background agent polling inbox");
    println!("  kiana -n <name> -p <prompt>  Name the local SDK session");
    println!("  kiana --no-session-persistence -p <prompt>  Run without saving a session");
    println!("  kiana --bare -p <prompt>  Minimal scripted mode with inherited MCP disabled");
    println!("  kiana --add-dir <dir...> -p <prompt>  Allow tool access to extra directories");
    println!("  kiana --mcp-config <file-or-json> -p <prompt>  Load dynamic MCP servers");
    println!("  kiana --strict-mcp-config -p <prompt>  Ignore inherited MCP servers");
    println!("  kiana --system-prompt <text> -p <prompt>  Override the system prompt");
    println!("  kiana --append-system-prompt <text> -p <prompt>  Add system prompt context");
    println!("  kiana --help          Show this help");
    println!("  kiana --version       Show version");
    println!("  kiana <command>       Run a local command when supported");
    println!("  kiana auth status     Inspect configured authentication state");
    println!("  kiana architecture status  Inspect control-plane migration status");
    println!("  kiana license status  Inspect enterprise license readiness");
    println!("  kiana agents          List configured agents");
    println!("  kiana auto-mode defaults  Print default auto mode classifier rules");
    println!("  kiana completion bash Generate shell completion scripts");
    println!("  kiana open <cc-url>   Connect to a direct-connect server");
    println!("  kiana server          Start a local direct-connect server");
    println!("  kiana plugin install  Install a plugin from a marketplace or path");
    println!("  kiana session list    List local SDK sessions");
    println!("  kiana new [title]     Create a local SDK session");
    println!("  kiana reply <id> ...  Run a prompt in a local SDK session");
    println!("  kiana reply <id> --record-only ...  Append without model execution");
    println!("  kiana reply <id> --json-schema '{{...}}' ...  Request structured output");
    println!("  kiana tui             Start the ratatui app backed by SDK sessions");
    println!("  kiana --bg <prompt>   Run a prompt through the local background worker");
    println!("  kiana ps              List local background tasks");
    println!("  kiana daemon start    Start the resident background task supervisor");
    println!("  kiana daemon enqueue <prompt>  Queue work for the resident daemon");
    println!("  kiana mcp serve       Start a stdio MCP server exposing local tools");
    println!("  kiana mcp-server      Legacy alias for kiana mcp serve");
    println!("  kiana mcp-server-http Start an HTTP/SSE/WS MCP server exposing local tools");
    println!("  kiana mcp-server-sse  Start an SSE MCP server exposing local tools");
    println!("  kiana mcp-server-ws   Start a WebSocket MCP server exposing local tools");
    println!("  kiana --claude-in-chrome-mcp  Start a stdio browser automation MCP server");
    println!("  kiana computer-mcp    Start a stdio computer-use MCP server");
    println!("  kiana --chrome-native-host    Start the Chrome native messaging host");
    println!("  kiana chrome install-native-host  Install Chrome native host manifests");
    println!("  kiana url register [scheme]  Register this kiana binary as URL handler");
    println!("  kiana remote-session status  Inspect remote session configuration");
    println!("  kiana remote-session environments  Manage remote environments");
    println!(
        "  kiana remote-session code-session create --title <title>  Create a CCR v2 code session"
    );
    println!(
        "  kiana remote-session create --environment-id <id> <message>  Create a remote session"
    );
    println!("  kiana remote-session archive <id>  Archive a remote session");
    println!("  kiana remote-session listen  Connect to a remote session WebSocket");
    println!("  kiana remote-session send    Send a message event to a remote session");
    println!("  kiana bridge status   Inspect remote bridge configuration");
    println!();
    println!("LOCAL COMMANDS:");
    for command in commands {
        println!("  {:<14} {}", command.name(), command.description());
    }
    println!();
    println!("CONFIG:");
    println!(
        "  Set ANTHROPIC_API_KEY or create ~/.kiana/config.toml before sending model prompts."
    );
}

fn print_print_help() {
    println!("Usage: kiana -p [options] <prompt>");
    println!("       kiana --print [options] <prompt>");
    println!("       kiana --print=<prompt> [options]");
    println!();
    println!("Options:");
    println!("  --record-only             Record the prompt without model execution");
    println!("  --execute                 Execute the prompt with the model");
    println!("  --json-schema <schema>    Request structured JSON output");
    println!("  --input-format <format>   text or stream-json");
    println!("  --output-format <format>  text, json, or stream-json");
    println!("  --sdk-url <url>           SDK stream-json endpoint");
    println!("  --repair-checks           Run isolated review/checks and repair failures");
    println!("  --repair-check-attempts <n>  Limit repair feedback rounds");
}

fn print_resume_help() {
    println!("{}", resume_usage_text());
    println!();
    println!("Options:");
    println!("  --record-only             Record a prompt without model execution");
    println!("  --execute                 Execute the prompt with the model");
    println!("  --fork-session            Fork before continuing");
    println!("  --json-schema <schema>    Request structured JSON output");
    println!("  --output-format <format>  text, json, or stream-json");
    println!("  --repair-checks           Run isolated review/checks and repair failures");
    println!("  --repair-check-attempts <n>  Limit repair feedback rounds");
}

fn background_prompt(args: &[String]) -> String {
    args.iter()
        .filter(|arg| arg.as_str() != "--bg" && arg.as_str() != "--background")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn daemon_prompt(args: &[String]) -> String {
    args.iter()
        .skip(2)
        .filter(|arg| arg.as_str() != "--")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_daemon_status(state: Option<crate::bg::ResidentDaemonState>) {
    println!(
        "Background task root: {}",
        crate::bg::default_root().display()
    );
    match state {
        Some(state) => {
            println!(
                "Resident daemon: {:?}\tpid={}\tupdated={}",
                state.status,
                state
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                state.updated_at
            );
            if let Some(error) = state.last_error.filter(|error| !error.trim().is_empty()) {
                println!("Last daemon error: {}", error);
            }
        }
        None => println!("Resident daemon: Stopped\tpid=-"),
    }
}

fn print_daemon_help() {
    println!("Usage: kiana daemon <status|start|stop|enqueue|ps>");
    println!();
    println!("Commands:");
    println!("  status              Show resident daemon state");
    println!("  start               Start the resident background task supervisor");
    println!("  stop                Stop the resident background task supervisor");
    println!("  enqueue <prompt>    Queue a prompt for the resident daemon");
    println!("  ps                  List background tasks");
}

fn print_background_help() {
    println!(
        "Usage: kiana --bg <prompt> | kiana ps | kiana logs <task_id> | kiana attach <task_id> | kiana kill <task_id>"
    );
    println!();
    println!("Commands:");
    println!("  --bg <prompt>       Queue a background prompt");
    println!("  ps                  List background tasks");
    println!("  logs <task_id>      Print task logs");
    println!("  attach <task_id>    Show task metadata and logs");
    println!("  kill <task_id>      Kill a running background task");
}

fn print_background_tasks(tasks: &[crate::bg::BackgroundTask]) -> Result<()> {
    if tasks.is_empty() {
        println!("No background tasks.");
        return Ok(());
    }

    for task in tasks {
        println!(
            "{}\t{:?}\tpid={}\tupdated={}\t{}",
            task.id,
            task.status,
            task.pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_string()),
            task.updated_at,
            task.prompt
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
    use std::collections::{BTreeSet, HashMap};
    use std::io;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc as StdArc, Mutex as StdMutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::AsyncWriteExt;

    #[derive(Clone)]
    struct BridgeLoopPermissionWriter {
        state: StdArc<StdMutex<BridgeLoopWriterState>>,
        input: StdArc<AsyncMutex<tokio::io::DuplexStream>>,
    }

    struct BridgeLoopWriterState {
        pending: Vec<u8>,
        lines: Vec<Value>,
    }

    #[derive(Debug)]
    struct PrintToolLoopState {
        write_path: String,
        requests: Vec<Value>,
    }

    #[derive(Debug)]
    struct PrintMultiToolLoopState {
        note_path: String,
        requests: Vec<Value>,
    }

    struct EnvSnapshot {
        values: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    struct CurrentDirGuard(PathBuf);

    impl EnvSnapshot {
        fn take(keys: &[&'static str]) -> Self {
            let values = keys
                .iter()
                .map(|key| (*key, take_env(key)))
                .collect::<Vec<_>>();
            Self { values }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in self.values.drain(..) {
                restore_env(key, value);
            }
        }
    }

    impl CurrentDirGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self(previous)
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    impl BridgeLoopPermissionWriter {
        fn new(input: StdArc<AsyncMutex<tokio::io::DuplexStream>>) -> Self {
            Self {
                state: StdArc::new(StdMutex::new(BridgeLoopWriterState {
                    pending: Vec::new(),
                    lines: Vec::new(),
                })),
                input,
            }
        }

        fn lines(&self) -> Vec<Value> {
            self.state.lock().unwrap().lines.clone()
        }
    }

    impl Write for BridgeLoopPermissionWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut completed_events = Vec::new();
            {
                let mut state = self.state.lock().unwrap();
                state.pending.extend_from_slice(buf);
                while let Some(position) = state.pending.iter().position(|byte| *byte == b'\n') {
                    let line = state.pending.drain(..=position).collect::<Vec<_>>();
                    let line = String::from_utf8(line)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let event: Value = serde_json::from_str(line)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                    state.lines.push(event.clone());
                    completed_events.push(event);
                }
            }

            for event in completed_events {
                if event.get("type").and_then(Value::as_str) == Some("control_request")
                    && event
                        .get("request")
                        .and_then(|request| request.get("subtype"))
                        .and_then(Value::as_str)
                        == Some("can_use_tool")
                {
                    let request_id = event
                        .get("request_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let input = self.input.clone();
                    tokio::spawn(async move {
                        let response = serde_json::json!({
                            "type": "control_response",
                            "response": {
                                "subtype": "success",
                                "request_id": request_id,
                                "response": {
                                    "allowed": true
                                }
                            }
                        })
                        .to_string();
                        let mut input = input.lock().await;
                        let _ = input.write_all(response.as_bytes()).await;
                        let _ = input.write_all(b"\n").await;
                        let _ = input.shutdown().await;
                    });
                }
            }

            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn write_test_sdk_session(root: &Path, session_id: &str, updated_at: u64, messages: Value) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(
            root.join(format!("{session_id}.json")),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": session_id,
                "title": session_id,
                "tag": null,
                "parent_session_id": null,
                "created_at": 1,
                "updated_at": updated_at,
                "messages": messages
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn repl_terminal_guard_rejects_non_interactive_stdio_before_session_setup() {
        let error = ensure_repl_terminal(false, false).unwrap_err().to_string();

        assert!(error.contains("interactive terminal"));
        assert!(error.contains("kiana -p"));
        assert!(error.contains("kiana tui"));
    }

    #[test]
    fn repl_terminal_guard_names_non_interactive_side() {
        let stdin_error = ensure_repl_terminal(false, true).unwrap_err().to_string();
        let stdout_error = ensure_repl_terminal(true, false).unwrap_err().to_string();

        assert!(stdin_error.contains("interactive stdin"));
        assert!(stdout_error.contains("interactive stdout"));
    }

    fn assert_environment_variable_update_rejects_atomically(blocked_key: &'static str) {
        const SAFE_KEY: &str = "APP_ENV_UPDATE_SAFE_TEST";

        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[blocked_key, SAFE_KEY]);
        let original_blocked_value = format!("original-{blocked_key}");
        std::env::set_var(blocked_key, &original_blocked_value);
        std::env::set_var(SAFE_KEY, "safe-before");
        let mut variables = serde_json::Map::new();
        variables.insert(
            blocked_key.to_string(),
            Value::String("attacker".to_string()),
        );
        variables.insert(
            SAFE_KEY.to_string(),
            Value::String("safe-after".to_string()),
        );

        let result = apply_environment_variable_update(&serde_json::json!({
            "type": "update_environment_variables",
            "variables": variables,
        }));

        assert_eq!(std::env::var(SAFE_KEY).unwrap(), "safe-before");
        assert_eq!(std::env::var(blocked_key).unwrap(), original_blocked_value);
        let error = result.unwrap_err().to_string();
        assert!(error.contains(blocked_key), "{error}");
    }

    #[test]
    fn environment_variable_update_rejects_kiana_home_atomically() {
        assert_environment_variable_update_rejects_atomically("KIANA_HOME");
    }

    #[test]
    fn environment_variable_update_rejects_home_atomically() {
        assert_environment_variable_update_rejects_atomically("HOME");
    }

    #[test]
    fn environment_variable_update_rejects_userprofile_atomically() {
        assert_environment_variable_update_rejects_atomically("USERPROFILE");
    }

    #[test]
    fn environment_variable_update_rejects_mixed_case_trust_authority_keys_atomically() {
        for blocked_key in ["kiana_home", "Home", "userProfile"] {
            assert_environment_variable_update_rejects_atomically(blocked_key);
        }
    }

    #[test]
    fn environment_variable_update_rejects_runtime_authority_keys_atomically() {
        for blocked_key in [
            "KIANA_HOOKS",
            "KIANA_HOOKS_FILE",
            "KIANA_SESSION_START_HOOKS",
            "KIANA_PLUGINS_DIR",
            "KIANA_MCP_SERVERS_JSON",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_MANAGED_SETTINGS_FILE",
            "KIANA_AGENT_COMMAND",
            "KIANA_AGENT_HOOKS",
            "KIANA_BASH",
            "KIANA_BASH_PATH",
            "KIANA_POWERSHELL",
            "KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED",
            "KIANA_FUTURE_RUNTIME_CONTROL",
        ] {
            assert_environment_variable_update_rejects_atomically(blocked_key);
        }
    }

    async fn start_bridge_loop_mock_model_server() -> (String, tokio::task::JoinHandle<()>) {
        async fn handle_bridge_loop_mock_model_request(
            State(calls): State<StdArc<AtomicUsize>>,
            Json(_body): Json<Value>,
        ) -> impl IntoResponse {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let content = if call == 0 {
                serde_json::json!([{
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "TaskCreate",
                    "input": {
                        "title": "create a task"
                    }
                }])
            } else {
                serde_json::json!([{
                    "type": "text",
                    "text": "task created"
                }])
            };
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": "msg_bridge_smoke",
                    "model": "mock-model",
                    "role": "assistant",
                    "content": content,
                    "stop_reason": if call == 0 { "tool_use" } else { "end_turn" },
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response()
        }

        let app = axum::Router::new()
            .route(
                "/v1/messages",
                axum::routing::post(handle_bridge_loop_mock_model_request),
            )
            .with_state(StdArc::new(AtomicUsize::new(0)));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), server)
    }

    async fn start_direct_connect_text_mock_model_server() -> (
        String,
        StdArc<StdMutex<Vec<Value>>>,
        tokio::task::JoinHandle<()>,
    ) {
        async fn handle_direct_connect_text_mock_model_request(
            State(requests): State<StdArc<StdMutex<Vec<Value>>>>,
            Json(body): Json<Value>,
        ) -> impl IntoResponse {
            requests.lock().unwrap().push(body);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": "msg_direct_connect_text",
                    "model": "mock-model",
                    "role": "assistant",
                    "content": [{
                        "type": "text",
                        "text": "direct server done"
                    }],
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response()
        }

        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let app = axum::Router::new()
            .route(
                "/v1/messages",
                axum::routing::post(handle_direct_connect_text_mock_model_request),
            )
            .with_state(requests.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), requests, server)
    }

    async fn start_print_tool_loop_mock_model_server(
        write_path: String,
    ) -> (
        String,
        StdArc<StdMutex<PrintToolLoopState>>,
        tokio::task::JoinHandle<()>,
    ) {
        async fn handle_print_tool_loop_mock_model_request(
            State(state): State<StdArc<StdMutex<PrintToolLoopState>>>,
            Json(body): Json<Value>,
        ) -> impl IntoResponse {
            let (call_count, write_path) = {
                let mut state = state.lock().unwrap();
                state.requests.push(body);
                (state.requests.len(), state.write_path.clone())
            };
            let content = if call_count == 1 {
                serde_json::json!([
                    {
                        "type": "text",
                        "text": "writing file"
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_cli_write",
                        "name": "Write",
                        "input": {
                            "file_path": write_path,
                            "content": "hello from cli\n"
                        }
                    }
                ])
            } else {
                serde_json::json!([{
                    "type": "text",
                    "text": "cli write complete"
                }])
            };
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": "msg_cli_tool_loop",
                    "model": "mock-cli-model",
                    "role": "assistant",
                    "content": content,
                    "stop_reason": if call_count == 1 { "tool_use" } else { "end_turn" },
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response()
        }

        let state = StdArc::new(StdMutex::new(PrintToolLoopState {
            write_path,
            requests: Vec::new(),
        }));
        let app = axum::Router::new()
            .route(
                "/v1/messages",
                axum::routing::post(handle_print_tool_loop_mock_model_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn start_print_multi_tool_loop_mock_model_server(
        note_path: String,
    ) -> (
        String,
        StdArc<StdMutex<PrintMultiToolLoopState>>,
        tokio::task::JoinHandle<()>,
    ) {
        async fn handle_print_multi_tool_loop_mock_model_request(
            State(state): State<StdArc<StdMutex<PrintMultiToolLoopState>>>,
            Json(body): Json<Value>,
        ) -> impl IntoResponse {
            let (call_count, note_path) = {
                let mut state = state.lock().unwrap();
                state.requests.push(body);
                (state.requests.len(), state.note_path.clone())
            };
            let content = match call_count {
                1 => serde_json::json!([{
                    "type": "tool_use",
                    "id": "toolu_cli_read",
                    "name": "Read",
                    "input": {
                        "file_path": note_path
                    }
                }]),
                2 => serde_json::json!([{
                    "type": "tool_use",
                    "id": "toolu_cli_edit",
                    "name": "Edit",
                    "input": {
                        "file_path": note_path,
                        "old_string": "alpha",
                        "new_string": "beta"
                    }
                }]),
                3 => serde_json::json!([{
                    "type": "tool_use",
                    "id": "toolu_cli_bash",
                    "name": "Bash",
                    "input": {
                        "command": format!("cat {}", shell_single_quote(&note_path))
                    }
                }]),
                _ => serde_json::json!([{
                    "type": "text",
                    "text": "multi-tool flow complete"
                }]),
            };
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "id": format!("msg_cli_multi_tool_{call_count}"),
                    "model": "mock-cli-model",
                    "role": "assistant",
                    "content": content,
                    "stop_reason": if call_count < 4 { "tool_use" } else { "end_turn" },
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response()
        }

        let state = StdArc::new(StdMutex::new(PrintMultiToolLoopState {
            note_path,
            requests: Vec::new(),
        }));
        let app = axum::Router::new()
            .route(
                "/v1/messages",
                axum::routing::post(handle_print_multi_tool_loop_mock_model_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    fn shell_single_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn clear_settings_env() {
        std::env::remove_var("KIANA_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_JSON");
    }

    fn clear_access_roots_env() {
        std::env::remove_var(kiana_tools::tool::ACCESS_ROOTS_ENV);
    }

    fn take_env(name: &str) -> Option<std::ffi::OsString> {
        let value = std::env::var_os(name);
        std::env::remove_var(name);
        value
    }

    fn restore_env(name: &str, value: Option<std::ffi::OsString>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }

    fn clear_mcp_servers_env() {
        std::env::remove_var(kiana_tools::mcp_tool::MCP_SERVERS_ENV);
    }

    fn clear_simple_env() {
        std::env::remove_var("CLAUDE_CODE_SIMPLE");
        std::env::remove_var("KIANA_CODE_SIMPLE");
    }

    #[test]
    fn bridge_config_reads_git_metadata_from_configured_dir() {
        if std::process::Command::new("git")
            .arg("--version")
            .output()
            .is_err()
        {
            return;
        }
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        let root = std::env::temp_dir().join(format!("kiana-bridge-git-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .arg("init")
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["checkout", "-b", "bridge-test"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["remote", "add", "origin", "https://example.test/repo.git"])
            .output()
            .unwrap();

        let config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--dir".to_string(),
            root.to_string_lossy().to_string(),
        ])
        .unwrap();

        assert_eq!(config.branch, "bridge-test");
        assert_eq!(
            config.git_repo_url.as_deref(),
            Some("https://example.test/repo.git")
        );
        let _ = std::fs::remove_dir_all(root);
        clear_bridge_env();
    }

    #[test]
    fn bridge_config_accepts_heartbeat_interval_override() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        std::env::set_var("KIANA_BRIDGE_HEARTBEAT_INTERVAL_MS", "45000");

        let env_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("env heartbeat interval should parse");
        assert_eq!(env_config.heartbeat_interval_ms, 45_000);

        let arg_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--heartbeat-interval-ms".to_string(),
            "0".to_string(),
        ])
        .expect("argument heartbeat interval should parse");
        assert_eq!(arg_config.heartbeat_interval_ms, 0);

        clear_bridge_env();
    }

    #[test]
    fn bridge_config_accepts_ccr_v2_sse_reconnect_budget_override() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        std::env::set_var("KIANA_BRIDGE_CCR_V2_SSE_RECONNECT_GIVE_UP_MS", "2500");

        let env_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("env CCR v2 SSE reconnect budget should parse");
        assert_eq!(env_config.ccr_v2_sse_reconnect_give_up_ms, Some(2500));

        let arg_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--ccr-v2-sse-reconnect-give-up-ms".to_string(),
            "0".to_string(),
        ])
        .expect("argument CCR v2 SSE reconnect budget should parse");
        assert_eq!(arg_config.ccr_v2_sse_reconnect_give_up_ms, Some(0));

        clear_bridge_env();
    }

    #[test]
    fn bridge_config_accepts_ccr_v2_sse_liveness_timeout_override() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        std::env::set_var("KIANA_BRIDGE_CCR_V2_SSE_LIVENESS_TIMEOUT_MS", "45000");

        let env_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("env CCR v2 SSE liveness timeout should parse");
        assert_eq!(env_config.ccr_v2_sse_liveness_timeout_ms, Some(45_000));

        let arg_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--ccr-v2-sse-liveness-timeout-ms".to_string(),
            "0".to_string(),
        ])
        .expect("argument CCR v2 SSE liveness timeout should parse");
        assert_eq!(arg_config.ccr_v2_sse_liveness_timeout_ms, Some(0));

        clear_bridge_env();
    }

    #[test]
    fn bridge_config_accepts_permission_mode_overrides() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        std::env::set_var("KIANA_PERMISSION_MODE", "ask");

        let env_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("env permission mode should parse");
        assert_eq!(env_config.permission_mode.as_deref(), Some("ask"));

        let arg_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--permission-mode".to_string(),
            "acceptEdits".to_string(),
        ])
        .expect("argument permission mode should parse");
        assert_eq!(arg_config.permission_mode.as_deref(), Some("acceptEdits"));

        clear_bridge_env();
    }

    #[test]
    fn bridge_config_accepts_session_timeout_overrides() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        let default_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("default bridge config should parse");
        assert_eq!(default_config.session_timeout_ms, 24 * 60 * 60 * 1000);

        let seconds_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--session-timeout".to_string(),
            "30".to_string(),
        ])
        .expect("seconds timeout should parse");
        assert_eq!(seconds_config.session_timeout_ms, 30_000);

        let disabled_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--session-timeout-ms=0".to_string(),
        ])
        .expect("zero timeout should disable watchdog");
        assert_eq!(disabled_config.session_timeout_ms, 0);

        std::env::set_var("KIANA_BRIDGE_SESSION_TIMEOUT_MS", "2500");
        let env_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("env timeout should parse");
        assert_eq!(env_config.session_timeout_ms, 2500);

        clear_bridge_env();
    }

    #[test]
    fn bridge_config_accepts_debug_file_override() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        std::env::set_var("KIANA_BRIDGE_DEBUG_FILE", "/tmp/env-bridge.log");

        let env_config = build_bridge_config(&["bridge".to_string(), "status".to_string()])
            .expect("env debug file should parse");
        assert_eq!(
            env_config.debug_file.as_deref(),
            Some("/tmp/env-bridge.log")
        );

        let arg_config = build_bridge_config(&[
            "bridge".to_string(),
            "status".to_string(),
            "--debug-file=/tmp/arg-bridge.log".to_string(),
        ])
        .expect("argument debug file should parse");
        assert_eq!(
            arg_config.debug_file.as_deref(),
            Some("/tmp/arg-bridge.log")
        );

        clear_bridge_env();
    }

    #[tokio::test]
    async fn cli_bridge_auth_provider_refresh_command_updates_access_token() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        std::env::set_var("KIANA_BRIDGE_ACCESS_TOKEN", "stale-token");
        let refresh_command = if cfg!(windows) {
            "echo fresh-token"
        } else {
            "printf fresh-token"
        };
        std::env::set_var("KIANA_BRIDGE_REFRESH_COMMAND", refresh_command);

        let provider = CliBridgeAuthProvider;
        assert_eq!(provider.access_token().as_deref(), Some("stale-token"));
        assert!(provider
            .refresh_after_unauthorized("stale-token")
            .await
            .unwrap());
        assert_eq!(provider.access_token().as_deref(), Some("fresh-token"));

        clear_bridge_env();
    }

    #[test]
    fn bridge_access_token_uses_oauth_file_when_env_tokens_are_missing() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        let token_path = temp_file_path("bridge-oauth-token", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "bridge-oauth-access".to_string(),
            refresh_token: Some("bridge-oauth-refresh".to_string()),
            expires_at: None,
        })
        .unwrap();

        assert_eq!(
            bridge_access_token().as_deref(),
            Some("bridge-oauth-access")
        );

        let _ = std::fs::remove_file(token_path);
        clear_bridge_env();
    }

    #[tokio::test]
    async fn bridge_access_token_refreshes_expiring_oauth_file_before_start() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        let token_path = temp_file_path("bridge-oauth-expiring", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        std::env::set_var("KIANA_OAUTH_CLIENT_ID", "bridge-expiring-client");
        std::fs::write(
            &token_path,
            serde_json::json!({
                "access_token": "expiring-bridge-oauth",
                "refresh_token": "refresh-expiring-bridge-oauth",
                "expires_at": "2000-01-01T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let (url, requests, server) = start_oauth_token_server(serde_json::json!({
            "access_token": "fresh-bridge-expiring-oauth",
            "expires_in": 3600
        }))
        .await;
        std::env::set_var("KIANA_OAUTH_TOKEN_URL", url);

        let token = bridge_access_token_refreshing_if_expiring()
            .await
            .unwrap()
            .unwrap();
        let persisted = kiana_services::oauth::load_oauth_tokens().unwrap().unwrap();

        assert_eq!(token, "fresh-bridge-expiring-oauth");
        assert_eq!(persisted.access_token, "fresh-bridge-expiring-oauth");
        assert_eq!(
            persisted.refresh_token.as_deref(),
            Some("refresh-expiring-bridge-oauth")
        );
        let body = requests.lock().unwrap().join("\n");
        assert!(
            body.contains("refresh_token=refresh-expiring-bridge-oauth"),
            "{body}"
        );
        assert!(body.contains("client_id=bridge-expiring-client"), "{body}");

        server.abort();
        let _ = std::fs::remove_file(token_path);
        clear_bridge_env();
    }

    #[tokio::test]
    async fn cli_bridge_auth_provider_refreshes_from_oauth_file_without_refresh_command() {
        let _guard = env_lock().lock().unwrap();
        clear_bridge_env();
        let token_path = temp_file_path("bridge-oauth-refresh", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        std::env::set_var("KIANA_OAUTH_CLIENT_ID", "bridge-client");
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "stale-bridge-oauth".to_string(),
            refresh_token: Some("refresh-bridge-oauth".to_string()),
            expires_at: None,
        })
        .unwrap();
        let (url, requests, server) = start_oauth_token_server(serde_json::json!({
            "access_token": "fresh-bridge-oauth",
            "expires_in": 3600
        }))
        .await;
        std::env::set_var("KIANA_OAUTH_TOKEN_URL", url);

        let provider = CliBridgeAuthProvider;
        assert!(provider
            .refresh_after_unauthorized("stale-bridge-oauth")
            .await
            .unwrap());

        assert_eq!(
            std::env::var("KIANA_BRIDGE_ACCESS_TOKEN").ok().as_deref(),
            Some("fresh-bridge-oauth")
        );
        let persisted = kiana_services::oauth::load_oauth_tokens().unwrap().unwrap();
        assert_eq!(persisted.access_token, "fresh-bridge-oauth");
        assert_eq!(
            persisted.refresh_token.as_deref(),
            Some("refresh-bridge-oauth")
        );
        let body = requests.lock().unwrap().join("\n");
        assert!(
            body.contains("refresh_token=refresh-bridge-oauth"),
            "{body}"
        );
        assert!(body.contains("client_id=bridge-client"), "{body}");

        server.abort();
        let _ = std::fs::remove_file(token_path);
        clear_bridge_env();
    }

    #[tokio::test]
    async fn remote_session_cli_refresh_command_updates_listen_token() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "stale-token");
        let refresh_command = if cfg!(windows) {
            "echo fresh-token"
        } else {
            "printf fresh-token"
        };
        std::env::set_var("KIANA_REMOTE_REFRESH_COMMAND", refresh_command);

        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        let token = Arc::new(StdMutex::new("stale-token".to_string()));
        let callbacks = RemoteSessionCliCallbacks {
            events: event_tx,
            access_token: token.clone(),
        };

        assert!(
            kiana_remote::RemoteSessionCallbacks::refresh_after_unauthorized(
                &callbacks,
                "stale-token".to_string(),
            )
            .await
        );
        assert_eq!(token.lock().unwrap().as_str(), "fresh-token");
        assert_eq!(
            std::env::var("KIANA_REMOTE_ACCESS_TOKEN").ok().as_deref(),
            Some("fresh-token")
        );

        clear_remote_session_env();
    }

    #[test]
    fn remote_session_access_token_uses_oauth_file_when_env_tokens_are_missing() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        let token_path = temp_file_path("remote-oauth-token", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "oauth-access-token".to_string(),
            refresh_token: Some("oauth-refresh-token".to_string()),
            expires_at: None,
        })
        .unwrap();

        let token = remote_session_access_token_details().unwrap();

        assert_eq!(token.value, "oauth-access-token");
        assert_eq!(token.source, "oauth_file");

        let _ = std::fs::remove_file(token_path);
        clear_remote_session_env();
    }

    #[tokio::test]
    async fn remote_session_live_access_token_refreshes_expiring_oauth_file_before_request() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        let token_path = temp_file_path("remote-oauth-expiring", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        std::env::set_var("KIANA_OAUTH_CLIENT_ID", "client-expiring-remote");
        std::fs::write(
            &token_path,
            serde_json::json!({
                "access_token": "expiring-oauth-token",
                "refresh_token": "refresh-expiring-oauth-token",
                "expires_at": "2000-01-01T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let (url, requests, server) = start_oauth_token_server(serde_json::json!({
            "access_token": "fresh-expiring-oauth-token",
            "expires_in": 3600
        }))
        .await;
        std::env::set_var("KIANA_OAUTH_TOKEN_URL", url);

        let token = remote_session_live_access_token("list").await.unwrap();
        let persisted = kiana_services::oauth::load_oauth_tokens().unwrap().unwrap();

        assert_eq!(token.value, "fresh-expiring-oauth-token");
        assert_eq!(token.source, "oauth_file");
        assert_eq!(persisted.access_token, "fresh-expiring-oauth-token");
        assert_eq!(
            persisted.refresh_token.as_deref(),
            Some("refresh-expiring-oauth-token")
        );
        let body = requests.lock().unwrap().join("\n");
        assert!(
            body.contains("refresh_token=refresh-expiring-oauth-token"),
            "{body}"
        );
        assert!(body.contains("client_id=client-expiring-remote"), "{body}");

        server.abort();
        let _ = std::fs::remove_file(token_path);
        clear_remote_session_env();
    }

    #[tokio::test]
    async fn remote_session_refresh_uses_oauth_file_when_refresh_command_is_missing() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        let token_path = temp_file_path("remote-oauth-refresh", "json");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        std::env::set_var("KIANA_OAUTH_CLIENT_ID", "client-remote");
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "stale-oauth-token".to_string(),
            refresh_token: Some("refresh-oauth-token".to_string()),
            expires_at: None,
        })
        .unwrap();
        let (url, requests, server) = start_oauth_token_server(serde_json::json!({
            "access_token": "fresh-oauth-token",
            "refresh_token": "fresh-refresh-token",
            "expires_in": 3600
        }))
        .await;
        std::env::set_var("KIANA_OAUTH_TOKEN_URL", url);

        let refreshed = refresh_remote_session_access_token("stale-oauth-token")
            .await
            .unwrap()
            .unwrap();
        let persisted = kiana_services::oauth::load_oauth_tokens().unwrap().unwrap();

        assert_eq!(refreshed, "fresh-oauth-token");
        assert_eq!(
            std::env::var("KIANA_REMOTE_ACCESS_TOKEN").ok().as_deref(),
            Some("fresh-oauth-token")
        );
        assert_eq!(persisted.access_token, "fresh-oauth-token");
        assert_eq!(
            persisted.refresh_token.as_deref(),
            Some("fresh-refresh-token")
        );
        let body = requests.lock().unwrap().join("\n");
        assert!(body.contains("grant_type=refresh_token"), "{body}");
        assert!(body.contains("refresh_token=refresh-oauth-token"), "{body}");
        assert!(body.contains("client_id=client-remote"), "{body}");

        server.abort();
        let _ = std::fs::remove_file(token_path);
        clear_remote_session_env();
    }

    #[test]
    fn parse_bridge_refresh_token_accepts_json_or_plain_output() {
        assert_eq!(
            parse_bridge_refresh_token(r#"{"access_token":"json-token"}"#).as_deref(),
            Some("json-token")
        );
        assert_eq!(
            parse_bridge_refresh_token("plain-token\n").as_deref(),
            Some("plain-token")
        );
        assert_eq!(parse_bridge_refresh_token("   "), None);
    }

    fn refresh_command(token: &str) -> String {
        if cfg!(windows) {
            format!("echo {token}")
        } else {
            format!("printf {token}")
        }
    }

    fn temp_file_path(name: &str, extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "kiana-{name}-{}.{}",
            uuid::Uuid::new_v4(),
            extension
        ))
    }

    #[derive(Clone)]
    struct OAuthTokenMockState {
        response: Value,
        requests: StdArc<StdMutex<Vec<String>>>,
    }

    async fn start_oauth_token_server(
        response: Value,
    ) -> (
        String,
        StdArc<StdMutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        async fn handle(
            State(state): State<OAuthTokenMockState>,
            body: String,
        ) -> impl IntoResponse {
            state.requests.lock().unwrap().push(body);
            (StatusCode::OK, Json(state.response)).into_response()
        }

        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let state = OAuthTokenMockState {
            response,
            requests: requests.clone(),
        };
        let app = axum::Router::new()
            .route("/oauth/token", axum::routing::post(handle))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}/oauth/token", addr), requests, server)
    }

    #[test]
    fn chrome_native_host_install_args_accept_overrides() {
        let args = vec![
            "chrome".to_string(),
            "install-native-host".to_string(),
            "--dry-run".to_string(),
            "--json".to_string(),
            "--home".to_string(),
            "/tmp/home".to_string(),
            "--binary=/opt/kiana/kiana".to_string(),
            "--platform".to_string(),
            "windows".to_string(),
            "--include-dev-origins".to_string(),
        ];

        let parsed = parse_chrome_native_host_install_args(&args, 2).unwrap();

        assert!(parsed.dry_run);
        assert!(parsed.json);
        assert_eq!(
            parsed.home_dir.unwrap(),
            std::path::PathBuf::from("/tmp/home")
        );
        assert_eq!(
            parsed.binary_path.unwrap(),
            std::path::PathBuf::from("/opt/kiana/kiana")
        );
        assert_eq!(
            parsed.platform.unwrap(),
            kiana_chrome_mcp::native_install::NativeHostPlatform::Windows
        );
        assert!(parsed.include_dev_origins);
    }

    #[test]
    fn chrome_native_host_install_args_reject_invalid_platform() {
        let args = vec![
            "chrome".to_string(),
            "install-native-host".to_string(),
            "--platform".to_string(),
            "solaris".to_string(),
        ];

        let error = parse_chrome_native_host_install_args(&args, 2)
            .unwrap_err()
            .to_string();
        assert!(error.contains("--platform must be linux, macos, or windows"));
    }

    #[test]
    fn computer_mcp_stdio_route_accepts_command_and_legacy_flags() {
        assert!(is_computer_mcp_stdio_command(&["computer-mcp".to_string()]));
        assert!(is_computer_mcp_stdio_command(&[
            "computer-use-mcp".to_string()
        ]));
        assert!(is_computer_mcp_stdio_command(&[
            "--computer-mcp".to_string()
        ]));
        assert!(is_computer_mcp_stdio_command(&[
            "--computer-use-mcp".to_string()
        ]));
        assert!(!is_computer_mcp_stdio_command(&["mcp-server".to_string()]));
    }

    #[test]
    fn remote_session_config_accepts_env_and_argument_overrides() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_SESSION_ID", "env-session");
        std::env::set_var("KIANA_REMOTE_ORG_UUID", "env-org");
        std::env::set_var("KIANA_REMOTE_API_BASE_URL", "https://remote.example");
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");

        let env_config = build_remote_session_cli_config(&["remote-session".to_string()]).unwrap();
        assert_eq!(env_config.session_id.as_deref(), Some("env-session"));
        assert_eq!(env_config.org_uuid.as_deref(), Some("env-org"));
        assert_eq!(env_config.api_base_url, "https://remote.example");
        assert!(env_config.token_configured);

        let arg_config = build_remote_session_cli_config(&[
            "remote-session".to_string(),
            "status".to_string(),
            "--session-id".to_string(),
            "arg-session".to_string(),
            "--org-uuid=arg-org".to_string(),
            "--api-base-url".to_string(),
            "http://localhost:7777/proxy".to_string(),
        ])
        .unwrap();
        assert_eq!(arg_config.session_id.as_deref(), Some("arg-session"));
        assert_eq!(arg_config.org_uuid.as_deref(), Some("arg-org"));
        assert_eq!(arg_config.api_base_url, "http://localhost:7777/proxy");

        let url = kiana_remote::sessions_websocket_url(
            &arg_config.api_base_url,
            arg_config.session_id.as_deref().unwrap(),
            arg_config.org_uuid.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(
            url,
            "ws://localhost:7777/proxy/v1/sessions/ws/arg-session/subscribe?organization_uuid=arg-org"
        );

        clear_remote_session_env();
    }

    #[test]
    fn remote_session_status_does_not_treat_anthropic_api_key_as_remote_token() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-test-api-key");

        let config = build_remote_session_cli_config(&["remote-session".to_string()]).unwrap();

        assert_eq!(
            config.token_status,
            RemoteSessionTokenStatus::AnthropicApiKeyMisuse
        );
        assert!(
            !config.token_configured,
            "sk-* ANTHROPIC_AUTH_TOKEN should not make remote-session status look ready"
        );
        let status = remote_session_status_text(&config);
        assert!(status.contains("token_configured: false"));
        assert!(status.contains("token_status: invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)"));
        assert!(status.contains(
            "token_fix: set KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN to the remote bearer token"
        ));

        clear_remote_session_env();
    }

    #[test]
    fn remote_session_permission_mode_defaults_to_deny_and_accepts_overrides() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();

        assert_eq!(
            remote_session_permission_mode(&["remote-session".to_string(), "listen".to_string()])
                .unwrap(),
            RemoteSessionPermissionMode::Deny
        );
        assert_eq!(
            remote_session_permission_mode(&[
                "remote-session".to_string(),
                "listen".to_string(),
                "--permission-mode".to_string(),
                "allow".to_string(),
            ])
            .unwrap(),
            RemoteSessionPermissionMode::Allow
        );
        assert_eq!(
            remote_session_permission_mode(&[
                "remote-session".to_string(),
                "listen".to_string(),
                "--auto-deny-permissions".to_string(),
            ])
            .unwrap(),
            RemoteSessionPermissionMode::Deny
        );

        std::env::set_var("KIANA_REMOTE_PERMISSION_MODE", "manual");
        assert_eq!(
            remote_session_permission_mode(&["remote-session".to_string(), "listen".to_string()])
                .unwrap(),
            RemoteSessionPermissionMode::Manual
        );

        clear_remote_session_env();
    }

    #[test]
    fn remote_session_send_message_from_args_accepts_message_option_and_positionals() {
        assert_eq!(
            remote_session_send_message_from_args(&[
                "remote-session".to_string(),
                "send".to_string(),
                "--session-id".to_string(),
                "session-1".to_string(),
                "--org-uuid".to_string(),
                "org-1".to_string(),
                "--message".to_string(),
                "hello".to_string(),
            ])
            .as_deref(),
            Some("hello")
        );

        assert_eq!(
            remote_session_send_message_from_args(&[
                "remote-session".to_string(),
                "send".to_string(),
                "--session-id=session-1".to_string(),
                "--org-uuid=org-1".to_string(),
                "--uuid".to_string(),
                "event-1".to_string(),
                "hello".to_string(),
                "remote".to_string(),
            ])
            .as_deref(),
            Some("hello remote")
        );

        assert!(remote_session_send_message_from_args(&[
            "remote-session".to_string(),
            "send".to_string(),
            "--session-id".to_string(),
            "session-1".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
        ])
        .is_none());
    }

    #[test]
    fn remote_session_positionals_support_session_id_and_titles() {
        assert_eq!(
            remote_session_positional_args(&[
                "remote-session".to_string(),
                "rename".to_string(),
                "--org-uuid".to_string(),
                "org-1".to_string(),
                "--api-base-url=http://localhost:7777".to_string(),
                "session-1".to_string(),
                "New".to_string(),
                "title".to_string(),
                "--json".to_string(),
            ]),
            vec![
                "session-1".to_string(),
                "New".to_string(),
                "title".to_string()
            ]
        );
    }

    #[test]
    fn remote_session_environment_positionals_drop_environment_subcommand() {
        assert_eq!(
            remote_session_environment_positionals(&[
                "remote-session".to_string(),
                "environments".to_string(),
                "create-default".to_string(),
                "--org-uuid".to_string(),
                "org-1".to_string(),
                "Default".to_string(),
                "Cloud".to_string(),
            ]),
            vec!["Default".to_string(), "Cloud".to_string()]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_create_posts_reference_shape() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/code/sessions",
                axum::routing::post(handle_remote_code_session_create_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "create".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--title".to_string(),
            "Remote bridge".to_string(),
            "--tag".to_string(),
            "ccr-mirror".to_string(),
            "--tag=tooling".to_string(),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/code/sessions");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta, None);
        assert_eq!(request.organization_uuid, None);
        assert_eq!(request.trusted_device_token, None);
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["title"], "Remote bridge");
        assert_eq!(body["bridge"], serde_json::json!({}));
        assert_eq!(body["tags"], serde_json::json!(["ccr-mirror", "tooling"]));

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_bridge_fetches_credentials_and_trusted_device_token() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/code/sessions/cse_session_1/bridge",
                axum::routing::post(handle_remote_code_session_bridge_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "bridge".to_string(),
            "cse_session_1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--trusted-device-token".to_string(),
            "trusted-token".to_string(),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/code/sessions/cse_session_1/bridge");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(
            request.trusted_device_token.as_deref(),
            Some("trusted-token")
        );
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body, serde_json::json!({}));

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_smoke_refreshes_after_create_unauthorized() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "stale-token");
        std::env::set_var(
            "KIANA_REMOTE_REFRESH_COMMAND",
            refresh_command("fresh-token"),
        );
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();
        let state = RemoteCodeSessionRetryState {
            requests: request_tx,
            create_count: StdArc::new(AtomicUsize::new(0)),
            bridge_count: StdArc::new(AtomicUsize::new(0)),
        };

        let app = axum::Router::new()
            .route(
                "/v1/code/sessions",
                axum::routing::post(handle_remote_code_session_retry_create_request),
            )
            .route(
                "/v1/code/sessions/cse_session_1/bridge",
                axum::routing::post(handle_remote_code_session_retry_bridge_request),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "smoke".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--json".to_string(),
        ])
        .await
        .unwrap();

        let first_create =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        let retried_create =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        let bridge_request =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();

        assert_eq!(first_create.path, "/v1/code/sessions");
        assert_eq!(
            first_create.authorization.as_deref(),
            Some("Bearer stale-token")
        );
        assert_eq!(retried_create.path, "/v1/code/sessions");
        assert_eq!(
            retried_create.authorization.as_deref(),
            Some("Bearer fresh-token")
        );
        assert_eq!(
            bridge_request.path,
            "/v1/code/sessions/cse_session_1/bridge"
        );
        assert_eq!(
            bridge_request.authorization.as_deref(),
            Some("Bearer fresh-token")
        );
        assert_eq!(
            std::env::var("KIANA_REMOTE_ACCESS_TOKEN").ok().as_deref(),
            Some("fresh-token")
        );

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_bridge_refreshes_after_credentials_unauthorized() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "stale-token");
        std::env::set_var(
            "KIANA_REMOTE_REFRESH_COMMAND",
            refresh_command("fresh-token"),
        );
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();
        let state = RemoteCodeSessionRetryState {
            requests: request_tx,
            create_count: StdArc::new(AtomicUsize::new(0)),
            bridge_count: StdArc::new(AtomicUsize::new(0)),
        };

        let app = axum::Router::new()
            .route(
                "/v1/code/sessions/cse_session_1/bridge",
                axum::routing::post(handle_remote_code_session_retry_bridge_request),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "bridge".to_string(),
            "cse_session_1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        let first_bridge =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        let retried_bridge =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();

        assert_eq!(first_bridge.path, "/v1/code/sessions/cse_session_1/bridge");
        assert_eq!(
            first_bridge.authorization.as_deref(),
            Some("Bearer stale-token")
        );
        assert_eq!(
            retried_bridge.path,
            "/v1/code/sessions/cse_session_1/bridge"
        );
        assert_eq!(
            retried_bridge.authorization.as_deref(),
            Some("Bearer fresh-token")
        );

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_hydrate_writes_local_sdk_session() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let root = std::env::temp_dir().join(format!(
            "kiana-code-session-hydrate-{}",
            uuid::Uuid::new_v4()
        ));
        std::env::set_var("KIANA_SDK_SESSIONS_DIR", &root);
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = RemoteCodeSessionHydrateState {
            requests: request_tx,
            api_base_url: format!("http://{addr}"),
        };
        let app = axum::Router::new()
            .route(
                "/v1/code/sessions/cse_session_1/bridge",
                axum::routing::post(handle_remote_code_session_hydrate_bridge_request),
            )
            .route(
                "/v1/code/sessions/cse_session_1/worker/internal-events",
                axum::routing::get(handle_remote_code_session_internal_events_request),
            )
            .with_state(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "hydrate".to_string(),
            "cse_session_1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--json".to_string(),
        ])
        .await
        .unwrap();

        let bridge_request =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        let foreground_request =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        let subagent_request =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(bridge_request.method, "POST");
        assert_eq!(
            bridge_request.path,
            "/v1/code/sessions/cse_session_1/bridge"
        );
        assert_eq!(foreground_request.method, "GET");
        assert_eq!(
            foreground_request.path,
            "/v1/code/sessions/cse_session_1/worker/internal-events"
        );
        assert_eq!(subagent_request.method, "GET");
        assert_eq!(
            subagent_request.path,
            "/v1/code/sessions/cse_session_1/worker/internal-events?subagents=true"
        );

        let messages = crate::sdk::get_session_messages("cse_session_1".to_string())
            .await
            .unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "hydrate me");
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"][0]["text"], "hydrated answer");
        let agent_transcript = crate::sdk::get_session_subagent_transcript(
            "cse_session_1".to_string(),
            "agent-1".to_string(),
        )
        .await
        .unwrap();
        assert_eq!(agent_transcript.len(), 1);
        assert_eq!(
            agent_transcript[0]["message"]["content"][0]["text"],
            "agent note"
        );

        server.abort();
        let _ = std::fs::remove_dir_all(&root);
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_sdk_url_does_not_require_token() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "sdk-url".to_string(),
            "cse_session_1".to_string(),
            "--api-base-url".to_string(),
            "https://api.example/".to_string(),
        ])
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_smoke_requires_token() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();

        let error = remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "smoke".to_string(),
        ])
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("remote-session code-session smoke requires"));
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_smoke_rejects_anthropic_auth_api_key_before_request() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-test-api-key");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/code/sessions",
                axum::routing::post(handle_remote_code_session_auth_failure_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let error = remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "smoke".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("ANTHROPIC_AUTH_TOKEN looks like an Anthropic API key"));
        assert!(error.contains("KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), request_rx.recv())
                .await
                .is_err(),
            "sk-* ANTHROPIC_AUTH_TOKEN should be rejected before any live request"
        );

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_code_session_smoke_creates_session_and_fetches_bridge_credentials() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/code/sessions",
                axum::routing::post(handle_remote_code_session_create_request),
            )
            .route(
                "/v1/code/sessions/cse_session_1/bridge",
                axum::routing::post(handle_remote_code_session_bridge_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "code-session".to_string(),
            "smoke".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--title".to_string(),
            "Live smoke".to_string(),
            "--tag".to_string(),
            "live-smoke".to_string(),
            "--trusted-device-token".to_string(),
            "trusted-token".to_string(),
            "--json".to_string(),
        ])
        .await
        .unwrap();

        let create_request =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();
        let bridge_request =
            tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
                .await
                .unwrap()
                .unwrap();

        assert_eq!(create_request.method, "POST");
        assert_eq!(create_request.path, "/v1/code/sessions");
        assert_eq!(
            create_request.authorization.as_deref(),
            Some("Bearer token")
        );
        let create_body: Value = serde_json::from_str(&create_request.body).unwrap();
        assert_eq!(create_body["title"], "Live smoke");
        assert_eq!(create_body["tags"], serde_json::json!(["live-smoke"]));

        assert_eq!(bridge_request.method, "POST");
        assert_eq!(
            bridge_request.path,
            "/v1/code/sessions/cse_session_1/bridge"
        );
        assert_eq!(
            bridge_request.authorization.as_deref(),
            Some("Bearer token")
        );
        assert_eq!(
            bridge_request.trusted_device_token.as_deref(),
            Some("trusted-token")
        );

        server.abort();
        clear_remote_session_env();
    }

    #[derive(Clone, Debug)]
    struct RemoteSessionSendRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        anthropic_version: Option<String>,
        anthropic_beta: Option<String>,
        organization_uuid: Option<String>,
        content_type: Option<String>,
        trusted_device_token: Option<String>,
        body: String,
    }

    #[derive(Clone)]
    struct RemoteCodeSessionHydrateState {
        requests: tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>,
        api_base_url: String,
    }

    async fn handle_remote_session_send_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/sessions/session-1/events".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "ok": true
            })),
        )
            .into_response()
    }

    async fn handle_remote_session_list_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "GET".to_string(),
            path: "/v1/sessions".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body: String::new(),
        };
        let _ = requests.send(request);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "data": [{
                    "type": "session",
                    "id": "session-1",
                    "title": "Fix issue",
                    "session_status": "running",
                    "environment_id": "env-1",
                    "created_at": "2026-06-15T00:00:00Z",
                    "updated_at": "2026-06-15T00:01:00Z",
                    "session_context": {
                        "sources": [{
                            "type": "git_repository",
                            "url": "https://github.com/acme/widgets.git",
                            "revision": "main"
                        }],
                        "cwd": "/repo",
                        "outcomes": null,
                        "custom_system_prompt": null,
                        "append_system_prompt": null,
                        "model": null
                    }
                }],
                "has_more": false,
                "first_id": "session-1",
                "last_id": "session-1"
            })),
        )
            .into_response()
    }

    async fn handle_remote_session_show_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "GET".to_string(),
            path: "/v1/sessions/session-1".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body: String::new(),
        };
        let _ = requests.send(request);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "type": "session",
                "id": "session-1",
                "title": "Fix issue",
                "session_status": "idle",
                "environment_id": "env-1",
                "created_at": "2026-06-15T00:00:00Z",
                "updated_at": "2026-06-15T00:01:00Z",
                "session_context": {
                    "sources": [],
                    "cwd": "/repo",
                    "outcomes": [{
                        "type": "git_repository",
                        "git_info": {
                            "type": "github",
                            "repo": "acme/widgets",
                            "branches": ["feature/remote-session"]
                        }
                    }],
                    "custom_system_prompt": null,
                    "append_system_prompt": null,
                    "model": null
                }
            })),
        )
            .into_response()
    }

    async fn handle_remote_session_rename_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "PATCH".to_string(),
            path: "/v1/sessions/session-1".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true
            })),
        )
            .into_response()
    }

    async fn handle_remote_session_create_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/sessions".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": "session-1"
            })),
        )
            .into_response()
    }

    async fn handle_remote_session_archive_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/sessions/session-1/archive".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "ok": true
            })),
        )
            .into_response()
    }

    async fn handle_remote_file_upload_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: axum::body::Bytes,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/files".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body: String::from_utf8_lossy(&body).to_string(),
        };
        let _ = requests.send(request);
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "id": "file-1"
            })),
        )
            .into_response()
    }

    async fn handle_remote_environment_list_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "GET".to_string(),
            path: "/v1/environment_providers".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body: String::new(),
        };
        let _ = requests.send(request);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "environments": [
                    {
                        "kind": "bridge",
                        "environment_id": "bridge-1",
                        "name": "Bridge",
                        "created_at": "2026-06-15T00:00:00Z",
                        "state": "active"
                    },
                    {
                        "kind": "anthropic_cloud",
                        "environment_id": "cloud-1",
                        "name": "Cloud",
                        "created_at": "2026-06-15T00:01:00Z",
                        "state": "active"
                    }
                ],
                "has_more": false,
                "first_id": "bridge-1",
                "last_id": "cloud-1"
            })),
        )
            .into_response()
    }

    async fn handle_remote_environment_create_default_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/environment_providers/cloud/create".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "kind": "anthropic_cloud",
                "environment_id": "cloud-1",
                "name": "Default Cloud",
                "created_at": "2026-06-15T00:00:00Z",
                "state": "active"
            })),
        )
            .into_response()
    }

    async fn handle_remote_code_session_create_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/code/sessions".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "session": {
                    "id": "cse_session_1"
                }
            })),
        )
            .into_response()
    }

    async fn handle_remote_code_session_auth_failure_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/code/sessions".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": {
                    "message": "Authentication failed"
                }
            })),
        )
            .into_response()
    }

    #[derive(Clone)]
    struct RemoteCodeSessionRetryState {
        requests: tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>,
        create_count: StdArc<AtomicUsize>,
        bridge_count: StdArc<AtomicUsize>,
    }

    async fn handle_remote_code_session_retry_create_request(
        State(state): State<RemoteCodeSessionRetryState>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let authorization = header_value(&headers, "authorization");
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/code/sessions".to_string(),
            authorization: authorization.clone(),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = state.requests.send(request);
        let count = state.create_count.fetch_add(1, Ordering::SeqCst);
        if count == 0 && authorization.as_deref() == Some("Bearer stale-token") {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": {
                        "message": "Authentication failed"
                    }
                })),
            )
                .into_response();
        }
        (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "session": {
                    "id": "cse_session_1"
                }
            })),
        )
            .into_response()
    }

    async fn handle_remote_code_session_retry_bridge_request(
        State(state): State<RemoteCodeSessionRetryState>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let authorization = header_value(&headers, "authorization");
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/code/sessions/cse_session_1/bridge".to_string(),
            authorization: authorization.clone(),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = state.requests.send(request);
        let count = state.bridge_count.fetch_add(1, Ordering::SeqCst);
        if count == 0 && authorization.as_deref() == Some("Bearer stale-token") {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": {
                        "message": "Authentication failed"
                    }
                })),
            )
                .into_response();
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "worker_jwt": "worker-token",
                "api_base_url": "https://worker.example",
                "expires_in": 3600,
                "worker_epoch": 42
            })),
        )
            .into_response()
    }

    async fn handle_remote_code_session_bridge_request(
        State(requests): State<tokio::sync::mpsc::UnboundedSender<RemoteSessionSendRequest>>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/code/sessions/cse_session_1/bridge".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = requests.send(request);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "worker_jwt": "worker-token",
                "api_base_url": "https://worker.example",
                "expires_in": 3600,
                "worker_epoch": 42
            })),
        )
            .into_response()
    }

    async fn handle_remote_code_session_hydrate_bridge_request(
        State(state): State<RemoteCodeSessionHydrateState>,
        headers: axum::http::HeaderMap,
        body: String,
    ) -> impl IntoResponse {
        let request = RemoteSessionSendRequest {
            method: "POST".to_string(),
            path: "/v1/code/sessions/cse_session_1/bridge".to_string(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body,
        };
        let _ = state.requests.send(request);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "worker_jwt": "worker-token",
                "api_base_url": state.api_base_url,
                "expires_in": 3600,
                "worker_epoch": 42
            })),
        )
            .into_response()
    }

    async fn handle_remote_code_session_internal_events_request(
        State(state): State<RemoteCodeSessionHydrateState>,
        uri: axum::extract::OriginalUri,
        headers: axum::http::HeaderMap,
    ) -> impl IntoResponse {
        let path = uri
            .0
            .path_and_query()
            .map(|value| value.as_str().to_string())
            .unwrap_or_else(|| uri.0.path().to_string());
        let request = RemoteSessionSendRequest {
            method: "GET".to_string(),
            path: path.clone(),
            authorization: header_value(&headers, "authorization"),
            anthropic_version: header_value(&headers, "anthropic-version"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            organization_uuid: header_value(&headers, "x-organization-uuid"),
            content_type: header_value(&headers, "content-type"),
            trusted_device_token: header_value(&headers, "x-trusted-device-token"),
            body: String::new(),
        };
        let _ = state.requests.send(request);
        let data = if path.contains("subagents=true") {
            serde_json::json!([{
                "event_id": "int-agent-1",
                "event_type": "transcript",
                "payload": {
                    "type": "assistant",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "agent note"}]
                    }
                },
                "is_compaction": false,
                "created_at": "2026-06-16T00:00:02Z",
                "agent_id": "agent-1"
            }])
        } else {
            serde_json::json!([
                {
                    "event_id": "int-1",
                    "event_type": "transcript",
                    "payload": {
                        "type": "user",
                        "message": {
                            "role": "user",
                            "content": "hydrate me"
                        }
                    },
                    "is_compaction": false,
                    "created_at": "2026-06-16T00:00:00Z"
                },
                {
                    "event_id": "int-2",
                    "event_type": "transcript",
                    "payload": {
                        "type": "assistant",
                        "message": {
                            "role": "assistant",
                            "content": [{"type": "text", "text": "hydrated answer"}]
                        }
                    },
                    "is_compaction": false,
                    "created_at": "2026-06-16T00:00:01Z"
                }
            ])
        };
        (StatusCode::OK, Json(serde_json::json!({ "data": data }))).into_response()
    }

    fn header_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string())
    }

    async fn create_test_git_repo(prefix: &str) -> PathBuf {
        let repo = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&repo).unwrap();
        run_git_for_test(&repo, &["init"]).await;
        run_git_for_test(&repo, &["config", "user.email", "kiana@example.com"]).await;
        run_git_for_test(&repo, &["config", "user.name", "Kiana"]).await;
        tokio::fs::write(repo.join("tracked.txt"), "initial\n")
            .await
            .unwrap();
        run_git_for_test(&repo, &["add", "tracked.txt"]).await;
        run_git_for_test(&repo, &["commit", "-m", "initial"]).await;
        repo
    }

    async fn run_git_for_test(repo: &Path, args: &[&str]) {
        let output = tokio::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_send_posts_reference_event_shape() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions/session-1/events",
                axum::routing::post(handle_remote_session_send_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "send".to_string(),
            "--session-id".to_string(),
            "session-1".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--uuid".to_string(),
            "event-1".to_string(),
            "hello".to_string(),
            "remote".to_string(),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions/session-1/events");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(
            request.anthropic_beta.as_deref(),
            Some("ccr-byoc-2025-07-29")
        );
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        assert!(request
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));

        let body: Value = serde_json::from_str(&request.body).unwrap();
        let event = &body["events"][0];
        assert_eq!(event["uuid"], "event-1");
        assert_eq!(event["session_id"], "session-1");
        assert_eq!(event["type"], "user");
        assert_eq!(event["message"]["role"], "user");
        assert_eq!(event["message"]["content"], "hello remote");

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_list_fetches_reference_sessions_api() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions",
                axum::routing::get(handle_remote_session_list_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "list".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/sessions");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(
            request.anthropic_beta.as_deref(),
            Some("ccr-byoc-2025-07-29")
        );
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_list_rejects_anthropic_auth_api_key_before_request() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-test-api-key");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions",
                axum::routing::get(handle_remote_session_list_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let error = remote_session_main(&[
            "remote-session".to_string(),
            "list".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("ANTHROPIC_AUTH_TOKEN looks like an Anthropic API key"));
        assert!(error.contains("KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), request_rx.recv())
                .await
                .is_err(),
            "sk-* ANTHROPIC_AUTH_TOKEN should be rejected before remote-session list sends a request"
        );

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_show_fetches_session_by_positional_id() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions/session-1",
                axum::routing::get(handle_remote_session_show_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "show".to_string(),
            "session-1".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/sessions/session-1");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_rename_patches_title_by_positional_id() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions/session-1",
                axum::routing::patch(handle_remote_session_rename_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "rename".to_string(),
            "session-1".to_string(),
            "New".to_string(),
            "title".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "PATCH");
        assert_eq!(request.path, "/v1/sessions/session-1");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["title"], "New title");

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_send_rejects_anthropic_auth_api_key_before_request() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-test-api-key");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions/session-1/events",
                axum::routing::post(handle_remote_session_send_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let error = remote_session_main(&[
            "remote-session".to_string(),
            "send".to_string(),
            "--session-id".to_string(),
            "session-1".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "hello".to_string(),
        ])
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("ANTHROPIC_AUTH_TOKEN looks like an Anthropic API key"));
        assert!(error.contains("KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN"));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), request_rx.recv())
                .await
                .is_err(),
            "sk-* ANTHROPIC_AUTH_TOKEN should be rejected before remote-session send sends a request"
        );

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_create_posts_reference_shape() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions",
                axum::routing::post(handle_remote_session_create_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "create".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--environment-id".to_string(),
            "env-1".to_string(),
            "--title".to_string(),
            "Remote task".to_string(),
            "--permission-mode".to_string(),
            "accept-edits".to_string(),
            "--request-id".to_string(),
            "set-mode-1".to_string(),
            "--uuid".to_string(),
            "event-1".to_string(),
            "--model".to_string(),
            "claude-sonnet-4-20250514".to_string(),
            "--git-url".to_string(),
            "https://github.com/acme/widgets.git".to_string(),
            "--git-revision".to_string(),
            "main".to_string(),
            "--outcome-branch".to_string(),
            "claude/task".to_string(),
            "--seed-bundle-file-id".to_string(),
            "file-1".to_string(),
            "hello".to_string(),
            "remote".to_string(),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(
            request.anthropic_beta.as_deref(),
            Some("ccr-byoc-2025-07-29")
        );
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        assert!(request
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));

        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["title"], "Remote task");
        assert_eq!(body["environment_id"], "env-1");
        assert_eq!(body["source"], "remote-control");
        assert_eq!(
            body["session_context"]["sources"][0]["url"],
            "https://github.com/acme/widgets.git"
        );
        assert_eq!(body["session_context"]["sources"][0]["revision"], "main");
        assert_eq!(
            body["session_context"]["outcomes"][0]["git_info"]["repo"],
            "acme/widgets"
        );
        assert_eq!(
            body["session_context"]["outcomes"][0]["git_info"]["branches"][0],
            "claude/task"
        );
        assert_eq!(body["session_context"]["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["session_context"]["seed_bundle_file_id"], "file-1");
        assert_eq!(body["session_context"]["reuse_outcome_branches"], true);
        assert_eq!(body["events"][0]["type"], "event");
        assert_eq!(body["events"][0]["data"]["type"], "control_request");
        assert_eq!(body["events"][0]["data"]["request_id"], "set-mode-1");
        assert_eq!(
            body["events"][0]["data"]["request"]["subtype"],
            "set_permission_mode"
        );
        assert_eq!(body["events"][0]["data"]["request"]["mode"], "acceptEdits");
        assert_eq!(body["events"][1]["data"]["uuid"], "event-1");
        assert_eq!(body["events"][1]["data"]["session_id"], "");
        assert_eq!(body["events"][1]["data"]["type"], "user");
        assert_eq!(body["events"][1]["data"]["message"]["role"], "user");
        assert_eq!(
            body["events"][1]["data"]["message"]["content"],
            "hello remote"
        );

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_create_seed_bundle_uploads_file_and_injects_file_id() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let repo = create_test_git_repo("kiana-cli-seed-bundle").await;
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/files",
                axum::routing::post(handle_remote_file_upload_request),
            )
            .route(
                "/v1/sessions",
                axum::routing::post(handle_remote_session_create_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "create".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--environment-id".to_string(),
            "env-1".to_string(),
            "--seed-bundle".to_string(),
            "--seed-bundle-dir".to_string(),
            repo.to_string_lossy().to_string(),
            "--title".to_string(),
            "Seeded task".to_string(),
            "hello".to_string(),
            "bundle".to_string(),
        ])
        .await
        .unwrap();

        let upload = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(upload.method, "POST");
        assert_eq!(upload.path, "/v1/files");
        assert_eq!(upload.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(upload.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(
            upload.anthropic_beta.as_deref(),
            Some("files-api-2025-04-14,oauth-2025-04-20")
        );
        assert!(upload
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("multipart/form-data; boundary=----FormBoundary"));
        assert!(upload.body.contains("filename=\"_source_seed.bundle\""));
        assert!(upload.body.contains("name=\"purpose\""));
        assert!(upload.body.contains("user_data"));

        let create = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(create.method, "POST");
        assert_eq!(create.path, "/v1/sessions");
        assert_eq!(create.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(create.organization_uuid.as_deref(), Some("org-1"));
        let body: Value = serde_json::from_str(&create.body).unwrap();
        assert_eq!(body["title"], "Seeded task");
        assert_eq!(body["environment_id"], "env-1");
        assert_eq!(body["session_context"]["seed_bundle_file_id"], "file-1");
        assert_eq!(
            body["events"][0]["data"]["message"]["content"],
            "hello bundle"
        );

        server.abort();
        let _ = std::fs::remove_dir_all(repo);
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_archive_posts_reference_archive_endpoint() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/sessions/session-1/archive",
                axum::routing::post(handle_remote_session_archive_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "archive".to_string(),
            "session-1".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/sessions/session-1/archive");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(
            request.anthropic_beta.as_deref(),
            Some("ccr-byoc-2025-07-29")
        );
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body, serde_json::json!({}));

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_environments_list_fetches_reference_endpoint() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/environment_providers",
                axum::routing::get(handle_remote_environment_list_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "environments".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/environment_providers");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta, None);
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_environments_selected_uses_default_environment_id() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/environment_providers",
                axum::routing::get(handle_remote_environment_list_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "environments".to_string(),
            "selected".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "--default-environment-id".to_string(),
            "bridge-1".to_string(),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/environment_providers");

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_environments_create_default_posts_reference_shape() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (request_tx, mut request_rx) =
            tokio::sync::mpsc::unbounded_channel::<RemoteSessionSendRequest>();

        let app = axum::Router::new()
            .route(
                "/v1/environment_providers/cloud/create",
                axum::routing::post(handle_remote_environment_create_default_request),
            )
            .with_state(request_tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "environments".to_string(),
            "create-default".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
            "Default".to_string(),
            "Cloud".to_string(),
        ])
        .await
        .unwrap();

        let request = tokio::time::timeout(std::time::Duration::from_secs(1), request_rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/environment_providers/cloud/create");
        assert_eq!(request.authorization.as_deref(), Some("Bearer token"));
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(
            request.anthropic_beta.as_deref(),
            Some("ccr-byoc-2025-07-29")
        );
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["name"], "Default Cloud");
        assert_eq!(body["kind"], "anthropic_cloud");
        assert_eq!(body["config"]["environment_type"], "anthropic");
        assert_eq!(body["config"]["languages"][0]["version"], "3.11");
        assert_eq!(body["config"]["languages"][1]["name"], "node");

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_listen_once_connects_to_websocket() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");

        let app = axum::Router::new().fallback(axum::routing::get(
            |ws: axum::extract::ws::WebSocketUpgrade| async move {
                ws.on_upgrade(|mut socket| async move {
                    let message = serde_json::json!({
                        "type": "auth_status"
                    })
                    .to_string();
                    let _ = socket
                        .send(axum::extract::ws::Message::Text(message.into()))
                        .await;
                    let _ = socket.recv().await;
                })
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        remote_session_main(&[
            "remote-session".to_string(),
            "listen".to_string(),
            "--once".to_string(),
            "--session-id".to_string(),
            "session-1".to_string(),
            "--org-uuid".to_string(),
            "org-1".to_string(),
            "--api-base-url".to_string(),
            format!("http://{addr}"),
        ])
        .await
        .unwrap();

        server.abort();
        clear_remote_session_env();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn remote_session_listen_auto_denies_permission_requests() {
        let _guard = env_lock().lock().unwrap();
        clear_remote_session_env();
        std::env::set_var("KIANA_REMOTE_ACCESS_TOKEN", "token");
        let (response_tx, mut response_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        let app = axum::Router::new().fallback(axum::routing::get({
            move |ws: axum::extract::ws::WebSocketUpgrade| {
                let response_tx = response_tx.clone();
                async move {
                    ws.on_upgrade(move |mut socket| async move {
                        let request = serde_json::json!({
                            "type": "control_request",
                            "request_id": "req-1",
                            "request": {
                                "subtype": "can_use_tool",
                                "tool_name": "Bash",
                                "tool_use_id": "tool-1",
                                "input": {
                                    "command": "pwd"
                                }
                            }
                        })
                        .to_string();
                        let _ = socket
                            .send(axum::extract::ws::Message::Text(request.into()))
                            .await;
                        if let Some(Ok(axum::extract::ws::Message::Text(text))) =
                            socket.recv().await
                        {
                            let _ = response_tx.send(text.to_string());
                        }
                        let message = serde_json::json!({
                            "type": "auth_status"
                        })
                        .to_string();
                        let _ = socket
                            .send(axum::extract::ws::Message::Text(message.into()))
                            .await;
                        let _ = socket.recv().await;
                    })
                }
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            remote_session_main(&[
                "remote-session".to_string(),
                "listen".to_string(),
                "--once".to_string(),
                "--session-id".to_string(),
                "session-1".to_string(),
                "--org-uuid".to_string(),
                "org-1".to_string(),
                "--api-base-url".to_string(),
                format!("http://{addr}"),
            ]),
        )
        .await
        .unwrap()
        .unwrap();

        let response = tokio::time::timeout(std::time::Duration::from_secs(1), response_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let response: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(response["type"], "control_response");
        assert_eq!(response["response"]["subtype"], "success");
        assert_eq!(response["response"]["request_id"], "req-1");
        assert_eq!(response["response"]["response"]["behavior"], "deny");

        server.abort();
        clear_remote_session_env();
    }

    fn clear_remote_session_env() {
        for key in [
            "KIANA_REMOTE_SESSION_ID",
            "CLAUDE_REMOTE_SESSION_ID",
            "KIANA_REMOTE_ORG_UUID",
            "CLAUDE_ORG_UUID",
            "ANTHROPIC_ORGANIZATION_ID",
            "KIANA_REMOTE_API_BASE_URL",
            "ANTHROPIC_API_BASE_URL",
            "KIANA_REMOTE_ACCESS_TOKEN",
            "KIANA_REMOTE_REFRESH_COMMAND",
            "KIANA_REMOTE_STALE_ACCESS_TOKEN",
            "CLAUDE_ACCESS_TOKEN",
            "ANTHROPIC_AUTH_TOKEN",
            "KIANA_REMOTE_PERMISSION_MODE",
            "KIANA_REMOTE_CREATE_PERMISSION_MODE",
            "KIANA_PERMISSION_MODE",
            "KIANA_REMOTE_ENVIRONMENT_ID",
            "CLAUDE_ENVIRONMENT_ID",
            "KIANA_REMOTE_DEFAULT_ENVIRONMENT_ID",
            "CLAUDE_DEFAULT_ENVIRONMENT_ID",
            "KIANA_REMOTE_MODEL",
            "KIANA_REMOTE_SESSION_SOURCE",
            "KIANA_REMOTE_SEED_BUNDLE",
            "CCR_FORCE_BUNDLE",
            "KIANA_REMOTE_SEED_BUNDLE_DIR",
            "KIANA_REMOTE_SEED_BUNDLE_MAX_BYTES",
            "KIANA_REMOTE_CODE_SESSION_TITLE",
            "KIANA_REMOTE_CODE_SESSION_TAGS",
            "KIANA_REMOTE_TRUSTED_DEVICE_TOKEN",
            "CLAUDE_TRUSTED_DEVICE_TOKEN",
            "KIANA_OAUTH_TOKENS_FILE",
            "CLAUDE_CODE_OAUTH_TOKENS_FILE",
            "KIANA_OAUTH_CLIENT_ID",
            "KIANA_OAUTH_AUTH_URL",
            "KIANA_OAUTH_TOKEN_URL",
            "KIANA_OAUTH_REDIRECT_URI",
            "KIANA_SDK_SESSIONS_DIR",
        ] {
            std::env::remove_var(key);
        }
    }

    fn clear_bridge_env() {
        for key in [
            "KIANA_BRIDGE_API_BASE_URL",
            "KIANA_BRIDGE_SESSION_INGRESS_URL",
            "KIANA_BRIDGE_DIR",
            "KIANA_BRIDGE_MACHINE_NAME",
            "KIANA_BRIDGE_BRANCH",
            "KIANA_BRIDGE_GIT_REPO_URL",
            "KIANA_BRIDGE_MAX_SESSIONS",
            "KIANA_BRIDGE_SPAWN_MODE",
            "KIANA_BRIDGE_HEARTBEAT_INTERVAL_MS",
            "KIANA_BRIDGE_SESSION_TIMEOUT",
            "KIANA_BRIDGE_SESSION_TIMEOUT_MS",
            "KIANA_BRIDGE_CCR_V2_SSE_RECONNECT_GIVE_UP_MS",
            "KIANA_BRIDGE_CCR_V2_SSE_LIVENESS_TIMEOUT_MS",
            "KIANA_BRIDGE_ID",
            "KIANA_BRIDGE_WORKER_TYPE",
            "KIANA_BRIDGE_ENVIRONMENT_ID",
            "KIANA_BRIDGE_ACCESS_TOKEN",
            "CLAUDE_ACCESS_TOKEN",
            "KIANA_BRIDGE_REFRESH_COMMAND",
            "KIANA_OAUTH_TOKENS_FILE",
            "CLAUDE_CODE_OAUTH_TOKENS_FILE",
            "KIANA_OAUTH_CLIENT_ID",
            "KIANA_OAUTH_AUTH_URL",
            "KIANA_OAUTH_TOKEN_URL",
            "KIANA_OAUTH_REDIRECT_URI",
            "KIANA_BRIDGE_DEBUG_FILE",
            "KIANA_BRIDGE_PERMISSION_MODE",
            "KIANA_PERMISSION_MODE",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn resume_cli_args_accept_continue_prompt() {
        let args = vec![
            "--continue".to_string(),
            "--record-only".to_string(),
            "follow".to_string(),
            "up".to_string(),
        ];

        assert_eq!(
            parse_resume_cli_args(&args).unwrap(),
            Some(ResumeCliArgs {
                mode: ResumeCliMode::ContinueLatest,
                message: Some("follow up".to_string()),
                execute: false,
                fork_session: false,
                output_format: PrintOutputFormat::Text,
                json_schema: None,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn resume_cli_args_accept_resume_equals_with_fork_and_json() {
        let args = vec![
            "--resume=session-1".to_string(),
            "--fork-session".to_string(),
            "--output-format=json".to_string(),
            "--json-schema".to_string(),
            r#"{"type":"object"}"#.to_string(),
            "--".to_string(),
            "summarize".to_string(),
        ];

        assert_eq!(
            parse_resume_cli_args(&args).unwrap(),
            Some(ResumeCliArgs {
                mode: ResumeCliMode::ResumeSession("session-1".to_string()),
                message: Some("summarize".to_string()),
                execute: true,
                fork_session: true,
                output_format: PrintOutputFormat::Json,
                json_schema: Some(serde_json::json!({"type": "object"})),
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn resume_cli_args_accept_print_before_continue() {
        let args = vec![
            "-p".to_string(),
            "--continue".to_string(),
            "--record-only".to_string(),
            "follow".to_string(),
            "up".to_string(),
        ];

        assert_eq!(
            parse_resume_cli_args(&args).unwrap(),
            Some(ResumeCliArgs {
                mode: ResumeCliMode::ContinueLatest,
                message: Some("follow up".to_string()),
                execute: false,
                fork_session: false,
                output_format: PrintOutputFormat::Text,
                json_schema: None,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn resume_cli_args_accept_print_before_resume() {
        let args = vec![
            "-p".to_string(),
            "--resume".to_string(),
            "session-1".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--".to_string(),
            "summarize".to_string(),
        ];

        assert_eq!(
            parse_resume_cli_args(&args).unwrap(),
            Some(ResumeCliArgs {
                mode: ResumeCliMode::ResumeSession("session-1".to_string()),
                message: Some("summarize".to_string()),
                execute: true,
                fork_session: false,
                output_format: PrintOutputFormat::StreamJson,
                json_schema: None,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn resume_cli_args_accept_include_partial_messages() {
        let args = vec![
            "-p".to_string(),
            "--resume=session-1".to_string(),
            "--output-format=stream-json".to_string(),
            "--include-partial-messages".to_string(),
            "summarize".to_string(),
        ];

        assert_eq!(
            parse_resume_cli_args(&args).unwrap(),
            Some(ResumeCliArgs {
                mode: ResumeCliMode::ResumeSession("session-1".to_string()),
                message: Some("summarize".to_string()),
                execute: true,
                fork_session: false,
                output_format: PrintOutputFormat::StreamJson,
                json_schema: None,
                include_partial_messages: true,
            })
        );
    }

    #[test]
    fn resume_cli_args_accept_print_equals_prompt_before_resume() {
        let args = vec![
            "--print=quick".to_string(),
            "--resume=session-1".to_string(),
            "check".to_string(),
        ];

        assert_eq!(
            parse_resume_cli_args(&args).unwrap(),
            Some(ResumeCliArgs {
                mode: ResumeCliMode::ResumeSession("session-1".to_string()),
                message: Some("quick check".to_string()),
                execute: true,
                fork_session: false,
                output_format: PrintOutputFormat::Text,
                json_schema: None,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn resume_cli_args_reject_multiple_resume_modes() {
        let args = vec![
            "-p".to_string(),
            "--continue".to_string(),
            "--resume=session-1".to_string(),
            "hello".to_string(),
        ];

        let error = parse_resume_cli_args(&args).unwrap_err().to_string();
        assert!(error.contains("use only one"));
    }

    #[test]
    fn resume_cli_args_ignore_print_prompt_text() {
        let args = vec![
            "-p".to_string(),
            "explain".to_string(),
            "--resume".to_string(),
            "as".to_string(),
            "text".to_string(),
        ];

        assert_eq!(parse_resume_cli_args(&args).unwrap(), None);
    }

    #[test]
    fn resume_cli_args_reject_resume_without_id() {
        let args = vec!["--resume".to_string(), "--fork-session".to_string()];

        let error = parse_resume_cli_args(&args).unwrap_err().to_string();
        assert!(error.contains("Usage: kiana --continue"));
    }

    #[tokio::test]
    async fn resume_cli_rejects_session_id_without_fork() {
        let error = resume_cli_main(
            ResumeCliArgs {
                mode: ResumeCliMode::ResumeSession("source-session".to_string()),
                message: None,
                execute: false,
                fork_session: false,
                output_format: PrintOutputFormat::Text,
                json_schema: None,
                include_partial_messages: false,
            },
            &RuntimeFlags {
                session_id: Some("target-session".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("--session-id can only be used"));
    }

    #[tokio::test]
    async fn resume_cli_rejects_no_session_persistence() {
        let error = resume_cli_main(
            ResumeCliArgs {
                mode: ResumeCliMode::ResumeSession("source-session".to_string()),
                message: None,
                execute: false,
                fork_session: false,
                output_format: PrintOutputFormat::Text,
                json_schema: None,
                include_partial_messages: false,
            },
            &RuntimeFlags {
                no_session_persistence: true,
                ..RuntimeFlags::default()
            },
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("cannot be used with --continue or --resume"));
    }

    #[tokio::test]
    async fn latest_session_id_prefers_recent_completed_session_over_newer_orphan() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-cli-latest-session-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::env::remove_var("KIANA_HOME");
        std::env::set_var("KIANA_SDK_SESSIONS_DIR", &root);
        write_test_sdk_session(
            &root,
            "newer-orphan",
            20,
            serde_json::json!([
                {"role": "user", "content": "failed prompt"}
            ]),
        );
        write_test_sdk_session(
            &root,
            "older-completed",
            10,
            serde_json::json!([
                {"role": "user", "content": "hello"},
                {"role": "assistant", "content": [{"type": "text", "text": "ok"}]}
            ]),
        );

        let session_id = latest_session_id().await.unwrap();

        assert_eq!(session_id, "older-completed");
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_SDK_SESSIONS_DIR");
    }

    #[tokio::test]
    async fn latest_session_id_falls_back_to_newest_session_when_none_completed() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-cli-latest-recorded-session-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::env::remove_var("KIANA_HOME");
        std::env::set_var("KIANA_SDK_SESSIONS_DIR", &root);
        write_test_sdk_session(
            &root,
            "older-recorded",
            10,
            serde_json::json!([
                {"role": "user", "content": "old"}
            ]),
        );
        write_test_sdk_session(
            &root,
            "newer-recorded",
            20,
            serde_json::json!([
                {"role": "user", "content": "new"}
            ]),
        );

        let session_id = latest_session_id().await.unwrap();

        assert_eq!(session_id, "newer-recorded");
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_SDK_SESSIONS_DIR");
    }

    #[test]
    fn runtime_flags_are_stripped_before_command() {
        let (args, flags) = extract_runtime_flags(vec![
            "--permission-mode".to_string(),
            "ask".to_string(),
            "--permission-profile=read-only".to_string(),
            "--allowed-tools=Bash(git:*)".to_string(),
            "--approve-local-write".to_string(),
            "reply".to_string(),
            "session-1".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["reply", "session-1", "hello"]);
        assert_eq!(
            flags,
            RuntimeFlags {
                permission_mode: Some("ask".to_string()),
                permission_profile: Some("read-only".to_string()),
                allowed_tools: Some("Bash(git:*)".to_string()),
                disallowed_tools: None,
                approve_local_write: true,
                ..RuntimeFlags::default()
            }
        );
    }

    #[test]
    fn apply_runtime_flags_sets_permission_profile_env() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["KIANA_PERMISSION_PROFILE"]);

        apply_runtime_flags(&RuntimeFlags {
            permission_profile: Some("read-only".to_string()),
            ..RuntimeFlags::default()
        })
        .unwrap();

        assert_eq!(
            std::env::var("KIANA_PERMISSION_PROFILE").ok().as_deref(),
            Some("read-only")
        );
    }

    #[test]
    fn runtime_flags_accept_model_base_url_and_max_turns() {
        let (args, flags) = extract_runtime_flags(vec![
            "--model".to_string(),
            "claude-opus-4-1".to_string(),
            "--fallback-model".to_string(),
            "claude-sonnet-4-6".to_string(),
            "--base-url=https://api.example.test".to_string(),
            "--api-timeout".to_string(),
            "30".to_string(),
            "--max-turns".to_string(),
            "3".to_string(),
            "--max-thinking-tokens=2048".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.model, Some("claude-opus-4-1".to_string()));
        assert_eq!(flags.fallback_model, Some("claude-sonnet-4-6".to_string()));
        assert_eq!(flags.base_url, Some("https://api.example.test".to_string()));
        assert_eq!(flags.api_timeout_ms, Some("30000".to_string()));
        assert_eq!(flags.max_turns, Some("3".to_string()));
        assert_eq!(flags.max_thinking_tokens, Some("2048".to_string()));
    }

    #[test]
    fn runtime_flags_accept_api_timeout_milliseconds() {
        let (args, flags) = extract_runtime_flags(vec![
            "--api-timeout-ms=750".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.api_timeout_ms, Some("750".to_string()));
    }

    #[test]
    fn runtime_flags_accept_repair_loop_values() {
        let (args, flags) = extract_runtime_flags(vec![
            "--repair-checks".to_string(),
            "--repair-check-attempts=2".to_string(),
            "-p".to_string(),
            "fix failing tests".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "fix failing tests"]);
        assert!(flags.repair_checks);
        assert_eq!(flags.repair_check_attempts, Some("2".to_string()));
    }

    #[test]
    fn runtime_flags_accept_permission_prompt_tool() {
        let (args, flags) = extract_runtime_flags(vec![
            "--permission-prompt-tool".to_string(),
            "perm.approve".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(
            flags.permission_prompt_tool,
            Some("perm.approve".to_string())
        );
    }

    #[test]
    fn runtime_flags_accept_agent_and_agents_json() {
        let (args, flags) = extract_runtime_flags(vec![
            "--agents".to_string(),
            r#"{"reviewer":{"description":"reviews code","prompt":"You review code."}}"#
                .to_string(),
            "--agent=reviewer".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.agent, Some("reviewer".to_string()));
        assert_eq!(
            flags.agents_json,
            Some(
                r#"{"reviewer":{"description":"reviews code","prompt":"You review code."}}"#
                    .to_string()
            )
        );
    }

    #[test]
    fn unknown_project_trust_hides_project_and_local_agents_but_keeps_other_sources() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-agents-unknown-trust-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = root.join("kiana-home");
        let project = root.join("project");
        let plugins = root.join("plugins");
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(kiana_home.join("agents")).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".kiana").join("agents")).unwrap();
        std::fs::create_dir_all(project.join(".claude").join("agents-local")).unwrap();
        std::fs::create_dir_all(plugins.join("alpha").join("agents")).unwrap();
        std::fs::write(
            kiana_home.join("agents").join("user-agent.md"),
            "---\ndescription: user agent\n---\nUser prompt.",
        )
        .unwrap();
        std::fs::write(
            project
                .join(".kiana")
                .join("agents")
                .join("project-agent.md"),
            "---\ndescription: project agent\n---\nProject prompt.",
        )
        .unwrap();
        std::fs::write(
            project
                .join(".claude")
                .join("agents-local")
                .join("local-agent.md"),
            "---\ndescription: local agent\n---\nLocal prompt.",
        )
        .unwrap();
        std::fs::write(
            plugins.join("alpha").join("agents").join("plugin-agent.md"),
            "---\ndescription: plugin agent\n---\nPlugin prompt.",
        )
        .unwrap();

        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::remove_var("KIANA_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_JSON");
        std::env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins);
        let cwd = CurrentDirGuard::set(&project);
        let flags = RuntimeFlags {
            agents_json: Some(
                r#"{"flag-agent":{"description":"flag agent","prompt":"Flag prompt."}}"#
                    .to_string(),
            ),
            ..RuntimeFlags::default()
        };

        let agents = discover_agents(&flags).unwrap();

        assert!(!agents
            .iter()
            .any(|agent| matches!(agent.source, AgentSource::Project | AgentSource::Local)));
        assert!(agents
            .iter()
            .any(|agent| agent.name == "user-agent" && agent.source == AgentSource::User));
        assert!(agents
            .iter()
            .any(|agent| { agent.name == "plugin-agent" && agent.source == AgentSource::Plugin }));
        assert!(agents
            .iter()
            .any(|agent| agent.name == "flag-agent" && agent.source == AgentSource::Flag));
        assert!(agents.iter().any(|agent| {
            agent.name == "general-purpose" && agent.source == AgentSource::BuiltIn
        }));

        drop(cwd);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agents_discovery_lists_sources_and_marks_shadowed_agents() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-agents-discovery-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = home.join(".kiana");
        let project = root.join("project");
        let plugins = root.join("plugins");
        let previous_cwd = std::env::current_dir().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);

        std::fs::create_dir_all(kiana_home.join("agents")).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".kiana").join("agents")).unwrap();
        std::fs::create_dir_all(project.join(".claude").join("agents-local")).unwrap();
        std::fs::create_dir_all(plugins.join("alpha").join("agents")).unwrap();
        std::fs::create_dir_all(plugins.join("beta").join("agents")).unwrap();
        std::fs::write(
            kiana_home.join("agents").join("reviewer.md"),
            "---\nname: reviewer\ndescription: user reviewer\nmodel: sonnet\n---\nReview code.",
        )
        .unwrap();
        std::fs::write(
            project.join(".kiana").join("agents").join("reviewer.md"),
            "---\nname: reviewer\ndescription: project reviewer\nmemory: project\ntools: [Read, Grep]\ndisallowedTools: [Write]\npermissionMode: plan\nmaxTurns: 7\n---\nReview project code.",
        )
        .unwrap();
        std::fs::write(
            project
                .join(".claude")
                .join("agents-local")
                .join("local-agent.md"),
            "---\ndescription: local agent\n---\nLocal prompt.",
        )
        .unwrap();
        std::fs::write(
            plugins.join("alpha").join("agents").join("planner.json"),
            r#"{"name":"planner","description":"plans work","prompt":"Plan work.","model":"inherit"}"#,
        )
        .unwrap();
        std::fs::write(
            plugins
                .join("beta")
                .join("agents")
                .join("disabled-planner.json"),
            r#"{"name":"disabled-planner","description":"disabled","prompt":"Do not load."}"#,
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins, "beta", false).unwrap();

        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins);
        std::env::set_current_dir(&project).unwrap();
        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();
        let flags = RuntimeFlags {
            agents_json: Some(
                r#"{"reviewer":{"description":"flag reviewer","prompt":"Flag review."}}"#
                    .to_string(),
            ),
            ..RuntimeFlags::default()
        };

        let agents = discover_agents(&flags).unwrap();
        let reviewer_sources = agents
            .iter()
            .filter(|agent| agent.name == "reviewer")
            .map(|agent| (agent.source, agent.overridden_by))
            .collect::<Vec<_>>();
        assert!(reviewer_sources.contains(&(AgentSource::User, Some(AgentSource::Flag))));
        assert!(reviewer_sources.contains(&(AgentSource::Project, Some(AgentSource::Flag))));
        assert!(reviewer_sources.contains(&(AgentSource::Flag, None)));
        assert!(agents
            .iter()
            .any(|agent| agent.name == "local-agent" && agent.source == AgentSource::Local));
        assert!(agents.iter().any(|agent| {
            agent.name == "planner"
                && agent.source == AgentSource::Plugin
                && agent.plugin.as_deref() == Some("alpha")
        }));
        assert!(!agents.iter().any(|agent| agent.name == "disabled-planner"));
        let text = format_agents_text(&agents);
        assert!(text.contains("CLI arg agents:"));
        assert!(text.contains("(shadowed by flag) reviewer"));
        assert!(text.contains("planner · inherit · plugin: alpha"));
        let json = agents_json(&agents);
        assert!(json["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|agent| agent["name"] == "reviewer" && agent["source"] == "flag"));
        let project_reviewer = json["agents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|agent| agent["name"] == "reviewer" && agent["source"] == "project")
            .unwrap();
        assert_eq!(
            project_reviewer["tools"],
            serde_json::json!(["Read", "Grep"])
        );
        assert_eq!(
            project_reviewer["disallowed_tools"],
            serde_json::json!(["Write"])
        );
        assert_eq!(project_reviewer["permission_mode"], "plan");
        assert_eq!(project_reviewer["max_turns"], 7);
        assert!(agent_source_matches_filter(
            AgentSource::Project,
            &parse_setting_sources("user,project")
        ));
        assert!(!agent_source_matches_filter(
            AgentSource::Plugin,
            &parse_setting_sources("user,project")
        ));

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_flags_accept_settings_json() {
        let (args, flags) = extract_runtime_flags(vec![
            "--settings".to_string(),
            r#"{"model":"settings-model"}"#.to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(
            flags.settings,
            Some(r#"{"model":"settings-model"}"#.to_string())
        );
    }

    #[test]
    fn runtime_flags_accept_session_id_and_name() {
        let (args, flags) = extract_runtime_flags(vec![
            "--session-id".to_string(),
            "fixed-session".to_string(),
            "-n".to_string(),
            "Named session".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.session_id, Some("fixed-session".to_string()));
        assert_eq!(flags.session_name, Some("Named session".to_string()));

        let (args, flags) = extract_runtime_flags(vec![
            "-p".to_string(),
            "--name=Inline name".to_string(),
            "--session-id=fixed-session".to_string(),
            "hello".to_string(),
        ])
        .unwrap();
        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.session_id, Some("fixed-session".to_string()));
        assert_eq!(flags.session_name, Some("Inline name".to_string()));
    }

    #[test]
    fn runtime_flags_accept_no_session_persistence() {
        let (args, flags) = extract_runtime_flags(vec![
            "--no-session-persistence".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert!(flags.no_session_persistence);
    }

    #[test]
    fn runtime_flags_accept_add_dir_values() {
        let (args, flags) = extract_runtime_flags(vec![
            "--add-dir".to_string(),
            "reference".to_string(),
            "../shared".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.add_dirs, vec!["reference", "../shared"]);
    }

    #[test]
    fn runtime_flags_accept_file_set_values() {
        let (args, flags) = extract_runtime_flags(vec![
            "--editable-file".to_string(),
            "src/lib.rs".to_string(),
            "--editableFile=tests/foo.rs".to_string(),
            "--read-only-file".to_string(),
            "README.md".to_string(),
            "--readOnlyFile=docs/plan.md".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.editable_files, vec!["src/lib.rs", "tests/foo.rs"]);
        assert_eq!(flags.read_only_files, vec!["README.md", "docs/plan.md"]);
    }

    #[test]
    fn runtime_flags_accept_readonly_file_aliases() {
        let (args, flags) = extract_runtime_flags(vec![
            "--readonly-file".to_string(),
            "vendor/generated.rs".to_string(),
            "--readonlyFile=docs/archive.md".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(
            flags.read_only_files,
            vec!["vendor/generated.rs", "docs/archive.md"]
        );
    }

    #[test]
    fn runtime_flags_accept_mcp_config_values() {
        let (args, flags) = extract_runtime_flags(vec![
            "--mcp-config".to_string(),
            r#"{"mcpServers":{"one":{"command":"one-mcp"}}}"#.to_string(),
            "mcp.json".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.mcp_configs.len(), 2);
        assert!(flags.mcp_configs[0].contains("mcpServers"));
        assert_eq!(flags.mcp_configs[1], "mcp.json");
    }

    #[test]
    fn runtime_flags_accept_strict_mcp_config() {
        let (args, flags) = extract_runtime_flags(vec![
            "--strict-mcp-config".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert!(flags.strict_mcp_config);
    }

    #[test]
    fn runtime_flags_accept_bare_before_print_prompt() {
        let (args, flags) = extract_runtime_flags(vec![
            "--bare".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert!(flags.bare);

        let (args, flags) = extract_runtime_flags(vec![
            "-p".to_string(),
            "--bare".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert!(flags.bare);
    }

    #[test]
    fn runtime_flags_preserve_bare_after_print_prompt_text() {
        let (args, flags) = extract_runtime_flags(vec![
            "-p".to_string(),
            "explain".to_string(),
            "--bare".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "explain", "--bare"]);
        assert_eq!(flags, RuntimeFlags::default());
    }

    #[test]
    fn bare_sets_simple_env_and_clears_inherited_mcp() {
        let _guard = env_lock().lock().unwrap();
        clear_simple_env();
        std::env::set_var(
            kiana_tools::mcp_tool::MCP_SERVERS_ENV,
            r#"{"inherited":{"command":"inherited-mcp"}}"#,
        );

        apply_runtime_flags(&RuntimeFlags {
            bare: true,
            ..RuntimeFlags::default()
        })
        .unwrap();

        assert_eq!(
            std::env::var("CLAUDE_CODE_SIMPLE").ok().as_deref(),
            Some("1")
        );
        assert_eq!(
            std::env::var("KIANA_CODE_SIMPLE").ok().as_deref(),
            Some("1")
        );
        let servers: Value =
            serde_json::from_str(&std::env::var(kiana_tools::mcp_tool::MCP_SERVERS_ENV).unwrap())
                .unwrap();
        assert_eq!(servers, serde_json::json!({}));
        clear_simple_env();
        clear_mcp_servers_env();
    }

    #[test]
    fn bare_keeps_explicit_mcp_config() {
        let _guard = env_lock().lock().unwrap();
        clear_simple_env();
        clear_mcp_servers_env();

        apply_runtime_flags(&RuntimeFlags {
            bare: true,
            mcp_configs: vec![
                r#"{"mcpServers":{"explicit":{"command":"explicit-mcp"}}}"#.to_string()
            ],
            ..RuntimeFlags::default()
        })
        .unwrap();

        assert_eq!(
            std::env::var("CLAUDE_CODE_SIMPLE").ok().as_deref(),
            Some("1")
        );
        let servers: Value =
            serde_json::from_str(&std::env::var(kiana_tools::mcp_tool::MCP_SERVERS_ENV).unwrap())
                .unwrap();
        assert_eq!(servers["explicit"]["command"], "explicit-mcp");
        assert!(servers.get("inherited").is_none());
        clear_simple_env();
        clear_mcp_servers_env();
    }

    #[test]
    fn strict_mcp_config_clears_inherited_servers() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(
            kiana_tools::mcp_tool::MCP_SERVERS_ENV,
            r#"{"inherited":{"command":"inherited-mcp"}}"#,
        );

        apply_runtime_flags(&RuntimeFlags {
            strict_mcp_config: true,
            ..RuntimeFlags::default()
        })
        .unwrap();

        let servers: Value =
            serde_json::from_str(&std::env::var(kiana_tools::mcp_tool::MCP_SERVERS_ENV).unwrap())
                .unwrap();
        assert_eq!(servers, serde_json::json!({}));
        clear_mcp_servers_env();
    }

    #[test]
    fn strict_mcp_config_keeps_explicit_mcp_config() {
        let _guard = env_lock().lock().unwrap();
        clear_mcp_servers_env();

        apply_runtime_flags(&RuntimeFlags {
            strict_mcp_config: true,
            mcp_configs: vec![
                r#"{"mcpServers":{"explicit":{"command":"explicit-mcp"}}}"#.to_string()
            ],
            ..RuntimeFlags::default()
        })
        .unwrap();

        let servers: Value =
            serde_json::from_str(&std::env::var(kiana_tools::mcp_tool::MCP_SERVERS_ENV).unwrap())
                .unwrap();
        assert_eq!(servers["explicit"]["command"], "explicit-mcp");
        assert!(servers.get("inherited").is_none());
        clear_mcp_servers_env();
    }

    #[test]
    fn mcp_config_flags_merge_inline_and_file_configs() {
        let _guard = env_lock().lock().unwrap();
        clear_mcp_servers_env();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kiana-mcp-config-{}-{unique}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"{"mcpServers":{"file-server":{"url":"http://127.0.0.1/mcp","type":"http"}}}"#,
        )
        .unwrap();

        apply_mcp_config_flags(&[
            r#"{"mcpServers":{"inline":{"command":"inline-mcp","args":["--stdio"]}}}"#.to_string(),
            path.to_string_lossy().to_string(),
        ])
        .unwrap();

        let servers: Value =
            serde_json::from_str(&std::env::var(kiana_tools::mcp_tool::MCP_SERVERS_ENV).unwrap())
                .unwrap();
        assert_eq!(servers["inline"]["command"], "inline-mcp");
        assert_eq!(servers["inline"]["args"][0], "--stdio");
        assert_eq!(servers["file-server"]["url"], "http://127.0.0.1/mcp");
        assert_eq!(servers["file-server"]["type"], "http");

        let _ = std::fs::remove_file(path);
        clear_mcp_servers_env();
    }

    #[tokio::test]
    async fn stream_json_init_reports_mcp_servers_from_env() {
        let _guard = env_lock().lock().unwrap();
        clear_mcp_servers_env();
        std::env::set_var(
            kiana_tools::mcp_tool::MCP_SERVERS_ENV,
            r#"{"docs":{"command":"docs-mcp"}}"#,
        );

        let init = stream_json_init_event("session-1").await.unwrap();

        assert_eq!(init["mcp_servers"]["docs"]["command"], "docs-mcp");
        clear_mcp_servers_env();
    }

    #[tokio::test]
    async fn stream_json_init_reports_output_styles() {
        let _guard = env_lock().lock().unwrap();
        clear_settings_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-output-style-init-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = root.join("kiana-home");
        let cwd = root.join("project");
        let styles_dir = cwd.join(".claude").join("output-styles");
        let plugins_dir = root.join("plugins");
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_PLUGINS_DIR",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
        ]);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        std::fs::create_dir_all(cwd.join(".kiana")).unwrap();
        std::fs::create_dir_all(&styles_dir).unwrap();
        std::fs::create_dir_all(&plugins_dir).unwrap();
        std::fs::write(cwd.join(".kiana").join("trust.json"), r#"{"trusted":true}"#).unwrap();
        std::fs::write(
            styles_dir.join("Local.md"),
            "---\ndescription: Local style\n---\nUse the local style.\n",
        )
        .unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        std::env::set_var(
            "KIANA_SETTINGS_JSON",
            r#"{"settings":{"output_style":"Local"}}"#,
        );
        let current_dir = CurrentDirGuard::set(&cwd);

        let unknown = stream_json_init_event("session-unknown").await.unwrap();
        assert_eq!(unknown["output_style"], "default");
        assert!(!unknown["available_output_styles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|style| style.as_str() == Some("Local")));

        kiana_types::write_project_trust(&cwd, kiana_types::ProjectTrust::Trusted).unwrap();
        let trusted = stream_json_init_event("session-trusted").await.unwrap();
        assert_eq!(trusted["output_style"], "Local");
        assert!(trusted["available_output_styles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|style| style.as_str() == Some("Local")));
        assert!(trusted["available_output_styles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|style| style.as_str() == Some("default")));

        drop(current_dir);
        clear_settings_env();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stream_json_init_reports_visible_skills_and_plugins() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-stream-json-capabilities-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = root.join("kiana-home");
        let project = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        let previous_cwd = std::env::current_dir().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_PLUGINS_DIR",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
        ]);

        std::fs::create_dir_all(project.join(".claude").join("skills").join("project-audit"))
            .unwrap();
        std::fs::write(
            project
                .join(".claude")
                .join("skills")
                .join("project-audit")
                .join("SKILL.md"),
            "---\ndescription: Audit this project\n---\nUse this project skill.\n",
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"review-tools","version":"1.0.0","description":"Review helpers"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("skills").join("code-audit")).unwrap();
        std::fs::write(
            plugin_root
                .join("skills")
                .join("code-audit")
                .join("SKILL.md"),
            "---\ndescription: Audit code from plugin\n---\nUse this plugin skill.\n",
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("commands")).unwrap();
        std::fs::write(
            plugin_root.join("commands").join("review.md"),
            "---\ndescription: Review command\n---\nReview this code.\n",
        )
        .unwrap();

        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        std::env::set_current_dir(&project).unwrap();
        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();

        let init = stream_json_init_event("session-1").await.unwrap();
        let skill_names = init["skills"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|skill| skill.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(skill_names.contains(&"project-audit"));
        assert!(skill_names.contains(&"review-tools:code-audit"));
        assert!(init["slash_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command.as_str() == Some("review-tools:review")));
        let plugin = init["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| plugin.get("name").and_then(Value::as_str) == Some("review-tools"))
            .expect("plugin summary visible");
        assert_eq!(plugin["enabled"], true);
        assert_eq!(plugin["valid"], true);
        assert_eq!(plugin["components"]["skills"], 1);
        assert_eq!(plugin["components"]["commands"], 1);

        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "review-tools", false).unwrap();
        kiana_skills::clear_caches();
        let disabled_init = stream_json_init_event("session-1").await.unwrap();
        let disabled_plugin = disabled_init["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|plugin| plugin.get("name").and_then(Value::as_str) == Some("review-tools"))
            .expect("disabled plugin summary visible");
        assert_eq!(disabled_plugin["enabled"], false);
        assert!(!disabled_init["skills"]
            .as_array()
            .unwrap()
            .iter()
            .any(|skill| skill.get("name").and_then(Value::as_str)
                == Some("review-tools:code-audit")));
        assert!(!disabled_init["slash_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|command| command.as_str() == Some("review-tools:review")));

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stream_json_skill_summaries_hide_project_skills_until_user_store_trusts_project() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-stream-json-project-trust-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = root.join("kiana-home");
        let project = root.join("project");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        let _env = EnvSnapshot::take(&["HOME", "USERPROFILE", "KIANA_HOME", "KIANA_PLUGINS_DIR"]);

        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".claude").join("skills").join("project-audit"))
            .unwrap();
        std::fs::write(
            project
                .join(".claude")
                .join("skills")
                .join("project-audit")
                .join("SKILL.md"),
            "---\ndescription: Audit this project\n---\nUse this project skill.\n",
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"review-tools","version":"1.0.0","description":"Review helpers"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("skills").join("code-audit")).unwrap();
        std::fs::write(
            plugin_root
                .join("skills")
                .join("code-audit")
                .join("SKILL.md"),
            "---\ndescription: Audit code from plugin\n---\nUse this plugin skill.\n",
        )
        .unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::write(
            project.join(".kiana").join("trust.json"),
            r#"{"trusted":true}"#,
        )
        .unwrap();
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let unknown = stream_json_skill_summaries(&project).await;
        assert!(!unknown
            .as_array()
            .unwrap()
            .iter()
            .any(|skill| { skill.get("name").and_then(Value::as_str) == Some("project-audit") }));
        assert!(unknown.as_array().unwrap().iter().any(|skill| {
            skill.get("name").and_then(Value::as_str) == Some("review-tools:code-audit")
        }));

        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();
        let trusted = stream_json_skill_summaries(&project).await;
        assert!(trusted
            .as_array()
            .unwrap()
            .iter()
            .any(|skill| { skill.get("name").and_then(Value::as_str) == Some("project-audit") }));
        assert!(trusted.as_array().unwrap().iter().any(|skill| {
            skill.get("name").and_then(Value::as_str) == Some("review-tools:code-audit")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn local_command_propagates_cwd_for_external_project_trust() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["HOME", "USERPROFILE", "KIANA_HOME"]);
        let root = std::env::temp_dir().join(format!(
            "kiana-local-command-project-trust-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project");
        let kiana_home = root.join("kiana-home");
        let plugin_root = project.join(".kiana").join("plugins").join("review-tools");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            r#"{"name":"review-tools","version":"1.0.0"}"#,
        )
        .unwrap();
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        let _cwd = CurrentDirGuard::set(&project);
        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();

        let result = run_local_command(
            &[
                "skills".to_string(),
                "audit".to_string(),
                "--json".to_string(),
            ],
            &RuntimeFlags::default(),
        )
        .await
        .unwrap()
        .unwrap();
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert!(report["plugin_load_audit"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry["plugin"] == "review-tools" && entry["status"] == "no-skills" }));

        drop(_cwd);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn apply_add_dirs_flag_sets_access_roots_env() {
        let _guard = env_lock().lock().unwrap();
        clear_access_roots_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-add-dir-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();

        apply_add_dirs_flag(&[root.to_string_lossy().to_string()]).unwrap();

        let roots = std::env::var_os(kiana_tools::tool::ACCESS_ROOTS_ENV)
            .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(roots, vec![std::fs::canonicalize(&root).unwrap()]);

        let _ = std::fs::remove_dir_all(root);
        clear_access_roots_env();
    }

    #[test]
    fn apply_settings_flag_sets_json_env() {
        let _guard = env_lock().lock().unwrap();
        clear_settings_env();

        apply_settings_flag(r#"{"model":"settings-model"}"#).unwrap();

        assert_eq!(
            std::env::var("KIANA_SETTINGS_JSON").ok().as_deref(),
            Some(r#"{"model":"settings-model"}"#)
        );
        assert!(std::env::var("KIANA_SETTINGS_FILE").is_err());
        clear_settings_env();
    }

    #[test]
    fn apply_settings_flag_sets_file_env_and_validates_json_file() {
        let _guard = env_lock().lock().unwrap();
        clear_settings_env();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kiana-settings-{}-{unique}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{"model":"file-model"}"#).unwrap();

        apply_settings_flag(path.to_str().unwrap()).unwrap();

        assert_eq!(
            std::env::var("KIANA_SETTINGS_FILE").ok().as_deref(),
            Some(path.to_str().unwrap())
        );
        assert!(std::env::var("KIANA_SETTINGS_JSON").is_err());
        let _ = std::fs::remove_file(path);
        clear_settings_env();
    }

    #[test]
    fn apply_settings_flag_rejects_invalid_json() {
        let error = apply_settings_flag("{bad json}").unwrap_err().to_string();

        assert!(error.contains("--settings JSON is invalid"));
    }

    #[test]
    fn runtime_flags_accept_tools_list() {
        let (args, flags) = extract_runtime_flags(vec![
            "--tools".to_string(),
            "Read".to_string(),
            "Bash".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.tools, Some("Read Bash".to_string()));
    }

    #[test]
    fn runtime_flags_accept_empty_tools_list() {
        let (args, flags) = extract_runtime_flags(vec![
            "--tools=".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.tools, Some(String::new()));
    }

    #[test]
    fn runtime_flags_accept_system_prompt_flags() {
        let (args, flags) = extract_runtime_flags(vec![
            "--system-prompt".to_string(),
            "base instructions".to_string(),
            "--append-system-prompt=extra instructions".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "hello"]);
        assert_eq!(flags.system_prompt, Some("base instructions".to_string()));
        assert_eq!(
            flags.append_system_prompt,
            Some("extra instructions".to_string())
        );
    }

    #[test]
    fn runtime_flags_reject_system_prompt_conflict() {
        let error = extract_runtime_flags(vec![
            "--system-prompt".to_string(),
            "inline".to_string(),
            "--system-prompt-file".to_string(),
            "prompt.txt".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("cannot use both --system-prompt"));
    }

    #[test]
    fn runtime_text_flag_reads_file() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "kiana-system-prompt-{}-{unique}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "from file\n").unwrap();

        let value = runtime_text_flag(
            "--system-prompt",
            None,
            "--system-prompt-file",
            Some(path.to_str().unwrap()),
        )
        .unwrap();

        assert_eq!(value, Some("from file".to_string()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn runtime_flags_reject_invalid_max_turns() {
        let error = extract_runtime_flags(vec![
            "--max-turns".to_string(),
            "0".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("--max-turns requires a positive integer"));
    }

    #[test]
    fn runtime_flags_reject_invalid_api_timeout() {
        let error = extract_runtime_flags(vec![
            "--api-timeout-ms".to_string(),
            "0".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("--api-timeout-ms requires a positive integer"));
    }

    #[test]
    fn runtime_flags_reject_same_fallback_model() {
        let error = extract_runtime_flags(vec![
            "--model".to_string(),
            "same-model".to_string(),
            "--fallback-model".to_string(),
            "same-model".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();

        assert!(error.contains("Fallback model cannot be the same"));
    }

    #[test]
    fn prompt_runtime_options_include_fallback_model() {
        let mut options = HashMap::new();
        apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                session_id: Some("session-1".to_string()),
                fallback_model: Some("fallback-model".to_string()),
                api_timeout_ms: Some("2500".to_string()),
                permission_prompt_tool: Some("perm.approve".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(options["session_id"], "session-1");
        assert_eq!(options["fallback_model"], "fallback-model");
        assert_eq!(options["api_timeout_ms"], "2500");
        assert_eq!(options["permission_prompt_tool"], "perm.approve");
    }

    #[test]
    fn prompt_runtime_options_include_file_sets() {
        let mut options = HashMap::new();
        apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                editable_files: vec!["src/lib.rs".to_string(), "tests/foo.rs".to_string()],
                read_only_files: vec!["README.md".to_string(), "docs/plan.md".to_string()],
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(
            options["editable_files"],
            serde_json::json!(["src/lib.rs", "tests/foo.rs"])
        );
        assert_eq!(
            options["read_only_files"],
            serde_json::json!(["README.md", "docs/plan.md"])
        );
    }

    #[test]
    fn prompt_runtime_options_include_repair_loop_settings() {
        let mut options = HashMap::new();
        apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                repair_checks: true,
                repair_check_attempts: Some("2".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(options["repairChecks"], true);
        assert_eq!(options["repairCheckAttempts"], "2");
    }

    #[test]
    fn prompt_runtime_options_apply_selected_agent() {
        let mut options = HashMap::new();
        let initial_prompt = apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                agent: Some("reviewer".to_string()),
                agents_json: Some(
                    r#"{"reviewer":{"description":"reviews code","prompt":"You review code.","initialPrompt":"First inspect the diff.","model":"agent-model","tools":["Read","Grep"],"disallowedTools":["Write"],"permissionMode":"plan","maxTurns":2}}"#.to_string(),
                ),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(options["agent"], "reviewer");
        assert_eq!(options["system_prompt"], "You review code.");
        assert_eq!(options["model"], "agent-model");
        assert_eq!(options["tools"], serde_json::json!(["Read", "Grep"]));
        assert_eq!(options["disallowed_tools"], serde_json::json!(["Write"]));
        assert_eq!(options["permission_mode"], "plan");
        assert_eq!(options["max_iterations"], 2);
        assert_eq!(initial_prompt.as_deref(), Some("First inspect the diff."));
        assert_eq!(
            prepend_initial_prompt("hello".to_string(), initial_prompt.as_deref()),
            "First inspect the diff.\n\nhello"
        );
    }

    #[test]
    fn unknown_project_trust_rejects_selected_project_agent() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-selected-agent-unknown-trust-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = root.join("kiana-home");
        let project = root.join("project");
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".claude").join("agents")).unwrap();
        std::fs::write(
            project
                .join(".claude")
                .join("agents")
                .join("project-only.md"),
            "---\ndescription: project-only agent\n---\nProject-controlled prompt.",
        )
        .unwrap();

        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::remove_var("KIANA_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_JSON");
        std::env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        let cwd = CurrentDirGuard::set(&project);

        let error = selected_cli_agent(&RuntimeFlags {
            agent: Some("project-only".to_string()),
            ..RuntimeFlags::default()
        })
        .unwrap_err()
        .to_string();

        assert!(error.contains("--agent 'project-only' was not found"));

        drop(cwd);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prompt_runtime_options_apply_discovered_project_agent() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-agent-runtime-project-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = home.join(".kiana");
        let project = root.join("project");
        let previous_cwd = std::env::current_dir().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);

        std::fs::create_dir_all(&kiana_home).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".claude").join("agents")).unwrap();
        std::fs::write(
            project.join(".claude").join("agents").join("reviewer.md"),
            "---\nname: reviewer\ndescription: reviews code\nmodel: agent-model\ninitialPrompt: First inspect the repository.\ntools: [Read, Grep]\ndisallowedTools: [Write]\npermissionMode: plan\nmaxTurns: 2\n---\nYou review code from the project agent file.",
        )
        .unwrap();

        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        std::env::set_current_dir(&project).unwrap();
        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();

        let mut options = HashMap::new();
        let initial_prompt = apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                agent: Some("reviewer".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(options["agent"], "reviewer");
        assert_eq!(
            options["system_prompt"],
            "You review code from the project agent file."
        );
        assert_eq!(options["model"], "agent-model");
        assert_eq!(options["tools"], serde_json::json!(["Read", "Grep"]));
        assert_eq!(options["disallowed_tools"], serde_json::json!(["Write"]));
        assert_eq!(options["permission_mode"], "plan");
        assert_eq!(options["max_iterations"], 2);
        assert_eq!(
            initial_prompt.as_deref(),
            Some("First inspect the repository.")
        );

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prompt_runtime_options_apply_builtin_agent() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-agent-runtime-builtin-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = home.join(".kiana");
        let project = root.join("project");
        let previous_cwd = std::env::current_dir().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);
        std::fs::create_dir_all(&project).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        std::env::set_current_dir(&project).unwrap();

        let mut options = HashMap::new();
        apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                agent: Some("claude-code-guide".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(options["agent"], "claude-code-guide");
        assert_eq!(options["model"], "haiku");
        assert_eq!(options["permission_mode"], "dontAsk");
        assert_eq!(
            options["tools"],
            serde_json::json!(["Glob", "Grep", "Read", "WebFetch", "WebSearch"])
        );
        assert!(options["system_prompt"]
            .as_str()
            .unwrap()
            .contains("official documentation"));

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prompt_runtime_options_selects_highest_precedence_discovered_agent() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-agent-runtime-shadow-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = root.join("kiana-home");
        let project = root.join("project");
        let _env = EnvSnapshot::take(&[
            "HOME",
            "USERPROFILE",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);

        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".kiana").join("agents")).unwrap();
        std::fs::create_dir_all(project.join(".claude").join("agents-local")).unwrap();
        std::fs::write(
            project.join(".kiana").join("agents").join("reviewer.md"),
            "---\nname: reviewer\n---\nProject prompt.",
        )
        .unwrap();
        std::fs::write(
            project
                .join(".claude")
                .join("agents-local")
                .join("reviewer.md"),
            "---\nname: reviewer\n---\nLocal prompt.",
        )
        .unwrap();

        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::remove_var("KIANA_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_JSON");
        std::env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        let cwd = CurrentDirGuard::set(&project);
        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();

        let mut options = HashMap::new();
        apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                agent: Some("reviewer".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert_eq!(options["system_prompt"], "Local prompt.");

        drop(cwd);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prompt_runtime_options_keep_explicit_system_prompt_over_agent() {
        let mut options = HashMap::new();
        apply_prompt_runtime_options(
            &mut options,
            &RuntimeFlags {
                agent: Some("reviewer".to_string()),
                agents_json: Some(r#"{"reviewer":{"prompt":"agent prompt"}}"#.to_string()),
                system_prompt: Some("explicit prompt".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap();

        assert!(options.get("system_prompt").is_none());
        assert_eq!(options["agent"], "reviewer");
    }

    #[test]
    fn prompt_runtime_options_reject_missing_or_invalid_agents() {
        let _guard = env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-agent-runtime-missing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = root.join("home");
        let kiana_home = home.join(".kiana");
        let project = root.join("project");
        let previous_cwd = std::env::current_dir().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "KIANA_REMOTE_SETTINGS_FILE",
            "KIANA_PLUGINS_DIR",
        ]);
        std::fs::create_dir_all(&project).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::env::set_var("KIANA_CONFIG_FILE", root.join("missing-config.toml"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        std::env::set_current_dir(&project).unwrap();

        let error = apply_prompt_runtime_options(
            &mut HashMap::new(),
            &RuntimeFlags {
                agent: Some("reviewer".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("--agent 'reviewer' was not found"));

        let error = apply_prompt_runtime_options(
            &mut HashMap::new(),
            &RuntimeFlags {
                agents_json: Some("[]".to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("--agents must be a JSON object"));

        let error = apply_prompt_runtime_options(
            &mut HashMap::new(),
            &RuntimeFlags {
                agent: Some("reviewer".to_string()),
                agents_json: Some(r#"{"reviewer":{"description":"no prompt"}}"#.to_string()),
                ..RuntimeFlags::default()
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("requires a prompt"));

        std::env::set_current_dir(previous_cwd).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_flags_are_stripped_after_command_before_positional_args() {
        let (args, flags) = extract_runtime_flags(vec![
            "reply".to_string(),
            "--disallowed-tools".to_string(),
            "Bash".to_string(),
            "session-1".to_string(),
            "hello".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["reply", "session-1", "hello"]);
        assert_eq!(flags.disallowed_tools, Some("Bash".to_string()));
    }

    #[test]
    fn runtime_flags_inside_prompt_text_are_preserved() {
        let (args, flags) = extract_runtime_flags(vec![
            "reply".to_string(),
            "session-1".to_string(),
            "explain".to_string(),
            "--permission-mode".to_string(),
            "ask".to_string(),
        ])
        .unwrap();

        assert_eq!(
            args,
            vec!["reply", "session-1", "explain", "--permission-mode", "ask"]
        );
        assert_eq!(flags, RuntimeFlags::default());
    }

    #[test]
    fn runtime_flags_inside_print_prompt_text_are_preserved() {
        let (args, flags) = extract_runtime_flags(vec![
            "-p".to_string(),
            "explain".to_string(),
            "--permission-mode".to_string(),
            "ask".to_string(),
            "--session-id".to_string(),
            "literal".to_string(),
            "--no-session-persistence".to_string(),
        ])
        .unwrap();

        assert_eq!(
            args,
            vec![
                "-p",
                "explain",
                "--permission-mode",
                "ask",
                "--session-id",
                "literal",
                "--no-session-persistence"
            ]
        );
        assert_eq!(flags, RuntimeFlags::default());
    }

    #[test]
    fn runtime_flags_before_print_prompt_text_are_stripped() {
        let (args, flags) = extract_runtime_flags(vec![
            "-p".to_string(),
            "--permission-mode".to_string(),
            "ask".to_string(),
            "explain".to_string(),
        ])
        .unwrap();

        assert_eq!(args, vec!["-p", "explain"]);
        assert_eq!(flags.permission_mode, Some("ask".to_string()));
    }

    #[test]
    fn print_args_default_to_model_execution() {
        let args = vec!["-p".to_string(), "hello".to_string(), "there".to_string()];

        assert_eq!(
            parse_print_args(&args).unwrap(),
            Some(PrintArgs {
                message: "hello there".to_string(),
                input_format: PrintInputFormat::Text,
                output_format: PrintOutputFormat::Text,
                execute: true,
                resident_teammate: false,
                json_schema: None,
                sdk_url: None,
                replay_user_messages: false,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn print_args_accept_resident_teammate_flag() {
        let args = vec![
            "-p".to_string(),
            "--resident-teammate".to_string(),
            "watch inbox".to_string(),
        ];

        let parsed = parse_print_args(&args).unwrap().unwrap();

        assert_eq!(parsed.message, "watch inbox");
        assert!(parsed.execute);
        assert!(parsed.resident_teammate);
    }

    #[test]
    fn print_args_accept_output_format_and_json_schema_before_message() {
        let args = vec![
            "--print".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--json-schema".to_string(),
            r#"{"type":"object","required":["answer"]}"#.to_string(),
            "answer".to_string(),
            "as".to_string(),
            "json".to_string(),
        ];

        let parsed = parse_print_args(&args).unwrap().unwrap();

        assert_eq!(parsed.message, "answer as json");
        assert_eq!(parsed.output_format, PrintOutputFormat::Json);
        assert!(parsed.execute);
        assert_eq!(parsed.json_schema.unwrap()["required"][0], "answer");
    }

    #[test]
    fn print_args_accept_print_flags_before_print_marker() {
        let args = vec![
            "--output-format=json".to_string(),
            "--record-only".to_string(),
            "-p".to_string(),
            "answer".to_string(),
        ];

        assert_eq!(
            parse_print_args(&args).unwrap(),
            Some(PrintArgs {
                message: "answer".to_string(),
                input_format: PrintInputFormat::Text,
                output_format: PrintOutputFormat::Json,
                execute: false,
                resident_teammate: false,
                json_schema: None,
                sdk_url: None,
                replay_user_messages: false,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn print_args_accept_stream_json_output_format() {
        let args = vec![
            "--print".to_string(),
            "--output-format=stream-json".to_string(),
            "--record-only".to_string(),
            "hello".to_string(),
        ];

        assert_eq!(
            parse_print_args(&args).unwrap(),
            Some(PrintArgs {
                message: "hello".to_string(),
                input_format: PrintInputFormat::Text,
                output_format: PrintOutputFormat::StreamJson,
                execute: false,
                resident_teammate: false,
                json_schema: None,
                sdk_url: None,
                replay_user_messages: false,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn print_args_accept_include_partial_messages_for_stream_json() {
        let args = vec![
            "--print".to_string(),
            "--output-format=stream-json".to_string(),
            "--include-partial-messages".to_string(),
            "hello".to_string(),
        ];

        let parsed = parse_print_args(&args).unwrap().unwrap();

        assert_eq!(parsed.message, "hello");
        assert_eq!(parsed.output_format, PrintOutputFormat::StreamJson);
        assert!(parsed.include_partial_messages);
    }

    #[test]
    fn print_args_accept_stream_json_input_without_prompt() {
        let args = vec![
            "--print".to_string(),
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--replay-user-messages".to_string(),
            "--record-only".to_string(),
        ];

        assert_eq!(
            parse_print_args(&args).unwrap(),
            Some(PrintArgs {
                message: String::new(),
                input_format: PrintInputFormat::StreamJson,
                output_format: PrintOutputFormat::StreamJson,
                execute: false,
                resident_teammate: false,
                json_schema: None,
                sdk_url: None,
                replay_user_messages: true,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn print_args_accept_sdk_url_for_bridge_stream_json_mode() {
        let args = vec![
            "--print".to_string(),
            "--sdk-url".to_string(),
            "wss://example.test/v1/session_ingress/ws/session-1".to_string(),
            "--debug-file".to_string(),
            "/tmp/bridge-session.log".to_string(),
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--replay-user-messages".to_string(),
        ];

        let parsed = parse_print_args(&args).unwrap().unwrap();

        assert_eq!(parsed.message, "");
        assert_eq!(
            parsed.sdk_url.as_deref(),
            Some("wss://example.test/v1/session_ingress/ws/session-1")
        );
        assert_eq!(parsed.input_format, PrintInputFormat::StreamJson);
        assert_eq!(parsed.output_format, PrintOutputFormat::StreamJson);
        assert!(parsed.replay_user_messages);
    }

    #[test]
    fn print_args_reject_invalid_stream_json_io_combinations() {
        let error = parse_print_args(&[
            "-p".to_string(),
            "--input-format=stream-json".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("--input-format=stream-json requires"));

        let error = parse_print_args(&[
            "-p".to_string(),
            "--output-format=stream-json".to_string(),
            "--replay-user-messages".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("--replay-user-messages requires"));

        let error = parse_print_args(&[
            "-p".to_string(),
            "--include-partial-messages".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("--include-partial-messages requires"));

        let error = parse_print_args(&[
            "-p".to_string(),
            "--sdk-url".to_string(),
            "wss://example.test/v1/session_ingress/ws/session-1".to_string(),
            "hello".to_string(),
        ])
        .unwrap_err()
        .to_string();
        assert!(error.contains("--sdk-url requires both --input-format=stream-json"));
    }

    #[test]
    fn stream_json_input_extracts_user_text_and_replay_events() {
        let raw = r#"{"type":"user","message":{"role":"user","content":"first"},"parent_tool_use_id":null}
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"second"}]},"parent_tool_use_id":null,"session_id":"remote"}
{"type":"user","message":{"role":"user","content":"tool result"},"parent_tool_use_id":"toolu_1"}
{"type":"control_response","response":{"subtype":"success"}}
{"type":"control_cancel_request","request_id":"perm-1"}
"#;

        let input = parse_stream_json_input(raw, "prefix", true).unwrap();

        assert_eq!(input.prompt, "prefix\nfirst\nsecond");
        assert_eq!(input.replay_events.len(), 4);
        assert_eq!(input.replay_events[0]["type"], "user");
        assert_eq!(input.replay_events[3]["type"], "control_response");
    }

    #[test]
    fn stream_json_input_tolerates_reference_history_events() {
        let raw = r#"{"type":"system","subtype":"init","session_id":"session-old"}
{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"previous answer"}]},"parent_tool_use_id":null,"session_id":"session-old"}
{"type":"control_response","response":{"request_id":"perm-1","subtype":"success"}}
{"type":"unknown_future_event","payload":true}
{"payload":"missing type is ignored like reference structured input"}
{"type":"user","message":{"role":"user","content":"next question"},"parent_tool_use_id":null}
"#;

        let input = parse_stream_json_input(raw, "", true).unwrap();

        assert_eq!(input.prompt, "next question");
        assert_eq!(input.replay_events.len(), 3);
        assert_eq!(input.replay_events[0]["type"], "assistant");
        assert_eq!(
            input.replay_events[0]["message"]["content"][0]["text"],
            "previous answer"
        );
        assert_eq!(input.replay_events[1]["type"], "control_response");
        assert_eq!(input.replay_events[2]["type"], "user");
        assert_eq!(input.history_messages.len(), 1);
        assert_eq!(input.history_messages[0]["role"], "assistant");
        assert_eq!(
            input.history_messages[0]["content"][0]["text"],
            "previous answer"
        );
    }

    #[test]
    fn stream_json_input_stops_at_end_session_control_request() {
        let raw = r#"{"type":"user","uuid":"user-1","message":{"role":"user","content":"first prompt"}}
{"type":"control_request","request_id":"end-1","request":{"subtype":"end_session","reason":"remote closed"}}
{"type":"user","uuid":"user-2","message":{"role":"user","content":"must not run"}}
"#;

        let input = parse_stream_json_input(raw, "", true).unwrap();

        assert_eq!(input.prompt, "first prompt");
        assert_eq!(input.replay_events.len(), 1);
        assert_eq!(input.replay_events[0]["uuid"], "user-1");
    }

    #[test]
    fn stream_json_input_ignores_history_events_without_replay() {
        let raw = r#"{"type":"assistant","message":{"role":"assistant","content":"previous answer"}}
{"type":"control_response","response":{"request_id":"perm-1","subtype":"success"}}
{"type":"user","message":{"role":"user","content":"next question"}}
"#;

        let input = parse_stream_json_input(raw, "", false).unwrap();

        assert_eq!(input.prompt, "next question");
        assert!(input.replay_events.is_empty());
        assert_eq!(input.history_messages.len(), 1);
        assert_eq!(input.history_messages[0]["role"], "assistant");
    }

    #[test]
    fn bridge_permission_request_event_includes_reference_permission_context() {
        let event = bridge_permission_request_event(&PermissionPromptRequest {
            request_id: "perm-1".to_string(),
            tool_name: "Write".to_string(),
            input: serde_json::json!({"file_path": "src/lib.rs"}),
            tool_use_id: "toolu_1".to_string(),
            permission_suggestions: serde_json::json!([
                {
                    "type": "addRules",
                    "destination": "session",
                    "rules": [{"toolName": "Write"}],
                    "behavior": "allow"
                }
            ]),
            blocked_path: Some("src/lib.rs".to_string()),
            decision_reason: serde_json::json!({
                "type": "other",
                "reason": "Tool Write requires permission in ask mode."
            }),
            agent_id: Some("researcher@review".to_string()),
        });

        assert_eq!(event["type"], "control_request");
        assert_eq!(event["request_id"], "perm-1");
        assert_eq!(event["request"]["subtype"], "can_use_tool");
        assert_eq!(event["request"]["tool_name"], "Write");
        assert_eq!(event["request"]["tool_use_id"], "toolu_1");
        assert_eq!(
            event["request"]["permission_suggestions"][0]["type"],
            "addRules"
        );
        assert_eq!(event["request"]["blocked_path"], "src/lib.rs");
        assert_eq!(event["request"]["decision_reason"]["type"], "other");
        assert_eq!(event["request"]["agent_id"], "researcher@review");
    }

    #[test]
    fn bridge_control_request_updates_local_stream_json_options() {
        let _guard = env_lock().lock().unwrap();
        let old_permission_mode = std::env::var("KIANA_PERMISSION_MODE").ok();
        let old_max_thinking_tokens = std::env::var("KIANA_MAX_THINKING_TOKENS").ok();
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_MAX_THINKING_TOKENS");
        let mut options = HashMap::new();

        let permission_response = handle_bridge_control_request(
            &serde_json::json!({
                "type": "control_request",
                "request_id": "req-mode",
                "request": {
                    "subtype": "set_permission_mode",
                    "mode": "accept-edits"
                }
            }),
            &mut options,
        );
        assert_eq!(permission_response["response"]["subtype"], "success");
        assert_eq!(permission_response["response"]["request_id"], "req-mode");
        assert_eq!(options["permission_mode"], "acceptEdits");
        assert_eq!(options["permissionMode"], "acceptEdits");
        assert_eq!(
            std::env::var("KIANA_PERMISSION_MODE").as_deref(),
            Ok("acceptEdits")
        );

        let tokens_response = handle_bridge_control_request(
            &serde_json::json!({
                "type": "control_request",
                "request_id": "req-tokens",
                "request": {
                    "subtype": "set_max_thinking_tokens",
                    "max_thinking_tokens": 2048
                }
            }),
            &mut options,
        );
        assert_eq!(tokens_response["response"]["subtype"], "success");
        assert_eq!(options["max_thinking_tokens"], 2048);
        assert_eq!(
            std::env::var("KIANA_MAX_THINKING_TOKENS").as_deref(),
            Ok("2048")
        );

        let clear_tokens_response = handle_bridge_control_request(
            &serde_json::json!({
                "type": "control_request",
                "request_id": "req-clear-tokens",
                "request": {
                    "subtype": "set_max_thinking_tokens",
                    "max_thinking_tokens": null
                }
            }),
            &mut options,
        );
        assert_eq!(clear_tokens_response["response"]["subtype"], "success");
        assert!(options.get("max_thinking_tokens").is_none());
        assert!(std::env::var("KIANA_MAX_THINKING_TOKENS").is_err());

        let mcp_status_response = handle_bridge_control_request(
            &serde_json::json!({
                "type": "control_request",
                "request_id": "req-mcp-status",
                "request": {
                    "subtype": "mcp_status"
                }
            }),
            &mut options,
        );
        assert_eq!(mcp_status_response["response"]["subtype"], "success");
        assert_eq!(
            mcp_status_response["response"]["request_id"],
            "req-mcp-status"
        );
        assert_eq!(
            mcp_status_response["response"]["response"]["mcpServers"],
            serde_json::json!([])
        );

        let unknown_response = handle_bridge_control_request(
            &serde_json::json!({
                "type": "control_request",
                "request_id": "req-unknown",
                "request": {
                    "subtype": "future_control"
                }
            }),
            &mut options,
        );
        assert_eq!(unknown_response["response"]["subtype"], "error");
        assert!(unknown_response["response"]["error"]
            .as_str()
            .unwrap()
            .contains("does not handle control_request subtype"));

        match old_permission_mode {
            Some(value) => std::env::set_var("KIANA_PERMISSION_MODE", value),
            None => std::env::remove_var("KIANA_PERMISSION_MODE"),
        }
        match old_max_thinking_tokens {
            Some(value) => std::env::set_var("KIANA_MAX_THINKING_TOKENS", value),
            None => std::env::remove_var("KIANA_MAX_THINKING_TOKENS"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bridge_stream_json_loop_end_session_exits_before_followup_user() {
        let input = br#"{"type":"control_request","request_id":"end-1","request":{"subtype":"end_session","reason":"remote closed"}}
{"type":"user","message":{"role":"user","content":"this should not reach the model"},"parent_tool_use_id":null}
"#;
        let reader = tokio::io::BufReader::new(&input[..]);
        let mut output = Vec::new();

        print_bridge_stream_json_loop_with_io(
            String::new(),
            HashMap::from([("execute".to_string(), Value::Bool(true))]),
            None,
            reader,
            &mut output,
        )
        .await
        .unwrap();

        let lines = String::from_utf8(output).unwrap();
        let events = lines
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 1, "{lines}");
        assert_eq!(events[0]["type"], "control_response");
        assert_eq!(events[0]["response"]["subtype"], "success");
        assert_eq!(events[0]["response"]["request_id"], "end-1");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bridge_stream_json_loop_round_trips_permission_prompt_and_result_event() {
        let _guard = env_lock().lock().unwrap();
        let old_tasks_root = take_env("KIANA_TASKS_ROOT");
        let old_agent_id = take_env("KIANA_AGENT_ID");
        let old_permission_mode = take_env("KIANA_PERMISSION_MODE");
        let old_kiana_home = take_env("KIANA_HOME");

        let tasks_root =
            std::env::temp_dir().join(format!("kiana-bridge-loop-smoke-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tasks_root).unwrap();
        std::env::set_var("KIANA_TASKS_ROOT", &tasks_root);
        std::env::set_var("KIANA_HOME", tasks_root.join("kiana-home"));
        std::env::set_var("KIANA_AGENT_ID", "agent-1");
        std::env::remove_var("KIANA_PERMISSION_MODE");
        kiana_types::write_project_trust(
            std::env::current_dir().unwrap(),
            kiana_types::ProjectTrust::Trusted,
        )
        .unwrap();

        let (base_url, server) = start_bridge_loop_mock_model_server().await;
        let options = HashMap::from([
            ("execute".to_string(), Value::Bool(true)),
            ("no_session_persistence".to_string(), Value::Bool(true)),
            ("api_key".to_string(), Value::String("test-key".to_string())),
            ("base_url".to_string(), Value::String(base_url)),
            ("model".to_string(), Value::String("mock-model".to_string())),
        ]);

        let (input_writer, input_reader) = tokio::io::duplex(8192);
        let input_writer = StdArc::new(AsyncMutex::new(input_writer));
        let mut writer = BridgeLoopPermissionWriter::new(input_writer.clone());
        {
            let mut input = input_writer.lock().await;
            input
                .write_all(
                    br#"{"type":"control_request","request_id":"set-mode-1","request":{"subtype":"set_permission_mode","mode":"ask"}}
{"type":"user","message":{"role":"user","content":"create a task"},"parent_tool_use_id":null}
"#,
                )
                .await
                .unwrap();
            input.flush().await.unwrap();
        }

        print_bridge_stream_json_loop_with_io(
            String::new(),
            options,
            None,
            tokio::io::BufReader::new(input_reader),
            &mut writer,
        )
        .await
        .unwrap();

        let lines = writer.lines();
        let permission_request = lines
            .iter()
            .find(|event| {
                event.get("type").and_then(Value::as_str) == Some("control_request")
                    && event
                        .get("request")
                        .and_then(|request| request.get("subtype"))
                        .and_then(Value::as_str)
                        == Some("can_use_tool")
            })
            .expect("permission request not written");
        let request_id = permission_request["request_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(permission_request["request"]["tool_name"], "TaskCreate");
        assert_eq!(permission_request["request"]["agent_id"], "agent-1");
        assert_eq!(permission_request["request"]["blocked_path"], Value::Null);
        assert_eq!(
            permission_request["request"]["decision_reason"]["reason"],
            "Tool TaskCreate requires permission in ask mode."
        );

        let permission_cancel = lines
            .iter()
            .find(|event| {
                event.get("type").and_then(Value::as_str) == Some("control_cancel_request")
                    && event.get("request_id").and_then(Value::as_str) == Some(request_id.as_str())
            })
            .expect("permission cancel not written");
        assert_eq!(permission_cancel["request_id"], request_id);

        let result_event = lines
            .iter()
            .find(|event| event.get("type").and_then(Value::as_str) == Some("result"))
            .expect("result event not written");
        assert_eq!(result_event["subtype"], "success");
        assert_eq!(result_event["result"], "task created");

        let assistant_event = lines
            .iter()
            .find(|event| event.get("type").and_then(Value::as_str) == Some("assistant"))
            .expect("assistant event not written");
        assert_eq!(assistant_event["message"]["role"], "assistant");
        assert_eq!(assistant_event["message"]["content"][0]["type"], "text");
        assert_eq!(
            assistant_event["message"]["content"][0]["text"],
            "task created"
        );
        assert_eq!(assistant_event["parent_tool_use_id"], Value::Null);
        assert!(assistant_event["session_id"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()));

        server.abort();
        let _ = std::fs::remove_dir_all(&tasks_root);
        restore_env("KIANA_TASKS_ROOT", old_tasks_root);
        restore_env("KIANA_AGENT_ID", old_agent_id);
        restore_env("KIANA_PERMISSION_MODE", old_permission_mode);
        restore_env("KIANA_HOME", old_kiana_home);
    }

    #[test]
    fn direct_connect_open_parses_cc_url_and_headless_args() {
        let target =
            parse_direct_connect_url("cc://127.0.0.1:7777/work?token=abc&scheme=https").unwrap();
        assert_eq!(target.server_url, "https://127.0.0.1:7777/work");
        assert_eq!(target.auth_token.as_deref(), Some("abc"));
        assert_eq!(target.transport, DirectConnectTransport::Http);
        assert_eq!(target.unix_socket, None);

        let unix_target =
            parse_direct_connect_url("cc+unix:///tmp/kiana.sock?token=unix-secret").unwrap();
        assert_eq!(unix_target.server_url, "unix:/tmp/kiana.sock");
        assert_eq!(unix_target.auth_token.as_deref(), Some("unix-secret"));
        assert_eq!(unix_target.transport, DirectConnectTransport::Unix);
        assert_eq!(
            unix_target.unix_socket.as_deref(),
            Some(Path::new("/tmp/kiana.sock"))
        );

        let args = parse_direct_connect_open_args(&[
            "open".to_string(),
            "cc://127.0.0.1:7777?authToken=secret".to_string(),
            "-p".to_string(),
            "hello".to_string(),
            "remote".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
            "--dangerously-skip-permissions".to_string(),
        ])
        .unwrap();
        assert_eq!(args.target.server_url, "http://127.0.0.1:7777");
        assert_eq!(args.target.auth_token.as_deref(), Some("secret"));
        assert_eq!(args.prompt, "hello remote");
        assert_eq!(args.output_format, PrintOutputFormat::Json);
        assert!(args.dangerously_skip_permissions);
    }

    #[test]
    fn direct_connect_server_parses_reference_defaults_and_unix_boundary() {
        let args = parse_direct_connect_server_args(&[
            "server".to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
            "--port=4567".to_string(),
            "--auth-token".to_string(),
            "secret".to_string(),
            "--workspace".to_string(),
            "/tmp/direct-work".to_string(),
            "--idle-timeout".to_string(),
            "1234".to_string(),
            "--max-sessions=2".to_string(),
        ])
        .unwrap();
        assert_eq!(args.host, "127.0.0.1");
        assert_eq!(args.port, 4567);
        assert_eq!(args.auth_token.as_deref(), Some("secret"));
        assert_eq!(
            args.workspace.as_deref(),
            Some(Path::new("/tmp/direct-work"))
        );
        assert_eq!(args.idle_timeout_ms, 1234);
        assert_eq!(args.max_sessions, 2);

        let defaults = parse_direct_connect_server_args(&["server".to_string()]).unwrap();
        assert_eq!(defaults.host, "0.0.0.0");
        assert_eq!(defaults.port, 0);
        assert_eq!(defaults.idle_timeout_ms, 600_000);
        assert_eq!(defaults.max_sessions, 32);

        let unix_args = parse_direct_connect_server_args(&[
            "server".to_string(),
            "--unix".to_string(),
            "/tmp/kiana.sock".to_string(),
        ])
        .unwrap();
        assert_eq!(
            unix_args.unix_socket.as_deref(),
            Some(Path::new("/tmp/kiana.sock"))
        );
    }

    #[cfg(not(unix))]
    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_open_reports_unix_socket_platform_boundary() {
        let open_args = parse_direct_connect_open_args(&[
            "open".to_string(),
            "cc+unix:///tmp/kiana.sock?token=abc".to_string(),
            "-p".to_string(),
            "hello".to_string(),
        ])
        .unwrap();
        let mut output = Vec::new();
        let error = direct_connect_open_with_writer(&open_args, &mut output)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("require Unix socket support"));
        assert!(output.is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_server_rejects_missing_bearer_on_sessions() {
        let workspace =
            std::env::temp_dir().join(format!("kiana-direct-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_args = DirectConnectServerArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("secret".to_string()),
            unix_socket: None,
            workspace: Some(workspace.clone()),
            idle_timeout_ms: 1000,
            max_sessions: 32,
        };
        let state = direct_connect_server_state(
            server_args,
            addr,
            Some("secret".to_string()),
            HashMap::new(),
        )
        .unwrap();
        let app = direct_connect_server_router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let response = reqwest::Client::new()
            .post(format!("http://{addr}/sessions"))
            .json(&serde_json::json!({
                "cwd": workspace.display().to_string()
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);

        server.abort();
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_server_rejects_session_cwd_outside_workspace() {
        let root = std::env::temp_dir().join(format!(
            "kiana-direct-cwd-boundary-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let outside = root.join("outside");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = direct_connect_server_state(
            DirectConnectServerArgs {
                host: "127.0.0.1".to_string(),
                port: 0,
                auth_token: Some("secret".to_string()),
                unix_socket: None,
                workspace: Some(workspace.clone()),
                idle_timeout_ms: 1000,
                max_sessions: 32,
            },
            addr,
            Some("secret".to_string()),
            HashMap::new(),
        )
        .unwrap();
        let app = direct_connect_server_router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        for cwd in [outside.display().to_string(), "../outside".to_string()] {
            let response = reqwest::Client::new()
                .post(format!("http://{addr}/sessions"))
                .bearer_auth("secret")
                .json(&serde_json::json!({ "cwd": cwd }))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
            let value: Value = response.json().await.unwrap();
            assert!(value["error"]
                .as_str()
                .unwrap_or_default()
                .contains("outside workspace"));
        }

        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn direct_connect_trust_status_uses_external_store_and_ignores_legacy_file() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["HOME", "USERPROFILE", "KIANA_HOME"]);
        let root = std::env::temp_dir().join(format!(
            "kiana-direct-connect-trust-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("project");
        let kiana_home = root.join("kiana-home");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".kiana")).unwrap();
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::fs::write(
            project.join(".kiana").join("trust.json"),
            r#"{"trusted":true}"#,
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &kiana_home);
        let project = std::fs::canonicalize(project).unwrap();
        let server_args = DirectConnectServerArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("secret".to_string()),
            unix_socket: None,
            workspace: Some(project.clone()),
            idle_timeout_ms: 1_000,
            max_sessions: 32,
        };
        let state = direct_connect_server_state(
            server_args,
            SocketAddr::from(([127, 0, 0, 1], 0)),
            Some("secret".to_string()),
            HashMap::new(),
        )
        .unwrap();

        let unknown = direct_connect_app_trust_status_payload(&state);
        assert_eq!(unknown["project_trust"], "unknown");
        assert_eq!(unknown["project_trusted"], false);
        assert_eq!(unknown["allows_project_resources"], false);
        assert_eq!(unknown["source"], "default");
        assert_eq!(
            unknown["project_id"],
            kiana_types::project_trust_id(&project)
        );
        assert_eq!(unknown["project_root"], project.display().to_string());
        assert_eq!(unknown["file"]["status"], "missing");
        assert_eq!(unknown["file"]["exists"], false);
        assert_eq!(unknown["file"]["error"], Value::Null);
        assert_eq!(unknown["legacy_project_file"]["exists"], true);
        assert_eq!(unknown["legacy_project_file"]["ignored"], true);

        kiana_types::write_project_trust(&project, kiana_types::ProjectTrust::Trusted).unwrap();
        let trusted = direct_connect_app_trust_status_payload(&state);
        assert_eq!(trusted["project_trust"], "trusted");
        assert_eq!(trusted["project_trusted"], true);
        assert_eq!(trusted["allows_project_resources"], true);
        assert_eq!(trusted["source"], "user_store");
        assert_eq!(trusted["file"]["status"], "found");
        assert_eq!(trusted["file"]["exists"], true);
        assert_eq!(
            trusted["file"]["path"],
            kiana_types::project_trust_file_path(&project)
                .unwrap()
                .display()
                .to_string()
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_app_contract_exposes_product_shell_endpoints() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "ANTHROPIC_API_KEY",
            "KIANA_CONFIG_FILE",
            "KIANA_HOME",
            "KIANA_OAUTH_TOKENS_FILE",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_BASE_URL",
            "OPENAI_BASE_URL",
            "KIANA_OPENAI_MODEL",
            "OPENAI_MODEL",
            "KIANA_OLLAMA_BASE_URL",
            "OLLAMA_BASE_URL",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
            "KIANA_PLUGINS_DIR",
            "KIANA_SDK_SESSIONS_DIR",
            "DIST_DIR",
            "KIANA_LOCAL_RC_EVIDENCE_OUT",
            "KIANA_PRODUCT_ACCEPTANCE_FILE",
            "KIANA_PRODUCT_ACCEPTANCE_OUT",
            "KIANA_ENTITLEMENT_PROOF_FILE",
            "KIANA_ENTITLEMENT_PROOF_OUT",
            "KIANA_RELEASE_OPS_FILE",
            "KIANA_RELEASE_OPS_OUT",
            "KIANA_PLATFORM_SECURITY_PROOF_FILE",
            "KIANA_PLATFORM_SECURITY_PROOF_OUT",
            "KIANA_PLATFORM_SECURITY_PROOF_DIR",
            "KIANA_SOURCE_CONTROL_PROOF_FILE",
            "KIANA_SOURCE_CONTROL_PROOF_OUT",
            "KIANA_TASKS_ROOT",
            "KIANA_RELEASE_SIGNATURE_PROOF_FILE",
            "KIANA_RELEASE_SIGNATURE_PROOF_OUT",
            "KIANA_ENTERPRISE_OFFLINE_MANIFEST_FILE",
            "KIANA_ENTERPRISE_OFFLINE_MANIFEST_OUT",
            "KIANA_COMMERCIAL_PROOF_MANIFEST_FILE",
            "KIANA_COMMERCIAL_PROOF_MANIFEST_OUT",
            "KIANA_PROVIDER_LIVE_CATALOG_FILE",
            "KIANA_PROVIDER_LIVE_CATALOG_OUT",
            "KIANA_PROVIDER_LIVE_SMOKE_FILE",
            "KIANA_PROVIDER_LIVE_SMOKE_OUT",
            "KIANA_REMOTE_SMOKE_PROOF_FILE",
            "KIANA_REMOTE_SMOKE_PROOF_OUT",
            "KIANA_DISTRIBUTION_REVIEW_DIST_DIR",
            "KIANA_PERMISSIONS_FILE",
            "KIANA_MANAGED_PERMISSIONS_FILE",
            "KIANA_MANAGED_POLICY_FILE",
            "KIANA_PERMISSION_PROFILE",
            "KIANA_PERMISSION_MODE",
            "KIANA_ALLOWED_TOOLS",
            "KIANA_DISALLOWED_TOOLS",
            "KIANA_ASK_TOOLS",
        ]);
        let workspace =
            std::env::temp_dir().join(format!("kiana-direct-app-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let plugins_dir = workspace.join(".plugins");
        let sessions_dir = workspace.join(".sdk-sessions");
        let plugin_root = plugins_dir.join("app-tools");
        let plugin_manifest_dir = plugin_root.join(".codex-plugin");
        std::fs::create_dir_all(&plugin_manifest_dir).unwrap();
        std::fs::write(
            plugin_manifest_dir.join("plugin.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": "app-tools",
                "version": "1.0.0",
                "description": "App server plugin"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("commands")).unwrap();
        std::fs::write(
            plugin_root.join("commands").join("audit.md"),
            "Audit command",
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("agents")).unwrap();
        std::fs::write(
            plugin_root.join("agents").join("reviewer.md"),
            "Review agent",
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("skills").join("triage")).unwrap();
        std::fs::write(
            plugin_root.join("skills").join("triage").join("SKILL.md"),
            "# Triage",
        )
        .unwrap();
        std::fs::create_dir_all(plugin_root.join("hooks")).unwrap();
        std::fs::write(plugin_root.join("hooks").join("hooks.json"), "[]").unwrap();
        std::fs::create_dir_all(plugin_root.join("output-styles")).unwrap();
        std::fs::write(
            plugin_root.join("output-styles").join("brief.md"),
            "Brief output",
        )
        .unwrap();
        std::fs::write(plugin_root.join(".lsp.json"), "{}").unwrap();
        std::fs::write(
            plugin_root.join("app.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "review-workbench",
                "title": "Review Workbench",
                "description": "Review app surface",
                "entry": "apps/review/index.html",
                "routes": [
                    {
                        "path": "/review",
                        "title": "Review"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(plugin_root.join(".mcp.json"), "{}").unwrap();
        std::fs::create_dir_all(workspace.join("src")).unwrap();
        std::fs::write(
            workspace.join("src").join("lib.rs"),
            "pub fn checkout() {}\n// checkout checkout\n",
        )
        .unwrap();
        std::fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"kiana-direct-app-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(workspace.join("README.md"), "checkout guide\n").unwrap();
        let disabled_plugin_root = plugins_dir.join("disabled-tools");
        let disabled_manifest_dir = disabled_plugin_root.join(".codex-plugin");
        std::fs::create_dir_all(&disabled_manifest_dir).unwrap();
        std::fs::write(
            disabled_manifest_dir.join("plugin.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": "disabled-tools",
                "version": "1.0.0",
                "description": "Disabled app server plugin"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(disabled_plugin_root.join("commands")).unwrap();
        std::fs::write(
            disabled_plugin_root.join("commands").join("disabled.md"),
            "Disabled command",
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "disabled-tools", false).unwrap();
        let project_plugin_root = workspace
            .join(".kiana")
            .join("plugins")
            .join("project-tools");
        std::fs::create_dir_all(project_plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            project_plugin_root
                .join(".codex-plugin")
                .join("plugin.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": "project-tools",
                "version": "1.0.0",
                "description": "Project app server plugin"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(project_plugin_root.join("commands")).unwrap();
        std::fs::write(
            project_plugin_root
                .join("commands")
                .join("project-audit.md"),
            "Project audit command",
        )
        .unwrap();
        let local_plugin_root = workspace
            .join(".kiana")
            .join("plugins.local")
            .join("local-tools");
        std::fs::create_dir_all(local_plugin_root.join(".codex-plugin")).unwrap();
        std::fs::write(
            local_plugin_root.join(".codex-plugin").join("plugin.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "name": "local-tools",
                "version": "1.0.0",
                "description": "Local app server plugin"
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(local_plugin_root.join("skills").join("local-triage")).unwrap();
        std::fs::write(
            local_plugin_root
                .join("skills")
                .join("local-triage")
                .join("SKILL.md"),
            "# Local triage",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        std::env::set_var("KIANA_SDK_SESSIONS_DIR", &sessions_dir);
        let kiana_home = workspace.with_extension("kiana-home");
        std::env::set_var("KIANA_HOME", &kiana_home);
        std::fs::create_dir_all(&kiana_home).unwrap();
        std::env::set_var("KIANA_CONFIG_FILE", workspace.join("config.toml"));
        std::env::set_var(
            "KIANA_OAUTH_TOKENS_FILE",
            workspace.join("oauth-tokens.json"),
        );
        let permissions_file = kiana_home.join("permissions.json");
        std::fs::write(
            &permissions_file,
            serde_json::to_string_pretty(&serde_json::json!({
                "profile": "workspace",
                "allowedTools": ["Read"],
                "disallowedTools": ["Bash"],
                "askTools": ["Edit"]
            }))
            .unwrap(),
        )
        .unwrap();
        let managed_permissions_file = kiana_home.join("managed-permissions.json");
        std::fs::write(
            &managed_permissions_file,
            serde_json::to_string_pretty(&serde_json::json!({
                "permissions": {
                    "profile": "commercial",
                    "allowedTools": ["Glob"],
                    "disallowedTools": ["Write"],
                    "askTools": ["Bash"]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_PERMISSIONS_FILE", &permissions_file);
        std::env::set_var("KIANA_MANAGED_PERMISSIONS_FILE", &managed_permissions_file);
        let prompt_history_path = kiana_home.join("tui-history.jsonl");
        std::fs::create_dir_all(prompt_history_path.parent().unwrap()).unwrap();
        let prompt_history_lines = [
            "not json".to_string(),
            serde_json::to_string(&HistoryEntry::new(
                "newer prompt".to_string(),
                "2".to_string(),
            ))
            .unwrap(),
            serde_json::to_string(&HistoryEntry::new(
                "older prompt".to_string(),
                "1".to_string(),
            ))
            .unwrap(),
            serde_json::to_string(&HistoryEntry::new(
                "newer prompt".to_string(),
                "0".to_string(),
            ))
            .unwrap(),
            serde_json::to_string(&HistoryEntry::new("   ".to_string(), "0".to_string())).unwrap(),
        ]
        .join("\n");
        std::fs::write(&prompt_history_path, format!("{prompt_history_lines}\n")).unwrap();
        let tasks_dir = workspace.join(".kiana").join("tasks").join("default");
        std::fs::create_dir_all(&tasks_dir).unwrap();
        std::fs::write(
            tasks_dir.join("1.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "1",
                "title": "Review task status",
                "subject": "Review task status",
                "status": "pending",
                "owner": "planner",
                "blockedBy": []
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            tasks_dir.join("2.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "id": "2",
                "title": "Ship app status",
                "subject": "Ship app status",
                "status": "completed",
                "owner": "builder",
                "blockedBy": []
            }))
            .unwrap(),
        )
        .unwrap();
        for key in [
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_BASE_URL",
            "OPENAI_BASE_URL",
            "KIANA_OPENAI_MODEL",
            "OPENAI_MODEL",
            "KIANA_OLLAMA_BASE_URL",
            "OLLAMA_BASE_URL",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
        ] {
            std::env::remove_var(key);
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_args = DirectConnectServerArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("secret".to_string()),
            unix_socket: None,
            workspace: Some(workspace.clone()),
            idle_timeout_ms: 1000,
            max_sessions: 32,
        };
        let state = direct_connect_server_state(
            server_args,
            addr,
            Some("secret".to_string()),
            HashMap::from([
                (
                    "api_key".to_string(),
                    Value::String("must-not-leak".to_string()),
                ),
                (
                    "model".to_string(),
                    Value::String("opus-4.8-1m".to_string()),
                ),
                (
                    "permission_mode".to_string(),
                    Value::String("ask".to_string()),
                ),
            ]),
        )
        .unwrap();
        let app = direct_connect_server_router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = reqwest::Client::new();

        let unauthorized = client
            .get(format!("http://{addr}/app"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let session: Value = client
            .post(format!("http://{addr}/sessions"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "cwd": workspace.display().to_string()
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let session_id = session["session_id"].as_str().unwrap();
        let session_event_dir = sessions_dir.join(session_id);
        std::fs::create_dir_all(&session_event_dir).unwrap();
        let events = vec![
            kiana_types::RuntimeEvent::new(
                "event-0",
                session_id,
                "turn-0",
                None,
                0,
                "2026-07-02T00:00:00Z",
                kiana_types::RuntimeEventPayload::UserMessage(kiana_types::MessageRuntimeEvent {
                    message: serde_json::json!({
                        "role": "user",
                        "content": "hello from app"
                    }),
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "event-1",
                session_id,
                "turn-0",
                None,
                1,
                "2026-07-02T00:00:01Z",
                kiana_types::RuntimeEventPayload::AssistantMessage(
                    kiana_types::MessageRuntimeEvent {
                        message: serde_json::json!({
                            "role": "assistant",
                            "content": [{
                                "type": "text",
                                "text": "hello from kiana"
                            }]
                        }),
                    },
                ),
            ),
            kiana_types::RuntimeEvent::new(
                "event-2",
                session_id,
                "turn-0",
                None,
                2,
                "2026-07-02T00:00:02Z",
                kiana_types::RuntimeEventPayload::ToolResult(kiana_types::RuntimeToolResultEvent {
                    tool_call_id: "toolu_write".to_string(),
                    name: Some("Write".to_string()),
                    workbench: Some("local-files".to_string()),
                    is_error: false,
                    content: serde_json::json!("updated src/lib.rs"),
                    changed_files: Some(serde_json::json!([
                        {
                            "path": "src/lib.rs",
                            "operation": "update",
                            "source": "Write"
                        }
                    ])),
                    error: None,
                }),
            ),
            kiana_types::RuntimeEvent::new(
                "event-3",
                session_id,
                "turn-0",
                None,
                3,
                "2026-07-02T00:00:03Z",
                kiana_types::RuntimeEventPayload::Result(kiana_types::RuntimeResultEvent {
                    status: "completed".to_string(),
                    stop_reason: "end_turn".to_string(),
                    assistant_text: Some("hello from kiana".to_string()),
                    metadata: serde_json::json!({
                        "turns": 1
                    }),
                }),
            ),
        ];
        let event_contents = events
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
            .join("\n");
        std::fs::write(
            session_event_dir.join("events.jsonl"),
            format!("{event_contents}\n"),
        )
        .unwrap();
        let archived_session_id = "archived-app-session";
        std::fs::write(
            sessions_dir.join(format!("{archived_session_id}.json")),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": archived_session_id,
                "title": "Archived app session",
                "tag": "handoff",
                "parent_session_id": null,
                "cwd": workspace.display().to_string(),
                "created_at": 7,
                "updated_at": 9,
                "messages": [
                    {
                        "role": "user",
                        "content": "resume this app session"
                    },
                    {
                        "role": "assistant",
                        "content": "ready"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        let contract: Value = client
            .get(format!("http://{addr}/app"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(contract["schema"], "kiana.app-server.contract.v1");
        assert_eq!(contract["transport"], "http");
        assert_eq!(contract["active_sessions"], 1);
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("conversations.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("events.snapshot.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("conversation.files.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("conversation.files.write".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("prompt.history.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("team.status.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("tasks.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("commands.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("commands.run".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("config.resolved.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("permissions.status.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("trust.status.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("plugins.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("auth.status.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("license.status.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("model.catalog.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("model.smoke.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("model.list.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("model.current.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("model.current.write".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.index.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.index.cache.write".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.artifacts.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.artifacts.cache.write".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.artifact_ingest.write".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.artifact_graph.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.artifact_store.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String(
                "context.artifact_readiness.read".to_string()
            )));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.search.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.vector_search.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.pack.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("context.repo_map.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("checks.dry_run.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("checks.run.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("diff.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("checkpoint.create".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("review.dry_run.read".to_string())));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .contains(&Value::String("review.run.read".to_string())));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/conversations"
                    && endpoint["schema"] == "kiana.app-server.conversations.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/conversations/{session_id}/events"
                    && endpoint["schema"] == "kiana.app-server.events.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/conversations/{session_id}/files"
                    && endpoint["schema"] == "kiana.app-server.conversation-files.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "POST"
                    && endpoint["path"] == "/app/conversations/{session_id}/files"
                    && endpoint["schema"] == "kiana.app-server.conversation-files.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/prompt-history"
                    && endpoint["schema"] == "kiana.app-server.prompt-history.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/team/status"
                    && endpoint["schema"] == "kiana.app-server.team-status.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/team/plan"
                    && endpoint["schema"] == "kiana.team-plan.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/commands"
                    && endpoint["schema"] == "kiana.app-server.commands.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "POST"
                    && endpoint["path"] == "/app/commands/run"
                    && endpoint["schema"] == "kiana.app-server.command-run.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/config/resolved"
                    && endpoint["schema"] == "kiana.app-server.config-resolved.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/permissions/status"
                    && endpoint["schema"] == "kiana.app-server.permissions-status.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/trust/status"
                    && endpoint["schema"] == "kiana.app-server.trust-status.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/plugins"
                    && endpoint["schema"] == "kiana.app-server.plugins.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/auth/status"
                    && endpoint["schema"] == "kiana.auth-status.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/license/status"
                    && endpoint["schema"] == "kiana.license-status.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/models/catalog"
                    && endpoint["schema"] == "kiana.model-catalog.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/models/smoke"
                    && endpoint["schema"] == "kiana.model-smoke.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/models/list"
                    && endpoint["schema"] == "kiana.model-list.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/models/current"
                    && endpoint["schema"] == "kiana.app-server.model-current.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "POST"
                    && endpoint["path"] == "/app/models/current"
                    && endpoint["schema"] == "kiana.app-server.model-current.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/doctor"
                    && endpoint["schema"] == "kiana.app-server.doctor.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/blockers"
                    && endpoint["schema"] == "kiana.commercial-release-blockers.v1"
            }));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.local_rc_evidence.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.product_acceptance.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.entitlement.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.ops.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.platform_security.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.source_control.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.signature.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.enterprise_offline_manifest.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.proof_manifest.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.live_provider_smoke.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.remote_code_session_smoke.read"));
        assert!(contract["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|capability| capability == "release.distribution_review.read"));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/local-rc-evidence"
                    && endpoint["schema"] == "kiana.local-rc-evidence.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/product-acceptance"
                    && endpoint["schema"] == "kiana.product-acceptance.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/entitlement"
                    && endpoint["schema"] == "kiana.entitlement-proof.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/ops"
                    && endpoint["schema"] == "kiana.release-ops.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/platform-security"
                    && endpoint["schema"] == "kiana.platform-security-proof.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/source-control"
                    && endpoint["schema"] == "kiana.source-control-proof.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/signature"
                    && endpoint["schema"] == "kiana.release-signature.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/enterprise-offline-manifest"
                    && endpoint["schema"] == "kiana.enterprise.offline-manifest.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/proof-manifest"
                    && endpoint["schema"] == "kiana.commercial-proof-manifest.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/live-provider-smoke"
                    && endpoint["schema"] == "kiana.app-server.live-provider-smoke.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/remote-code-session-smoke"
                    && endpoint["schema"] == "kiana.remote-code-session-smoke.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/release/distribution"
                    && endpoint["schema"] == "kiana.app-server.distribution-review.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/index"
                    && endpoint["schema"] == "kiana.context-index.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/artifacts"
                    && endpoint["schema"] == "kiana.context-artifacts.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "POST"
                    && endpoint["path"] == "/app/context/ingest"
                    && endpoint["schema"] == "kiana.context-artifact-ingest.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/artifact-graph"
                    && endpoint["schema"] == "kiana.context-artifact-dependency-graph.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/artifact-store"
                    && endpoint["schema"] == "kiana.context-artifact-store.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/artifact-readiness"
                    && endpoint["schema"] == "kiana.context-artifact-readiness.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/search"
                    && endpoint["schema"] == "kiana.context-search.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/vector-search"
                    && endpoint["schema"] == "kiana.context-vector-search.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/pack"
                    && endpoint["schema"] == "kiana.context-pack.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/context/repo-map"
                    && endpoint["schema"] == "kiana.repo-map.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/checks/dry-run"
                    && endpoint["schema"] == "kiana.checks.dry_run.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/checks"
                    && endpoint["schema"] == "kiana.checks.run.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/diff"
                    && endpoint["schema"] == "kiana.diff.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "POST"
                    && endpoint["path"] == "/app/checkpoints"
                    && endpoint["schema"] == "kiana.checkpoint.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/review/dry-run"
                    && endpoint["schema"] == "kiana.review.dry_run.v1"
            }));
        assert!(contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|endpoint| {
                endpoint["method"] == "GET"
                    && endpoint["path"] == "/app/review"
                    && endpoint["schema"] == "kiana.review.run.v1"
            }));

        let conversations: Value = client
            .get(format!("http://{addr}/app/conversations"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(conversations["schema"], "kiana.app-server.conversations.v1");
        assert_eq!(conversations["count"], 2);
        let conversation_items = conversations["conversations"].as_array().unwrap();
        let live_conversation = conversation_items
            .iter()
            .find(|conversation| conversation["id"] == session_id)
            .expect("live direct-connect conversation");
        assert_eq!(live_conversation["active"], true);
        assert_eq!(live_conversation["source"], "direct-connect");
        assert_eq!(
            live_conversation["events_url"],
            format!("/sessions/{session_id}/ws")
        );
        assert_eq!(
            live_conversation["events_snapshot_url"],
            format!("/app/conversations/{session_id}/events")
        );
        let archived_conversation = conversation_items
            .iter()
            .find(|conversation| conversation["id"] == archived_session_id)
            .expect("archived SDK conversation");
        assert_eq!(archived_conversation["active"], false);
        assert_eq!(archived_conversation["source"], "sdk-session-store");
        assert_eq!(archived_conversation["title"], "Archived app session");
        assert_eq!(archived_conversation["tag"], "handoff");
        assert_eq!(archived_conversation["message_count"], 2);
        assert_eq!(archived_conversation["assistant_message_count"], 1);
        assert_eq!(archived_conversation["last_role"], "assistant");
        assert_eq!(archived_conversation["updated_at"], 9);
        assert_eq!(archived_conversation["events_url"], Value::Null);
        assert_eq!(
            archived_conversation["events_snapshot_url"],
            format!("/app/conversations/{archived_session_id}/events")
        );

        let event_snapshot: Value = client
            .get(format!(
                "http://{addr}/app/conversations/{session_id}/events"
            ))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(event_snapshot["schema"], "kiana.app-server.events.v1");
        assert_eq!(event_snapshot["session_id"], session_id);
        assert_eq!(event_snapshot["active"], true);
        assert_eq!(event_snapshot["event_source"]["available"], true);
        assert_eq!(event_snapshot["count"], 4);
        assert_eq!(event_snapshot["total_events"], 4);
        assert_eq!(event_snapshot["summary"]["turns"], 1);
        assert_eq!(event_snapshot["summary"]["event_types"]["user_message"], 1);
        assert_eq!(
            event_snapshot["summary"]["event_types"]["assistant_message"],
            1
        );
        assert_eq!(event_snapshot["summary"]["event_types"]["tool_result"], 1);
        assert_eq!(event_snapshot["summary"]["event_types"]["result"], 1);
        assert_eq!(event_snapshot["summary"]["tool_results"]["total"], 1);
        assert_eq!(event_snapshot["summary"]["tool_results"]["errors"], 0);
        assert_eq!(event_snapshot["summary"]["file_changes"]["count"], 1);
        assert_eq!(
            event_snapshot["summary"]["file_changes"]["paths"][0],
            "src/lib.rs"
        );
        assert_eq!(event_snapshot["summary"]["terminal"]["present"], true);
        assert_eq!(event_snapshot["summary"]["terminal"]["status"], "completed");
        assert_eq!(
            event_snapshot["summary"]["terminal"]["stop_reason"],
            "end_turn"
        );
        assert_eq!(
            event_snapshot["view"]["schema"],
            "kiana.app-server.events-view.v1"
        );
        assert_eq!(event_snapshot["view"]["message_count"], 3);
        assert_eq!(event_snapshot["view"]["messages"][0]["role"], "user");
        assert_eq!(
            event_snapshot["view"]["messages"][0]["content"],
            "hello from app"
        );
        assert_eq!(event_snapshot["view"]["messages"][1]["role"], "assistant");
        assert_eq!(
            event_snapshot["view"]["messages"][1]["content"],
            "hello from kiana"
        );
        assert_eq!(event_snapshot["view"]["messages"][2]["role"], "tool");
        assert_eq!(
            event_snapshot["view"]["messages"][2]["changed_files"][0]["path"],
            "src/lib.rs"
        );
        assert_eq!(event_snapshot["events"][0]["type"], "user_message");
        assert_eq!(
            event_snapshot["events"][1]["message"]["content"][0]["text"],
            "hello from kiana"
        );

        let initial_files: Value = client
            .get(format!(
                "http://{addr}/app/conversations/{archived_session_id}/files"
            ))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            initial_files["schema"],
            "kiana.app-server.conversation-files.v1"
        );
        assert_eq!(initial_files["session_id"], archived_session_id);
        assert_eq!(initial_files["editable_files"], serde_json::json!([]));
        assert_eq!(initial_files["read_only_files"], serde_json::json!([]));
        assert_eq!(initial_files["changed"], false);

        let updated_files: Value = client
            .post(format!(
                "http://{addr}/app/conversations/{archived_session_id}/files"
            ))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "editable_files": ["src/lib.rs", "tests/app.rs"],
                "read_only_files": ["README.md"],
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            updated_files["schema"],
            "kiana.app-server.conversation-files.v1"
        );
        assert_eq!(updated_files["session_id"], archived_session_id);
        assert_eq!(
            updated_files["editable_files"],
            serde_json::json!(["src/lib.rs", "tests/app.rs"])
        );
        assert_eq!(
            updated_files["read_only_files"],
            serde_json::json!(["README.md"])
        );
        assert_eq!(updated_files["changed"], true);

        let persisted_files: Value = serde_json::from_str(
            &std::fs::read_to_string(sessions_dir.join(format!("{archived_session_id}.json")))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            persisted_files["editable_files"],
            serde_json::json!(["src/lib.rs", "tests/app.rs"])
        );
        assert_eq!(
            persisted_files["read_only_files"],
            serde_json::json!(["README.md"])
        );

        let cleared_files: Value = client
            .post(format!(
                "http://{addr}/app/conversations/{archived_session_id}/files"
            ))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "clear_editable": true,
                "clear_read_only": true,
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(cleared_files["editable_files"], serde_json::json!([]));
        assert_eq!(cleared_files["read_only_files"], serde_json::json!([]));
        assert_eq!(cleared_files["changed"], true);

        let settings: Value = client
            .get(format!("http://{addr}/app/settings"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(settings["schema"], "kiana.app-server.settings.v1");
        assert_eq!(settings["auth"]["type"], "bearer");
        assert_eq!(
            settings["base_option_keys"],
            serde_json::json!(["api_key", "model", "permission_mode"])
        );
        let setting_sections = settings["sections"].as_array().unwrap();
        let setting_titles = setting_sections
            .iter()
            .map(|section| section["title"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            setting_titles,
            vec![
                "Account/Auth",
                "Provider/Model",
                "Permissions",
                "MCP",
                "Remote/Diagnostics"
            ]
        );
        assert!(setting_sections.iter().all(|section| section["rows"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty())));
        assert!(setting_sections
            .iter()
            .any(|section| section["title"] == "Remote/Diagnostics"
                && section["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["label"] == "commercial_security")));
        assert!(!settings.to_string().contains("must-not-leak"));

        let prompt_history: Value = client
            .get(format!("http://{addr}/app/prompt-history"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            prompt_history["schema"],
            "kiana.app-server.prompt-history.v1"
        );
        assert_eq!(prompt_history["workspace"], workspace.display().to_string());
        assert_eq!(prompt_history["storage"]["kind"], "kiana_home");
        assert_eq!(prompt_history["storage"]["file"], "tui-history.jsonl");
        assert_eq!(
            prompt_history["limit"],
            crate::tui::TUI_PROMPT_HISTORY_LIMIT
        );
        assert_eq!(prompt_history["count"], 2);
        assert_eq!(prompt_history["entries"][0]["prompt"], "newer prompt");
        assert_eq!(prompt_history["entries"][0]["timestamp"], "2");
        assert_eq!(prompt_history["entries"][1]["prompt"], "older prompt");
        assert!(!prompt_history.to_string().contains(".kiana-home"));

        let team_status: Value = client
            .get(format!("http://{addr}/app/team/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(team_status["schema"], "kiana.app-server.team-status.v1");
        assert_eq!(team_status["workspace"], workspace.display().to_string());
        assert_eq!(team_status["team"]["name"], "default");
        assert_eq!(team_status["team"]["source"], "default_task_list");
        assert_eq!(team_status["task_list"]["id"], "default");
        assert_eq!(team_status["task_list"]["count"], 2);
        assert_eq!(team_status["task_list"]["status_counts"]["pending"], 1);
        assert_eq!(team_status["task_list"]["status_counts"]["completed"], 1);
        assert_eq!(team_status["tasks"]["schema"], "kiana.tasks.v1");
        assert_eq!(team_status["tasks"]["count"], 2);
        assert_eq!(team_status["tasks"]["tasks"][0]["id"], "1");
        assert_eq!(
            team_status["tasks"]["tasks"][0]["subject"],
            "Review task status"
        );
        assert!(
            team_status["tasks"]["tasks_dir"]
                .as_str()
                .unwrap()
                .ends_with(".kiana\\tasks\\default")
                || team_status["tasks"]["tasks_dir"]
                    .as_str()
                    .unwrap()
                    .ends_with(".kiana/tasks/default")
        );

        let team_plan: Value = client
            .get(format!("http://{addr}/app/team/plan"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(team_plan["schema"], "kiana.team-plan.v1");
        assert_eq!(team_plan["workspace"], workspace.display().to_string());
        assert_eq!(team_plan["task_list"]["id"], "default");
        assert_eq!(team_plan["role_runtime"]["status"], "incomplete");
        assert!(team_plan["role_runtime"]["missing_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "pm"));
        assert!(team_plan["artifact_readiness"]["missing_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role == "prd"));

        let commands: Value = client
            .get(format!("http://{addr}/app/commands"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(commands["schema"], "kiana.app-server.commands.v1");
        assert_eq!(commands["workspace"], workspace.display().to_string());
        let command_entries = commands["commands"].as_array().unwrap();
        assert_eq!(
            commands["count"].as_u64().unwrap(),
            command_entries.len() as u64
        );
        let model_command = command_entries
            .iter()
            .find(|command| command["name"] == "model")
            .expect("model command missing");
        assert_eq!(model_command["slash"], "/model");
        assert_eq!(model_command["source"]["kind"], "core");
        assert_eq!(model_command["source"]["plugin"], Value::Null);
        assert_eq!(model_command["routes_to"], "local_command");
        let plugin_command = command_entries
            .iter()
            .find(|command| command["name"] == "app-tools:audit")
            .expect("plugin command missing");
        assert_eq!(plugin_command["slash"], "/app-tools:audit");
        assert_eq!(plugin_command["command_type"], "prompt");
        assert_eq!(plugin_command["supports_non_interactive"], true);
        assert_eq!(plugin_command["routes_to"], "assistant_prompt");
        assert_eq!(plugin_command["source"]["kind"], "plugin");
        assert_eq!(plugin_command["source"]["plugin"], "app-tools");
        assert!(!command_entries
            .iter()
            .any(|command| command["name"] == "disabled-tools:disabled"));

        let version_run: Value = client
            .post(format!("http://{addr}/app/commands/run"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "name": "version",
                "args": [],
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(version_run["schema"], "kiana.app-server.command-run.v1");
        assert_eq!(version_run["workspace"], workspace.display().to_string());
        assert_eq!(version_run["command"]["name"], "version");
        assert_eq!(version_run["command"]["routes_to"], "local_command");
        assert_eq!(version_run["executed"], true);
        assert_eq!(version_run["request"]["arg_count"], 0);
        assert_eq!(version_run["output"]["output_type"], "text");
        assert_eq!(version_run["output"]["value"], env!("CARGO_PKG_VERSION"));

        let prompt_run: Value = client
            .post(format!("http://{addr}/app/commands/run"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "name": "app-tools:audit",
                "args": ["src/lib.rs"],
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(prompt_run["schema"], "kiana.app-server.command-run.v1");
        assert_eq!(prompt_run["command"]["command_type"], "prompt");
        assert_eq!(prompt_run["command"]["routes_to"], "assistant_prompt");
        assert_eq!(prompt_run["executed"], false);
        assert_eq!(
            prompt_run["request"]["args"],
            serde_json::json!(["src/lib.rs"])
        );
        assert_eq!(prompt_run["output"]["output_type"], "text");
        assert!(prompt_run["output"]["value"]
            .as_str()
            .unwrap()
            .contains("Audit command"));

        let missing_command = client
            .post(format!("http://{addr}/app/commands/run"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "name": "missing-command",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(missing_command.status(), reqwest::StatusCode::NOT_FOUND);

        let config_resolved: Value = client
            .get(format!("http://{addr}/app/config/resolved"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            config_resolved["schema"],
            "kiana.app-server.config-resolved.v1"
        );
        assert_eq!(
            config_resolved["workspace"],
            workspace.display().to_string()
        );
        assert_eq!(
            config_resolved["config"]["schema"],
            "kiana.config-resolved.v1"
        );
        assert_eq!(
            config_resolved["config"]["config_file"]["path"],
            workspace.join("config.toml").display().to_string()
        );
        assert_eq!(
            config_resolved["config"]["values"]["api_key"]["value"],
            "redacted"
        );
        assert_eq!(
            config_resolved["config"]["values"]["api_key"]["source"],
            "app_state"
        );
        assert_eq!(
            config_resolved["config"]["values"]["model"]["value"],
            "opus-4.8-1m"
        );
        assert!(!config_resolved.to_string().contains("must-not-leak"));

        let secrets: Value = client
            .get(format!("http://{addr}/app/secrets"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(secrets["schema"], "kiana.app-server.secrets.v1");
        assert_eq!(secrets["metadata_supported"], true);
        assert_eq!(secrets["read_supported"], false);
        assert_eq!(secrets["write_supported"], false);
        assert!(secrets["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|secret| secret["id"] == "anthropic_api_key"
                && secret["status"] == "set"
                && secret["source"] == "app_state"
                && secret["value"] == "redacted"
                && secret["readable"] == false));
        assert!(secrets["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|secret| secret["id"] == "anthropic_oauth_token"
                && secret["status"] == "missing"
                && secret["source"] == "oauth_file"));
        assert!(secrets["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|secret| secret["id"] == "openai_compatible_api_key"
                && secret["status"] == "missing"
                && secret["source"] == "none"));
        assert!(secrets["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|secret| secret["id"] == "enterprise_license_key"
                && secret["status"] == "missing"
                && secret["source"] == "none"));
        assert_eq!(secrets["summary"]["set"], 1);
        assert!(secrets["summary"]["missing"].as_u64().unwrap() >= 3);
        assert!(!secrets.to_string().contains("must-not-leak"));

        let sandbox: Value = client
            .get(format!("http://{addr}/app/sandbox"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(sandbox["schema"], "kiana.app-server.sandbox.v1");
        assert_eq!(sandbox["default_permission_mode"], "ask");

        let permissions_status: Value = client
            .get(format!("http://{addr}/app/permissions/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            permissions_status["schema"],
            "kiana.app-server.permissions-status.v1"
        );
        assert_eq!(
            permissions_status["workspace"],
            workspace.display().to_string()
        );
        assert_eq!(permissions_status["profile"], "commercial");
        assert_eq!(permissions_status["mode"], "ask");
        assert_eq!(
            permissions_status["file"]["path"],
            permissions_file.display().to_string()
        );
        assert_eq!(permissions_status["file"]["status"], "loaded");
        assert_eq!(
            permissions_status["managed_policy"]["path"],
            managed_permissions_file.display().to_string()
        );
        assert_eq!(permissions_status["managed_policy"]["status"], "loaded");
        let permission_sources = permissions_status["sources"].as_array().unwrap();
        assert!(permission_sources
            .iter()
            .any(|source| source == "file_profile"));
        assert!(permission_sources
            .iter()
            .any(|source| source == "managed_profile"));
        assert!(permissions_status["rules"]["allowed_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule == "Read"));
        assert!(permissions_status["rules"]["managed_disallowed_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule == "Write"));
        assert_eq!(
            permissions_status["interactive_prompts"]["non_interactive_requires_allow_rule"],
            true
        );

        let trust_status: Value = client
            .get(format!("http://{addr}/app/trust/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(trust_status["schema"], "kiana.app-server.trust-status.v1");
        assert_eq!(trust_status["workspace"], workspace.display().to_string());
        assert_eq!(trust_status["project_trust"], "unknown");
        assert_eq!(trust_status["project_trusted"], false);
        assert_eq!(trust_status["allows_project_resources"], false);
        assert_eq!(trust_status["source"], "default");
        assert_eq!(
            trust_status["project_id"],
            kiana_types::project_trust_id(&workspace)
        );
        assert_eq!(
            trust_status["project_root"],
            workspace.display().to_string()
        );
        assert_eq!(trust_status["file"]["status"], "missing");
        assert_eq!(trust_status["file"]["exists"], false);
        assert_eq!(trust_status["file"]["error"], Value::Null);
        assert_eq!(
            trust_status["file"]["path"],
            kiana_types::project_trust_file_path(&workspace)
                .unwrap()
                .display()
                .to_string()
        );
        assert_eq!(trust_status["legacy_project_file"]["exists"], false);
        assert_eq!(trust_status["legacy_project_file"]["ignored"], true);

        let plugins: Value = client
            .get(format!("http://{addr}/app/plugins"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(plugins["schema"], "kiana.app-server.plugins.v1");
        assert_eq!(plugins["count"], 2);
        let plugin_items = plugins["plugins"].as_array().unwrap();
        let app_tools = plugin_items
            .iter()
            .find(|plugin| plugin["id"] == "app-tools")
            .expect("app-tools plugin summary");
        assert_eq!(app_tools["scope"], "user");
        assert_eq!(app_tools["enabled"], true);
        assert_eq!(app_tools["valid"], true);
        assert_eq!(app_tools["components"]["commands"], 1);
        assert_eq!(app_tools["components"]["agents"], 1);
        assert_eq!(app_tools["components"]["skills"], 1);
        assert_eq!(app_tools["components"]["hooks"], 1);
        assert_eq!(app_tools["components"]["output_styles"], 1);
        assert_eq!(app_tools["components"]["lsp_servers"], 1);
        assert_eq!(app_tools["components"]["apps"], 1);
        assert_eq!(app_tools["components"]["mcp_servers"], 1);
        assert_eq!(app_tools["app_manifest"]["id"], "review-workbench");
        assert_eq!(app_tools["app_manifest"]["title"], "Review Workbench");
        assert_eq!(app_tools["app_manifest"]["entry"], "apps/review/index.html");
        assert_eq!(app_tools["app_manifest"]["routes"][0]["path"], "/review");
        assert_eq!(app_tools["app_manifest"]["routes"][0]["title"], "Review");
        let disabled_tools = plugin_items
            .iter()
            .find(|plugin| plugin["id"] == "disabled-tools")
            .expect("disabled-tools plugin summary");
        assert_eq!(disabled_tools["scope"], "user");
        assert_eq!(disabled_tools["enabled"], false);
        assert_eq!(disabled_tools["valid"], true);
        assert_eq!(disabled_tools["components"]["commands"], 1);
        assert!(!plugin_items
            .iter()
            .any(|plugin| plugin["id"] == "project-tools"));
        assert!(!plugin_items
            .iter()
            .any(|plugin| plugin["id"] == "local-tools"));

        kiana_types::write_project_trust(&workspace, kiana_types::ProjectTrust::Trusted).unwrap();
        let trusted_plugins: Value = client
            .get(format!("http://{addr}/app/plugins"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(trusted_plugins["schema"], "kiana.app-server.plugins.v1");
        assert_eq!(trusted_plugins["count"], 4);
        let trusted_plugin_items = trusted_plugins["plugins"].as_array().unwrap();
        let project_tools = trusted_plugin_items
            .iter()
            .find(|plugin| plugin["id"] == "project-tools")
            .expect("project-tools plugin summary");
        assert_eq!(project_tools["scope"], "project");
        assert_eq!(
            project_tools["root"],
            project_plugin_root.display().to_string()
        );
        assert_eq!(project_tools["enabled"], true);
        assert_eq!(project_tools["valid"], true);
        assert_eq!(project_tools["components"]["commands"], 1);
        let local_tools = trusted_plugin_items
            .iter()
            .find(|plugin| plugin["id"] == "local-tools")
            .expect("local-tools plugin summary");
        assert_eq!(local_tools["scope"], "local");
        assert_eq!(local_tools["root"], local_plugin_root.display().to_string());
        assert_eq!(local_tools["enabled"], true);
        assert_eq!(local_tools["valid"], true);
        assert_eq!(local_tools["components"]["skills"], 1);

        let auth_status: Value = client
            .get(format!("http://{addr}/app/auth/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(auth_status["schema"], "kiana.auth-status.v1");
        assert_eq!(auth_status["api_key"], "missing");
        assert_eq!(auth_status["source"], "none");
        assert_eq!(auth_status["oauth"]["status"], "missing");
        assert!(auth_status["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider["provider_id"] == "anthropic"
                && provider["status"] == "missing"));
        assert!(auth_status["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider["provider_id"] == "ollama"
                && provider["auth"] == "not_required"));
        assert!(!auth_status.to_string().contains("must-not-leak"));

        let license_status: Value = client
            .get(format!("http://{addr}/app/license/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(license_status["schema"], "kiana.license-status.v1");
        assert_eq!(license_status["status"], "missing");
        assert_eq!(license_status["source"], "none");
        assert_eq!(license_status["license_key"], "missing");
        assert_eq!(license_status["offline"], false);
        assert!(license_status["issues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue == "no enterprise license configured"));
        assert!(!license_status.to_string().contains("must-not-leak"));

        let model_catalog: Value = client
            .get(format!("http://{addr}/app/models/catalog"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(model_catalog["schema"], "kiana.model-catalog.v1");
        assert_eq!(model_catalog["live"], false);
        assert_eq!(model_catalog["summary"]["failed"], 0);
        assert!(model_catalog["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider["provider_id"] == "fake" && provider["status"] == "static"));
        assert!(model_catalog["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| provider["provider_id"] == "openai-compatible"
                && provider["status"] == "skipped"));

        let model_list: Value = client
            .get(format!("http://{addr}/app/models/list"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(model_list["schema"], "kiana.model-list.v1");
        let profiles = model_list["profiles"].as_array().unwrap();
        assert!(profiles.iter().any(|profile| {
            profile["provider_id"] == "anthropic"
                && profile["model_id"] == "claude-sonnet-4-6"
                && profile["streaming_mode"] == "native"
                && profile["native_streaming"] == true
        }));
        assert!(profiles.iter().any(|profile| {
            profile["provider_id"] == "openai-compatible"
                && profile["model_id"] == "gpt-4.1"
                && profile["supports_tools"] == true
                && profile["streaming_mode"] == "synthetic"
        }));
        assert_eq!(model_list["summary"]["profiles"], profiles.len());

        let current_model: Value = client
            .get(format!("http://{addr}/app/models/current"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current_model["schema"], "kiana.app-server.model-current.v1");
        assert_eq!(current_model["workspace"], workspace.display().to_string());
        assert_eq!(current_model["source"], "app_state");
        assert_eq!(current_model["provider_id"], "anthropic");
        assert_eq!(current_model["model_id"], "opus-4.8-1m");
        assert_eq!(current_model["configured"], true);
        assert_eq!(current_model["profile"]["provider_id"], "anthropic");
        assert_eq!(current_model["profile"]["model_id"], "opus-4.8-1m");
        assert_eq!(current_model["profile"]["supports_tools"], true);
        assert_eq!(current_model["profile"]["streaming_mode"], "native");
        assert_eq!(current_model["profile"]["native_streaming"], true);

        let model_write_conflict = client
            .post(format!("http://{addr}/app/models/current"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "model_id": "gpt-4.1",
                "provider_id": "openai-compatible"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(model_write_conflict.status(), reqwest::StatusCode::CONFLICT);

        let doctor: Value = client
            .get(format!("http://{addr}/app/doctor"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(doctor["schema"], "kiana.app-server.doctor.v1");
        assert_eq!(doctor["workspace"], workspace.display().to_string());
        assert_eq!(doctor["doctor"]["schema"], "kiana.doctor.v1");
        let reference_capabilities = doctor["doctor"]["reference_capabilities"]
            .as_array()
            .unwrap();
        let local_coding = reference_capabilities
            .iter()
            .find(|capability| capability["id"] == "local-coding-workflow")
            .expect("local-coding-workflow readiness missing");
        assert_eq!(local_coding["status"], "ready");
        assert!(local_coding["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .any(|evidence| evidence == "deterministic-repo-map-budget"));
        assert!(local_coding["risks"].as_array().unwrap().is_empty());

        let release_blockers: Value = client
            .get(format!("http://{addr}/app/release/blockers"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            release_blockers["schema"], "kiana.commercial-release-blockers.v1",
            "unexpected release blockers response: {}",
            release_blockers
        );
        assert!(release_blockers["summary"]["total_checks"]
            .as_u64()
            .is_some_and(|count| count > 0));
        assert!(release_blockers["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["id"] == "source.remote" && check["external"] == true));

        let local_rc_evidence_path = workspace.join("local-rc-evidence.json");
        std::fs::write(
            &local_rc_evidence_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.local-rc-evidence.v1",
                "version": "0.1.0",
                "generated_at": "2026-07-05T00:00:00Z",
                "status": "local_rc_ready",
                "dist_dir": "dist",
                "summary": {
                    "release_artifacts": 1,
                    "manifests": 2,
                    "proofs": 2,
                    "blockers_total": 13,
                    "local_blockers": 0,
                    "external_blockers": 13
                },
                "release_artifacts": [{
                    "target": "linux-x86_64",
                    "archive": "dist/kiana-0.1.0-linux-x86_64.tar.gz",
                    "archive_sha256": "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507",
                    "binary_sha256": "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0",
                    "lifecycle_smoke": "passed"
                }],
                "distribution_manifests": {
                    "enterprise_offline_manifest": "dist/manifests/enterprise/offline-manifest.json",
                    "homebrew_formulae": ["dist/manifests/homebrew/kiana-linux-x86_64.rb"],
                    "winget_manifests": [],
                    "blocked_channels": ["dist/manifests/winget/BLOCKED.md"]
                },
                "proofs": [{
                    "path": "dist/proofs/product/product-acceptance-local-rc.json",
                    "schema": "kiana.product-acceptance.v1",
                    "status": "headless_smoke_only",
                    "accepted": false
                }],
                "blockers": {
                    "status": "blocked",
                    "blocking": 13,
                    "local_blocking": 0,
                    "external_blocking": 13,
                    "blocking_ids": ["source.remote"],
                    "external_blocking_ids": ["source.remote"],
                    "handoff_status": "external_action_required",
                    "handoff_artifacts": [{
                        "kind": "blockers_json",
                        "path": "dist/proofs/local-rc/blockers/commercial-release-blockers.json",
                        "sha256": "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507"
                    }, {
                        "kind": "handoff_markdown",
                        "path": "dist/proofs/local-rc/blockers/commercial-release-handoff.md",
                        "sha256": "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0"
                    }]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_LOCAL_RC_EVIDENCE_OUT", &local_rc_evidence_path);
        let local_rc_evidence: Value = client
            .get(format!("http://{addr}/app/release/local-rc-evidence"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(local_rc_evidence["schema"], "kiana.local-rc-evidence.v1");
        assert_eq!(local_rc_evidence["status"], "local_rc_ready");
        assert_eq!(local_rc_evidence["summary"]["local_blockers"], 0);
        assert_eq!(
            local_rc_evidence["release_artifacts"][0]["target"],
            "linux-x86_64"
        );
        assert!(local_rc_evidence["proofs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|proof| proof["schema"] == "kiana.product-acceptance.v1"));
        assert_eq!(
            local_rc_evidence["blockers"]["handoff_status"],
            "external_action_required"
        );
        assert_eq!(
            local_rc_evidence["blockers"]["external_blocking_ids"][0],
            "source.remote"
        );
        assert!(local_rc_evidence["blockers"]["handoff_artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["kind"] == "handoff_markdown"));

        let product_acceptance_path = std::env::temp_dir().join(format!(
            "kiana-product-acceptance-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &product_acceptance_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.product-acceptance.v1",
                "version": "0.1.0",
                "status": "headless_smoke_only",
                "accepted": false,
                "accepted_by": "",
                "accepted_at": "2026-07-05T00:00:00Z",
                "scope": "headless product-shell, app-server, and context-search smoke only",
                "workflows": [
                    "permission",
                    "diff",
                    "history",
                    "onboarding",
                    "resume",
                    "settings",
                    "app-server",
                    "context-search",
                    "context-cache-recovery"
                ],
                "notes": [
                    "Generated by scripts/product-acceptance-report.sh --local-rc"
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_PRODUCT_ACCEPTANCE_OUT", &product_acceptance_path);
        let product_acceptance: Value = client
            .get(format!("http://{addr}/app/release/product-acceptance"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(product_acceptance["schema"], "kiana.product-acceptance.v1");
        assert_eq!(product_acceptance["status"], "headless_smoke_only");
        assert_eq!(product_acceptance["accepted"], false);
        assert!(product_acceptance["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workflow| workflow == "app-server"));
        assert!(product_acceptance["workflows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workflow| workflow == "context-cache-recovery"));
        let _ = std::fs::remove_file(&product_acceptance_path);

        let entitlement_path = std::env::temp_dir().join(format!(
            "kiana-entitlement-proof-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &entitlement_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.entitlement-proof.v1",
                "version": "0.1.0",
                "status": "local_rc_only",
                "accepted": false,
                "accepted_by": "",
                "accepted_at": "2026-07-05T00:00:00Z",
                "account_id": "",
                "organization": "",
                "plan": "",
                "license_status": "missing",
                "entitlements": [
                    "commercial-use",
                    "enterprise-support",
                    "managed-policy"
                ],
                "license_key_fingerprint": "",
                "support_contact": "",
                "backend": {
                    "name": "",
                    "environment": "",
                    "checked_at": "2026-07-05T00:00:00Z",
                    "request_id": ""
                },
                "notes": [
                    "Generated by scripts/entitlement-proof-report.sh --local-rc"
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_ENTITLEMENT_PROOF_OUT", &entitlement_path);
        let entitlement: Value = client
            .get(format!("http://{addr}/app/release/entitlement"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(entitlement["schema"], "kiana.entitlement-proof.v1");
        assert_eq!(entitlement["status"], "local_rc_only");
        assert_eq!(entitlement["accepted"], false);
        assert_eq!(entitlement["license_status"], "missing");
        assert!(entitlement["entitlements"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entitlement| entitlement == "managed-policy"));
        let _ = std::fs::remove_file(&entitlement_path);

        let release_ops_path =
            std::env::temp_dir().join(format!("kiana-release-ops-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(
            &release_ops_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.release-ops.v1",
                "version": "0.1.0",
                "status": "local_rc_only",
                "accepted": false,
                "accepted_by": "",
                "accepted_at": "2026-07-05T00:00:00Z",
                "security_contact": "",
                "vulnerability_report_channel": "",
                "release_credentials_owner": "",
                "support_contact": "",
                "artifact_retention_days": 90,
                "log_retention_days": 30,
                "credential_review": {
                    "status": "pending",
                    "reviewed_by": "",
                    "reviewed_at": "",
                    "scope": "local RC only; production release credentials are not accepted"
                },
                "notes": [
                    "Generated by scripts/release-ops-report.sh --local-rc"
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_RELEASE_OPS_OUT", &release_ops_path);
        let release_ops: Value = client
            .get(format!("http://{addr}/app/release/ops"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(release_ops["schema"], "kiana.release-ops.v1");
        assert_eq!(release_ops["status"], "local_rc_only");
        assert_eq!(release_ops["accepted"], false);
        assert_eq!(release_ops["artifact_retention_days"], 90);
        assert_eq!(release_ops["credential_review"]["status"], "pending");
        let _ = std::fs::remove_file(&release_ops_path);

        let platform_security_path = std::env::temp_dir().join(format!(
            "kiana-platform-security-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &platform_security_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.platform-security-proof.v1",
                "version": "0.1.0",
                "status": "local_rc_only",
                "accepted": false,
                "accepted_by": "",
                "accepted_at": "2026-07-05T00:00:00Z",
                "platform": "linux",
                "runner": "local",
                "isolation": "linux_bwrap",
                "controls": [
                    "permission_profile:commercial",
                    "permission_mode:ask",
                    "exec_policy:bash+powershell"
                ],
                "doctor_status": "unknown",
                "evidence": [
                    {
                        "label": "mode",
                        "value": "local_rc_only"
                    },
                    {
                        "label": "isolation",
                        "value": "linux_bwrap"
                    }
                ],
                "notes": [
                    "Generated by scripts/platform-security-proof-report.sh --local-rc"
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_PLATFORM_SECURITY_PROOF_OUT", &platform_security_path);
        let platform_security: Value = client
            .get(format!("http://{addr}/app/release/platform-security"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            platform_security["schema"],
            "kiana.platform-security-proof.v1"
        );
        assert_eq!(platform_security["status"], "local_rc_only");
        assert_eq!(platform_security["accepted"], false);
        assert_eq!(platform_security["platform"], "linux");
        assert_eq!(platform_security["isolation"], "linux_bwrap");
        assert_eq!(platform_security["doctor_status"], "unknown");
        let _ = std::fs::remove_file(&platform_security_path);

        let source_control_path = std::env::temp_dir().join(format!(
            "kiana-source-control-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &source_control_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.source-control-proof.v1",
                "version": "0.1.0",
                "status": "local_rc_only",
                "accepted": false,
                "accepted_by": "",
                "accepted_at": "2026-07-05T00:00:00Z",
                "remote_url": "",
                "commit": "0000000000000000000000000000000000000000",
                "release_tag": "v0.1.0",
                "tagged_commit": "0000000000000000000000000000000000000000",
                "pushed": false,
                "reviewed": false,
                "notes": [
                    "Local RC only; production source-control proof is not accepted"
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_SOURCE_CONTROL_PROOF_OUT", &source_control_path);
        let source_control: Value = client
            .get(format!("http://{addr}/app/release/source-control"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(source_control["schema"], "kiana.source-control-proof.v1");
        assert_eq!(source_control["status"], "local_rc_only");
        assert_eq!(source_control["accepted"], false);
        assert_eq!(source_control["release_tag"], "v0.1.0");
        assert_eq!(source_control["pushed"], false);
        assert_eq!(source_control["reviewed"], false);
        let _ = std::fs::remove_file(&source_control_path);

        let release_signature_path = std::env::temp_dir().join(format!(
            "kiana-release-signature-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &release_signature_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.release-signature.v1",
                "target": "linux-x86_64",
                "archive": "kiana-0.1.0-linux-x86_64.tar.gz",
                "archive_sha256": "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507",
                "binary_sha256_file_sha256": "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0",
                "signed_at": "2026-07-05T00:00:00Z",
                "signer": "local-rc-signer",
                "signature_files": {
                    "archive": "kiana-0.1.0-linux-x86_64.tar.gz.sig",
                    "binary": "kiana-0.1.0-linux-x86_64.binary.sig"
                },
                "verification": {
                    "method": "KIANA_SIGNATURE_VERIFY_COMMAND",
                    "archive": "verified",
                    "binary": "verified",
                    "verified_at": "2026-07-05T00:00:00Z"
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_RELEASE_SIGNATURE_PROOF_OUT", &release_signature_path);
        let release_signature: Value = client
            .get(format!("http://{addr}/app/release/signature"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(release_signature["schema"], "kiana.release-signature.v1");
        assert_eq!(release_signature["target"], "linux-x86_64");
        assert_eq!(
            release_signature["archive"],
            "kiana-0.1.0-linux-x86_64.tar.gz"
        );
        assert_eq!(release_signature["signer"], "local-rc-signer");
        assert_eq!(
            release_signature["verification"]["method"],
            "KIANA_SIGNATURE_VERIFY_COMMAND"
        );
        assert_eq!(release_signature["verification"]["archive"], "verified");
        assert_eq!(release_signature["verification"]["binary"], "verified");
        let _ = std::fs::remove_file(&release_signature_path);

        let enterprise_offline_manifest_path = std::env::temp_dir().join(format!(
            "kiana-enterprise-offline-manifest-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &enterprise_offline_manifest_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.enterprise.offline-manifest.v1",
                "version": "0.1.0",
                "release_base_url": "https://downloads.example.test/kiana/v0.1.0",
                "artifacts": [{
                    "target": "linux-x86_64",
                    "archive": "kiana-0.1.0-linux-x86_64.tar.gz",
                    "url": "https://downloads.example.test/kiana/v0.1.0/kiana-0.1.0-linux-x86_64.tar.gz",
                    "sha256": "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507",
                    "binary_sha256": "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0",
                    "local_path": "kiana-0.1.0-linux-x86_64.tar.gz",
                    "checksum_path": "kiana-0.1.0-linux-x86_64.tar.gz.sha256",
                    "binary_checksum_path": "kiana-0.1.0-linux-x86_64.binary.sha256"
                }],
                "channels": {
                    "github_releases": "generated_from_release_base_url",
                    "homebrew": "generated",
                    "winget": "blocked_no_windows_publishable_artifact"
                },
                "generated_by": "scripts/generate-distribution-manifests.sh"
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var(
            "KIANA_ENTERPRISE_OFFLINE_MANIFEST_OUT",
            &enterprise_offline_manifest_path,
        );
        let enterprise_offline_manifest: Value = client
            .get(format!(
                "http://{addr}/app/release/enterprise-offline-manifest"
            ))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            enterprise_offline_manifest["schema"],
            "kiana.enterprise.offline-manifest.v1"
        );
        assert_eq!(enterprise_offline_manifest["version"], "0.1.0");
        assert_eq!(
            enterprise_offline_manifest["artifacts"][0]["target"],
            "linux-x86_64"
        );
        assert_eq!(
            enterprise_offline_manifest["channels"]["homebrew"],
            "generated"
        );
        assert_eq!(
            enterprise_offline_manifest["generated_by"],
            "scripts/generate-distribution-manifests.sh"
        );
        let _ = std::fs::remove_file(&enterprise_offline_manifest_path);

        let distribution_dist_dir =
            std::env::temp_dir().join(format!("kiana-dist-review-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(distribution_dist_dir.join("manifests").join("homebrew")).unwrap();
        std::fs::create_dir_all(distribution_dist_dir.join("manifests").join("winget")).unwrap();
        std::fs::create_dir_all(distribution_dist_dir.join("manifests").join("enterprise"))
            .unwrap();
        std::fs::write(
            distribution_dist_dir.join("kiana-0.1.0-linux-x86_64.tar.gz"),
            "archive",
        )
        .unwrap();
        std::fs::write(
            distribution_dist_dir.join("kiana-0.1.0-linux-x86_64.tar.gz.sha256"),
            "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507  kiana-0.1.0-linux-x86_64.tar.gz\n",
        )
        .unwrap();
        std::fs::write(
            distribution_dist_dir.join("kiana-0.1.0-linux-x86_64.binary.sha256"),
            "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0  kiana\n",
        )
        .unwrap();
        std::fs::write(
            distribution_dist_dir
                .join("manifests")
                .join("homebrew")
                .join("kiana-linux-x86_64.rb"),
            "class Kiana < Formula\nend\n",
        )
        .unwrap();
        std::fs::write(
            distribution_dist_dir
                .join("manifests")
                .join("winget")
                .join("BLOCKED.md"),
            "blocked until Windows artifact is publishable\n",
        )
        .unwrap();
        std::fs::write(
            distribution_dist_dir
                .join("manifests")
                .join("enterprise")
                .join("offline-manifest.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.enterprise.offline-manifest.v1",
                "version": "0.1.0",
                "release_base_url": "https://downloads.example.test/kiana/v0.1.0",
                "artifacts": [{
                    "target": "linux-x86_64",
                    "archive": "kiana-0.1.0-linux-x86_64.tar.gz",
                    "url": "https://downloads.example.test/kiana/v0.1.0/kiana-0.1.0-linux-x86_64.tar.gz",
                    "sha256": "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507",
                    "binary_sha256": "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0",
                    "local_path": "kiana-0.1.0-linux-x86_64.tar.gz",
                    "checksum_path": "kiana-0.1.0-linux-x86_64.tar.gz.sha256",
                    "binary_checksum_path": "kiana-0.1.0-linux-x86_64.binary.sha256"
                }],
                "channels": {
                    "github_releases": "generated_from_release_base_url",
                    "homebrew": "generated",
                    "winget": "blocked_no_windows_publishable_artifact"
                },
                "generated_by": "scripts/generate-distribution-manifests.sh"
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_DISTRIBUTION_REVIEW_DIST_DIR", &distribution_dist_dir);
        let distribution_review: Value = client
            .get(format!("http://{addr}/app/release/distribution"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            distribution_review["schema"],
            "kiana.app-server.distribution-review.v1"
        );
        assert_eq!(distribution_review["summary"]["artifacts"], 1);
        assert_eq!(distribution_review["summary"]["platforms"]["linux"], true);
        assert_eq!(distribution_review["summary"]["platforms"]["macos"], false);
        assert_eq!(
            distribution_review["summary"]["platforms"]["windows"],
            false
        );
        assert_eq!(
            distribution_review["channels"]["homebrew"]["status"],
            "ready"
        );
        assert_eq!(
            distribution_review["channels"]["winget"]["status"],
            "blocked"
        );
        assert_eq!(
            distribution_review["channels"]["winget"]["blocked_path"],
            "manifests/winget/BLOCKED.md"
        );
        assert_eq!(
            distribution_review["enterprise_offline_manifest"]["valid"],
            true
        );
        assert!(distribution_review["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["target"] == "linux-x86_64"
                && artifact["sha256_file"] == "kiana-0.1.0-linux-x86_64.tar.gz.sha256"
                && artifact["binary_sha256_file"] == "kiana-0.1.0-linux-x86_64.binary.sha256"));
        assert!(distribution_review["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker["id"] == "distribution.platform-artifacts"));
        std::fs::remove_file(
            distribution_dist_dir
                .join("manifests")
                .join("winget")
                .join("BLOCKED.md"),
        )
        .unwrap();
        let nested_winget_dir = distribution_dist_dir
            .join("manifests")
            .join("winget")
            .join("Kiana.Kiana")
            .join("0.1.0");
        std::fs::create_dir_all(&nested_winget_dir).unwrap();
        std::fs::write(
            nested_winget_dir.join("Kiana.Kiana.installer.yaml"),
            "PackageIdentifier: Kiana.Kiana\nManifestType: installer\n",
        )
        .unwrap();
        let distribution_review_with_winget: Value = client
            .get(format!("http://{addr}/app/release/distribution"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            distribution_review_with_winget["channels"]["winget"]["status"],
            "ready"
        );
        assert!(
            distribution_review_with_winget["channels"]["winget"]["manifests"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path.as_str()
                    == Some("manifests/winget/Kiana.Kiana/0.1.0/Kiana.Kiana.installer.yaml"))
        );
        let _ = std::fs::remove_dir_all(&distribution_dist_dir);

        let commercial_proof_manifest_path = std::env::temp_dir().join(format!(
            "kiana-commercial-proof-manifest-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &commercial_proof_manifest_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.commercial-proof-manifest.v1",
                "version": "0.1.0",
                "generated_at": "2026-07-05T00:00:00Z",
                "proof_root": "dist/proofs",
                "summary": {
                    "proofs": 2,
                    "accepted": 1,
                    "live": 1,
                    "platforms": ["linux"]
                },
                "proofs": [
                    {
                        "id": "source-control",
                        "category": "source-control",
                        "schema": "kiana.source-control-proof.v1",
                        "status": "accepted",
                        "accepted": true,
                        "live": null,
                        "source": "docs/source-control/0.1.0.json",
                        "path": "dist/proofs/source-control/source-control.json",
                        "sha256": "571df486310be5fd8d2f156fefb1bde471819469cbadd7da496661d31685b507",
                        "details": {
                            "release_tag": "v0.1.0"
                        }
                    },
                    {
                        "id": "live.provider-smoke",
                        "category": "live-service",
                        "schema": "kiana.model-smoke.v1",
                        "status": "passed",
                        "accepted": null,
                        "live": true,
                        "source": "target/live-smoke/provider/model-smoke-live-tools.json",
                        "path": "dist/proofs/live-smoke/provider/model-smoke-live-tools.json",
                        "sha256": "c5b71ee1539499b16c41126e922bb05355318745e27015a5e82c864c24b375c0",
                        "details": {
                            "provider": "fake-live"
                        }
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var(
            "KIANA_COMMERCIAL_PROOF_MANIFEST_OUT",
            &commercial_proof_manifest_path,
        );
        let commercial_proof_manifest: Value = client
            .get(format!("http://{addr}/app/release/proof-manifest"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            commercial_proof_manifest["schema"],
            "kiana.commercial-proof-manifest.v1"
        );
        assert_eq!(commercial_proof_manifest["summary"]["proofs"], 2);
        assert_eq!(commercial_proof_manifest["summary"]["accepted"], 1);
        assert_eq!(commercial_proof_manifest["summary"]["live"], 1);
        assert!(commercial_proof_manifest["proofs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|proof| proof["id"] == "source-control" && proof["category"] == "source-control"));
        assert!(commercial_proof_manifest["proofs"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |proof| proof["id"] == "live.provider-smoke" && proof["category"] == "live-service"
            ));
        let _ = std::fs::remove_file(&commercial_proof_manifest_path);

        let provider_catalog_path = std::env::temp_dir().join(format!(
            "kiana-provider-live-catalog-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &provider_catalog_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.model-catalog.v1",
                "live": true,
                "summary": {
                    "providers": 1,
                    "discovered_models": 1,
                    "skipped": 0,
                    "failed": 0
                },
                "providers": [
                    {
                        "provider_id": "openai-compatible",
                        "display_name": "OpenAI Compatible",
                        "protocol": "open_ai_chat_completions",
                        "models_source": "user_configured",
                        "status": "passed",
                        "live": true,
                        "model_ids": ["gpt-4.1"],
                        "discovered_model_ids": ["gpt-4.1"],
                        "message": "live catalog accepted",
                        "base_url": "https://api.example.test/v1"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let provider_smoke_path = std::env::temp_dir().join(format!(
            "kiana-provider-live-smoke-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &provider_smoke_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.model-smoke.v1",
                "live": true,
                "tools": true,
                "summary": {
                    "passed": 2,
                    "skipped": 0,
                    "failed": 0
                },
                "results": [
                    {
                        "provider_id": "openai-compatible",
                        "model_id": "gpt-4.1",
                        "status": "passed",
                        "live": true,
                        "capability": "text",
                        "message": "text live smoke accepted",
                        "output_preview": "ok"
                    },
                    {
                        "provider_id": "openai-compatible",
                        "model_id": "gpt-4.1",
                        "status": "passed",
                        "live": true,
                        "capability": "tools",
                        "message": "tool live smoke accepted",
                        "output_preview": "tool ok"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_PROVIDER_LIVE_CATALOG_OUT", &provider_catalog_path);
        std::env::set_var("KIANA_PROVIDER_LIVE_SMOKE_OUT", &provider_smoke_path);
        let live_provider_smoke: Value = client
            .get(format!("http://{addr}/app/release/live-provider-smoke"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            live_provider_smoke["schema"],
            "kiana.app-server.live-provider-smoke.v1"
        );
        assert_eq!(
            live_provider_smoke["catalog"]["schema"],
            "kiana.model-catalog.v1"
        );
        assert_eq!(live_provider_smoke["catalog"]["live"], true);
        assert_eq!(
            live_provider_smoke["catalog"]["summary"]["discovered_models"],
            1
        );
        assert_eq!(
            live_provider_smoke["smoke"]["schema"],
            "kiana.model-smoke.v1"
        );
        assert_eq!(live_provider_smoke["smoke"]["live"], true);
        assert_eq!(live_provider_smoke["smoke"]["tools"], true);
        assert_eq!(live_provider_smoke["smoke"]["summary"]["passed"], 2);
        assert!(live_provider_smoke["catalog_path"]
            .as_str()
            .unwrap()
            .contains("kiana-provider-live-catalog"));
        assert!(live_provider_smoke["smoke_path"]
            .as_str()
            .unwrap()
            .contains("kiana-provider-live-smoke"));
        let _ = std::fs::remove_file(&provider_catalog_path);
        let _ = std::fs::remove_file(&provider_smoke_path);

        let model_smoke: Value = client
            .get(format!("http://{addr}/app/models/smoke"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(model_smoke["schema"], "kiana.model-smoke.v1");
        assert_eq!(model_smoke["live"], false);
        assert_eq!(model_smoke["tools"], true);
        assert_eq!(model_smoke["summary"]["failed"], 0);
        assert!(model_smoke["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["provider_id"] == "fake"
                && result["capability"] == "text"
                && result["status"] == "passed"));
        assert!(model_smoke["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|result| result["provider_id"] == "fake"
                && result["capability"] == "tools"
                && result["status"] == "passed"));

        let remote_code_session_smoke_path = std::env::temp_dir().join(format!(
            "kiana-remote-code-session-smoke-{}.json",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(
            &remote_code_session_smoke_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "schema": "kiana.remote-code-session-smoke.v1",
                "status": "ok",
                "checked_at": "2026-07-05T00:00:00Z",
                "session_id": "cse_live_remote_smoke",
                "api_base_url": "https://api.example.test",
                "sdk_url": "https://sdk.example.test",
                "expires_in": 3600,
                "worker_epoch": 3
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var(
            "KIANA_REMOTE_SMOKE_PROOF_OUT",
            &remote_code_session_smoke_path,
        );
        let remote_code_session_smoke: Value = client
            .get(format!(
                "http://{addr}/app/release/remote-code-session-smoke"
            ))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            remote_code_session_smoke["schema"],
            "kiana.remote-code-session-smoke.v1"
        );
        assert_eq!(remote_code_session_smoke["status"], "ok");
        assert_eq!(
            remote_code_session_smoke["session_id"],
            "cse_live_remote_smoke"
        );
        assert_eq!(
            remote_code_session_smoke["api_base_url"],
            "https://api.example.test"
        );
        assert_eq!(
            remote_code_session_smoke["sdk_url"],
            "https://sdk.example.test"
        );
        assert_eq!(remote_code_session_smoke["expires_in"], 3600);
        assert_eq!(remote_code_session_smoke["worker_epoch"], 3);
        let _ = std::fs::remove_file(&remote_code_session_smoke_path);

        let context_index: Value = client
            .get(format!("http://{addr}/app/context/index"))
            .bearer_auth("secret")
            .query(&[("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(context_index["schema"], "kiana.context-index.v1");
        assert_eq!(context_index["files_indexed"], 3);
        assert!(context_index["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "src/lib.rs"));
        assert!(context_index["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "Cargo.toml"));

        let cached_context_index: Value = client
            .get(format!("http://{addr}/app/context/index"))
            .bearer_auth("secret")
            .query(&[("cache", "true"), ("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(cached_context_index["schema"], "kiana.context-index.v1");
        assert_eq!(cached_context_index["cache"]["status"], "created");
        assert_eq!(cached_context_index["cache"]["added_files"], 3);
        assert!(cached_context_index["cache"]["path"]
            .as_str()
            .unwrap()
            .ends_with(".kiana/context-index.json"));
        assert!(workspace
            .join(".kiana")
            .join("context-index.json")
            .is_file());

        let context_artifacts: Value = client
            .get(format!("http://{addr}/app/context/artifacts"))
            .bearer_auth("secret")
            .query(&[("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(context_artifacts["schema"], "kiana.context-artifacts.v1");
        assert_eq!(context_artifacts["files_indexed"], 3);
        assert!(context_artifacts["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["path"] == "src/lib.rs"
                && artifact["kind"] == "file"
                && artifact["id"]
                    .as_str()
                    .unwrap()
                    .starts_with("file:src/lib.rs:")));

        let cached_context_artifacts: Value = client
            .get(format!("http://{addr}/app/context/artifacts"))
            .bearer_auth("secret")
            .query(&[("cache", "true"), ("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            cached_context_artifacts["schema"],
            "kiana.context-artifacts.v1"
        );
        assert_eq!(cached_context_artifacts["cache"]["status"], "created");
        assert_eq!(cached_context_artifacts["cache"]["added_artifacts"], 3);
        assert!(cached_context_artifacts["cache"]["path"]
            .as_str()
            .unwrap()
            .ends_with(".kiana/context-artifacts.json"));
        assert!(workspace
            .join(".kiana")
            .join("context-artifacts.json")
            .is_file());

        std::fs::create_dir_all(workspace.join("support")).unwrap();
        std::fs::write(workspace.join("support/brief.md"), "# Brief\nShip ingest\n").unwrap();
        let context_ingest: Value = client
            .post(format!("http://{addr}/app/context/ingest"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "source": "support",
                "max_bytes_per_file": 1024
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(context_ingest["schema"], "kiana.context-artifact-ingest.v1");
        assert_eq!(
            context_ingest["artifacts_schema"],
            "kiana.context-artifacts.v1"
        );
        assert_eq!(context_ingest["ingested_files"], 1);
        assert_eq!(context_ingest["skipped_files"], 0);
        assert_eq!(
            context_ingest["manifest_path"],
            ".kiana/context-ingest/manifest.json"
        );
        assert_eq!(
            context_ingest["sync"]["path"],
            ".kiana/context-ingest/manifest.json"
        );
        assert_eq!(context_ingest["sync"]["status"], "created");
        assert_eq!(context_ingest["sync"]["added_files"], 1);
        assert_eq!(context_ingest["sync"]["reused_files"], 0);
        assert!(context_ingest["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["source_path"] == "brief.md"
                && artifact["kind"] == "artifact"
                && artifact["stored_path"]
                    .as_str()
                    .unwrap()
                    .starts_with(".kiana/context-ingest/files/")));
        let stored_path = context_ingest["artifacts"][0]["stored_path"]
            .as_str()
            .unwrap();
        assert!(workspace.join(stored_path).is_file());
        assert!(workspace
            .join(".kiana/context-ingest/manifest.json")
            .is_file());
        let _ = std::fs::remove_dir_all(workspace.join("support"));

        let context_artifact_graph: Value = client
            .get(format!("http://{addr}/app/context/artifact-graph"))
            .bearer_auth("secret")
            .query(&[("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            context_artifact_graph["schema"],
            "kiana.context-artifact-dependency-graph.v1"
        );
        assert_eq!(context_artifact_graph["nodes"].as_array().unwrap().len(), 3);
        assert!(context_artifact_graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["path"] == "src/lib.rs" && node["kind"] == "file"));

        let context_artifact_store: Value = client
            .get(format!("http://{addr}/app/context/artifact-store"))
            .bearer_auth("secret")
            .query(&[("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            context_artifact_store["schema"],
            "kiana.context-artifact-store.v1"
        );
        assert_eq!(context_artifact_store["artifact_count"], 3);
        assert!(context_artifact_store["artifact_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "source" && role["count"] == 1));
        assert!(context_artifact_store["artifact_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "artifact" && role["count"] == 2));
        assert_eq!(
            context_artifact_store["dependency_graph_schema"],
            "kiana.context-artifact-dependency-graph.v1"
        );
        assert!(context_artifact_store["artifacts"]["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["path"] == "src/lib.rs"));

        let cached_context_artifact_store: Value = client
            .get(format!("http://{addr}/app/context/artifact-store"))
            .bearer_auth("secret")
            .query(&[("cache", "true"), ("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            cached_context_artifact_store["schema"],
            "kiana.context-artifact-store.v1"
        );
        assert_eq!(cached_context_artifact_store["cache"]["status"], "created");
        assert_eq!(cached_context_artifact_store["cache"]["added_artifacts"], 3);
        assert!(cached_context_artifact_store["artifact_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "source" && role["count"] == 1));
        assert_eq!(
            cached_context_artifact_store["cache"]["added_dependencies"],
            0
        );
        assert!(cached_context_artifact_store["cache"]["path"]
            .as_str()
            .unwrap()
            .ends_with(".kiana/context-artifact-store.json"));
        assert!(workspace
            .join(".kiana")
            .join("context-artifact-store.json")
            .is_file());

        let context_artifact_readiness: Value = client
            .get(format!("http://{addr}/app/context/artifact-readiness"))
            .bearer_auth("secret")
            .query(&[("max_bytes_per_file", "1024")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            context_artifact_readiness["schema"],
            "kiana.context-artifact-readiness.v1"
        );
        assert_eq!(context_artifact_readiness["status"], "incomplete");
        assert_eq!(
            context_artifact_readiness["artifact_store_schema"],
            "kiana.context-artifact-store.v1"
        );
        assert_eq!(context_artifact_readiness["artifact_count"], 3);
        assert_eq!(
            context_artifact_readiness["missing_roles"],
            serde_json::json!(["prd", "design", "tasks", "test"])
        );
        assert!(context_artifact_readiness["required_roles"]
            .as_array()
            .unwrap()
            .iter()
            .any(|role| role["role"] == "source" && role["present"] == true && role["count"] == 1));

        let context_search: Value = client
            .get(format!("http://{addr}/app/context/search"))
            .bearer_auth("secret")
            .query(&[("q", "checkout"), ("limit", "1")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(context_search["schema"], "kiana.context-search.v1");
        assert_eq!(context_search["query"], "checkout");
        assert_eq!(context_search["limit"], 1);
        assert_eq!(context_search["hits"].as_array().unwrap().len(), 1);
        assert_eq!(context_search["hits"][0]["path"], "src/lib.rs");

        let context_vector_search: Value = client
            .get(format!("http://{addr}/app/context/vector-search"))
            .bearer_auth("secret")
            .query(&[("q", "checkout flow"), ("limit", "1")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            context_vector_search["schema"],
            "kiana.context-vector-search.v1"
        );
        assert_eq!(context_vector_search["query"], "checkout flow");
        assert_eq!(
            context_vector_search["embedding_model"],
            "kiana.deterministic-hash-embedding.v1"
        );
        assert_eq!(context_vector_search["dimensions"], 64);
        assert_eq!(context_vector_search["limit"], 1);
        assert_eq!(context_vector_search["hits"].as_array().unwrap().len(), 1);
        assert_eq!(context_vector_search["hits"][0]["path"], "src/lib.rs");

        let context_pack: Value = client
            .get(format!("http://{addr}/app/context/pack"))
            .bearer_auth("secret")
            .query(&[
                ("q", "checkout"),
                ("limit", "1"),
                ("max_snippet_lines", "1"),
            ])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(context_pack["schema"], "kiana.context-pack.v1");
        assert_eq!(context_pack["query"], "checkout");
        assert_eq!(context_pack["limit"], 1);
        assert_eq!(context_pack["max_snippet_lines"], 1);
        assert_eq!(context_pack["snippets"].as_array().unwrap().len(), 1);
        assert_eq!(context_pack["snippets"][0]["path"], "src/lib.rs");
        assert_eq!(
            context_pack["artifact_graph"]["schema"],
            "kiana.context-artifact-graph.v1"
        );
        assert_eq!(
            context_pack["artifact_graph"]["nodes"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            context_pack["artifact_graph"]["edges"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            context_pack["artifact_graph"]["nodes"][0]["path"],
            "src/lib.rs"
        );
        assert_eq!(
            context_pack["artifact_graph"]["nodes"][0]["content_hash"],
            context_pack["snippets"][0]["content_hash"]
        );
        assert_eq!(
            context_pack["artifact_graph"]["edges"][0]["source"],
            "query:checkout"
        );
        assert_eq!(
            context_pack["artifact_graph"]["edges"][0]["target"],
            context_pack["artifact_graph"]["nodes"][0]["id"]
        );
        assert_eq!(
            context_pack["artifact_graph"]["edges"][0]["relation"],
            "matched"
        );

        let repo_map: Value = client
            .get(format!("http://{addr}/app/context/repo-map"))
            .bearer_auth("secret")
            .query(&[("max_tokens", "1000")])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(repo_map["schema"], "kiana.repo-map.v1");
        assert_eq!(repo_map["root"], workspace.display().to_string());
        assert_eq!(repo_map["token_budget"], 1000);
        assert_eq!(repo_map["truncated"], false);
        assert!(repo_map["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "src/lib.rs"
                && file["language"] == "rust"
                && file["symbols"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|symbol| symbol == "fn checkout")));

        let checks_dry_run: Value = client
            .get(format!("http://{addr}/app/checks/dry-run"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(checks_dry_run["schema"], "kiana.checks.dry_run.v1");
        assert_eq!(checks_dry_run["root"], workspace.display().to_string());
        let check_ids = checks_dry_run["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|check| check["id"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert!(check_ids.contains(&"rustfmt"));
        assert!(check_ids.contains(&"cargo_check"));

        let review_dry_run: Value = client
            .get(format!("http://{addr}/app/review/dry-run"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(review_dry_run["schema"], "kiana.review.dry_run.v1");
        assert_eq!(review_dry_run["root"], workspace.display().to_string());
        assert_eq!(review_dry_run["dry_run"], true);
        assert_eq!(review_dry_run["inside_git_repo"], false);
        assert!(review_dry_run["planned_steps"]
            .as_array()
            .unwrap()
            .is_empty());

        run_git_for_test(&workspace, &["init"]).await;
        run_git_for_test(&workspace, &["config", "user.email", "kiana@example.com"]).await;
        run_git_for_test(&workspace, &["config", "user.name", "Kiana"]).await;
        std::fs::create_dir_all(workspace.join("scripts")).unwrap();
        std::fs::write(
            workspace.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf 'app-review-ok\\n'\n",
        )
        .unwrap();
        run_git_for_test(&workspace, &["add", "."]).await;
        run_git_for_test(&workspace, &["commit", "-m", "initial"]).await;
        std::fs::write(
            workspace.join("src").join("lib.rs"),
            "pub fn checkout() {}\n// checkout checkout\n// working review\n",
        )
        .unwrap();
        std::fs::write(workspace.join("review-notes.txt"), "client review notes\n").unwrap();

        let checks_run: Value = client
            .get(format!("http://{addr}/app/checks"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(checks_run["schema"], "kiana.checks.run.v1");
        assert_eq!(checks_run["root"], workspace.display().to_string());
        assert_eq!(checks_run["dry_run"], false);
        assert_eq!(checks_run["inside_git_repo"], true);
        assert_eq!(checks_run["execution"]["isolation"], "git_worktree");
        assert_eq!(checks_run["summary"]["failed"], 0);
        assert!(checks_run["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["id"] == "release_smoke"
                && check["status"] == "passed"
                && check["stdout"] == "app-review-ok\n"));

        let review_run: Value = client
            .get(format!("http://{addr}/app/review"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(review_run["schema"], "kiana.review.run.v1");
        assert_eq!(review_run["root"], workspace.display().to_string());
        assert_eq!(review_run["dry_run"], false);
        assert_eq!(review_run["inside_git_repo"], true);
        assert_eq!(
            review_run["checks"]["execution"]["isolation"],
            "git_worktree"
        );
        assert_eq!(review_run["checks"]["summary"]["failed"], 0);
        assert!(review_run["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "review-notes.txt"));

        let diff: Value = client
            .get(format!("http://{addr}/app/diff"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(diff["schema"], "kiana.diff.v1");
        assert_eq!(diff["root"], workspace.display().to_string());
        assert_eq!(diff["inside_git_repo"], true);
        assert_eq!(diff["dirty"], true);
        assert!(diff["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "review-notes.txt"
                && file["index"] == "?"
                && file["worktree"] == "?"));

        let checkpoint: Value = client
            .post(format!("http://{addr}/app/checkpoints"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(checkpoint["schema"], "kiana.checkpoint.v1");
        assert_eq!(checkpoint["root"], workspace.display().to_string());
        assert_eq!(checkpoint["inside_git_repo"], true);
        assert_eq!(checkpoint["dirty"], true);
        assert_eq!(checkpoint["kind"], "manual");
        assert!(checkpoint["manifest_path"]
            .as_str()
            .unwrap()
            .ends_with("manifest.json"));
        assert!(checkpoint["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == "review-notes.txt"
                && file["index"] == "?"
                && file["worktree"] == "?"));

        let git_status: Value = client
            .get(format!("http://{addr}/app/git/status"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(git_status["schema"], "kiana.app-server.git-status.v1");
        assert_eq!(git_status["workspace"], workspace.display().to_string());

        server.abort();
        let _ = std::fs::remove_dir_all(workspace);
        let _ = std::fs::remove_dir_all(kiana_home);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_app_model_current_post_updates_config_when_no_runtime_override() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["ANTHROPIC_MODEL", "KIANA_CONFIG_FILE", "KIANA_HOME"]);
        let workspace =
            std::env::temp_dir().join(format!("kiana-direct-model-post-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        std::env::remove_var("ANTHROPIC_MODEL");
        std::env::set_var("KIANA_HOME", workspace.join(".kiana-home"));
        std::env::set_var("KIANA_CONFIG_FILE", workspace.join("config.toml"));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_args = DirectConnectServerArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("secret".to_string()),
            unix_socket: None,
            workspace: Some(workspace.clone()),
            idle_timeout_ms: 1000,
            max_sessions: 32,
        };
        let state = direct_connect_server_state(
            server_args,
            addr,
            Some("secret".to_string()),
            HashMap::new(),
        )
        .unwrap();
        let app = direct_connect_server_router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let client = reqwest::Client::new();

        let invalid_provider = client
            .post(format!("http://{addr}/app/models/current"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "model_id": "gpt-4.1",
                "provider_id": "anthropic"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_provider.status(), reqwest::StatusCode::BAD_REQUEST);

        let updated: Value = client
            .post(format!("http://{addr}/app/models/current"))
            .bearer_auth("secret")
            .json(&serde_json::json!({
                "model_id": "gpt-4.1",
                "provider_id": "openai-compatible"
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(updated["schema"], "kiana.app-server.model-current.v1");
        assert_eq!(updated["workspace"], workspace.display().to_string());
        assert_eq!(updated["source"], "config");
        assert_eq!(updated["provider_id"], "openai-compatible");
        assert_eq!(updated["model_id"], "gpt-4.1");
        assert_eq!(updated["configured"], true);
        assert_eq!(updated["profile"]["provider_id"], "openai-compatible");
        assert_eq!(updated["profile"]["model_id"], "gpt-4.1");
        assert_eq!(updated["profile"]["supports_tools"], true);
        assert_eq!(updated["profile"]["streaming_mode"], "synthetic");

        let current: Value = client
            .get(format!("http://{addr}/app/models/current"))
            .bearer_auth("secret")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(current["source"], "config");
        assert_eq!(current["provider_id"], "openai-compatible");
        assert_eq!(current["model_id"], "gpt-4.1");

        let config = std::fs::read_to_string(workspace.join("config.toml")).unwrap();
        assert!(config.contains("gpt-4.1"));

        server.abort();
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[test]
    fn direct_connect_app_contract_schemas_match_packaged_schema_files() {
        let workspace =
            std::env::temp_dir().join(format!("kiana-direct-app-schema-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let state = direct_connect_server_state(
            DirectConnectServerArgs {
                host: "127.0.0.1".to_string(),
                port: 0,
                auth_token: Some("secret".to_string()),
                unix_socket: None,
                workspace: Some(workspace.clone()),
                idle_timeout_ms: 1000,
                max_sessions: 32,
            },
            "127.0.0.1:0".parse().unwrap(),
            Some("secret".to_string()),
            HashMap::new(),
        )
        .unwrap();
        let contract = direct_connect_app_contract(&state, 0);
        let advertised = contract["endpoints"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|endpoint| endpoint.get("schema").and_then(Value::as_str))
            .filter(|schema| schema.starts_with("kiana.app-server."))
            .map(str::to_string)
            .collect::<BTreeSet<_>>();

        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root");
        let schema_dir = workspace_root.join("docs").join("schemas");
        let documented = std::fs::read_dir(&schema_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("kiana-app-server-")
            })
            .map(|entry| {
                let contents = std::fs::read_to_string(entry.path()).unwrap();
                let schema: Value = serde_json::from_str(&contents).unwrap();
                schema["properties"]["schema"]["const"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(advertised, documented);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_server_round_trips_open_client_to_model() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "KIANA_PERMISSION_MODE",
        ]);
        let workspace =
            std::env::temp_dir().join(format!("kiana-direct-server-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        let cwd = CurrentDirGuard::set(&workspace);

        let (base_url, model_requests, model_server) =
            start_direct_connect_text_mock_model_server().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_args = DirectConnectServerArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("abc".to_string()),
            unix_socket: None,
            workspace: Some(workspace.clone()),
            idle_timeout_ms: 10_000,
            max_sessions: 1,
        };
        let state = direct_connect_server_state(
            server_args,
            addr,
            Some("abc".to_string()),
            HashMap::from([
                ("api_key".to_string(), Value::String("test-key".to_string())),
                ("base_url".to_string(), Value::String(base_url)),
                ("model".to_string(), Value::String("mock-model".to_string())),
                ("no_session_persistence".to_string(), Value::Bool(true)),
            ]),
        )
        .unwrap();
        let app = direct_connect_server_router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let open_args = parse_direct_connect_open_args(&[
            "open".to_string(),
            format!("cc://{addr}?token=abc"),
            "-p".to_string(),
            "hello direct server".to_string(),
        ])
        .unwrap();
        let mut output = Vec::new();
        direct_connect_open_with_writer(&open_args, &mut output)
            .await
            .unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "direct server done\n");
        let requests = model_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["model"], "mock-model");
        assert!(requests[0]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"] == "hello direct server"));

        server.abort();
        model_server.abort();
        drop(cwd);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_server_round_trips_permission_prompt() {
        use futures_util::{SinkExt, StreamExt};

        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "KIANA_AGENT_ID",
            "KIANA_PERMISSION_MODE",
            "KIANA_TASKS_ROOT",
        ]);
        let workspace =
            std::env::temp_dir().join(format!("kiana-direct-permission-{}", uuid::Uuid::new_v4()));
        let tasks_root = workspace.join("tasks");
        std::fs::create_dir_all(&tasks_root).unwrap();
        std::env::set_var("KIANA_TASKS_ROOT", &tasks_root);
        std::env::set_var("KIANA_AGENT_ID", "agent-direct");

        let (base_url, model_server) = start_bridge_loop_mock_model_server().await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_args = DirectConnectServerArgs {
            host: "127.0.0.1".to_string(),
            port: 0,
            auth_token: Some("abc".to_string()),
            unix_socket: None,
            workspace: Some(workspace.clone()),
            idle_timeout_ms: 10_000,
            max_sessions: 1,
        };
        let state = direct_connect_server_state(
            server_args,
            addr,
            Some("abc".to_string()),
            HashMap::from([
                ("api_key".to_string(), Value::String("test-key".to_string())),
                ("base_url".to_string(), Value::String(base_url)),
                ("model".to_string(), Value::String("mock-model".to_string())),
                ("no_session_persistence".to_string(), Value::Bool(true)),
                (
                    "permission_mode".to_string(),
                    Value::String("ask".to_string()),
                ),
                ("project_trusted".to_string(), Value::Bool(true)),
            ]),
        )
        .unwrap();
        let app = direct_connect_server_router(state);
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let session: DirectConnectSessionResponse = reqwest::Client::new()
            .post(format!("http://{addr}/sessions"))
            .bearer_auth("abc")
            .json(&serde_json::json!({
                "cwd": workspace.display().to_string()
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut request = session.ws_url.as_str().into_client_request().unwrap();
        request
            .headers_mut()
            .insert(AUTHORIZATION, HeaderValue::from_static("Bearer abc"));
        let (mut websocket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
        websocket
            .send(Message::Text(
                serde_json::json!({
                    "type": "user",
                    "message": {
                        "role": "user",
                        "content": "create a task"
                    },
                    "parent_tool_use_id": null,
                    "session_id": session.session_id,
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let mut saw_permission_request = false;
        let mut final_result = None;
        while final_result.is_none() {
            let message = tokio::time::timeout(std::time::Duration::from_secs(2), websocket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let Message::Text(text) = message else {
                continue;
            };
            for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let event: Value = serde_json::from_str(line).unwrap();
                if event.get("type").and_then(Value::as_str) == Some("control_request")
                    && event
                        .get("request")
                        .and_then(|request| request.get("subtype"))
                        .and_then(Value::as_str)
                        == Some("can_use_tool")
                {
                    saw_permission_request = true;
                    assert_eq!(event["request"]["tool_name"], "TaskCreate");
                    assert_eq!(event["request"]["agent_id"], "agent-direct");
                    let request_id = event["request_id"].as_str().unwrap();
                    websocket
                        .send(Message::Text(
                            serde_json::json!({
                                "type": "control_response",
                                "response": {
                                    "subtype": "success",
                                    "request_id": request_id,
                                    "response": {
                                        "allowed": true
                                    }
                                }
                            })
                            .to_string()
                            .into(),
                        ))
                        .await
                        .unwrap();
                    continue;
                }
                if event.get("type").and_then(Value::as_str) == Some("result") {
                    final_result = Some(event);
                    break;
                }
            }
        }

        assert!(saw_permission_request);
        assert_eq!(final_result.unwrap()["result"], "task created");

        server.abort();
        model_server.abort();
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn direct_connect_open_headless_posts_session_and_streams_prompt() {
        let _guard = env_lock().lock().unwrap();
        let session_bodies = StdArc::new(StdMutex::new(Vec::<Value>::new()));
        let session_auth = StdArc::new(StdMutex::new(Vec::<Option<String>>::new()));
        let websocket_auth = StdArc::new(StdMutex::new(Vec::<Option<String>>::new()));
        let (message_tx, mut message_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let ws_url = format!("ws://{addr}/ws");

        let app = axum::Router::new()
            .route(
                "/sessions",
                axum::routing::post({
                    let session_bodies = session_bodies.clone();
                    let session_auth = session_auth.clone();
                    let ws_url = ws_url.clone();
                    move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
                        let session_bodies = session_bodies.clone();
                        let session_auth = session_auth.clone();
                        let ws_url = ws_url.clone();
                        async move {
                            session_bodies.lock().unwrap().push(body);
                            session_auth.lock().unwrap().push(
                                headers
                                    .get(axum::http::header::AUTHORIZATION)
                                    .and_then(|value| value.to_str().ok())
                                    .map(str::to_string),
                            );
                            Json(serde_json::json!({
                                "session_id": "direct-session-1",
                                "ws_url": ws_url,
                                "work_dir": "/tmp/direct-work"
                            }))
                        }
                    }
                }),
            )
            .route(
                "/ws",
                axum::routing::get({
                    let websocket_auth = websocket_auth.clone();
                    let message_tx = message_tx.clone();
                    move |headers: axum::http::HeaderMap,
                          ws: axum::extract::ws::WebSocketUpgrade| {
                        let websocket_auth = websocket_auth.clone();
                        let message_tx = message_tx.clone();
                        async move {
                            websocket_auth.lock().unwrap().push(
                                headers
                                    .get(axum::http::header::AUTHORIZATION)
                                    .and_then(|value| value.to_str().ok())
                                    .map(str::to_string),
                            );
                            ws.on_upgrade(move |mut socket| async move {
                                if let Some(Ok(axum::extract::ws::Message::Text(text))) =
                                    socket.recv().await
                                {
                                    let value: Value = serde_json::from_str(&text).unwrap();
                                    let _ = message_tx.send(value);
                                }
                                let assistant = serde_json::json!({
                                    "type": "assistant",
                                    "message": {
                                        "role": "assistant",
                                        "content": [{
                                            "type": "text",
                                            "text": "remote done"
                                        }]
                                    },
                                    "session_id": "direct-session-1",
                                    "parent_tool_use_id": null
                                })
                                .to_string();
                                let result = serde_json::json!({
                                    "type": "result",
                                    "subtype": "success",
                                    "is_error": false,
                                    "session_id": "direct-session-1",
                                    "result": "remote done"
                                })
                                .to_string();
                                let _ = socket
                                    .send(axum::extract::ws::Message::Text(
                                        format!("{assistant}\n{result}").into(),
                                    ))
                                    .await;
                            })
                        }
                    }
                }),
            );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let open_args = parse_direct_connect_open_args(&[
            "open".to_string(),
            format!("cc://{addr}?token=abc"),
            "-p".to_string(),
            "hello server".to_string(),
            "--dangerously-skip-permissions".to_string(),
        ])
        .unwrap();
        let mut output = Vec::new();
        direct_connect_open_with_writer(&open_args, &mut output)
            .await
            .unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "remote done\n");
        assert_eq!(
            session_auth.lock().unwrap()[0].as_deref(),
            Some("Bearer abc")
        );
        assert_eq!(
            websocket_auth.lock().unwrap()[0].as_deref(),
            Some("Bearer abc")
        );
        assert_eq!(
            session_bodies.lock().unwrap()[0]["dangerously_skip_permissions"],
            true
        );
        let user_message =
            tokio::time::timeout(std::time::Duration::from_secs(1), message_rx.recv())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(user_message["type"], "user");
        assert_eq!(user_message["message"]["content"], "hello server");
        assert_eq!(user_message["session_id"], "direct-session-1");

        server.abort();
    }

    #[test]
    fn stream_json_input_rejects_invalid_lines() {
        let error = parse_stream_json_input("{bad json}\n", "", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("line 1 is not valid JSON"));
        assert_eq!(
            stream_json_input_error_payload(&error),
            serde_json::json!({
                "schema": "kiana.stream-json-input-error.v1",
                "code": "invalid_json",
                "line": 1,
                "message": error,
            })
        );

        let error = parse_stream_json_input(
            r#"{"type":"assistant","message":{"role":"assistant","content":"history only"}}"#,
            "",
            false,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("requires at least one user message or prompt"));
    }

    #[test]
    fn print_args_accept_print_equals_form() {
        let args = vec![
            "--print=hello".to_string(),
            "--record-only".to_string(),
            "there".to_string(),
        ];

        assert_eq!(
            parse_print_args(&args).unwrap(),
            Some(PrintArgs {
                message: "hello there".to_string(),
                input_format: PrintInputFormat::Text,
                output_format: PrintOutputFormat::Text,
                execute: false,
                resident_teammate: false,
                json_schema: None,
                sdk_url: None,
                replay_user_messages: false,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn print_args_keep_flag_like_words_after_message_starts() {
        let args = vec![
            "-p".to_string(),
            "--record-only".to_string(),
            "explain".to_string(),
            "--json-schema".to_string(),
            "{}".to_string(),
        ];

        assert_eq!(
            parse_print_args(&args).unwrap(),
            Some(PrintArgs {
                message: "explain --json-schema {}".to_string(),
                input_format: PrintInputFormat::Text,
                output_format: PrintOutputFormat::Text,
                execute: false,
                resident_teammate: false,
                json_schema: None,
                sdk_url: None,
                replay_user_messages: false,
                include_partial_messages: false,
            })
        );
    }

    #[test]
    fn session_usage_documents_supported_subcommands() {
        let usage = session_usage();

        assert!(usage.contains(
            "kiana session <new|list|status|path|current|reply|show|rename|tag|files|fork|delete|import|export|compact>"
        ));
        assert!(usage.contains("path"));
        assert!(usage.contains("current"));
        assert!(usage.contains("delete <id>"));
        assert!(usage.contains("import <path>"));
        assert!(usage.contains("export <id>"));
        assert!(usage.contains("compact <id>"));
        assert!(usage.contains("reply <id>"));
        assert!(usage.contains("show <id>"));
        assert!(usage.contains("files <id>"));
    }

    #[tokio::test]
    async fn top_level_session_can_reach_local_session_management_commands() {
        let _guard = env_lock().lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-cli-session-routing-{}-{unique}",
            std::process::id()
        ));
        std::env::set_var("KIANA_HOME", &root);

        let result = run_local_session_command(&["path".to_string()])
            .await
            .unwrap();

        assert_eq!(
            result.value,
            root.join("sdk-sessions").display().to_string()
        );

        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("KIANA_HOME");
    }

    #[tokio::test]
    async fn top_level_session_files_updates_durable_file_sets() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["KIANA_HOME", "KIANA_SDK_SESSIONS_DIR"]);
        let root =
            std::env::temp_dir().join(format!("kiana-cli-session-files-{}", uuid::Uuid::new_v4()));
        let sessions = root.join("sdk-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("session-files.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "session_id": "session-files",
                "title": "Files",
                "tag": null,
                "parent_session_id": null,
                "created_at": 1,
                "updated_at": 1,
                "messages": []
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOME", &root);

        session_main(&[
            "files".to_string(),
            "session-files".to_string(),
            "--editable".to_string(),
            "src/lib.rs".to_string(),
            "--read-only".to_string(),
            "README.md".to_string(),
        ])
        .await
        .unwrap();

        let session: Value = serde_json::from_str(
            &std::fs::read_to_string(sessions.join("session-files.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(session["editable_files"], serde_json::json!(["src/lib.rs"]));
        assert_eq!(session["read_only_files"], serde_json::json!(["README.md"]));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn top_level_new_help_does_not_create_session() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["KIANA_HOME", "KIANA_SDK_SESSIONS_DIR"]);
        let root =
            std::env::temp_dir().join(format!("kiana-cli-new-help-{}", uuid::Uuid::new_v4()));
        std::env::set_var("KIANA_HOME", &root);

        session_main(&["new".to_string(), "--help".to_string()])
            .await
            .unwrap();

        assert!(!root.join("sdk-sessions").exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn top_level_reply_help_reports_usage() {
        let result = session_main(&["reply".to_string(), "--help".to_string()]).await;

        assert!(result.is_ok(), "reply --help failed: {result:?}");
    }

    #[tokio::test]
    async fn main_print_tool_loop_persists_session_and_model_facing_tool_result() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "KIANA_TOOLS",
            "KIANA_SDK_SESSIONS_DIR",
            "KIANA_HOME",
            "KIANA_PERMISSION_MODE",
            "KIANA_ALLOWED_TOOLS",
            "KIANA_DISALLOWED_TOOLS",
            "KIANA_MAX_ITERATIONS",
            "KIANA_MAX_THINKING_TOKENS",
            "KIANA_MAX_TOKENS",
            "ANTHROPIC_MAX_THINKING_TOKENS",
            "ANTHROPIC_MAX_TOKENS",
            kiana_tools::tool::ACCESS_ROOTS_ENV,
        ]);
        let root = std::env::temp_dir().join(format!(
            "kiana-cli-print-tool-loop-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&workspace).unwrap();
        let write_path = workspace.join("created.txt");
        let (base_url, state, server) =
            start_print_tool_loop_mock_model_server(write_path.to_string_lossy().to_string()).await;

        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        std::env::set_var("KIANA_SDK_SESSIONS_DIR", &sessions);
        std::env::set_var("KIANA_HOME", root.join("kiana-home"));
        kiana_types::write_project_trust(
            std::env::current_dir().unwrap(),
            kiana_types::ProjectTrust::Trusted,
        )
        .unwrap();

        main_with_args(vec![
            "--base-url".to_string(),
            base_url,
            "--model".to_string(),
            "mock-cli-model".to_string(),
            "--tools".to_string(),
            "Write".to_string(),
            "--session-id".to_string(),
            "cli-tool-loop-session".to_string(),
            "--name".to_string(),
            "CLI tool loop".to_string(),
            "-p".to_string(),
            "create a file".to_string(),
        ])
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(&write_path).unwrap(),
            "hello from cli\n"
        );
        let session_file = sessions.join("cli-tool-loop-session.json");
        let session: Value =
            serde_json::from_str(&std::fs::read_to_string(&session_file).unwrap()).unwrap();
        assert_eq!(session["title"], "CLI tool loop");
        assert_eq!(session["messages"].as_array().unwrap().len(), 4);
        assert_eq!(session["messages"][0]["content"], "create a file");
        assert_eq!(session["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(session["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(
            session["messages"][3]["content"][0]["text"],
            "cli write complete"
        );

        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0]["model"], "mock-cli-model");
        assert_eq!(requests[0]["tools"][0]["name"], "Write");
        let tool_result =
            &requests[1]["messages"].as_array().unwrap().last().unwrap()["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_cli_write");
        assert_eq!(
            tool_result["content"],
            serde_json::json!(format!(
                "File created successfully at: {}",
                write_path.to_string_lossy()
            ))
        );
        assert!(tool_result["content"]["originalFile"].is_null());

        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn main_print_multi_tool_loop_reads_edits_and_bashes_with_model_facing_results() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "KIANA_TOOLS",
            "KIANA_SDK_SESSIONS_DIR",
            "KIANA_HOME",
            "KIANA_PERMISSION_MODE",
            "KIANA_ALLOWED_TOOLS",
            "KIANA_DISALLOWED_TOOLS",
            "KIANA_MAX_ITERATIONS",
            "KIANA_MAX_THINKING_TOKENS",
            "KIANA_MAX_TOKENS",
            "ANTHROPIC_MAX_THINKING_TOKENS",
            "ANTHROPIC_MAX_TOKENS",
            kiana_tools::tool::ACCESS_ROOTS_ENV,
        ]);
        let previous_cwd = std::env::current_dir().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-cli-print-multi-tool-loop-{}",
            uuid::Uuid::new_v4()
        ));
        let workspace = root.join("workspace");
        let sessions = root.join("sessions");
        std::fs::create_dir_all(&workspace).unwrap();
        let note_path = workspace.join("note.txt");
        std::fs::write(&note_path, "alpha\n").unwrap();
        std::env::set_current_dir(&workspace).unwrap();
        let (base_url, state, server) =
            start_print_multi_tool_loop_mock_model_server(note_path.to_string_lossy().to_string())
                .await;

        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        std::env::set_var("KIANA_SDK_SESSIONS_DIR", &sessions);
        std::env::set_var("KIANA_HOME", root.join("kiana-home"));
        kiana_types::write_project_trust(&workspace, kiana_types::ProjectTrust::Trusted).unwrap();

        let result = main_with_args(vec![
            "--base-url".to_string(),
            base_url,
            "--model".to_string(),
            "mock-cli-model".to_string(),
            "--tools".to_string(),
            "Read,Edit,Bash".to_string(),
            "--session-id".to_string(),
            "cli-multi-tool-loop-session".to_string(),
            "--name".to_string(),
            "CLI multi tool loop".to_string(),
            "-p".to_string(),
            "read, edit, and verify the note".to_string(),
        ])
        .await;
        std::env::set_current_dir(previous_cwd).unwrap();
        result.unwrap();

        assert_eq!(std::fs::read_to_string(&note_path).unwrap(), "beta\n");
        let session_file = sessions.join("cli-multi-tool-loop-session.json");
        let session: Value =
            serde_json::from_str(&std::fs::read_to_string(&session_file).unwrap()).unwrap();
        assert_eq!(session["title"], "CLI multi tool loop");
        assert_eq!(session["messages"].as_array().unwrap().len(), 8);
        assert_eq!(
            session["messages"][7]["content"][0]["text"],
            "multi-tool flow complete"
        );

        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 4);
        let mut tool_names = requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<Vec<_>>();
        tool_names.sort();
        assert_eq!(tool_names, vec!["Bash", "Edit", "Read"]);

        let read_result =
            &requests[1]["messages"].as_array().unwrap().last().unwrap()["content"][0];
        assert_eq!(read_result["type"], "tool_result");
        assert_eq!(read_result["tool_use_id"], "toolu_cli_read");
        assert_eq!(read_result["content"], "1\talpha");
        assert!(read_result["content"]["file"].is_null());

        let edit_result =
            &requests[2]["messages"].as_array().unwrap().last().unwrap()["content"][0];
        assert_eq!(edit_result["type"], "tool_result");
        assert_eq!(edit_result["tool_use_id"], "toolu_cli_edit");
        assert_eq!(
            edit_result["content"],
            serde_json::json!(format!(
                "The file {} has been updated successfully.",
                note_path.to_string_lossy()
            ))
        );
        assert!(edit_result["content"]["updatedContent"].is_null());

        let bash_result =
            &requests[3]["messages"].as_array().unwrap().last().unwrap()["content"][0];
        assert_eq!(bash_result["type"], "tool_result");
        assert_eq!(bash_result["tool_use_id"], "toolu_cli_bash");
        assert_eq!(bash_result["content"], "beta");
        assert_eq!(bash_result["is_error"], false);
        assert!(bash_result["content"]["stdout"].is_null());

        server.abort();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn print_args_reject_invalid_json_schema() {
        let args = vec![
            "-p".to_string(),
            "--json-schema".to_string(),
            "not-json".to_string(),
            "message".to_string(),
        ];

        let error = parse_print_args(&args).unwrap_err().to_string();
        assert!(error.contains("--json-schema must be valid JSON"));
    }

    #[tokio::test]
    async fn format_print_result_returns_assistant_text_in_text_mode() {
        let result = serde_json::json!({
            "type": "sdk_prompt_completed",
            "assistant_text": "done",
        });

        assert_eq!(
            format_print_result(&result, &PrintOutputFormat::Text)
                .await
                .unwrap(),
            "done"
        );
    }

    #[tokio::test]
    async fn format_print_result_returns_structured_output_in_text_mode() {
        let result = serde_json::json!({
            "type": "sdk_prompt_completed",
            "assistant_text": "ignored",
            "structured_output": {
                "answer": "done"
            },
        });

        assert_eq!(
            format_print_result(&result, &PrintOutputFormat::Text)
                .await
                .unwrap(),
            "{\n  \"answer\": \"done\"\n}"
        );
    }

    #[tokio::test]
    async fn format_print_result_returns_full_result_in_json_mode() {
        let result = serde_json::json!({
            "type": "sdk_prompt_completed",
            "assistant_text": "done",
            "structured_output": {
                "answer": "done"
            },
        });

        let formatted = format_print_result(&result, &PrintOutputFormat::Json)
            .await
            .unwrap();
        let parsed: Value = serde_json::from_str(&formatted).unwrap();

        assert_eq!(parsed["type"], "sdk_prompt_completed");
        assert_eq!(parsed["assistant_text"], "done");
        assert_eq!(parsed["structured_output"]["answer"], "done");
    }

    #[tokio::test]
    async fn format_print_result_returns_stream_json_events() {
        let result = serde_json::json!({
            "type": "sdk_prompt_completed",
            "session_id": "session-1",
            "assistant_text": "done",
            "iterations": 2,
            "structured_output": {
                "answer": "done"
            },
        });

        let formatted =
            format_print_result_with_duration(&result, &PrintOutputFormat::StreamJson, 42, &[])
                .await
                .unwrap();
        let events = formatted
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 3);
        assert_eq!(events[0]["type"], "system");
        assert_eq!(events[0]["subtype"], "init");
        assert_eq!(events[0]["session_id"], "session-1");
        assert!(events[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool == "Bash"));
        assert_eq!(events[1]["type"], "assistant");
        assert_eq!(events[1]["message"]["content"][0]["text"], "done");
        assert_eq!(events[2]["type"], "result");
        assert_eq!(events[2]["subtype"], "success");
        assert_eq!(events[2]["duration_ms"], 42);
        assert_eq!(events[2]["num_turns"], 2);
        assert_eq!(events[2]["result"], "done");
        assert_eq!(events[2]["structured_output"]["answer"], "done");
    }

    #[test]
    fn stream_json_partial_event_exposes_tool_lifecycle_runtime_events() {
        let event = stream_json_partial_event(
            kiana_services::api::streaming::StreamEvent::ContentBlockStart {
                index: 0,
                content_block: kiana_services::api::streaming::ContentBlock::ToolUse(
                    kiana_services::api::streaming::ToolUse {
                        id: "toolu_mcp".to_string(),
                        name: "MCP".to_string(),
                        input: serde_json::json!({"server": "docs", "tool": "search"}),
                    },
                ),
            },
            "session-1",
        )
        .unwrap();

        assert_eq!(event["type"], "stream_event");
        assert_eq!(event["event"]["type"], "content_block_start");
        let lifecycle = event["runtime_events"].as_array().unwrap();
        assert_eq!(lifecycle.len(), 1);
        assert_eq!(lifecycle[0]["type"], "tool_call");
        assert_eq!(lifecycle[0]["tool_call_id"], "toolu_mcp");
        assert_eq!(lifecycle[0]["name"], "MCP");
        assert_eq!(lifecycle[0]["workbench"], "mcp");
    }

    #[test]
    fn stream_json_partial_event_exposes_tool_result_changed_files() {
        let event = stream_json_runner_event(
            crate::runner::RunnerStreamEvent::ToolResult {
                id: "toolu_write".to_string(),
                name: "Write".to_string(),
                is_error: false,
                content: "The file src/lib.rs has been updated successfully.".to_string(),
                error: None,
                changed_files: Some(serde_json::json!([
                    {
                        "path": "src/lib.rs",
                        "operation": "update",
                        "source": "Write"
                    }
                ])),
            },
            "session-1",
        )
        .unwrap();

        assert_eq!(event["type"], "stream_event");
        assert_eq!(event["event"]["type"], "tool_result");
        assert_eq!(event["event"]["tool_use_id"], "toolu_write");
        assert_eq!(event["event"]["changed_files"][0]["path"], "src/lib.rs");
        assert_eq!(event["event"]["changed_files"][0]["operation"], "update");
        let lifecycle = event["runtime_events"].as_array().unwrap();
        assert_eq!(lifecycle.len(), 1);
        assert_eq!(lifecycle[0]["type"], "tool_result");
        assert_eq!(lifecycle[0]["changed_files"][0]["path"], "src/lib.rs");
        assert_eq!(lifecycle[0]["changed_files"][0]["source"], "Write");
    }

    #[test]
    fn stream_json_partial_event_exposes_tool_result_error_lifecycle_runtime_events() {
        let event = stream_json_runner_event(
            crate::runner::RunnerStreamEvent::ToolResult {
                id: "toolu_mcp".to_string(),
                name: "MCP".to_string(),
                is_error: true,
                content: "denied by fake server".to_string(),
                changed_files: None,
                error: Some(serde_json::json!({
                    "type": "tool_error",
                    "code": "tool_validation_error",
                    "message": "server is required",
                    "repair_hint": "Provide the required input fields for MCP and retry the tool call."
                })),
            },
            "session-1",
        )
        .unwrap();

        assert_eq!(event["type"], "stream_event");
        assert_eq!(event["event"]["type"], "tool_result");
        assert_eq!(event["event"]["tool_use_id"], "toolu_mcp");
        assert_eq!(event["event"]["name"], "MCP");
        assert_eq!(event["event"]["is_error"], true);
        assert_eq!(event["event"]["content"], "denied by fake server");
        assert_eq!(event["event"]["error"]["code"], "tool_validation_error");
        assert_eq!(
            event["event"]["error"]["repair_hint"],
            "Provide the required input fields for MCP and retry the tool call."
        );
        let lifecycle = event["runtime_events"].as_array().unwrap();
        assert_eq!(lifecycle.len(), 1);
        assert_eq!(lifecycle[0]["type"], "tool_result");
        assert_eq!(lifecycle[0]["tool_call_id"], "toolu_mcp");
        assert_eq!(lifecycle[0]["name"], "MCP");
        assert_eq!(lifecycle[0]["workbench"], "mcp");
        assert_eq!(lifecycle[0]["is_error"], true);
        assert_eq!(lifecycle[0]["content"], "denied by fake server");
        assert_eq!(lifecycle[0]["error"]["code"], "tool_validation_error");
        assert_eq!(
            lifecycle[0]["error"]["repair_hint"],
            "Provide the required input fields for MCP and retry the tool call."
        );
    }

    #[tokio::test]
    async fn format_print_result_stream_json_handles_record_only_result() {
        let result = serde_json::json!({
            "type": "sdk_prompt_recorded",
            "session_id": "session-1",
            "execution": "record_only",
        });

        let formatted =
            format_print_result_with_duration(&result, &PrintOutputFormat::StreamJson, 0, &[])
                .await
                .unwrap();
        let events = formatted
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["type"], "system");
        assert_eq!(events[1]["type"], "result");
        assert_eq!(events[1]["result"], "");
        assert_eq!(events[1]["num_turns"], 0);
    }

    #[tokio::test]
    async fn format_stream_json_replays_user_messages() {
        let result = serde_json::json!({
            "type": "sdk_prompt_recorded",
            "session_id": "session-1",
            "execution": "record_only",
        });
        let replay = vec![
            serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": "hello from stdin"
                },
                "parent_tool_use_id": null
            }),
            serde_json::json!({
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [{
                        "type": "text",
                        "text": "previous answer"
                    }]
                },
                "parent_tool_use_id": null
            }),
            serde_json::json!({
                "type": "control_response",
                "response": {
                    "request_id": "perm-1",
                    "subtype": "success"
                }
            }),
        ];

        let formatted =
            format_print_result_with_duration(&result, &PrintOutputFormat::StreamJson, 0, &replay)
                .await
                .unwrap();
        let events = formatted
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 5);
        assert_eq!(events[1]["type"], "user");
        assert_eq!(events[1]["session_id"], "session-1");
        assert_eq!(events[1]["message"]["content"], "hello from stdin");
        assert!(events[1]["uuid"]
            .as_str()
            .is_some_and(|value| !value.is_empty()));
        assert_eq!(events[2]["type"], "assistant");
        assert_eq!(events[2]["session_id"], "session-1");
        assert_eq!(
            events[2]["message"]["content"][0]["text"],
            "previous answer"
        );
        assert_eq!(events[3]["type"], "control_response");
        assert_eq!(events[3]["session_id"], "session-1");
        assert_eq!(events[4]["type"], "result");
    }

    #[test]
    fn reply_args_default_to_model_execution() {
        let args = vec![
            "reply".to_string(),
            "session-1".to_string(),
            "hello".to_string(),
            "there".to_string(),
        ];

        assert_eq!(
            parse_reply_args(&args).unwrap(),
            ReplyArgs {
                session_id: "session-1".to_string(),
                message: "hello there".to_string(),
                execute: true,
                json_schema: None,
            }
        );
    }

    #[tokio::test]
    async fn prompt_local_commands_allow_help_in_non_interactive_cli() {
        for name in ["commit", "init"] {
            let result = run_local_command(
                &[name.to_string(), "--help".to_string()],
                &RuntimeFlags::default(),
            )
            .await
            .unwrap()
            .expect("local command result");
            assert!(
                result.value.contains(&format!("Usage: kiana {name}")),
                "{name} help output was:\n{}",
                result.value
            );
        }
    }

    #[tokio::test]
    async fn plugin_prompt_commands_run_from_non_interactive_cli() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["KIANA_PLUGINS_DIR"]);
        let root = std::env::temp_dir().join(format!(
            "kiana-cli-plugin-command-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        std::fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(plugin_root.join("commands")).unwrap();
        std::fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            serde_json::json!({ "name": "review-tools" }).to_string(),
        )
        .unwrap();
        std::fs::write(
            plugin_root.join("commands").join("audit.md"),
            "---\ndescription: Audit command\narguments: target\n---\nAudit $target from ${KIANA_PLUGIN_ROOT}",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let result = run_local_command(
            &["review-tools:audit".to_string(), "src/lib.rs".to_string()],
            &RuntimeFlags::default(),
        )
        .await
        .unwrap()
        .expect("plugin prompt command result");

        assert!(result.value.contains("Audit src/lib.rs"));
        assert!(result
            .value
            .contains(&plugin_root.to_string_lossy().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn cli_model_list_json_outputs_provider_capabilities() {
        let result = run_local_command(
            &[
                "model".to_string(),
                "list".to_string(),
                "--json".to_string(),
            ],
            &RuntimeFlags::default(),
        )
        .await
        .unwrap()
        .expect("model list result");
        let profiles: Value = serde_json::from_str(&result.value).unwrap();

        assert!(profiles.as_array().unwrap().iter().any(|profile| {
            profile["provider_id"].as_str() == Some("fake")
                && profile["model_id"].as_str() == Some("fake-model")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
        }));
        assert!(profiles.as_array().unwrap().iter().any(|profile| {
            profile["provider_id"].as_str() == Some("openai-compatible")
                && profile["model_id"].as_str() == Some("gpt-4.1")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
                && profile["streaming_mode"].as_str() == Some("synthetic")
                && profile["native_streaming"].as_bool() == Some(false)
        }));
        assert!(profiles.as_array().unwrap().iter().any(|profile| {
            profile["provider_id"].as_str() == Some("ollama")
                && profile["model_id"].as_str() == Some("llama3.1")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
        }));
    }

    #[tokio::test]
    async fn cli_model_catalog_json_reports_default_offline_catalog() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "KIANA_MODEL_CATALOG_LIVE",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_BASE_URL",
            "OPENAI_BASE_URL",
            "KIANA_OLLAMA_BASE_URL",
            "OLLAMA_BASE_URL",
        ]);
        for key in [
            "KIANA_MODEL_CATALOG_LIVE",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_BASE_URL",
            "OPENAI_BASE_URL",
            "KIANA_OLLAMA_BASE_URL",
            "OLLAMA_BASE_URL",
        ] {
            std::env::remove_var(key);
        }

        let result = run_local_command(
            &[
                "model".to_string(),
                "catalog".to_string(),
                "--json".to_string(),
            ],
            &RuntimeFlags::default(),
        )
        .await
        .unwrap()
        .expect("model catalog result");
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.model-catalog.v1");
        assert_eq!(report["live"], false);
        assert_eq!(report["summary"]["failed"], 0);
        assert!(report["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| {
                provider["provider_id"].as_str() == Some("openai-compatible")
                    && provider["status"].as_str() == Some("skipped")
            }));
        assert!(report["providers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|provider| {
                provider["provider_id"].as_str() == Some("fake")
                    && provider["status"].as_str() == Some("static")
            }));
    }

    #[tokio::test]
    async fn cli_model_smoke_json_reports_fake_pass() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "KIANA_PROVIDER_SMOKE_LIVE",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
        ]);
        for key in [
            "KIANA_PROVIDER_SMOKE_LIVE",
            "ANTHROPIC_API_KEY",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
        ] {
            std::env::remove_var(key);
        }

        let result = run_local_command(
            &[
                "model".to_string(),
                "smoke".to_string(),
                "--json".to_string(),
            ],
            &RuntimeFlags::default(),
        )
        .await
        .unwrap()
        .expect("model smoke result");
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.model-smoke.v1");
        assert_eq!(report["summary"]["passed"], 1);
        assert!(report["results"].as_array().unwrap().iter().any(|result| {
            result["provider_id"].as_str() == Some("fake")
                && result["status"].as_str() == Some("passed")
        }));
    }

    #[tokio::test]
    async fn cli_license_status_json_reports_redacted_env_license() {
        let _guard = env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "KIANA_LICENSE_FILE",
            "KIANA_LICENSE_KEY",
            "KIANA_LICENSE_PLAN",
            "KIANA_LICENSE_ENTITLEMENTS",
            "KIANA_ENTERPRISE_ACCOUNT_ID",
            "KIANA_SUPPORT_CONTACT",
            "KIANA_MANAGED_POLICY_FILE",
            "KIANA_MANAGED_SETTINGS_FILE",
        ]);
        for key in [
            "KIANA_LICENSE_FILE",
            "KIANA_MANAGED_POLICY_FILE",
            "KIANA_MANAGED_SETTINGS_FILE",
        ] {
            std::env::remove_var(key);
        }
        std::env::set_var("KIANA_LICENSE_KEY", "cli-license-secret-4444");
        std::env::set_var("KIANA_LICENSE_PLAN", "enterprise");
        std::env::set_var("KIANA_LICENSE_ENTITLEMENTS", "audit,policy");
        std::env::set_var("KIANA_ENTERPRISE_ACCOUNT_ID", "acct_cli");
        std::env::set_var("KIANA_SUPPORT_CONTACT", "support@example.test");

        let result = run_local_command(
            &[
                "license".to_string(),
                "status".to_string(),
                "--json".to_string(),
            ],
            &RuntimeFlags::default(),
        )
        .await
        .unwrap()
        .expect("license status result");
        let report: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.license-status.v1");
        assert_eq!(report["status"], "configured");
        assert_eq!(report["source"], "KIANA_LICENSE_KEY");
        assert_eq!(report["license_key"], "set");
        assert_eq!(report["license_key_preview"], "redacted-4444");
        assert_eq!(report["account_id"], "acct_cli");
        assert_eq!(report["plan"], "enterprise");
        assert_eq!(report["support_contact"], "support@example.test");
        assert!(!result.value.contains("cli-license-secret"));
    }

    #[test]
    fn reply_args_allow_record_only_mode() {
        let args = vec![
            "reply".to_string(),
            "session-1".to_string(),
            "--record-only".to_string(),
            "store".to_string(),
            "this".to_string(),
        ];

        assert_eq!(
            parse_reply_args(&args).unwrap(),
            ReplyArgs {
                session_id: "session-1".to_string(),
                message: "store this".to_string(),
                execute: false,
                json_schema: None,
            }
        );
    }

    #[test]
    fn reply_args_accept_json_schema_before_message() {
        let args = vec![
            "reply".to_string(),
            "session-1".to_string(),
            "--json-schema".to_string(),
            r#"{"type":"object","required":["answer"]}"#.to_string(),
            "answer".to_string(),
            "as".to_string(),
            "json".to_string(),
        ];

        let parsed = parse_reply_args(&args).unwrap();

        assert_eq!(parsed.session_id, "session-1");
        assert_eq!(parsed.message, "answer as json");
        assert_eq!(parsed.json_schema.unwrap()["required"][0], "answer");
    }

    #[test]
    fn reply_args_accept_json_schema_equals_form_before_session() {
        let args = vec![
            "reply".to_string(),
            r#"--json-schema={"type":"object","properties":{"score":{"type":"integer"}}}"#
                .to_string(),
            "session-1".to_string(),
            "score".to_string(),
            "this".to_string(),
        ];

        let parsed = parse_reply_args(&args).unwrap();

        assert_eq!(parsed.session_id, "session-1");
        assert_eq!(parsed.message, "score this");
        assert_eq!(
            parsed.json_schema.unwrap()["properties"]["score"]["type"],
            "integer"
        );
    }

    #[test]
    fn reply_args_keep_flag_like_words_after_message_starts() {
        let args = vec![
            "reply".to_string(),
            "--record-only".to_string(),
            "session-1".to_string(),
            "explain".to_string(),
            "--record-only".to_string(),
        ];

        assert_eq!(
            parse_reply_args(&args).unwrap(),
            ReplyArgs {
                session_id: "session-1".to_string(),
                message: "explain --record-only".to_string(),
                execute: false,
                json_schema: None,
            }
        );
    }

    #[test]
    fn reply_args_keep_json_schema_like_words_after_message_starts() {
        let args = vec![
            "reply".to_string(),
            "session-1".to_string(),
            "explain".to_string(),
            "--json-schema".to_string(),
            "{}".to_string(),
        ];

        assert_eq!(
            parse_reply_args(&args).unwrap(),
            ReplyArgs {
                session_id: "session-1".to_string(),
                message: "explain --json-schema {}".to_string(),
                execute: true,
                json_schema: None,
            }
        );
    }

    #[test]
    fn reply_args_reject_invalid_json_schema() {
        let args = vec![
            "reply".to_string(),
            "session-1".to_string(),
            "--json-schema".to_string(),
            "not-json".to_string(),
            "message".to_string(),
        ];

        let error = parse_reply_args(&args).unwrap_err().to_string();
        assert!(error.contains("--json-schema must be valid JSON"));
    }

    #[test]
    fn daemon_prompt_uses_words_after_enqueue_command() {
        let args = vec![
            "daemon".to_string(),
            "enqueue".to_string(),
            "inspect".to_string(),
            "workspace".to_string(),
        ];

        assert_eq!(daemon_prompt(&args), "inspect workspace");
    }

    #[test]
    fn daemon_prompt_preserves_flag_like_words_after_separator() {
        let args = vec![
            "daemon".to_string(),
            "enqueue".to_string(),
            "--".to_string(),
            "explain".to_string(),
            "--record-only".to_string(),
        ];

        assert_eq!(daemon_prompt(&args), "explain --record-only");
    }

    #[test]
    fn main_url_handler_command_points_back_to_kiana_url_handle() {
        assert_eq!(
            main_kiana_url_handler_command("/tmp/Kiana Code/kiana"),
            "\"/tmp/Kiana Code/kiana\" url handle %u"
        );
    }

    #[test]
    fn handled_url_json_exposes_deep_link_parts() {
        let parsed = url::Url::parse("kiana://resume/session-1?cwd=/tmp/work").unwrap();
        let value = handled_url_json(&parsed);

        assert_eq!(value["handled"], true);
        assert_eq!(value["scheme"], "kiana");
        assert_eq!(value["action"], "resume");
        assert_eq!(value["path"], "/session-1");
        assert_eq!(value["query"], "cwd=/tmp/work");
    }
}
