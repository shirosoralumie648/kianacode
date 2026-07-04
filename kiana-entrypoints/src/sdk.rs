use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub type SdkMessage = Value;
pub type SdkResultMessage = Value;
pub type SdkUserMessage = Value;
pub(crate) const STREAM_JSON_HISTORY_MESSAGES_OPTION: &str = "stream_json_history_messages";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkSessionInfo {
    pub session_id: String,
    pub title: Option<String>,
    pub tag: Option<String>,
    pub parent_session_id: Option<String>,
    pub cwd: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub message_count: usize,
    pub assistant_message_count: usize,
    pub last_role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkSession {
    pub session_id: String,
    pub title: Option<String>,
    pub tag: Option<String>,
    pub parent_session_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CcrV2HydrateReport {
    pub session_id: String,
    pub internal_event_count: usize,
    pub subagent_event_count: usize,
    pub message_count: usize,
    pub skipped_event_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Query {
    pub session_id: String,
    pub messages: Vec<SdkMessage>,
}

#[derive(Debug)]
pub struct AbortError;

impl std::fmt::Display for AbortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operation aborted")
    }
}

impl std::error::Error for AbortError {}

pub struct CronTask {
    pub id: String,
    pub cron: String,
    pub prompt: String,
    pub created_at: u64,
    pub recurring: Option<bool>,
}

pub enum ScheduledTaskEvent {
    Fire { task: CronTask },
    Missed { tasks: Vec<CronTask> },
}

pub struct ScheduledTasksHandle {
    pub dir: String,
}

pub struct InboundPrompt {
    pub content: Value,
    pub uuid: Option<String>,
}

pub struct RemoteControlHandle {
    pub session_url: String,
    pub environment_id: String,
    pub bridge_session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    session_id: String,
    title: Option<String>,
    tag: Option<String>,
    parent_session_id: Option<String>,
    cwd: Option<String>,
    created_at: u64,
    updated_at: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    editable_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    read_only_files: Vec<String>,
    messages: Vec<SdkMessage>,
}

impl PersistedSession {
    fn info(&self) -> SdkSessionInfo {
        SdkSessionInfo {
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            tag: self.tag.clone(),
            parent_session_id: self.parent_session_id.clone(),
            cwd: self.cwd.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            message_count: self.messages.len(),
            assistant_message_count: self
                .messages
                .iter()
                .filter(|message| message_role(message) == Some("assistant"))
                .count(),
            last_role: self
                .messages
                .last()
                .and_then(message_role)
                .map(str::to_string),
        }
    }

    fn sdk_session(&self) -> SdkSession {
        SdkSession {
            session_id: self.session_id.clone(),
            title: self.title.clone(),
            tag: self.tag.clone(),
            parent_session_id: self.parent_session_id.clone(),
            cwd: self.cwd.clone(),
        }
    }
}

pub async fn query(prompt: String, mut options: HashMap<String, Value>) -> Result<Query> {
    options.insert("execute".to_string(), Value::Bool(true));
    let result = unstable_v2_prompt(prompt, options).await?;
    let session_id = result
        .get("session_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("query did not return a session_id"))?
        .to_string();
    let session = read_session(&default_sessions_dir(), &session_id)?;
    Ok(Query {
        session_id: session.session_id,
        messages: session.messages,
    })
}

pub async fn unstable_v2_create_session(options: HashMap<String, Value>) -> Result<SdkSession> {
    create_session_at(default_sessions_dir(), options).map(|session| session.sdk_session())
}

pub async fn unstable_v2_resume_session(
    session_id: String,
    _options: HashMap<String, Value>,
) -> Result<SdkSession> {
    read_session(&default_sessions_dir(), &session_id).map(|session| session.sdk_session())
}

pub async fn unstable_v2_prompt(
    message: String,
    options: HashMap<String, Value>,
) -> Result<SdkResultMessage> {
    unstable_v2_prompt_with_optional_permission_handler(message, options, None).await
}

pub async fn unstable_v2_prompt_with_permission_handler(
    message: String,
    options: HashMap<String, Value>,
    permission_handler: &dyn kiana_tools::tool_execution::PermissionPromptHandler,
) -> Result<SdkResultMessage> {
    unstable_v2_prompt_with_optional_permission_handler(message, options, Some(permission_handler))
        .await
}

async fn unstable_v2_prompt_with_optional_permission_handler(
    message: String,
    options: HashMap<String, Value>,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
) -> Result<SdkResultMessage> {
    if bool_option(&options, "no_session_persistence").unwrap_or(false) {
        return prompt_without_persistence(message, options, permission_handler).await;
    }

    prompt_with_persistence_at(default_sessions_dir(), message, options, permission_handler).await
}

async fn prompt_with_persistence_at(
    root: PathBuf,
    message: String,
    options: HashMap<String, Value>,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
) -> Result<SdkResultMessage> {
    if should_execute_model(&options) {
        let session = build_user_prompt_session(&root, message, &options)?;
        let options = prompt_options_with_session_file_sets(&session, options);
        let run = crate::runner::run_assistant_turn_with_permission_handler(
            session.messages.clone(),
            &options,
            permission_handler,
        )
        .await?;
        return persist_completed_model_prompt(&root, session, run);
    }

    let session = append_user_prompt(root, message, options)?;
    Ok(serde_json::json!({
        "type": "sdk_prompt_recorded",
        "session_id": session.session_id,
        "message_count": session.messages.len(),
        "status": "recorded",
        "execution": "record_only"
    }))
}

pub async fn unstable_v2_prompt_streaming<F>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
) -> Result<SdkResultMessage>
where
    F: FnMut(kiana_services::api::streaming::StreamEvent) -> Result<()>,
{
    unstable_v2_prompt_streaming_with_optional_permission_handler(
        message,
        options,
        on_stream_event,
        None,
        None,
    )
    .await
}

pub async fn unstable_v2_prompt_streaming_with_local_events<F>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
) -> Result<SdkResultMessage>
where
    F: FnMut(SdkPromptStreamEvent) -> Result<()>,
{
    unstable_v2_prompt_streaming_with_optional_local_events(
        message,
        options,
        on_stream_event,
        None,
        None,
    )
    .await
}

pub async fn unstable_v2_prompt_streaming_with_permission_handler<F>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: &dyn kiana_tools::tool_execution::PermissionPromptHandler,
) -> Result<SdkResultMessage>
where
    F: FnMut(kiana_services::api::streaming::StreamEvent) -> Result<()>,
{
    unstable_v2_prompt_streaming_with_optional_permission_handler(
        message,
        options,
        on_stream_event,
        Some(permission_handler),
        None,
    )
    .await
}

pub async fn unstable_v2_prompt_streaming_with_permission_handler_and_abort_signal<F>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: &dyn kiana_tools::tool_execution::PermissionPromptHandler,
    abort_signal: tokio::sync::watch::Receiver<bool>,
) -> Result<SdkResultMessage>
where
    F: FnMut(kiana_services::api::streaming::StreamEvent) -> Result<()>,
{
    unstable_v2_prompt_streaming_with_optional_permission_handler(
        message,
        options,
        on_stream_event,
        Some(permission_handler),
        Some(abort_signal),
    )
    .await
}

pub type SdkPromptStreamEvent = crate::runner::RunnerStreamEvent;

pub fn runtime_events_from_sdk_prompt_stream_event(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    start_sequence: u64,
    timestamp: &str,
    event: SdkPromptStreamEvent,
) -> Vec<kiana_types::RuntimeEvent> {
    crate::runner::runtime_events_from_runner_stream_event(
        session_id,
        turn_id,
        parent_turn_id,
        start_sequence,
        timestamp,
        event,
    )
}

pub fn runtime_event_from_sdk_permission_prompt_request(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    request: &kiana_tools::tool_execution::PermissionPromptRequest,
) -> kiana_types::RuntimeEvent {
    crate::runner::runtime_event_from_permission_prompt_request(
        session_id,
        turn_id,
        parent_turn_id,
        sequence,
        timestamp,
        request,
    )
}

pub fn runtime_event_from_sdk_result(
    session_id: &str,
    turn_id: &str,
    parent_turn_id: Option<String>,
    sequence: u64,
    timestamp: &str,
    result: &crate::runner::AssistantRunResult,
) -> kiana_types::RuntimeEvent {
    crate::runner::runtime_event_from_assistant_run_result(
        session_id,
        turn_id,
        parent_turn_id,
        sequence,
        timestamp,
        result,
    )
}

pub async fn unstable_v2_prompt_streaming_with_local_events_and_permission_handler_and_abort_signal<
    F,
>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: &dyn kiana_tools::tool_execution::PermissionPromptHandler,
    abort_signal: tokio::sync::watch::Receiver<bool>,
) -> Result<SdkResultMessage>
where
    F: FnMut(SdkPromptStreamEvent) -> Result<()>,
{
    unstable_v2_prompt_streaming_with_optional_local_events(
        message,
        options,
        on_stream_event,
        Some(permission_handler),
        Some(abort_signal),
    )
    .await
}

async fn unstable_v2_prompt_streaming_with_optional_permission_handler<F>(
    message: String,
    options: HashMap<String, Value>,
    mut on_stream_event: F,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
    abort_signal: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<SdkResultMessage>
where
    F: FnMut(kiana_services::api::streaming::StreamEvent) -> Result<()>,
{
    unstable_v2_prompt_streaming_with_optional_local_events(
        message,
        options,
        move |event| {
            if let SdkPromptStreamEvent::Model(event) = event {
                on_stream_event(event)?;
            }
            Ok(())
        },
        permission_handler,
        abort_signal,
    )
    .await
}

async fn unstable_v2_prompt_streaming_with_optional_local_events<F>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
    abort_signal: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<SdkResultMessage>
where
    F: FnMut(SdkPromptStreamEvent) -> Result<()>,
{
    if bool_option(&options, "no_session_persistence").unwrap_or(false) {
        return prompt_without_persistence_streaming_with_local_events(
            message,
            options,
            on_stream_event,
            permission_handler,
            abort_signal,
        )
        .await;
    }

    prompt_streaming_with_persistence_with_local_events_at(
        default_sessions_dir(),
        message,
        options,
        on_stream_event,
        permission_handler,
        abort_signal,
    )
    .await
}

#[cfg(test)]
async fn prompt_streaming_with_persistence_at<F>(
    root: PathBuf,
    message: String,
    options: HashMap<String, Value>,
    mut on_stream_event: F,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
    abort_signal: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<SdkResultMessage>
where
    F: FnMut(kiana_services::api::streaming::StreamEvent) -> Result<()>,
{
    prompt_streaming_with_persistence_with_local_events_at(
        root,
        message,
        options,
        move |event| {
            if let SdkPromptStreamEvent::Model(event) = event {
                on_stream_event(event)?;
            }
            Ok(())
        },
        permission_handler,
        abort_signal,
    )
    .await
}

async fn prompt_streaming_with_persistence_with_local_events_at<F>(
    root: PathBuf,
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
    abort_signal: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<SdkResultMessage>
where
    F: FnMut(SdkPromptStreamEvent) -> Result<()>,
{
    if should_execute_model(&options) {
        let session = build_user_prompt_session(&root, message, &options)?;
        let options = prompt_options_with_session_file_sets(&session, options);
        let run = if let Some(abort_signal) = abort_signal {
            crate::runner::run_assistant_turn_streaming_with_runner_events_and_abort_signal(
                session.messages.clone(),
                &options,
                on_stream_event,
                permission_handler,
                abort_signal,
            )
            .await?
        } else {
            let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
            crate::runner::run_assistant_turn_streaming_with_runner_events_and_abort_signal(
                session.messages.clone(),
                &options,
                on_stream_event,
                permission_handler,
                abort_signal,
            )
            .await?
        };
        return persist_completed_model_prompt(&root, session, run);
    }

    let session = append_user_prompt(root, message, options)?;
    Ok(serde_json::json!({
        "type": "sdk_prompt_recorded",
        "session_id": session.session_id,
        "message_count": session.messages.len(),
        "status": "recorded",
        "execution": "record_only"
    }))
}

async fn prompt_without_persistence(
    message: String,
    options: HashMap<String, Value>,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
) -> Result<SdkResultMessage> {
    let prompt = message.trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!("prompt cannot be empty"));
    }
    let session_id =
        string_option(&options, "session_id").unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_session_id(&session_id)?;
    let mut messages = stream_json_history_messages_option(&options)?;
    messages.push(serde_json::json!({
        "role": "user",
        "content": prompt,
        "created_at": now_unix_seconds()
    }));

    if should_execute_model(&options) {
        let run = crate::runner::run_assistant_turn_with_permission_handler(
            messages,
            &options,
            permission_handler,
        )
        .await?;
        let mut result = serde_json::json!({
            "type": "sdk_prompt_completed",
            "session_id": session_id,
            "message_count": run.messages.len(),
            "status": "completed",
            "execution": "model",
            "persistence": "disabled",
            "assistant_text": run.text,
            "iterations": run.iterations,
        });
        if let Some(structured_output) = run.structured_output {
            result["structured_output"] = structured_output;
        }
        return Ok(result);
    }

    Ok(serde_json::json!({
        "type": "sdk_prompt_recorded",
        "session_id": session_id,
        "message_count": messages.len(),
        "status": "recorded",
        "execution": "record_only",
        "persistence": "disabled"
    }))
}

async fn prompt_without_persistence_streaming_with_local_events<F>(
    message: String,
    options: HashMap<String, Value>,
    on_stream_event: F,
    permission_handler: Option<&dyn kiana_tools::tool_execution::PermissionPromptHandler>,
    abort_signal: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<SdkResultMessage>
where
    F: FnMut(SdkPromptStreamEvent) -> Result<()>,
{
    let prompt = message.trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!("prompt cannot be empty"));
    }
    let session_id =
        string_option(&options, "session_id").unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_session_id(&session_id)?;
    let mut messages = stream_json_history_messages_option(&options)?;
    messages.push(serde_json::json!({
        "role": "user",
        "content": prompt,
        "created_at": now_unix_seconds()
    }));

    if should_execute_model(&options) {
        let run = if let Some(abort_signal) = abort_signal {
            crate::runner::run_assistant_turn_streaming_with_runner_events_and_abort_signal(
                messages,
                &options,
                on_stream_event,
                permission_handler,
                abort_signal,
            )
            .await?
        } else {
            let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
            crate::runner::run_assistant_turn_streaming_with_runner_events_and_abort_signal(
                messages,
                &options,
                on_stream_event,
                permission_handler,
                abort_signal,
            )
            .await?
        };
        let mut result = serde_json::json!({
            "type": "sdk_prompt_completed",
            "session_id": session_id,
            "message_count": run.messages.len(),
            "status": "completed",
            "execution": "model",
            "persistence": "disabled",
            "assistant_text": run.text,
            "iterations": run.iterations,
        });
        if let Some(structured_output) = run.structured_output {
            result["structured_output"] = structured_output;
        }
        return Ok(result);
    }

    Ok(serde_json::json!({
        "type": "sdk_prompt_recorded",
        "session_id": session_id,
        "message_count": messages.len(),
        "status": "recorded",
        "execution": "record_only",
        "persistence": "disabled"
    }))
}

pub async fn get_session_messages(session_id: String) -> Result<Vec<Value>> {
    read_session(&default_sessions_dir(), &session_id).map(|session| session.messages)
}

pub async fn get_session_subagent_transcript(
    session_id: String,
    agent_id: String,
) -> Result<Vec<Value>> {
    read_jsonl_values(&subagent_transcript_file(
        &default_sessions_dir(),
        &session_id,
        &agent_id,
    )?)
}

pub async fn hydrate_ccr_v2_session_from_worker(
    session_id: String,
    client: &kiana_remote::CcrV2WorkerClient,
) -> Result<CcrV2HydrateReport> {
    let events = client.read_internal_events().await?;
    let subagent_events = client.read_subagent_internal_events().await?;
    hydrate_ccr_v2_internal_events_at(
        &default_sessions_dir(),
        &session_id,
        events,
        subagent_events,
    )
}

pub async fn list_sessions() -> Result<Vec<SdkSessionInfo>> {
    list_sessions_at(&default_sessions_dir())
}

pub async fn get_session_info(session_id: String) -> Result<Option<SdkSessionInfo>> {
    let root = default_sessions_dir();
    if !session_file(&root, &session_id)?.exists()
        && !session_events_file(&root, &session_id)?.exists()
    {
        return Ok(None);
    }
    read_session(&root, &session_id).map(|session| Some(session.info()))
}

pub async fn rename_session(session_id: String, title: String) -> Result<()> {
    update_session(default_sessions_dir(), &session_id, |session| {
        session.title = normalize_optional_string(Some(title));
        session.updated_at = now_unix_seconds();
    })
}

pub async fn tag_session(session_id: String, tag: Option<String>) -> Result<()> {
    update_session(default_sessions_dir(), &session_id, |session| {
        session.tag = normalize_optional_string(tag);
        session.updated_at = now_unix_seconds();
    })
}

pub async fn fork_session(session_id: String) -> Result<String> {
    fork_session_with_options(session_id, HashMap::new()).await
}

pub async fn fork_session_with_options(
    session_id: String,
    options: HashMap<String, Value>,
) -> Result<String> {
    let root = default_sessions_dir();
    fork_session_at_with_options(&root, &session_id, options)
}

pub fn watch_scheduled_tasks(dir: String) -> Result<ScheduledTasksHandle> {
    let path = PathBuf::from(&dir);
    fs::create_dir_all(&path)
        .with_context(|| format!("failed to create scheduled task directory {}", dir))?;
    Ok(ScheduledTasksHandle { dir })
}

pub fn build_missed_task_notification(_missed: Vec<CronTask>) -> String {
    if _missed.is_empty() {
        return "No scheduled tasks were missed.".to_string();
    }

    let mut lines = vec![format!("{} scheduled task(s) were missed:", _missed.len())];
    for task in _missed {
        lines.push(format!("- {} ({}) {}", task.id, task.cron, task.prompt));
    }
    lines.join("\n")
}

pub async fn connect_remote_control(
    opts: HashMap<String, Value>,
) -> Result<Option<RemoteControlHandle>> {
    let Some(session_url) = string_option(&opts, "session_url") else {
        return Ok(None);
    };
    let environment_id = string_option(&opts, "environment_id")
        .ok_or_else(|| anyhow!("remote control requires environment_id"))?;
    let bridge_session_id =
        string_option(&opts, "bridge_session_id").unwrap_or_else(|| Uuid::new_v4().to_string());

    Ok(Some(RemoteControlHandle {
        session_url,
        environment_id,
        bridge_session_id,
    }))
}

fn append_user_prompt(
    root: PathBuf,
    prompt: String,
    options: HashMap<String, Value>,
) -> Result<PersistedSession> {
    let session = build_user_prompt_session(&root, prompt, &options)?;
    write_session(&root, &session)?;
    Ok(session)
}

fn build_user_prompt_session(
    root: &Path,
    prompt: String,
    options: &HashMap<String, Value>,
) -> Result<PersistedSession> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err(anyhow!("prompt cannot be empty"));
    }

    let mut session = match string_option(&options, "session_id") {
        Some(session_id) => {
            let path = session_file(root, &session_id)?;
            if path.exists() {
                read_session(root, &session_id)?
            } else if bool_option(&options, "create_session_if_missing").unwrap_or(false) {
                new_session_from_options(options)?
            } else {
                return Err(anyhow!("session '{}' was not found", session_id));
            }
        }
        None => new_session_from_options(options)?,
    };

    if session.messages.is_empty() {
        session
            .messages
            .extend(stream_json_history_messages_option(options)?);
    }

    let message = serde_json::json!({
        "role": "user",
        "content": prompt,
        "created_at": now_unix_seconds()
    });

    if session.title.is_none() {
        session.title = Some(infer_title(&prompt));
    }

    session.messages.push(message);
    session.updated_at = now_unix_seconds();
    Ok(session)
}

fn stream_json_history_messages_option(options: &HashMap<String, Value>) -> Result<Vec<Value>> {
    let Some(value) = options.get(STREAM_JSON_HISTORY_MESSAGES_OPTION) else {
        return Ok(Vec::new());
    };
    let messages = value
        .as_array()
        .ok_or_else(|| anyhow!("{STREAM_JSON_HISTORY_MESSAGES_OPTION} must be an array"))?;
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let role = message.get("role").and_then(Value::as_str).ok_or_else(|| {
                anyhow!("{STREAM_JSON_HISTORY_MESSAGES_OPTION}[{index}] missing role")
            })?;
            if role != "user" && role != "assistant" {
                return Err(anyhow!(
                    "{STREAM_JSON_HISTORY_MESSAGES_OPTION}[{index}] has unsupported role '{role}'"
                ));
            }
            let content = message.get("content").cloned().ok_or_else(|| {
                anyhow!("{STREAM_JSON_HISTORY_MESSAGES_OPTION}[{index}] missing content")
            })?;
            Ok(serde_json::json!({
                "role": role,
                "content": content,
            }))
        })
        .collect()
}

fn create_session_at(root: PathBuf, options: HashMap<String, Value>) -> Result<PersistedSession> {
    let session = new_session_from_options(&options)?;
    let path = session_file(&root, &session.session_id)?;
    if path.exists() {
        return Err(anyhow!("session '{}' already exists", session.session_id));
    }

    write_session(&root, &session)?;
    Ok(session)
}

fn new_session_from_options(options: &HashMap<String, Value>) -> Result<PersistedSession> {
    let session_id =
        string_option(options, "session_id").unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_session_id(&session_id)?;
    let now = now_unix_seconds();
    Ok(PersistedSession {
        session_id,
        title: session_title_option(options),
        tag: normalize_optional_string(string_option(options, "tag")),
        parent_session_id: normalize_optional_string(string_option(options, "parent_session_id")),
        cwd: normalize_optional_string(string_option(options, "cwd")).or_else(current_cwd_option),
        created_at: now,
        updated_at: now,
        editable_files: string_list_option(options, &["editable_files", "editableFiles"])?
            .unwrap_or_default(),
        read_only_files: string_list_option(
            options,
            &[
                "read_only_files",
                "readOnlyFiles",
                "readonly_files",
                "readonlyFiles",
            ],
        )?
        .unwrap_or_default(),
        messages: Vec::new(),
    })
}

fn fork_session_at_with_options(
    root: &Path,
    session_id: &str,
    options: HashMap<String, Value>,
) -> Result<String> {
    let source = read_session(root, session_id)?;
    let now = now_unix_seconds();
    let forked_id =
        string_option(&options, "session_id").unwrap_or_else(|| Uuid::new_v4().to_string());
    validate_session_id(&forked_id)?;
    let path = session_file(root, &forked_id)?;
    if path.exists() {
        return Err(anyhow!("session '{}' already exists", forked_id));
    }
    let forked = PersistedSession {
        session_id: forked_id.clone(),
        title: session_title_option(&options).or(source.title),
        tag: normalize_optional_string(string_option(&options, "tag")).or(source.tag),
        parent_session_id: Some(source.session_id),
        cwd: normalize_optional_string(string_option(&options, "cwd"))
            .or(source.cwd)
            .or_else(current_cwd_option),
        created_at: now,
        updated_at: now,
        editable_files: string_list_option(&options, &["editable_files", "editableFiles"])?
            .unwrap_or(source.editable_files),
        read_only_files: string_list_option(
            &options,
            &[
                "read_only_files",
                "readOnlyFiles",
                "readonly_files",
                "readonlyFiles",
            ],
        )?
        .unwrap_or(source.read_only_files),
        messages: source.messages,
    };
    write_session(root, &forked)?;
    Ok(forked_id)
}

fn prompt_options_with_session_file_sets(
    session: &PersistedSession,
    mut options: HashMap<String, Value>,
) -> HashMap<String, Value> {
    if !session.editable_files.is_empty()
        && !options.contains_key("editable_files")
        && !options.contains_key("editableFiles")
    {
        options.insert(
            "editable_files".to_string(),
            Value::Array(
                session
                    .editable_files
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !session.read_only_files.is_empty()
        && !options.contains_key("read_only_files")
        && !options.contains_key("readOnlyFiles")
        && !options.contains_key("readonly_files")
        && !options.contains_key("readonlyFiles")
    {
        options.insert(
            "read_only_files".to_string(),
            Value::Array(
                session
                    .read_only_files
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    options
}

fn update_session<F>(root: PathBuf, session_id: &str, update: F) -> Result<()>
where
    F: FnOnce(&mut PersistedSession),
{
    let mut session = read_session(&root, session_id)?;
    update(&mut session);
    write_session(&root, &session)
}

fn replace_session_messages(
    root: &Path,
    mut session: PersistedSession,
    messages: Vec<Value>,
) -> Result<PersistedSession> {
    session.messages = messages;
    session.updated_at = now_unix_seconds();
    write_session(root, &session)?;
    rewrite_runtime_events_from_session(root, &session)?;
    Ok(session)
}

fn hydrate_ccr_v2_internal_events_at(
    root: &Path,
    session_id: &str,
    events: Vec<kiana_remote::CcrV2InternalEvent>,
    subagent_events: Vec<kiana_remote::CcrV2InternalEvent>,
) -> Result<CcrV2HydrateReport> {
    validate_session_id(session_id)?;
    let now = now_unix_seconds();
    let mut skipped_event_count = 0_usize;
    let messages = events
        .iter()
        .filter_map(|event| match ccr_v2_internal_event_message(event) {
            Some(message) => Some(message),
            None => {
                skipped_event_count += 1;
                None
            }
        })
        .collect::<Vec<_>>();

    let existing = session_file(root, session_id)?
        .exists()
        .then(|| read_session(root, session_id))
        .transpose()?;
    let mut session = existing.unwrap_or_else(|| PersistedSession {
        session_id: session_id.to_string(),
        title: infer_session_title_from_messages(&messages),
        tag: None,
        parent_session_id: None,
        cwd: current_cwd_option(),
        created_at: now,
        updated_at: now,
        editable_files: Vec::new(),
        read_only_files: Vec::new(),
        messages: Vec::new(),
    });
    if session.title.is_none() {
        session.title = infer_session_title_from_messages(&messages);
    }
    session.messages = messages;
    session.updated_at = now;
    write_session(root, &session)?;
    rewrite_runtime_events_from_session(root, &session)?;
    let skipped_subagent_event_count =
        hydrate_ccr_v2_subagent_internal_events_at(root, session_id, &subagent_events)?;

    Ok(CcrV2HydrateReport {
        session_id: session.session_id,
        internal_event_count: events.len(),
        subagent_event_count: subagent_events.len(),
        message_count: session.messages.len(),
        skipped_event_count: skipped_event_count + skipped_subagent_event_count,
    })
}

fn hydrate_ccr_v2_subagent_internal_events_at(
    root: &Path,
    session_id: &str,
    events: &[kiana_remote::CcrV2InternalEvent],
) -> Result<usize> {
    validate_session_id(session_id)?;
    let mut grouped: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let mut skipped = 0_usize;

    for event in events {
        let Some(agent_id) = event
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            skipped += 1;
            continue;
        };
        grouped
            .entry(agent_id.to_string())
            .or_default()
            .push(&event.payload);
    }

    for (agent_id, payloads) in grouped {
        let path = subagent_transcript_file(root, session_id, &agent_id)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        write_jsonl_values(&path, payloads.into_iter())?;
    }

    Ok(skipped)
}

fn ccr_v2_internal_event_message(event: &kiana_remote::CcrV2InternalEvent) -> Option<Value> {
    if event.event_type != "transcript" {
        return None;
    }
    transcript_payload_message(&event.payload)
}

fn transcript_payload_message(payload: &Value) -> Option<Value> {
    if payload_has_role_and_content(payload) {
        return Some(payload.clone());
    }
    let message = payload.get("message")?;
    if payload_has_role_and_content(message) {
        return Some(message.clone());
    }
    None
}

fn payload_has_role_and_content(payload: &Value) -> bool {
    payload.get("role").and_then(Value::as_str).is_some() && payload.get("content").is_some()
}

fn infer_session_title_from_messages(messages: &[Value]) -> Option<String> {
    messages.iter().find_map(|message| {
        (message_role(message) == Some("user"))
            .then(|| message_content_as_text(message.get("content")?))
            .flatten()
            .map(|text| infer_title(&text))
    })
}

fn message_content_as_text(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        _ => None,
    }
    .map(|text| text.trim().to_string())
    .filter(|text| !text.is_empty())
}

fn persist_completed_model_prompt(
    root: &Path,
    session: PersistedSession,
    run: crate::runner::AssistantRunResult,
) -> Result<SdkResultMessage> {
    let crate::runner::AssistantRunResult {
        text,
        messages,
        iterations,
        stop_reason,
        structured_output,
        teammate_shutdown_approved,
    } = run;
    let session = replace_session_messages(root, session, runner_messages_to_values(messages))?;
    let mut result = serde_json::json!({
        "type": "sdk_prompt_completed",
        "session_id": session.session_id,
        "message_count": session.messages.len(),
        "status": "completed",
        "execution": "model",
        "assistant_text": text,
        "iterations": iterations,
        "stop_reason": stop_reason,
        "teammate_shutdown_approved": teammate_shutdown_approved,
    });
    if let Some(structured_output) = structured_output {
        result["structured_output"] = structured_output;
    }
    Ok(result)
}

fn list_sessions_at(root: &Path) -> Result<Vec<SdkSessionInfo>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let mut seen_session_ids = BTreeSet::new();
    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Some(session) = read_session_info_for_listing(&path)? {
            seen_session_ids.insert(session.session_id.clone());
            sessions.push(session);
        }
    }
    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(session_id) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if seen_session_ids.contains(session_id)
            || session_events_file(root, session_id)?.exists() == false
        {
            continue;
        }
        let session = read_session_from_event_tree(root, session_id)?;
        seen_session_ids.insert(session.session_id.clone());
        sessions.push(session.info());
    }

    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(sessions)
}

fn read_session_info_for_listing(path: &Path) -> Result<Option<SdkSessionInfo>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read session file {}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse session file {}", path.display()))?;
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let has_session_shape = ["session_id", "created_at", "updated_at", "messages"]
        .into_iter()
        .all(|key| object.contains_key(key));
    if !has_session_shape {
        return Ok(None);
    }

    let session: PersistedSession = serde_json::from_value(value)
        .with_context(|| format!("failed to parse session file {}", path.display()))?;
    if path.file_stem().and_then(|stem| stem.to_str()) != Some(session.session_id.as_str()) {
        return Ok(None);
    }
    Ok(Some(session.info()))
}

fn read_session(root: &Path, session_id: &str) -> Result<PersistedSession> {
    let path = session_file(root, session_id)?;
    if path.exists() {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("session '{}' was not found", session_id))?;
        return serde_json::from_str(&contents)
            .with_context(|| format!("failed to parse session file {}", path.display()));
    }
    read_session_from_event_tree(root, session_id)
}

fn write_session(root: &Path, session: &PersistedSession) -> Result<()> {
    validate_session_id(&session.session_id)?;
    fs::create_dir_all(root).with_context(|| format!("failed to create {}", root.display()))?;

    let path = session_file(root, &session.session_id)?;
    let tmp_path = root.join(format!(
        ".{}.{}.tmp",
        session.session_id,
        std::process::id()
    ));
    let contents = serde_json::to_string_pretty(session)?;
    fs::write(&tmp_path, contents)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    append_runtime_events_for_missing_messages(root, session)?;
    Ok(())
}

fn session_file(root: &Path, session_id: &str) -> Result<PathBuf> {
    validate_session_id(session_id)?;
    Ok(root.join(format!("{}.json", session_id)))
}

fn session_tree_dir(root: &Path, session_id: &str) -> Result<PathBuf> {
    validate_session_id(session_id)?;
    Ok(root.join(session_id))
}

fn session_events_file(root: &Path, session_id: &str) -> Result<PathBuf> {
    Ok(session_tree_dir(root, session_id)?.join("events.jsonl"))
}

fn read_session_from_event_tree(root: &Path, session_id: &str) -> Result<PersistedSession> {
    validate_session_id(session_id)?;
    let path = session_events_file(root, session_id)?;
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("session '{}' was not found", session_id))?;
    let mut messages = Vec::new();
    let mut created_at = None;
    let mut updated_at = None;

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
                "runtime event {} in {} belongs to session '{}'",
                line_index + 1,
                path.display(),
                event.session_id
            ));
        }
        let event_time = parse_runtime_event_timestamp(&event.timestamp);
        created_at = Some(created_at.unwrap_or(event_time));
        updated_at = Some(event_time);
        match event.payload {
            kiana_types::RuntimeEventPayload::UserMessage(message)
            | kiana_types::RuntimeEventPayload::AssistantMessage(message) => {
                messages.push(message.message);
            }
            _ => {}
        }
    }

    let now = now_unix_seconds();
    Ok(PersistedSession {
        session_id: session_id.to_string(),
        title: infer_session_title_from_messages(&messages),
        tag: None,
        parent_session_id: None,
        cwd: None,
        created_at: created_at.unwrap_or(now),
        updated_at: updated_at.unwrap_or_else(|| created_at.unwrap_or(now)),
        editable_files: Vec::new(),
        read_only_files: Vec::new(),
        messages,
    })
}

fn append_runtime_events_for_missing_messages(
    root: &Path,
    session: &PersistedSession,
) -> Result<()> {
    if session.messages.is_empty() {
        return Ok(());
    }

    let path = session_events_file(root, &session.session_id)?;
    let existing_count = read_existing_runtime_event_count(&path)?;
    if existing_count >= session.messages.len() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open {}", path.display()))?;

    for (index, message) in session.messages.iter().enumerate().skip(existing_count) {
        let event = runtime_event_from_session_message(session, index, message);
        writeln!(file, "{}", serde_json::to_string(&event)?)
            .with_context(|| format!("failed to append {}", path.display()))?;
    }

    Ok(())
}

fn rewrite_runtime_events_from_session(root: &Path, session: &PersistedSession) -> Result<()> {
    let path = session_events_file(root, &session.session_id)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut contents = String::new();
    for (index, message) in session.messages.iter().enumerate() {
        let event = runtime_event_from_session_message(session, index, message);
        contents.push_str(&serde_json::to_string(&event)?);
        contents.push('\n');
    }
    fs::write(&path, contents).with_context(|| format!("failed to write {}", path.display()))
}

fn runtime_event_from_session_message(
    session: &PersistedSession,
    index: usize,
    message: &Value,
) -> kiana_types::RuntimeEvent {
    let turn_id = format!("turn-{index}");
    let parent_turn_id = index.checked_sub(1).map(|parent| format!("turn-{parent}"));
    let timestamp = runtime_timestamp_from_message(message);
    kiana_types::sdk_message_to_runtime_event(
        &session.session_id,
        &turn_id,
        parent_turn_id,
        index as u64,
        &timestamp,
        message.clone(),
    )
}

fn read_existing_runtime_event_count(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut count = 0;
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        serde_json::from_str::<Value>(line).with_context(|| {
            format!(
                "failed to parse runtime event {} in {}",
                index + 1,
                path.display()
            )
        })?;
        count += 1;
    }
    Ok(count)
}

fn runtime_timestamp_from_message(message: &Value) -> String {
    match message.get("created_at") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(value) => {
            serde_json::to_string(value).unwrap_or_else(|_| now_unix_seconds().to_string())
        }
        None => now_unix_seconds().to_string(),
    }
}

fn parse_runtime_event_timestamp(timestamp: &str) -> u64 {
    timestamp.trim().parse::<u64>().unwrap_or_default()
}

fn subagent_transcript_file(root: &Path, session_id: &str, agent_id: &str) -> Result<PathBuf> {
    validate_session_id(session_id)?;
    let agent_id = agent_id.trim();
    if agent_id.is_empty() {
        return Err(anyhow!("agent_id cannot be empty"));
    }
    Ok(root
        .join(session_id)
        .join("subagents")
        .join(format!("agent-{}.jsonl", encode_path_component(agent_id))))
}

fn encode_path_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'@' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn write_jsonl_values<'a, I>(path: &Path, values: I) -> Result<()>
where
    I: IntoIterator<Item = &'a Value>,
{
    let mut contents = String::new();
    for value in values {
        contents.push_str(&serde_json::to_string(value)?);
        contents.push('\n');
    }
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
}

fn read_jsonl_values(path: &Path) -> Result<Vec<Value>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read transcript file {}", path.display()))?;
    contents
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if session_id.trim().is_empty() {
        return Err(anyhow!("session_id cannot be empty"));
    }
    if session_id.contains('/') || session_id.contains('\\') || session_id.contains("..") {
        return Err(anyhow!("session_id contains invalid path characters"));
    }
    Ok(())
}

fn default_sessions_dir() -> PathBuf {
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

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn string_option(options: &HashMap<String, Value>, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn string_list_option(
    options: &HashMap<String, Value>,
    keys: &[&str],
) -> Result<Option<Vec<String>>> {
    for key in keys {
        let Some(value) = options.get(*key) else {
            continue;
        };
        let files = match value {
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    item.as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .ok_or_else(|| anyhow!("{key}[{index}] must be a non-empty string"))
                })
                .collect::<Result<Vec<_>>>()?,
            Value::String(value) => value
                .split([',', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
            _ => return Err(anyhow!("{key} must be an array or string")),
        };
        return Ok(Some(files));
    }
    Ok(None)
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn current_cwd_option() -> Option<String> {
    std::env::current_dir()
        .ok()
        .map(|path| path.to_string_lossy().to_string())
        .and_then(|path| normalize_optional_string(Some(path)))
}

fn session_title_option(options: &HashMap<String, Value>) -> Option<String> {
    normalize_optional_string(
        string_option(options, "title").or_else(|| string_option(options, "name")),
    )
}

fn should_execute_model(options: &HashMap<String, Value>) -> bool {
    bool_option(options, "execute")
        .or_else(|| bool_option(options, "run_model"))
        .unwrap_or_else(|| env_truthy("KIANA_SDK_EXECUTE_MODEL"))
}

fn bool_option(options: &HashMap<String, Value>, key: &str) -> Option<bool> {
    match options.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(value) => parse_bool(value),
        _ => None,
    }
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| parse_bool(&value))
        .unwrap_or(false)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn runner_messages_to_values(messages: Vec<kiana_services::api::messages::Message>) -> Vec<Value> {
    messages
        .into_iter()
        .map(|message| {
            serde_json::json!({
                "role": message.role,
                "content": message.content,
            })
        })
        .collect()
}

fn message_role(message: &Value) -> Option<&str> {
    message.get("role").and_then(Value::as_str)
}

fn infer_title(prompt: &str) -> String {
    let title = prompt.lines().next().unwrap_or(prompt).trim();
    let mut chars = title.chars();
    let shortened: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{}...", shortened)
    } else {
        shortened.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router,
    };
    use tokio::task::JoinHandle;

    fn test_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kiana-sdk-test-{}-{}", name, Uuid::new_v4()));
        let _ = fs::remove_dir_all(&root);
        root
    }

    #[test]
    fn sdk_runtime_event_adapter_maps_local_stream_events() {
        let events = runtime_events_from_sdk_prompt_stream_event(
            "session-1",
            "turn-1",
            None,
            0,
            "2026-06-23T00:00:00Z",
            SdkPromptStreamEvent::Model(
                kiana_services::api::streaming::StreamEvent::ContentBlockDelta {
                    index: 0,
                    delta: kiana_services::api::streaming::Delta::TextDelta {
                        text: "partial".to_string(),
                    },
                },
            ),
        );

        assert_eq!(events.len(), 1);
        let value = serde_json::to_value(&events[0]).unwrap();
        assert_eq!(value["type"], "stream_delta");
        assert_eq!(value["delta"]["delta"]["text"], "partial");
    }

    #[test]
    fn sdk_runtime_event_adapter_maps_permission_request_and_result() {
        let permission_event = runtime_event_from_sdk_permission_prompt_request(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            1,
            "2026-06-23T00:00:01Z",
            &kiana_tools::tool_execution::PermissionPromptRequest {
                request_id: "perm-1".to_string(),
                tool_name: "Bash".to_string(),
                input: serde_json::json!({"command": "pwd"}),
                tool_use_id: "toolu_bash".to_string(),
                permission_suggestions: serde_json::json!([]),
                blocked_path: None,
                decision_reason: serde_json::json!({"reason": "ask mode"}),
                agent_id: None,
            },
        );
        let permission_value = serde_json::to_value(permission_event).unwrap();
        assert_eq!(permission_value["type"], "permission_request");
        assert_eq!(permission_value["request_id"], "perm-1");
        assert_eq!(permission_value["input"]["command"], "pwd");

        let result_event = runtime_event_from_sdk_result(
            "session-1",
            "turn-2",
            Some("turn-1".to_string()),
            2,
            "2026-06-23T00:00:02Z",
            &crate::runner::AssistantRunResult {
                text: "done".to_string(),
                messages: vec![kiana_services::api::messages::Message {
                    role: "assistant".to_string(),
                    content: serde_json::json!([{"type": "text", "text": "done"}]),
                }],
                iterations: 1,
                stop_reason: "model_stop".to_string(),
                structured_output: None,
                teammate_shutdown_approved: false,
            },
        );
        let result_value = serde_json::to_value(result_event).unwrap();
        assert_eq!(result_value["type"], "result");
        assert_eq!(result_value["stop_reason"], "model_stop");
        assert_eq!(result_value["assistant_text"], "done");
        assert_eq!(result_value["metadata"]["iterations"], 1);
    }

    #[tokio::test]
    async fn sdk_session_store_round_trips_messages_and_metadata() {
        let root = test_root("round-trip");
        let mut options = HashMap::new();
        options.insert(
            "title".to_string(),
            Value::String("Initial title".to_string()),
        );
        options.insert("tag".to_string(), Value::String("dev".to_string()));
        options.insert(
            "cwd".to_string(),
            Value::String(root.to_string_lossy().to_string()),
        );

        let session = create_session_at(root.clone(), options).unwrap();
        assert_eq!(session.title.as_deref(), Some("Initial title"));
        assert_eq!(session.tag.as_deref(), Some("dev"));
        assert_eq!(
            session.cwd.as_deref(),
            Some(root.to_string_lossy().as_ref())
        );

        let mut prompt_options = HashMap::new();
        prompt_options.insert(
            "session_id".to_string(),
            Value::String(session.session_id.clone()),
        );
        let updated = append_user_prompt(
            root.clone(),
            "Inspect the local SDK session store".to_string(),
            prompt_options,
        )
        .unwrap();
        assert_eq!(updated.messages.len(), 1);
        assert_eq!(updated.messages[0]["role"], "user");

        let listed = list_sessions_at(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, session.session_id);
        assert_eq!(listed[0].message_count, 1);
        assert_eq!(listed[0].assistant_message_count, 0);
        assert_eq!(listed[0].last_role.as_deref(), Some("user"));
        assert_eq!(
            listed[0].cwd.as_deref(),
            Some(root.to_string_lossy().as_ref())
        );

        update_session(root.clone(), &session.session_id, |session| {
            session.title = Some("Renamed".to_string());
            session.tag = None;
        })
        .unwrap();
        let renamed = read_session(&root, &session.session_id).unwrap();
        assert_eq!(renamed.title.as_deref(), Some("Renamed"));
        assert_eq!(renamed.tag, None);

        let forked_id =
            fork_session_at_with_options(&root, &session.session_id, HashMap::new()).unwrap();
        let forked = read_session(&root, &forked_id).unwrap();
        assert_eq!(
            forked.parent_session_id.as_deref(),
            Some(session.session_id.as_str())
        );
        assert_eq!(forked.messages.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prompt_options_inherit_session_file_sets_without_overwriting_explicit_options() {
        let session = PersistedSession {
            session_id: "file-set-session".to_string(),
            title: None,
            tag: None,
            parent_session_id: None,
            cwd: None,
            created_at: 1,
            updated_at: 1,
            editable_files: vec!["src/lib.rs".to_string()],
            read_only_files: vec!["README.md".to_string()],
            messages: Vec::new(),
        };

        let inherited = prompt_options_with_session_file_sets(&session, HashMap::new());
        assert_eq!(
            inherited["editable_files"],
            serde_json::json!(["src/lib.rs"])
        );
        assert_eq!(
            inherited["read_only_files"],
            serde_json::json!(["README.md"])
        );

        let explicit = prompt_options_with_session_file_sets(
            &session,
            HashMap::from([(
                "editable_files".to_string(),
                serde_json::json!(["override.rs"]),
            )]),
        );
        assert_eq!(
            explicit["editable_files"],
            serde_json::json!(["override.rs"])
        );
        assert_eq!(
            explicit["read_only_files"],
            serde_json::json!(["README.md"])
        );
    }

    #[test]
    fn sdk_session_store_writes_runtime_event_jsonl_tree() {
        let root = test_root("runtime-event-tree");
        let session = create_session_at(
            root.clone(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String("event-session".to_string()),
            )]),
        )
        .unwrap();

        append_user_prompt(
            root.clone(),
            "first prompt".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();
        append_user_prompt(
            root.clone(),
            "second prompt".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();

        let event_file = root.join("event-session").join("events.jsonl");
        let lines = fs::read_to_string(&event_file)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", event_file.display()))
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["type"], "user_message");
        assert_eq!(lines[0]["sequence"], 0);
        assert_eq!(lines[0]["turn_id"], "turn-0");
        assert!(lines[0].get("parent_turn_id").is_none());
        assert_eq!(lines[0]["message"]["content"], "first prompt");
        assert_eq!(lines[1]["type"], "user_message");
        assert_eq!(lines[1]["sequence"], 1);
        assert_eq!(lines[1]["turn_id"], "turn-1");
        assert_eq!(lines[1]["parent_turn_id"], "turn-0");
        assert_eq!(lines[1]["message"]["content"], "second prompt");

        let legacy_session = read_session(&root, "event-session").unwrap();
        assert_eq!(legacy_session.messages.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn replacing_session_messages_rebuilds_runtime_event_jsonl_tree() {
        let root = test_root("runtime-event-rebuild");
        let session = create_session_at(
            root.clone(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String("replace-session".to_string()),
            )]),
        )
        .unwrap();
        append_user_prompt(
            root.clone(),
            "first stale prompt".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();
        append_user_prompt(
            root.clone(),
            "second stale prompt".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();

        let session = read_session(&root, "replace-session").unwrap();
        replace_session_messages(
            &root,
            session,
            vec![serde_json::json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "fresh replacement"}],
                "created_at": 99
            })],
        )
        .unwrap();

        let event_file = root.join("replace-session").join("events.jsonl");
        let lines = fs::read_to_string(&event_file)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", event_file.display()))
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["type"], "assistant_message");
        assert_eq!(lines[0]["sequence"], 0);
        assert_eq!(lines[0]["turn_id"], "turn-0");
        assert!(lines[0].get("parent_turn_id").is_none());
        assert_eq!(
            lines[0]["message"]["content"][0]["text"],
            "fresh replacement"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sdk_session_store_reads_jsonl_only_runtime_event_tree() {
        let root = test_root("jsonl-only-session");
        let events_dir = root.join("jsonl-session");
        fs::create_dir_all(&events_dir).unwrap();
        let events = [
            kiana_types::sdk_message_to_runtime_event(
                "jsonl-session",
                "turn-0",
                None,
                0,
                "10",
                serde_json::json!({
                    "role": "user",
                    "content": "jsonl only prompt",
                    "created_at": 10
                }),
            ),
            kiana_types::sdk_message_to_runtime_event(
                "jsonl-session",
                "turn-1",
                Some("turn-0".to_string()),
                1,
                "11",
                serde_json::json!({
                    "role": "assistant",
                    "content": [{"type": "text", "text": "jsonl only reply"}],
                    "created_at": 11
                }),
            ),
        ];
        fs::write(
            events_dir.join("events.jsonl"),
            events
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
                + "\n",
        )
        .unwrap();

        let session = read_session(&root, "jsonl-session").unwrap();
        assert_eq!(session.session_id, "jsonl-session");
        assert_eq!(session.title.as_deref(), Some("jsonl only prompt"));
        assert_eq!(session.created_at, 10);
        assert_eq!(session.updated_at, 11);
        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0]["content"], "jsonl only prompt");
        assert_eq!(
            session.messages[1]["content"][0]["text"],
            "jsonl only reply"
        );

        let listed = list_sessions_at(&root).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, "jsonl-session");
        assert_eq!(listed[0].message_count, 2);
        assert_eq!(listed[0].assistant_message_count, 1);
        assert_eq!(listed[0].last_role.as_deref(), Some("assistant"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hydrate_ccr_v2_internal_events_writes_local_session_messages() {
        let root = test_root("hydrate-ccr-v2");
        let events = vec![
            ccr_v2_internal_event(
                "int-1",
                "transcript",
                serde_json::json!({
                    "type": "user",
                    "uuid": "user-1",
                    "message": {
                        "role": "user",
                        "content": "restore this task"
                    }
                }),
                None,
            ),
            ccr_v2_internal_event(
                "int-2",
                "transcript",
                serde_json::json!({
                    "role": "assistant",
                    "content": [{"type": "text", "text": "restored answer"}]
                }),
                None,
            ),
            ccr_v2_internal_event(
                "int-3",
                "diagnostic",
                serde_json::json!({
                    "type": "diagnostic",
                    "message": "not transcript"
                }),
                None,
            ),
        ];
        let subagent_events = vec![
            ccr_v2_internal_event(
                "int-agent-1",
                "transcript",
                serde_json::json!({
                    "type": "assistant",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": "agent answer"}]
                    }
                }),
                Some("agent-1"),
            ),
            ccr_v2_internal_event(
                "int-agent-2",
                "transcript",
                serde_json::json!({
                    "type": "user",
                    "message": {
                        "role": "user",
                        "content": "unsafe agent id path"
                    }
                }),
                Some("agent/unsafe"),
            ),
            ccr_v2_internal_event(
                "int-agent-missing",
                "transcript",
                serde_json::json!({
                    "type": "assistant",
                    "message": {
                        "role": "assistant",
                        "content": "missing agent id"
                    }
                }),
                None,
            ),
        ];

        let report =
            hydrate_ccr_v2_internal_events_at(&root, "cse_session_1", events, subagent_events)
                .unwrap();

        assert_eq!(report.session_id, "cse_session_1");
        assert_eq!(report.internal_event_count, 3);
        assert_eq!(report.subagent_event_count, 3);
        assert_eq!(report.message_count, 2);
        assert_eq!(report.skipped_event_count, 2);
        let session = read_session(&root, "cse_session_1").unwrap();
        assert_eq!(session.title.as_deref(), Some("restore this task"));
        assert_eq!(session.messages[0]["role"], "user");
        assert_eq!(session.messages[0]["content"], "restore this task");
        assert_eq!(session.messages[1]["role"], "assistant");
        assert_eq!(session.messages[1]["content"][0]["text"], "restored answer");
        let agent_entries = read_jsonl_values(
            &subagent_transcript_file(&root, "cse_session_1", "agent-1").unwrap(),
        )
        .unwrap();
        assert_eq!(agent_entries.len(), 1);
        assert_eq!(
            agent_entries[0]["message"]["content"][0]["text"],
            "agent answer"
        );
        let unsafe_agent_file =
            subagent_transcript_file(&root, "cse_session_1", "agent/unsafe").unwrap();
        assert_eq!(
            unsafe_agent_file.file_name().and_then(|name| name.to_str()),
            Some("agent-agent%2Funsafe.jsonl")
        );
        let unsafe_agent_entries = read_jsonl_values(&unsafe_agent_file).unwrap();
        assert_eq!(
            unsafe_agent_entries[0]["message"]["content"],
            "unsafe agent id path"
        );
        assert!(
            !subagent_transcript_file(&root, "cse_session_1", "agent-missing")
                .unwrap()
                .exists()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hydrate_ccr_v2_internal_events_preserves_existing_metadata() {
        let root = test_root("hydrate-preserve");
        create_session_at(
            root.clone(),
            HashMap::from([
                (
                    "session_id".to_string(),
                    Value::String("cse_session_2".to_string()),
                ),
                (
                    "title".to_string(),
                    Value::String("Existing title".to_string()),
                ),
                ("tag".to_string(), Value::String("remote".to_string())),
            ]),
        )
        .unwrap();
        let events = vec![ccr_v2_internal_event(
            "int-1",
            "transcript",
            serde_json::json!({
                "message": {
                    "role": "user",
                    "content": "fresh remote state"
                }
            }),
            None,
        )];

        let report =
            hydrate_ccr_v2_internal_events_at(&root, "cse_session_2", events, Vec::new()).unwrap();

        assert_eq!(report.message_count, 1);
        let session = read_session(&root, "cse_session_2").unwrap();
        assert_eq!(session.title.as_deref(), Some("Existing title"));
        assert_eq!(session.tag.as_deref(), Some("remote"));
        assert_eq!(session.messages[0]["content"], "fresh remote state");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_and_fork_sessions_accept_explicit_id_and_name() {
        let root = test_root("explicit-id");
        let session = create_session_at(
            root.clone(),
            HashMap::from([
                (
                    "session_id".to_string(),
                    Value::String("fixed-session".to_string()),
                ),
                (
                    "name".to_string(),
                    Value::String("Named session".to_string()),
                ),
            ]),
        )
        .unwrap();
        assert_eq!(session.session_id, "fixed-session");
        assert_eq!(session.title.as_deref(), Some("Named session"));

        let updated = append_user_prompt(
            root.clone(),
            "keep this history".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();
        assert_eq!(updated.messages.len(), 1);

        let forked_id = fork_session_at_with_options(
            &root,
            &session.session_id,
            HashMap::from([
                (
                    "session_id".to_string(),
                    Value::String("forked-session".to_string()),
                ),
                (
                    "title".to_string(),
                    Value::String("Forked title".to_string()),
                ),
            ]),
        )
        .unwrap();
        assert_eq!(forked_id, "forked-session");
        let forked = read_session(&root, &forked_id).unwrap();
        assert_eq!(forked.title.as_deref(), Some("Forked title"));
        assert_eq!(forked.parent_session_id.as_deref(), Some("fixed-session"));
        assert_eq!(forked.messages.len(), 1);

        let duplicate_error = fork_session_at_with_options(
            &root,
            &session.session_id,
            HashMap::from([(
                "session_id".to_string(),
                Value::String("forked-session".to_string()),
            )]),
        )
        .unwrap_err()
        .to_string();
        assert!(duplicate_error.contains("already exists"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn append_user_prompt_only_creates_missing_explicit_session_when_requested() {
        let root = test_root("explicit-missing");

        let missing_error = append_user_prompt(
            root.clone(),
            "should fail".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String("missing-session".to_string()),
            )]),
        )
        .unwrap_err()
        .to_string();
        assert!(missing_error.contains("was not found"));

        let created = append_user_prompt(
            root.clone(),
            "should create".to_string(),
            HashMap::from([
                (
                    "session_id".to_string(),
                    Value::String("missing-session".to_string()),
                ),
                ("create_session_if_missing".to_string(), Value::Bool(true)),
                (
                    "name".to_string(),
                    Value::String("Created by prompt".to_string()),
                ),
            ]),
        )
        .unwrap();

        assert_eq!(created.session_id, "missing-session");
        assert_eq!(created.title.as_deref(), Some("Created by prompt"));
        assert_eq!(created.messages[0]["content"], "should create");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stream_json_history_messages_seed_new_prompt_session() {
        let root = test_root("stream-json-history");
        let session = append_user_prompt(
            root.clone(),
            "continue from history".to_string(),
            HashMap::from([(
                STREAM_JSON_HISTORY_MESSAGES_OPTION.to_string(),
                serde_json::json!([{
                    "role": "assistant",
                    "content": [{
                        "type": "text",
                        "text": "previous answer"
                    }]
                }]),
            )]),
        )
        .unwrap();

        assert_eq!(session.messages.len(), 2);
        assert_eq!(session.messages[0]["role"], "assistant");
        assert_eq!(session.messages[0]["content"][0]["text"], "previous answer");
        assert_eq!(session.messages[1]["role"], "user");
        assert_eq!(session.messages[1]["content"], "continue from history");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stream_json_history_messages_do_not_duplicate_existing_session() {
        let root = test_root("stream-json-history-existing");
        let session = create_session_at(root.clone(), HashMap::new()).unwrap();
        append_user_prompt(
            root.clone(),
            "existing prompt".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();

        let updated = append_user_prompt(
            root.clone(),
            "new prompt".to_string(),
            HashMap::from([
                (
                    "session_id".to_string(),
                    Value::String(session.session_id.clone()),
                ),
                (
                    STREAM_JSON_HISTORY_MESSAGES_OPTION.to_string(),
                    serde_json::json!([{
                        "role": "assistant",
                        "content": "duplicated history"
                    }]),
                ),
            ]),
        )
        .unwrap();

        assert_eq!(updated.messages.len(), 2);
        assert_eq!(updated.messages[0]["content"], "existing prompt");
        assert_eq!(updated.messages[1]["content"], "new prompt");

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn prompt_without_persistence_counts_stream_json_history_messages() {
        let result = prompt_without_persistence(
            "current prompt".to_string(),
            HashMap::from([
                ("execute".to_string(), Value::Bool(false)),
                (
                    "session_id".to_string(),
                    Value::String("transient-history".to_string()),
                ),
                (
                    STREAM_JSON_HISTORY_MESSAGES_OPTION.to_string(),
                    serde_json::json!([{
                        "role": "assistant",
                        "content": "previous answer"
                    }]),
                ),
            ]),
            None,
        )
        .await
        .unwrap();

        assert_eq!(result["session_id"], "transient-history");
        assert_eq!(result["message_count"], 2);
        assert_eq!(result["persistence"], "disabled");
    }

    #[tokio::test]
    async fn model_execution_failure_does_not_persist_missing_session_prompt() {
        let root = test_root("execute-failure-new");

        let error = prompt_with_persistence_at(
            root.clone(),
            "this should not become latest".to_string(),
            failing_execution_options("failed-session"),
            None,
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("Fallback model cannot be the same"));
        assert!(!session_file(&root, "failed-session").unwrap().exists());
        assert!(list_sessions_at(&root).unwrap().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn model_execution_failure_preserves_existing_session_history() {
        let root = test_root("execute-failure-existing");
        let session = create_session_at(
            root.clone(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String("existing-session".to_string()),
            )]),
        )
        .unwrap();
        append_user_prompt(
            root.clone(),
            "previous committed prompt".to_string(),
            HashMap::from([(
                "session_id".to_string(),
                Value::String(session.session_id.clone()),
            )]),
        )
        .unwrap();

        let error = prompt_with_persistence_at(
            root.clone(),
            "failed follow-up prompt".to_string(),
            failing_execution_options(&session.session_id),
            None,
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("Fallback model cannot be the same"));
        let persisted = read_session(&root, &session.session_id).unwrap();
        assert_eq!(persisted.messages.len(), 1);
        assert_eq!(
            persisted.messages[0]["content"],
            "previous committed prompt"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn streaming_model_execution_failure_does_not_persist_missing_session_prompt() {
        let root = test_root("streaming-execute-failure-new");

        let error = prompt_streaming_with_persistence_at(
            root.clone(),
            "streaming failure should not persist".to_string(),
            failing_execution_options("failed-streaming-session"),
            |_| Ok(()),
            None,
            None,
        )
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("Fallback model cannot be the same"));
        assert!(!session_file(&root, "failed-streaming-session")
            .unwrap()
            .exists());
        assert!(list_sessions_at(&root).unwrap().is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn model_execution_success_persists_completed_messages() {
        let root = test_root("execute-success");
        let (base_url, server) = start_sdk_mock_messages_server("model ok").await;

        let result = prompt_with_persistence_at(
            root.clone(),
            "hello model".to_string(),
            HashMap::from([
                ("execute".to_string(), Value::Bool(true)),
                ("api_key".to_string(), Value::String("test-key".to_string())),
                ("base_url".to_string(), Value::String(base_url)),
                ("model".to_string(), Value::String("mock-model".to_string())),
                ("tools".to_string(), Value::String("".to_string())),
            ]),
            None,
        )
        .await
        .unwrap();

        assert_eq!(result["type"], "sdk_prompt_completed");
        assert_eq!(result["assistant_text"], "model ok");
        let session_id = result["session_id"].as_str().unwrap();
        let persisted = read_session(&root, session_id).unwrap();
        assert_eq!(persisted.messages.len(), 2);
        assert_eq!(persisted.messages[0]["role"], "user");
        assert_eq!(persisted.messages[0]["content"], "hello model");
        assert_eq!(persisted.messages[1]["role"], "assistant");
        assert_eq!(persisted.messages[1]["content"][0]["text"], "model ok");

        server.abort();
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn model_execution_preserves_stream_json_history_messages() {
        let root = test_root("execute-stream-json-history");
        let (base_url, server) = start_sdk_mock_messages_server("model ok").await;

        let result = prompt_with_persistence_at(
            root.clone(),
            "follow up".to_string(),
            HashMap::from([
                ("execute".to_string(), Value::Bool(true)),
                ("api_key".to_string(), Value::String("test-key".to_string())),
                ("base_url".to_string(), Value::String(base_url)),
                ("model".to_string(), Value::String("mock-model".to_string())),
                ("tools".to_string(), Value::String("".to_string())),
                (
                    STREAM_JSON_HISTORY_MESSAGES_OPTION.to_string(),
                    serde_json::json!([{
                        "role": "assistant",
                        "content": [{
                            "type": "text",
                            "text": "previous answer"
                        }]
                    }]),
                ),
            ]),
            None,
        )
        .await
        .unwrap();

        let session_id = result["session_id"].as_str().unwrap();
        let persisted = read_session(&root, session_id).unwrap();
        assert_eq!(persisted.messages.len(), 3);
        assert_eq!(persisted.messages[0]["role"], "assistant");
        assert_eq!(
            persisted.messages[0]["content"][0]["text"],
            "previous answer"
        );
        assert_eq!(persisted.messages[1]["role"], "user");
        assert_eq!(persisted.messages[1]["content"], "follow up");
        assert_eq!(persisted.messages[2]["role"], "assistant");
        assert_eq!(persisted.messages[2]["content"][0]["text"], "model ok");

        server.abort();
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn prompt_without_persistence_records_without_session_store() {
        let result = prompt_without_persistence(
            "transient prompt".to_string(),
            HashMap::from([
                ("execute".to_string(), Value::Bool(false)),
                (
                    "session_id".to_string(),
                    Value::String("transient-session".to_string()),
                ),
            ]),
            None,
        )
        .await
        .unwrap();

        assert_eq!(result["session_id"], "transient-session");
        assert_eq!(result["message_count"], 1);
        assert_eq!(result["execution"], "record_only");
        assert_eq!(result["persistence"], "disabled");
    }

    #[test]
    fn list_sessions_ignores_unrelated_json_files() {
        let root = test_root("ignore-unrelated");
        let session = create_session_at(root.clone(), HashMap::new()).unwrap();
        fs::write(
            root.join("sdk-result.json"),
            serde_json::json!({
                "type": "sdk_prompt_recorded",
                "session_id": session.session_id,
                "message_count": 1
            })
            .to_string(),
        )
        .unwrap();
        fs::write(root.join("empty-output.json"), "").unwrap();
        fs::write(root.join("note.txt"), "not json").unwrap();

        let listed = list_sessions_at(&root).unwrap();

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, session.session_id);
        assert!(read_session(&root, "sdk-result").is_err());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_sessions_rejects_session_shaped_invalid_files() {
        let root = test_root("reject-invalid-shaped");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("broken.json"),
            serde_json::json!({
                "session_id": "broken",
                "created_at": 1,
                "updated_at": 2,
                "messages": "not an array"
            })
            .to_string(),
        )
        .unwrap();

        let error = list_sessions_at(&root).unwrap_err().to_string();

        assert!(error.contains("failed to parse session file"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_execution_requires_explicit_opt_in() {
        let options = HashMap::new();
        assert!(!should_execute_model(&options));

        let mut options = HashMap::new();
        options.insert("execute".to_string(), Value::Bool(true));
        assert!(should_execute_model(&options));

        options.insert("execute".to_string(), Value::String("false".to_string()));
        assert!(!should_execute_model(&options));
    }

    fn failing_execution_options(session_id: &str) -> HashMap<String, Value> {
        HashMap::from([
            ("execute".to_string(), Value::Bool(true)),
            ("api_key".to_string(), Value::String("test-key".to_string())),
            ("model".to_string(), Value::String("same-model".to_string())),
            (
                "fallback_model".to_string(),
                Value::String("same-model".to_string()),
            ),
            (
                "session_id".to_string(),
                Value::String(session_id.to_string()),
            ),
            ("create_session_if_missing".to_string(), Value::Bool(true)),
            ("tools".to_string(), Value::String("".to_string())),
        ])
    }

    fn ccr_v2_internal_event(
        event_id: &str,
        event_type: &str,
        payload: Value,
        agent_id: Option<&str>,
    ) -> kiana_remote::CcrV2InternalEvent {
        kiana_remote::CcrV2InternalEvent {
            event_id: event_id.to_string(),
            event_type: event_type.to_string(),
            payload,
            event_metadata: None,
            is_compaction: false,
            created_at: "2026-06-16T00:00:00Z".to_string(),
            agent_id: agent_id.map(str::to_string),
        }
    }

    async fn start_sdk_mock_messages_server(text: &'static str) -> (String, JoinHandle<()>) {
        let app = Router::new()
            .route("/v1/messages", post(handle_sdk_mock_messages_request))
            .with_state(text.to_string());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{}", addr), server)
    }

    async fn handle_sdk_mock_messages_request(
        State(text): State<String>,
        Json(body): Json<Value>,
    ) -> impl IntoResponse {
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("mock-model");
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": "msg_sdk_mock",
                "model": model,
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": text
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
}
