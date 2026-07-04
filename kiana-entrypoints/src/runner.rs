use anyhow::{anyhow, Result};
use futures::StreamExt;
use kiana_services::api::{
    errors::{ApiError, ApiErrorKind},
    messages::{Message, MessagesRequest},
    provider::{
        provider_registry_entry, AnthropicProvider, FakeProvider, OllamaProvider,
        OpenAiCompatibleProvider, Provider, ProviderError, ProviderProtocol, ProviderStream,
        ANTHROPIC_PROVIDER_ID,
    },
    streaming::{ContentBlock as StreamContentBlock, Delta, StreamEvent},
};
use kiana_services::compact::{compact_context_report, CompactionConfig, CompactionReport};
use kiana_tools::{
    create_default_registry,
    mcp_tool::{MCP_SERVERS_APP_STATE_KEY, MCP_SERVERS_ENV},
    task_create::{TaskListTool, TaskUpdateTool},
    tool::{
        ACCESS_ROOTS_APP_STATE_KEY, EDITABLE_FILES_APP_STATE_KEY, READ_ONLY_FILES_APP_STATE_KEY,
    },
    tool_execution::{
        execute_tool_calls_with_permission_handler, PermissionPromptHandler, ToolCallRequest,
        ToolExecutionResult, PERMISSION_PROMPT_TOOL_APP_STATE_KEY, PERMISSION_PROMPT_TOOL_ENV,
    },
    Tool, ToolContext,
};
use kiana_types::{
    RuntimeErrorEvent, RuntimeEvent, RuntimeEventPayload, RuntimePermissionRequestEvent,
    RuntimeResultEvent, RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_MAX_TOKENS: u32 = 4096;
const THINKING_RESPONSE_TOKEN_RESERVE: u64 = 1024;

#[derive(Debug, Clone)]
pub struct AssistantRunResult {
    pub text: String,
    pub messages: Vec<Message>,
    pub iterations: usize,
    pub stop_reason: String,
    pub structured_output: Option<Value>,
    pub teammate_shutdown_approved: bool,
}

#[derive(Debug, Clone)]
pub enum RunnerStreamEvent {
    Model(StreamEvent),
    ToolResult {
        id: String,
        name: String,
        is_error: bool,
        content: String,
        error: Option<Value>,
    },
}

pub fn runtime_events_from_runner_stream_event(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    start_sequence: u64,
    timestamp: &str,
    event: RunnerStreamEvent,
) -> Vec<RuntimeEvent> {
    match event {
        RunnerStreamEvent::Model(event) => runtime_events_from_model_stream_event(
            session_id,
            turn_id,
            parent_turn_id,
            start_sequence,
            timestamp,
            event,
        ),
        RunnerStreamEvent::ToolResult {
            id,
            name,
            is_error,
            content,
            error,
        } => {
            let workbench = default_tool_workbench(&name);
            vec![runner_runtime_event(
                session_id,
                turn_id,
                parent_turn_id,
                start_sequence,
                timestamp,
                RuntimeEventPayload::ToolResult(RuntimeToolResultEvent {
                    tool_call_id: id,
                    name: Some(name),
                    workbench,
                    is_error,
                    content: json!(content),
                    error,
                }),
            )]
        }
    }
}

pub fn runtime_event_from_permission_prompt_request(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    request: &kiana_tools::tool_execution::PermissionPromptRequest,
) -> RuntimeEvent {
    runner_runtime_event(
        session_id,
        turn_id,
        parent_turn_id,
        sequence,
        timestamp,
        RuntimeEventPayload::PermissionRequest(RuntimePermissionRequestEvent {
            request_id: request.request_id.clone(),
            tool_name: request.tool_name.clone(),
            action: "can_use_tool".to_string(),
            input: request.input.clone(),
            reason: permission_prompt_reason(request),
        }),
    )
}

pub fn runtime_event_from_assistant_run_result(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    result: &AssistantRunResult,
) -> RuntimeEvent {
    runner_runtime_event(
        session_id,
        turn_id,
        parent_turn_id,
        sequence,
        timestamp,
        RuntimeEventPayload::Result(RuntimeResultEvent {
            status: "completed".to_string(),
            stop_reason: result.stop_reason.clone(),
            assistant_text: Some(result.text.clone()),
            metadata: json!({
                "iterations": result.iterations,
                "message_count": result.messages.len(),
                "structured_output": result.structured_output,
                "teammate_shutdown_approved": result.teammate_shutdown_approved
            }),
        }),
    )
}

fn normalized_model_stop_reason(stop_reason: Option<&str>) -> String {
    match stop_reason {
        Some("end_turn") | Some("stop") => "model_stop".to_string(),
        Some(reason) if !reason.trim().is_empty() => reason.to_string(),
        _ => "model_stop".to_string(),
    }
}

fn runtime_events_from_model_stream_event(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    event: StreamEvent,
) -> Vec<RuntimeEvent> {
    let event_value = serde_json::to_value(&event).unwrap_or_else(|_| json!({}));
    match event {
        StreamEvent::ContentBlockStart {
            content_block: StreamContentBlock::ToolUse(tool_use),
            ..
        } => {
            let workbench = default_tool_workbench(&tool_use.name);
            vec![runner_runtime_event(
                session_id,
                turn_id,
                parent_turn_id,
                sequence,
                timestamp,
                RuntimeEventPayload::ToolCall(RuntimeToolCallEvent {
                    tool_call_id: tool_use.id,
                    name: tool_use.name,
                    workbench,
                    input: tool_use.input,
                }),
            )]
        }
        StreamEvent::Error { error } => vec![runner_runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            sequence,
            timestamp,
            RuntimeEventPayload::Error(RuntimeErrorEvent {
                code: error
                    .get("type")
                    .or_else(|| error.get("code"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                message: stream_error_message(&error),
                details: error,
            }),
        )],
        _ => vec![runner_runtime_event(
            session_id,
            turn_id,
            parent_turn_id,
            sequence,
            timestamp,
            RuntimeEventPayload::StreamDelta(RuntimeStreamDeltaEvent { delta: event_value }),
        )],
    }
}

pub(crate) fn default_tool_workbench(name: &str) -> Option<String> {
    create_default_registry()
        .get(name)
        .and_then(|tool| tool.workbench())
        .map(str::to_string)
}

fn runner_runtime_event(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    payload: RuntimeEventPayload,
) -> RuntimeEvent {
    RuntimeEvent::new(
        format!("{session_id}:{turn_id}:{sequence}"),
        session_id,
        turn_id,
        parent_turn_id,
        sequence,
        timestamp,
        payload,
    )
}

fn permission_prompt_reason(
    request: &kiana_tools::tool_execution::PermissionPromptRequest,
) -> Option<String> {
    let mut parts = Vec::new();
    if !request.decision_reason.is_null() {
        parts.push(format!("decision_reason={}", request.decision_reason));
    }
    if let Some(blocked_path) = &request.blocked_path {
        parts.push(format!("blocked_path={blocked_path}"));
    }
    if let Some(agent_id) = &request.agent_id {
        parts.push(format!("agent_id={agent_id}"));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

fn stream_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string())
}

#[derive(Debug, Clone)]
pub struct ResidentTeammateLoopConfig {
    pub poll_interval: Duration,
    pub max_idle_polls: Option<usize>,
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentTeammateLoopResult {
    pub session_id: String,
    pub turns: usize,
    pub idle_polls: usize,
    pub stop_reason: String,
}

impl ResidentTeammateLoopConfig {
    pub fn from_env() -> Self {
        Self {
            poll_interval: Duration::from_millis(env_u64("KIANA_RESIDENT_TEAMMATE_POLL_MS", 1000)),
            max_idle_polls: env_usize("KIANA_RESIDENT_TEAMMATE_MAX_IDLE_POLLS"),
            max_turns: env_usize("KIANA_RESIDENT_TEAMMATE_MAX_TURNS"),
        }
    }
}

pub async fn run_resident_teammate_loop_with<F, Fut>(
    initial_prompt: String,
    mut options: HashMap<String, Value>,
    config: ResidentTeammateLoopConfig,
    mut run_prompt: F,
) -> Result<ResidentTeammateLoopResult>
where
    F: FnMut(String, HashMap<String, Value>) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let session_id =
        string_option(&options, "session_id").unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    options.insert("session_id".to_string(), json!(session_id));
    options.insert("create_session_if_missing".to_string(), Value::Bool(true));
    options.insert("execute".to_string(), Value::Bool(true));

    let _ = sync_current_member_lifecycle_status("running", Some(true));
    let mut first_options = options.clone();
    first_options.insert(
        "skip_initial_mailbox_prompt".to_string(),
        json!(initial_prompt.clone()),
    );
    let result = run_prompt(initial_prompt, first_options).await?;
    let session_id = result
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|session_id| !session_id.trim().is_empty())
        .unwrap_or(&session_id)
        .to_string();
    options.insert("session_id".to_string(), json!(session_id));

    let mut turns = 1;
    let mut idle_polls = 0;
    if result_bool(&result, "teammate_shutdown_approved") {
        let _ = sync_current_member_lifecycle_status("shutdown_approved", Some(false));
        return Ok(ResidentTeammateLoopResult {
            session_id,
            turns,
            idle_polls,
            stop_reason: "shutdown_approved".to_string(),
        });
    }
    loop {
        if config.max_turns.is_some_and(|max_turns| turns >= max_turns) {
            let _ = sync_current_member_lifecycle_status("max_turns", Some(false));
            return Ok(ResidentTeammateLoopResult {
                session_id,
                turns,
                idle_polls,
                stop_reason: "max_turns".to_string(),
            });
        }

        let _ = sync_current_member_lifecycle_status("idle", Some(true));
        if teammate_mailbox_has_unread_messages()? {
            idle_polls = 0;
            let _ = sync_current_member_lifecycle_status("running", Some(true));
            let result = run_prompt(
                "Continue from teammate mailbox.".to_string(),
                options.clone(),
            )
            .await?;
            turns += 1;
            if result_bool(&result, "teammate_shutdown_approved") {
                let _ = sync_current_member_lifecycle_status("shutdown_approved", Some(false));
                return Ok(ResidentTeammateLoopResult {
                    session_id,
                    turns,
                    idle_polls,
                    stop_reason: "shutdown_approved".to_string(),
                });
            }
            continue;
        }

        if let Some(task_prompt) = try_claim_next_team_task_prompt().await? {
            idle_polls = 0;
            let _ = sync_current_member_lifecycle_status("running", Some(true));
            let result = run_prompt(task_prompt, options.clone()).await?;
            turns += 1;
            if result_bool(&result, "teammate_shutdown_approved") {
                let _ = sync_current_member_lifecycle_status("shutdown_approved", Some(false));
                return Ok(ResidentTeammateLoopResult {
                    session_id,
                    turns,
                    idle_polls,
                    stop_reason: "shutdown_approved".to_string(),
                });
            }
            continue;
        }

        idle_polls += 1;
        if config
            .max_idle_polls
            .is_some_and(|max_idle_polls| idle_polls >= max_idle_polls)
        {
            let _ = sync_current_member_lifecycle_status("idle_limit", Some(false));
            return Ok(ResidentTeammateLoopResult {
                session_id,
                turns,
                idle_polls,
                stop_reason: "idle_limit".to_string(),
            });
        }
        tokio::time::sleep(config.poll_interval).await;
    }
}

pub async fn run_assistant_turn(
    messages: Vec<Value>,
    options: &HashMap<String, Value>,
) -> Result<AssistantRunResult> {
    run_assistant_turn_with_permission_handler(messages, options, None).await
}

pub async fn run_assistant_turn_with_permission_handler(
    messages: Vec<Value>,
    options: &HashMap<String, Value>,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> Result<AssistantRunResult> {
    let config = kiana_bootstrap::config::load_config();
    let provider_id = provider_id_option(options);
    let model = model_option(options, &config, &provider_id);
    let fallback_model = fallback_model_option(options);
    if fallback_model.as_deref() == Some(model.as_str()) {
        return Err(anyhow!(
            "Fallback model cannot be the same as the main model"
        ));
    }
    let api_timeout = api_timeout_option(options, config.api_timeout_ms)?;
    let thinking = thinking_option(options)?;
    let max_tokens = max_tokens_option(options, thinking.as_ref())?;
    let max_iterations = max_iterations_option(options);
    let repair_checks = repair_checks_option(options);
    let max_repair_attempts = repair_check_attempts_option(options)?;
    let compaction_config = compaction_config_option(options)?;
    let provider = build_provider(&provider_id, &model, options, &config, api_timeout)?;

    let registry = create_default_registry();
    let enabled_tools = provider_default_tool_filter(
        tool_filter_option(options, &registry)?,
        options,
        provider.as_ref(),
        &model,
    );
    let tool_schemas = filtered_tool_schemas(&registry, enabled_tools.as_ref());
    let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
    let cwd = string_option(options, "cwd").unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    let assistant_checkpoint = create_assistant_turn_checkpoint_if_requested(options, &cwd);
    let mut app_state = HashMap::new();
    if let Some(access_roots) = access_roots_option(options) {
        app_state.insert(ACCESS_ROOTS_APP_STATE_KEY.to_string(), json!(access_roots));
    }
    seed_file_set_options(&mut app_state, options);
    if let Some(mcp_servers) = mcp_servers_option(options)? {
        app_state.insert(MCP_SERVERS_APP_STATE_KEY.to_string(), mcp_servers);
    }
    if let Some(sandbox) = sandbox_option(options, config.sandbox.clone())? {
        app_state.insert("sandbox".to_string(), sandbox);
    }
    if let Some(permission_mode) = permission_mode_option(options)? {
        apply_permission_mode(&mut app_state, &permission_mode);
    }
    apply_permission_rule_options(&mut app_state, options);
    if let Some(permission_prompt_tool) = permission_prompt_tool_option(options) {
        app_state.insert(
            PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
            json!(permission_prompt_tool),
        );
    }
    seed_team_app_state_from_env(&mut app_state);
    seed_mailbox_options_from_run_options(&mut app_state, options);

    let mut tool_context = ToolContext {
        cwd,
        read_file_state: HashMap::new(),
        app_state,
        abort_signal: abort_rx,
    };
    if let Some(schema) = structured_output_schema_option(options)? {
        tool_context
            .app_state
            .insert("structured_output_schema".to_string(), schema);
    }
    let mut messages = normalize_messages(messages)?;
    if let Some(mailbox_context) = consume_initial_mailbox_messages(&mut tool_context.app_state)? {
        append_mailbox_context_to_messages(&mut messages, mailbox_context);
    }
    let session_start_hook_contexts = session_start_contexts(&tool_context, "runner").await?;
    let user_prompt_submit_result =
        user_prompt_submit_hook_result(&tool_context, "runner", &messages).await?;
    apply_user_prompt_submit_update(&mut messages, &user_prompt_submit_result);
    let system_prompt = append_session_start_contexts_to_system_prompt(
        system_prompt_option(options),
        session_start_hook_contexts,
    );
    let system_prompt = append_user_prompt_submit_contexts_to_system_prompt(
        system_prompt,
        user_prompt_submit_result.add_contexts,
    );
    let mut final_text = String::new();
    let mut final_structured_output = None;
    let mut active_model = model.clone();
    let mut fallback_used = false;
    let mut repair_attempts = 0_u32;

    for iteration in 1..=max_iterations {
        messages = compact_messages_for_request(messages, &compaction_config).await?;
        let request = MessagesRequest {
            model: active_model.clone(),
            messages: messages.clone(),
            max_tokens,
            system: system_prompt.clone(),
            temperature: None,
            tools: if tool_schemas.is_empty() {
                None
            } else {
                Some(tool_schemas.clone())
            },
            thinking: thinking.clone(),
            stream: None,
        };
        let response = match provider
            .create_message(request)
            .await
            .map_err(anyhow::Error::from)
        {
            Ok(response) => response,
            Err(error)
                if should_retry_with_fallback(&error, fallback_model.as_deref(), fallback_used) =>
            {
                active_model = fallback_model.clone().expect("fallback checked above");
                fallback_used = true;
                let request = MessagesRequest {
                    model: active_model.clone(),
                    messages: messages.clone(),
                    max_tokens,
                    system: system_prompt.clone(),
                    temperature: None,
                    tools: if tool_schemas.is_empty() {
                        None
                    } else {
                        Some(tool_schemas.clone())
                    },
                    thinking: thinking.clone(),
                    stream: None,
                };
                provider
                    .create_message(request)
                    .await
                    .map_err(anyhow::Error::from)?
            }
            Err(error) => return Err(error),
        };
        let response_text = text_from_content(&response.content);
        let tool_uses = tool_uses_from_content(&response.content);

        messages.push(Message {
            role: "assistant".to_string(),
            content: json!(response.content),
        });

        if tool_uses.is_empty() {
            final_text = response_text;
            if repair_checks && repair_attempts < max_repair_attempts {
                if let Some(feedback) = repair_feedback_if_checks_failed(
                    &tool_context.cwd,
                    repair_attempts + 1,
                    max_repair_attempts,
                )
                .await?
                {
                    repair_attempts += 1;
                    messages.push(Message {
                        role: "user".to_string(),
                        content: json!(feedback),
                    });
                    continue;
                }
            }
            record_assistant_turn_final_state_if_requested(
                assistant_checkpoint.as_deref(),
                &tool_context.cwd,
            );
            return Ok(AssistantRunResult {
                text: final_text,
                messages,
                iterations: iteration,
                stop_reason: normalized_model_stop_reason(response.stop_reason.as_deref()),
                structured_output: final_structured_output,
                teammate_shutdown_approved: app_state_bool(
                    &tool_context.app_state,
                    "teammate_shutdown_approved",
                ),
            });
        }

        let tool_call_requests = tool_uses
            .iter()
            .map(|tool_use| ToolCallRequest {
                name: tool_use.name.clone(),
                input: tool_use.input.clone(),
                tool_use_id: Some(tool_use.id.clone()),
            })
            .collect::<Vec<_>>();
        let batch_results = execute_tool_calls_with_permission_handler(
            &registry,
            enabled_tools.as_ref(),
            &mut tool_context,
            &tool_call_requests,
            permission_handler,
        )
        .await;
        let mut tool_results = Vec::new();
        for result in batch_results {
            if let Some(structured_output) = &result.structured_output {
                final_structured_output = Some(structured_output.clone());
            }
            tool_results.push(result.api_result);
        }

        messages.push(Message {
            role: "user".to_string(),
            content: json!(tool_results),
        });
        final_text = response_text;
        if final_structured_output.is_some() {
            record_assistant_turn_final_state_if_requested(
                assistant_checkpoint.as_deref(),
                &tool_context.cwd,
            );
            return Ok(AssistantRunResult {
                text: final_text,
                messages,
                iterations: iteration,
                stop_reason: "structured_output".to_string(),
                structured_output: final_structured_output,
                teammate_shutdown_approved: app_state_bool(
                    &tool_context.app_state,
                    "teammate_shutdown_approved",
                ),
            });
        }
    }

    Err(anyhow!(
        "assistant turn exceeded max_iterations={} after partial output: {}",
        max_iterations,
        final_text
    ))
}

async fn repair_feedback_if_checks_failed(
    cwd: &str,
    repair_attempt: u32,
    max_repair_attempts: u32,
) -> Result<Option<String>> {
    let review = kiana_commands::review::ReviewCommand;
    let report = kiana_commands::Command::execute(
        &review,
        kiana_commands::CommandContext {
            args: "--json".to_string(),
            app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
        },
    )
    .await?;
    let report: Value = serde_json::from_str(&report.value)
        .map_err(|error| anyhow!("failed to parse review report JSON: {error}"))?;
    let failed = report
        .pointer("/checks/summary/failed")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let skipped = report
        .pointer("/checks/summary/skipped")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if failed == 0 && skipped == 0 {
        return Ok(None);
    }
    Ok(Some(format_repair_feedback(
        &report,
        repair_attempt,
        max_repair_attempts,
    )))
}

fn format_repair_feedback(report: &Value, repair_attempt: u32, max_repair_attempts: u32) -> String {
    let summary = report.pointer("/checks/summary").unwrap_or(&Value::Null);
    let failed = summary.get("failed").and_then(Value::as_u64).unwrap_or(0);
    let skipped = summary.get("skipped").and_then(Value::as_u64).unwrap_or(0);
    let attention_checks = report
        .pointer("/checks/results")
        .and_then(Value::as_array)
        .map(|results| {
            results
                .iter()
                .filter(|result| {
                    matches!(
                        result.get("status").and_then(Value::as_str),
                        Some("failed" | "skipped")
                    )
                })
                .filter_map(|result| result.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let failed_checks = if attention_checks.is_empty() {
        "none".to_string()
    } else {
        attention_checks.join(",")
    };
    let mut lines = vec![
        "Repair checks failed.".to_string(),
        "Fix the reported failures, then finish the turn.".to_string(),
        format!("repair_attempt: {repair_attempt}/{max_repair_attempts}"),
        "final_status: retrying".to_string(),
        format!("check_summary: failed={failed} skipped={skipped}"),
        format!("failed_checks: {failed_checks}"),
        format!("summary: {}", compact_json(summary)),
    ];
    if let Some(results) = report.pointer("/checks/results").and_then(Value::as_array) {
        for result in results.iter().filter(|result| {
            matches!(
                result.get("status").and_then(Value::as_str),
                Some("failed" | "skipped")
            )
        }) {
            let id = result
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let status = result
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let command = result.get("command").and_then(Value::as_str).unwrap_or("");
            let exit_code = result
                .get("exit_code")
                .map(compact_json)
                .unwrap_or_else(|| "null".to_string());
            lines.push(format!(
                "- {id}: status={status} exit_code={exit_code} command={command}"
            ));
            if let Some(error) = result.get("error").and_then(Value::as_str) {
                if !error.trim().is_empty() {
                    lines.push(format!("  error: {}", truncate_for_repair_feedback(error)));
                }
            }
            if let Some(stderr) = result.get("stderr").and_then(Value::as_str) {
                if !stderr.trim().is_empty() {
                    lines.push(format!(
                        "  stderr: {}",
                        truncate_for_repair_feedback(stderr)
                    ));
                }
            }
            if let Some(stdout) = result.get("stdout").and_then(Value::as_str) {
                if !stdout.trim().is_empty() {
                    lines.push(format!(
                        "  stdout: {}",
                        truncate_for_repair_feedback(stdout)
                    ));
                }
            }
        }
    }
    lines.join("\n")
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn truncate_for_repair_feedback(value: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= MAX_CHARS {
            output.push_str("...<truncated>");
            return output;
        }
        output.push(ch);
    }
    output
}

pub async fn run_assistant_turn_streaming<F>(
    messages: Vec<Value>,
    options: &HashMap<String, Value>,
    on_stream_event: F,
) -> Result<AssistantRunResult>
where
    F: FnMut(StreamEvent) -> Result<()>,
{
    run_assistant_turn_streaming_with_permission_handler(messages, options, on_stream_event, None)
        .await
}

pub async fn run_assistant_turn_streaming_with_permission_handler<F>(
    messages: Vec<Value>,
    options: &HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> Result<AssistantRunResult>
where
    F: FnMut(StreamEvent) -> Result<()>,
{
    let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
    run_assistant_turn_streaming_with_permission_handler_and_abort_signal(
        messages,
        options,
        on_stream_event,
        permission_handler,
        abort_signal,
    )
    .await
}

pub async fn run_assistant_turn_streaming_with_permission_handler_and_abort_signal<F>(
    messages: Vec<Value>,
    options: &HashMap<String, Value>,
    mut on_stream_event: F,
    permission_handler: Option<&dyn PermissionPromptHandler>,
    abort_signal: tokio::sync::watch::Receiver<bool>,
) -> Result<AssistantRunResult>
where
    F: FnMut(StreamEvent) -> Result<()>,
{
    run_assistant_turn_streaming_with_runner_events_and_abort_signal(
        messages,
        options,
        move |event| {
            if let RunnerStreamEvent::Model(event) = event {
                on_stream_event(event)?;
            }
            Ok(())
        },
        permission_handler,
        abort_signal,
    )
    .await
}

pub async fn run_assistant_turn_streaming_with_runner_events_and_abort_signal<F>(
    messages: Vec<Value>,
    options: &HashMap<String, Value>,
    mut on_runner_event: F,
    permission_handler: Option<&dyn PermissionPromptHandler>,
    abort_signal: tokio::sync::watch::Receiver<bool>,
) -> Result<AssistantRunResult>
where
    F: FnMut(RunnerStreamEvent) -> Result<()>,
{
    let config = kiana_bootstrap::config::load_config();
    let provider_id = provider_id_option(options);
    let model = model_option(options, &config, &provider_id);
    let fallback_model = fallback_model_option(options);
    if fallback_model.as_deref() == Some(model.as_str()) {
        return Err(anyhow!(
            "Fallback model cannot be the same as the main model"
        ));
    }
    let api_timeout = api_timeout_option(options, config.api_timeout_ms)?;
    let thinking = thinking_option(options)?;
    let max_tokens = max_tokens_option(options, thinking.as_ref())?;
    let max_iterations = max_iterations_option(options);
    let repair_checks = repair_checks_option(options);
    let max_repair_attempts = repair_check_attempts_option(options)?;
    let compaction_config = compaction_config_option(options)?;
    let provider = build_provider(&provider_id, &model, options, &config, api_timeout)?;

    let registry = create_default_registry();
    let enabled_tools = provider_default_tool_filter(
        tool_filter_option(options, &registry)?,
        options,
        provider.as_ref(),
        &model,
    );
    let tool_schemas = filtered_tool_schemas(&registry, enabled_tools.as_ref());
    let mut run_abort_signal = abort_signal.clone();
    let cwd = string_option(options, "cwd").unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    });
    let assistant_checkpoint = create_assistant_turn_checkpoint_if_requested(options, &cwd);
    let mut app_state = HashMap::new();
    if let Some(access_roots) = access_roots_option(options) {
        app_state.insert(ACCESS_ROOTS_APP_STATE_KEY.to_string(), json!(access_roots));
    }
    seed_file_set_options(&mut app_state, options);
    if let Some(mcp_servers) = mcp_servers_option(options)? {
        app_state.insert(MCP_SERVERS_APP_STATE_KEY.to_string(), mcp_servers);
    }
    if let Some(sandbox) = sandbox_option(options, config.sandbox.clone())? {
        app_state.insert("sandbox".to_string(), sandbox);
    }
    if let Some(permission_mode) = permission_mode_option(options)? {
        apply_permission_mode(&mut app_state, &permission_mode);
    }
    apply_permission_rule_options(&mut app_state, options);
    if let Some(permission_prompt_tool) = permission_prompt_tool_option(options) {
        app_state.insert(
            PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
            json!(permission_prompt_tool),
        );
    }
    seed_team_app_state_from_env(&mut app_state);
    seed_mailbox_options_from_run_options(&mut app_state, options);

    let mut tool_context = ToolContext {
        cwd,
        read_file_state: HashMap::new(),
        app_state,
        abort_signal,
    };
    if let Some(schema) = structured_output_schema_option(options)? {
        tool_context
            .app_state
            .insert("structured_output_schema".to_string(), schema);
    }
    let mut messages = normalize_messages(messages)?;
    if let Some(mailbox_context) = consume_initial_mailbox_messages(&mut tool_context.app_state)? {
        append_mailbox_context_to_messages(&mut messages, mailbox_context);
    }
    let session_start_hook_contexts =
        session_start_contexts(&tool_context, "runner_streaming").await?;
    let user_prompt_submit_result =
        user_prompt_submit_hook_result(&tool_context, "runner_streaming", &messages).await?;
    apply_user_prompt_submit_update(&mut messages, &user_prompt_submit_result);
    let system_prompt = append_session_start_contexts_to_system_prompt(
        system_prompt_option(options),
        session_start_hook_contexts,
    );
    let system_prompt = append_user_prompt_submit_contexts_to_system_prompt(
        system_prompt,
        user_prompt_submit_result.add_contexts,
    );
    let mut final_text = String::new();
    let mut final_structured_output = None;
    let mut active_model = model.clone();
    let mut fallback_used = false;
    let mut repair_attempts = 0_u32;

    for iteration in 1..=max_iterations {
        messages = compact_messages_for_request(messages, &compaction_config).await?;
        let request = MessagesRequest {
            model: active_model.clone(),
            messages: messages.clone(),
            max_tokens,
            system: system_prompt.clone(),
            temperature: None,
            tools: if tool_schemas.is_empty() {
                None
            } else {
                Some(tool_schemas.clone())
            },
            thinking: thinking.clone(),
            stream: None,
        };
        let mut stream = match abortable_provider_stream(
            &mut run_abort_signal,
            provider.as_ref(),
            request,
        )
        .await
        {
            Ok(stream) => stream,
            Err(error)
                if should_retry_with_fallback(&error, fallback_model.as_deref(), fallback_used) =>
            {
                active_model = fallback_model.clone().expect("fallback checked above");
                fallback_used = true;
                let request = MessagesRequest {
                    model: active_model.clone(),
                    messages: messages.clone(),
                    max_tokens,
                    system: system_prompt.clone(),
                    temperature: None,
                    tools: if tool_schemas.is_empty() {
                        None
                    } else {
                        Some(tool_schemas.clone())
                    },
                    thinking: thinking.clone(),
                    stream: None,
                };
                abortable_provider_stream(&mut run_abort_signal, provider.as_ref(), request).await?
            }
            Err(error) => return Err(error),
        };

        let response_content = abortable_runner_result(
            &mut run_abort_signal,
            collect_streaming_content(&mut stream, &mut |event| {
                on_runner_event(RunnerStreamEvent::Model(event))
            }),
        )
        .await?;
        let response_text = text_from_content(&response_content);
        let tool_uses = tool_uses_from_content(&response_content);

        messages.push(Message {
            role: "assistant".to_string(),
            content: json!(response_content),
        });

        if tool_uses.is_empty() {
            final_text = response_text;
            if repair_checks && repair_attempts < max_repair_attempts {
                let feedback = abortable_runner_result(
                    &mut run_abort_signal,
                    repair_feedback_if_checks_failed(
                        &tool_context.cwd,
                        repair_attempts + 1,
                        max_repair_attempts,
                    ),
                )
                .await?;
                if let Some(feedback) = feedback {
                    repair_attempts += 1;
                    messages.push(Message {
                        role: "user".to_string(),
                        content: json!(feedback),
                    });
                    continue;
                }
            }
            record_assistant_turn_final_state_if_requested(
                assistant_checkpoint.as_deref(),
                &tool_context.cwd,
            );
            return Ok(AssistantRunResult {
                text: final_text,
                messages,
                iterations: iteration,
                stop_reason: "model_stop".to_string(),
                structured_output: final_structured_output,
                teammate_shutdown_approved: app_state_bool(
                    &tool_context.app_state,
                    "teammate_shutdown_approved",
                ),
            });
        }

        let tool_call_requests = tool_uses
            .iter()
            .map(|tool_use| ToolCallRequest {
                name: tool_use.name.clone(),
                input: tool_use.input.clone(),
                tool_use_id: Some(tool_use.id.clone()),
            })
            .collect::<Vec<_>>();
        let batch_results = abortable_runner_value(
            &mut run_abort_signal,
            execute_tool_calls_with_permission_handler(
                &registry,
                enabled_tools.as_ref(),
                &mut tool_context,
                &tool_call_requests,
                permission_handler,
            ),
        )
        .await?;
        let mut tool_results = Vec::new();
        for (tool_use, result) in tool_uses.into_iter().zip(batch_results) {
            if let Some(structured_output) = &result.structured_output {
                final_structured_output = Some(structured_output.clone());
            }
            on_runner_event(RunnerStreamEvent::ToolResult {
                id: tool_use.id,
                name: tool_use.name,
                is_error: result.is_error,
                content: tool_result_event_content(&result),
                error: result.api_result.get("error").cloned(),
            })?;
            tool_results.push(result.api_result);
        }

        messages.push(Message {
            role: "user".to_string(),
            content: json!(tool_results),
        });
        final_text = response_text;
        if final_structured_output.is_some() {
            record_assistant_turn_final_state_if_requested(
                assistant_checkpoint.as_deref(),
                &tool_context.cwd,
            );
            return Ok(AssistantRunResult {
                text: final_text,
                messages,
                iterations: iteration,
                stop_reason: "structured_output".to_string(),
                structured_output: final_structured_output,
                teammate_shutdown_approved: app_state_bool(
                    &tool_context.app_state,
                    "teammate_shutdown_approved",
                ),
            });
        }
    }

    Err(anyhow!(
        "assistant turn exceeded max_iterations={} after partial output: {}",
        max_iterations,
        final_text
    ))
}

async fn abortable_runner_result<T, Fut>(
    abort_signal: &mut tokio::sync::watch::Receiver<bool>,
    future: Fut,
) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    if *abort_signal.borrow() {
        return Err(anyhow!("assistant turn cancelled"));
    }
    tokio::pin!(future);
    let mut abort_open = true;
    loop {
        tokio::select! {
            result = &mut future => return result,
            changed = abort_signal.changed(), if abort_open => {
                if changed.is_err() {
                    abort_open = false;
                } else if *abort_signal.borrow() {
                    return Err(anyhow!("assistant turn cancelled"));
                }
            }
        }
    }
}

async fn abortable_provider_stream(
    abort_signal: &mut tokio::sync::watch::Receiver<bool>,
    provider: &dyn Provider,
    request: MessagesRequest,
) -> Result<ProviderStream> {
    abortable_runner_result(abort_signal, async {
        provider
            .stream_message(request)
            .await
            .map_err(anyhow::Error::from)
    })
    .await
}

async fn abortable_runner_value<T, Fut>(
    abort_signal: &mut tokio::sync::watch::Receiver<bool>,
    future: Fut,
) -> Result<T>
where
    Fut: Future<Output = T>,
{
    if *abort_signal.borrow() {
        return Err(anyhow!("assistant turn cancelled"));
    }
    tokio::pin!(future);
    let mut abort_open = true;
    loop {
        tokio::select! {
            result = &mut future => return Ok(result),
            changed = abort_signal.changed(), if abort_open => {
                if changed.is_err() {
                    abort_open = false;
                } else if *abort_signal.borrow() {
                    return Err(anyhow!("assistant turn cancelled"));
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ToolUseBlock {
    id: String,
    name: String,
    input: Value,
}

#[cfg(test)]
async fn call_tool(
    registry: &kiana_tools::ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
    context: &mut ToolContext,
    name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> ToolExecutionResult {
    match permission_handler {
        Some(permission_handler) => {
            kiana_tools::tool_execution::execute_tool_call_with_permission_handler(
                registry,
                enabled_tools,
                context,
                name,
                input,
                tool_use_id,
                Some(permission_handler),
            )
            .await
        }
        None => {
            kiana_tools::tool_execution::execute_tool_call(
                registry,
                enabled_tools,
                context,
                name,
                input,
                tool_use_id,
            )
            .await
        }
    }
}

fn normalize_messages(messages: Vec<Value>) -> Result<Vec<Message>> {
    messages
        .into_iter()
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("message missing role"))?;
            let content = message
                .get("content")
                .cloned()
                .ok_or_else(|| anyhow!("message missing content"))?;
            Ok(Message {
                role: role.to_string(),
                content,
            })
        })
        .collect()
}

async fn compact_messages_for_request(
    messages: Vec<Message>,
    config: &CompactionConfig,
) -> Result<Vec<Message>> {
    let rendered_messages = messages.iter().map(render_compaction_message).collect();
    let report = compact_context_report(rendered_messages, config.clone()).await?;
    if !report.compacted {
        return Ok(messages);
    }
    apply_compaction_report(messages, &report)
}

fn apply_compaction_report(
    messages: Vec<Message>,
    report: &CompactionReport,
) -> Result<Vec<Message>> {
    let mut suffix_start = messages
        .len()
        .checked_sub(report.kept_suffix_count)
        .ok_or_else(|| anyhow!("invalid compaction suffix count"))?;
    if suffix_start > report.kept_prefix_count
        && message_contains_tool_result(&messages[suffix_start])
    {
        suffix_start -= 1;
    }

    let mut compacted = Vec::with_capacity(report.compacted_message_count + 1);
    compacted.extend(messages.iter().take(report.kept_prefix_count).cloned());
    compacted.push(compaction_summary_message(report)?);
    compacted.extend(messages.iter().skip(suffix_start).cloned());
    Ok(compacted)
}

fn compaction_summary_message(report: &CompactionReport) -> Result<Message> {
    let summary = report
        .summary
        .as_deref()
        .ok_or_else(|| anyhow!("compaction report did not contain a summary"))?;
    Ok(Message {
        role: "user".to_string(),
        content: json!([{
            "type": "text",
            "text": summary
        }]),
    })
}

fn render_compaction_message(message: &Message) -> String {
    format!(
        "{}: {}",
        message.role,
        content_text_for_compaction(&message.content)
    )
}

fn content_text_for_compaction(content: &Value) -> String {
    match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(block_text_for_compaction)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string(),
        Value::Object(_) => block_text_for_compaction(content)
            .unwrap_or_else(|| serde_json::to_string(content).unwrap_or_default()),
        _ => content.to_string(),
    }
}

fn block_text_for_compaction(block: &Value) -> Option<String> {
    if let Some(text) = block.get("text").and_then(Value::as_str) {
        return Some(text.trim().to_string());
    }
    if let Some(text) = block.get("content").and_then(Value::as_str) {
        return Some(text.trim().to_string());
    }
    if let Some(input) = block.get("input") {
        return serde_json::to_string(input).ok();
    }
    None
}

fn message_contains_tool_result(message: &Message) -> bool {
    message.content.as_array().is_some_and(|blocks| {
        blocks
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
    })
}

fn seed_team_app_state_from_env(app_state: &mut HashMap<String, Value>) {
    let agent_id = env_string("KIANA_AGENT_ID").or_else(|| env_string("CLAUDE_CODE_AGENT_ID"));
    let agent_name =
        env_string("KIANA_AGENT_NAME").or_else(|| env_string("CLAUDE_CODE_AGENT_NAME"));
    let agent_color = env_string("KIANA_AGENT_COLOR");
    let team_name = env_string("KIANA_TEAM_NAME").or_else(|| env_string("CLAUDE_CODE_TEAM_NAME"));
    let task_list_id =
        env_string("KIANA_TASK_LIST_ID").or_else(|| env_string("CLAUDE_CODE_TASK_LIST_ID"));
    let team_file = env_string("KIANA_TEAM_FILE");
    let team_mailbox = env_string("KIANA_TEAM_MAILBOX");
    let teams_root = env_string("KIANA_TEAMS_ROOT");
    let tasks_root = env_string("KIANA_TASKS_ROOT");
    let plan_mode_required = env_bool("KIANA_PLAN_MODE_REQUIRED").unwrap_or(false);

    insert_if_absent(app_state, "agent_id", agent_id.clone().map(Value::String));
    insert_if_absent(app_state, "agentId", agent_id.clone().map(Value::String));
    insert_if_absent(
        app_state,
        "agent_name",
        agent_name.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "agentName",
        agent_name.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "agent_color",
        agent_color.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "agentColor",
        agent_color.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "task_list_id",
        task_list_id.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "taskListId",
        task_list_id.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "team_mailbox",
        team_mailbox.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "teamMailbox",
        team_mailbox.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "teams_root",
        teams_root.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "teamsRoot",
        teams_root.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "tasks_root",
        tasks_root.clone().map(Value::String),
    );
    insert_if_absent(
        app_state,
        "tasksRoot",
        tasks_root.clone().map(Value::String),
    );

    let Some(team_name) = team_name else {
        return;
    };

    let agent_id = agent_id.unwrap_or_else(|| {
        let name = agent_name.as_deref().unwrap_or("agent");
        format!(
            "{}@{}",
            sanitize_agent_part(name),
            sanitize_agent_part(&team_name)
        )
    });
    let agent_name = agent_name.unwrap_or_else(|| "agent".to_string());
    let mut teammates = serde_json::Map::new();
    teammates.insert(
        agent_id.clone(),
        json!({
            "agent_id": agent_id,
            "agentId": agent_id,
            "name": agent_name,
            "color": agent_color,
            "mailbox": team_mailbox,
            "planModeRequired": plan_mode_required,
            "plan_mode_required": plan_mode_required
        }),
    );
    let team_context = json!({
        "team_name": team_name,
        "teamName": team_name,
        "team_file_path": team_file,
        "teamFilePath": team_file,
        "task_list_id": task_list_id,
        "taskListId": task_list_id,
        "teammates": teammates
    });
    insert_if_absent(app_state, "team_context", Some(team_context.clone()));
    insert_if_absent(app_state, "teamContext", Some(team_context));
}

fn insert_if_absent(app_state: &mut HashMap<String, Value>, key: &str, value: Option<Value>) {
    let Some(value) = value else {
        return;
    };
    app_state.entry(key.to_string()).or_insert(value);
}

fn seed_mailbox_options_from_run_options(
    app_state: &mut HashMap<String, Value>,
    options: &HashMap<String, Value>,
) {
    if let Some(prompt) = string_option(options, "skip_initial_mailbox_prompt") {
        app_state.insert("skip_initial_mailbox_prompt".to_string(), json!(prompt));
    }
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
}

fn sanitize_agent_part(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
}

fn consume_initial_mailbox_messages(
    app_state: &mut HashMap<String, Value>,
) -> Result<Option<String>> {
    let Some(mailbox_path) = teammate_mailbox_path() else {
        return Ok(None);
    };
    if !mailbox_path.is_file() {
        return Ok(None);
    }

    let mut mailbox: Value = serde_json::from_str(&fs::read_to_string(&mailbox_path)?)
        .map_err(|error| anyhow!("team mailbox is not valid JSON: {error}"))?;

    let (context, changed) = {
        let Some(messages) = mailbox.as_array_mut() else {
            return Ok(None);
        };

        let mut changed = false;
        if let Some(request) = take_first_unread_shutdown_request(messages, &mut changed) {
            (Some(format_shutdown_request_context(&request)), changed)
        } else if let Some(message) = take_first_unread_permission_message(messages, &mut changed) {
            record_permission_message(app_state, &message);
            (Some(format_permission_context(&message)), changed)
        } else if let Some(message) =
            take_first_unread_sandbox_permission_message(messages, &mut changed)
        {
            record_sandbox_permission_message(app_state, &message);
            (Some(format_sandbox_permission_context(&message)), changed)
        } else if let Some(message) =
            take_first_unread_plan_approval_message(messages, &mut changed)
        {
            record_plan_approval_message(app_state, &message);
            if let Some(context) =
                auto_approve_plan_approval_request(app_state, &mailbox_path, &message)?
            {
                (Some(context), changed)
            } else {
                (Some(format_plan_approval_context(&message)), changed)
            }
        } else {
            let state_updates =
                apply_unread_state_update_messages(messages, app_state, &mut changed);
            let mut delivered = Vec::new();
            for message in messages.iter_mut() {
                let read = message
                    .get("read")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if read {
                    continue;
                }
                let text = message
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if text.trim().is_empty() || is_structured_protocol_message(&text) {
                    continue;
                }
                if should_skip_initial_mailbox_prompt(app_state, message, &text) {
                    if let Some(object) = message.as_object_mut() {
                        object.insert("read".to_string(), json!(true));
                        changed = true;
                    }
                    continue;
                }
                delivered.push(TeammateMailboxMessage {
                    from: message
                        .get("from")
                        .and_then(Value::as_str)
                        .unwrap_or("team-lead")
                        .to_string(),
                    text,
                    summary: message
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    color: message
                        .get("color")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
                if let Some(object) = message.as_object_mut() {
                    object.insert("read".to_string(), json!(true));
                    changed = true;
                }
            }

            if delivered.is_empty() {
                if state_updates.is_empty() {
                    (None, changed)
                } else {
                    (Some(format_state_update_context(&state_updates)), changed)
                }
            } else {
                (
                    Some(format!(
                        "# Teammate Mailbox\n\n{}",
                        format_teammate_messages(&delivered)
                    )),
                    changed,
                )
            }
        }
    };

    if changed {
        fs::write(&mailbox_path, serde_json::to_string_pretty(&mailbox)?)?;
    }
    Ok(context)
}

fn teammate_mailbox_path() -> Option<PathBuf> {
    env_string("KIANA_TEAM_MAILBOX")
        .map(PathBuf::from)
        .or_else(|| {
            let teams_root = env_string("KIANA_TEAMS_ROOT")?;
            let team_name =
                env_string("KIANA_TEAM_NAME").or_else(|| env_string("CLAUDE_CODE_TEAM_NAME"))?;
            let agent_name =
                env_string("KIANA_AGENT_NAME").or_else(|| env_string("CLAUDE_CODE_AGENT_NAME"))?;
            Some(
                PathBuf::from(teams_root)
                    .join(sanitize_team_name(&team_name))
                    .join("inboxes")
                    .join(format!("{}.json", sanitize_agent_part(&agent_name))),
            )
        })
}

pub fn teammate_mailbox_has_unread_messages() -> Result<bool> {
    let Some(mailbox_path) = teammate_mailbox_path() else {
        return Ok(false);
    };
    if !mailbox_path.is_file() {
        return Ok(false);
    }
    let mailbox: Value = serde_json::from_str(&fs::read_to_string(&mailbox_path)?)
        .map_err(|error| anyhow!("team mailbox is not valid JSON: {error}"))?;
    let Some(messages) = mailbox.as_array() else {
        return Ok(false);
    };
    Ok(messages.iter().any(|message| {
        !message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && message
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
    }))
}

async fn try_claim_next_team_task_prompt() -> Result<Option<String>> {
    let Some(agent_name) = env_string("KIANA_AGENT_NAME")
        .or_else(|| env_string("CLAUDE_CODE_AGENT_NAME"))
        .filter(|name| !name.eq_ignore_ascii_case("team-lead"))
    else {
        return Ok(None);
    };
    let Some(task_list_id) = resident_task_list_id() else {
        return Ok(None);
    };

    let Ok(mut context) = resident_task_tool_context(&task_list_id, &agent_name) else {
        return Ok(None);
    };
    let listed = match TaskListTool::new().call(&json!({}), &mut context).await {
        Ok(output) => output.data,
        Err(_) => return Ok(None),
    };
    let tasks = listed
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(task) = find_available_task(&tasks) else {
        return Ok(None);
    };
    let Some(task_id) = task.get("id").and_then(Value::as_str).map(str::to_string) else {
        return Ok(None);
    };

    let claimed = match TaskUpdateTool::new()
        .call(
            &json!({
                "task_id": task_id,
                "owner": agent_name,
                "claim": true
            }),
            &mut context,
        )
        .await
    {
        Ok(output) => output.data,
        Err(_) => return Ok(None),
    };
    if !result_bool(&claimed, "success") {
        return Ok(None);
    }
    let updated = match TaskUpdateTool::new()
        .call(
            &json!({
                "task_id": task_id,
                "status": "in_progress"
            }),
            &mut context,
        )
        .await
    {
        Ok(output) => output.data,
        Err(_) => return Ok(None),
    };
    if !result_bool(&updated, "success") {
        return Ok(None);
    }

    Ok(Some(format_task_as_prompt(
        updated.get("task").unwrap_or(&task),
    )))
}

fn resident_task_list_id() -> Option<String> {
    env_string("KIANA_TASK_LIST_ID")
        .or_else(|| env_string("CLAUDE_CODE_TASK_LIST_ID"))
        .or_else(|| env_string("KIANA_TEAM_NAME").map(|team| sanitize_team_name(&team)))
        .or_else(|| env_string("CLAUDE_CODE_TEAM_NAME").map(|team| sanitize_team_name(&team)))
}

fn resident_task_tool_context(task_list_id: &str, agent_name: &str) -> Result<ToolContext> {
    let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
    let cwd = std::env::current_dir()?.to_string_lossy().to_string();
    let mut app_state = HashMap::from([
        ("task_list_id".to_string(), json!(task_list_id)),
        ("taskListId".to_string(), json!(task_list_id)),
        ("agent_name".to_string(), json!(agent_name)),
        ("agentName".to_string(), json!(agent_name)),
    ]);
    if let Some(tasks_root) = env_string("KIANA_TASKS_ROOT") {
        app_state.insert("tasks_root".to_string(), json!(tasks_root.clone()));
        app_state.insert("tasksRoot".to_string(), json!(tasks_root));
    }
    Ok(ToolContext {
        cwd,
        read_file_state: HashMap::new(),
        app_state,
        abort_signal,
    })
}

fn find_available_task(tasks: &[Value]) -> Option<Value> {
    let unresolved_task_ids = tasks
        .iter()
        .filter(|task| !task_status_is_terminal(task.get("status").and_then(Value::as_str)))
        .filter_map(|task| task.get("id").and_then(Value::as_str))
        .collect::<HashSet<_>>();

    tasks
        .iter()
        .find(|task| {
            task.get("status").and_then(Value::as_str) == Some("pending")
                && task
                    .get("owner")
                    .and_then(Value::as_str)
                    .is_none_or(|owner| owner.trim().is_empty())
                && task_blocked_by(task)
                    .iter()
                    .all(|blocker| !unresolved_task_ids.contains(blocker.as_str()))
        })
        .cloned()
}

fn task_status_is_terminal(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("completed" | "complete" | "success" | "failed" | "cancelled" | "canceled" | "killed")
    )
}

fn task_blocked_by(task: &Value) -> Vec<String> {
    task.get("blockedBy")
        .or_else(|| task.get("blocked_by"))
        .and_then(Value::as_array)
        .map(|blockers| {
            blockers
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn format_task_as_prompt(task: &Value) -> String {
    let task_id = task.get("id").and_then(Value::as_str).unwrap_or_default();
    let subject = task
        .get("subject")
        .or_else(|| task.get("title"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut prompt = format!("Complete all open tasks. Start with task #{task_id}: \n\n {subject}");
    if let Some(description) = task
        .get("description")
        .and_then(Value::as_str)
        .filter(|description| !description.trim().is_empty())
    {
        prompt.push_str("\n\n");
        prompt.push_str(description);
    }
    prompt
}

fn sanitize_team_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

struct TeammateMailboxMessage {
    from: String,
    text: String,
    summary: Option<String>,
    color: Option<String>,
}

struct ShutdownRequestMailboxMessage {
    from: String,
    request_id: Option<String>,
    reason: Option<String>,
    timestamp: Option<String>,
    original_text: String,
}

enum PlanApprovalMailboxMessage {
    Request {
        from: String,
        request_id: Option<String>,
        plan_file_path: Option<String>,
        plan_content: Option<String>,
        timestamp: Option<String>,
        original_text: String,
    },
    Response {
        from: String,
        request_id: Option<String>,
        approved: Option<bool>,
        feedback: Option<String>,
        permission_mode: Option<String>,
        timestamp: Option<String>,
        original_text: String,
    },
}

enum PermissionMailboxMessage {
    Request {
        from: String,
        request_id: String,
        agent_id: Option<String>,
        tool_name: Option<String>,
        tool_use_id: Option<String>,
        description: Option<String>,
        input: Value,
        permission_suggestions: Value,
        original_text: String,
    },
    Response {
        from: String,
        request_id: String,
        subtype: String,
        error: Option<String>,
        response: Value,
        tool_name: Option<String>,
        input: Option<Value>,
        original_text: String,
    },
}

enum SandboxPermissionMailboxMessage {
    Request {
        from: String,
        request_id: String,
        worker_id: Option<String>,
        worker_name: Option<String>,
        worker_color: Option<String>,
        host: String,
        created_at: Option<Value>,
        original_text: String,
    },
    Response {
        from: String,
        request_id: String,
        host: String,
        allow: bool,
        timestamp: Option<String>,
        original_text: String,
    },
}

enum StateUpdateMailboxMessage {
    TeamPermissionUpdate {
        from: String,
        behavior: String,
        rules: Vec<String>,
        tool_name: Option<String>,
        directory_path: Option<String>,
        original_text: String,
    },
    ModeSetRequest {
        from: String,
        mode: String,
        applied: bool,
        original_text: String,
    },
}

fn take_first_unread_shutdown_request(
    messages: &mut [Value],
    changed: &mut bool,
) -> Option<ShutdownRequestMailboxMessage> {
    for message in messages.iter_mut() {
        let read = message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if read {
            continue;
        }
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let fallback_from = message
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or("team-lead");
        let Some(request) = parse_shutdown_request(text, fallback_from) else {
            continue;
        };
        if let Some(object) = message.as_object_mut() {
            object.insert("read".to_string(), json!(true));
            *changed = true;
        }
        return Some(request);
    }
    None
}

fn parse_shutdown_request(
    text: &str,
    fallback_from: &str,
) -> Option<ShutdownRequestMailboxMessage> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("shutdown_request") {
        return None;
    }
    let from = value
        .get("from")
        .and_then(Value::as_str)
        .filter(|from| !from.trim().is_empty())
        .unwrap_or(fallback_from)
        .to_string();
    Some(ShutdownRequestMailboxMessage {
        from,
        request_id: value
            .get("requestId")
            .or_else(|| value.get("request_id"))
            .and_then(Value::as_str)
            .filter(|request_id| !request_id.trim().is_empty())
            .map(str::to_string),
        reason: value
            .get("reason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.trim().is_empty())
            .map(str::to_string),
        timestamp: value
            .get("timestamp")
            .and_then(Value::as_str)
            .filter(|timestamp| !timestamp.trim().is_empty())
            .map(str::to_string),
        original_text: text.to_string(),
    })
}

fn take_first_unread_permission_message(
    messages: &mut [Value],
    changed: &mut bool,
) -> Option<PermissionMailboxMessage> {
    for message in messages.iter_mut() {
        let read = message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if read {
            continue;
        }
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let fallback_from = message
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or("team-lead");
        let Some(permission_message) = parse_permission_message(text, fallback_from) else {
            continue;
        };
        if let Some(object) = message.as_object_mut() {
            object.insert("read".to_string(), json!(true));
            *changed = true;
        }
        return Some(permission_message);
    }
    None
}

fn parse_permission_message(text: &str, fallback_from: &str) -> Option<PermissionMailboxMessage> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let message_type = value.get("type").and_then(Value::as_str)?;
    let request_id = value
        .get("request_id")
        .or_else(|| value.get("requestId"))
        .and_then(Value::as_str)
        .filter(|request_id| !request_id.trim().is_empty())?
        .to_string();
    let from = value
        .get("from")
        .and_then(Value::as_str)
        .filter(|from| !from.trim().is_empty())
        .unwrap_or(fallback_from)
        .to_string();

    match message_type {
        "permission_request" => Some(PermissionMailboxMessage::Request {
            from,
            request_id,
            agent_id: value
                .get("agent_id")
                .or_else(|| value.get("agentId"))
                .and_then(Value::as_str)
                .filter(|agent_id| !agent_id.trim().is_empty())
                .map(str::to_string),
            tool_name: value
                .get("tool_name")
                .or_else(|| value.get("toolName"))
                .and_then(Value::as_str)
                .filter(|tool_name| !tool_name.trim().is_empty())
                .map(str::to_string),
            tool_use_id: value
                .get("tool_use_id")
                .or_else(|| value.get("toolUseId"))
                .and_then(Value::as_str)
                .filter(|tool_use_id| !tool_use_id.trim().is_empty())
                .map(str::to_string),
            description: value
                .get("description")
                .and_then(Value::as_str)
                .filter(|description| !description.trim().is_empty())
                .map(str::to_string),
            input: value.get("input").cloned().unwrap_or_else(|| json!({})),
            permission_suggestions: value
                .get("permission_suggestions")
                .or_else(|| value.get("permissionSuggestions"))
                .cloned()
                .unwrap_or_else(|| json!([])),
            original_text: text.to_string(),
        }),
        "permission_response" => Some(PermissionMailboxMessage::Response {
            from,
            request_id,
            subtype: value
                .get("subtype")
                .and_then(Value::as_str)
                .unwrap_or("error")
                .to_string(),
            error: value
                .get("error")
                .or_else(|| value.get("reason"))
                .and_then(Value::as_str)
                .filter(|error| !error.trim().is_empty())
                .map(str::to_string),
            response: value.get("response").cloned().unwrap_or_else(|| json!({})),
            tool_name: value
                .get("tool_name")
                .or_else(|| value.get("toolName"))
                .and_then(Value::as_str)
                .filter(|tool_name| !tool_name.trim().is_empty())
                .map(str::to_string),
            input: value.get("input").cloned(),
            original_text: text.to_string(),
        }),
        _ => None,
    }
}

fn remove_request_from_state_map(
    app_state: &mut HashMap<String, Value>,
    key: &str,
    request_id: &str,
) {
    let Some(mut requests) = app_state.get(key).and_then(Value::as_object).cloned() else {
        return;
    };
    if requests.remove(request_id).is_some() {
        app_state.insert(key.to_string(), Value::Object(requests));
    }
}

fn record_permission_message(
    app_state: &mut HashMap<String, Value>,
    message: &PermissionMailboxMessage,
) {
    match message {
        PermissionMailboxMessage::Request {
            request_id,
            from,
            agent_id,
            tool_name,
            tool_use_id,
            description,
            input,
            permission_suggestions,
            ..
        } => {
            let mut requests = app_state
                .get("mailbox_permission_requests")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            requests.insert(
                request_id.clone(),
                json!({
                    "request_id": request_id,
                    "from": from,
                    "agent_id": agent_id,
                    "tool_name": tool_name,
                    "tool_use_id": tool_use_id,
                    "description": description,
                    "input": input,
                    "permission_suggestions": permission_suggestions
                }),
            );
            app_state.insert(
                "mailbox_permission_requests".to_string(),
                Value::Object(requests),
            );
        }
        PermissionMailboxMessage::Response {
            request_id,
            subtype,
            response,
            tool_name,
            input,
            ..
        } => {
            let pending = app_state
                .get("pending_mailbox_permission_requests")
                .and_then(Value::as_object)
                .and_then(|requests| requests.get(request_id))
                .cloned();
            remove_request_from_state_map(
                app_state,
                "pending_mailbox_permission_requests",
                request_id,
            );
            if subtype != "success" {
                return;
            }
            let tool_name = tool_name.as_deref().map(str::to_string).or_else(|| {
                pending
                    .as_ref()
                    .and_then(|request| request.get("tool_name"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            let input = response
                .get("updated_input")
                .or_else(|| response.get("updatedInput"))
                .cloned()
                .or_else(|| input.clone())
                .or_else(|| {
                    pending
                        .as_ref()
                        .and_then(|request| request.get("input"))
                        .cloned()
                })
                .unwrap_or_else(|| json!({}));
            let mut grants = app_state
                .get("mailbox_permission_grants")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            grants.push(json!({
                "request_id": request_id,
                "tool_name": tool_name,
                "input": input,
                "used": false
            }));
            app_state.insert(
                "mailbox_permission_grants".to_string(),
                Value::Array(grants),
            );
        }
    }
}

fn format_permission_context(message: &PermissionMailboxMessage) -> String {
    match message {
        PermissionMailboxMessage::Request {
            from,
            request_id,
            agent_id,
            tool_name,
            tool_use_id,
            description,
            input,
            permission_suggestions,
            original_text,
        } => {
            let mut context = format!(
                "# Permission Request\n\nTeammate `{from}` requested permission for a tool call.\n\n- request_id: `{request_id}`\n"
            );
            if let Some(agent_id) = agent_id {
                context.push_str(&format!("- agent_id: `{agent_id}`\n"));
            }
            if let Some(tool_name) = tool_name {
                context.push_str(&format!("- tool_name: `{tool_name}`\n"));
            }
            if let Some(tool_use_id) = tool_use_id {
                context.push_str(&format!("- tool_use_id: `{tool_use_id}`\n"));
            }
            if let Some(description) = description {
                context.push_str(&format!("- description: {description}\n"));
            }
            context.push_str("\nRequested input:\n```json\n");
            context.push_str(
                &serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string()),
            );
            context.push_str("\n```\n");
            if permission_suggestions != &json!([]) {
                context.push_str("\nPermission suggestions:\n```json\n");
                context.push_str(
                    &serde_json::to_string_pretty(permission_suggestions)
                        .unwrap_or_else(|_| permission_suggestions.to_string()),
                );
                context.push_str("\n```\n");
            }
            context.push_str(
                "\nApprove or reject this request with the `SendMessage` tool. Send `to` to the requesting teammate and `message: {\"type\":\"permission_response\",\"request_id\":\"...\",\"approve\":true}` to approve, or set `approve:false` with `reason` to reject.\n\nOriginal protocol message:\n```json\n",
            );
            context.push_str(&pretty_json_text(original_text));
            context.push_str("\n```");
            context
        }
        PermissionMailboxMessage::Response {
            from,
            request_id,
            subtype,
            error,
            response,
            original_text,
            ..
        } => {
            let approved = subtype == "success";
            let mut context = format!(
                "# Permission Response\n\nA permission response was received from `{from}`.\n\n- request_id: `{request_id}`\n- status: {}\n",
                if approved { "approved" } else { "rejected" }
            );
            if let Some(error) = error {
                context.push_str(&format!("- reason: {error}\n"));
            }
            if approved && response != &json!({}) {
                context.push_str("\nResponse data:\n```json\n");
                context.push_str(
                    &serde_json::to_string_pretty(response)
                        .unwrap_or_else(|_| response.to_string()),
                );
                context.push_str("\n```\n");
            }
            if approved {
                context.push_str(
                    "\nThe approval has been recorded as a one-time grant. Retry the approved tool call now.\n",
                );
            } else {
                context.push_str(
                    "\nDo not retry the rejected tool call unless you change the request.\n",
                );
            }
            context.push_str("\nOriginal protocol message:\n```json\n");
            context.push_str(&pretty_json_text(original_text));
            context.push_str("\n```");
            context
        }
    }
}

fn take_first_unread_sandbox_permission_message(
    messages: &mut [Value],
    changed: &mut bool,
) -> Option<SandboxPermissionMailboxMessage> {
    for message in messages.iter_mut() {
        let read = message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if read {
            continue;
        }
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let fallback_from = message
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or("team-lead");
        let Some(sandbox_message) = parse_sandbox_permission_message(text, fallback_from) else {
            continue;
        };
        if let Some(object) = message.as_object_mut() {
            object.insert("read".to_string(), json!(true));
            *changed = true;
        }
        return Some(sandbox_message);
    }
    None
}

fn parse_sandbox_permission_message(
    text: &str,
    fallback_from: &str,
) -> Option<SandboxPermissionMailboxMessage> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let message_type = value.get("type").and_then(Value::as_str)?;
    let request_id = value
        .get("requestId")
        .or_else(|| value.get("request_id"))
        .and_then(Value::as_str)
        .filter(|request_id| !request_id.trim().is_empty())?
        .to_string();
    let from = value
        .get("from")
        .and_then(Value::as_str)
        .filter(|from| !from.trim().is_empty())
        .unwrap_or(fallback_from)
        .to_string();

    match message_type {
        "sandbox_permission_request" => Some(SandboxPermissionMailboxMessage::Request {
            from,
            request_id,
            worker_id: value
                .get("workerId")
                .or_else(|| value.get("worker_id"))
                .and_then(Value::as_str)
                .filter(|worker_id| !worker_id.trim().is_empty())
                .map(str::to_string),
            worker_name: value
                .get("workerName")
                .or_else(|| value.get("worker_name"))
                .and_then(Value::as_str)
                .filter(|worker_name| !worker_name.trim().is_empty())
                .map(str::to_string),
            worker_color: value
                .get("workerColor")
                .or_else(|| value.get("worker_color"))
                .and_then(Value::as_str)
                .filter(|worker_color| !worker_color.trim().is_empty())
                .map(str::to_string),
            host: value
                .get("hostPattern")
                .or_else(|| value.get("host_pattern"))
                .and_then(|host_pattern| host_pattern.get("host"))
                .or_else(|| value.get("host"))
                .and_then(Value::as_str)
                .filter(|host| !host.trim().is_empty())
                .unwrap_or("*")
                .to_string(),
            created_at: value
                .get("createdAt")
                .or_else(|| value.get("created_at"))
                .cloned(),
            original_text: text.to_string(),
        }),
        "sandbox_permission_response" => Some(SandboxPermissionMailboxMessage::Response {
            from,
            request_id,
            host: value
                .get("host")
                .and_then(Value::as_str)
                .filter(|host| !host.trim().is_empty())
                .unwrap_or("*")
                .to_string(),
            allow: value
                .get("allow")
                .or_else(|| value.get("approve"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            timestamp: value
                .get("timestamp")
                .and_then(Value::as_str)
                .filter(|timestamp| !timestamp.trim().is_empty())
                .map(str::to_string),
            original_text: text.to_string(),
        }),
        _ => None,
    }
}

fn record_sandbox_permission_message(
    app_state: &mut HashMap<String, Value>,
    message: &SandboxPermissionMailboxMessage,
) {
    match message {
        SandboxPermissionMailboxMessage::Request {
            request_id,
            from,
            worker_id,
            worker_name,
            worker_color,
            host,
            created_at,
            ..
        } => {
            let mut requests = app_state
                .get("sandbox_permission_requests")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            requests.insert(
                request_id.clone(),
                json!({
                    "requestId": request_id,
                    "request_id": request_id,
                    "from": from,
                    "workerId": worker_id,
                    "worker_id": worker_id,
                    "workerName": worker_name,
                    "worker_name": worker_name,
                    "workerColor": worker_color,
                    "worker_color": worker_color,
                    "host": host,
                    "createdAt": created_at,
                    "created_at": created_at
                }),
            );
            app_state.insert(
                "sandbox_permission_requests".to_string(),
                Value::Object(requests),
            );
        }
        SandboxPermissionMailboxMessage::Response {
            request_id,
            host,
            allow,
            ..
        } => {
            remove_request_from_state_map(
                app_state,
                "pending_sandbox_permission_requests",
                request_id,
            );
            if !allow {
                return;
            }
            let mut grants = app_state
                .get("sandbox_permission_grants")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            grants.push(json!({
                "requestId": request_id,
                "request_id": request_id,
                "host": host,
                "allow": true
            }));
            app_state.insert(
                "sandbox_permission_grants".to_string(),
                Value::Array(grants),
            );
        }
    }
}

fn format_sandbox_permission_context(message: &SandboxPermissionMailboxMessage) -> String {
    match message {
        SandboxPermissionMailboxMessage::Request {
            from,
            request_id,
            worker_id,
            worker_name,
            worker_color,
            host,
            created_at,
            original_text,
        } => {
            let mut context = format!(
                "# Sandbox Permission Request\n\nTeammate `{from}` requested sandbox network access.\n\n- request_id: `{request_id}`\n- host: `{host}`\n"
            );
            if let Some(worker_id) = worker_id {
                context.push_str(&format!("- worker_id: `{worker_id}`\n"));
            }
            if let Some(worker_name) = worker_name {
                context.push_str(&format!("- worker_name: `{worker_name}`\n"));
            }
            if let Some(worker_color) = worker_color {
                context.push_str(&format!("- worker_color: `{worker_color}`\n"));
            }
            if let Some(created_at) = created_at {
                context.push_str(&format!("- created_at: `{}`\n", created_at));
            }
            context.push_str(
                "\nApprove or reject this request with the `SendMessage` tool. Send `to` to the requesting teammate and `message: {\"type\":\"sandbox_permission_response\",\"request_id\":\"...\",\"host\":\"...\",\"allow\":true}` to approve, or set `allow:false` to reject.\n\nOriginal protocol message:\n```json\n",
            );
            context.push_str(&pretty_json_text(original_text));
            context.push_str("\n```");
            context
        }
        SandboxPermissionMailboxMessage::Response {
            from,
            request_id,
            host,
            allow,
            timestamp,
            original_text,
        } => {
            let mut context = format!(
                "# Sandbox Permission Response\n\nA sandbox permission response was received from `{from}`.\n\n- request_id: `{request_id}`\n- host: `{host}`\n- status: {}\n",
                if *allow { "approved" } else { "rejected" }
            );
            if let Some(timestamp) = timestamp {
                context.push_str(&format!("- timestamp: `{timestamp}`\n"));
            }
            if *allow {
                context.push_str("\nThe approval has been recorded as a one-time sandbox grant. Retry the sandboxed command now.\n");
            } else {
                context.push_str("\nDo not retry the rejected sandbox network request unless you change the command.\n");
            }
            context.push_str("\nOriginal protocol message:\n```json\n");
            context.push_str(&pretty_json_text(original_text));
            context.push_str("\n```");
            context
        }
    }
}

fn take_first_unread_plan_approval_message(
    messages: &mut [Value],
    changed: &mut bool,
) -> Option<PlanApprovalMailboxMessage> {
    for message in messages.iter_mut() {
        let read = message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if read {
            continue;
        }
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let fallback_from = message
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or("team-lead");
        let Some(plan_message) = parse_plan_approval_message(text, fallback_from) else {
            continue;
        };
        if let Some(object) = message.as_object_mut() {
            object.insert("read".to_string(), json!(true));
            *changed = true;
        }
        return Some(plan_message);
    }
    None
}

fn parse_plan_approval_message(
    text: &str,
    fallback_from: &str,
) -> Option<PlanApprovalMailboxMessage> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let message_type = value.get("type").and_then(Value::as_str)?;
    let from = value
        .get("from")
        .and_then(Value::as_str)
        .filter(|from| !from.trim().is_empty())
        .unwrap_or(fallback_from)
        .to_string();
    let request_id = value
        .get("requestId")
        .or_else(|| value.get("request_id"))
        .and_then(Value::as_str)
        .filter(|request_id| !request_id.trim().is_empty())
        .map(str::to_string);
    let timestamp = value
        .get("timestamp")
        .and_then(Value::as_str)
        .filter(|timestamp| !timestamp.trim().is_empty())
        .map(str::to_string);

    match message_type {
        "plan_approval_request" => Some(PlanApprovalMailboxMessage::Request {
            from,
            request_id,
            plan_file_path: value
                .get("planFilePath")
                .or_else(|| value.get("plan_file_path"))
                .and_then(Value::as_str)
                .filter(|path| !path.trim().is_empty())
                .map(str::to_string),
            plan_content: value
                .get("planContent")
                .or_else(|| value.get("plan_content"))
                .and_then(Value::as_str)
                .filter(|plan| !plan.trim().is_empty())
                .map(str::to_string),
            timestamp,
            original_text: text.to_string(),
        }),
        "plan_approval_response" => Some(PlanApprovalMailboxMessage::Response {
            from,
            request_id,
            approved: value
                .get("approved")
                .or_else(|| value.get("approve"))
                .and_then(Value::as_bool),
            feedback: value
                .get("feedback")
                .and_then(Value::as_str)
                .filter(|feedback| !feedback.trim().is_empty())
                .map(str::to_string),
            permission_mode: value
                .get("permissionMode")
                .or_else(|| value.get("permission_mode"))
                .and_then(Value::as_str)
                .filter(|mode| !mode.trim().is_empty())
                .map(str::to_string),
            timestamp,
            original_text: text.to_string(),
        }),
        _ => None,
    }
}

fn is_current_team_lead(
    app_state: &HashMap<String, Value>,
    mailbox_path: &std::path::Path,
) -> bool {
    current_agent_name(app_state)
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case("team-lead"))
        || mailbox_path
            .file_stem()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("team-lead"))
}

fn current_permission_mode_for_plan_approval(app_state: &HashMap<String, Value>) -> String {
    let mode = app_state
        .get("permission_mode")
        .or_else(|| app_state.get("permissionMode"))
        .or_else(|| app_state.get("mode"))
        .and_then(Value::as_str)
        .or_else(|| {
            app_state
                .get("permissions")
                .and_then(Value::as_object)
                .and_then(|permissions| {
                    permissions
                        .get("mode")
                        .or_else(|| permissions.get("permissionMode"))
                        .and_then(Value::as_str)
                })
        })
        .and_then(normalize_permission_mode)
        .unwrap_or_else(|| "default".to_string());
    if mode == "plan" {
        "default".to_string()
    } else {
        mode
    }
}

fn append_mailbox_entry(path: &std::path::Path, entry: Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut inbox = if path.is_file() {
        serde_json::from_str::<Value>(&fs::read_to_string(path)?)
            .map_err(|error| anyhow!("team mailbox is not valid JSON: {error}"))?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    inbox.push(entry);
    fs::write(path, serde_json::to_string_pretty(&Value::Array(inbox))?)?;
    Ok(())
}

fn auto_approve_plan_approval_request(
    app_state: &HashMap<String, Value>,
    mailbox_path: &std::path::Path,
    message: &PlanApprovalMailboxMessage,
) -> Result<Option<String>> {
    if !is_current_team_lead(app_state, mailbox_path) {
        return Ok(None);
    }
    let PlanApprovalMailboxMessage::Request {
        from,
        request_id: Some(request_id),
        plan_file_path,
        plan_content,
        timestamp,
        original_text,
    } = message
    else {
        return Ok(None);
    };
    if from.eq_ignore_ascii_case("team-lead") {
        return Ok(None);
    }

    let Some(inbox_dir) = mailbox_path.parent() else {
        return Ok(None);
    };
    let permission_mode = current_permission_mode_for_plan_approval(app_state);
    let response_timestamp = now_unix_seconds().to_string();
    let response_body = json!({
        "type": "plan_approval_response",
        "requestId": request_id,
        "request_id": request_id,
        "approved": true,
        "permissionMode": permission_mode,
        "permission_mode": permission_mode,
        "timestamp": response_timestamp
    });
    let target_path = inbox_dir.join(format!("{}.json", sanitize_agent_part(from)));
    append_mailbox_entry(
        &target_path,
        json!({
            "from": "team-lead",
            "text": serde_json::to_string(&response_body)?,
            "summary": "plan_approval_response",
            "timestamp": response_timestamp,
            "read": false
        }),
    )?;

    let mut context = format!(
        "# Plan Approval Request Auto-Approved\n\nTeammate `{from}` requested approval to exit plan mode. The request was automatically approved and a `plan_approval_response` was written to `{}`.\n\n- request_id: `{request_id}`\n- permission_mode: `{permission_mode}`\n",
        target_path.display()
    );
    if let Some(plan_file_path) = plan_file_path {
        context.push_str(&format!("- plan_file_path: `{plan_file_path}`\n"));
    }
    if let Some(timestamp) = timestamp {
        context.push_str(&format!("- request_timestamp: `{timestamp}`\n"));
    }
    if let Some(plan_content) = plan_content {
        context.push_str("\nSubmitted plan:\n```text\n");
        context.push_str(plan_content);
        context.push_str("\n```\n");
    }
    context.push_str("\nOriginal protocol message:\n```json\n");
    context.push_str(&pretty_json_text(original_text));
    context.push_str("\n```");
    Ok(Some(context))
}

fn format_shutdown_request_context(request: &ShutdownRequestMailboxMessage) -> String {
    let request_id = request
        .request_id
        .as_deref()
        .unwrap_or("<missing-request-id>");
    let reason = request.reason.as_deref().unwrap_or("not provided");
    let mut context = format!(
        "# Shutdown Request\n\nA shutdown request was received from `{}`.\n\n- request_id: `{}`\n- reason: {}\n",
        request.from, request_id, reason
    );
    if let Some(timestamp) = &request.timestamp {
        context.push_str(&format!("- timestamp: `{timestamp}`\n"));
    }
    context.push_str(
        "\nDecide whether it is safe to stop now. Use the `SendMessage` tool with `to: \"team-lead\"` and a `shutdown_response` message. Set `approve: true` to approve shutdown, or set `approve: false` with a `reason` to reject it.\n\nOriginal protocol message:\n```json\n",
    );
    context.push_str(&pretty_json_text(&request.original_text));
    context.push_str("\n```");
    context
}

fn format_plan_approval_context(message: &PlanApprovalMailboxMessage) -> String {
    match message {
        PlanApprovalMailboxMessage::Request {
            from,
            request_id,
            plan_file_path,
            plan_content,
            timestamp,
            original_text,
        } => {
            let request_id = request_id.as_deref().unwrap_or("<missing-request-id>");
            let mut context = format!(
                "# Plan Approval Request\n\nTeammate `{from}` requested approval to exit plan mode.\n\n- request_id: `{request_id}`\n"
            );
            if let Some(plan_file_path) = plan_file_path {
                context.push_str(&format!("- plan_file_path: `{plan_file_path}`\n"));
            }
            if let Some(timestamp) = timestamp {
                context.push_str(&format!("- timestamp: `{timestamp}`\n"));
            }
            if let Some(plan_content) = plan_content {
                context.push_str("\nSubmitted plan:\n```text\n");
                context.push_str(plan_content);
                context.push_str("\n```\n");
            }
            context.push_str(
                "\nReview the plan. Use the `SendMessage` tool with `to` set to that teammate and a `plan_approval_response` message. Set `approve: true` to approve, or set `approve: false` with `feedback` to request changes.\n\nOriginal protocol message:\n```json\n",
            );
            context.push_str(&pretty_json_text(original_text));
            context.push_str("\n```");
            context
        }
        PlanApprovalMailboxMessage::Response {
            from,
            request_id,
            approved,
            feedback,
            permission_mode,
            timestamp,
            original_text,
        } => {
            let request_id = request_id.as_deref().unwrap_or("<missing-request-id>");
            let status = match approved {
                Some(true) => "approved",
                Some(false) => "rejected",
                None => "unknown",
            };
            let mut context = format!(
                "# Plan Approval Response\n\nA plan approval response was received from `{from}`.\n\n- request_id: `{request_id}`\n- status: {status}\n"
            );
            if let Some(permission_mode) = permission_mode {
                context.push_str(&format!("- permission_mode: `{permission_mode}`\n"));
            }
            if let Some(feedback) = feedback {
                context.push_str(&format!("- feedback: {feedback}\n"));
            }
            if let Some(timestamp) = timestamp {
                context.push_str(&format!("- timestamp: `{timestamp}`\n"));
            }
            context.push_str(
                "\nIf approved, proceed with implementation. If rejected, revise the plan and call `ExitPlanMode` again with the updated plan.\n\nOriginal protocol message:\n```json\n",
            );
            context.push_str(&pretty_json_text(original_text));
            context.push_str("\n```");
            context
        }
    }
}

fn record_plan_approval_message(
    app_state: &mut HashMap<String, Value>,
    message: &PlanApprovalMailboxMessage,
) {
    let PlanApprovalMailboxMessage::Response {
        from,
        approved: Some(true),
        permission_mode,
        ..
    } = message
    else {
        return;
    };
    if !from.eq_ignore_ascii_case("team-lead") {
        return;
    }
    let mode = permission_mode.as_deref().unwrap_or("default");
    apply_permission_mode(app_state, mode);
}

fn apply_unread_state_update_messages(
    messages: &mut [Value],
    app_state: &mut HashMap<String, Value>,
    changed: &mut bool,
) -> Vec<StateUpdateMailboxMessage> {
    let mut updates = Vec::new();
    for message in messages.iter_mut() {
        let read = message
            .get("read")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if read {
            continue;
        }
        let text = message
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.trim().is_empty() {
            continue;
        }
        let fallback_from = message
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or("team-lead");
        let Some(update) = parse_state_update_message(text, fallback_from) else {
            continue;
        };
        apply_state_update_message(app_state, &update);
        if let Some(object) = message.as_object_mut() {
            object.insert("read".to_string(), json!(true));
            *changed = true;
        }
        updates.push(update);
    }
    updates
}

fn parse_state_update_message(
    text: &str,
    fallback_from: &str,
) -> Option<StateUpdateMailboxMessage> {
    let value = serde_json::from_str::<Value>(text).ok()?;
    let message_type = value.get("type").and_then(Value::as_str)?;
    let from = value
        .get("from")
        .and_then(Value::as_str)
        .filter(|from| !from.trim().is_empty())
        .unwrap_or(fallback_from)
        .to_string();

    match message_type {
        "team_permission_update" => {
            let permission_update = value
                .get("permissionUpdate")
                .or_else(|| value.get("permission_update"))?;
            let behavior = permission_update
                .get("behavior")
                .and_then(Value::as_str)
                .filter(|behavior| !behavior.trim().is_empty())?
                .to_string();
            let rules = permission_update
                .get("rules")
                .and_then(Value::as_array)
                .map(|rules| {
                    rules
                        .iter()
                        .filter_map(permission_rule_string)
                        .collect::<Vec<_>>()
                })
                .filter(|rules| !rules.is_empty())?;
            Some(StateUpdateMailboxMessage::TeamPermissionUpdate {
                from,
                behavior,
                rules,
                tool_name: value
                    .get("toolName")
                    .or_else(|| value.get("tool_name"))
                    .and_then(Value::as_str)
                    .filter(|tool_name| !tool_name.trim().is_empty())
                    .map(str::to_string),
                directory_path: value
                    .get("directoryPath")
                    .or_else(|| value.get("directory_path"))
                    .and_then(Value::as_str)
                    .filter(|directory_path| !directory_path.trim().is_empty())
                    .map(str::to_string),
                original_text: text.to_string(),
            })
        }
        "mode_set_request" => {
            let mode = value
                .get("mode")
                .and_then(Value::as_str)
                .and_then(normalize_permission_mode)?;
            let applied = from.eq_ignore_ascii_case("team-lead");
            Some(StateUpdateMailboxMessage::ModeSetRequest {
                from,
                mode,
                applied,
                original_text: text.to_string(),
            })
        }
        _ => None,
    }
}

fn permission_rule_string(rule: &Value) -> Option<String> {
    if let Some(rule) = rule.as_str().map(str::trim).filter(|rule| !rule.is_empty()) {
        return Some(rule.to_string());
    }
    let object = rule.as_object()?;
    let tool_name = object
        .get("toolName")
        .or_else(|| object.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tool_name| !tool_name.is_empty())?;
    let rule_content = object
        .get("ruleContent")
        .or_else(|| object.get("rule_content"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|rule_content| !rule_content.is_empty());
    Some(match rule_content {
        Some(rule_content) => format!("{tool_name}({rule_content})"),
        None => tool_name.to_string(),
    })
}

fn apply_state_update_message(
    app_state: &mut HashMap<String, Value>,
    update: &StateUpdateMailboxMessage,
) {
    match update {
        StateUpdateMailboxMessage::TeamPermissionUpdate {
            behavior, rules, ..
        } => {
            apply_permission_rules(app_state, behavior, rules);
        }
        StateUpdateMailboxMessage::ModeSetRequest { mode, applied, .. } if *applied => {
            apply_permission_mode(app_state, mode);
            let _ = sync_current_member_mode(app_state, mode);
        }
        _ => {}
    }
}

fn apply_permission_mode(app_state: &mut HashMap<String, Value>, mode: &str) {
    let Some(mode) = normalize_permission_mode(mode) else {
        return;
    };
    app_state.insert("mode".to_string(), json!(mode));
    app_state.insert("permission_mode".to_string(), json!(mode));
    let mut permissions = app_state
        .get("permissions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    permissions.insert("mode".to_string(), json!(mode));
    permissions.insert("permissionMode".to_string(), json!(mode));
    app_state.insert("permissions".to_string(), Value::Object(permissions));
}

fn permission_mode_option(options: &HashMap<String, Value>) -> Result<Option<String>> {
    let Some(mode) = string_option(options, "permission_mode")
        .or_else(|| string_option(options, "permissionMode"))
    else {
        return Ok(None);
    };
    normalize_permission_mode(&mode)
        .map(Some)
        .ok_or_else(|| anyhow!("invalid permission_mode '{mode}'"))
}

fn apply_permission_rule_options(
    app_state: &mut HashMap<String, Value>,
    options: &HashMap<String, Value>,
) {
    for (behavior, snake_key, camel_key) in [
        ("allow", "allowed_tools", "allowedTools"),
        ("deny", "disallowed_tools", "disallowedTools"),
        ("ask", "ask_tools", "askTools"),
    ] {
        let rules = permission_rule_option(options, snake_key, camel_key);
        if !rules.is_empty() {
            apply_permission_rules(app_state, behavior, &rules);
        }
    }
}

fn permission_rule_option(
    options: &HashMap<String, Value>,
    snake_key: &str,
    camel_key: &str,
) -> Vec<String> {
    options
        .get(snake_key)
        .or_else(|| options.get(camel_key))
        .map(permission_rule_values)
        .unwrap_or_default()
}

fn permission_rule_values(value: &Value) -> Vec<String> {
    let mut rules = Vec::new();
    match value {
        Value::String(text) => extend_permission_rule_tokens(&mut rules, text),
        Value::Array(items) => {
            for item in items {
                if let Some(rule) = permission_rule_string(item) {
                    extend_permission_rule_tokens(&mut rules, &rule);
                }
            }
        }
        Value::Object(_) => {
            if let Some(rule) = permission_rule_string(value) {
                extend_permission_rule_tokens(&mut rules, &rule);
            }
        }
        _ => {}
    }
    rules
}

fn extend_permission_rule_tokens(target: &mut Vec<String>, value: &str) {
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

fn apply_permission_rules(
    app_state: &mut HashMap<String, Value>,
    behavior: &str,
    rules: &[String],
) {
    let (snake_key, camel_key) = match behavior {
        "allow" => ("allowed_tools", "allowedTools"),
        "deny" => ("disallowed_tools", "disallowedTools"),
        "ask" => ("ask_tools", "askTools"),
        _ => return,
    };
    append_unique_string_values(app_state, snake_key, rules);

    let mut permissions = app_state
        .get("permissions")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    append_unique_string_values_to_object(&mut permissions, snake_key, rules);
    append_unique_string_values_to_object(&mut permissions, camel_key, rules);
    app_state.insert("permissions".to_string(), Value::Object(permissions));
}

fn append_unique_string_values(
    app_state: &mut HashMap<String, Value>,
    key: &str,
    values: &[String],
) {
    let mut current = app_state
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    append_unique_json_strings(&mut current, values);
    app_state.insert(key.to_string(), Value::Array(current));
}

fn append_unique_string_values_to_object(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    values: &[String],
) {
    let mut current = object
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    append_unique_json_strings(&mut current, values);
    object.insert(key.to_string(), Value::Array(current));
}

fn append_unique_json_strings(current: &mut Vec<Value>, values: &[String]) {
    for value in values {
        if !current
            .iter()
            .filter_map(Value::as_str)
            .any(|existing| existing.eq_ignore_ascii_case(value))
        {
            current.push(json!(value));
        }
    }
}

fn normalize_permission_mode(mode: &str) -> Option<String> {
    let mode = mode.trim();
    if mode.is_empty() {
        return None;
    }
    Some(
        match mode {
            "accept-edits" | "accept_edits" | "acceptEdits" => "acceptEdits",
            "bypass-permissions" | "bypass_permissions" | "bypassPermissions" => {
                "bypassPermissions"
            }
            "dont-ask" | "dont_ask" | "dontAsk" => "dontAsk",
            "default" => "default",
            "plan" => "plan",
            "auto" => "auto",
            "ask" => "ask",
            other => other,
        }
        .to_string(),
    )
}

fn sync_current_member_mode(app_state: &HashMap<String, Value>, mode: &str) -> Result<()> {
    let Some(team_file_path) = current_team_file_path(app_state) else {
        return Ok(());
    };
    if !team_file_path.is_file() {
        return Ok(());
    }
    let Some(agent_name) = current_agent_name(app_state) else {
        return Ok(());
    };
    let agent_id = current_agent_id(app_state);
    let mut team_file: Value = serde_json::from_str(&fs::read_to_string(&team_file_path)?)?;
    let Some(members) = team_file.get_mut("members").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut updated = false;
    for member in members {
        let name_matches = member
            .get("name")
            .and_then(Value::as_str)
            .map(|name| name.eq_ignore_ascii_case(&agent_name))
            .unwrap_or(false);
        let id_matches = agent_id
            .as_deref()
            .and_then(|agent_id| {
                member
                    .get("agentId")
                    .or_else(|| member.get("agent_id"))
                    .and_then(Value::as_str)
                    .map(|member_id| member_id.eq_ignore_ascii_case(agent_id))
            })
            .unwrap_or(false);
        if name_matches || id_matches {
            if let Some(object) = member.as_object_mut() {
                object.insert("mode".to_string(), json!(mode));
                object.insert("permissionMode".to_string(), json!(mode));
                updated = true;
            }
        }
    }
    if updated {
        fs::write(team_file_path, serde_json::to_string_pretty(&team_file)?)?;
    }
    Ok(())
}

fn sync_current_member_lifecycle_status(status: &str, is_active: Option<bool>) -> Result<()> {
    let app_state = HashMap::new();
    let Some(team_file_path) = current_team_file_path(&app_state) else {
        return Ok(());
    };
    if !team_file_path.is_file() {
        return Ok(());
    }
    let Some(agent_name) = current_agent_name(&app_state) else {
        return Ok(());
    };
    let agent_id = current_agent_id(&app_state);
    let mut team_file: Value = serde_json::from_str(&fs::read_to_string(&team_file_path)?)?;
    let Some(members) = team_file.get_mut("members").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let now = now_unix_seconds();
    let mut updated = false;
    for member in members {
        let name_matches = member
            .get("name")
            .and_then(Value::as_str)
            .map(|name| name.eq_ignore_ascii_case(&agent_name))
            .unwrap_or(false);
        let id_matches = agent_id
            .as_deref()
            .and_then(|agent_id| {
                member
                    .get("agentId")
                    .or_else(|| member.get("agent_id"))
                    .and_then(Value::as_str)
                    .map(|member_id| member_id.eq_ignore_ascii_case(agent_id))
            })
            .unwrap_or(false);
        if !(name_matches || id_matches) {
            continue;
        }
        if let Some(object) = member.as_object_mut() {
            let current_status = object
                .get("lifecycleStatus")
                .or_else(|| object.get("lifecycle_status"))
                .and_then(Value::as_str);
            let current_active = object
                .get("isActive")
                .or_else(|| object.get("is_active"))
                .and_then(Value::as_bool);
            let active_changed = is_active
                .map(|is_active| current_active != Some(is_active))
                .unwrap_or(false);
            if current_status == Some(status)
                && !active_changed
                && object.get("lastStatusAt").is_some()
                && object.get("last_status_at").is_some()
            {
                continue;
            }
            object.insert("lifecycleStatus".to_string(), json!(status));
            object.insert("lifecycle_status".to_string(), json!(status));
            object.insert("lastStatusAt".to_string(), json!(now));
            object.insert("last_status_at".to_string(), json!(now));
            if let Some(is_active) = is_active {
                object.insert("isActive".to_string(), json!(is_active));
                object.insert("is_active".to_string(), json!(is_active));
                if !is_active {
                    object.insert("lastExitReason".to_string(), json!(status));
                    object.insert("last_exit_reason".to_string(), json!(status));
                    object.insert("lastExitedAt".to_string(), json!(now));
                    object.insert("last_exited_at".to_string(), json!(now));
                }
            }
            updated = true;
        }
    }
    if updated {
        fs::write(team_file_path, serde_json::to_string_pretty(&team_file)?)?;
    }
    Ok(())
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn current_team_file_path(app_state: &HashMap<String, Value>) -> Option<PathBuf> {
    env_string("KIANA_TEAM_FILE")
        .map(PathBuf::from)
        .or_else(|| {
            app_state
                .get("team_file_path")
                .or_else(|| app_state.get("teamFilePath"))
                .and_then(Value::as_str)
                .map(PathBuf::from)
        })
        .or_else(|| {
            app_state
                .get("team_context")
                .or_else(|| app_state.get("teamContext"))
                .and_then(|context| {
                    context
                        .get("team_file_path")
                        .or_else(|| context.get("teamFilePath"))
                })
                .and_then(Value::as_str)
                .map(PathBuf::from)
        })
}

fn current_agent_name(app_state: &HashMap<String, Value>) -> Option<String> {
    env_string("KIANA_AGENT_NAME")
        .or_else(|| env_string("CLAUDE_CODE_AGENT_NAME"))
        .or_else(|| {
            app_state
                .get("agent_name")
                .or_else(|| app_state.get("agentName"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn current_agent_id(app_state: &HashMap<String, Value>) -> Option<String> {
    env_string("KIANA_AGENT_ID")
        .or_else(|| env_string("CLAUDE_CODE_AGENT_ID"))
        .or_else(|| {
            app_state
                .get("agent_id")
                .or_else(|| app_state.get("agentId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn format_state_update_context(updates: &[StateUpdateMailboxMessage]) -> String {
    let mut context = "# Mailbox State Updates\n\n".to_string();
    for update in updates {
        match update {
            StateUpdateMailboxMessage::TeamPermissionUpdate {
                from,
                behavior,
                rules,
                tool_name,
                directory_path,
                original_text,
            } => {
                context.push_str(&format!(
                    "- Applied `{behavior}` permission update from `{from}`"
                ));
                if let Some(tool_name) = tool_name {
                    context.push_str(&format!(" for `{tool_name}`"));
                }
                if let Some(directory_path) = directory_path {
                    context.push_str(&format!(" in `{directory_path}`"));
                }
                context.push_str(". Rules: ");
                context.push_str(&rules.join(", "));
                context.push('\n');
                context.push_str("\nOriginal protocol message:\n```json\n");
                context.push_str(&pretty_json_text(original_text));
                context.push_str("\n```\n");
            }
            StateUpdateMailboxMessage::ModeSetRequest {
                from,
                mode,
                applied,
                original_text,
            } => {
                if *applied {
                    context.push_str(&format!(
                        "- Applied mode change from `{from}`. Current permission mode: `{mode}`.\n"
                    ));
                } else {
                    context.push_str(&format!(
                        "- Ignored mode change from non-lead sender `{from}`.\n"
                    ));
                }
                context.push_str("\nOriginal protocol message:\n```json\n");
                context.push_str(&pretty_json_text(original_text));
                context.push_str("\n```\n");
            }
        }
    }
    context.trim_end().to_string()
}

fn pretty_json_text(text: &str) -> String {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| text.to_string())
}

fn format_teammate_messages(messages: &[TeammateMailboxMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let mut attrs = format!(" teammate_id=\"{}\"", escape_xml_attr(&message.from));
            if let Some(color) = &message.color {
                attrs.push_str(&format!(" color=\"{}\"", escape_xml_attr(color)));
            }
            if let Some(summary) = &message.summary {
                attrs.push_str(&format!(" summary=\"{}\"", escape_xml_attr(summary)));
            }
            format!(
                "<teammate-message{attrs}>\n{}\n</teammate-message>",
                message.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn should_skip_initial_mailbox_prompt(
    app_state: &HashMap<String, Value>,
    message: &Value,
    text: &str,
) -> bool {
    let Some(initial_prompt) = app_state
        .get("skip_initial_mailbox_prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    else {
        return false;
    };
    let is_initial_prompt = message
        .get("summary")
        .and_then(Value::as_str)
        .map(|summary| summary.eq_ignore_ascii_case("initial_prompt"))
        .unwrap_or(false);
    let from_lead = message
        .get("from")
        .and_then(Value::as_str)
        .map(|from| from.eq_ignore_ascii_case("team-lead"))
        .unwrap_or(false);
    is_initial_prompt && from_lead && text.trim() == initial_prompt
}

fn escape_xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn is_structured_protocol_message(text: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    matches!(
        value.get("type").and_then(Value::as_str),
        Some(
            "shutdown_request"
                | "shutdown_response"
                | "shutdown_approved"
                | "shutdown_rejected"
                | "permission_request"
                | "permission_response"
                | "sandbox_permission_request"
                | "sandbox_permission_response"
                | "plan_approval_request"
                | "plan_approval_response"
                | "team_permission_update"
                | "mode_set_request"
        )
    )
}

fn append_mailbox_context_to_messages(messages: &mut Vec<Message>, mailbox_context: String) {
    if let Some(last_user) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == "user")
    {
        append_text_to_message_content(&mut last_user.content, &mailbox_context);
        return;
    }
    messages.push(Message {
        role: "user".to_string(),
        content: Value::String(mailbox_context),
    });
}

async fn session_start_contexts(
    tool_context: &ToolContext,
    query_source: &str,
) -> Result<Vec<String>> {
    let mut app_state = tool_context.app_state.clone();
    app_state
        .entry("cwd".to_string())
        .or_insert_with(|| json!(tool_context.cwd.clone()));
    let permission_mode = app_state
        .get("permission_mode")
        .or_else(|| app_state.get("permissionMode"))
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    kiana_query::run_session_start_hooks(kiana_query::SessionStartHookContext {
        abort_signal: std::sync::Arc::new(tokio::sync::Notify::new()),
        cwd: PathBuf::from(&tool_context.cwd),
        project_trust: kiana_types::project_trust_from_app_state(&app_state),
        permission_mode,
        query_source: query_source.to_string(),
    })
    .await
    .map_err(|error| anyhow!("SessionStart hook failed: {error}"))
}

async fn user_prompt_submit_hook_result(
    tool_context: &ToolContext,
    query_source: &str,
    messages: &[Message],
) -> Result<kiana_query::UserPromptSubmitHookResult> {
    let mut app_state = tool_context.app_state.clone();
    app_state
        .entry("cwd".to_string())
        .or_insert_with(|| json!(tool_context.cwd.clone()));
    let permission_mode = app_state
        .get("permission_mode")
        .or_else(|| app_state.get("permissionMode"))
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    kiana_query::run_user_prompt_submit_hooks(kiana_query::UserPromptSubmitHookContext {
        abort_signal: std::sync::Arc::new(tokio::sync::Notify::new()),
        cwd: PathBuf::from(&tool_context.cwd),
        project_trust: kiana_types::project_trust_from_app_state(&app_state),
        permission_mode,
        query_source: query_source.to_string(),
        user_prompt: submitted_user_prompt_text(messages),
    })
    .await
    .map_err(|error| anyhow!("UserPromptSubmit hook failed: {error}"))
}

fn apply_user_prompt_submit_update(
    messages: &mut [Message],
    result: &kiana_query::UserPromptSubmitHookResult,
) {
    let Some(updated_prompt) = result.updated_prompt.as_ref() else {
        return;
    };
    if let Some(message) = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == "user")
    {
        message.content = Value::String(updated_prompt.clone());
    }
}

fn append_session_start_contexts_to_system_prompt(
    system_prompt: Option<Value>,
    contexts: Vec<String>,
) -> Option<Value> {
    if contexts.is_empty() {
        return system_prompt;
    }
    let hook_context = format!(
        "# SessionStart Hook Context\n\n{}",
        contexts.join("\n\n").trim()
    );

    match system_prompt {
        Some(Value::String(system)) if !system.trim().is_empty() => Some(Value::String(format!(
            "{}\n\n{}",
            system.trim(),
            hook_context
        ))),
        Some(Value::String(_)) | None => Some(Value::String(hook_context)),
        Some(other) => Some(Value::String(format!("{}\n\n{}", other, hook_context))),
    }
}

fn append_user_prompt_submit_contexts_to_system_prompt(
    system_prompt: Option<Value>,
    contexts: Vec<String>,
) -> Option<Value> {
    if contexts.is_empty() {
        return system_prompt;
    }
    let hook_context = format!(
        "# UserPromptSubmit Hook Context\n\n{}",
        contexts.join("\n\n").trim()
    );

    match system_prompt {
        Some(Value::String(system)) if !system.trim().is_empty() => Some(Value::String(format!(
            "{}\n\n{}",
            system.trim(),
            hook_context
        ))),
        Some(Value::String(_)) | None => Some(Value::String(hook_context)),
        Some(other) => Some(Value::String(format!("{}\n\n{}", other, hook_context))),
    }
}

fn submitted_user_prompt_text(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "user")
        .map(|message| content_text_for_compaction(&message.content))
        .unwrap_or_default()
}

fn append_text_to_message_content(content: &mut Value, text: &str) {
    match content {
        Value::String(existing) => {
            existing.push_str("\n\n");
            existing.push_str(text);
        }
        Value::Array(items) => {
            items.push(json!({
                "type": "text",
                "text": text
            }));
        }
        other => {
            let original = other.clone();
            *other = json!([
                {
                    "type": "text",
                    "text": original.to_string()
                },
                {
                    "type": "text",
                    "text": text
                }
            ]);
        }
    }
}

fn text_from_content(content: &[Value]) -> String {
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn tool_uses_from_content(content: &[Value]) -> Vec<ToolUseBlock> {
    content
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        .filter_map(|block| {
            Some(ToolUseBlock {
                id: block.get("id")?.as_str()?.to_string(),
                name: block.get("name")?.as_str()?.to_string(),
                input: block.get("input").cloned().unwrap_or_else(|| json!({})),
            })
        })
        .collect()
}

fn tool_result_event_content(result: &ToolExecutionResult) -> String {
    let content = result.api_result.get("content").unwrap_or(&result.content);
    tool_result_value_text(content)
}

fn tool_result_value_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(blocks) = value.as_array() {
        let text_blocks = blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>();
        if !text_blocks.is_empty() {
            return text_blocks.join("\n");
        }
    }
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

async fn collect_streaming_content<F>(
    stream: &mut ProviderStream,
    on_stream_event: &mut F,
) -> Result<Vec<Value>>
where
    F: FnMut(StreamEvent) -> Result<()>,
{
    let mut blocks: Vec<Option<Value>> = Vec::new();
    let mut tool_input_json: HashMap<usize, String> = HashMap::new();

    while let Some(event) = stream.next().await {
        let event = event.map_err(anyhow::Error::from)?;
        on_stream_event(event.clone())?;
        match event {
            StreamEvent::MessageStart { .. }
            | StreamEvent::MessageDelta { .. }
            | StreamEvent::Ping
            | StreamEvent::MessageStop => {}
            StreamEvent::Error { error } => {
                return Err(anyhow!("stream error event: {}", error));
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                ensure_stream_block_slot(&mut blocks, index);
                blocks[index] = Some(match content_block {
                    StreamContentBlock::Text { text } => json!({
                        "type": "text",
                        "text": text,
                    }),
                    StreamContentBlock::Thinking {
                        thinking,
                        signature,
                    } => {
                        let mut block = json!({
                            "type": "thinking",
                            "thinking": thinking,
                        });
                        if let Some(signature) = signature {
                            block["signature"] = json!(signature);
                        }
                        block
                    }
                    StreamContentBlock::RedactedThinking { data } => json!({
                        "type": "redacted_thinking",
                        "data": data,
                    }),
                    StreamContentBlock::ToolUse(tool_use) => json!({
                        "type": "tool_use",
                        "id": tool_use.id,
                        "name": tool_use.name,
                        "input": tool_use.input,
                    }),
                });
            }
            StreamEvent::ContentBlockDelta { index, delta } => match delta {
                Delta::TextDelta { text } => {
                    let block = stream_block_mut(&mut blocks, index)?;
                    let Some(Value::String(existing)) = block.get_mut("text") else {
                        return Err(anyhow!(
                            "stream text delta arrived for non-text content block {}",
                            index
                        ));
                    };
                    existing.push_str(&text);
                }
                Delta::InputJsonDelta { partial_json } => {
                    tool_input_json
                        .entry(index)
                        .or_default()
                        .push_str(&partial_json);
                }
                Delta::ThinkingDelta { thinking } => {
                    let block = stream_block_mut(&mut blocks, index)?;
                    let Some(Value::String(existing)) = block.get_mut("thinking") else {
                        return Err(anyhow!(
                            "stream thinking delta arrived for non-thinking content block {}",
                            index
                        ));
                    };
                    existing.push_str(&thinking);
                }
                Delta::SignatureDelta { signature } => {
                    let block = stream_block_mut(&mut blocks, index)?;
                    if block.get("type").and_then(Value::as_str) != Some("thinking") {
                        return Err(anyhow!(
                            "stream signature delta arrived for non-thinking content block {}",
                            index
                        ));
                    }
                    match block.get_mut("signature") {
                        Some(Value::String(existing)) => existing.push_str(&signature),
                        Some(_) => {
                            return Err(anyhow!(
                                "stream signature delta arrived for invalid signature block {}",
                                index
                            ));
                        }
                        None => {
                            block["signature"] = json!(signature);
                        }
                    }
                }
            },
            StreamEvent::ContentBlockStop { index } => {
                if let Some(input_json) = tool_input_json.remove(&index) {
                    let input = if input_json.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str::<Value>(&input_json).map_err(|error| {
                            anyhow!(
                                "stream tool input block {} is invalid JSON: {}",
                                index,
                                error
                            )
                        })?
                    };
                    let block = stream_block_mut(&mut blocks, index)?;
                    if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                        block["input"] = input;
                    }
                }
            }
        }
    }

    Ok(blocks.into_iter().flatten().collect())
}

fn ensure_stream_block_slot(blocks: &mut Vec<Option<Value>>, index: usize) {
    if blocks.len() <= index {
        blocks.resize_with(index + 1, || None);
    }
}

fn stream_block_mut(blocks: &mut [Option<Value>], index: usize) -> Result<&mut Value> {
    blocks
        .get_mut(index)
        .and_then(Option::as_mut)
        .ok_or_else(|| anyhow!("stream delta arrived before content block {}", index))
}

fn string_option(options: &HashMap<String, Value>, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|value| !value.trim().is_empty())
}

fn first_string_option(options: &HashMap<String, Value>, keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| {
        string_option(options, key)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn first_env_string(keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn provider_id_option(options: &HashMap<String, Value>) -> String {
    string_option(options, "provider")
        .or_else(|| string_option(options, "provider_id"))
        .or_else(|| string_option(options, "providerId"))
        .or_else(|| std::env::var("KIANA_PROVIDER").ok())
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ANTHROPIC_PROVIDER_ID.to_string())
}

fn model_option(
    options: &HashMap<String, Value>,
    config: &kiana_bootstrap::config::Config,
    provider_id: &str,
) -> String {
    let Some(entry) = provider_registry_entry(provider_id) else {
        return string_option(options, "model").unwrap_or_else(|| config.model.clone());
    };
    first_string_option(options, &entry.model_option_aliases)
        .or_else(|| first_env_string(&entry.model_env_vars))
        .unwrap_or_else(|| {
            if provider_id == ANTHROPIC_PROVIDER_ID {
                config.model.clone()
            } else {
                entry.default_model_id
            }
        })
}

fn build_provider(
    provider_id: &str,
    model: &str,
    options: &HashMap<String, Value>,
    config: &kiana_bootstrap::config::Config,
    api_timeout: Duration,
) -> Result<Box<dyn Provider>> {
    let entry = provider_registry_entry(provider_id)
        .ok_or_else(|| anyhow!("unknown provider: {provider_id}"))?;
    match entry.protocol {
        ProviderProtocol::Fake => Ok(Box::new(
            FakeProvider::from_script_value(
                model.to_string(),
                options
                    .get("fake_provider_script")
                    .or_else(|| options.get("fakeProviderScript")),
            )
            .map_err(anyhow::Error::from)?,
        )),
        ProviderProtocol::AnthropicMessages => {
            let api_key = first_string_option(options, &entry.api_key_option_aliases)
                .or_else(|| config.api_key.clone())
                .or_else(|| first_env_string(&entry.api_key_env_vars))
                .ok_or_else(|| {
                    anyhow!(
                        "API key not found. Set {}, configure api_key, or pass api_key.",
                        entry.api_key_env_vars.join(" or ")
                    )
                })?;
            let base_url = first_string_option(options, &entry.base_url_option_aliases)
                .or_else(|| first_env_string(&entry.base_url_env_vars))
                .or_else(|| config.base_url.clone())
                .or(entry.default_base_url)
                .unwrap_or_default();
            Ok(Box::new(AnthropicProvider::new(
                api_key,
                base_url,
                api_timeout,
            )))
        }
        ProviderProtocol::OpenAiChatCompletions => {
            let api_key = first_string_option(options, &entry.api_key_option_aliases)
                .or_else(|| first_env_string(&entry.api_key_env_vars))
                .ok_or_else(|| {
                    anyhow!(
                        "OpenAI-compatible API key not found. Set {}, or pass {}.",
                        entry.api_key_env_vars.join(" or "),
                        entry.api_key_option_aliases.join(" or ")
                    )
                })?;
            let base_url = first_string_option(options, &entry.base_url_option_aliases)
                .or_else(|| first_env_string(&entry.base_url_env_vars))
                .or(entry.default_base_url)
                .unwrap_or_default();
            Ok(Box::new(OpenAiCompatibleProvider::new(
                api_key,
                base_url,
                api_timeout,
            )?))
        }
        ProviderProtocol::OllamaChat => {
            let base_url = first_string_option(options, &entry.base_url_option_aliases)
                .or_else(|| first_env_string(&entry.base_url_env_vars))
                .or(entry.default_base_url)
                .unwrap_or_default();
            Ok(Box::new(OllamaProvider::new(base_url, api_timeout)?))
        }
    }
}

fn create_assistant_turn_checkpoint_if_requested(
    options: &HashMap<String, Value>,
    cwd: &str,
) -> Option<PathBuf> {
    let Some(session_id) = string_option(options, "session_id") else {
        return None;
    };
    let Some(turn_id) = string_option(options, "turn_id") else {
        return None;
    };
    kiana_commands::checkpoint::create_assistant_turn_checkpoint(
        &PathBuf::from(cwd),
        &session_id,
        &turn_id,
    )
    .ok()
}

fn record_assistant_turn_final_state_if_requested(checkpoint_manifest: Option<&Path>, cwd: &str) {
    let Some(checkpoint_manifest) = checkpoint_manifest else {
        return;
    };
    let _ = kiana_commands::checkpoint::record_assistant_turn_final_state(
        &PathBuf::from(cwd),
        checkpoint_manifest,
    );
}

fn result_bool(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn app_state_bool(app_state: &HashMap<String, Value>, key: &str) -> bool {
    app_state.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn fallback_model_option(options: &HashMap<String, Value>) -> Option<String> {
    string_option(options, "fallback_model")
        .or_else(|| string_option(options, "fallbackModel"))
        .or_else(|| std::env::var("KIANA_FALLBACK_MODEL").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn permission_prompt_tool_option(options: &HashMap<String, Value>) -> Option<String> {
    string_option(options, "permission_prompt_tool")
        .or_else(|| string_option(options, "permissionPromptTool"))
        .or_else(|| std::env::var(PERMISSION_PROMPT_TOOL_ENV).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn should_retry_with_fallback(
    error: &anyhow::Error,
    fallback_model: Option<&str>,
    fallback_used: bool,
) -> bool {
    if fallback_used || fallback_model.is_none() {
        return false;
    }
    if error
        .downcast_ref::<ApiError>()
        .is_some_and(|error| error.kind == ApiErrorKind::ServerOverload)
    {
        return true;
    }
    error
        .downcast_ref::<ProviderError>()
        .is_some_and(|error| matches!(error, ProviderError::Api(api) if matches!(&api.kind, ApiErrorKind::ServerOverload)))
}

fn structured_output_schema_option(options: &HashMap<String, Value>) -> Result<Option<Value>> {
    let Some(schema) = options
        .get("structured_output_schema")
        .or_else(|| options.get("json_schema"))
    else {
        return Ok(None);
    };

    if let Some(schema_text) = schema.as_str() {
        let parsed: Value = serde_json::from_str(schema_text)
            .map_err(|error| anyhow!("json_schema is not valid JSON: {}", error))?;
        return Ok(Some(parsed));
    }

    Ok(Some(schema.clone()))
}

fn max_iterations_option(options: &HashMap<String, Value>) -> usize {
    options
        .get("max_iterations")
        .or_else(|| options.get("maxIterations"))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .or_else(|| {
            std::env::var("KIANA_MAX_ITERATIONS")
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|value| *value > 0)
        })
        .unwrap_or(8)
}

fn repair_checks_option(options: &HashMap<String, Value>) -> bool {
    bool_option(options, "repair_checks")
        .or_else(|| bool_option(options, "repairChecks"))
        .unwrap_or(false)
}

fn repair_check_attempts_option(options: &HashMap<String, Value>) -> Result<u32> {
    Ok(first_u32_option(
        options,
        &[
            "repair_check_attempts",
            "repairCheckAttempts",
            "max_repair_attempts",
            "maxRepairAttempts",
        ],
    )?
    .unwrap_or(1))
}

fn thinking_option(options: &HashMap<String, Value>) -> Result<Option<Value>> {
    let budget = first_nonnegative_u64_option(
        options,
        &[
            "max_thinking_tokens",
            "maxThinkingTokens",
            "thinking_budget_tokens",
            "thinkingBudgetTokens",
        ],
    )?
    .or(first_nonnegative_env_u64(&[
        "KIANA_MAX_THINKING_TOKENS",
        "ANTHROPIC_MAX_THINKING_TOKENS",
    ])?)
    .flatten();

    Ok(budget
        .filter(|budget| *budget > 0)
        .map(|budget| json!({ "type": "enabled", "budget_tokens": budget })))
}

fn max_tokens_option(options: &HashMap<String, Value>, thinking: Option<&Value>) -> Result<u32> {
    let configured = first_u32_option(options, &["max_tokens", "maxTokens"])?;
    let configured_from_env = configured.is_none()
        && first_present_env(&[
            "KIANA_MAX_TOKENS",
            "ANTHROPIC_MAX_TOKENS",
            "MAX_OUTPUT_TOKENS",
        ]);
    let mut max_tokens = match configured {
        Some(value) => value,
        None => first_env_u32(&[
            "KIANA_MAX_TOKENS",
            "ANTHROPIC_MAX_TOKENS",
            "MAX_OUTPUT_TOKENS",
        ])?
        .unwrap_or(DEFAULT_MAX_TOKENS),
    };

    if let Some(budget_tokens) = thinking_budget_tokens(thinking) {
        let max_tokens_was_configured = configured.is_some() || configured_from_env;
        if u64::from(max_tokens) <= budget_tokens {
            if max_tokens_was_configured {
                return Err(anyhow!(
                    "max_tokens must be greater than max_thinking_tokens when thinking is enabled"
                ));
            }
            let adjusted = budget_tokens
                .checked_add(THINKING_RESPONSE_TOKEN_RESERVE)
                .ok_or_else(|| anyhow!("max_thinking_tokens is too large"))?;
            max_tokens =
                u32::try_from(adjusted).map_err(|_| anyhow!("max_thinking_tokens is too large"))?;
        }
    }

    Ok(max_tokens)
}

fn thinking_budget_tokens(thinking: Option<&Value>) -> Option<u64> {
    thinking?
        .get("budget_tokens")
        .and_then(Value::as_u64)
        .filter(|budget| *budget > 0)
}

fn first_present_env(names: &[&str]) -> bool {
    names.iter().any(|name| {
        std::env::var(name)
            .ok()
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn api_timeout_option(
    options: &HashMap<String, Value>,
    config_timeout_ms: Option<u64>,
) -> Result<Duration> {
    if let Some(timeout_ms) = first_u64_option(
        options,
        &["api_timeout_ms", "apiTimeoutMs", "timeout_ms", "timeoutMs"],
    )? {
        return Ok(Duration::from_millis(timeout_ms));
    }
    if let Some(timeout_seconds) = first_u64_option(
        options,
        &[
            "api_timeout",
            "apiTimeout",
            "api_timeout_seconds",
            "apiTimeoutSeconds",
        ],
    )? {
        return Ok(Duration::from_secs(timeout_seconds));
    }
    if let Some(timeout_ms) = first_env_u64(&["KIANA_API_TIMEOUT_MS", "ANTHROPIC_API_TIMEOUT_MS"])?
    {
        return Ok(Duration::from_millis(timeout_ms));
    }
    if let Some(timeout_seconds) = first_env_u64(&[
        "KIANA_API_TIMEOUT",
        "KIANA_API_TIMEOUT_SECONDS",
        "ANTHROPIC_API_TIMEOUT",
        "ANTHROPIC_API_TIMEOUT_SECONDS",
    ])? {
        return Ok(Duration::from_secs(timeout_seconds));
    }
    if let Some(timeout_ms) = config_timeout_ms.filter(|value| *value > 0) {
        return Ok(Duration::from_millis(timeout_ms));
    }
    Ok(Duration::from_secs(600))
}

fn compaction_config_option(options: &HashMap<String, Value>) -> Result<CompactionConfig> {
    let mut config = CompactionConfig::default();

    if let Some(enabled) = bool_option(options, "auto_compact")
        .or_else(|| bool_option(options, "autoCompact"))
        .or_else(|| env_bool("KIANA_AUTO_COMPACT"))
    {
        config.enabled = enabled;
    }
    if bool_option(options, "no_auto_compact")
        .or_else(|| bool_option(options, "noAutoCompact"))
        .or_else(|| env_bool("KIANA_NO_AUTO_COMPACT"))
        .unwrap_or(false)
    {
        config.enabled = false;
    }

    if let Some(threshold) = first_u32_option(
        options,
        &[
            "compact_threshold_tokens",
            "compactThresholdTokens",
            "auto_compact_threshold_tokens",
            "autoCompactThresholdTokens",
        ],
    )?
    .or(first_env_u32(&[
        "KIANA_COMPACT_THRESHOLD_TOKENS",
        "KIANA_AUTO_COMPACT_THRESHOLD_TOKENS",
    ])?) {
        config.threshold_tokens = threshold;
    }

    if let Some(target) = first_u32_option(
        options,
        &[
            "compact_target_tokens",
            "compactTargetTokens",
            "auto_compact_target_tokens",
            "autoCompactTargetTokens",
        ],
    )?
    .or(first_env_u32(&[
        "KIANA_COMPACT_TARGET_TOKENS",
        "KIANA_AUTO_COMPACT_TARGET_TOKENS",
    ])?) {
        config.target_tokens = target;
    }

    Ok(config)
}

fn first_u32_option(options: &HashMap<String, Value>, keys: &[&str]) -> Result<Option<u32>> {
    for key in keys {
        if let Some(value) = options.get(*key) {
            return parse_positive_u32_value(key, value).map(Some);
        }
    }
    Ok(None)
}

fn first_u64_option(options: &HashMap<String, Value>, keys: &[&str]) -> Result<Option<u64>> {
    for key in keys {
        if let Some(value) = options.get(*key) {
            return parse_positive_u64_value(key, value).map(Some);
        }
    }
    Ok(None)
}

fn first_nonnegative_u64_option(
    options: &HashMap<String, Value>,
    keys: &[&str],
) -> Result<Option<Option<u64>>> {
    for key in keys {
        if let Some(value) = options.get(*key) {
            if value.is_null() {
                return Ok(Some(None));
            }
            return parse_nonnegative_u64_value(key, value).map(|value| Some(Some(value)));
        }
    }
    Ok(None)
}

fn first_env_u32(names: &[&str]) -> Result<Option<u32>> {
    for name in names {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        return value
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{name} must be a positive integer"))
            .map(Some);
    }
    Ok(None)
}

fn first_nonnegative_env_u64(names: &[&str]) -> Result<Option<Option<u64>>> {
    for name in names {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        return value
            .parse::<u64>()
            .map(Some)
            .map_err(|_| anyhow!("{name} must be a non-negative integer"))
            .map(Some);
    }
    Ok(None)
}

fn first_env_u64(names: &[&str]) -> Result<Option<u64>> {
    for name in names {
        let Ok(value) = std::env::var(name) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        return value
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{name} must be a positive integer"))
            .map(Some);
    }
    Ok(None)
}

fn parse_positive_u32_value(key: &str, value: &Value) -> Result<u32> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{key} must be a positive integer")),
        Value::String(text) => text
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{key} must be a positive integer")),
        _ => Err(anyhow!("{key} must be a positive integer")),
    }
}

fn parse_nonnegative_u64_value(key: &str, value: &Value) -> Result<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .ok_or_else(|| anyhow!("{key} must be a non-negative integer")),
        Value::String(text) => text
            .trim()
            .parse::<u64>()
            .ok()
            .ok_or_else(|| anyhow!("{key} must be a non-negative integer")),
        _ => Err(anyhow!("{key} must be a non-negative integer")),
    }
}

fn parse_positive_u64_value(key: &str, value: &Value) -> Result<u64> {
    match value {
        Value::Number(number) => number
            .as_u64()
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{key} must be a positive integer")),
        Value::String(text) => text
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| anyhow!("{key} must be a positive integer")),
        _ => Err(anyhow!("{key} must be a positive integer")),
    }
}

fn system_prompt_option(options: &HashMap<String, Value>) -> Option<Value> {
    let base = string_option(options, "system_prompt")
        .or_else(|| string_option(options, "systemPrompt"))
        .or_else(|| std::env::var("KIANA_SYSTEM_PROMPT").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let append = string_option(options, "append_system_prompt")
        .or_else(|| string_option(options, "appendSystemPrompt"))
        .or_else(|| std::env::var("KIANA_APPEND_SYSTEM_PROMPT").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match (base, append) {
        (Some(base), Some(append)) => Some(Value::String(format!("{base}\n\n{append}"))),
        (Some(base), None) => Some(Value::String(base)),
        (None, Some(append)) => Some(Value::String(append)),
        (None, None) => None,
    }
}

fn tool_filter_option(
    options: &HashMap<String, Value>,
    registry: &kiana_tools::ToolRegistry,
) -> Result<Option<HashSet<String>>> {
    let available: HashMap<String, String> = registry
        .list_tools()
        .into_iter()
        .map(|tool| (tool.name().to_ascii_lowercase(), tool.name().to_string()))
        .collect();

    let Some(raw) = tools_option_value(options).or_else(|| std::env::var("KIANA_TOOLS").ok())
    else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("default") || raw.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    if raw.eq_ignore_ascii_case("core") || raw.eq_ignore_ascii_case("safe") {
        return Ok(Some(core_model_tool_filter(&available)));
    }
    if raw.is_empty() {
        return Ok(Some(HashSet::new()));
    }

    let mut enabled = HashSet::new();
    for token in raw
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        let Some(actual_name) = available.get(&token.to_ascii_lowercase()) else {
            return Err(anyhow!("unknown tool in --tools: {token}"));
        };
        enabled.insert(actual_name.clone());
    }
    Ok(Some(enabled))
}

fn provider_default_tool_filter(
    enabled_tools: Option<HashSet<String>>,
    options: &HashMap<String, Value>,
    provider: &dyn Provider,
    model: &str,
) -> Option<HashSet<String>> {
    if enabled_tools.is_none()
        && !tools_are_explicitly_configured(options)
        && !provider.model_profile(model).supports_tools
    {
        return Some(HashSet::new());
    }
    enabled_tools
}

fn tools_are_explicitly_configured(options: &HashMap<String, Value>) -> bool {
    tools_option_value(options).is_some() || std::env::var("KIANA_TOOLS").is_ok()
}

fn core_model_tool_filter(available: &HashMap<String, String>) -> HashSet<String> {
    [
        "Read",
        "Write",
        "Edit",
        "Glob",
        "Grep",
        "Bash",
        "TodoWrite",
        "WebFetch",
        "WebSearch",
        "NotebookEdit",
    ]
    .into_iter()
    .filter_map(|name| available.get(&name.to_ascii_lowercase()).cloned())
    .collect()
}

fn tools_option_value(options: &HashMap<String, Value>) -> Option<String> {
    let value = options
        .get("tools")
        .or_else(|| options.get("enabled_tools"))?;
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(items) = value.as_array() {
        return Some(
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" "),
        );
    }
    None
}

fn access_roots_option(options: &HashMap<String, Value>) -> Option<Vec<String>> {
    string_list_option(
        options,
        &[
            "add_dirs",
            "additional_directories",
            "additionalDirectories",
        ],
    )
}

fn seed_file_set_options(app_state: &mut HashMap<String, Value>, options: &HashMap<String, Value>) {
    if let Some(files) = string_list_option(options, &["editable_files", "editableFiles"]) {
        app_state.insert(EDITABLE_FILES_APP_STATE_KEY.to_string(), json!(files));
    }
    if let Some(files) = string_list_option(
        options,
        &[
            "read_only_files",
            "readOnlyFiles",
            "readonly_files",
            "readonlyFiles",
        ],
    ) {
        app_state.insert(READ_ONLY_FILES_APP_STATE_KEY.to_string(), json!(files));
    }
}

fn string_list_option(options: &HashMap<String, Value>, keys: &[&str]) -> Option<Vec<String>> {
    let value = keys.iter().find_map(|key| options.get(*key))?;
    let values = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Value::String(value) => value
            .split(|ch: char| ch == ',' || ch == '\n')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn mcp_servers_option(options: &HashMap<String, Value>) -> Result<Option<Value>> {
    if let Some(value) = options
        .get(MCP_SERVERS_APP_STATE_KEY)
        .or_else(|| options.get("mcpServers"))
    {
        return normalize_mcp_servers_value(value.clone()).map(Some);
    }
    if let Some(value) = options
        .get("mcp_config")
        .or_else(|| options.get("mcpConfig"))
    {
        return normalize_mcp_config_value(value.clone()).map(Some);
    }
    if bool_option(options, "strict_mcp_config")
        .or_else(|| bool_option(options, "strictMcpConfig"))
        .or_else(|| env_bool("KIANA_STRICT_MCP_CONFIG"))
        .unwrap_or(false)
        || simple_mode_option(options)
    {
        return Ok(Some(json!({})));
    }
    let Ok(raw) = std::env::var(MCP_SERVERS_ENV) else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&raw)
        .map_err(|error| anyhow!("{MCP_SERVERS_ENV} is not valid JSON: {error}"))?;
    normalize_mcp_servers_value(value).map(Some)
}

fn sandbox_option(
    options: &HashMap<String, Value>,
    config_sandbox: Option<Value>,
) -> Result<Option<Value>> {
    if let Some(value) = options
        .get("sandbox")
        .or_else(|| options.get("bash_sandbox"))
        .or_else(|| options.get("bashSandbox"))
    {
        return normalize_sandbox_value(value.clone()).map(Some);
    }
    if let Some(value) = config_sandbox {
        return normalize_sandbox_value(value).map(Some);
    }
    Ok(None)
}

fn normalize_sandbox_value(value: Value) -> Result<Value> {
    match value {
        Value::Object(_) => Ok(value),
        Value::Bool(enabled) => Ok(json!({ "enabled": enabled })),
        _ => Err(anyhow!("sandbox option must be an object or boolean")),
    }
}

fn normalize_mcp_config_value(value: Value) -> Result<Value> {
    let Some(object) = value.as_object() else {
        return Err(anyhow!("mcp_config must be a JSON object"));
    };
    if let Some(servers) = object
        .get("mcpServers")
        .or_else(|| object.get(MCP_SERVERS_APP_STATE_KEY))
    {
        return normalize_mcp_servers_value(servers.clone());
    }
    normalize_mcp_servers_value(value)
}

fn normalize_mcp_servers_value(value: Value) -> Result<Value> {
    match value {
        Value::Object(object) => Ok(Value::Object(object)),
        Value::Array(items) => Ok(Value::Array(items)),
        Value::Null => Ok(Value::Null),
        _ => Err(anyhow!("MCP servers config must be an object or array")),
    }
}

fn bool_option(options: &HashMap<String, Value>, key: &str) -> Option<bool> {
    match options.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(value) => parse_bool(value),
        _ => None,
    }
}

fn env_bool(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_bool(&value))
}

fn simple_mode_option(options: &HashMap<String, Value>) -> bool {
    bool_option(options, "bare")
        .or_else(|| bool_option(options, "simple"))
        .or_else(|| bool_option(options, "simple_mode"))
        .or_else(|| bool_option(options, "simpleMode"))
        .or_else(|| env_bool("CLAUDE_CODE_SIMPLE"))
        .or_else(|| env_bool("KIANA_CODE_SIMPLE"))
        .unwrap_or(false)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn filtered_tool_schemas(
    registry: &kiana_tools::ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
) -> Vec<Value> {
    registry
        .get_schemas()
        .into_iter()
        .filter(|schema| {
            let Some(enabled_tools) = enabled_tools else {
                return true;
            };
            schema
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| enabled_tools.contains(name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::{get, post},
        Json, Router,
    };
    use futures::SinkExt;
    use kiana_tools::tool_execution::{PermissionPromptDecision, PermissionPromptRequest};
    use std::collections::HashSet;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;
    use tokio::task::JoinHandle;
    use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};

    fn isolated_tasks_root(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("kiana-runner-{name}-{}", uuid::Uuid::new_v4()))
            .to_string_lossy()
            .to_string()
    }

    fn run_git(root: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout={}\nstderr={}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn clear_team_env() {
        for key in [
            "KIANA_AGENT_ID",
            "KIANA_AGENT_NAME",
            "KIANA_AGENT_COLOR",
            "KIANA_TEAM_NAME",
            "KIANA_TASK_LIST_ID",
            "KIANA_TEAM_FILE",
            "KIANA_TEAM_MAILBOX",
            "KIANA_TEAMS_ROOT",
            "KIANA_TASKS_ROOT",
            "KIANA_PLAN_MODE_REQUIRED",
            "CLAUDE_CODE_AGENT_ID",
            "CLAUDE_CODE_AGENT_NAME",
            "CLAUDE_CODE_TEAM_NAME",
            "CLAUDE_CODE_TASK_LIST_ID",
        ] {
            std::env::remove_var(key);
        }
    }

    fn clear_api_timeout_env() {
        for key in [
            "KIANA_API_TIMEOUT_MS",
            "KIANA_API_TIMEOUT",
            "KIANA_API_TIMEOUT_SECONDS",
            "ANTHROPIC_API_TIMEOUT_MS",
            "ANTHROPIC_API_TIMEOUT",
            "ANTHROPIC_API_TIMEOUT_SECONDS",
        ] {
            std::env::remove_var(key);
        }
    }

    fn clear_thinking_env() {
        std::env::remove_var("KIANA_MAX_THINKING_TOKENS");
        std::env::remove_var("ANTHROPIC_MAX_THINKING_TOKENS");
    }

    fn clear_max_tokens_env() {
        std::env::remove_var("KIANA_MAX_TOKENS");
        std::env::remove_var("ANTHROPIC_MAX_TOKENS");
        std::env::remove_var("MAX_OUTPUT_TOKENS");
    }

    fn clear_openai_compatible_env() {
        for key in [
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_BASE_URL",
            "OPENAI_BASE_URL",
            "KIANA_OPENAI_MODEL",
            "OPENAI_MODEL",
        ] {
            std::env::remove_var(key);
        }
    }

    fn clear_ollama_env() {
        for key in [
            "KIANA_OLLAMA_BASE_URL",
            "OLLAMA_BASE_URL",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn extracts_text_and_tool_uses_from_content_blocks() {
        let content = vec![
            json!({"type": "text", "text": "hello "}),
            json!({"type": "text", "text": "world"}),
            json!({
                "type": "tool_use",
                "id": "toolu_1",
                "name": "TaskCreate",
                "input": { "title": "inspect" }
            }),
        ];

        assert_eq!(text_from_content(&content), "hello world");
        assert_eq!(
            tool_uses_from_content(&content),
            vec![ToolUseBlock {
                id: "toolu_1".to_string(),
                name: "TaskCreate".to_string(),
                input: json!({ "title": "inspect" }),
            }]
        );
    }

    #[test]
    fn runner_runtime_event_adapter_emits_assistant_stream_delta() {
        let events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-1",
            None,
            0,
            "2026-06-23T00:00:00Z",
            RunnerStreamEvent::Model(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::TextDelta {
                    text: "hi".to_string(),
                },
            }),
        );

        assert_eq!(events.len(), 1);
        let value = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["type"], "stream_delta");
        assert_eq!(value["delta"]["type"], "content_block_delta");
        assert_eq!(value["delta"]["index"], 0);
        assert_eq!(value["delta"]["delta"]["type"], "text_delta");
        assert_eq!(value["delta"]["delta"]["text"], "hi");
    }

    #[test]
    fn runner_runtime_event_adapter_emits_tool_call_from_stream_start() {
        let events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-1",
            None,
            1,
            "2026-06-23T00:00:00Z",
            RunnerStreamEvent::Model(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: StreamContentBlock::ToolUse(
                    kiana_services::api::streaming::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "Read".to_string(),
                        input: json!({"file_path": "README.md"}),
                    },
                ),
            }),
        );

        assert_eq!(events.len(), 1);
        let value = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["type"], "tool_call");
        assert_eq!(value["tool_call_id"], "toolu_1");
        assert_eq!(value["name"], "Read");
        assert_eq!(value["input"]["file_path"], "README.md");
    }

    #[test]
    fn runner_runtime_event_adapter_emits_local_tool_result() {
        let events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-23T00:00:01Z",
            RunnerStreamEvent::ToolResult {
                id: "toolu_1".to_string(),
                name: "Read".to_string(),
                is_error: false,
                content: "file body".to_string(),
                error: None,
            },
        );

        assert_eq!(events.len(), 1);
        let value = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["type"], "tool_result");
        assert_eq!(value["parent_turn_id"], "turn-1");
        assert_eq!(value["tool_call_id"], "toolu_1");
        assert_eq!(value["name"], "Read");
        assert_eq!(value["is_error"], false);
        assert_eq!(value["content"], "file body");
    }

    #[test]
    fn runner_runtime_event_adapter_enriches_workbench_lifecycle_events() {
        let call_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-1",
            None,
            1,
            "2026-06-24T00:00:00Z",
            RunnerStreamEvent::Model(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: StreamContentBlock::ToolUse(
                    kiana_services::api::streaming::ToolUse {
                        id: "toolu_mcp".to_string(),
                        name: "MCP".to_string(),
                        input: json!({"server": "docs", "tool": "search"}),
                    },
                ),
            }),
        );
        let call = serde_json::to_value(&call_events[0]).unwrap();
        assert_eq!(call["type"], "tool_call");
        assert_eq!(call["workbench"], "mcp");

        let result_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-24T00:00:01Z",
            RunnerStreamEvent::ToolResult {
                id: "toolu_mcp".to_string(),
                name: "MCP".to_string(),
                is_error: false,
                content: "ok".to_string(),
                error: None,
            },
        );
        let result = serde_json::to_value(&result_events[0]).unwrap();
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["workbench"], "mcp");
    }

    #[test]
    fn runner_runtime_event_adapter_emits_permission_request_from_prompt() {
        let event = runtime_event_from_permission_prompt_request(
            "session-1",
            "turn-3",
            Some("turn-2".to_string()),
            3,
            "2026-06-23T00:00:02Z",
            &PermissionPromptRequest {
                request_id: "perm-1".to_string(),
                tool_name: "Write".to_string(),
                input: json!({"file_path": "src/lib.rs"}),
                tool_use_id: "toolu_write".to_string(),
                permission_suggestions: json!([{"type": "addRules"}]),
                blocked_path: Some("src/lib.rs".to_string()),
                decision_reason: json!({
                    "type": "other",
                    "reason": "Tool Write requires permission in ask mode."
                }),
                agent_id: Some("researcher@review".to_string()),
            },
        );

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "permission_request");
        assert_eq!(value["request_id"], "perm-1");
        assert_eq!(value["tool_name"], "Write");
        assert_eq!(value["action"], "can_use_tool");
        assert_eq!(value["input"]["file_path"], "src/lib.rs");
        assert!(value["reason"]
            .as_str()
            .unwrap()
            .contains("blocked_path=src/lib.rs"));
    }

    #[test]
    fn runner_runtime_event_adapter_emits_stream_error() {
        let events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-4",
            Some("turn-3".to_string()),
            4,
            "2026-06-23T00:00:03Z",
            RunnerStreamEvent::Model(StreamEvent::Error {
                error: json!({
                    "type": "overloaded_error",
                    "message": "try again"
                }),
            }),
        );

        assert_eq!(events.len(), 1);
        let value = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["type"], "error");
        assert_eq!(value["code"], "overloaded_error");
        assert_eq!(value["message"], "try again");
        assert_eq!(value["details"]["type"], "overloaded_error");
    }

    #[test]
    fn runner_runtime_event_adapter_emits_turn_result() {
        let event = runtime_event_from_assistant_run_result(
            "session-1",
            "turn-5",
            Some("turn-4".to_string()),
            5,
            "2026-06-23T00:00:04Z",
            &AssistantRunResult {
                text: "done".to_string(),
                messages: vec![Message {
                    role: "assistant".to_string(),
                    content: json!([{"type": "text", "text": "done"}]),
                }],
                iterations: 2,
                stop_reason: "model_stop".to_string(),
                structured_output: Some(json!({"answer": "done"})),
                teammate_shutdown_approved: false,
            },
        );

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(value["type"], "result");
        assert_eq!(value["status"], "completed");
        assert_eq!(value["stop_reason"], "model_stop");
        assert_eq!(value["assistant_text"], "done");
        assert_eq!(value["metadata"]["iterations"], 2);
        assert_eq!(value["metadata"]["message_count"], 1);
        assert_eq!(value["metadata"]["structured_output"]["answer"], "done");
    }

    #[test]
    fn normalizes_persisted_session_messages() {
        let messages = normalize_messages(vec![json!({
            "role": "user",
            "content": "hello"
        })])
        .unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, json!("hello"));
    }

    #[tokio::test]
    async fn auto_compact_messages_replaces_middle_messages() {
        let messages = normalize_messages(vec![
            json!({"role": "user", "content": "first message ".repeat(80)}),
            json!({"role": "assistant", "content": "middle one ".repeat(80)}),
            json!({"role": "user", "content": "middle two ".repeat(80)}),
            json!({"role": "assistant", "content": "middle three ".repeat(80)}),
            json!({"role": "user", "content": "recent tail"}),
        ])
        .unwrap();

        let compacted = compact_messages_for_request(
            messages,
            &CompactionConfig {
                enabled: true,
                threshold_tokens: 10,
                target_tokens: 80,
            },
        )
        .await
        .unwrap();

        assert!(compacted.len() < 5);
        assert_eq!(compacted.first().unwrap().role, "user");
        assert_eq!(compacted.last().unwrap().content, json!("recent tail"));
        assert!(content_text_for_compaction(&compacted[1].content)
            .contains("[Compacted conversation summary]"));
    }

    #[test]
    fn compaction_config_accepts_options_and_env() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("KIANA_AUTO_COMPACT", "false");
        std::env::set_var("KIANA_COMPACT_THRESHOLD_TOKENS", "900");
        std::env::set_var("KIANA_COMPACT_TARGET_TOKENS", "450");

        let from_env = compaction_config_option(&HashMap::new()).unwrap();
        assert!(!from_env.enabled);
        assert_eq!(from_env.threshold_tokens, 900);
        assert_eq!(from_env.target_tokens, 450);

        let options = HashMap::from([
            ("autoCompact".to_string(), json!(true)),
            ("compact_threshold_tokens".to_string(), json!("300")),
            ("compactTargetTokens".to_string(), json!(150)),
        ]);
        let from_options = compaction_config_option(&options).unwrap();
        assert!(from_options.enabled);
        assert_eq!(from_options.threshold_tokens, 300);
        assert_eq!(from_options.target_tokens, 150);

        std::env::remove_var("KIANA_AUTO_COMPACT");
        std::env::remove_var("KIANA_COMPACT_THRESHOLD_TOKENS");
        std::env::remove_var("KIANA_COMPACT_TARGET_TOKENS");
    }

    #[tokio::test]
    async fn call_tool_enforces_permission_rules() {
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([("disallowed_tools".to_string(), json!(["TaskCreate"]))]),
            abort_signal: abort_rx,
        };

        let result = call_tool(
            &registry,
            None,
            &mut context,
            "TaskCreate",
            &json!({ "title": "blocked" }),
            None,
            None,
        )
        .await;

        assert!(result.is_error);
        assert!(result.content.as_str().unwrap().contains("denied"));
    }

    #[tokio::test]
    async fn call_tool_sends_teammate_permission_request_on_ask() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env();
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let root = std::env::temp_dir().join(format!(
            "kiana-runner-permission-request-{}",
            uuid::Uuid::new_v4()
        ));
        let teams_root = root.join("teams");
        let tasks_root = root.join("tasks");
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                ("agent_name".to_string(), json!("researcher")),
                ("agent_id".to_string(), json!("researcher@review")),
                (
                    "teams_root".to_string(),
                    json!(teams_root.to_string_lossy()),
                ),
                (
                    "tasks_root".to_string(),
                    json!(tasks_root.to_string_lossy()),
                ),
                (
                    "team_context".to_string(),
                    json!({
                        "team_name": "review",
                        "teamName": "review"
                    }),
                ),
            ]),
            abort_signal: abort_rx,
        };

        let input = json!({ "title": "needs approval" });
        let result = call_tool(
            &registry,
            None,
            &mut context,
            "TaskCreate",
            &input,
            Some("toolu_perm"),
            None,
        )
        .await;

        assert!(result.is_error);
        assert!(result
            .content
            .as_str()
            .unwrap()
            .contains("Permission request"));
        let leader_inbox = teams_root.join("review/inboxes/team-lead.json");
        let inbox: Value =
            serde_json::from_str(&fs::read_to_string(&leader_inbox).unwrap()).unwrap();
        let body: Value = serde_json::from_str(inbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["type"], "permission_request");
        assert_eq!(body["agent_id"], "researcher@review");
        assert_eq!(body["tool_name"], "TaskCreate");
        assert_eq!(body["tool_use_id"], "toolu_perm");
        assert_eq!(body["input"], input);

        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn call_tool_consumes_mailbox_permission_grant_once() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env();
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let input = json!({ "title": "approved by leader" });
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                (
                    "tasks_root".to_string(),
                    json!(isolated_tasks_root("mailbox-permission-grant")),
                ),
                (
                    "mailbox_permission_grants".to_string(),
                    json!([{
                        "request_id": "perm-1",
                        "tool_name": "TaskCreate",
                        "input": input,
                        "used": false
                    }]),
                ),
            ]),
            abort_signal: abort_rx,
        };
        let input = json!({ "title": "approved by leader" });

        let result = call_tool(
            &registry,
            None,
            &mut context,
            "TaskCreate",
            &input,
            Some("toolu_perm"),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["task"]["title"], "approved by leader");
        assert_eq!(
            context
                .app_state
                .get("mailbox_permission_grants")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            0
        );
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn call_tool_uses_mcp_permission_prompt_tool_on_ask() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env();
        let (url, state, server) =
            start_mock_permission_mcp_server(json!({"decision":"allow"})).await;
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                (
                    PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
                    json!("approve"),
                ),
                (
                    MCP_SERVERS_APP_STATE_KEY.to_string(),
                    json!({
                        "perm": {
                            "transport": "http",
                            "url": url
                        }
                    }),
                ),
                (
                    "tasks_root".to_string(),
                    json!(isolated_tasks_root("permission-prompt")),
                ),
            ]),
            abort_signal: abort_rx,
        };

        let input = json!({ "title": "allowed by MCP" });
        let result = call_tool(
            &registry,
            None,
            &mut context,
            "TaskCreate",
            &input,
            Some("toolu_allow"),
            None,
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["task"]["title"], "allowed by MCP");
        let calls = state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool_name"], "TaskCreate");
        assert_eq!(calls[0]["input"], input);
        assert_eq!(calls[0]["tool_use_id"], "toolu_allow");
        assert_eq!(context.app_state["tasks"].as_array().unwrap().len(), 1);
        server.abort();
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    struct RecordingPermissionPromptHandler {
        requests: Arc<Mutex<Vec<PermissionPromptRequest>>>,
        decision: PermissionPromptDecision,
    }

    #[async_trait::async_trait]
    impl PermissionPromptHandler for RecordingPermissionPromptHandler {
        async fn prompt(
            &self,
            request: PermissionPromptRequest,
        ) -> Result<PermissionPromptDecision, String> {
            self.requests.lock().unwrap().push(request);
            Ok(self.decision.clone())
        }
    }

    #[tokio::test]
    async fn call_tool_uses_stdio_permission_prompt_handler_on_ask() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env();
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler = RecordingPermissionPromptHandler {
            requests: requests.clone(),
            decision: PermissionPromptDecision::Allow,
        };
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                ("agent_id".to_string(), json!("researcher@review")),
                (
                    "permission_suggestions".to_string(),
                    json!([
                        {
                            "type": "addRules",
                            "destination": "session",
                            "rules": [{"toolName": "TaskCreate"}],
                            "behavior": "allow"
                        }
                    ]),
                ),
                (
                    PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
                    json!("stdio"),
                ),
                (
                    "tasks_root".to_string(),
                    json!(isolated_tasks_root("stdio-permission-prompt")),
                ),
            ]),
            abort_signal: abort_rx,
        };

        let input = json!({ "title": "allowed by stdio" });
        let result = call_tool(
            &registry,
            None,
            &mut context,
            "TaskCreate",
            &input,
            Some("toolu_stdio"),
            Some(&handler),
        )
        .await;

        assert!(!result.is_error, "{:?}", result.content);
        assert_eq!(result.content["task"]["title"], "allowed by stdio");
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].request_id.trim().is_empty());
        assert_eq!(requests[0].tool_name, "TaskCreate");
        assert_eq!(requests[0].input, input);
        assert_eq!(requests[0].tool_use_id, "toolu_stdio");
        assert_eq!(requests[0].agent_id.as_deref(), Some("researcher@review"));
        assert_eq!(
            requests[0].decision_reason,
            json!({
                "type": "other",
                "reason": "Tool TaskCreate requires permission in ask mode."
            })
        );
        assert_eq!(
            requests[0].permission_suggestions[0]["destination"],
            "session"
        );
        assert_eq!(requests[0].blocked_path, None);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn call_tool_honors_mcp_permission_prompt_denial() {
        let _guard = env_lock().lock().unwrap();
        isolate_permission_env();
        let (url, state, server) = start_mock_permission_mcp_server(json!({
            "decision": "deny",
            "reason": "blocked by policy"
        }))
        .await;
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                (
                    PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
                    json!("perm.approve"),
                ),
                (
                    MCP_SERVERS_APP_STATE_KEY.to_string(),
                    json!({
                        "perm": {
                            "transport": "http",
                            "url": url
                        }
                    }),
                ),
            ]),
            abort_signal: abort_rx,
        };

        let result = call_tool(
            &registry,
            None,
            &mut context,
            "TaskCreate",
            &json!({ "title": "denied by MCP" }),
            Some("toolu_deny"),
            None,
        )
        .await;

        assert!(result.is_error);
        assert!(result
            .content
            .as_str()
            .unwrap()
            .contains("blocked by policy"));
        assert!(context.app_state.get("tasks").is_none());
        assert_eq!(state.lock().unwrap().calls.len(), 1);
        server.abort();
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn call_tool_captures_structured_output_metadata() {
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([(
                "structured_output_schema".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "answer": { "type": "string" }
                    },
                    "required": ["answer"]
                }),
            )]),
            abort_signal: abort_rx,
        };

        let result = call_tool(
            &registry,
            None,
            &mut context,
            "StructuredOutput",
            &json!({ "answer": "done" }),
            None,
            None,
        )
        .await;

        assert!(!result.is_error);
        assert_eq!(result.structured_output, Some(json!({ "answer": "done" })));
    }

    #[test]
    fn parses_structured_output_schema_from_options() {
        let options = HashMap::from([(
            "json_schema".to_string(),
            json!("{\"type\":\"object\",\"required\":[\"name\"]}"),
        )]);

        let schema = structured_output_schema_option(&options).unwrap().unwrap();
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["required"][0], "name");
    }

    #[test]
    fn max_iterations_prefers_options_then_env_then_default() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("KIANA_MAX_ITERATIONS", "5");
        assert_eq!(max_iterations_option(&HashMap::new()), 5);

        let options = HashMap::from([("max_iterations".to_string(), json!(2))]);
        assert_eq!(max_iterations_option(&options), 2);

        std::env::set_var("KIANA_MAX_ITERATIONS", "0");
        assert_eq!(max_iterations_option(&HashMap::new()), 8);
        std::env::remove_var("KIANA_MAX_ITERATIONS");
    }

    #[test]
    fn thinking_option_accepts_options_env_and_explicit_disable() {
        let _guard = env_lock().lock().unwrap();
        clear_thinking_env();
        clear_max_tokens_env();

        std::env::set_var("KIANA_MAX_THINKING_TOKENS", "4096");
        assert_eq!(
            thinking_option(&HashMap::new()).unwrap(),
            Some(json!({ "type": "enabled", "budget_tokens": 4096_u64 }))
        );

        let options = HashMap::from([("maxThinkingTokens".to_string(), json!("2048"))]);
        assert_eq!(
            thinking_option(&options).unwrap(),
            Some(json!({ "type": "enabled", "budget_tokens": 2048_u64 }))
        );

        let disabled = HashMap::from([("max_thinking_tokens".to_string(), json!(0))]);
        assert_eq!(thinking_option(&disabled).unwrap(), None);

        let cleared = HashMap::from([("max_thinking_tokens".to_string(), Value::Null)]);
        assert_eq!(thinking_option(&cleared).unwrap(), None);

        let invalid = HashMap::from([("max_thinking_tokens".to_string(), json!(-1))]);
        assert!(thinking_option(&invalid)
            .unwrap_err()
            .to_string()
            .contains("max_thinking_tokens must be a non-negative integer"));

        clear_thinking_env();
    }

    #[test]
    fn max_tokens_option_keeps_thinking_budget_below_max_tokens() {
        let _guard = env_lock().lock().unwrap();
        clear_thinking_env();
        clear_max_tokens_env();
        let thinking = json!({ "type": "enabled", "budget_tokens": 4096_u64 });

        assert_eq!(
            max_tokens_option(&HashMap::new(), Some(&thinking)).unwrap(),
            5120
        );

        let valid = HashMap::from([("maxTokens".to_string(), json!(6000))]);
        assert_eq!(max_tokens_option(&valid, Some(&thinking)).unwrap(), 6000);

        let invalid = HashMap::from([("max_tokens".to_string(), json!(4096))]);
        assert!(max_tokens_option(&invalid, Some(&thinking))
            .unwrap_err()
            .to_string()
            .contains("max_tokens must be greater than max_thinking_tokens"));

        std::env::set_var("KIANA_MAX_TOKENS", "4096");
        assert!(max_tokens_option(&HashMap::new(), Some(&thinking))
            .unwrap_err()
            .to_string()
            .contains("max_tokens must be greater than max_thinking_tokens"));
        clear_max_tokens_env();
    }

    #[test]
    fn api_timeout_prefers_options_then_env_then_config_then_default() {
        let _guard = env_lock().lock().unwrap();
        clear_api_timeout_env();

        assert_eq!(
            api_timeout_option(&HashMap::new(), None).unwrap(),
            Duration::from_secs(600)
        );
        assert_eq!(
            api_timeout_option(&HashMap::new(), Some(250)).unwrap(),
            Duration::from_millis(250)
        );

        std::env::set_var("KIANA_API_TIMEOUT", "2");
        assert_eq!(
            api_timeout_option(&HashMap::new(), Some(250)).unwrap(),
            Duration::from_secs(2)
        );

        let options = HashMap::from([("api_timeout_ms".to_string(), json!(75))]);
        assert_eq!(
            api_timeout_option(&options, Some(250)).unwrap(),
            Duration::from_millis(75)
        );

        clear_api_timeout_env();
    }

    #[test]
    fn api_timeout_rejects_invalid_env() {
        let _guard = env_lock().lock().unwrap();
        clear_api_timeout_env();
        std::env::set_var("KIANA_API_TIMEOUT_MS", "0");

        let error = api_timeout_option(&HashMap::new(), None)
            .unwrap_err()
            .to_string();

        assert!(error.contains("KIANA_API_TIMEOUT_MS must be a positive integer"));
        clear_api_timeout_env();
    }

    #[test]
    fn system_prompt_option_combines_base_and_append() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var("KIANA_SYSTEM_PROMPT", "base from env");
        std::env::set_var("KIANA_APPEND_SYSTEM_PROMPT", "append from env");
        assert_eq!(
            system_prompt_option(&HashMap::new()),
            Some(json!("base from env\n\nappend from env"))
        );

        let options = HashMap::from([
            ("systemPrompt".to_string(), json!("base from option")),
            (
                "append_system_prompt".to_string(),
                json!("append from option"),
            ),
        ]);
        assert_eq!(
            system_prompt_option(&options),
            Some(json!("base from option\n\nappend from option"))
        );

        std::env::remove_var("KIANA_SYSTEM_PROMPT");
        std::env::remove_var("KIANA_APPEND_SYSTEM_PROMPT");
    }

    #[test]
    fn permission_rule_options_seed_app_state() {
        let mut app_state = HashMap::new();
        let options = HashMap::from([
            (
                "allowed_tools".to_string(),
                json!(["Read", {"toolName": "Bash", "ruleContent": "git:*"}]),
            ),
            ("disallowedTools".to_string(), json!("Write")),
            ("ask_tools".to_string(), json!(["WebFetch"])),
        ]);

        apply_permission_rule_options(&mut app_state, &options);

        assert_eq!(app_state["allowed_tools"], json!(["Read", "Bash(git:*)"]));
        assert_eq!(app_state["disallowed_tools"], json!(["Write"]));
        assert_eq!(app_state["ask_tools"], json!(["WebFetch"]));
        assert_eq!(
            app_state["permissions"]["allowedTools"],
            json!(["Read", "Bash(git:*)"])
        );
        assert_eq!(
            app_state["permissions"]["disallowedTools"],
            json!(["Write"])
        );
        assert_eq!(app_state["permissions"]["askTools"], json!(["WebFetch"]));
    }

    #[test]
    fn tool_filter_resolves_selected_tools_and_schemas() {
        let registry = create_default_registry();
        let options = HashMap::from([("tools".to_string(), json!("read, Bash"))]);

        let filter = tool_filter_option(&options, &registry).unwrap().unwrap();
        assert_eq!(
            filter,
            HashSet::from(["Read".to_string(), "Bash".to_string()])
        );

        let schemas = filtered_tool_schemas(&registry, Some(&filter));
        let names: HashSet<String> = schemas
            .iter()
            .filter_map(|schema| schema.get("name").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        assert_eq!(names, filter);
    }

    #[test]
    fn tool_filter_can_disable_default_or_use_core_tools() {
        let registry = create_default_registry();
        let disabled = HashMap::from([("tools".to_string(), json!(""))]);
        let filter = tool_filter_option(&disabled, &registry).unwrap().unwrap();
        assert!(filter.is_empty());
        assert!(filtered_tool_schemas(&registry, Some(&filter)).is_empty());

        let defaulted = HashMap::from([("tools".to_string(), json!("default"))]);
        assert!(tool_filter_option(&defaulted, &registry).unwrap().is_none());
        let all = HashMap::from([("tools".to_string(), json!("all"))]);
        assert!(tool_filter_option(&all, &registry).unwrap().is_none());
        assert!(tool_filter_option(&HashMap::new(), &registry)
            .unwrap()
            .is_none());

        let core = HashMap::from([("tools".to_string(), json!("core"))]);
        let core_filter = tool_filter_option(&core, &registry).unwrap().unwrap();
        assert!(core_filter.contains("Read"));
        assert!(core_filter.contains("Bash"));
        assert!(core_filter.contains("TodoWrite"));
        assert!(core_filter.contains("WebSearch"));
        assert!(!core_filter.contains("TeamCreate"));
        assert!(!core_filter.contains("TaskCreate"));

        let safe = HashMap::from([("tools".to_string(), json!("safe"))]);
        let safe_filter = tool_filter_option(&safe, &registry).unwrap().unwrap();
        assert_eq!(safe_filter, core_filter);
    }

    #[test]
    fn tool_filter_rejects_unknown_tools() {
        let registry = create_default_registry();
        let options = HashMap::from([("tools".to_string(), json!("MissingTool"))]);

        let error = tool_filter_option(&options, &registry)
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown tool in --tools"));
    }

    #[tokio::test]
    async fn collect_streaming_content_reconstructs_text_and_tool_input() {
        let events: Vec<std::result::Result<StreamEvent, ProviderError>> = vec![
            Ok(StreamEvent::MessageStart {
                message: kiana_services::api::streaming::MessageStart {
                    id: "msg_1".to_string(),
                    model: "model".to_string(),
                    role: "assistant".to_string(),
                    usage: kiana_services::api::streaming::DeltaUsage {
                        input_tokens: 1,
                        output_tokens: 0,
                    },
                },
            }),
            Ok(StreamEvent::ContentBlockStart {
                index: 0,
                content_block: StreamContentBlock::Text {
                    text: String::new(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::TextDelta {
                    text: "hel".to_string(),
                },
            }),
            Ok(StreamEvent::Ping),
            Ok(StreamEvent::ContentBlockDelta {
                index: 0,
                delta: Delta::TextDelta {
                    text: "lo".to_string(),
                },
            }),
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
            Ok(StreamEvent::ContentBlockStart {
                index: 1,
                content_block: StreamContentBlock::ToolUse(
                    kiana_services::api::streaming::ToolUse {
                        id: "toolu_1".to_string(),
                        name: "Read".to_string(),
                        input: json!({}),
                    },
                ),
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 1,
                delta: Delta::InputJsonDelta {
                    partial_json: r#"{"file_path":"Cargo.toml"}"#.to_string(),
                },
            }),
            Ok(StreamEvent::ContentBlockStop { index: 1 }),
            Ok(StreamEvent::ContentBlockStart {
                index: 2,
                content_block: StreamContentBlock::Thinking {
                    thinking: String::new(),
                    signature: None,
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 2,
                delta: Delta::ThinkingDelta {
                    thinking: "deep ".to_string(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 2,
                delta: Delta::ThinkingDelta {
                    thinking: "thought".to_string(),
                },
            }),
            Ok(StreamEvent::ContentBlockDelta {
                index: 2,
                delta: Delta::SignatureDelta {
                    signature: "sig".to_string(),
                },
            }),
            Ok(StreamEvent::ContentBlockStop { index: 2 }),
            Ok(StreamEvent::ContentBlockStart {
                index: 3,
                content_block: StreamContentBlock::RedactedThinking {
                    data: "encrypted".to_string(),
                },
            }),
            Ok(StreamEvent::ContentBlockStop { index: 3 }),
            Ok(StreamEvent::MessageStop),
        ];
        let mut stream: ProviderStream = Box::pin(futures::stream::iter(events));
        let mut emitted = 0;

        let content = collect_streaming_content(&mut stream, &mut |_| {
            emitted += 1;
            Ok(())
        })
        .await
        .unwrap();

        assert_eq!(emitted, 17);
        assert_eq!(content[0]["text"], "hello");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["input"]["file_path"], "Cargo.toml");
        assert_eq!(content[2]["type"], "thinking");
        assert_eq!(content[2]["thinking"], "deep thought");
        assert_eq!(content[2]["signature"], "sig");
        assert_eq!(content[3]["type"], "redacted_thinking");
        assert_eq!(content[3]["data"], "encrypted");
    }

    #[test]
    fn seed_team_app_state_from_env_exposes_teammate_context_to_tools() {
        let _guard = env_lock().lock().unwrap();
        for key in [
            "KIANA_AGENT_ID",
            "KIANA_AGENT_NAME",
            "KIANA_AGENT_COLOR",
            "KIANA_TEAM_NAME",
            "KIANA_TASK_LIST_ID",
            "KIANA_TEAM_FILE",
            "KIANA_TEAM_MAILBOX",
            "KIANA_TEAMS_ROOT",
            "KIANA_TASKS_ROOT",
            "KIANA_PLAN_MODE_REQUIRED",
            "CLAUDE_CODE_AGENT_ID",
            "CLAUDE_CODE_AGENT_NAME",
            "CLAUDE_CODE_TEAM_NAME",
            "CLAUDE_CODE_TASK_LIST_ID",
        ] {
            std::env::remove_var(key);
        }
        std::env::set_var("KIANA_AGENT_ID", "runner@review");
        std::env::set_var("KIANA_AGENT_NAME", "runner");
        std::env::set_var("KIANA_AGENT_COLOR", "green");
        std::env::set_var("KIANA_TEAM_NAME", "review");
        std::env::set_var("KIANA_TASK_LIST_ID", "review");
        std::env::set_var("KIANA_TEAM_FILE", "/tmp/review/config.json");
        std::env::set_var("KIANA_TEAM_MAILBOX", "/tmp/review/inboxes/runner.json");
        std::env::set_var("KIANA_TEAMS_ROOT", "/tmp/teams");
        std::env::set_var("KIANA_TASKS_ROOT", "/tmp/tasks");
        std::env::set_var("KIANA_PLAN_MODE_REQUIRED", "true");

        let mut app_state = HashMap::new();
        seed_team_app_state_from_env(&mut app_state);

        assert_eq!(app_state["agent_name"], "runner");
        assert_eq!(app_state["task_list_id"], "review");
        assert_eq!(app_state["teams_root"], "/tmp/teams");
        assert_eq!(app_state["tasks_root"], "/tmp/tasks");
        assert_eq!(app_state["team_context"]["teamName"], "review");
        assert_eq!(
            app_state["team_context"]["teammates"]["runner@review"]["name"],
            "runner"
        );
        assert_eq!(
            app_state["team_context"]["teammates"]["runner@review"]["planModeRequired"],
            true
        );

        for key in [
            "KIANA_AGENT_ID",
            "KIANA_AGENT_NAME",
            "KIANA_AGENT_COLOR",
            "KIANA_TEAM_NAME",
            "KIANA_TASK_LIST_ID",
            "KIANA_TEAM_FILE",
            "KIANA_TEAM_MAILBOX",
            "KIANA_TEAMS_ROOT",
            "KIANA_TASKS_ROOT",
            "KIANA_PLAN_MODE_REQUIRED",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn consume_initial_mailbox_messages_applies_state_updates_and_delivers_plain_messages() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root =
            std::env::temp_dir().join(format!("kiana-runner-mailbox-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("runner.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "Inspect the parser",
                    "summary": "initial_prompt",
                    "color": "green",
                    "timestamp": "1",
                    "read": false
                },
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"team_permission_update\",\"permissionUpdate\":{\"type\":\"addRules\",\"rules\":[{\"toolName\":\"Bash\",\"ruleContent\":\"git:*\"}],\"behavior\":\"allow\",\"destination\":\"session\"},\"directoryPath\":\"/repo\",\"toolName\":\"Bash\"}",
                    "timestamp": "2",
                    "read": false
                },
                {
                    "from": "old",
                    "text": "already read",
                    "timestamp": "3",
                    "read": true
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::new();
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Teammate Mailbox"));
        assert!(context.contains("<teammate-message teammate_id=\"team-lead\" color=\"green\" summary=\"initial_prompt\">"));
        assert!(context.contains("Inspect the parser"));
        assert!(!context.contains("team_permission_update"));
        assert_eq!(app_state["allowed_tools"], json!(["Bash(git:*)"]));
        assert_eq!(
            app_state["permissions"]["allowedTools"],
            json!(["Bash(git:*)"])
        );

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);
        assert_eq!(mailbox_after[1]["read"], true);
        assert_eq!(mailbox_after[2]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_skips_duplicate_initial_prompt() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-mailbox-dedupe-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("runner.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "Run background review",
                    "summary": "initial_prompt",
                    "timestamp": "1",
                    "read": false
                },
                {
                    "from": "team-lead",
                    "text": "Follow-up after start",
                    "summary": "follow_up",
                    "timestamp": "2",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::from([(
            "skip_initial_mailbox_prompt".to_string(),
            json!("Run background review"),
        )]);
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();

        assert!(context.contains("Follow-up after start"));
        assert!(!context.contains("Run background review"));
        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);
        assert_eq!(mailbox_after[1]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_applies_mode_set_request_and_updates_team_file() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");
        std::env::remove_var("KIANA_AGENT_ID");
        std::env::remove_var("KIANA_TEAM_FILE");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-mode-set-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        let team_dir = root.join("teams/review");
        fs::create_dir_all(team_dir.join("inboxes")).unwrap();
        let mailbox = team_dir.join("inboxes/researcher.json");
        let team_file = team_dir.join("config.json");
        fs::write(
            &team_file,
            serde_json::to_string_pretty(&json!({
                "name": "review",
                "members": [
                    {
                        "agentId": "researcher@review",
                        "agent_id": "researcher@review",
                        "name": "researcher",
                        "mode": "plan"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"mode_set_request\",\"mode\":\"ask\",\"from\":\"team-lead\"}",
                    "summary": "mode_set_request",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);
        std::env::set_var("KIANA_TEAM_FILE", &team_file);
        std::env::set_var("KIANA_AGENT_NAME", "researcher");
        std::env::set_var("KIANA_AGENT_ID", "researcher@review");

        let mut app_state = HashMap::new();
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Mailbox State Updates"));
        assert!(context.contains("Current permission mode: `ask`"));
        assert_eq!(app_state["permission_mode"], "ask");
        assert_eq!(app_state["permissions"]["mode"], "ask");

        let team_file_after: Value =
            serde_json::from_str(&fs::read_to_string(&team_file).unwrap()).unwrap();
        assert_eq!(team_file_after["members"][0]["mode"], "ask");
        assert_eq!(team_file_after["members"][0]["permissionMode"], "ask");
        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAM_FILE");
        std::env::remove_var("KIANA_AGENT_NAME");
        std::env::remove_var("KIANA_AGENT_ID");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sync_current_member_lifecycle_status_updates_team_file() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_FILE");
        std::env::remove_var("KIANA_AGENT_NAME");
        std::env::remove_var("KIANA_AGENT_ID");
        std::env::remove_var("KIANA_TEAM_MAILBOX");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-lifecycle-status-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let team_file = root.join("config.json");
        fs::write(
            &team_file,
            serde_json::to_string_pretty(&json!({
                "name": "review",
                "members": [
                    {
                        "agentId": "runner@review",
                        "agent_id": "runner@review",
                        "name": "runner",
                        "isActive": false,
                        "is_active": false
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_FILE", &team_file);
        std::env::set_var("KIANA_AGENT_NAME", "runner");
        std::env::set_var("KIANA_AGENT_ID", "runner@review");

        sync_current_member_lifecycle_status("running", Some(true)).unwrap();
        let after_running: Value =
            serde_json::from_str(&fs::read_to_string(&team_file).unwrap()).unwrap();
        assert_eq!(after_running["members"][0]["lifecycleStatus"], "running");
        assert_eq!(after_running["members"][0]["isActive"], true);

        let mut stable_running = after_running.clone();
        stable_running["members"][0]["lastStatusAt"] = json!(123);
        stable_running["members"][0]["last_status_at"] = json!(123);
        fs::write(
            &team_file,
            serde_json::to_string_pretty(&stable_running).unwrap(),
        )
        .unwrap();
        sync_current_member_lifecycle_status("running", Some(true)).unwrap();
        let after_idempotent: Value =
            serde_json::from_str(&fs::read_to_string(&team_file).unwrap()).unwrap();
        assert_eq!(after_idempotent["members"][0]["lastStatusAt"], 123);

        sync_current_member_lifecycle_status("shutdown_approved", Some(false)).unwrap();
        let after_shutdown: Value =
            serde_json::from_str(&fs::read_to_string(&team_file).unwrap()).unwrap();
        assert_eq!(
            after_shutdown["members"][0]["lifecycleStatus"],
            "shutdown_approved"
        );
        assert_eq!(after_shutdown["members"][0]["isActive"], false);
        assert_eq!(
            after_shutdown["members"][0]["lastExitReason"],
            "shutdown_approved"
        );

        std::env::remove_var("KIANA_TEAM_FILE");
        std::env::remove_var("KIANA_AGENT_NAME");
        std::env::remove_var("KIANA_AGENT_ID");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resident_teammate_loop_runs_again_when_mailbox_has_unread_message() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-resident-loop-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("runner.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "Follow-up task",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let calls_for_runner = calls.clone();
        let result = run_resident_teammate_loop_with(
            "Initial task".to_string(),
            HashMap::new(),
            ResidentTeammateLoopConfig {
                poll_interval: Duration::from_millis(0),
                max_idle_polls: Some(10),
                max_turns: Some(2),
            },
            move |prompt, _options| {
                let calls = calls_for_runner.clone();
                async move {
                    let mut calls = calls.lock().unwrap();
                    calls.push(prompt);
                    if calls.len() > 1 {
                        let mut app_state = HashMap::new();
                        let _ = consume_initial_mailbox_messages(&mut app_state)?;
                    }
                    Ok(json!({ "session_id": "resident-session" }))
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(result.session_id, "resident-session");
        assert_eq!(result.turns, 2);
        assert_eq!(result.stop_reason, "max_turns");
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "Initial task".to_string(),
                "Continue from teammate mailbox.".to_string()
            ]
        );
        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resident_teammate_loop_claims_available_team_task() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");
        std::env::remove_var("KIANA_TASK_LIST_ID");
        std::env::remove_var("KIANA_TASKS_ROOT");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-resident-task-list-{}",
            uuid::Uuid::new_v4()
        ));
        let task_dir = root.join("tasks/review");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(
            task_dir.join("1.json"),
            serde_json::to_string_pretty(&json!({
                "id": "1",
                "title": "Audit task claim",
                "subject": "Audit task claim",
                "description": "Check the worker picks this up.",
                "status": "pending",
                "blocks": [],
                "blockedBy": [],
                "created_at": 1,
                "updated_at": 1
            }))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TASKS_ROOT", root.join("tasks"));
        std::env::set_var("KIANA_TASK_LIST_ID", "review");
        std::env::set_var("KIANA_AGENT_NAME", "runner");

        let calls = Arc::new(Mutex::new(Vec::new()));
        let calls_for_runner = calls.clone();
        let result = run_resident_teammate_loop_with(
            "Initial task".to_string(),
            HashMap::new(),
            ResidentTeammateLoopConfig {
                poll_interval: Duration::from_millis(0),
                max_idle_polls: Some(10),
                max_turns: Some(2),
            },
            move |prompt, _options| {
                let calls = calls_for_runner.clone();
                async move {
                    calls.lock().unwrap().push(prompt);
                    Ok(json!({ "session_id": "resident-session" }))
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(result.turns, 2);
        assert_eq!(result.stop_reason, "max_turns");
        assert_eq!(calls.lock().unwrap()[0], "Initial task");
        assert_eq!(
            calls.lock().unwrap()[1],
            "Complete all open tasks. Start with task #1: \n\n Audit task claim\n\nCheck the worker picks this up."
        );
        let task_after: Value =
            serde_json::from_str(&fs::read_to_string(task_dir.join("1.json")).unwrap()).unwrap();
        assert_eq!(task_after["owner"], "runner");
        assert_eq!(task_after["status"], "in_progress");

        std::env::remove_var("KIANA_TASKS_ROOT");
        std::env::remove_var("KIANA_TASK_LIST_ID");
        std::env::remove_var("KIANA_AGENT_NAME");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resident_teammate_loop_ignores_unreadable_task_list() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_AGENT_NAME");
        std::env::remove_var("KIANA_TASK_LIST_ID");
        std::env::remove_var("KIANA_TASKS_ROOT");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-resident-bad-task-list-{}",
            uuid::Uuid::new_v4()
        ));
        let task_dir = root.join("tasks/review");
        fs::create_dir_all(&task_dir).unwrap();
        fs::write(task_dir.join("1.json"), "{not valid json").unwrap();
        std::env::set_var("KIANA_TASKS_ROOT", root.join("tasks"));
        std::env::set_var("KIANA_TASK_LIST_ID", "review");
        std::env::set_var("KIANA_AGENT_NAME", "runner");

        let result = run_resident_teammate_loop_with(
            "Initial task".to_string(),
            HashMap::new(),
            ResidentTeammateLoopConfig {
                poll_interval: Duration::from_millis(0),
                max_idle_polls: Some(1),
                max_turns: Some(5),
            },
            |_prompt, _options| async { Ok(json!({ "session_id": "resident-session" })) },
        )
        .await
        .unwrap();

        assert_eq!(result.turns, 1);
        assert_eq!(result.stop_reason, "idle_limit");

        std::env::remove_var("KIANA_TASKS_ROOT");
        std::env::remove_var("KIANA_TASK_LIST_ID");
        std::env::remove_var("KIANA_AGENT_NAME");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resident_teammate_loop_does_not_rerun_for_initial_prompt_mailbox_copy() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-resident-initial-dedupe-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("runner.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "Initial resident task",
                    "summary": "initial_prompt",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let calls_for_runner = calls.clone();
        let result = run_resident_teammate_loop_with(
            "Initial resident task".to_string(),
            HashMap::new(),
            ResidentTeammateLoopConfig {
                poll_interval: Duration::from_millis(0),
                max_idle_polls: Some(1),
                max_turns: Some(5),
            },
            move |_prompt, options| {
                let calls = calls_for_runner.clone();
                async move {
                    *calls.lock().unwrap() += 1;
                    let mut app_state = HashMap::new();
                    if let Some(prompt) = options.get("skip_initial_mailbox_prompt").cloned() {
                        app_state.insert("skip_initial_mailbox_prompt".to_string(), prompt);
                    }
                    let _ = consume_initial_mailbox_messages(&mut app_state)?;
                    Ok(json!({ "session_id": "resident-session" }))
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(result.turns, 1);
        assert_eq!(result.stop_reason, "idle_limit");
        assert_eq!(*calls.lock().unwrap(), 1);
        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn resident_teammate_loop_stops_after_shutdown_approval_result() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-resident-shutdown-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("runner.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"shutdown_request\",\"requestId\":\"shutdown-1\",\"from\":\"team-lead\",\"reason\":\"done\"}",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let calls = std::sync::Arc::new(std::sync::Mutex::new(0usize));
        let calls_for_runner = calls.clone();
        let result = run_resident_teammate_loop_with(
            "Initial task".to_string(),
            HashMap::new(),
            ResidentTeammateLoopConfig {
                poll_interval: Duration::from_millis(0),
                max_idle_polls: Some(10),
                max_turns: Some(5),
            },
            move |_prompt, _options| {
                let calls = calls_for_runner.clone();
                async move {
                    let mut calls = calls.lock().unwrap();
                    *calls += 1;
                    if *calls > 1 {
                        let mut app_state = HashMap::new();
                        let _ = consume_initial_mailbox_messages(&mut app_state)?;
                        Ok(json!({
                            "session_id": "resident-session",
                            "teammate_shutdown_approved": true
                        }))
                    } else {
                        Ok(json!({ "session_id": "resident-session" }))
                    }
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(result.turns, 2);
        assert_eq!(result.stop_reason, "shutdown_approved");
        assert_eq!(*calls.lock().unwrap(), 2);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_prioritizes_shutdown_request() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-shutdown-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("runner.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "peer",
                    "text": "Peer note that should wait",
                    "timestamp": "1",
                    "read": false
                },
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"shutdown_request\",\"requestId\":\"shutdown-runner-1\",\"from\":\"team-lead\",\"reason\":\"work complete\",\"timestamp\":\"123\"}",
                    "summary": "shutdown_request",
                    "timestamp": "2",
                    "read": false
                },
                {
                    "from": "old",
                    "text": "already read",
                    "timestamp": "3",
                    "read": true
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::new();
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Shutdown Request"));
        assert!(context.contains("shutdown-runner-1"));
        assert!(context.contains("work complete"));
        assert!(context.contains("shutdown_response"));
        assert!(!context.contains("Peer note that should wait"));

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], false);
        assert_eq!(mailbox_after[1]["read"], true);
        assert_eq!(mailbox_after[2]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_auto_approves_plan_approval_request_for_lead() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-plan-request-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("team-lead.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "peer",
                    "text": "Peer note that should wait",
                    "timestamp": "1",
                    "read": false
                },
                {
                    "from": "researcher",
                    "text": "{\"type\":\"plan_approval_request\",\"requestId\":\"plan-1\",\"from\":\"researcher\",\"planFilePath\":\".claude/plans/researcher.md\",\"planContent\":\"1. Inspect\\n2. Implement\",\"timestamp\":\"123\"}",
                    "summary": "plan_approval_request",
                    "timestamp": "2",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);
        std::env::set_var("KIANA_AGENT_NAME", "team-lead");

        let mut app_state = HashMap::from([("permission_mode".to_string(), json!("plan"))]);
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Plan Approval Request Auto-Approved"));
        assert!(context.contains("researcher"));
        assert!(context.contains("plan-1"));
        assert!(context.contains("1. Inspect\n2. Implement"));
        assert!(context.contains("plan_approval_response"));
        assert!(!context.contains("Use the `SendMessage` tool"));
        assert!(!context.contains("Peer note that should wait"));

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], false);
        assert_eq!(mailbox_after[1]["read"], true);
        let researcher_inbox = root.join("researcher.json");
        let teammate_mailbox: Value =
            serde_json::from_str(&fs::read_to_string(&researcher_inbox).unwrap()).unwrap();
        assert_eq!(teammate_mailbox[0]["from"], "team-lead");
        assert_eq!(teammate_mailbox[0]["summary"], "plan_approval_response");
        assert_eq!(teammate_mailbox[0]["read"], false);
        let response: Value =
            serde_json::from_str(teammate_mailbox[0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(response["type"], "plan_approval_response");
        assert_eq!(response["requestId"], "plan-1");
        assert_eq!(response["approved"], true);
        assert_eq!(response["permissionMode"], "default");

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_AGENT_NAME");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_injects_plan_approval_response() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-plan-response-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("researcher.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"plan_approval_response\",\"requestId\":\"plan-1\",\"approved\":true,\"permissionMode\":\"default\",\"timestamp\":\"124\"}",
                    "summary": "plan_approval_response",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::new();
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Plan Approval Response"));
        assert!(context.contains("plan-1"));
        assert!(context.contains("approved"));
        assert!(context.contains("permission_mode"));
        assert!(context.contains("proceed with implementation"));
        assert_eq!(app_state["permission_mode"], "default");
        assert_eq!(app_state["permissions"]["mode"], "default");

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_records_permission_request_context() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-permission-request-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("team-lead.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "researcher",
                    "text": "{\"type\":\"permission_request\",\"request_id\":\"perm-1\",\"agent_id\":\"researcher@review\",\"tool_name\":\"Bash\",\"tool_use_id\":\"toolu_1\",\"description\":\"Tool Bash requires permission in ask mode.\",\"input\":{\"command\":\"git status\"},\"permission_suggestions\":[]}",
                    "summary": "permission_request",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::new();
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Permission Request"));
        assert!(context.contains("perm-1"));
        assert!(context.contains("Bash"));
        assert!(context.contains("permission_response"));
        assert_eq!(
            app_state["mailbox_permission_requests"]["perm-1"]["tool_name"],
            "Bash"
        );
        assert_eq!(
            app_state["mailbox_permission_requests"]["perm-1"]["input"]["command"],
            "git status"
        );

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_records_permission_response_grant() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-permission-response-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("researcher.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"permission_response\",\"request_id\":\"perm-1\",\"subtype\":\"success\",\"response\":{},\"tool_name\":\"Bash\",\"input\":{\"command\":\"git status\"}}",
                    "summary": "permission_response",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::from([(
            "pending_mailbox_permission_requests".to_string(),
            json!({
                "perm-1": {
                    "request_id": "perm-1",
                    "tool_name": "Bash",
                    "input": { "command": "git status" }
                }
            }),
        )]);
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Permission Response"));
        assert!(context.contains("approved"));
        assert!(context.contains("one-time grant"));
        assert_eq!(
            app_state["mailbox_permission_grants"][0]["tool_name"],
            "Bash"
        );
        assert_eq!(
            app_state["mailbox_permission_grants"][0]["input"]["command"],
            "git status"
        );
        assert!(app_state["pending_mailbox_permission_requests"]
            .as_object()
            .unwrap()
            .is_empty());

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_clears_pending_permission_on_rejection() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-permission-rejection-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("researcher.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"permission_response\",\"request_id\":\"perm-1\",\"subtype\":\"error\",\"error\":\"No writes allowed\",\"tool_name\":\"Bash\",\"input\":{\"command\":\"git status\"}}",
                    "summary": "permission_response",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::from([(
            "pending_mailbox_permission_requests".to_string(),
            json!({
                "perm-1": {
                    "request_id": "perm-1",
                    "tool_name": "Bash",
                    "input": { "command": "git status" }
                }
            }),
        )]);
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Permission Response"));
        assert!(context.contains("rejected"));
        assert!(context.contains("No writes allowed"));
        assert!(app_state.get("mailbox_permission_grants").is_none());
        assert!(app_state["pending_mailbox_permission_requests"]
            .as_object()
            .unwrap()
            .is_empty());

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_records_sandbox_permission_request_context() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-sandbox-request-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("team-lead.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "researcher",
                    "text": "{\"type\":\"sandbox_permission_request\",\"requestId\":\"sandbox-1\",\"workerId\":\"researcher@review\",\"workerName\":\"researcher\",\"workerColor\":\"green\",\"hostPattern\":{\"host\":\"*\"},\"createdAt\":123}",
                    "summary": "sandbox_permission_request",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::new();
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Sandbox Permission Request"));
        assert!(context.contains("sandbox-1"));
        assert!(context.contains("sandbox network access"));
        assert!(context.contains("sandbox_permission_response"));
        assert_eq!(
            app_state["sandbox_permission_requests"]["sandbox-1"]["host"],
            "*"
        );
        assert_eq!(
            app_state["sandbox_permission_requests"]["sandbox-1"]["workerName"],
            "researcher"
        );

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_records_sandbox_permission_response_grant() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-sandbox-response-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("researcher.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"sandbox_permission_response\",\"requestId\":\"sandbox-1\",\"host\":\"*\",\"allow\":true,\"timestamp\":\"124\"}",
                    "summary": "sandbox_permission_response",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::from([(
            "pending_sandbox_permission_requests".to_string(),
            json!({
                "sandbox-1": {
                    "requestId": "sandbox-1",
                    "host": "*"
                }
            }),
        )]);
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Sandbox Permission Response"));
        assert!(context.contains("approved"));
        assert!(context.contains("one-time sandbox grant"));
        assert_eq!(
            app_state["sandbox_permission_grants"][0]["requestId"],
            "sandbox-1"
        );
        assert_eq!(app_state["sandbox_permission_grants"][0]["host"], "*");
        assert_eq!(app_state["sandbox_permission_grants"][0]["allow"], true);
        assert!(app_state["pending_sandbox_permission_requests"]
            .as_object()
            .unwrap()
            .is_empty());

        let mailbox_after: Value =
            serde_json::from_str(&fs::read_to_string(&mailbox).unwrap()).unwrap();
        assert_eq!(mailbox_after[0]["read"], true);

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn consume_initial_mailbox_messages_clears_pending_sandbox_on_rejection() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_TEAM_MAILBOX");
        std::env::remove_var("KIANA_TEAMS_ROOT");
        std::env::remove_var("KIANA_TEAM_NAME");
        std::env::remove_var("KIANA_AGENT_NAME");

        let root = std::env::temp_dir().join(format!(
            "kiana-runner-sandbox-rejection-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let mailbox = root.join("researcher.json");
        fs::write(
            &mailbox,
            serde_json::to_string_pretty(&json!([
                {
                    "from": "team-lead",
                    "text": "{\"type\":\"sandbox_permission_response\",\"requestId\":\"sandbox-1\",\"host\":\"*\",\"allow\":false,\"timestamp\":\"124\"}",
                    "summary": "sandbox_permission_response",
                    "timestamp": "1",
                    "read": false
                }
            ]))
            .unwrap(),
        )
        .unwrap();
        std::env::set_var("KIANA_TEAM_MAILBOX", &mailbox);

        let mut app_state = HashMap::from([(
            "pending_sandbox_permission_requests".to_string(),
            json!({
                "sandbox-1": {
                    "requestId": "sandbox-1",
                    "host": "*"
                }
            }),
        )]);
        let context = consume_initial_mailbox_messages(&mut app_state)
            .unwrap()
            .unwrap();
        assert!(context.contains("# Sandbox Permission Response"));
        assert!(context.contains("rejected"));
        assert!(app_state.get("sandbox_permission_grants").is_none());
        assert!(app_state["pending_sandbox_permission_requests"]
            .as_object()
            .unwrap()
            .is_empty());

        std::env::remove_var("KIANA_TEAM_MAILBOX");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn append_mailbox_context_updates_existing_user_message() {
        let mut messages = normalize_messages(vec![json!({
            "role": "user",
            "content": "original task"
        })])
        .unwrap();

        append_mailbox_context_to_messages(&mut messages, "mailbox text".to_string());

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, json!("original task\n\nmailbox text"));

        let mut messages = normalize_messages(vec![json!({
            "role": "user",
            "content": [
                {
                    "type": "text",
                    "text": "original"
                }
            ]
        })])
        .unwrap();

        append_mailbox_context_to_messages(&mut messages, "second".to_string());
        assert_eq!(messages[0].content[1]["type"], "text");
        assert_eq!(messages[0].content[1]["text"], "second");
    }

    #[tokio::test]
    async fn run_assistant_turn_retries_overload_with_fallback_model() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        let (base_url, state, server) = start_mock_messages_server().await;
        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "hello"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("main-model")),
                ("fallback_model".to_string(), json!("fallback-model")),
                ("tools".to_string(), json!("")),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fallback ok");
        assert_eq!(
            state.lock().unwrap().requested_models,
            vec!["main-model".to_string(), "fallback-model".to_string()]
        );
        server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_sends_thinking_budget_on_initial_and_fallback_requests() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        let (base_url, state, server) = start_mock_messages_server().await;
        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "hello"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("main-model")),
                ("fallback_model".to_string(), json!("fallback-model")),
                ("max_thinking_tokens".to_string(), json!(4096)),
                ("tools".to_string(), json!("")),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fallback ok");
        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(
            requests[0]["thinking"],
            json!({ "type": "enabled", "budget_tokens": 4096_u64 })
        );
        assert_eq!(requests[0]["max_tokens"], 5120);
        assert_eq!(
            requests[1]["thinking"],
            json!({ "type": "enabled", "budget_tokens": 4096_u64 })
        );
        assert_eq!(requests[1]["max_tokens"], 5120);
        server.abort();
        clear_thinking_env();
        clear_max_tokens_env();
    }

    #[tokio::test]
    async fn run_assistant_turn_auto_compacts_messages_before_request() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        let (base_url, state, server) = start_mock_messages_server().await;
        let messages = vec![
            json!({"role": "user", "content": "first message ".repeat(80)}),
            json!({"role": "assistant", "content": "middle one ".repeat(80)}),
            json!({"role": "user", "content": "middle two ".repeat(80)}),
            json!({"role": "assistant", "content": "middle three ".repeat(80)}),
            json!({"role": "user", "content": "recent tail"}),
        ];

        let result = run_assistant_turn(
            messages,
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("main-model")),
                ("fallback_model".to_string(), json!("fallback-model")),
                ("tools".to_string(), json!("")),
                ("compact_threshold_tokens".to_string(), json!(10)),
                ("compact_target_tokens".to_string(), json!(80)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fallback ok");
        let requests = state.lock().unwrap().requests.clone();
        let request_messages = requests[0]["messages"].as_array().unwrap();
        assert!(request_messages.len() < 5);
        assert!(request_messages.iter().any(|message| {
            content_text_for_compaction(&message["content"])
                .contains("[Compacted conversation summary]")
        }));
        server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_session_start_hook_adds_context_without_rewriting_user_prompt() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("KIANA_HOOKS");
        std::env::remove_var("KIANA_SESSION_START_HOOKS");
        let root =
            std::env::temp_dir().join(format!("kiana-session-start-hook-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let hooks_file = root.join("hooks.json");
        fs::write(
            &hooks_file,
            json!({
                "SessionStart": [
                    "printf '%s' '{\"add_context\":\"Project fact: use the staged release checklist\"}'"
                ]
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &hooks_file);
        let (base_url, state, server) = start_mock_messages_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "original task"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("main-model")),
                ("fallback_model".to_string(), json!("fallback-model")),
                ("tools".to_string(), json!("")),
                ("system_prompt".to_string(), json!("Base system")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fallback ok");
        let requests = state.lock().unwrap().requests.clone();
        let first_messages = requests[0]["messages"].as_array().unwrap();
        assert_eq!(first_messages[0]["content"], json!("original task"));
        let system = requests[0]["system"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains("Project fact: use the staged release checklist"));
        assert!(!first_messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("Project fact"));
        server.abort();
        std::env::remove_var("KIANA_HOOKS_FILE");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn run_assistant_turn_user_prompt_submit_hook_adds_context_without_rewriting_user_prompt()
    {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("KIANA_HOOKS");
        std::env::remove_var("KIANA_USER_PROMPT_SUBMIT_HOOKS");
        let root = std::env::temp_dir().join(format!(
            "kiana-user-prompt-submit-hook-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let hooks_file = root.join("hooks.json");
        fs::write(
            &hooks_file,
            json!({
                "UserPromptSubmit": [
                    "input=$(cat); snake=0; camel=0; case \"$input\" in *'\"user_prompt\":\"original customer request\"'*) snake=1;; esac; case \"$input\" in *'\"userPrompt\":\"original customer request\"'*) camel=1;; esac; if [ \"$snake$camel\" = 11 ]; then printf '%s' '{\"add_context\":\"Prompt fact: hook saw original customer request\"}'; else printf '%s' '{\"blocking_error\":\"missing prompt payload\"}'; fi"
                ]
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &hooks_file);
        let (base_url, state, server) = start_mock_messages_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "original customer request"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("main-model")),
                ("fallback_model".to_string(), json!("fallback-model")),
                ("tools".to_string(), json!("")),
                ("system_prompt".to_string(), json!("Base system")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fallback ok");
        let requests = state.lock().unwrap().requests.clone();
        let first_messages = requests[0]["messages"].as_array().unwrap();
        assert_eq!(
            first_messages[0]["content"],
            json!("original customer request")
        );
        let system = requests[0]["system"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains("Prompt fact: hook saw original customer request"));
        assert!(!first_messages[0]["content"]
            .as_str()
            .unwrap()
            .contains("Prompt fact"));
        server.abort();
        std::env::remove_var("KIANA_HOOKS_FILE");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn run_assistant_turn_user_prompt_submit_hook_updates_prompt_input() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("KIANA_HOOKS");
        std::env::remove_var("KIANA_USER_PROMPT_SUBMIT_HOOKS");
        let root = std::env::temp_dir().join(format!(
            "kiana-user-prompt-submit-update-input-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let hooks_file = root.join("hooks.json");
        fs::write(
            &hooks_file,
            json!({
                "UserPromptSubmit": [
                    "input=$(cat); case \"$input\" in *'\"user_prompt\":\"original customer request\"'*) printf '%s' '{\"update_input\":\"normalized customer request\",\"add_context\":\"Prompt normalized by hook\"}';; *) printf '%s' '{\"blocking_error\":\"missing prompt payload\"}';; esac"
                ]
            })
            .to_string(),
        )
        .unwrap();
        std::env::set_var("KIANA_HOOKS_FILE", &hooks_file);
        let (base_url, state, server) = start_mock_messages_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "original customer request"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("main-model")),
                ("fallback_model".to_string(), json!("fallback-model")),
                ("tools".to_string(), json!("")),
                ("system_prompt".to_string(), json!("Base system")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fallback ok");
        let requests = state.lock().unwrap().requests.clone();
        let first_messages = requests[0]["messages"].as_array().unwrap();
        assert_eq!(
            first_messages[0]["content"],
            json!("normalized customer request")
        );
        let system = requests[0]["system"].as_str().unwrap();
        assert!(system.contains("Base system"));
        assert!(system.contains("Prompt normalized by hook"));
        server.abort();
        std::env::remove_var("KIANA_HOOKS_FILE");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn run_assistant_turn_sends_tool_mapped_api_result_to_model() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        let root =
            std::env::temp_dir().join(format!("kiana-runner-tool-loop-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let write_path = root.join("created.txt");
        let (base_url, state, server) =
            start_mock_tool_loop_server(write_path.to_string_lossy().to_string()).await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "create a file"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("tool-loop-model")),
                ("tools".to_string(), json!("Write")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("max_iterations".to_string(), json!(2)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "write complete");
        assert_eq!(
            fs::read_to_string(&write_path).unwrap(),
            "hello from tool\n"
        );
        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 2);
        let second_messages = requests[1]["messages"].as_array().unwrap();
        let tool_result = &second_messages.last().unwrap()["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_write");
        assert_eq!(
            tool_result["content"],
            json!(format!(
                "File created successfully at: {}",
                write_path.to_string_lossy()
            ))
        );
        assert!(tool_result["content"]["originalFile"].is_null());

        server.abort();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn run_assistant_turn_fake_provider_returns_text_without_api_key() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "say hello"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("fake")),
                ("model".to_string(), json!("fake-model")),
                ("tools".to_string(), json!("")),
                (
                    "fake_provider_script".to_string(),
                    json!([
                        {
                            "type": "assistant_text",
                            "text": "hello from fake"
                        }
                    ]),
                ),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "hello from fake");
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn run_assistant_turn_openai_compatible_provider_returns_text() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        clear_openai_compatible_env();
        let (base_url, state, server) = start_mock_openai_compatible_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "say hello"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("openai-compatible")),
                ("model".to_string(), json!("gpt-test")),
                ("api_key".to_string(), json!("openai-test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("tools".to_string(), json!("")),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "openai compatible ok");
        assert_eq!(result.iterations, 1);
        let state = state.lock().unwrap();
        assert_eq!(state.auth_headers, vec!["Bearer openai-test-key"]);
        assert_eq!(state.requests.len(), 1);
        assert_eq!(state.requests[0]["model"], "gpt-test");
        assert_eq!(state.requests[0]["messages"][0]["role"], "user");
        assert_eq!(state.requests[0]["messages"][0]["content"], "say hello");
        assert!(state.requests[0]["tools"].is_null());

        server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_openai_compatible_provider_runs_tool_loop() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        clear_openai_compatible_env();
        let (base_url, state, server) = start_mock_openai_compatible_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "update todos"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("openai-compatible")),
                ("model".to_string(), json!("gpt-test")),
                ("api_key".to_string(), json!("openai-test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("tools".to_string(), json!("TodoWrite")),
                ("max_iterations".to_string(), json!(2)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "openai tool loop ok");
        assert_eq!(result.iterations, 2);
        let state = state.lock().unwrap();
        assert_eq!(state.requests.len(), 2);
        assert_eq!(
            state.requests[0]["tools"][0]["function"]["name"],
            "TodoWrite"
        );
        assert_eq!(state.requests[0]["tool_choice"], "auto");
        assert_eq!(state.requests[1]["messages"].as_array().unwrap().len(), 3);
        let tool_result = &state.requests[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(tool_result["role"], "tool");
        assert_eq!(tool_result["tool_call_id"], "call_todo");
        assert!(tool_result["content"]
            .as_str()
            .unwrap()
            .contains("Todos have been modified successfully"));

        server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_ollama_provider_returns_text() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        clear_ollama_env();
        let (base_url, state, server) = start_mock_ollama_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "say hello"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("ollama")),
                ("model".to_string(), json!("llama-test")),
                ("base_url".to_string(), json!(base_url)),
                ("tools".to_string(), json!("")),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "ollama ok");
        assert_eq!(result.iterations, 1);
        let state = state.lock().unwrap();
        assert_eq!(state.requests.len(), 1);
        assert_eq!(state.requests[0]["model"], "llama-test");
        assert_eq!(state.requests[0]["stream"], false);
        assert_eq!(state.requests[0]["messages"][0]["role"], "user");
        assert_eq!(state.requests[0]["messages"][0]["content"], "say hello");
        assert!(state.requests[0]["tools"].is_null());

        server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_ollama_provider_runs_tool_loop() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        clear_ollama_env();
        let (base_url, state, server) = start_mock_ollama_server().await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "update todos"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("ollama")),
                ("model".to_string(), json!("llama-test")),
                ("base_url".to_string(), json!(base_url)),
                ("tools".to_string(), json!("TodoWrite")),
                ("max_iterations".to_string(), json!(2)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "ollama tool loop ok");
        assert_eq!(result.iterations, 2);
        let state = state.lock().unwrap();
        assert_eq!(state.requests.len(), 2);
        assert_eq!(
            state.requests[0]["tools"][0]["function"]["name"],
            "TodoWrite"
        );
        assert_eq!(state.requests[1]["messages"].as_array().unwrap().len(), 3);
        let tool_result = &state.requests[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(tool_result["role"], "tool");
        assert_eq!(tool_result["tool_name"], "TodoWrite");
        assert!(tool_result["content"]
            .as_str()
            .unwrap()
            .contains("Todos have been modified successfully"));

        server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_fake_provider_reads_file_then_returns_final_answer() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");
        let root =
            std::env::temp_dir().join(format!("kiana-fake-provider-read-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("README.md"), "alpha fake read\n").unwrap();

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "read the README"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("fake")),
                ("model".to_string(), json!("fake-model")),
                ("tools".to_string(), json!("Read")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("max_iterations".to_string(), json!(2)),
                (
                    "fake_provider_script".to_string(),
                    json!([
                        {
                            "type": "tool_call",
                            "id": "toolu_read",
                            "name": "Read",
                            "input": { "file_path": "README.md" },
                            "text": "I will read it."
                        },
                        {
                            "type": "final_answer",
                            "text": "README read complete"
                        }
                    ]),
                ),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "README read complete");
        assert_eq!(result.iterations, 2);
        let tool_result_message = &result.messages[2];
        assert_eq!(tool_result_message.role, "user");
        assert!(serde_json::to_string(&tool_result_message.content)
            .unwrap()
            .contains("alpha fake read"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn run_assistant_turn_fake_provider_can_drive_tool_error_branch() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");
        let root = std::env::temp_dir().join(format!(
            "kiana-fake-provider-tool-error-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "read a missing file"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("fake")),
                ("model".to_string(), json!("fake-model")),
                ("tools".to_string(), json!("Read")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("max_iterations".to_string(), json!(2)),
                (
                    "fake_provider_script".to_string(),
                    json!([
                        {
                            "type": "tool_call",
                            "id": "toolu_missing",
                            "name": "Read",
                            "input": { "file_path": "missing.txt" }
                        },
                        {
                            "type": "final_answer",
                            "text": "handled missing file"
                        }
                    ]),
                ),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "handled missing file");
        let tool_result = &result.messages[2].content[0];
        assert_eq!(tool_result["tool_use_id"], "toolu_missing");
        assert_eq!(tool_result["is_error"], true);
        assert!(tool_result["content"]
            .as_str()
            .unwrap()
            .contains("File does not exist"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn run_assistant_turn_fake_provider_returns_typed_provider_error() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");

        let error = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "fail"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("fake")),
                ("model".to_string(), json!("fake-model")),
                ("tools".to_string(), json!("")),
                (
                    "fake_provider_script".to_string(),
                    json!([
                        {
                            "type": "provider_error",
                            "code": "scripted_failure",
                            "message": "fake provider failed"
                        }
                    ]),
                ),
            ]),
        )
        .await
        .unwrap_err();

        let provider_error = error.downcast_ref::<ProviderError>().unwrap();
        assert_eq!(provider_error.code(), "scripted_failure");
        assert!(provider_error.to_string().contains("fake provider failed"));
    }

    #[tokio::test]
    async fn run_assistant_turn_fails_before_request_when_model_does_not_support_tools() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("KIANA_FAKE_PROVIDER_SCRIPT");

        let error = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "use a tool"
            })],
            &HashMap::from([
                ("provider".to_string(), json!("fake")),
                ("model".to_string(), json!("fake-text-only")),
                ("tools".to_string(), json!("Read")),
                (
                    "fake_provider_script".to_string(),
                    json!([
                        {
                            "type": "assistant_text",
                            "text": "this script step should not be consumed"
                        }
                    ]),
                ),
            ]),
        )
        .await
        .unwrap_err();

        let provider_error = error.downcast_ref::<ProviderError>().unwrap();
        assert_eq!(provider_error.code(), "unsupported_tools");
    }

    #[tokio::test]
    async fn run_assistant_turn_batches_read_only_tools_and_preserves_read_state_for_later_edit() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root =
            std::env::temp_dir().join(format!("kiana-runner-read-batch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let file_path = root.join("editable.txt");
        fs::write(&file_path, "alpha\n").unwrap();
        let file_path = file_path.to_string_lossy().to_string();
        let (base_url, state, server) =
            start_mock_read_sleep_edit_loop_server(file_path.clone()).await;

        let started_at = Instant::now();
        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "read, wait, then edit"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("read-sleep-edit-model")),
                ("tools".to_string(), json!("Read Sleep Edit")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("max_iterations".to_string(), json!(3)),
            ]),
        )
        .await
        .unwrap();
        let elapsed = started_at.elapsed();

        assert_eq!(result.text, "edit complete");
        assert_eq!(fs::read_to_string(&file_path).unwrap(), "omega\n");
        assert!(
            elapsed < Duration::from_millis(420),
            "read-only tool batch should overlap two 250ms sleeps; elapsed={elapsed:?}"
        );
        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 3);
        let first_tool_results = requests[1]["messages"].as_array().unwrap().last().unwrap()
            ["content"]
            .as_array()
            .unwrap();
        assert_eq!(first_tool_results[0]["tool_use_id"], "toolu_read");
        assert_eq!(first_tool_results[1]["tool_use_id"], "toolu_sleep_one");
        assert_eq!(first_tool_results[2]["tool_use_id"], "toolu_sleep_two");

        server.abort();
        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_applies_editable_file_options_to_tool_context() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-runner-editable-files-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let blocked_path = root.join("blocked.txt");
        fs::write(&blocked_path, "alpha\n").unwrap();
        let allowed_path = root.join("allowed.txt");
        fs::write(&allowed_path, "allowed\n").unwrap();
        let (base_url, state, server) =
            start_mock_read_sleep_edit_loop_server(blocked_path.to_string_lossy().to_string())
                .await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "read and edit"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("read-sleep-edit-model")),
                ("tools".to_string(), json!("Read Sleep Edit")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("editableFiles".to_string(), json!(["allowed.txt"])),
                ("max_iterations".to_string(), json!(3)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "edit complete");
        assert_eq!(fs::read_to_string(&blocked_path).unwrap(), "alpha\n");
        let requests = state.lock().unwrap().requests.clone();
        let edit_result = &requests[2]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_array()
            .unwrap()[0];
        assert_eq!(edit_result["type"], "tool_result");
        assert_eq!(edit_result["tool_use_id"], "toolu_edit");
        assert!(edit_result["content"]
            .as_str()
            .unwrap()
            .contains("not in the editable files set"));

        server.abort();
        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_applies_read_only_file_options_to_tool_context() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root =
            std::env::temp_dir().join(format!("kiana-runner-readonly-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let write_path = root.join("created.txt");
        let (base_url, state, server) =
            start_mock_tool_loop_server(write_path.to_string_lossy().to_string()).await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "create a file"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("tool-loop-model")),
                ("tools".to_string(), json!("Write")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("readOnlyFiles".to_string(), json!(["created.txt"])),
                ("max_iterations".to_string(), json!(2)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "write complete");
        assert!(!write_path.exists());
        let requests = state.lock().unwrap().requests.clone();
        let write_result = &requests[1]["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_array()
            .unwrap()[0];
        assert_eq!(write_result["type"], "tool_result");
        assert_eq!(write_result["tool_use_id"], "toolu_write");
        assert!(write_result["content"]
            .as_str()
            .unwrap()
            .contains("marked read-only"));

        server.abort();
        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_repairs_failed_checks_with_mock_model() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root =
            std::env::temp_dir().join(format!("kiana-runner-repair-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("scripts")).unwrap();
        let app_path = root.join("app.txt");
        fs::write(&app_path, "base\n").unwrap();
        fs::write(
            root.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\ngrep -q '^fixed$' app.txt\n",
        )
        .unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "initial"]);
        let (base_url, state, server) =
            start_mock_repair_checks_loop_server(app_path.to_string_lossy().to_string()).await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "edit the app and repair failed checks"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("repair-check-model")),
                ("tools".to_string(), json!("Read Edit")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("repairChecks".to_string(), json!(true)),
                ("max_iterations".to_string(), json!(6)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "repair complete");
        assert_eq!(fs::read_to_string(&app_path).unwrap(), "fixed\n");
        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 4);
        let repair_feedback = requests[2]["messages"].as_array().unwrap().last().unwrap()
            ["content"]
            .as_str()
            .unwrap();
        assert!(repair_feedback.contains("Repair checks failed"));
        assert!(repair_feedback.contains("repair_attempt: 1/1"));
        assert!(repair_feedback.contains("final_status: retrying"));
        assert!(repair_feedback.contains("check_summary: failed=1 skipped=0"));
        assert!(repair_feedback.contains("failed_checks: release_smoke"));
        assert!(repair_feedback.contains("release_smoke"));

        server.abort();
        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_repairs_failed_checks_with_mock_model() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-runner-stream-repair-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        let app_path = root.join("app.txt");
        fs::write(&app_path, "base\n").unwrap();
        fs::write(
            root.join("scripts").join("release-smoke.sh"),
            "#!/usr/bin/env bash\nset -euo pipefail\ngrep -q '^fixed$' app.txt\n",
        )
        .unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        run_git(&root, &["add", "."]);
        run_git(&root, &["commit", "-m", "initial"]);
        let (base_url, state, server) =
            start_mock_streaming_repair_checks_loop_server(app_path.to_string_lossy().to_string())
                .await;

        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
        let result = run_assistant_turn_streaming_with_runner_events_and_abort_signal(
            vec![json!({
                "role": "user",
                "content": "edit the app and repair failed checks"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-repair-check-model")),
                ("tools".to_string(), json!("Read Edit")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("repairChecks".to_string(), json!(true)),
                ("max_iterations".to_string(), json!(6)),
            ]),
            |_| Ok(()),
            None,
            abort_signal,
        )
        .await
        .unwrap();

        assert_eq!(result.text, "stream repair complete");
        assert_eq!(fs::read_to_string(&app_path).unwrap(), "fixed\n");
        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 4);
        let repair_feedback = requests[2]["messages"].as_array().unwrap().last().unwrap()
            ["content"]
            .as_str()
            .unwrap();
        assert!(repair_feedback.contains("Repair checks failed"));
        assert!(repair_feedback.contains("repair_attempt: 1/1"));
        assert!(repair_feedback.contains("final_status: retrying"));
        assert!(repair_feedback.contains("check_summary: failed=1 skipped=0"));
        assert!(repair_feedback.contains("failed_checks: release_smoke"));
        assert!(repair_feedback.contains("release_smoke"));

        server.abort();
        let _ = fs::remove_dir_all(root);
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_creates_assistant_checkpoint_for_last_assistant_diff() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-runner-assistant-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-runner-assistant-checkpoint-home-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        let tracked_path = root.join("tracked.txt");
        fs::write(&tracked_path, "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(&tracked_path, "base\nuser\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let assistant_path = root.join("assistant.txt");
        let (base_url, _state, server) =
            start_mock_tool_loop_server(assistant_path.to_string_lossy().to_string()).await;
        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "create a file"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("tool-loop-model")),
                ("tools".to_string(), json!("Write")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("session_id".to_string(), json!("session-runner")),
                ("turn_id".to_string(), json!("turn-runner-1")),
                ("max_iterations".to_string(), json!(2)),
            ]),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "write complete");

        let diff = kiana_commands::diff::DiffCommand;
        let diff_result = kiana_commands::Command::execute(
            &diff,
            kiana_commands::CommandContext {
                args: "--last-assistant --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root.clone()))]),
            },
        )
        .await
        .unwrap();
        let value: Value = serde_json::from_str(&diff_result.value).unwrap();

        assert_eq!(value["checkpoint"]["kind"], "assistant_turn");
        assert_eq!(value["checkpoint"]["session_id"], "session-runner");
        assert_eq!(value["checkpoint"]["turn_id"], "turn-runner-1");
        let patch = value["patch"].as_str().unwrap();
        assert!(patch.contains("+hello from tool"));
        assert!(
            !patch.contains("+user"),
            "runner-created assistant checkpoint must exclude pre-existing user changes:\n{patch}"
        );

        server.abort();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_undo_reports_late_user_edit_conflict() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-runner-undo-late-edit-{}",
            uuid::Uuid::new_v4()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-runner-undo-late-edit-home-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        let tracked_path = root.join("tracked.txt");
        fs::write(&tracked_path, "alpha\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);
        std::env::set_var("KIANA_HOME", &home);

        let (base_url, _state, server) =
            start_mock_read_sleep_edit_loop_server(tracked_path.to_string_lossy().to_string())
                .await;
        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "read and edit"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("read-sleep-edit-model")),
                ("tools".to_string(), json!("Read Sleep Edit")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("session_id".to_string(), json!("session-late-edit")),
                ("turn_id".to_string(), json!("turn-late-edit-1")),
                ("max_iterations".to_string(), json!(3)),
            ]),
        )
        .await
        .unwrap();
        assert_eq!(result.text, "edit complete");
        assert_eq!(fs::read_to_string(&tracked_path).unwrap(), "omega\n");

        fs::write(&tracked_path, "omega\nuser after assistant\n").unwrap();

        let checkpoint = kiana_commands::checkpoint::CheckpointCommand;
        let undo = kiana_commands::Command::execute(
            &checkpoint,
            kiana_commands::CommandContext {
                args: "undo --last-assistant --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root.clone()))]),
            },
        )
        .await
        .unwrap();
        let value: Value = serde_json::from_str(&undo.value).unwrap();

        assert_eq!(
            fs::read_to_string(&tracked_path).unwrap(),
            "omega\nuser after assistant\n"
        );
        assert_eq!(value["undone_files"].as_array().unwrap().len(), 0);
        assert_eq!(value["conflicts"][0]["path"], "tracked.txt");
        assert_eq!(
            value["conflicts"][0]["reason"],
            "changed_after_assistant_turn"
        );

        server.abort();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_creates_assistant_checkpoint_for_last_assistant_diff() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        let root = std::env::temp_dir().join(format!(
            "kiana-runner-stream-assistant-checkpoint-{}",
            uuid::Uuid::new_v4()
        ));
        let home = std::env::temp_dir().join(format!(
            "kiana-runner-stream-assistant-checkpoint-home-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        run_git(&root, &["init"]);
        run_git(&root, &["config", "user.email", "test@example.com"]);
        run_git(&root, &["config", "user.name", "Kiana Test"]);
        let tracked_path = root.join("tracked.txt");
        fs::write(&tracked_path, "base\n").unwrap();
        run_git(&root, &["add", "tracked.txt"]);
        run_git(&root, &["commit", "-m", "initial"]);
        fs::write(&tracked_path, "base\nuser\n").unwrap();
        std::env::set_var("KIANA_HOME", &home);

        let assistant_path = root.join("assistant.txt");
        let (base_url, _state, server) =
            start_mock_streaming_write_loop_server(assistant_path.to_string_lossy().to_string())
                .await;
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
        let result = run_assistant_turn_streaming_with_runner_events_and_abort_signal(
            vec![json!({
                "role": "user",
                "content": "create a file"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-write-loop-model")),
                ("tools".to_string(), json!("Write")),
                ("cwd".to_string(), json!(root.to_string_lossy())),
                ("session_id".to_string(), json!("session-stream-runner")),
                ("turn_id".to_string(), json!("turn-stream-runner-1")),
                ("max_iterations".to_string(), json!(2)),
            ]),
            |_| Ok(()),
            None,
            abort_signal,
        )
        .await
        .unwrap();
        assert_eq!(result.text, "stream write complete");

        let diff = kiana_commands::diff::DiffCommand;
        let diff_result = kiana_commands::Command::execute(
            &diff,
            kiana_commands::CommandContext {
                args: "--last-assistant --json".to_string(),
                app_state: HashMap::from([("cwd".to_string(), json!(root.clone()))]),
            },
        )
        .await
        .unwrap();
        let value: Value = serde_json::from_str(&diff_result.value).unwrap();

        assert_eq!(value["checkpoint"]["kind"], "assistant_turn");
        assert_eq!(value["checkpoint"]["session_id"], "session-stream-runner");
        assert_eq!(value["checkpoint"]["turn_id"], "turn-stream-runner-1");
        let patch = value["patch"].as_str().unwrap();
        assert!(patch.contains("+hello from streaming tool"));
        assert!(
            !patch.contains("+user"),
            "streaming runner-created assistant checkpoint must exclude pre-existing user changes:\n{patch}"
        );

        server.abort();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(home);
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_uses_permission_handler_for_tool_calls() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        std::env::set_var("KIANA_PERMISSION_MODE", "ask");
        let (base_url, state, server) = start_mock_streaming_tool_loop_server().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler = RecordingPermissionPromptHandler {
            requests: requests.clone(),
            decision: PermissionPromptDecision::Allow,
        };

        let result = run_assistant_turn_streaming_with_permission_handler(
            vec![json!({
                "role": "user",
                "content": "update todos"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-tool-loop-model")),
                ("tools".to_string(), json!("TodoWrite")),
                ("permission_prompt_tool".to_string(), json!("stdio")),
                ("max_iterations".to_string(), json!(2)),
            ]),
            |_| Ok(()),
            Some(&handler),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "stream done");
        let prompt_requests = requests.lock().unwrap();
        assert_eq!(prompt_requests.len(), 1);
        assert_eq!(prompt_requests[0].tool_name, "TodoWrite");
        assert_eq!(prompt_requests[0].tool_use_id, "toolu_todo");
        assert_eq!(
            prompt_requests[0].input["todos"][0]["content"],
            "verify streaming permissions"
        );
        assert!(prompt_requests[0].decision_reason["reason"]
            .as_str()
            .unwrap()
            .contains("ask mode"));

        let model_requests = state.lock().unwrap().requests.clone();
        assert_eq!(model_requests.len(), 2);
        let tool_result = &model_requests[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_todo");
        assert!(tool_result["content"]
            .as_str()
            .unwrap()
            .contains("Todos have been modified successfully"));

        server.abort();
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_emits_local_tool_result_events() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        std::env::set_var("KIANA_PERMISSION_MODE", "ask");
        let (base_url, _state, server) = start_mock_streaming_tool_loop_server().await;
        let handler = RecordingPermissionPromptHandler {
            requests: Arc::new(Mutex::new(Vec::new())),
            decision: PermissionPromptDecision::Allow,
        };
        let mut events = Vec::new();
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);

        let result = run_assistant_turn_streaming_with_runner_events_and_abort_signal(
            vec![json!({
                "role": "user",
                "content": "update todos"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-tool-loop-model")),
                ("tools".to_string(), json!("TodoWrite")),
                ("permission_prompt_tool".to_string(), json!("stdio")),
                ("max_iterations".to_string(), json!(2)),
            ]),
            |event| {
                events.push(event);
                Ok(())
            },
            Some(&handler),
            abort_signal,
        )
        .await
        .unwrap();

        assert_eq!(result.text, "stream done");
        let tool_result = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::ToolResult {
                    id,
                    name,
                    is_error,
                    content,
                    ..
                } => Some((id, name, is_error, content)),
                RunnerStreamEvent::Model(_) => None,
            })
            .expect("runner should emit a local tool result event");
        assert_eq!(tool_result.0, "toolu_todo");
        assert_eq!(tool_result.1, "TodoWrite");
        assert!(!tool_result.2);
        assert!(tool_result
            .3
            .contains("Todos have been modified successfully"));

        server.abort();
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_surfaces_mcp_error_lifecycle_events() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        std::env::set_var("KIANA_PERMISSION_MODE", "ask");
        let (mcp_url, mcp_state, mcp_server) = start_mock_error_mcp_server().await;
        let (base_url, model_state, model_server) =
            start_mock_streaming_mcp_error_loop_server(mcp_url, "http").await;
        let handler = RecordingPermissionPromptHandler {
            requests: Arc::new(Mutex::new(Vec::new())),
            decision: PermissionPromptDecision::Allow,
        };
        let mut events = Vec::new();
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);

        let result = run_assistant_turn_streaming_with_runner_events_and_abort_signal(
            vec![json!({
                "role": "user",
                "content": "run failing mcp tool"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-mcp-error-model")),
                ("tools".to_string(), json!("MCP")),
                ("permission_prompt_tool".to_string(), json!("stdio")),
                ("max_iterations".to_string(), json!(2)),
            ]),
            |event| {
                events.push(event);
                Ok(())
            },
            Some(&handler),
            abort_signal,
        )
        .await
        .unwrap();

        assert_eq!(result.text, "mcp handled");
        let tool_result = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::ToolResult {
                    id,
                    name,
                    is_error,
                    content,
                    ..
                } => Some((id, name, is_error, content)),
                RunnerStreamEvent::Model(_) => None,
            })
            .expect("runner should emit an MCP tool result event");
        assert_eq!(tool_result.0, "toolu_mcp");
        assert_eq!(tool_result.1, "MCP");
        assert!(tool_result.2);
        assert!(tool_result.3.contains("denied by fake server"));

        let tool_call_event = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::Model(StreamEvent::ContentBlockStart {
                    content_block: StreamContentBlock::ToolUse(tool_use),
                    ..
                }) if tool_use.id == "toolu_mcp" => Some(event.clone()),
                _ => None,
            })
            .expect("runner should emit the MCP tool-use stream event");
        let runtime_call_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-1",
            None,
            1,
            "2026-06-24T00:00:01Z",
            tool_call_event,
        );
        let runtime_call_event = serde_json::to_value(&runtime_call_events[0]).unwrap();
        assert_eq!(runtime_call_event["type"], "tool_call");
        assert_eq!(runtime_call_event["tool_call_id"], "toolu_mcp");
        assert_eq!(runtime_call_event["name"], "MCP");
        assert_eq!(runtime_call_event["workbench"], "mcp");

        let runtime_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-24T00:00:02Z",
            RunnerStreamEvent::ToolResult {
                id: tool_result.0.clone(),
                name: tool_result.1.clone(),
                is_error: *tool_result.2,
                content: tool_result.3.clone(),
                error: None,
            },
        );
        let runtime_event = serde_json::to_value(&runtime_events[0]).unwrap();
        assert_eq!(runtime_event["type"], "tool_result");
        assert_eq!(runtime_event["tool_call_id"], "toolu_mcp");
        assert_eq!(runtime_event["name"], "MCP");
        assert_eq!(runtime_event["workbench"], "mcp");
        assert_eq!(runtime_event["is_error"], true);
        assert!(runtime_event["content"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));

        let calls = mcp_state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool_name"], "fail");

        let model_requests = model_state.lock().unwrap().requests.clone();
        assert_eq!(model_requests.len(), 2);
        let tool_result_block = &model_requests[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["content"][0];
        assert_eq!(tool_result_block["type"], "tool_result");
        assert_eq!(tool_result_block["tool_use_id"], "toolu_mcp");
        assert_eq!(tool_result_block["is_error"], true);
        assert!(tool_result_block["content"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));

        model_server.abort();
        mcp_server.abort();
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_surfaces_sse_mcp_error_lifecycle_events() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        std::env::set_var("KIANA_PERMISSION_MODE", "ask");
        let (mcp_url, mcp_state, mcp_server) = start_mock_error_sse_mcp_server().await;
        let (base_url, model_state, model_server) =
            start_mock_streaming_mcp_error_loop_server(mcp_url, "sse").await;
        let handler = RecordingPermissionPromptHandler {
            requests: Arc::new(Mutex::new(Vec::new())),
            decision: PermissionPromptDecision::Allow,
        };
        let mut events = Vec::new();
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);

        let result = run_assistant_turn_streaming_with_runner_events_and_abort_signal(
            vec![json!({
                "role": "user",
                "content": "run failing sse mcp tool"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-mcp-error-model")),
                ("tools".to_string(), json!("MCP")),
                ("permission_prompt_tool".to_string(), json!("stdio")),
                ("max_iterations".to_string(), json!(2)),
            ]),
            |event| {
                events.push(event);
                Ok(())
            },
            Some(&handler),
            abort_signal,
        )
        .await
        .unwrap();

        assert_eq!(result.text, "mcp handled");
        let tool_result = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::ToolResult {
                    id,
                    name,
                    is_error,
                    content,
                    ..
                } => Some((id, name, is_error, content)),
                RunnerStreamEvent::Model(_) => None,
            })
            .expect("runner should emit an SSE MCP tool result event");
        assert_eq!(tool_result.0, "toolu_mcp");
        assert_eq!(tool_result.1, "MCP");
        assert!(tool_result.2);
        assert!(tool_result.3.contains("denied by fake server"));

        let tool_call_event = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::Model(StreamEvent::ContentBlockStart {
                    content_block: StreamContentBlock::ToolUse(tool_use),
                    ..
                }) if tool_use.id == "toolu_mcp" => Some(event.clone()),
                _ => None,
            })
            .expect("runner should emit the SSE MCP tool-use stream event");
        let runtime_call_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-1",
            None,
            1,
            "2026-06-24T00:00:01Z",
            tool_call_event,
        );
        let runtime_call_event = serde_json::to_value(&runtime_call_events[0]).unwrap();
        assert_eq!(runtime_call_event["type"], "tool_call");
        assert_eq!(runtime_call_event["tool_call_id"], "toolu_mcp");
        assert_eq!(runtime_call_event["name"], "MCP");
        assert_eq!(runtime_call_event["workbench"], "mcp");

        let runtime_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-24T00:00:02Z",
            RunnerStreamEvent::ToolResult {
                id: tool_result.0.clone(),
                name: tool_result.1.clone(),
                is_error: *tool_result.2,
                content: tool_result.3.clone(),
                error: None,
            },
        );
        let runtime_event = serde_json::to_value(&runtime_events[0]).unwrap();
        assert_eq!(runtime_event["type"], "tool_result");
        assert_eq!(runtime_event["tool_call_id"], "toolu_mcp");
        assert_eq!(runtime_event["name"], "MCP");
        assert_eq!(runtime_event["workbench"], "mcp");
        assert_eq!(runtime_event["is_error"], true);
        assert!(runtime_event["content"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));

        let calls = mcp_state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool_name"], "fail");

        let model_requests = model_state.lock().unwrap().requests.clone();
        assert_eq!(model_requests.len(), 2);
        let tool_result_block = &model_requests[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["content"][0];
        assert_eq!(tool_result_block["type"], "tool_result");
        assert_eq!(tool_result_block["tool_use_id"], "toolu_mcp");
        assert_eq!(tool_result_block["is_error"], true);
        assert!(tool_result_block["content"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));

        model_server.abort();
        mcp_server.abort();
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_streaming_surfaces_ws_mcp_error_lifecycle_events() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        isolate_permission_env();
        std::env::set_var("KIANA_PERMISSION_MODE", "ask");
        let (mcp_url, mcp_state, mcp_server) = start_mock_error_ws_mcp_server().await;
        let (base_url, model_state, model_server) =
            start_mock_streaming_mcp_error_loop_server(mcp_url, "ws").await;
        let handler = RecordingPermissionPromptHandler {
            requests: Arc::new(Mutex::new(Vec::new())),
            decision: PermissionPromptDecision::Allow,
        };
        let mut events = Vec::new();
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);

        let result = run_assistant_turn_streaming_with_runner_events_and_abort_signal(
            vec![json!({
                "role": "user",
                "content": "run failing ws mcp tool"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("stream-mcp-error-model")),
                ("tools".to_string(), json!("MCP")),
                ("permission_prompt_tool".to_string(), json!("stdio")),
                ("max_iterations".to_string(), json!(2)),
            ]),
            |event| {
                events.push(event);
                Ok(())
            },
            Some(&handler),
            abort_signal,
        )
        .await
        .unwrap();

        assert_eq!(result.text, "mcp handled");
        let tool_result = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::ToolResult {
                    id,
                    name,
                    is_error,
                    content,
                    ..
                } => Some((id, name, is_error, content)),
                RunnerStreamEvent::Model(_) => None,
            })
            .expect("runner should emit a WebSocket MCP tool result event");
        assert_eq!(tool_result.0, "toolu_mcp");
        assert_eq!(tool_result.1, "MCP");
        assert!(tool_result.2);
        assert!(tool_result.3.contains("denied by fake server"));

        let tool_call_event = events
            .iter()
            .find_map(|event| match event {
                RunnerStreamEvent::Model(StreamEvent::ContentBlockStart {
                    content_block: StreamContentBlock::ToolUse(tool_use),
                    ..
                }) if tool_use.id == "toolu_mcp" => Some(event.clone()),
                _ => None,
            })
            .expect("runner should emit the WebSocket MCP tool-use stream event");
        let runtime_call_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-1",
            None,
            1,
            "2026-06-24T00:00:01Z",
            tool_call_event,
        );
        let runtime_call_event = serde_json::to_value(&runtime_call_events[0]).unwrap();
        assert_eq!(runtime_call_event["type"], "tool_call");
        assert_eq!(runtime_call_event["tool_call_id"], "toolu_mcp");
        assert_eq!(runtime_call_event["name"], "MCP");
        assert_eq!(runtime_call_event["workbench"], "mcp");

        let runtime_events = runtime_events_from_runner_stream_event(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-24T00:00:02Z",
            RunnerStreamEvent::ToolResult {
                id: tool_result.0.clone(),
                name: tool_result.1.clone(),
                is_error: *tool_result.2,
                content: tool_result.3.clone(),
                error: None,
            },
        );
        let runtime_event = serde_json::to_value(&runtime_events[0]).unwrap();
        assert_eq!(runtime_event["type"], "tool_result");
        assert_eq!(runtime_event["tool_call_id"], "toolu_mcp");
        assert_eq!(runtime_event["name"], "MCP");
        assert_eq!(runtime_event["workbench"], "mcp");
        assert_eq!(runtime_event["is_error"], true);
        assert!(runtime_event["content"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));

        let calls = mcp_state.lock().unwrap().calls.clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["tool_name"], "fail");

        let model_requests = model_state.lock().unwrap().requests.clone();
        assert_eq!(model_requests.len(), 2);
        let tool_result_block = &model_requests[1]["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["content"][0];
        assert_eq!(tool_result_block["type"], "tool_result");
        assert_eq!(tool_result_block["tool_use_id"], "toolu_mcp");
        assert_eq!(tool_result_block["is_error"], true);
        assert!(tool_result_block["content"]
            .as_str()
            .unwrap()
            .contains("denied by fake server"));

        model_server.abort();
        mcp_server.abort();
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_PERMISSIONS_FILE");
    }

    #[tokio::test]
    async fn run_assistant_turn_sends_web_fetch_network_policy_error_to_model() {
        let _guard = env_lock().lock().unwrap();
        clear_team_env();
        clear_thinking_env();
        clear_max_tokens_env();
        let (base_url, state, model_server) =
            start_mock_web_fetch_loop_server("http://127.0.0.1/page".to_string()).await;

        let result = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "fetch a page"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("base_url".to_string(), json!(base_url)),
                ("model".to_string(), json!("web-fetch-loop-model")),
                ("tools".to_string(), json!("WebFetch")),
                ("max_iterations".to_string(), json!(2)),
            ]),
        )
        .await
        .unwrap();

        assert_eq!(result.text, "fetch complete");
        let requests = state.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 2);
        let second_messages = requests[1]["messages"].as_array().unwrap();
        let tool_result = &second_messages.last().unwrap()["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        assert_eq!(tool_result["tool_use_id"], "toolu_web_fetch");
        assert_eq!(tool_result["is_error"], true);
        let content = tool_result["content"].as_str().unwrap();
        assert!(content.contains("network policy denied URL"));
        assert!(content.contains("127.0.0.1"));
        assert!(!content.contains("Status: 200"));

        model_server.abort();
    }

    #[tokio::test]
    async fn run_assistant_turn_rejects_identical_fallback_model() {
        let error = run_assistant_turn(
            vec![json!({
                "role": "user",
                "content": "hello"
            })],
            &HashMap::from([
                ("api_key".to_string(), json!("test-key")),
                ("model".to_string(), json!("same-model")),
                ("fallback_model".to_string(), json!("same-model")),
            ]),
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("Fallback model cannot be the same"));
    }

    #[test]
    fn access_roots_option_accepts_array_and_string_aliases() {
        let options = HashMap::from([(
            "add_dirs".to_string(),
            json!(["/workspace/one", "/workspace/two"]),
        )]);
        assert_eq!(
            access_roots_option(&options),
            Some(vec![
                "/workspace/one".to_string(),
                "/workspace/two".to_string()
            ])
        );

        let options = HashMap::from([(
            "additionalDirectories".to_string(),
            json!("/workspace/one,/workspace/two"),
        )]);
        assert_eq!(
            access_roots_option(&options),
            Some(vec![
                "/workspace/one".to_string(),
                "/workspace/two".to_string()
            ])
        );
    }

    #[test]
    fn mcp_servers_option_accepts_options_and_env() {
        let _guard = env_lock().lock().unwrap();
        std::env::remove_var("KIANA_STRICT_MCP_CONFIG");
        std::env::remove_var(MCP_SERVERS_ENV);

        let options = HashMap::from([(
            "mcpConfig".to_string(),
            json!({
                "mcpServers": {
                    "docs": {
                        "command": "docs-mcp",
                        "args": ["--stdio"]
                    }
                }
            }),
        )]);
        let servers = mcp_servers_option(&options).unwrap().unwrap();
        assert_eq!(servers["docs"]["command"], "docs-mcp");
        assert_eq!(servers["docs"]["args"][0], "--stdio");

        std::env::set_var(
            MCP_SERVERS_ENV,
            r#"{"env-docs":{"url":"http://127.0.0.1/mcp","type":"http"}}"#,
        );
        let servers = mcp_servers_option(&HashMap::new()).unwrap().unwrap();
        assert_eq!(servers["env-docs"]["type"], "http");
        std::env::remove_var(MCP_SERVERS_ENV);
    }

    #[test]
    fn sandbox_option_accepts_options_and_config_default() {
        let options = HashMap::from([("bashSandbox".to_string(), json!(true))]);
        let sandbox = sandbox_option(&options, None).unwrap().unwrap();
        assert_eq!(sandbox["enabled"], true);

        let options = HashMap::from([(
            "sandbox".to_string(),
            json!({
                "enabled": true,
                "failIfUnavailable": true
            }),
        )]);
        let sandbox = sandbox_option(&options, None).unwrap().unwrap();
        assert_eq!(sandbox["failIfUnavailable"], true);

        let sandbox = sandbox_option(
            &HashMap::new(),
            Some(json!({
                "enabled": true,
                "allowUnsandboxedCommands": false
            })),
        )
        .unwrap()
        .unwrap();
        assert_eq!(sandbox["allowUnsandboxedCommands"], false);

        let sandbox = sandbox_option(&HashMap::new(), Some(json!(true)))
            .unwrap()
            .unwrap();
        assert_eq!(sandbox["enabled"], true);

        let error = sandbox_option(&HashMap::new(), Some(json!("yes"))).unwrap_err();
        assert!(error
            .to_string()
            .contains("sandbox option must be an object or boolean"));
    }

    #[test]
    fn mcp_servers_option_supports_strict_empty_config() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(
            MCP_SERVERS_ENV,
            r#"{"env-docs":{"url":"http://127.0.0.1/mcp","type":"http"}}"#,
        );
        std::env::remove_var("KIANA_STRICT_MCP_CONFIG");

        let options = HashMap::from([("strict_mcp_config".to_string(), json!(true))]);
        let servers = mcp_servers_option(&options).unwrap().unwrap();
        assert_eq!(servers, json!({}));

        std::env::set_var("KIANA_STRICT_MCP_CONFIG", "true");
        let servers = mcp_servers_option(&HashMap::new()).unwrap().unwrap();
        assert_eq!(servers, json!({}));

        std::env::remove_var("KIANA_STRICT_MCP_CONFIG");
        std::env::remove_var(MCP_SERVERS_ENV);
    }

    #[test]
    fn mcp_servers_option_supports_simple_empty_config() {
        let _guard = env_lock().lock().unwrap();
        std::env::set_var(
            MCP_SERVERS_ENV,
            r#"{"env-docs":{"url":"http://127.0.0.1/mcp","type":"http"}}"#,
        );
        std::env::remove_var("CLAUDE_CODE_SIMPLE");
        std::env::remove_var("KIANA_CODE_SIMPLE");

        let options = HashMap::from([("bare".to_string(), json!(true))]);
        let servers = mcp_servers_option(&options).unwrap().unwrap();
        assert_eq!(servers, json!({}));

        std::env::set_var("CLAUDE_CODE_SIMPLE", "1");
        let servers = mcp_servers_option(&HashMap::new()).unwrap().unwrap();
        assert_eq!(servers, json!({}));

        std::env::remove_var("CLAUDE_CODE_SIMPLE");
        std::env::remove_var("KIANA_CODE_SIMPLE");
        std::env::remove_var(MCP_SERVERS_ENV);
    }

    #[tokio::test]
    async fn call_tool_rejects_tools_disabled_for_run() {
        let registry = create_default_registry();
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        let mut context = ToolContext {
            cwd: ".".to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        };
        let enabled = HashSet::from(["Read".to_string()]);

        let result = call_tool(
            &registry,
            Some(&enabled),
            &mut context,
            "TaskCreate",
            &json!({ "title": "blocked" }),
            None,
            None,
        )
        .await;

        assert!(result.is_error);
        assert!(result.content.as_str().unwrap().contains("not enabled"));
    }

    #[derive(Debug, Default)]
    struct MockMessagesState {
        requested_models: Vec<String>,
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockOpenAiCompatibleState {
        auth_headers: Vec<String>,
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockOllamaState {
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockToolLoopState {
        write_path: String,
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockStreamingToolLoopState {
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockStreamingMcpErrorLoopState {
        mcp_url: String,
        transport: String,
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockReadSleepEditLoopState {
        file_path: String,
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockRepairChecksLoopState {
        file_path: String,
        requests: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockWebFetchLoopState {
        fetch_url: String,
        requests: Vec<Value>,
    }

    #[derive(Debug)]
    struct MockPermissionMcpState {
        decision: Value,
        calls: Vec<Value>,
    }

    #[derive(Debug, Default)]
    struct MockErrorMcpState {
        endpoint: Option<String>,
        calls: Vec<Value>,
    }

    async fn start_mock_messages_server() -> (String, Arc<Mutex<MockMessagesState>>, JoinHandle<()>)
    {
        let state = Arc::new(Mutex::new(MockMessagesState::default()));
        let app = Router::new()
            .route("/v1/messages", post(handle_mock_messages_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn start_mock_openai_compatible_server() -> (
        String,
        Arc<Mutex<MockOpenAiCompatibleState>>,
        JoinHandle<()>,
    ) {
        let state = Arc::new(Mutex::new(MockOpenAiCompatibleState::default()));
        let app = Router::new()
            .route(
                "/chat/completions",
                post(handle_mock_openai_compatible_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn start_mock_ollama_server() -> (String, Arc<Mutex<MockOllamaState>>, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(MockOllamaState::default()));
        let app = Router::new()
            .route("/api/chat", post(handle_mock_ollama_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_messages_request(
        State(state): State<Arc<Mutex<MockMessagesState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let call_count = {
            let mut state = state.lock().unwrap();
            state.requested_models.push(model.clone());
            state.requests.push(body.clone());
            state.requested_models.len()
        };

        if call_count == 1 {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": {
                        "type": "overloaded_error",
                        "message": "model overloaded"
                    }
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "id": "msg_mock",
                "model": model,
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "fallback ok"
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

    async fn handle_mock_openai_compatible_request(
        State(state): State<Arc<Mutex<MockOpenAiCompatibleState>>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let auth_header = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let call_count = {
            let mut state = state.lock().unwrap();
            state.auth_headers.push(auth_header);
            state.requests.push(body.clone());
            state.requests.len()
        };

        if call_count == 1 && body.get("tools").is_some_and(|tools| !tools.is_null()) {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "chatcmpl_tool_mock",
                    "model": model,
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [{
                                "id": "call_todo",
                                "type": "function",
                                "function": {
                                    "name": "TodoWrite",
                                    "arguments": "{\"todos\":[{\"content\":\"verify OpenAI-compatible tools\",\"status\":\"in_progress\",\"activeForm\":\"verifying OpenAI-compatible tools\"}]}"
                                }
                            }]
                        },
                        "finish_reason": "tool_calls"
                    }],
                    "usage": {
                        "prompt_tokens": 2,
                        "completion_tokens": 3
                    }
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "id": "chatcmpl_mock",
                "model": model,
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": if call_count > 1 { "openai tool loop ok" } else { "openai compatible ok" }
                    },
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 2,
                    "completion_tokens": 3
                }
            })),
        )
            .into_response()
    }

    async fn handle_mock_ollama_request(
        State(state): State<Arc<Mutex<MockOllamaState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let call_count = {
            let mut state = state.lock().unwrap();
            state.requests.push(body.clone());
            state.requests.len()
        };

        if call_count == 1 && body.get("tools").is_some_and(|tools| !tools.is_null()) {
            return (
                StatusCode::OK,
                Json(json!({
                    "model": model,
                    "created_at": "2026-07-01T00:00:00Z",
                    "message": {
                        "role": "assistant",
                        "content": "",
                        "tool_calls": [{
                            "function": {
                                "name": "TodoWrite",
                                "arguments": {
                                    "todos": [{
                                        "content": "verify Ollama tools",
                                        "status": "in_progress",
                                        "activeForm": "verifying Ollama tools"
                                    }]
                                }
                            }
                        }]
                    },
                    "done": true,
                    "done_reason": "stop",
                    "prompt_eval_count": 2,
                    "eval_count": 3
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "model": model,
                "created_at": "2026-07-01T00:00:00Z",
                "message": {
                    "role": "assistant",
                    "content": if call_count > 1 { "ollama tool loop ok" } else { "ollama ok" }
                },
                "done": true,
                "done_reason": "stop",
                "prompt_eval_count": 2,
                "eval_count": 3
            })),
        )
            .into_response()
    }

    async fn start_mock_tool_loop_server(
        write_path: String,
    ) -> (String, Arc<Mutex<MockToolLoopState>>, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(MockToolLoopState {
            write_path,
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route("/v1/messages", post(handle_mock_tool_loop_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_tool_loop_request(
        State(state): State<Arc<Mutex<MockToolLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, write_path) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (state.requests.len(), state.write_path.clone())
        };

        if call_count == 1 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_tool_use",
                    "model": "tool-loop-model",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "creating it"
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_write",
                            "name": "Write",
                            "input": {
                                "file_path": write_path,
                                "content": "hello from tool\n"
                            }
                        }
                    ],
                    "stop_reason": "tool_use",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "id": "msg_tool_done",
                "model": "tool-loop-model",
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "write complete"
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

    async fn start_mock_repair_checks_loop_server(
        file_path: String,
    ) -> (
        String,
        Arc<Mutex<MockRepairChecksLoopState>>,
        JoinHandle<()>,
    ) {
        let state = Arc::new(Mutex::new(MockRepairChecksLoopState {
            file_path,
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route("/v1/messages", post(handle_mock_repair_checks_loop_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_repair_checks_loop_request(
        State(state): State<Arc<Mutex<MockRepairChecksLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, file_path) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (state.requests.len(), state.file_path.clone())
        };

        if call_count == 1 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_repair_use",
                    "model": "repair-check-model",
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_read_fail",
                        "name": "Read",
                        "input": {
                            "file_path": file_path
                        }
                    },{
                        "type": "tool_use",
                        "id": "toolu_edit_fail",
                        "name": "Edit",
                        "input": {
                            "file_path": file_path,
                            "old_string": "base",
                            "new_string": "broken"
                        }
                    }],
                    "stop_reason": "tool_use",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        if call_count == 2 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_repair_before_checks",
                    "model": "repair-check-model",
                    "role": "assistant",
                    "content": [{
                        "type": "text",
                        "text": "first pass complete"
                    }],
                    "stop_reason": "end_turn",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        if call_count == 3 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_repair_fix",
                    "model": "repair-check-model",
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_read_fix",
                        "name": "Read",
                        "input": {
                            "file_path": file_path
                        }
                    },{
                        "type": "tool_use",
                        "id": "toolu_edit_fix",
                        "name": "Edit",
                        "input": {
                            "file_path": file_path,
                            "old_string": "broken",
                            "new_string": "fixed"
                        }
                    }],
                    "stop_reason": "tool_use",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "id": "msg_repair_done",
                "model": "repair-check-model",
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "repair complete"
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

    async fn start_mock_streaming_repair_checks_loop_server(
        file_path: String,
    ) -> (
        String,
        Arc<Mutex<MockRepairChecksLoopState>>,
        JoinHandle<()>,
    ) {
        let state = Arc::new(Mutex::new(MockRepairChecksLoopState {
            file_path,
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route(
                "/v1/messages",
                post(handle_mock_streaming_repair_checks_loop_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_streaming_repair_checks_loop_request(
        State(state): State<Arc<Mutex<MockRepairChecksLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, file_path) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (state.requests.len(), state.file_path.clone())
        };

        if call_count == 1 {
            return mock_sse_response(vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream_repair_use",
                        "model": "stream-repair-check-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_stream_read_fail",
                        "name": "Read",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "file_path": file_path
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_stream_edit_fail",
                        "name": "Edit",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "file_path": file_path,
                            "old_string": "base",
                            "new_string": "broken"
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 1}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 1}
                }),
                json!({"type": "message_stop"}),
            ]);
        }

        if call_count == 2 {
            return mock_sse_response(vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream_repair_before_checks",
                        "model": "stream-repair-check-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {"type": "text", "text": ""}
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": "first stream pass complete"}
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "end_turn"},
                    "usage": {"input_tokens": 0, "output_tokens": 1}
                }),
                json!({"type": "message_stop"}),
            ]);
        }

        if call_count == 3 {
            return mock_sse_response(vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream_repair_fix",
                        "model": "stream-repair-check-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_stream_read_fix",
                        "name": "Read",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "file_path": file_path
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "content_block_start",
                    "index": 1,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_stream_edit_fix",
                        "name": "Edit",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 1,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "file_path": file_path,
                            "old_string": "broken",
                            "new_string": "fixed"
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 1}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 1}
                }),
                json!({"type": "message_stop"}),
            ]);
        }

        mock_sse_response(vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_stream_repair_done",
                    "model": "stream-repair-check-model",
                    "role": "assistant",
                    "usage": {"input_tokens": 1, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "stream repair complete"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 0, "output_tokens": 1}
            }),
            json!({"type": "message_stop"}),
        ])
    }

    async fn start_mock_read_sleep_edit_loop_server(
        file_path: String,
    ) -> (
        String,
        Arc<Mutex<MockReadSleepEditLoopState>>,
        JoinHandle<()>,
    ) {
        let state = Arc::new(Mutex::new(MockReadSleepEditLoopState {
            file_path,
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route(
                "/v1/messages",
                post(handle_mock_read_sleep_edit_loop_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_read_sleep_edit_loop_request(
        State(state): State<Arc<Mutex<MockReadSleepEditLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, file_path) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (state.requests.len(), state.file_path.clone())
        };

        if call_count == 1 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_read_sleep_use",
                    "model": "read-sleep-edit-model",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "tool_use",
                            "id": "toolu_read",
                            "name": "Read",
                            "input": {
                                "file_path": file_path
                            }
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_sleep_one",
                            "name": "Sleep",
                            "input": {
                                "duration_ms": 250
                            }
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_sleep_two",
                            "name": "Sleep",
                            "input": {
                                "duration_ms": 250
                            }
                        }
                    ],
                    "stop_reason": "tool_use",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        if call_count == 2 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_edit_use",
                    "model": "read-sleep-edit-model",
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_edit",
                        "name": "Edit",
                        "input": {
                            "file_path": file_path,
                            "old_string": "alpha",
                            "new_string": "omega"
                        }
                    }],
                    "stop_reason": "tool_use",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "id": "msg_edit_done",
                "model": "read-sleep-edit-model",
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "edit complete"
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

    async fn start_mock_streaming_write_loop_server(
        write_path: String,
    ) -> (String, Arc<Mutex<MockToolLoopState>>, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(MockToolLoopState {
            write_path,
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route(
                "/v1/messages",
                post(handle_mock_streaming_write_loop_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_streaming_write_loop_request(
        State(state): State<Arc<Mutex<MockToolLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, write_path) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (state.requests.len(), state.write_path.clone())
        };

        if call_count == 1 {
            return mock_sse_response(vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream_write_tool_use",
                        "model": "stream-write-loop-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_stream_write",
                        "name": "Write",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "file_path": write_path,
                            "content": "hello from streaming tool\n"
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 1}
                }),
                json!({"type": "message_stop"}),
            ]);
        }

        mock_sse_response(vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_stream_write_done",
                    "model": "stream-write-loop-model",
                    "role": "assistant",
                    "usage": {"input_tokens": 1, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "stream write complete"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 0, "output_tokens": 1}
            }),
            json!({"type": "message_stop"}),
        ])
    }

    async fn start_mock_streaming_tool_loop_server() -> (
        String,
        Arc<Mutex<MockStreamingToolLoopState>>,
        JoinHandle<()>,
    ) {
        let state = Arc::new(Mutex::new(MockStreamingToolLoopState::default()));
        let app = Router::new()
            .route(
                "/v1/messages",
                post(handle_mock_streaming_tool_loop_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_streaming_tool_loop_request(
        State(state): State<Arc<Mutex<MockStreamingToolLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let call_count = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            state.requests.len()
        };

        if call_count == 1 {
            return mock_sse_response(vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream_tool_use",
                        "model": "stream-tool-loop-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_todo",
                        "name": "TodoWrite",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "todos": [{
                                "content": "verify streaming permissions",
                                "status": "in_progress",
                                "activeForm": "Verifying streaming permissions"
                            }]
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 1}
                }),
                json!({"type": "message_stop"}),
            ]);
        }

        mock_sse_response(vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_stream_done",
                    "model": "stream-tool-loop-model",
                    "role": "assistant",
                    "usage": {"input_tokens": 1, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "stream done"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 0, "output_tokens": 1}
            }),
            json!({"type": "message_stop"}),
        ])
    }

    async fn start_mock_streaming_mcp_error_loop_server(
        mcp_url: String,
        transport: &str,
    ) -> (
        String,
        Arc<Mutex<MockStreamingMcpErrorLoopState>>,
        JoinHandle<()>,
    ) {
        let state = Arc::new(Mutex::new(MockStreamingMcpErrorLoopState {
            mcp_url,
            transport: transport.to_string(),
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route(
                "/v1/messages",
                post(handle_mock_streaming_mcp_error_loop_request),
            )
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_streaming_mcp_error_loop_request(
        State(state): State<Arc<Mutex<MockStreamingMcpErrorLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, mcp_url, transport) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (
                state.requests.len(),
                state.mcp_url.clone(),
                state.transport.clone(),
            )
        };

        if call_count == 1 {
            return mock_sse_response(vec![
                json!({
                    "type": "message_start",
                    "message": {
                        "id": "msg_stream_mcp_tool_use",
                        "model": "stream-mcp-error-model",
                        "role": "assistant",
                        "usage": {"input_tokens": 1, "output_tokens": 0}
                    }
                }),
                json!({
                    "type": "content_block_start",
                    "index": 0,
                    "content_block": {
                        "type": "tool_use",
                        "id": "toolu_mcp",
                        "name": "MCP",
                        "input": {}
                    }
                }),
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": serde_json::to_string(&json!({
                            "server": "runner-error-fixture",
                            "transport": transport,
                            "url": mcp_url,
                            "tool_name": "fail",
                            "args": {"message": "denied by fake server"}
                        })).unwrap()
                    }
                }),
                json!({"type": "content_block_stop", "index": 0}),
                json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": "tool_use"},
                    "usage": {"input_tokens": 0, "output_tokens": 1}
                }),
                json!({"type": "message_stop"}),
            ]);
        }

        mock_sse_response(vec![
            json!({
                "type": "message_start",
                "message": {
                    "id": "msg_stream_mcp_done",
                    "model": "stream-mcp-error-model",
                    "role": "assistant",
                    "usage": {"input_tokens": 1, "output_tokens": 0}
                }
            }),
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
            json!({
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "mcp handled"}
            }),
            json!({"type": "content_block_stop", "index": 0}),
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"input_tokens": 0, "output_tokens": 1}
            }),
            json!({"type": "message_stop"}),
        ])
    }

    fn mock_sse_response(events: Vec<Value>) -> impl IntoResponse {
        let body = events
            .into_iter()
            .map(|event| format!("data: {}\n\n", serde_json::to_string(&event).unwrap()))
            .collect::<String>();
        (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/event-stream; charset=utf-8",
            )],
            body,
        )
            .into_response()
    }

    async fn start_mock_web_fetch_loop_server(
        fetch_url: String,
    ) -> (String, Arc<Mutex<MockWebFetchLoopState>>, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(MockWebFetchLoopState {
            fetch_url,
            requests: Vec::new(),
        }));
        let app = Router::new()
            .route("/v1/messages", post(handle_mock_web_fetch_loop_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn handle_mock_web_fetch_loop_request(
        State(state): State<Arc<Mutex<MockWebFetchLoopState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let (call_count, fetch_url) = {
            let mut state = state.lock().unwrap();
            state.requests.push(body);
            (state.requests.len(), state.fetch_url.clone())
        };

        if call_count == 1 {
            return (
                StatusCode::OK,
                Json(json!({
                    "id": "msg_web_fetch_use",
                    "model": "web-fetch-loop-model",
                    "role": "assistant",
                    "content": [{
                        "type": "tool_use",
                        "id": "toolu_web_fetch",
                        "name": "WebFetch",
                        "input": {
                            "url": fetch_url,
                            "timeout_seconds": 5,
                            "max_bytes": 4096
                        }
                    }],
                    "stop_reason": "tool_use",
                    "usage": {
                        "input_tokens": 1,
                        "output_tokens": 1
                    }
                })),
            )
                .into_response();
        }

        (
            StatusCode::OK,
            Json(json!({
                "id": "msg_web_fetch_done",
                "model": "web-fetch-loop-model",
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "fetch complete"
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

    async fn start_mock_permission_mcp_server(
        decision: Value,
    ) -> (String, Arc<Mutex<MockPermissionMcpState>>, JoinHandle<()>) {
        let state = Arc::new(Mutex::new(MockPermissionMcpState {
            decision,
            calls: Vec::new(),
        }));
        let app = Router::new()
            .route("/", post(handle_mock_permission_mcp_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn start_mock_error_mcp_server() -> (String, Arc<Mutex<MockErrorMcpState>>, JoinHandle<()>)
    {
        let state = Arc::new(Mutex::new(MockErrorMcpState {
            endpoint: None,
            calls: Vec::new(),
        }));
        let app = Router::new()
            .route("/", post(handle_mock_error_mcp_request))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), state, server)
    }

    async fn start_mock_error_sse_mcp_server(
    ) -> (String, Arc<Mutex<MockErrorMcpState>>, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint = format!("http://{}/message?sessionId=test-session", addr);
        let state = Arc::new(Mutex::new(MockErrorMcpState {
            endpoint: Some(endpoint),
            calls: Vec::new(),
        }));
        let app = Router::new()
            .route("/sse", get(handle_mock_error_sse_request))
            .route("/message", post(handle_mock_error_mcp_request))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}/sse", addr), state, server)
    }

    async fn start_mock_error_ws_mcp_server(
    ) -> (String, Arc<Mutex<MockErrorMcpState>>, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(Mutex::new(MockErrorMcpState {
            endpoint: None,
            calls: Vec::new(),
        }));
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let state = server_state.clone();
                tokio::spawn(async move {
                    let _ = handle_mock_error_ws_mcp_request(stream, state).await;
                });
            }
        });
        (format!("ws://{}", addr), state, server)
    }

    async fn handle_mock_error_ws_mcp_request(
        stream: tokio::net::TcpStream,
        state: Arc<Mutex<MockErrorMcpState>>,
    ) -> std::io::Result<()> {
        let mut websocket = accept_async(stream).await.map_err(std::io::Error::other)?;

        while let Some(message) = websocket.next().await {
            let message = message.map_err(std::io::Error::other)?;
            let value = match message {
                WsMessage::Text(text) => serde_json::from_str::<Value>(&text).ok(),
                WsMessage::Binary(bytes) => serde_json::from_slice::<Value>(&bytes).ok(),
                WsMessage::Ping(payload) => {
                    websocket
                        .send(WsMessage::Pong(payload))
                        .await
                        .map_err(std::io::Error::other)?;
                    None
                }
                WsMessage::Close(_) => break,
                _ => None,
            };

            let Some(value) = value else {
                continue;
            };
            if let Some(response) = mock_error_mcp_response(&state, &value) {
                websocket
                    .send(WsMessage::Text(response.to_string().into()))
                    .await
                    .map_err(std::io::Error::other)?;
            }
        }

        Ok(())
    }

    fn mock_error_mcp_response(
        state: &Arc<Mutex<MockErrorMcpState>>,
        body: &Value,
    ) -> Option<Value> {
        let method = body.get("method").and_then(Value::as_str);
        let id = body.get("id").cloned().unwrap_or(Value::Null);

        match method {
            Some("notifications/initialized") => None,
            Some("initialize") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": "runner-error-mock", "version": "1.0.0"}
                }
            })),
            Some("tools/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "fail",
                        "description": "Return a protocol-level MCP error result",
                        "inputSchema": {"type": "object"}
                    }]
                }
            })),
            Some("resources/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resources": []}
            })),
            Some("resources/templates/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resourceTemplates": []}
            })),
            Some("prompts/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"prompts": []}
            })),
            Some("tools/call") => {
                let tool_name = body
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let args = body
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                {
                    let mut state = state.lock().unwrap();
                    state.calls.push(json!({
                        "tool_name": tool_name,
                        "args": args
                    }));
                }
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": "denied by fake server"
                        }],
                        "isError": true
                    }
                }))
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"}
            })),
        }
    }

    async fn handle_mock_error_sse_request(
        State(state): State<Arc<Mutex<MockErrorMcpState>>>,
    ) -> impl IntoResponse {
        let endpoint = state
            .lock()
            .unwrap()
            .endpoint
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1/message?sessionId=test-session".to_string());
        mock_sse_endpoint_response(&endpoint)
    }

    async fn handle_mock_error_mcp_request(
        State(state): State<Arc<Mutex<MockErrorMcpState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let method = body.get("method").and_then(Value::as_str);
        let id = body.get("id").cloned().unwrap_or(Value::Null);

        match method {
            Some("notifications/initialized") => StatusCode::NO_CONTENT.into_response(),
            Some("initialize") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": "runner-error-mock", "version": "1.0.0"}
                }
            }))
            .into_response(),
            Some("tools/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "fail",
                        "description": "Return a protocol-level MCP error result",
                        "inputSchema": {"type": "object"}
                    }]
                }
            }))
            .into_response(),
            Some("resources/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resources": []}
            }))
            .into_response(),
            Some("resources/templates/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resourceTemplates": []}
            }))
            .into_response(),
            Some("prompts/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"prompts": []}
            }))
            .into_response(),
            Some("tools/call") => {
                let tool_name = body
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let args = body
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                {
                    let mut state = state.lock().unwrap();
                    state.calls.push(json!({
                        "tool_name": tool_name,
                        "args": args
                    }));
                }
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": "denied by fake server"
                        }],
                        "isError": true
                    }
                }))
                .into_response()
            }
            _ => (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "method not found"}
                })),
            )
                .into_response(),
        }
    }

    fn mock_sse_endpoint_response(endpoint: &str) -> impl IntoResponse {
        (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/event-stream; charset=utf-8",
            )],
            format!("event: endpoint\ndata: {endpoint}\n\n"),
        )
            .into_response()
    }

    async fn handle_mock_permission_mcp_request(
        State(state): State<Arc<Mutex<MockPermissionMcpState>>>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let method = body.get("method").and_then(Value::as_str);
        let id = body.get("id").cloned().unwrap_or(Value::Null);

        match method {
            Some("notifications/initialized") => StatusCode::NO_CONTENT.into_response(),
            Some("initialize") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}, "resources": {}, "prompts": {}},
                    "serverInfo": {"name": "permission-mock", "version": "1.0.0"}
                }
            }))
            .into_response(),
            Some("tools/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [{
                        "name": "approve",
                        "description": "Approve a Kiana tool call",
                        "inputSchema": {"type": "object"}
                    }]
                }
            }))
            .into_response(),
            Some("resources/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"resources": []}
            }))
            .into_response(),
            Some("prompts/list") => Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {"prompts": []}
            }))
            .into_response(),
            Some("tools/call") => {
                let args = body
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let decision = {
                    let mut state = state.lock().unwrap();
                    state.calls.push(args);
                    state.decision.clone()
                };
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": decision.to_string()
                        }],
                        "isError": false
                    }
                }))
                .into_response()
            }
            _ => (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {"code": -32601, "message": "method not found"}
                })),
            )
                .into_response(),
        }
    }

    fn isolate_permission_env() {
        std::env::remove_var("KIANA_PERMISSION_MODE");
        std::env::remove_var("KIANA_ALLOWED_TOOLS");
        std::env::remove_var("KIANA_DISALLOWED_TOOLS");
        let path = std::env::temp_dir().join(format!(
            "kiana-runner-permissions-{}.json",
            std::process::id()
        ));
        std::env::set_var("KIANA_PERMISSIONS_FILE", path);
    }
}
