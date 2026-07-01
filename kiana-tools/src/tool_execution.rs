use crate::{
    mcp_tool::MCP_SERVERS_APP_STATE_KEY, permissions::ToolPermissionCheck, Tool, ToolContext,
    ToolRegistry,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const PERMISSION_PROMPT_TOOL_APP_STATE_KEY: &str = "permission_prompt_tool";
pub const PERMISSION_PROMPT_TOOL_ENV: &str = "KIANA_PERMISSION_PROMPT_TOOL";
const MAILBOX_PERMISSION_GRANTS_KEY: &str = "mailbox_permission_grants";
const PENDING_MAILBOX_PERMISSION_REQUESTS_KEY: &str = "pending_mailbox_permission_requests";

#[derive(Debug)]
pub struct ToolExecutionResult {
    pub is_error: bool,
    pub content: Value,
    pub api_result: Value,
    pub structured_output: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallRequest {
    pub name: String,
    pub input: Value,
    pub tool_use_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionPromptDecision {
    Allow,
    Deny(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionPromptRequest {
    pub request_id: String,
    pub tool_name: String,
    pub input: Value,
    pub tool_use_id: String,
    pub permission_suggestions: Value,
    pub blocked_path: Option<String>,
    pub decision_reason: Value,
    pub agent_id: Option<String>,
}

#[async_trait]
pub trait PermissionPromptHandler: Send + Sync {
    async fn prompt(
        &self,
        request: PermissionPromptRequest,
    ) -> Result<PermissionPromptDecision, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionPromptToolTarget {
    server: Option<String>,
    tool_name: String,
}

pub async fn execute_tool_call(
    registry: &ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
    context: &mut ToolContext,
    name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
) -> ToolExecutionResult {
    execute_tool_call_with_permission_handler(
        registry,
        enabled_tools,
        context,
        name,
        input,
        tool_use_id,
        None,
    )
    .await
}

pub async fn execute_tool_calls(
    registry: &ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
    context: &mut ToolContext,
    calls: &[ToolCallRequest],
) -> Vec<ToolExecutionResult> {
    execute_tool_calls_with_permission_handler(registry, enabled_tools, context, calls, None).await
}

pub async fn execute_tool_calls_with_permission_handler(
    registry: &ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
    context: &mut ToolContext,
    calls: &[ToolCallRequest],
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> Vec<ToolExecutionResult> {
    let mut results = Vec::with_capacity(calls.len());
    let mut index = 0;

    while index < calls.len() {
        let mut batch = Vec::new();
        let mut scan_index = index;
        while scan_index < calls.len() {
            let Some(tool) = concurrent_tool_call_candidate(
                registry,
                enabled_tools,
                context,
                &calls[scan_index],
            )
            .await
            else {
                break;
            };
            batch.push(ConcurrentToolCall {
                index: scan_index,
                tool,
                name: calls[scan_index].name.clone(),
                input: calls[scan_index].input.clone(),
                tool_use_id: calls[scan_index].tool_use_id.clone(),
            });
            scan_index += 1;
        }

        if batch.len() > 1 {
            results.extend(execute_concurrent_tool_batch(context, batch).await);
            index = scan_index;
            continue;
        }

        let call = &calls[index];
        results.push(
            execute_tool_call_with_permission_handler(
                registry,
                enabled_tools,
                context,
                &call.name,
                &call.input,
                call.tool_use_id.as_deref(),
                permission_handler,
            )
            .await,
        );
        index += 1;
    }

    results
}

pub async fn execute_tool_call_with_permission_handler(
    registry: &ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
    context: &mut ToolContext,
    name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> ToolExecutionResult {
    if let Some(enabled_tools) = enabled_tools {
        if !enabled_tools.contains(name) {
            return tool_error_result(
                tool_use_id,
                format!("Tool {name} is not enabled for this run"),
            );
        }
    }

    let Some(tool) = registry.get(name).cloned() else {
        return tool_error_result(tool_use_id, format!("Tool {} not found", name));
    };

    let pre_tool_use = match pre_tool_use_hook_input(context, name, input, tool_use_id).await {
        Ok(outcome) => outcome,
        Err(reason) => return tool_error_result(tool_use_id, reason),
    };
    let hook_ask_reason = pre_tool_use.ask_reason().map(str::to_string);
    let input = pre_tool_use.input();

    let validation = tool.validate_input(input, context).await;
    if !validation.result {
        return tool_error_result(
            tool_use_id,
            validation
                .message
                .unwrap_or_else(|| "Invalid tool input".to_string()),
        );
    }

    if let Some(reason) = hook_ask_reason.as_deref() {
        if let Err(reason) = request_hook_permission_approval(
            registry,
            context,
            name,
            input,
            tool_use_id,
            reason,
            permission_handler,
        )
        .await
        {
            return tool_error_result(tool_use_id, reason);
        }
    }

    match crate::permissions::permission_check_for_tool(
        tool.name(),
        tool.is_read_only(),
        input,
        &context.app_state,
    ) {
        ToolPermissionCheck::Allow => {}
        ToolPermissionCheck::Deny(reason) => {
            return tool_error_result(tool_use_id, reason);
        }
        ToolPermissionCheck::Ask(reason) => {
            let mut allowed_by_external_decision = false;
            if consume_mailbox_permission_grant(context, name, input).is_some() {
                // One-time grants come from a leader mailbox response and are
                // consumed before the tool call is allowed to continue.
                allowed_by_external_decision = true;
            } else if let Some(permission_prompt_tool) =
                permission_prompt_tool_from_app_state(&context.app_state)
            {
                match call_permission_prompt_tool(
                    registry,
                    context,
                    &permission_prompt_tool,
                    name,
                    input,
                    tool_use_id,
                    &reason,
                    permission_handler,
                )
                .await
                {
                    Ok(PermissionPromptDecision::Allow) => {
                        allowed_by_external_decision = true;
                    }
                    Ok(PermissionPromptDecision::Deny(reason)) | Err(reason) => {
                        return tool_error_result(tool_use_id, reason);
                    }
                }
            } else {
                match send_mailbox_permission_request(context, name, input, tool_use_id, &reason) {
                    Ok(Some(request_id)) => {
                        return tool_error_result(
                            tool_use_id,
                            format!(
                                "Permission request {request_id} sent to team-lead. Wait for a permission_response, then retry the tool call."
                            ),
                        );
                    }
                    Ok(None) => {}
                    Err(reason) => {
                        return tool_error_result(tool_use_id, reason);
                    }
                }
            }

            if !allowed_by_external_decision {
                let permission = tool.check_permissions(input, context).await;
                if !permission.granted {
                    return tool_error_result(
                        tool_use_id,
                        permission
                            .reason
                            .unwrap_or_else(|| "Permission denied".to_string()),
                    );
                }
            }
        }
    }

    match tool.call(input, context).await {
        Ok(output) => {
            let result = tool_success_result(tool.as_ref(), output, tool_use_id);
            if let Some(reason) =
                post_tool_use_hook_block_reason(context, name, input, tool_use_id, &result).await
            {
                return tool_error_result(tool_use_id, reason);
            }
            result
        }
        Err(error) => {
            let result = tool_error_result(tool_use_id, error.to_string());
            if let Some(reason) =
                post_tool_use_hook_block_reason(context, name, input, tool_use_id, &result).await
            {
                return tool_error_result(tool_use_id, reason);
            }
            result
        }
    }
}

#[derive(Clone)]
struct ConcurrentToolCall {
    index: usize,
    tool: Arc<dyn Tool>,
    name: String,
    input: Value,
    tool_use_id: Option<String>,
}

enum PreToolUseHookOutcome<'a> {
    Allow(Cow<'a, Value>),
    Ask {
        reason: String,
        input: Cow<'a, Value>,
    },
}

impl<'a> PreToolUseHookOutcome<'a> {
    fn input(&self) -> &Value {
        match self {
            PreToolUseHookOutcome::Allow(input) => input.as_ref(),
            PreToolUseHookOutcome::Ask { input, .. } => input.as_ref(),
        }
    }

    fn ask_reason(&self) -> Option<&str> {
        match self {
            PreToolUseHookOutcome::Allow(_) => None,
            PreToolUseHookOutcome::Ask { reason, .. } => Some(reason.as_str()),
        }
    }
}

async fn concurrent_tool_call_candidate(
    registry: &ToolRegistry,
    enabled_tools: Option<&HashSet<String>>,
    context: &ToolContext,
    call: &ToolCallRequest,
) -> Option<Arc<dyn Tool>> {
    if let Some(enabled_tools) = enabled_tools {
        if !enabled_tools.contains(&call.name) {
            return None;
        }
    }

    let tool = registry.get(&call.name).cloned()?;
    if !tool.is_read_only() || !tool.is_concurrency_safe() {
        return None;
    }

    let validation = tool.validate_input(&call.input, context).await;
    if !validation.result {
        return None;
    }

    match crate::permissions::permission_check_for_tool(
        tool.name(),
        tool.is_read_only(),
        &call.input,
        &context.app_state,
    ) {
        ToolPermissionCheck::Allow => Some(tool),
        ToolPermissionCheck::Deny(_) | ToolPermissionCheck::Ask(_) => None,
    }
}

async fn execute_concurrent_tool_batch(
    context: &mut ToolContext,
    batch: Vec<ConcurrentToolCall>,
) -> Vec<ToolExecutionResult> {
    let base_app_state = context.app_state.clone();
    let mut handles = Vec::with_capacity(batch.len());

    for call in batch {
        let mut child_context = clone_tool_context(context);
        handles.push((
            call.index,
            call.tool_use_id.clone(),
            tokio::spawn(async move {
                let result = execute_preapproved_tool_call(
                    call.tool,
                    &mut child_context,
                    &call.name,
                    &call.input,
                    call.tool_use_id.as_deref(),
                )
                .await;
                (call.index, result, child_context)
            }),
        ));
    }

    let mut completed = Vec::with_capacity(handles.len());
    for (index, tool_use_id, handle) in handles {
        match handle.await {
            Ok((index, result, child_context)) => {
                merge_read_only_child_context(context, child_context, &base_app_state);
                completed.push((index, result));
            }
            Err(error) => {
                completed.push((
                    index,
                    tool_error_result(
                        tool_use_id.as_deref(),
                        format!("Tool execution task failed: {error}"),
                    ),
                ));
            }
        }
    }

    completed.sort_by_key(|(index, _)| *index);
    completed.into_iter().map(|(_, result)| result).collect()
}

async fn execute_preapproved_tool_call(
    tool: Arc<dyn Tool>,
    context: &mut ToolContext,
    name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
) -> ToolExecutionResult {
    let pre_tool_use = match pre_tool_use_hook_input(context, name, input, tool_use_id).await {
        Ok(outcome) => outcome,
        Err(reason) => return tool_error_result(tool_use_id, reason),
    };
    let hook_ask_reason = pre_tool_use.ask_reason().map(str::to_string);
    let input = pre_tool_use.input();

    let validation = tool.validate_input(input, context).await;
    if !validation.result {
        return tool_error_result(
            tool_use_id,
            validation
                .message
                .unwrap_or_else(|| "Invalid tool input".to_string()),
        );
    }

    if let Some(reason) = hook_ask_reason.as_deref() {
        if let Err(reason) = request_hook_permission_approval(
            &ToolRegistry::new(),
            context,
            name,
            input,
            tool_use_id,
            reason,
            None,
        )
        .await
        {
            return tool_error_result(tool_use_id, reason);
        }
    }

    match tool.call(input, context).await {
        Ok(output) => {
            let result = tool_success_result(tool.as_ref(), output, tool_use_id);
            if let Some(reason) =
                post_tool_use_hook_block_reason(context, name, input, tool_use_id, &result).await
            {
                return tool_error_result(tool_use_id, reason);
            }
            result
        }
        Err(error) => {
            let result = tool_error_result(tool_use_id, format!("{name}: {error}"));
            if let Some(reason) =
                post_tool_use_hook_block_reason(context, name, input, tool_use_id, &result).await
            {
                return tool_error_result(tool_use_id, reason);
            }
            result
        }
    }
}

fn tool_success_result(
    tool: &dyn Tool,
    output: crate::ToolOutput,
    tool_use_id: Option<&str>,
) -> ToolExecutionResult {
    let structured_output = output
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("structured_output").cloned());
    let api_result =
        normalize_tool_api_result(tool.map_to_api_result(&output, tool_use_id.unwrap_or_default()));
    let is_error = tool_output_marks_error(&output, &api_result);
    ToolExecutionResult {
        is_error,
        content: output.data,
        api_result,
        structured_output,
    }
}

fn clone_tool_context(context: &ToolContext) -> ToolContext {
    ToolContext {
        cwd: context.cwd.clone(),
        read_file_state: context.read_file_state.clone(),
        app_state: context.app_state.clone(),
        abort_signal: context.abort_signal.clone(),
    }
}

async fn post_tool_use_hook_block_reason(
    context: &ToolContext,
    name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
    result: &ToolExecutionResult,
) -> Option<String> {
    let mut app_state = context.app_state.clone();
    app_state
        .entry("cwd".to_string())
        .or_insert_with(|| json!(context.cwd.clone()));
    let project_trust = kiana_types::project_trust_from_app_state(&app_state);
    let permission_mode = app_state_string(&app_state, "permission_mode")
        .or_else(|| app_state_string(&app_state, "permissionMode"))
        .unwrap_or_else(|| "default".to_string());
    let query_source = app_state_string(&app_state, "query_source")
        .or_else(|| app_state_string(&app_state, "querySource"))
        .unwrap_or_else(|| "tool_execution".to_string());

    match kiana_query::run_post_tool_use_hooks(kiana_query::PostToolUseHookContext {
        abort_signal: Arc::new(tokio::sync::Notify::new()),
        cwd: PathBuf::from(&context.cwd),
        project_trust,
        permission_mode,
        query_source,
        tool_name: name.to_string(),
        tool_input: input.clone(),
        tool_use_id: tool_use_id.map(str::to_string),
        tool_result: result.api_result.clone(),
        is_error: result.is_error,
    })
    .await
    {
        kiana_query::ToolHookDecision::Allow => None,
        kiana_query::ToolHookDecision::Ask { reason, .. } => Some(reason),
        kiana_query::ToolHookDecision::Block(reason) => Some(reason),
        kiana_query::ToolHookDecision::UpdateInput(_) => None,
    }
}

async fn pre_tool_use_hook_input<'a>(
    context: &ToolContext,
    name: &str,
    input: &'a Value,
    tool_use_id: Option<&str>,
) -> Result<PreToolUseHookOutcome<'a>, String> {
    let mut app_state = context.app_state.clone();
    app_state
        .entry("cwd".to_string())
        .or_insert_with(|| json!(context.cwd.clone()));
    let project_trust = kiana_types::project_trust_from_app_state(&app_state);
    let permission_mode = app_state_string(&app_state, "permission_mode")
        .or_else(|| app_state_string(&app_state, "permissionMode"))
        .unwrap_or_else(|| "default".to_string());
    let query_source = app_state_string(&app_state, "query_source")
        .or_else(|| app_state_string(&app_state, "querySource"))
        .unwrap_or_else(|| "tool_execution".to_string());

    match kiana_query::run_pre_tool_use_hooks(kiana_query::PreToolUseHookContext {
        abort_signal: Arc::new(tokio::sync::Notify::new()),
        cwd: PathBuf::from(&context.cwd),
        project_trust,
        permission_mode,
        query_source,
        tool_name: name.to_string(),
        tool_input: input.clone(),
        tool_use_id: tool_use_id.map(str::to_string),
    })
    .await
    {
        kiana_query::ToolHookDecision::Allow => {
            Ok(PreToolUseHookOutcome::Allow(Cow::Borrowed(input)))
        }
        kiana_query::ToolHookDecision::Ask {
            reason,
            updated_input,
        } => Ok(PreToolUseHookOutcome::Ask {
            reason,
            input: updated_input
                .map(Cow::Owned)
                .unwrap_or_else(|| Cow::Borrowed(input)),
        }),
        kiana_query::ToolHookDecision::Block(reason) => Err(reason),
        kiana_query::ToolHookDecision::UpdateInput(input) => {
            Ok(PreToolUseHookOutcome::Allow(Cow::Owned(input)))
        }
    }
}

async fn request_hook_permission_approval(
    registry: &ToolRegistry,
    context: &mut ToolContext,
    name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
    reason: &str,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> Result<(), String> {
    if consume_mailbox_permission_grant(context, name, input).is_some() {
        return Ok(());
    }

    if let Some(permission_prompt_tool) = permission_prompt_tool_from_app_state(&context.app_state)
    {
        return match call_permission_prompt_tool(
            registry,
            context,
            &permission_prompt_tool,
            name,
            input,
            tool_use_id,
            reason,
            permission_handler,
        )
        .await
        {
            Ok(PermissionPromptDecision::Allow) => Ok(()),
            Ok(PermissionPromptDecision::Deny(reason)) | Err(reason) => Err(reason),
        };
    }

    if let Some(request_id) =
        send_mailbox_permission_request(context, name, input, tool_use_id, reason)?
    {
        return Err(format!(
            "Permission request {request_id} sent to team-lead. Wait for a permission_response, then retry the tool call."
        ));
    }

    Err(format!(
        "Hook requested approval for {name}: {reason}. Configure --permission-prompt-tool or run with a team permission workflow."
    ))
}

fn app_state_string(app_state: &HashMap<String, Value>, key: &str) -> Option<String> {
    app_state
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn merge_read_only_child_context(
    context: &mut ToolContext,
    child_context: ToolContext,
    base_app_state: &HashMap<String, Value>,
) {
    context
        .read_file_state
        .extend(child_context.read_file_state);

    for (key, child_value) in child_context.app_state {
        if base_app_state.get(&key) == Some(&child_value) {
            continue;
        }
        merge_app_state_value(context, &key, child_value, base_app_state.get(&key));
    }
}

fn merge_app_state_value(
    context: &mut ToolContext,
    key: &str,
    child_value: Value,
    base_value: Option<&Value>,
) {
    let child_array = child_value.as_array();
    let base_array = base_value.and_then(Value::as_array);
    let parent_array = context.app_state.get_mut(key).and_then(Value::as_array_mut);

    if let (Some(child_array), parent_array) = (child_array, parent_array) {
        let appended = match base_array {
            Some(base_array) if child_array.starts_with(base_array) => {
                &child_array[base_array.len()..]
            }
            None => child_array.as_slice(),
            _ => {
                context.app_state.insert(key.to_string(), child_value);
                return;
            }
        };
        if let Some(parent_array) = parent_array {
            parent_array.extend(appended.iter().cloned());
        } else {
            context.app_state.insert(key.to_string(), child_value);
        }
        return;
    }

    context.app_state.insert(key.to_string(), child_value);
}

fn tool_error_result(tool_use_id: Option<&str>, message: String) -> ToolExecutionResult {
    let content = json!(message);
    ToolExecutionResult {
        is_error: true,
        api_result: json!({
            "type": "tool_result",
            "tool_use_id": tool_use_id.unwrap_or_default(),
            "is_error": true,
            "content": content,
        }),
        content,
        structured_output: None,
    }
}

fn tool_output_marks_error(output: &crate::ToolOutput, api_result: &Value) -> bool {
    output
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("is_error"))
        .and_then(Value::as_bool)
        == Some(true)
        || api_result
            .get("is_error")
            .or_else(|| api_result.get("isError"))
            .and_then(Value::as_bool)
            == Some(true)
}

fn normalize_tool_api_result(mut api_result: Value) -> Value {
    if api_result.get("type").and_then(Value::as_str) != Some("tool_result") {
        api_result["type"] = json!("tool_result");
    }
    api_result
}

fn consume_mailbox_permission_grant(
    context: &mut ToolContext,
    tool_name: &str,
    input: &Value,
) -> Option<String> {
    let grants = context
        .app_state
        .get(MAILBOX_PERMISSION_GRANTS_KEY)
        .and_then(Value::as_array)?
        .clone();
    let mut remaining = Vec::new();
    let mut consumed = None;

    for grant in grants {
        if consumed.is_none() && mailbox_permission_grant_matches(&grant, tool_name, input) {
            consumed = grant
                .get("request_id")
                .or_else(|| grant.get("requestId"))
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| Some("mailbox-permission".to_string()));
            continue;
        }
        remaining.push(grant);
    }

    if consumed.is_some() {
        context.app_state.insert(
            MAILBOX_PERMISSION_GRANTS_KEY.to_string(),
            Value::Array(remaining),
        );
    }
    consumed
}

fn mailbox_permission_grant_matches(grant: &Value, tool_name: &str, input: &Value) -> bool {
    let Some(grant_tool) = grant
        .get("tool_name")
        .or_else(|| grant.get("toolName"))
        .and_then(Value::as_str)
    else {
        return false;
    };
    if !grant_tool.eq_ignore_ascii_case(tool_name) {
        return false;
    }
    grant
        .get("input")
        .is_none_or(|granted_input| granted_input == input)
}

fn send_mailbox_permission_request(
    context: &mut ToolContext,
    tool_name: &str,
    input: &Value,
    tool_use_id: Option<&str>,
    description: &str,
) -> Result<Option<String>, String> {
    let Some(team_name) = active_team_name(&context.app_state) else {
        return Ok(None);
    };
    let sender = agent_name(&context.app_state);
    if sender.eq_ignore_ascii_case("team-lead") {
        return Ok(None);
    }

    if let Some(existing) = pending_permission_request_id(&context.app_state, tool_name, input) {
        return Ok(Some(existing));
    }

    let request_id = format!(
        "permission-{}-{}",
        sanitize_path_component(&sender),
        Uuid::new_v4().simple()
    );
    let tool_use_id = tool_use_id.unwrap_or_default().to_string();
    let timestamp = now_unix_seconds().to_string();
    let body = json!({
        "type": "permission_request",
        "request_id": request_id,
        "agent_id": agent_id(&context.app_state).unwrap_or_else(|| sender.clone()),
        "tool_name": tool_name,
        "tool_use_id": tool_use_id,
        "description": description,
        "input": input,
        "permission_suggestions": []
    });
    let content = serde_json::to_string(&body)
        .map_err(|error| format!("failed to serialize permission request: {error}"))?;
    let message = json!({
        "id": Uuid::new_v4().to_string(),
        "role": "assistant",
        "from": sender,
        "to": "team-lead",
        "recipients": ["team-lead"],
        "summary": "permission_request",
        "content": content,
        "structured": body,
        "timestamp": timestamp,
        "created_at": now_unix_seconds()
    });
    append_mailbox_message(context, &team_name, "team-lead", &message)?;

    let mut pending = context
        .app_state
        .get(PENDING_MAILBOX_PERMISSION_REQUESTS_KEY)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    pending.insert(
        request_id.clone(),
        json!({
            "request_id": request_id,
            "tool_name": tool_name,
            "tool_use_id": tool_use_id,
            "input": input,
            "description": description,
            "team_name": team_name
        }),
    );
    context.app_state.insert(
        PENDING_MAILBOX_PERMISSION_REQUESTS_KEY.to_string(),
        Value::Object(pending),
    );

    Ok(Some(request_id))
}

fn pending_permission_request_id(
    app_state: &HashMap<String, Value>,
    tool_name: &str,
    input: &Value,
) -> Option<String> {
    let pending = app_state
        .get(PENDING_MAILBOX_PERMISSION_REQUESTS_KEY)
        .and_then(Value::as_object)?;
    pending.iter().find_map(|(request_id, request)| {
        let request_tool = request
            .get("tool_name")
            .or_else(|| request.get("toolName"))
            .and_then(Value::as_str)?;
        if request_tool.eq_ignore_ascii_case(tool_name)
            && request
                .get("input")
                .is_some_and(|request_input| request_input == input)
        {
            Some(request_id.clone())
        } else {
            None
        }
    })
}

fn append_mailbox_message(
    context: &mut ToolContext,
    team_name: &str,
    recipient: &str,
    message: &Value,
) -> Result<(), String> {
    let mut mailboxes = context
        .app_state
        .get("team_mailboxes")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut inbox = mailboxes
        .remove(recipient)
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    inbox.push(message.clone());
    mailboxes.insert(recipient.to_string(), Value::Array(inbox));
    context
        .app_state
        .insert("team_mailboxes".to_string(), Value::Object(mailboxes));

    let Some(teams_root) = teams_root(&context.app_state) else {
        return Ok(());
    };
    let path = teams_root
        .join(sanitize_path_component(team_name))
        .join("inboxes")
        .join(format!("{}.json", sanitize_path_component(recipient)));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {}", parent.display(), error))?;
    }
    let mut inbox = if path.is_file() {
        serde_json::from_str::<Value>(
            &fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {}", path.display(), error))?,
        )
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))?
        .as_array()
        .cloned()
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    inbox.push(json!({
        "from": message.get("from").and_then(Value::as_str).unwrap_or("agent"),
        "text": message
            .get("content")
            .or_else(|| message.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "summary": message.get("summary").cloned().unwrap_or(Value::Null),
        "timestamp": message
            .get("timestamp")
            .cloned()
            .unwrap_or_else(|| json!(now_unix_seconds().to_string())),
        "read": false
    }));
    fs::write(
        &path,
        serde_json::to_string_pretty(&Value::Array(inbox))
            .map_err(|error| format!("failed to serialize {}: {}", path.display(), error))?,
    )
    .map_err(|error| format!("failed to write {}: {}", path.display(), error))?;
    Ok(())
}

fn active_team_name(app_state: &HashMap<String, Value>) -> Option<String> {
    app_state
        .get("team_context")
        .or_else(|| app_state.get("teamContext"))
        .and_then(|team| team.get("team_name").or_else(|| team.get("teamName")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_TEAM_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_TEAM_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
}

fn agent_name(app_state: &HashMap<String, Value>) -> String {
    app_state
        .get("agent_name")
        .or_else(|| app_state.get("agentName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_AGENT_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "team-lead".to_string())
}

fn agent_id(app_state: &HashMap<String, Value>) -> Option<String> {
    app_state
        .get("agent_id")
        .or_else(|| app_state.get("agentId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_AGENT_ID")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_AGENT_ID").ok())
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
        })
}

fn teams_root(app_state: &HashMap<String, Value>) -> Option<PathBuf> {
    app_state
        .get("teams_root")
        .or_else(|| app_state.get("teamsRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var("KIANA_TEAMS_ROOT").ok().map(PathBuf::from))
}

fn permission_suggestions(app_state: &HashMap<String, Value>) -> Value {
    app_state
        .get("permission_suggestions")
        .or_else(|| app_state.get("permissionSuggestions"))
        .cloned()
        .unwrap_or_else(|| json!([]))
}

fn permission_blocked_path(input: &Value) -> Option<String> {
    ["file_path", "filePath", "path"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_string)
}

fn sanitize_path_component(value: &str) -> String {
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
        "default".to_string()
    } else {
        sanitized
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn permission_prompt_tool_from_app_state(app_state: &HashMap<String, Value>) -> Option<String> {
    app_state
        .get(PERMISSION_PROMPT_TOOL_APP_STATE_KEY)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var(PERMISSION_PROMPT_TOOL_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

async fn call_permission_prompt_tool(
    registry: &ToolRegistry,
    context: &mut ToolContext,
    selector: &str,
    requested_tool_name: &str,
    requested_input: &Value,
    tool_use_id: Option<&str>,
    permission_reason: &str,
    permission_handler: Option<&dyn PermissionPromptHandler>,
) -> Result<PermissionPromptDecision, String> {
    if selector.trim() == "stdio" {
        let Some(permission_handler) = permission_handler else {
            return Err(
                "--permission-prompt-tool stdio requires an SDK permission prompt handler"
                    .to_string(),
            );
        };
        return permission_handler
            .prompt(PermissionPromptRequest {
                request_id: Uuid::new_v4().to_string(),
                tool_name: requested_tool_name.to_string(),
                input: requested_input.clone(),
                tool_use_id: tool_use_id.unwrap_or_default().to_string(),
                permission_suggestions: permission_suggestions(&context.app_state),
                blocked_path: permission_blocked_path(requested_input),
                decision_reason: json!({
                    "type": "other",
                    "reason": permission_reason,
                }),
                agent_id: agent_id(&context.app_state),
            })
            .await;
    }

    let target = parse_permission_prompt_tool_target(selector, &context.app_state)?;
    if requested_tool_name == "MCP"
        && requested_input
            .get("tool_name")
            .and_then(Value::as_str)
            .is_some_and(|tool_name| tool_name == target.tool_name)
        && requested_input
            .get("server")
            .and_then(Value::as_str)
            .map(|server| target.server.as_deref() == Some(server))
            .unwrap_or(true)
    {
        return Err(format!(
            "--permission-prompt-tool {selector} cannot approve its own MCP invocation"
        ));
    }

    let Some(mcp_tool) = registry.get("MCP").cloned() else {
        return Err("MCP tool is not available for permission prompting".to_string());
    };

    let mut prompt_input = json!({
        "tool_name": target.tool_name,
        "args": {
            "tool_name": requested_tool_name,
            "input": requested_input,
            "tool_use_id": tool_use_id.unwrap_or_default(),
        }
    });
    if let Some(server) = target.server {
        prompt_input["server"] = json!(server);
    }

    let validation = mcp_tool.validate_input(&prompt_input, context).await;
    if !validation.result {
        return Err(validation
            .message
            .unwrap_or_else(|| "Invalid permission prompt tool input".to_string()));
    }

    let output = mcp_tool
        .call(&prompt_input, context)
        .await
        .map_err(|error| format!("permission prompt tool failed: {error}"))?;
    parse_permission_prompt_output(selector, &output.data)
}

fn parse_permission_prompt_tool_target(
    selector: &str,
    app_state: &HashMap<String, Value>,
) -> Result<PermissionPromptToolTarget, String> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err("--permission-prompt-tool requires a non-empty tool name".to_string());
    }

    if let Some(rest) = selector.strip_prefix("mcp__") {
        if let Some((server, tool_name)) = rest.split_once("__") {
            return permission_prompt_target(Some(server), tool_name, selector);
        }
    }

    let server_names = configured_mcp_server_names(app_state);
    if let Some((server, tool_name)) = selector.split_once('.') {
        if server_names.iter().any(|name| name == server) {
            return permission_prompt_target(Some(server), tool_name, selector);
        }
        if server_names.is_empty() {
            return permission_prompt_target(Some(server), tool_name, selector);
        }
    }

    match server_names.as_slice() {
        [server] => permission_prompt_target(Some(server), selector, selector),
        [] => Err(format!(
            "--permission-prompt-tool {selector} requires an MCP server config"
        )),
        _ => Err(format!(
            "--permission-prompt-tool {selector} must include a server prefix because multiple MCP servers are configured"
        )),
    }
}

fn permission_prompt_target(
    server: Option<&str>,
    tool_name: &str,
    selector: &str,
) -> Result<PermissionPromptToolTarget, String> {
    let tool_name = tool_name.trim();
    if tool_name.is_empty() {
        return Err(format!(
            "--permission-prompt-tool {selector} has an empty MCP tool name"
        ));
    }
    let server = server
        .map(str::trim)
        .filter(|server| !server.is_empty())
        .map(str::to_string);
    Ok(PermissionPromptToolTarget {
        server,
        tool_name: tool_name.to_string(),
    })
}

fn configured_mcp_server_names(app_state: &HashMap<String, Value>) -> Vec<String> {
    let Some(servers) = app_state.get(MCP_SERVERS_APP_STATE_KEY) else {
        return Vec::new();
    };
    if let Some(object) = servers.as_object() {
        return object
            .keys()
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .collect();
    }
    servers
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_permission_prompt_output(
    selector: &str,
    output: &Value,
) -> Result<PermissionPromptDecision, String> {
    if output.get("status").and_then(Value::as_str) != Some("executed") {
        return Err(format!(
            "--permission-prompt-tool {selector} did not execute against an MCP server"
        ));
    }
    let result = output.get("result").unwrap_or(&Value::Null);
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Ok(PermissionPromptDecision::Deny(
            mcp_result_text(result)
                .unwrap_or_else(|| "Permission prompt tool returned an error".to_string()),
        ));
    }
    let decision = permission_prompt_decision_value(result).map_err(|error| {
        format!("--permission-prompt-tool {selector} returned an invalid decision: {error}")
    })?;
    parse_permission_prompt_decision(&decision)
}

fn permission_prompt_decision_value(result: &Value) -> Result<Value, String> {
    if result.get("decision").is_some()
        || result.get("behavior").is_some()
        || result.get("allowed").is_some()
    {
        return Ok(result.clone());
    }
    if let Some(text) = mcp_result_text(result) {
        return parse_permission_prompt_decision_text(&text);
    }
    Err("expected JSON with decision, behavior, or allowed".to_string())
}

fn mcp_result_text(result: &Value) -> Option<String> {
    result
        .get("content")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .find(|text| !text.trim().is_empty())
        })
        .or_else(|| result.as_str())
        .map(str::to_string)
}

fn parse_permission_prompt_decision_text(text: &str) -> Result<Value, String> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("allow") || text.eq_ignore_ascii_case("approved") {
        return Ok(json!({ "decision": "allow" }));
    }
    if text.eq_ignore_ascii_case("deny") || text.eq_ignore_ascii_case("denied") {
        return Ok(json!({ "decision": "deny" }));
    }
    serde_json::from_str(text).map_err(|error| format!("decision text is not JSON: {error}"))
}

fn parse_permission_prompt_decision(value: &Value) -> Result<PermissionPromptDecision, String> {
    if let Some(allowed) = value.get("allowed").and_then(Value::as_bool) {
        return if allowed {
            Ok(PermissionPromptDecision::Allow)
        } else {
            Ok(PermissionPromptDecision::Deny(
                permission_prompt_denial_reason(value),
            ))
        };
    }
    let decision = value
        .get("decision")
        .or_else(|| value.get("behavior"))
        .and_then(Value::as_str)
        .map(|decision| decision.trim().to_ascii_lowercase())
        .ok_or_else(|| "missing decision or behavior".to_string())?;

    match decision.as_str() {
        "allow" | "allowed" | "approve" | "approved" => Ok(PermissionPromptDecision::Allow),
        "deny" | "denied" | "reject" | "rejected" => Ok(PermissionPromptDecision::Deny(
            permission_prompt_denial_reason(value),
        )),
        _ => Err(format!("unsupported decision '{decision}'")),
    }
}

fn permission_prompt_denial_reason(value: &Value) -> String {
    value
        .get("reason")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or("Permission denied by --permission-prompt-tool")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolResult;
    use crate::{PermissionDecision, ToolError, ToolOutput, ValidationResult};
    use std::sync::{Arc, Mutex};

    struct TestWriteTool;

    struct RecordingPermissionPromptHandler {
        decision: PermissionPromptDecision,
        requests: Mutex<Vec<PermissionPromptRequest>>,
    }

    impl RecordingPermissionPromptHandler {
        fn allowing() -> Self {
            Self {
                decision: PermissionPromptDecision::Allow,
                requests: Mutex::new(Vec::new()),
            }
        }

        fn denying(reason: &str) -> Self {
            Self {
                decision: PermissionPromptDecision::Deny(reason.to_string()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<PermissionPromptRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PermissionPromptHandler for RecordingPermissionPromptHandler {
        async fn prompt(
            &self,
            request: PermissionPromptRequest,
        ) -> Result<PermissionPromptDecision, String> {
            self.requests.lock().unwrap().push(request);
            Ok(self.decision.clone())
        }
    }

    #[async_trait]
    impl Tool for TestWriteTool {
        fn name(&self) -> &str {
            "TestWrite"
        }

        fn description(&self) -> &str {
            "Writes a file for tool execution tests"
        }

        fn input_schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn output_schema(&self) -> Value {
            json!({ "type": "object" })
        }

        async fn validate_input(&self, _input: &Value, _context: &ToolContext) -> ValidationResult {
            ValidationResult::ok()
        }

        async fn check_permissions(
            &self,
            _input: &Value,
            _context: &ToolContext,
        ) -> PermissionDecision {
            PermissionDecision::allow()
        }

        async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
            let relative_path = input
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("output.txt");
            let content = input
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("called");
            let target = PathBuf::from(&context.cwd).join(relative_path);
            std::fs::write(&target, content).map_err(ToolError::IoError)?;
            Ok(ToolOutput {
                data: json!({ "path": target }),
                metadata: None,
            })
        }
    }

    fn test_context(cwd: &std::path::Path) -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: cwd.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: rx,
        }
    }

    fn clear_hook_env() {
        for key in [
            "KIANA_HOOKS",
            "KIANA_PRE_TOOL_USE_HOOKS",
            "KIANA_POST_TOOL_USE_HOOKS",
            "KIANA_HOOKS_FILE",
            "KIANA_HOME",
            "KIANA_PLUGINS_DIR",
        ] {
            std::env::remove_var(key);
        }
    }

    #[tokio::test]
    async fn pre_tool_use_hook_blocks_mutating_tool_before_call() {
        let _guard = crate::test_support::lock_env();
        clear_hook_env();
        std::env::set_var(
            "KIANA_PRE_TOOL_USE_HOOKS",
            serde_json::to_string(&vec![
                "printf '%s' '{\"decision\":\"block\",\"reason\":\"dangerous tool blocked\"}'",
            ])
            .unwrap(),
        );
        let root =
            std::env::temp_dir().join(format!("kiana-pre-tool-use-hook-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("danger.txt");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestWriteTool));
        let mut context = test_context(&root);

        let result = execute_tool_call_with_permission_handler(
            &registry,
            None,
            &mut context,
            "TestWrite",
            &json!({ "path": "danger.txt", "content": "mutated" }),
            Some("toolu_1"),
            None,
        )
        .await;

        assert!(result.is_error);
        assert_eq!(result.content, json!("dangerous tool blocked"));
        assert!(
            !target.exists(),
            "blocked PreToolUse hook should prevent the tool call from mutating the worktree"
        );
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn pre_tool_use_hook_denies_mutating_tool_before_call() {
        let _guard = crate::test_support::lock_env();
        clear_hook_env();
        std::env::set_var(
            "KIANA_PRE_TOOL_USE_HOOKS",
            serde_json::to_string(&vec![
                "printf '%s' '{\"decision\":\"deny\",\"reason\":\"writes disabled by policy\"}'",
            ])
            .unwrap(),
        );
        let root =
            std::env::temp_dir().join(format!("kiana-pre-tool-deny-hook-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("denied.txt");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestWriteTool));
        let mut context = test_context(&root);

        let result = execute_tool_call_with_permission_handler(
            &registry,
            None,
            &mut context,
            "TestWrite",
            &json!({ "path": "denied.txt", "content": "mutated" }),
            Some("toolu_deny"),
            None,
        )
        .await;

        assert!(result.is_error);
        assert_eq!(result.content, json!("writes disabled by policy"));
        assert!(
            !target.exists(),
            "denied PreToolUse hook should prevent the tool call from mutating the worktree"
        );
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn pre_tool_use_hook_updates_tool_input_before_call() {
        let _guard = crate::test_support::lock_env();
        clear_hook_env();
        std::env::set_var(
            "KIANA_PRE_TOOL_USE_HOOKS",
            serde_json::to_string(&vec![
                "printf '%s' '{\"decision\":\"update_input\",\"update_input\":{\"path\":\"sanitized.txt\",\"content\":\"rewritten by hook\"}}'",
            ])
            .unwrap(),
        );
        let root = std::env::temp_dir().join(format!(
            "kiana-pre-tool-update-hook-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let original = root.join("original.txt");
        let sanitized = root.join("sanitized.txt");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestWriteTool));
        let mut context = test_context(&root);

        let result = execute_tool_call_with_permission_handler(
            &registry,
            None,
            &mut context,
            "TestWrite",
            &json!({ "path": "original.txt", "content": "raw input" }),
            Some("toolu_update"),
            None,
        )
        .await;

        assert!(!result.is_error);
        assert!(
            !original.exists(),
            "updated PreToolUse input should prevent the original path from being used"
        );
        assert_eq!(
            std::fs::read_to_string(&sanitized).unwrap(),
            "rewritten by hook"
        );
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn pre_tool_use_hook_ask_requests_permission_before_call() {
        let _guard = crate::test_support::lock_env();
        clear_hook_env();
        std::env::set_var(
            "KIANA_PRE_TOOL_USE_HOOKS",
            serde_json::to_string(&vec![
                "printf '%s' '{\"decision\":\"ask\",\"reason\":\"review hook requested approval\"}'",
            ])
            .unwrap(),
        );
        let root =
            std::env::temp_dir().join(format!("kiana-pre-tool-ask-hook-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("approved.txt");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestWriteTool));
        let mut context = test_context(&root);
        context.app_state.insert(
            PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
            json!("stdio"),
        );
        let handler = RecordingPermissionPromptHandler::allowing();

        let result = execute_tool_call_with_permission_handler(
            &registry,
            None,
            &mut context,
            "TestWrite",
            &json!({ "path": "approved.txt", "content": "approved by prompt" }),
            Some("toolu_ask_allow"),
            Some(&handler),
        )
        .await;

        assert!(!result.is_error);
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "approved by prompt"
        );
        let requests = handler.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].tool_name, "TestWrite");
        assert_eq!(
            requests[0].decision_reason["reason"],
            json!("review hook requested approval")
        );
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn pre_tool_use_hook_ask_honors_permission_denial() {
        let _guard = crate::test_support::lock_env();
        clear_hook_env();
        std::env::set_var(
            "KIANA_PRE_TOOL_USE_HOOKS",
            serde_json::to_string(&vec![
                "printf '%s' '{\"decision\":\"ask\",\"reason\":\"manual review required\"}'",
            ])
            .unwrap(),
        );
        let root = std::env::temp_dir().join(format!(
            "kiana-pre-tool-ask-deny-hook-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("denied.txt");
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestWriteTool));
        let mut context = test_context(&root);
        context.app_state.insert(
            PERMISSION_PROMPT_TOOL_APP_STATE_KEY.to_string(),
            json!("stdio"),
        );
        let handler = RecordingPermissionPromptHandler::denying("operator denied hook ask");

        let result = execute_tool_call_with_permission_handler(
            &registry,
            None,
            &mut context,
            "TestWrite",
            &json!({ "path": "denied.txt", "content": "should not write" }),
            Some("toolu_ask_deny"),
            Some(&handler),
        )
        .await;

        assert!(result.is_error);
        assert_eq!(result.content, json!("operator denied hook ask"));
        assert!(!target.exists());
        assert_eq!(handler.requests().len(), 1);
        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    #[tokio::test]
    async fn post_tool_use_hook_runs_after_successful_tool_call() {
        let _guard = crate::test_support::lock_env();
        clear_hook_env();
        let root =
            std::env::temp_dir().join(format!("kiana-post-tool-use-hook-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let hook_payload = root.join("post-hook.json");
        std::env::set_var(
            "KIANA_POST_TOOL_USE_HOOKS",
            serde_json::to_string(&vec![format!("cat > {}", shell_quote_path(&hook_payload))])
                .unwrap(),
        );
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(TestWriteTool));
        let mut context = test_context(&root);

        let result = execute_tool_call_with_permission_handler(
            &registry,
            None,
            &mut context,
            "TestWrite",
            &json!({ "path": "post.txt", "content": "mutated" }),
            Some("toolu_post"),
            None,
        )
        .await;

        assert!(!result.is_error);
        assert_eq!(
            std::fs::read_to_string(root.join("post.txt")).unwrap(),
            "mutated"
        );
        let payload: Value = serde_json::from_str(&std::fs::read_to_string(&hook_payload).unwrap())
            .expect("PostToolUse hook should receive JSON stdin");
        assert_eq!(payload["hook_event_name"], "PostToolUse");
        assert_eq!(payload["tool_name"], "TestWrite");
        assert_eq!(payload["toolName"], "TestWrite");
        assert_eq!(payload["tool_input"]["path"], "post.txt");
        assert_eq!(payload["toolInput"]["content"], "mutated");
        assert_eq!(payload["tool_use_id"], "toolu_post");
        assert_eq!(payload["toolUseID"], "toolu_post");
        assert_eq!(payload["tool_result"]["type"], "tool_result");
        assert_eq!(payload["toolResult"]["tool_use_id"], "toolu_post");
        assert_eq!(payload["is_error"], false);

        let _ = std::fs::remove_dir_all(root);
        clear_hook_env();
    }

    fn shell_quote_path(path: &std::path::Path) -> String {
        let value = shell_path(path);
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    fn shell_path(path: &std::path::Path) -> String {
        let value = path.to_string_lossy().to_string();
        #[cfg(windows)]
        {
            value.replace('\\', "/")
        }
        #[cfg(not(windows))]
        {
            value
        }
    }
}
