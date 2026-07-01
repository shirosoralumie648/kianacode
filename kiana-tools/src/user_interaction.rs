use crate::tool::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const USER_QUESTIONS_KEY: &str = "user_questions";
const USER_MESSAGES_KEY: &str = "user_messages";
const USER_FILES_KEY: &str = "user_files";

#[derive(Debug, Deserialize, Serialize)]
struct QuestionOption {
    label: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    preview: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Question {
    question: String,
    header: String,
    options: Vec<QuestionOption>,
    #[serde(default, alias = "multiSelect", rename = "multiSelect")]
    multi_select: bool,
}

#[derive(Debug, Deserialize)]
struct AskUserQuestionInput {
    questions: Vec<Question>,
    #[serde(default)]
    answers: HashMap<String, String>,
    #[serde(default)]
    annotations: Option<Value>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SendUserMessageInput {
    message: String,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default = "default_message_status")]
    status: String,
}

#[derive(Debug, Deserialize)]
struct SendUserFileInput {
    #[serde(default)]
    files: Vec<String>,
    #[serde(default, alias = "path")]
    file_path: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SleepInput {
    #[serde(default, alias = "milliseconds", alias = "timeout_ms")]
    duration_ms: Option<u64>,
    #[serde(default)]
    seconds: Option<f64>,
}

pub struct AskUserQuestionTool;
pub struct SendUserMessageTool;
pub struct SendUserFileTool;
pub struct SleepTool;

impl AskUserQuestionTool {
    pub fn new() -> Self {
        Self
    }
}

impl SendUserMessageTool {
    pub fn new() -> Self {
        Self
    }
}

impl SendUserFileTool {
    pub fn new() -> Self {
        Self
    }
}

impl SleepTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }

    fn description(&self) -> &str {
        "Ask the user one or more multiple-choice questions and record answers"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("prompt the user with multiple-choice questions")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": 4,
                    "items": {
                        "type": "object",
                        "properties": {
                            "question": { "type": "string" },
                            "header": { "type": "string" },
                            "options": {
                                "type": "array",
                                "minItems": 2,
                                "maxItems": 4,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "description": { "type": "string" },
                                        "preview": { "type": "string" }
                                    },
                                    "required": ["label"]
                                }
                            },
                            "multiSelect": { "type": "boolean" }
                        },
                        "required": ["question", "header", "options"]
                    }
                },
                "answers": {
                    "type": "object",
                    "additionalProperties": { "type": "string" }
                },
                "annotations": { "type": "object" },
                "metadata": { "type": "object" }
            },
            "required": ["questions"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "question_id": { "type": "string" },
                "questions": { "type": "array" },
                "answers": { "type": "object" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: AskUserQuestionInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        validate_questions(&input.questions)
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: AskUserQuestionInput = serde_json::from_value(input.clone())?;
        let validation = validate_questions(&input.questions);
        if !validation.result {
            return Err(ToolError::ValidationError(
                validation
                    .message
                    .unwrap_or_else(|| "invalid questions".to_string()),
            ));
        }

        let question_id = uuid::Uuid::new_v4().to_string();
        let status = if input.answers.is_empty() {
            "waiting_for_user"
        } else {
            "answered"
        };
        let record = json!({
            "id": question_id,
            "status": status,
            "questions": input.questions,
            "answers": input.answers,
            "annotations": input.annotations,
            "metadata": input.metadata,
            "created_at": now_unix_seconds()
        });
        push_app_state_record(context, USER_QUESTIONS_KEY, record.clone())?;

        Ok(ToolOutput {
            data: json!({
                "status": status,
                "question_id": record["id"],
                "questions": record["questions"],
                "answers": record["answers"],
                "message": if status == "waiting_for_user" {
                    "Question recorded for user response."
                } else {
                    "User answers recorded."
                }
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let status = output
            .data
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("waiting_for_user");
        let content = if status == "answered" {
            format!(
                "User answers: {}",
                output
                    .data
                    .get("answers")
                    .map(Value::to_string)
                    .unwrap_or_else(|| "{}".to_string())
            )
        } else {
            "Questions have been recorded for the user. Continue after answers are provided."
                .to_string()
        };
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content
        })
    }
}

#[async_trait]
impl Tool for SendUserMessageTool {
    fn name(&self) -> &str {
        "SendUserMessage"
    }

    fn description(&self) -> &str {
        "Send a visible message to the user and optionally attach files"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("send a message to the user")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "attachments": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "status": { "type": "string", "enum": ["normal", "proactive"] }
            },
            "required": ["message"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" },
                "attachments": { "type": "array" },
                "sent_at": { "type": "integer" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: SendUserMessageInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        if input.message.trim().is_empty() {
            return ValidationResult::err("message cannot be empty".to_string(), 2);
        }
        if !matches!(input.status.as_str(), "normal" | "proactive") {
            return ValidationResult::err(
                "status must be either normal or proactive".to_string(),
                3,
            );
        }
        for attachment in &input.attachments {
            let path = match context.resolve_access_path(attachment) {
                Ok(path) => path,
                Err(error) => return ValidationResult::err(error, 5),
            };
            if !path.exists() {
                return ValidationResult::err(
                    format!("attachment does not exist: {}", path.display()),
                    4,
                );
            }
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: SendUserMessageInput = serde_json::from_value(input.clone())?;
        if input.message.trim().is_empty() {
            return Err(ToolError::ValidationError(
                "message cannot be empty".to_string(),
            ));
        }
        let attachments = resolve_attachments(context, &input.attachments)?;
        let sent_at = now_unix_seconds();
        let record = json!({
            "message": input.message,
            "attachments": attachments,
            "status": input.status,
            "sent_at": sent_at
        });
        push_app_state_record(context, USER_MESSAGES_KEY, record.clone())?;

        Ok(ToolOutput {
            data: record,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let attachment_count = output
            .data
            .get("attachments")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let suffix = if attachment_count == 0 {
            String::new()
        } else {
            format!(" ({attachment_count} attachment(s) included)")
        };
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format!("Message delivered to user.{suffix}")
        })
    }
}

#[async_trait]
impl Tool for SendUserFileTool {
    fn name(&self) -> &str {
        "send_user_file"
    }

    fn description(&self) -> &str {
        "Deliver one or more local files to the user"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("send files to the user")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "minItems": 1,
                    "items": { "type": "string" }
                },
                "file_path": { "type": "string" },
                "path": { "type": "string" },
                "message": { "type": "string" }
            },
            "required": ["files"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "files": { "type": "array" },
                "message": { "type": ["string", "null"] },
                "sent_at": { "type": "integer" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: SendUserFileInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        match validate_send_user_files(context, &input) {
            Ok(()) => ValidationResult::ok(),
            Err(message) => ValidationResult::err(message, 2),
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: SendUserFileInput = serde_json::from_value(input.clone())?;
        validate_send_user_files(context, &input).map_err(ToolError::ValidationError)?;
        let files = resolve_sendable_files(context, &input)?;
        let sent_at = now_unix_seconds();
        let record = json!({
            "files": files,
            "message": input.message,
            "sent_at": sent_at
        });
        push_app_state_record(context, USER_FILES_KEY, record.clone())?;

        Ok(ToolOutput {
            data: record,
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let files = output
            .data
            .get("files")
            .and_then(Value::as_array)
            .map(|files| {
                files
                    .iter()
                    .filter_map(|file| file.get("path").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": format!("Delivered {} file(s) to the user: {}", files.len(), files.join(", "))
        })
    }
}

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "Sleep"
    }

    fn description(&self) -> &str {
        "Wait for a specified duration without spawning a shell process"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("wait or pause for a duration")
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "duration_ms": {
                    "type": "integer",
                    "minimum": 0,
                    "maximum": 600000
                },
                "seconds": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": 600
                }
            }
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "slept_ms": { "type": "integer" },
                "interrupted": { "type": "boolean" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: SleepInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(error) => return ValidationResult::err(format!("Invalid input: {error}"), 1),
        };
        match sleep_duration_ms(&input) {
            Some(_) => ValidationResult::ok(),
            None => ValidationResult::err(
                "duration_ms or seconds must be provided and non-negative".to_string(),
                2,
            ),
        }
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: SleepInput = serde_json::from_value(input.clone())?;
        let duration_ms = sleep_duration_ms(&input).ok_or_else(|| {
            ToolError::ValidationError(
                "duration_ms or seconds must be provided and non-negative".to_string(),
            )
        })?;
        let mut abort_signal = context.abort_signal.clone();
        let sleep = tokio::time::sleep(std::time::Duration::from_millis(duration_ms));
        tokio::pin!(sleep);

        let interrupted = tokio::select! {
            _ = &mut sleep => false,
            changed = abort_signal.changed() => {
                changed.is_ok() && *abort_signal.borrow()
            }
        };

        Ok(ToolOutput {
            data: json!({
                "slept_ms": duration_ms,
                "interrupted": interrupted
            }),
            metadata: None,
        })
    }
}

fn validate_questions(questions: &[Question]) -> ValidationResult {
    if questions.is_empty() || questions.len() > 4 {
        return ValidationResult::err("questions must contain 1 to 4 items".to_string(), 2);
    }
    let mut seen_questions = std::collections::HashSet::new();
    for question in questions {
        if question.question.trim().is_empty() {
            return ValidationResult::err("question cannot be empty".to_string(), 3);
        }
        if question.header.trim().is_empty() || question.header.chars().count() > 12 {
            return ValidationResult::err(
                "header must be non-empty and at most 12 characters".to_string(),
                4,
            );
        }
        if !seen_questions.insert(question.question.trim().to_string()) {
            return ValidationResult::err("question texts must be unique".to_string(), 5);
        }
        if question.options.len() < 2 || question.options.len() > 4 {
            return ValidationResult::err(
                "each question must contain 2 to 4 options".to_string(),
                6,
            );
        }
        let mut seen_labels = std::collections::HashSet::new();
        for option in &question.options {
            if option.label.trim().is_empty() {
                return ValidationResult::err("option label cannot be empty".to_string(), 7);
            }
            if !seen_labels.insert(option.label.trim().to_string()) {
                return ValidationResult::err(
                    "option labels must be unique within each question".to_string(),
                    8,
                );
            }
        }
    }
    ValidationResult::ok()
}

fn default_message_status() -> String {
    "normal".to_string()
}

fn sleep_duration_ms(input: &SleepInput) -> Option<u64> {
    if let Some(ms) = input.duration_ms {
        return Some(ms.min(600_000));
    }
    let seconds = input.seconds?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(((seconds * 1000.0).round() as u64).min(600_000))
}

fn send_user_file_paths(input: &SendUserFileInput) -> Vec<String> {
    let mut files = input.files.clone();
    if let Some(file_path) = &input.file_path {
        files.push(file_path.clone());
    }
    files
}

fn validate_send_user_files(
    context: &ToolContext,
    input: &SendUserFileInput,
) -> Result<(), String> {
    let files = send_user_file_paths(input);
    if files.is_empty() {
        return Err("files, file_path, or path must include at least one file".to_string());
    }
    for file in files {
        if file.trim().is_empty() {
            return Err("file paths cannot be empty".to_string());
        }
        resolve_sendable_file(context, &file).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn resolve_sendable_files(
    context: &ToolContext,
    input: &SendUserFileInput,
) -> ToolResult<Vec<Value>> {
    send_user_file_paths(input)
        .iter()
        .map(|file| {
            let path = resolve_sendable_file(context, file)?;
            let metadata = std::fs::metadata(&path)?;
            Ok(json!({
                "path": path.to_string_lossy(),
                "size": metadata.len(),
                "is_image": is_image_path(&path),
                "mime_type": mime_type_for_path(&path)
            }))
        })
        .collect()
}

fn resolve_sendable_file(context: &ToolContext, path: &str) -> ToolResult<PathBuf> {
    let path = context
        .resolve_access_path(path)
        .map_err(ToolError::ValidationError)?;
    let path = std::fs::canonicalize(path)?;
    if context.access_roots().is_none() {
        let cwd = std::fs::canonicalize(&context.cwd)?;
        if !path.starts_with(&cwd) {
            return Err(ToolError::ValidationError(format!(
                "file is outside the current working directory: {}",
                path.display()
            )));
        }
    }
    if !path.is_file() {
        return Err(ToolError::ValidationError(format!(
            "path is not a file: {}",
            path.display()
        )));
    }
    Ok(path)
}

fn push_app_state_record(context: &mut ToolContext, key: &str, record: Value) -> ToolResult<()> {
    let mut records = context
        .app_state
        .get(key)
        .cloned()
        .unwrap_or_else(|| json!([]));
    let Some(records_array) = records.as_array_mut() else {
        return Err(ToolError::Other(format!("{key} state is not an array")));
    };
    records_array.push(record);
    context.app_state.insert(key.to_string(), records);
    Ok(())
}

fn resolve_attachments(context: &ToolContext, attachments: &[String]) -> ToolResult<Vec<Value>> {
    attachments
        .iter()
        .map(|attachment| {
            let path = context
                .resolve_access_path(attachment)
                .map_err(ToolError::ValidationError)?;
            let metadata = std::fs::metadata(&path)?;
            Ok(json!({
                "path": path.to_string_lossy(),
                "size": metadata.len(),
                "is_image": is_image_path(&path)
            }))
        })
        .collect()
}

fn is_image_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

fn mime_type_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("json") => "application/json",
        Some("md") => "text/markdown",
        Some("txt" | "log" | "csv" | "tsv") => "text/plain",
        Some("html" | "htm") => "text/html",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{AskUserQuestionTool, SendUserFileTool, SendUserMessageTool, SleepTool};
    use crate::{Tool, ToolContext};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use uuid::Uuid;

    fn test_context(cwd: String) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd,
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal: abort_rx,
        }
    }

    #[tokio::test]
    async fn ask_user_question_records_pending_and_answered_questions() {
        let mut context = test_context(".".to_string());
        let tool = AskUserQuestionTool::new();
        let input = json!({
            "questions": [{
                "question": "Which approach should we use?",
                "header": "Approach",
                "options": [
                    { "label": "Fast", "description": "Move quickly" },
                    { "label": "Safe", "description": "Add more checks" }
                ]
            }]
        });

        let validation = tool.validate_input(&input, &context).await;
        assert!(validation.result, "{:?}", validation.message);
        let pending = tool.call(&input, &mut context).await.unwrap();
        assert_eq!(pending.data["status"], "waiting_for_user");
        assert_eq!(
            context.app_state["user_questions"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        let answered = tool
            .call(
                &json!({
                    "questions": input["questions"],
                    "answers": { "Which approach should we use?": "Safe" }
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(answered.data["status"], "answered");
    }

    #[tokio::test]
    async fn send_user_message_records_attachment_metadata() {
        let root = std::env::temp_dir().join(format!("kiana-user-msg-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("note.txt"), "hello").unwrap();
        let mut context = test_context(root.to_string_lossy().to_string());

        let output = SendUserMessageTool::new()
            .call(
                &json!({
                    "message": "Done",
                    "attachments": ["note.txt"],
                    "status": "normal"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["message"], "Done");
        assert_eq!(output.data["attachments"][0]["size"], 5);
        assert_eq!(
            context.app_state["user_messages"].as_array().unwrap().len(),
            1
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn send_user_file_records_file_metadata() {
        let root = std::env::temp_dir().join(format!("kiana-user-file-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("report.md"), "# Report").unwrap();
        let mut context = test_context(root.to_string_lossy().to_string());

        let output = SendUserFileTool::new()
            .call(
                &json!({
                    "files": ["report.md"],
                    "message": "Final report"
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["message"], "Final report");
        assert_eq!(output.data["files"][0]["size"], 8);
        assert_eq!(output.data["files"][0]["mime_type"], "text/markdown");
        assert_eq!(context.app_state["user_files"].as_array().unwrap().len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn send_user_file_rejects_file_outside_cwd() {
        let root = std::env::temp_dir().join(format!("kiana-user-file-root-{}", Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("kiana-user-file-outside-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        fs::write(&outside, "secret").unwrap();
        let context = test_context(root.to_string_lossy().to_string());

        let validation = SendUserFileTool::new()
            .validate_input(&json!({ "file_path": outside.to_string_lossy() }), &context)
            .await;

        assert!(!validation.result);
        assert!(validation.message.unwrap().contains("outside"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_file(outside);
    }

    #[tokio::test]
    async fn sleep_waits_without_shelling_out() {
        let mut context = test_context(".".to_string());
        let output = SleepTool::new()
            .call(&json!({ "duration_ms": 1 }), &mut context)
            .await
            .unwrap();

        assert_eq!(output.data["slept_ms"], 1);
        assert_eq!(output.data["interrupted"], false);

        let api_result = SleepTool::new().map_to_api_result(&output, "toolu_sleep");
        assert_eq!(api_result["type"], "tool_result");
        assert_eq!(api_result["tool_use_id"], "toolu_sleep");
        let content = api_result["content"].as_str().unwrap();
        assert!(content.contains("\"slept_ms\": 1"));
        assert!(content.contains("\"interrupted\": false"));
    }
}
