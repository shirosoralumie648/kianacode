//! Shared model values. Provider wire content is compiled before admission.
use crate::{RequestId, RunId, TokenBudget, TurnId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 模型消息的角色分类。
pub enum ModelRole {
    /// 系统提示或受控岗位说明。
    System,
    /// 用户输入、预算提醒或 steering 文本。
    User,
    /// 模型自然语言和工具调用声明。
    Assistant,
    /// Broker 返回的工具结果。
    Tool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 发给模型的单条消息。
pub struct ModelMessage {
    /// 消息角色。
    pub role: ModelRole,
    /// 文本内容；工具消息通常是结构化结果的 JSON 文本。
    pub text: String,
    /// 当角色为 Tool 时关联的工具调用 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 当角色为 Assistant 时声明的工具调用列表。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ModelToolCall>,
}

impl ModelMessage {
    /// 构造系统消息。
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::System,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// 构造用户消息。
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// 构造不含工具调用的 assistant 消息。
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
        }
    }

    /// 构造携带工具调用声明的 assistant 消息。
    pub fn assistant_with_tools(text: impl Into<String>, tool_calls: Vec<ModelToolCall>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls,
        }
    }

    /// 构造与某个工具调用 ID 关联的工具结果消息。
    pub fn tool(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Tool,
            text: text.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 模型请求的单个工具调用声明。
pub struct ModelToolCall {
    /// 模型生成的调用 ID；Runner 用它把结果对应回声明。
    pub id: String,
    /// 模型可见的工具名，后续由工具目录映射为能力请求。
    pub name: String,
    /// 未执行的原始 JSON 参数；不能直接当作授权。
    pub arguments: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// 一次模型补全请求。
pub struct ModelRequest {
    /// 当前上下文消息。
    pub messages: Vec<ModelMessage>,
    /// 当前允许模型看到的工具 schema。
    pub tools: Vec<Value>,
    /// sandbox 展示值；真实边界由 Broker/执行器再次校验。
    pub sandbox: String,
}

/// Effective provider request context used by accounting and trace projection.
/// Text is submitted to the provider; callers persist only its fingerprint and sources.
#[derive(Clone, Debug)]
pub struct ModelRequestContext {
    pub system_prompt: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub context_window: u64,
    pub reserved_output_tokens: u64,
    pub message_bytes: usize,
    pub tool_schema_bytes: usize,
    pub sources: Vec<String>,
}
impl ModelRequestContext {
    pub fn for_request(request: &ModelRequest) -> Self {
        let messages = request
            .messages
            .iter()
            .filter(|m| m.role != ModelRole::System)
            .collect::<Vec<_>>();
        Self {
            system_prompt: request
                .messages
                .iter()
                .filter(|m| m.role == ModelRole::System)
                .map(|m| {
                    crate::PromptBundle::decode(&m.text)
                        .map(|b| b.system_prompt())
                        .unwrap_or_else(|_| m.text.clone())
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
            provider_id: None,
            model_id: None,
            context_window: 128_000,
            reserved_output_tokens: 4096,
            message_bytes: serde_json::to_vec(&messages)
                .map(|v| v.len())
                .unwrap_or(usize::MAX),
            tool_schema_bytes: serde_json::to_vec(&request.tools)
                .map(|v| v.len())
                .unwrap_or(usize::MAX),
            sources: Vec::new(),
        }
    }
    pub fn budget(&self) -> crate::TokenBudget {
        crate::TokenBudget::new(
            self.message_bytes,
            self.system_prompt.len(),
            self.tool_schema_bytes,
            self.reserved_output_tokens,
            self.context_window,
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
/// 模型返回的文本和工具调用声明。
pub struct ModelOutput {
    /// 自然语言文本。
    #[serde(default)]
    pub text: String,
    /// 需要外层转成能力请求的工具调用。
    #[serde(default)]
    pub tool_calls: Vec<ModelToolCall>,
    /// provider 报告的 token 用量；旧 cassette 或缺省响应为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ModelUsage>,
    /// provider 报告的停止原因。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    /// provider 实际使用的模型 ID。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
/// 单次模型响应报告的 token 用量。
///
/// 这是 provider 回执的记录，不是计费证明；缺失或为零时不能推断实际费用。
pub struct ModelUsage {
    /// 输入 token 数。
    #[serde(default)]
    pub input_tokens: u64,
    /// 输出 token 数。
    #[serde(default)]
    pub output_tokens: u64,
}

impl ModelOutput {
    /// 构造纯文本响应。
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tool_calls: Vec::new(),
            usage: None,
            stop_reason: None,
            model_id: None,
        }
    }

    /// 构造带一个固定测试调用 ID 的工具响应。
    pub fn with_tool(text: impl Into<String>, name: impl Into<String>, arguments: Value) -> Self {
        Self {
            text: text.into(),
            tool_calls: vec![ModelToolCall {
                id: "call-1".to_owned(),
                name: name.into(),
                arguments,
            }],
            usage: None,
            stop_reason: None,
            model_id: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
/// 模型轮次中逐步交付的增量。
///
/// 当前只定义文本增量；后续可新增工具调用参数、usage、stop_reason 等变体，调用方应保留
/// 通配分支。无论交付多少增量，`complete_streaming` 返回的 [`ModelOutput`] 始终是该轮次的
/// 完整聚合结果。
pub enum ModelDelta {
    /// 一段自然语言文本增量。
    Text {
        /// 本次新增的文本。
        text: String,
    },
}

pub const MODEL_CALL_SCHEMA: &str = "kiana.model-call.v1";

/// Installed by ControlPlane, never decoded from model or project text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelAssignment {
    pub schema: String,
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub role_id: String,
    pub profile: String,
    pub project_root: String,
    pub project_trusted: bool,
    pub authority_revision: Option<String>,
    pub max_wall_time_ms: u64,
}
impl ModelAssignment {
    pub fn validate(&self) -> Result<(), ModelError> {
        let role = crate::RoleSpec::lookup(&self.role_id)
            .ok_or_else(|| ModelError::invalid("model_role_unknown"))?;
        if self.schema != "kiana.model-assignment.v1"
            || self.profile != role.model_profile
            || !self.project_trusted
            || self.project_root.trim().is_empty()
            || self.max_wall_time_ms == 0
        {
            return Err(ModelError::invalid("model_assignment_invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelPurpose {
    Task,
    Compaction,
    OutputRepair,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProtocol {
    Legacy,
    AnthropicMessages,
    OpenAiChat,
    OpenAiResponses,
    OllamaChat,
    GeminiInteractions,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCapabilities {
    pub tools: CapabilitySupport,
    pub streaming: CapabilitySupport,
    pub structured_output: CapabilitySupport,
    pub images: CapabilitySupport,
    pub reasoning_replay: CapabilitySupport,
    pub context_window: u64,
    pub max_output: u64,
    pub source: String,
    pub revision: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRoute {
    pub provider_id: String,
    pub protocol: ModelProtocol,
    pub connection_id: String,
    pub model_id: String,
    pub profile: String,
    pub configuration_revision: String,
    pub streaming: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelResponseFormat {
    Text,
    JsonObject,
    JsonSchema { name: String, schema: Value },
}
impl Default for ModelResponseFormat {
    fn default() -> Self {
        Self::Text
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedReplayRef {
    pub artifact_id: String,
    pub sha256: String,
    pub route_digest: String,
    pub source_call_id: RequestId,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCallSpec {
    pub call_id: RequestId,
    pub attempt_id: RequestId,
    pub step: u32,
    pub purpose: ModelPurpose,
    pub assignment: Option<ModelAssignment>,
    pub response_format: ModelResponseFormat,
    pub replay: Vec<ProtectedReplayRef>,
    pub deadline_unix_ms: u64,
}

/// Contains private wire data. Persist only audit(), never this object or its Debug form.
#[derive(Clone)]
pub struct PreparedModelCall {
    pub schema: String,
    pub spec: ModelCallSpec,
    pub route: ModelRoute,
    pub request: ModelRequest,
    pub wire_body: Value,
    pub request_hash: String,
    pub budget: TokenBudget,
    pub tool_catalog_hash: String,
}
impl std::fmt::Debug for PreparedModelCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedModelCall")
            .field("audit", &self.audit())
            .finish()
    }
}
impl PreparedModelCall {
    pub fn fingerprint(&self) -> String {
        crate::json_digest(
            &json!({"schema":self.schema,"spec":self.spec,"route":self.route,
            "wire":self.wire_body,"request":self.request,"budget":self.budget,"tool_catalog_hash":self.tool_catalog_hash}),
        )
    }
    pub fn seal(&mut self) {
        self.request_hash = self.fingerprint();
    }
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != MODEL_CALL_SCHEMA || self.request_hash != self.fingerprint() {
            return Err(ModelError::invalid("model_prepared_request_changed"));
        }
        self.budget.validate().map_err(ModelError::invalid)?;
        validate_model_history(&self.request.messages)?;
        if let Some(assignment) = &self.spec.assignment {
            assignment.validate()?;
        }
        if self.spec.purpose == ModelPurpose::Compaction && !self.request.tools.is_empty() {
            return Err(ModelError::invalid("compaction_tools_denied"));
        }
        Ok(())
    }
    pub fn audit(&self) -> Value {
        json!({"schema":self.schema,"model_call_id":self.spec.call_id,"model_request_id":self.spec.attempt_id,
            "run_id":self.spec.assignment.as_ref().map(|a|a.run_id),"step":self.spec.step,
            "purpose":self.spec.purpose,"route":self.route,"request_hash":self.request_hash,
            "tool_catalog_hash":self.tool_catalog_hash,"budget":self.budget,
            "deadline_unix_ms":self.spec.deadline_unix_ms})
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelCallPermit {
    pub schema: String,
    pub permit_id: RequestId,
    pub run_id: RunId,
    pub attempt_id: RequestId,
    pub request_hash: String,
    pub expires_at_unix_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFinish {
    EndTurn,
    ToolUse,
    Length,
    Refusal,
    Pause,
    Incomplete,
}
impl ModelFinish {
    pub fn parse(reason: Option<&str>, tools: bool, legacy: bool) -> Result<Self, ModelError> {
        let finish = match reason {
            Some("stop" | "end_turn" | "completed") => Self::EndTurn,
            Some("tool_use" | "tool_calls" | "requires_action") => Self::ToolUse,
            Some("length" | "max_tokens" | "MAX_TOKENS" | "incomplete") => Self::Length,
            Some("refusal" | "content_filter" | "SAFETY") => Self::Refusal,
            Some("pause_turn") => Self::Pause,
            None if legacy => {
                if tools {
                    Self::ToolUse
                } else {
                    Self::EndTurn
                }
            }
            _ => return Err(ModelError::invalid("model_stop_reason_unknown")),
        };
        if (finish == Self::EndTurn && tools) || (finish == Self::ToolUse && !tools) {
            return Err(ModelError::invalid("model_stop_content_mismatch"));
        }
        Ok(finish)
    }
    pub fn require_complete(self) -> Result<(), ModelError> {
        match self {
            Self::EndTurn | Self::ToolUse => Ok(()),
            Self::Length => Err(ModelError::invalid("model_output_truncated")),
            Self::Refusal => Err(ModelError::invalid("model_refused")),
            Self::Pause => Err(ModelError::invalid(
                "model_pause_requires_explicit_continue",
            )),
            Self::Incomplete => Err(ModelError::invalid("model_transport_incomplete")),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRetryClass {
    Never,
    BeforeSend,
    Rejected,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelError {
    pub code: String,
    pub phase: String,
    pub retry_class: ModelRetryClass,
    pub request_sent: bool,
    pub retry_after_ms: Option<u64>,
    pub safe_message: String,
}
impl ModelError {
    pub fn invalid(code: impl Into<String>) -> Self {
        let code = code.into();
        Self {
            safe_message: crate::redact_text(&code),
            code,
            phase: "validation".to_owned(),
            retry_class: ModelRetryClass::Never,
            request_sent: false,
            retry_after_ms: None,
        }
    }
    pub fn transport(code: &str, retry_class: ModelRetryClass, request_sent: bool) -> Self {
        Self {
            code: code.to_owned(),
            phase: "transport".to_owned(),
            retry_class,
            request_sent,
            retry_after_ms: None,
            safe_message: code.to_owned(),
        }
    }
}
impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}",
            self.code,
            crate::redact_text(&self.safe_message)
        )
    }
}
impl std::error::Error for ModelError {}

#[derive(Clone, Debug)]
pub struct ModelReply {
    pub output: ModelOutput,
    pub finish: ModelFinish,
    pub structured: Option<Value>,
    pub replay: Vec<ProtectedReplayRef>,
    pub provider_request_id: Option<String>,
    pub provider_response_id: Option<String>,
}
impl ModelReply {
    pub fn legacy(output: ModelOutput) -> Result<Self, ModelError> {
        validate_model_calls(&output.tool_calls)?;
        let finish = ModelFinish::parse(
            output.stop_reason.as_deref(),
            !output.tool_calls.is_empty(),
            true,
        )?;
        finish.require_complete()?;
        Ok(Self {
            output,
            finish,
            structured: None,
            replay: Vec::new(),
            provider_request_id: None,
            provider_response_id: None,
        })
    }
}

pub fn validate_model_calls(calls: &[ModelToolCall]) -> Result<(), ModelError> {
    if calls.len() > 32 {
        return Err(ModelError::invalid("model_tool_batch_too_large"));
    }
    let mut ids = std::collections::HashSet::new();
    for call in calls {
        if call.id.trim().is_empty()
            || call.id.len() > 256
            || call.name.trim().is_empty()
            || !ids.insert(&call.id)
        {
            return Err(ModelError::invalid("model_tool_identity_invalid"));
        }
        if !call.arguments.is_object()
            || serde_json::to_vec(&call.arguments).map_or(true, |v| v.len() > 256 * 1024)
        {
            return Err(ModelError::invalid("model_tool_arguments_invalid"));
        }
    }
    Ok(())
}

/// Every assistant batch is complete before another user/assistant message is admitted.
pub fn validate_model_history(messages: &[ModelMessage]) -> Result<(), ModelError> {
    let mut pending = std::collections::HashSet::new();
    for message in messages {
        match message.role {
            ModelRole::System => {
                if !pending.is_empty() {
                    return Err(ModelError::invalid("model_history_incomplete_batch"));
                }
            }
            ModelRole::Assistant => {
                if !pending.is_empty() {
                    return Err(ModelError::invalid("model_history_incomplete_batch"));
                }
                validate_model_calls(&message.tool_calls)?;
                pending.extend(message.tool_calls.iter().map(|call| call.id.clone()));
            }
            ModelRole::Tool => {
                if !message.tool_calls.is_empty()
                    || !message
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|id| pending.remove(id))
                {
                    return Err(ModelError::invalid("model_history_orphan_tool_result"));
                }
            }
            ModelRole::User => {
                if !pending.is_empty() {
                    return Err(ModelError::invalid("model_history_incomplete_batch"));
                }
            }
        }
    }
    if !pending.is_empty() {
        return Err(ModelError::invalid("model_history_incomplete_batch"));
    }
    Ok(())
}
