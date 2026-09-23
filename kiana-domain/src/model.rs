//! Shared model values. Provider wire content is compiled before admission.
use crate::{
    ModelAttemptId, RequestId, RunId, RuntimeBudget, SchemaVersion, StepId, TokenBudget, TurnId,
};
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

pub const MODEL_CONTENT_SCHEMA: &str = "kiana.model-content.v1";
pub const PROVIDER_CONTINUATION_SCHEMA: &str = "kiana.provider-continuation.v1";
pub const MODEL_OUTCOME_SCHEMA: &str = "kiana.model-outcome.v1";

/// Structured message content. Legacy `ModelMessage.text/tool_calls` remains the wire-compatible
/// fallback; new callers can use these blocks without flattening provider items into text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelContent {
    Text {
        text: String,
    },
    ToolCall {
        call: ModelToolCall,
    },
    ToolResult {
        call_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
    AttachmentRef {
        artifact_ref: String,
        media_type: String,
        digest: String,
    },
    ProviderOpaque {
        provider_id: String,
        protocol: ModelProtocol,
        route_digest: String,
        item_ref: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderContinuation {
    pub schema: String,
    pub provider_id: String,
    pub protocol: ModelProtocol,
    pub route_digest: String,
    pub item_ref: String,
}

impl ProviderContinuation {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != PROVIDER_CONTINUATION_SCHEMA
            || self.provider_id.trim().is_empty()
            || self.item_ref.trim().is_empty()
            || self.route_digest.len() != 71
            || !self.route_digest.starts_with("sha256:")
            || !self.route_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(ModelError::invalid("provider_continuation_invalid"));
        }
        Ok(())
    }
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
    /// Structured content; empty means use the legacy text/tool_calls representation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ModelContent>,
    /// Provider-specific continuation metadata, always reference-only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ProviderContinuation>,
}

impl ModelMessage {
    /// 构造系统消息。
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::System,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            content: Vec::new(),
            continuation: None,
        }
    }

    /// 构造用户消息。
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::User,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            content: Vec::new(),
            continuation: None,
        }
    }

    /// 构造不含工具调用的 assistant 消息。
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls: Vec::new(),
            content: Vec::new(),
            continuation: None,
        }
    }

    /// 构造携带工具调用声明的 assistant 消息。
    pub fn assistant_with_tools(text: impl Into<String>, tool_calls: Vec<ModelToolCall>) -> Self {
        Self {
            role: ModelRole::Assistant,
            text: text.into(),
            tool_call_id: None,
            tool_calls,
            content: Vec::new(),
            continuation: None,
        }
    }

    /// 构造与某个工具调用 ID 关联的工具结果消息。
    pub fn tool(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: ModelRole::Tool,
            text: text.into(),
            tool_call_id: Some(tool_call_id.into()),
            tool_calls: Vec::new(),
            content: Vec::new(),
            continuation: None,
        }
    }

    pub fn with_content(role: ModelRole, content: Vec<ModelContent>) -> Result<Self, ModelError> {
        let tool_call_id = content.iter().find_map(|block| match block {
            ModelContent::ToolResult { call_id, .. } => Some(call_id.clone()),
            _ => None,
        });
        let message = Self {
            role,
            text: String::new(),
            tool_call_id,
            tool_calls: Vec::new(),
            content,
            continuation: None,
        };
        message.validate_content()?;
        Ok(message)
    }

    pub fn content_blocks(&self) -> Result<Vec<ModelContent>, ModelError> {
        if self.content.is_empty() {
            let mut blocks = Vec::new();
            if !self.text.is_empty() {
                blocks.push(ModelContent::Text {
                    text: self.text.clone(),
                });
            }
            blocks.extend(
                self.tool_calls
                    .iter()
                    .cloned()
                    .map(|call| ModelContent::ToolCall { call }),
            );
            if self.role == ModelRole::Tool {
                let call_id = self
                    .tool_call_id
                    .clone()
                    .ok_or_else(|| ModelError::invalid("model_history_orphan_tool_result"))?;
                blocks.push(ModelContent::ToolResult {
                    call_id,
                    content: self.text.clone(),
                    is_error: false,
                });
            }
            if blocks.is_empty() && self.role == ModelRole::Assistant {
                blocks.push(ModelContent::Text {
                    text: String::new(),
                });
            }
            return Ok(blocks);
        }
        self.validate_content()?;
        Ok(self.content.clone())
    }

    pub fn validate_content(&self) -> Result<(), ModelError> {
        if let Some(continuation) = &self.continuation {
            continuation.validate()?;
        }
        if !self.content.is_empty()
            && (!self.text.is_empty()
                || !self.tool_calls.is_empty()
                || (self.role != ModelRole::Tool && self.tool_call_id.is_some()))
        {
            return Err(ModelError::invalid("model_content_legacy_conflict"));
        }
        let blocks = if self.content.is_empty() {
            return Ok(());
        } else {
            &self.content
        };
        let mut tool_calls = 0usize;
        let mut tool_results = 0usize;
        let mut tool_result_id = None;
        for block in blocks {
            match block {
                ModelContent::Text { text } => {
                    if text.len() > 1024 * 1024 || text.contains('\0') {
                        return Err(ModelError::invalid("model_content_text_invalid"));
                    }
                }
                ModelContent::ToolCall { call } => {
                    tool_calls += 1;
                    validate_model_calls(std::slice::from_ref(call))?
                }
                ModelContent::ToolResult {
                    call_id, content, ..
                } => {
                    tool_results += 1;
                    tool_result_id = Some(call_id.as_str());
                    if call_id.trim().is_empty() || call_id.len() > 256 || content.contains('\0') {
                        return Err(ModelError::invalid("model_content_tool_result_invalid"));
                    }
                }
                ModelContent::AttachmentRef {
                    artifact_ref,
                    media_type,
                    digest,
                } => {
                    if !artifact_ref.starts_with("artifact:")
                        || artifact_ref.len() > 512
                        || media_type.trim().is_empty()
                        || media_type.len() > 128
                        || !digest.starts_with("sha256:")
                        || digest.len() != 71
                        || !digest[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
                    {
                        return Err(ModelError::invalid("model_content_attachment_invalid"));
                    }
                }
                ModelContent::ProviderOpaque {
                    provider_id,
                    route_digest,
                    item_ref,
                    ..
                } => {
                    if provider_id.trim().is_empty()
                        || route_digest.len() != 71
                        || !route_digest.starts_with("sha256:")
                        || !route_digest[7..]
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit())
                        || !item_ref.starts_with("artifact:")
                    {
                        return Err(ModelError::invalid("model_content_opaque_invalid"));
                    }
                }
            }
        }
        match self.role {
            ModelRole::Tool
                if tool_results != 1
                    || tool_calls != 0
                    || self.tool_call_id.as_deref() != tool_result_id =>
            {
                return Err(ModelError::invalid("model_content_tool_role_invalid"));
            }
            ModelRole::Tool => {}
            _ if tool_results > 0 => {
                return Err(ModelError::invalid(
                    "model_content_tool_result_role_invalid",
                ));
            }
            _ => {}
        }
        Ok(())
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
    /// Structured response blocks; empty means use legacy text/tool_calls fields.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<ModelContent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<ProviderContinuation>,
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
            content: Vec::new(),
            continuation: None,
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
            content: Vec::new(),
            continuation: None,
        }
    }

    pub fn content_blocks(&self) -> Result<Vec<ModelContent>, ModelError> {
        if !self.content.is_empty() {
            let message = ModelMessage {
                role: ModelRole::Assistant,
                text: String::new(),
                tool_call_id: None,
                tool_calls: Vec::new(),
                content: self.content.clone(),
                continuation: self.continuation.clone(),
            };
            message.validate_content()?;
            for block in &self.content {
                match block {
                    ModelContent::ToolResult { .. } => {
                        return Err(ModelError::invalid("model_output_tool_result_invalid"))
                    }
                    _ => {}
                }
            }
            return Ok(self.content.clone());
        }
        let mut blocks = Vec::new();
        if !self.text.is_empty() {
            blocks.push(ModelContent::Text {
                text: self.text.clone(),
            });
        }
        blocks.extend(
            self.tool_calls
                .iter()
                .cloned()
                .map(|call| ModelContent::ToolCall { call }),
        );
        Ok(blocks)
    }

    /// Normalize provider stop text to the closed model stop vocabulary. Unknown or missing
    /// values remain `Unknown` and therefore cannot be treated as a completed turn.
    pub fn normalized_stop_reason(&self) -> ModelStopReason {
        ModelFinish::parse(
            self.stop_reason.as_deref(),
            !self.tool_calls.is_empty()
                || self
                    .content
                    .iter()
                    .any(|block| matches!(block, ModelContent::ToolCall { .. })),
            false,
        )
        .map(ModelStopReason::from)
        .unwrap_or(ModelStopReason::Unknown)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
/// 模型轮次中逐步交付的增量。
///
/// 增量可携带文本、工具参数、累计 usage 或显式 stop；调用方必须保留通配分支，以便未知
/// provider item 失败关闭。无论交付多少增量，`complete_streaming` 返回的 [`ModelOutput`]
/// 始终是该轮次的完整聚合结果。
pub enum ModelDelta {
    /// 一段自然语言文本增量。
    Text {
        /// 本次新增的文本。
        text: String,
    },
    /// A fragment of one tool's JSON arguments. The index/identity is server/provider supplied;
    /// fragments are buffered and never become a tool request before final validation.
    ToolArguments {
        index: u32,
        id: String,
        name: String,
        partial_json: String,
    },
    /// Cumulative usage observed while a provider stream is open.
    Usage { usage: ModelUsage },
    /// Explicit provider stop marker. A duplicate or conflicting marker is invalid.
    Stop { reason: String },
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
    #[serde(default)]
    pub role_version: Option<SchemaVersion>,
    #[serde(default)]
    pub catalog_version: Option<SchemaVersion>,
    #[serde(default)]
    pub prompt_hash: Option<String>,
    #[serde(default)]
    pub input_schema: Option<String>,
    #[serde(default)]
    pub output_schema: Option<String>,
    pub profile: String,
    pub project_root: String,
    pub project_trusted: bool,
    pub authority_revision: Option<String>,
    pub max_wall_time_ms: u64,
    /// Authority-selected runtime ceiling. Legacy assignments may omit this field; when present
    /// the Harness intersects it with deployment and role/task limits before admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_budget: Option<RuntimeBudget>,
}
impl ModelAssignment {
    pub fn validate(&self) -> Result<(), ModelError> {
        let role = crate::RoleSpec::lookup(&self.role_id)
            .ok_or_else(|| ModelError::invalid("model_role_unknown"))?;
        if self.schema != "kiana.model-assignment.v1"
            || self.profile != role.model_profile
            || self.role_version != Some(role.version)
            || self.catalog_version != Some(crate::SchemaVersion::new(1, 0))
            || self.prompt_hash.as_deref() != Some(role.prompt_hash.as_str())
            || self.input_schema.as_deref() != Some(role.input_schema.as_str())
            || self.output_schema.as_deref() != Some(role.output_schema.as_str())
            || !self.project_trusted
            || self.project_root.trim().is_empty()
            || self.max_wall_time_ms == 0
            || self
                .runtime_budget
                .as_ref()
                .is_some_and(|budget| budget.validate().is_err())
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

impl ModelRoute {
    /// Canonical digest used when a model route is copied into an admission permit or receipt.
    /// Keep this identity independent from wire request bytes and authentication material.
    pub fn digest(&self) -> String {
        crate::json_digest(&json!({
            "provider_id": self.provider_id,
            "protocol": self.protocol,
            "model_id": self.model_id,
            "profile": self.profile,
            "configuration_revision": self.configuration_revision,
            "streaming": self.streaming,
        }))
    }
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
    #[serde(default)]
    pub model_attempt_id: Option<ModelAttemptId>,
    #[serde(default)]
    pub step_id: Option<StepId>,
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
    /// Opaque provider binding copied from the trusted connection; raw credentials never enter
    /// this object.
    pub provider_account: Option<String>,
    /// Digest of the credential revision observed when the connection was resolved.
    pub credential_revision: Option<String>,
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
            "wire":self.wire_body,"request":self.request,"budget":self.budget,"tool_catalog_hash":self.tool_catalog_hash,
            "provider_account":self.provider_account,"credential_revision":self.credential_revision}),
        )
    }
    pub fn seal(&mut self) {
        self.request_hash = self.fingerprint();
    }
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != MODEL_CALL_SCHEMA
            || self.tool_catalog_hash != crate::tool_catalog_hash(&self.request.tools)
            || self.request_hash != self.fingerprint()
        {
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
        // This is the only model-request summary that may enter a RuntimeEvent.  Keep the
        // route fields bounded and add hashes for route/prompt identity; never include the
        // compiled wire body, messages, tools, headers or provider response here.
        json!({"schema":self.schema,"model_call_id":self.spec.call_id,"model_request_id":self.spec.attempt_id,
            "model_attempt_id":self.spec.model_attempt_id,"step_id":self.spec.step_id,
            "run_id":self.spec.assignment.as_ref().map(|a|a.run_id),"step":self.spec.step,
            "purpose":self.spec.purpose,"route":self.route,"route_digest":self.route.digest(),
            "prompt_version":self.request_hash,"request_hash":self.request_hash,
            "tool_catalog_hash":self.tool_catalog_hash,"budget":self.budget,
            "streaming":self.route.streaming,"deadline_unix_ms":self.spec.deadline_unix_ms,
            "provider_account":self.provider_account,"credential_revision":self.credential_revision})
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
    /// Optional during migration; the provider path requires these fields before network effect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_account: Option<String>,
}

impl ModelCallPermit {
    /// Validate the immutable route/authority/credential binding copied from a prepared call.
    /// Legacy adapters may omit the optional fields, but a networked provider must require them
    /// at its own effect boundary.
    pub fn validate_for_prepared(
        &self,
        prepared: &PreparedModelCall,
        now_unix_ms: u64,
    ) -> Result<(), ModelError> {
        if self.schema != "kiana.model-call-permit.v1"
            || self.request_hash != prepared.request_hash
            || self.attempt_id != prepared.spec.attempt_id
            || prepared
                .spec
                .assignment
                .as_ref()
                .is_some_and(|assignment| assignment.run_id != self.run_id)
            || now_unix_ms >= self.expires_at_unix_ms
        {
            return Err(ModelError::invalid("model_permit_scope_or_expiry_mismatch"));
        }
        let expected_route_digest = prepared.route.digest();
        if self.route_digest.as_deref() != Some(expected_route_digest.as_str())
            || self.configuration_revision.as_deref()
                != Some(prepared.route.configuration_revision.as_str())
            || self.credential_revision != prepared.credential_revision
            || self.provider_account != prepared.provider_account
        {
            return Err(ModelError::invalid("model_route_admission_drift"));
        }
        let expected_authority = prepared
            .spec
            .assignment
            .as_ref()
            .and_then(|assignment| assignment.authority_revision.as_ref());
        if self.authority_revision.as_ref() != expected_authority {
            return Err(ModelError::invalid("model_authority_revision_drift"));
        }
        if self.route_digest.is_none()
            || self.configuration_revision.is_none()
            || self.credential_revision.is_none()
            || self.provider_account.is_none()
        {
            return Err(ModelError::invalid("model_route_admission_missing"));
        }
        Ok(())
    }
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStopReason {
    EndTurn,
    ToolUse,
    Length,
    Refusal,
    Pause,
    Incomplete,
    #[default]
    Unknown,
}

impl From<ModelFinish> for ModelStopReason {
    fn from(finish: ModelFinish) -> Self {
        match finish {
            ModelFinish::EndTurn => Self::EndTurn,
            ModelFinish::ToolUse => Self::ToolUse,
            ModelFinish::Length => Self::Length,
            ModelFinish::Refusal => Self::Refusal,
            ModelFinish::Pause => Self::Pause,
            ModelFinish::Incomplete => Self::Incomplete,
        }
    }
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
    #[serde(default, skip_serializing_if = "ModelSideEffectState::is_none")]
    pub side_effect_state: ModelSideEffectState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSideEffectState {
    #[default]
    None,
    Unknown,
}

impl ModelSideEffectState {
    fn is_none(&self) -> bool {
        *self == Self::None
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelOutcome {
    pub schema: String,
    pub stop_reason: ModelStopReason,
    pub phase: String,
    pub retry_class: ModelRetryClass,
    pub request_sent: bool,
    pub side_effect_state: ModelSideEffectState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    pub safe_message: String,
}

impl ModelOutcome {
    pub fn validate(&self) -> Result<(), ModelError> {
        if self.schema != MODEL_OUTCOME_SCHEMA
            || self.phase.trim().is_empty()
            || self.phase.len() > 128
            || self.safe_message.len() > 4_096
            || (self.side_effect_state == ModelSideEffectState::Unknown && !self.request_sent)
        {
            return Err(ModelError::invalid("model_outcome_invalid"));
        }
        Ok(())
    }
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
            side_effect_state: ModelSideEffectState::None,
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
            side_effect_state: if request_sent {
                ModelSideEffectState::Unknown
            } else {
                ModelSideEffectState::None
            },
        }
    }

    pub fn outcome(&self) -> ModelOutcome {
        let stop_reason = match self.code.as_str() {
            "model_output_truncated" => ModelStopReason::Length,
            "model_refused" => ModelStopReason::Refusal,
            "model_pause_requires_explicit_continue" => ModelStopReason::Pause,
            "model_transport_incomplete" => ModelStopReason::Incomplete,
            _ => ModelStopReason::Unknown,
        };
        ModelOutcome {
            schema: MODEL_OUTCOME_SCHEMA.to_owned(),
            stop_reason,
            phase: self.phase.clone(),
            retry_class: self.retry_class,
            request_sent: self.request_sent,
            side_effect_state: self.side_effect_state,
            error_code: Some(self.code.clone()),
            safe_message: crate::redact_text(&self.safe_message),
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
    pub provider_timing: Option<ProviderTiming>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderTiming {
    pub load_duration_ns: Option<u64>,
    pub generation_duration_ns: Option<u64>,
}

impl ProviderTiming {
    pub fn is_empty(&self) -> bool {
        self.load_duration_ns.is_none() && self.generation_duration_ns.is_none()
    }
}
impl ModelReply {
    pub fn legacy(mut output: ModelOutput) -> Result<Self, ModelError> {
        let blocks = output.content_blocks()?;
        let tool_calls = blocks
            .iter()
            .filter_map(|block| match block {
                ModelContent::ToolCall { call } => Some(call.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        validate_model_calls(&tool_calls)?;
        let finish =
            ModelFinish::parse(output.stop_reason.as_deref(), !tool_calls.is_empty(), true)?;
        finish.require_complete()?;
        if output.stop_reason.is_none() {
            output.stop_reason = Some(
                match finish {
                    ModelFinish::EndTurn => "end_turn",
                    ModelFinish::ToolUse => "tool_use",
                    _ => "incomplete",
                }
                .to_owned(),
            );
        }
        Ok(Self {
            output,
            finish,
            structured: None,
            replay: Vec::new(),
            provider_request_id: None,
            provider_response_id: None,
            provider_timing: None,
        })
    }

    pub fn outcome(&self) -> ModelOutcome {
        ModelOutcome {
            schema: MODEL_OUTCOME_SCHEMA.to_owned(),
            stop_reason: self.finish.into(),
            phase: "completed".to_owned(),
            retry_class: ModelRetryClass::Never,
            request_sent: true,
            side_effect_state: ModelSideEffectState::None,
            error_code: None,
            safe_message: String::new(),
        }
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
        let blocks = message.content_blocks()?;
        let calls = blocks
            .iter()
            .filter_map(|block| match block {
                ModelContent::ToolCall { call } => Some(call.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_result = blocks.iter().find_map(|block| match block {
            ModelContent::ToolResult { call_id, .. } => Some(call_id.as_str()),
            _ => None,
        });
        match message.role {
            ModelRole::System => {
                if !pending.is_empty() || !calls.is_empty() || tool_result.is_some() {
                    return Err(ModelError::invalid("model_history_incomplete_batch"));
                }
            }
            ModelRole::Assistant => {
                if !pending.is_empty() || tool_result.is_some() {
                    return Err(ModelError::invalid("model_history_incomplete_batch"));
                }
                validate_model_calls(&calls)?;
                pending.extend(calls.into_iter().map(|call| call.id));
            }
            ModelRole::Tool => {
                if !message.tool_calls.is_empty()
                    || !tool_result.is_some_and(|id| pending.remove(id))
                {
                    return Err(ModelError::invalid("model_history_orphan_tool_result"));
                }
            }
            ModelRole::User => {
                if !pending.is_empty() || !calls.is_empty() || tool_result.is_some() {
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
