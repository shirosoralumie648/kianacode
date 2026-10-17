//! 统一模型请求到 provider wire body 的编译器。
//!
//! 上游是 `kiana-runner` 生成的 `ModelRequest`、`ModelCallSpec` 和模型工具 schema；下游
//! 是 `ProviderGateway::complete_admitted` 调用的 transport。这里按 `ModelProtocol` 选择
//! Anthropic、OpenAI Chat/Responses、Ollama 或 Gemini 的请求形状，同时保留统一的 route、
//! request hash、工具目录摘要和 token budget。
//!
//! 编译阶段只构造请求，不访问网络，也不消费 permit。它会先拒绝非法历史、未支持的工具
//! 或结构化输出、未准入的图片引用、过大的 wire body 和 legacy network route。生成的
//! `PreparedModelCall` 在返回前封存并校验，之后还必须经过 provider gateway 的 admission、
//! 凭据代次和配置路由检查才能发送。

use crate::config::Connection;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use kiana_domain::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub(crate) fn internal_name(name: &str, tools: &[Value]) -> Result<String, ModelError> {
    ToolNameMap::from_tools(tools, true)?
        .internal_name(name)
        .map(str::to_owned)
}
fn tool_map(tools: &[Value], names: &ToolNameMap) -> Result<Vec<Value>, ModelError> {
    tools.iter().map(|tool| {
        let name = tool["name"]
            .as_str()
            .ok_or_else(|| ModelError::invalid("model_tool_schema_name_missing"))?;
        let wire = names.wire_name(name)?.to_owned();
        let parameters=tool.get("parameters").or_else(||tool.get("input_schema")).filter(|v|v.is_object())
            .ok_or_else(||ModelError::invalid("model_tool_schema_missing"))?;
        Ok(json!({"name":wire,"description":tool["description"].as_str().unwrap_or(""),"parameters":parameters}))
    }).collect()
}
/// 编译普通模型请求；图片输入默认为空，未带独立 admission 的附件会被拒绝。
pub(crate) fn compile(
    connection: &Connection,
    request: ModelRequest,
    spec: ModelCallSpec,
) -> Result<PreparedModelCall, ModelError> {
    compile_with_images(connection, request, spec, &[])
}

/// Compile a request carrying only image admissions created by the Artifact/data-governance
/// boundary.  The ordinary ModelClient path calls `compile` and therefore rejects an
/// AttachmentRef that has no matching admission.  This explicit entry point keeps image bytes
/// out of arbitrary path handling while allowing a caller with a valid ProcessingGrant to use the
/// same provider request and budget machinery.
pub(crate) fn compile_with_images(
    connection: &Connection,
    request: ModelRequest,
    spec: ModelCallSpec,
    images: &[ImageInputAdmission],
) -> Result<PreparedModelCall, ModelError> {
    compile_inner(connection, request, spec, images, None)
}

/// Compile a request that resumes a provider continuation through a protected replay adapter.
///
/// The adapter resolves each declared reference inside the exact connection/model/protocol/
/// effort/TTL scope and returns the private bytes; the compiler refuses a missing, revoked,
/// expired or foreign entry before any network dispatch. Callers without an adapter keep using
/// `compile`/`compile_with_images`, which fail closed with `model_replay_storage_unavailable`.
pub(crate) fn compile_with_protected_replay(
    connection: &Connection,
    request: ModelRequest,
    spec: ModelCallSpec,
    images: &[ImageInputAdmission],
    source: &dyn ProtectedReplaySource,
    now_unix_ms: u64,
) -> Result<PreparedModelCall, ModelError> {
    compile_inner(
        connection,
        request,
        spec,
        images,
        Some((source, now_unix_ms)),
    )
}

fn compile_inner(
    connection: &Connection,
    request: ModelRequest,
    spec: ModelCallSpec,
    images: &[ImageInputAdmission],
    replay_source: Option<(&dyn ProtectedReplaySource, u64)>,
) -> Result<PreparedModelCall, ModelError> {
    spec.response_format.validate()?;
    validate_model_history(&request.messages)?;
    if !request.tools.is_empty() && connection.capabilities.tools != CapabilitySupport::Supported {
        return Err(ModelError::invalid(
            "model_tool_capability_unknown_or_unsupported",
        ));
    }
    if spec.response_format.is_structured()
        && connection.capabilities.structured_output != CapabilitySupport::Supported
    {
        return Err(ModelError::invalid("model_structured_output_unsupported"));
    }
    if let ModelResponseFormat::JsonSchema { schema, .. } = &spec.response_format {
        check_schema(schema, 0)?;
    }
    let mut route = connection.route.clone();
    require_gemini_streaming(&mut route);
    if let Some(assignment) = &spec.assignment {
        route.profile = assignment.profile.clone();
    }
    let replay = match replay_source {
        Some((source, now_unix_ms)) => {
            crate::replay::plan_injection(&route, &request, &spec, source, now_unix_ms)?
        }
        None => {
            if !spec.replay.is_empty() {
                for reference in &spec.replay {
                    reference.validate_for_call(&route, spec.call_id, spec.deadline_unix_ms)?;
                }
                // The provider crate only receives reference metadata. A deployment without a
                // protected artifact adapter must refuse before network dispatch rather than
                // guessing at a replay payload or persisting private reasoning in the
                // request/receipt.
                return Err(ModelError::invalid("model_replay_storage_unavailable"));
            }
            crate::replay::ReplayInjection::default()
        }
    };
    validate_image_admissions(&request, &route, connection, images)?;
    let placements = image_placements(&request, images, connection.limits.max_body)?;
    let request = normalize_structured_request(request, &route, replay_source.is_some())?;
    let context = ModelRequestContext::for_request(&request);
    let tool_names = ToolNameMap::from_tools(&request.tools, true)?;
    let tools = tool_map(&request.tools, &tool_names)?;
    let mut body = match route.protocol {
        ModelProtocol::AnthropicMessages => anthropic_body(
            &request,
            &context.system_prompt,
            &tools,
            &tool_names,
            &replay,
        ),
        ModelProtocol::OpenAiChat => chat_body(
            &request,
            &context.system_prompt,
            &tools,
            &tool_names,
            &replay,
        ),
        ModelProtocol::OpenAiResponses => responses_body(
            &request,
            &context.system_prompt,
            &tools,
            &tool_names,
            &replay,
        ),
        ModelProtocol::OllamaChat => ollama_body(
            &request,
            &context.system_prompt,
            &tools,
            &tool_names,
            &replay,
        ),
        ModelProtocol::GeminiInteractions => gemini_body(
            &request,
            &context.system_prompt,
            &tools,
            &tool_names,
            &replay,
        ),
        ModelProtocol::Legacy => {
            return Err(ModelError::invalid(
                "provider_legacy_protocol_not_networked",
            ))
        }
    }?;
    inject_image_placements(&mut body, route.protocol, &placements)?;
    body["model"] = json!(route.model_id);
    body["stream"] = json!(route.streaming);
    match route.protocol {
        ModelProtocol::AnthropicMessages => body["max_tokens"] = json!(connection.max_output),
        ModelProtocol::OpenAiChat => {
            body["max_completion_tokens"] = json!(connection.max_output);
            if route.streaming {
                body["stream_options"] = json!({"include_usage":true});
            }
        }
        ModelProtocol::OpenAiResponses => body["max_output_tokens"] = json!(connection.max_output),
        ModelProtocol::OllamaChat => body["options"] = json!({"num_predict":connection.max_output}),
        ModelProtocol::GeminiInteractions => {
            gemini_generation_config(&mut body, connection.max_output);
        }
        _ => {}
    }
    apply_response_format(&mut body, route.protocol, &spec.response_format)?;
    let bytes = serde_json::to_vec(&body)
        .map_err(|_| ModelError::invalid("model_request_encoding_failed"))?
        .len();
    if bytes > connection.limits.max_body {
        return Err(ModelError::invalid("model_request_body_limit"));
    }
    let system_bytes = body
        .get("system")
        .or_else(|| body.get("instructions"))
        .or_else(|| body.get("system_instruction"))
        .map(|value| serde_json::to_vec(value).map_or(0, |encoded| encoded.len()))
        .unwrap_or(0);
    let schema_bytes = body
        .get("tools")
        .map(|value| serde_json::to_vec(value).map_or(0, |encoded| encoded.len()))
        .unwrap_or(0);
    let message_bytes = bytes.saturating_sub(system_bytes.saturating_add(schema_bytes));
    // Count the final wire shape once, while retaining the system/schema/output breakdown used by
    // the admission budget. The request hash below freezes the exact body after this calculation.
    let budget = TokenBudget::new(
        message_bytes,
        system_bytes,
        schema_bytes,
        connection.max_output,
        connection.capabilities.context_window,
    );
    let mut prepared = PreparedModelCall {
        schema: MODEL_CALL_SCHEMA.to_owned(),
        spec,
        route,
        request_hash: String::new(),
        budget,
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
        provider_account: Some(connection.provider_account.clone()),
        credential_revision: Some(connection.credential_revision.clone()),
        request,
        wire_body: body,
    };
    prepared.seal();
    prepared.validate()?;
    Ok(prepared)
}

fn validate_image_admissions(
    request: &ModelRequest,
    route: &ModelRoute,
    connection: &Connection,
    images: &[ImageInputAdmission],
) -> Result<(), ModelError> {
    let mut attachments = Vec::new();
    for message in &request.messages {
        for block in message.content_blocks()? {
            if let ModelContent::AttachmentRef {
                artifact_ref,
                media_type,
                digest,
            } = block
            {
                attachments.push((artifact_ref, media_type, digest));
            }
        }
    }
    if attachments.is_empty() {
        if !images.is_empty() {
            return Err(ModelError::invalid("image_admission_without_attachment"));
        }
        return Ok(());
    }
    if connection.capabilities.images != CapabilitySupport::Supported {
        return Err(ModelError::invalid(
            "unsupported_content_block_fails_before_request",
        ));
    }
    if images.len() > 32 {
        return Err(ModelError::invalid("image_admission_count_limit"));
    }
    let limits = ImagePayloadLimits::for_request(connection.limits.max_body);
    // Dispatch-time freshness: retention is compared against the real wall clock here, not
    // against `admitted_at_unix_ms` (that timestamp only proves the admission was valid when it
    // was created). A grant that expired between admission and send fails before any encoding.
    let dispatch_now_unix_ms = crate::transport::unix_ms()?;
    for image in images {
        image.validate_for_route(
            route,
            &connection.capabilities,
            dispatch_now_unix_ms,
            &limits,
        )?;
        if !attachments
            .iter()
            .any(|(artifact_ref, media_type, digest)| {
                image.matches_attachment(artifact_ref, media_type, digest)
            })
        {
            return Err(ModelError::invalid("image_admission_unreferenced"));
        }
    }
    for (artifact_ref, media_type, digest) in attachments {
        if !images
            .iter()
            .any(|image| image.matches_attachment(&artifact_ref, &media_type, &digest))
        {
            return Err(ModelError::invalid("image_admission_required"));
        }
    }
    Ok(())
}

fn image_data(
    image: &ImageInputAdmission,
    max_request_bytes: usize,
) -> Result<(String, String, String), ModelError> {
    let limits = ImagePayloadLimits::for_request(max_request_bytes);
    if image.source_digest() != image.artifact.content_hash {
        return Err(ModelError::invalid("image_artifact_hash_mismatch"));
    }
    let encoded = STANDARD.encode(image.payload_bytes());
    if encoded.len() > limits.max_encoded_bytes {
        return Err(ModelError::invalid(
            "image_payload_limit_applies_after_encoding",
        ));
    }
    let data_url = format!("data:{};base64,{}", image.media_type, encoded);
    if data_url.len() > limits.max_request_bytes {
        return Err(ModelError::invalid(
            "image_payload_limit_applies_after_encoding",
        ));
    }
    Ok((encoded, data_url, image.media_type.clone()))
}

/// 一个已编码准入图片在 wire body 里的最终落点。
///
/// 落点来自引用该 Artifact 的消息本身，而不是“请求里的第一条 user 消息”：把工具结果
/// 里的图片塞进用户轮次会改变请求语义，也会让 provider 无从把图片关联回 tool_use。
#[derive(Clone, Debug)]
enum ImagePlacementTarget {
    /// 第 N 个用户轮次（从 0 计数）；承载 tool_result 的 user 消息不计入用户轮次。
    UserTurn(usize),
    /// Anthropic `tool_result` 块，用模型工具调用 id 定位。
    ToolResult(String),
}

/// 已编码的准入图片与其引用位置。
#[derive(Clone, Debug)]
struct ImagePlacement {
    target: ImagePlacementTarget,
    encoded: String,
    data_url: String,
    media_type: String,
}

/// 按请求消息顺序把每个 `AttachmentRef` 解析成“准入图片 + wire 落点”。
///
/// 附件只允许出现在 user 轮（用户附件）或 tool 轮（工具返回的图片）；两类都复用同一份
/// `ImageInputAdmission` 准入通道。缺失匹配 admission、admission 未被引用、或附件出现在
/// 无法表达图片的角色上，都在编码和网络发送前拒绝。
fn image_placements(
    request: &ModelRequest,
    images: &[ImageInputAdmission],
    max_request_bytes: usize,
) -> Result<Vec<ImagePlacement>, ModelError> {
    let mut placements = Vec::new();
    let mut user_turns = 0usize;
    for message in &request.messages {
        let mut attachments = Vec::new();
        for block in message.content_blocks()? {
            if let ModelContent::AttachmentRef {
                artifact_ref,
                media_type,
                digest,
            } = block
            {
                attachments.push((artifact_ref, media_type, digest));
            }
        }
        let target = if message.role == ModelRole::User {
            let target = ImagePlacementTarget::UserTurn(user_turns);
            user_turns += 1;
            Some(target)
        } else if message.role == ModelRole::Tool {
            message
                .tool_call_id
                .clone()
                .map(ImagePlacementTarget::ToolResult)
        } else {
            None
        };
        if attachments.is_empty() {
            continue;
        }
        let target =
            target.ok_or_else(|| ModelError::invalid("image_attachment_role_unsupported"))?;
        for (artifact_ref, media_type, digest) in attachments {
            let image = images
                .iter()
                .find(|image| image.matches_attachment(&artifact_ref, &media_type, &digest))
                .ok_or_else(|| ModelError::invalid("image_admission_required"))?;
            let (encoded, data_url, media_type) = image_data(image, max_request_bytes)?;
            placements.push(ImagePlacement {
                target: target.clone(),
                encoded,
                data_url,
                media_type,
            });
        }
    }
    if !images.is_empty() && placements.is_empty() {
        return Err(ModelError::invalid("image_admission_unreferenced"));
    }
    Ok(placements)
}

/// 在 `values` 里找第 `ordinal` 个满足谓词的元素下标。
fn ordinal_index(
    values: &[Value],
    predicate: impl Fn(&Value) -> bool,
    ordinal: usize,
) -> Option<usize> {
    values
        .iter()
        .enumerate()
        .filter(|(_, value)| predicate(value))
        .map(|(index, _)| index)
        .nth(ordinal)
}

/// Anthropic 把连续的 tool 消息编译成同一个 user 消息，其 content 数组里有 `tool_result` 块。
/// 这类 user 项不是用户轮次，计数时要跳过。
fn anthropic_tool_result_carrier(message: &Value) -> bool {
    message["content"]
        .as_array()
        .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"))
}

fn push_text_and_images(
    content: &mut Value,
    placements: &[&ImagePlacement],
    kind: &str,
) -> Result<(), ModelError> {
    if content.is_string() {
        let text = content.take();
        *content = json!([{"type":"text","text":text}]);
    }
    let values = content
        .as_array_mut()
        .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
    for placement in placements {
        values.push(match kind {
            "chat" => json!({"type":"image_url","image_url":{"url":placement.data_url}}),
            "responses" => json!({"type":"input_image","image_url":placement.data_url}),
            _ => return Err(ModelError::invalid("image_wire_protocol_unsupported")),
        });
    }
    Ok(())
}

fn push_anthropic_image(content: &mut Value, placement: &ImagePlacement) -> Result<(), ModelError> {
    if content.is_string() {
        let text = content.take();
        *content = json!([{"type":"text","text":text}]);
    }
    content
        .as_array_mut()
        .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?
        .push(json!({
            "type":"image",
            "source":{"type":"base64","media_type":placement.media_type,"data":placement.encoded}
        }));
    Ok(())
}

/// 只把已准入并已编码的图片写进其引用消息；不重新读文件、抓 URL 或改动 admission。
///
/// 图片在工具结果里的唯一受支持形状是 Anthropic `tool_result.content` 内容块数组；
/// 其它协议没有可表达“工具返回图片”的 wire 形状，因此在发送前拒绝而不是错位注入。
fn inject_image_placements(
    body: &mut Value,
    protocol: ModelProtocol,
    placements: &[ImagePlacement],
) -> Result<(), ModelError> {
    if placements.is_empty() {
        return Ok(());
    }
    let has_tool_result = placements
        .iter()
        .any(|placement| matches!(&placement.target, ImagePlacementTarget::ToolResult(_)));
    if has_tool_result && protocol != ModelProtocol::AnthropicMessages {
        return Err(ModelError::invalid(
            "image_tool_result_protocol_unsupported",
        ));
    }
    match protocol {
        ModelProtocol::AnthropicMessages => {
            let messages = body["messages"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            for placement in placements {
                match &placement.target {
                    ImagePlacementTarget::UserTurn(ordinal) => {
                        let index = ordinal_index(
                            messages,
                            |message| {
                                message["role"] == "user" && !anthropic_tool_result_carrier(message)
                            },
                            *ordinal,
                        )
                        .ok_or_else(|| ModelError::invalid("image_wire_user_message_missing"))?;
                        push_anthropic_image(&mut messages[index]["content"], placement)?;
                    }
                    ImagePlacementTarget::ToolResult(call_id) => {
                        let index = messages
                            .iter()
                            .position(|message| {
                                message["role"] == "user"
                                    && message["content"].as_array().is_some_and(|blocks| {
                                        blocks.iter().any(|block| {
                                            block["type"] == "tool_result"
                                                && block["tool_use_id"] == call_id.as_str()
                                        })
                                    })
                            })
                            .ok_or_else(|| ModelError::invalid("image_wire_tool_result_missing"))?;
                        let blocks =
                            messages[index]["content"].as_array_mut().ok_or_else(|| {
                                ModelError::invalid("image_wire_content_shape_invalid")
                            })?;
                        let block = blocks
                            .iter_mut()
                            .find(|block| {
                                block["type"] == "tool_result"
                                    && block["tool_use_id"] == call_id.as_str()
                            })
                            .ok_or_else(|| ModelError::invalid("image_wire_tool_result_missing"))?;
                        let text =
                            block["content"]
                                .as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| {
                                    ModelError::invalid("image_wire_tool_result_content_invalid")
                                })?;
                        block["content"] = json!([
                            {"type":"text","text":text},
                            {
                                "type":"image",
                                "source":{
                                    "type":"base64",
                                    "media_type":placement.media_type,
                                    "data":placement.encoded
                                }
                            }
                        ]);
                    }
                }
            }
        }
        ModelProtocol::OpenAiChat
        | ModelProtocol::OpenAiResponses
        | ModelProtocol::OllamaChat
        | ModelProtocol::GeminiInteractions => {
            for placement in placements {
                let ImagePlacementTarget::UserTurn(ordinal) = &placement.target else {
                    continue;
                };
                let ordinal = *ordinal;
                match protocol {
                    ModelProtocol::OpenAiChat => {
                        let messages = body["messages"].as_array_mut().ok_or_else(|| {
                            ModelError::invalid("image_wire_content_shape_invalid")
                        })?;
                        let index =
                            ordinal_index(messages, |message| message["role"] == "user", ordinal)
                                .ok_or_else(|| {
                                ModelError::invalid("image_wire_user_message_missing")
                            })?;
                        push_text_and_images(
                            &mut messages[index]["content"],
                            std::slice::from_ref(&placement),
                            "chat",
                        )?;
                    }
                    ModelProtocol::OpenAiResponses => {
                        let input = body["input"].as_array_mut().ok_or_else(|| {
                            ModelError::invalid("image_wire_content_shape_invalid")
                        })?;
                        let index = ordinal_index(input, |item| item["role"] == "user", ordinal)
                            .ok_or_else(|| {
                                ModelError::invalid("image_wire_user_message_missing")
                            })?;
                        push_text_and_images(
                            &mut input[index]["content"],
                            std::slice::from_ref(&placement),
                            "responses",
                        )?;
                    }
                    ModelProtocol::OllamaChat => {
                        let messages = body["messages"].as_array_mut().ok_or_else(|| {
                            ModelError::invalid("image_wire_content_shape_invalid")
                        })?;
                        let index =
                            ordinal_index(messages, |message| message["role"] == "user", ordinal)
                                .ok_or_else(|| {
                                ModelError::invalid("image_wire_user_message_missing")
                            })?;
                        let values = messages[index]
                            .as_object_mut()
                            .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?
                            .entry("images")
                            .or_insert_with(|| json!([]));
                        values
                            .as_array_mut()
                            .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?
                            .push(json!(placement.encoded));
                    }
                    ModelProtocol::GeminiInteractions => {
                        let input = body["input"].as_array_mut().ok_or_else(|| {
                            ModelError::invalid("image_wire_content_shape_invalid")
                        })?;
                        let index =
                            ordinal_index(input, |item| item["type"] == "user_input", ordinal)
                                .ok_or_else(|| {
                                    ModelError::invalid("image_wire_user_message_missing")
                                })?;
                        input[index]["content"]
                            .as_array_mut()
                            .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?
                            .push(json!({
                                "type":"inline_data",
                                "inline_data":{
                                    "mime_type":placement.media_type,
                                    "data":placement.encoded
                                }
                            }));
                    }
                    ModelProtocol::AnthropicMessages | ModelProtocol::Legacy => {
                        return Err(ModelError::invalid("image_wire_protocol_unsupported"));
                    }
                }
            }
        }
        ModelProtocol::Legacy => {
            return Err(ModelError::invalid("image_wire_protocol_unsupported"));
        }
    }
    Ok(())
}

fn gemini_generation_config(body: &mut Value, max_output: u64) {
    // Interactions stream events are the only supported Gemini wire dialect here.
    body["stream"] = json!(true);
    body["store"] = json!(false);
    body["generation_config"] = json!({"max_output_tokens":max_output});
}

fn require_gemini_streaming(route: &mut ModelRoute) {
    if route.protocol == ModelProtocol::GeminiInteractions {
        route.streaming = true;
    }
}

fn normalize_structured_request(
    mut request: ModelRequest,
    route: &ModelRoute,
    allow_continuation: bool,
) -> Result<ModelRequest, ModelError> {
    for message in &mut request.messages {
        if message.content.is_empty() && message.continuation.is_none() {
            continue;
        }
        let blocks = message.content_blocks()?;
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_id = None;
        for block in blocks {
            match block {
                ModelContent::Text { text: value } => text.push_str(&value),
                ModelContent::ToolCall { call } => tool_calls.push(call),
                ModelContent::ToolResult {
                    call_id, content, ..
                } => {
                    tool_call_id = Some(call_id);
                    text = content;
                }
                // AttachmentRef was already matched against the admitted images (and re-checked
                // for role/route/retention) by `validate_image_admissions` and `image_placements`
                // before normalization. The reference itself is not wire text; the image block is
                // injected into the referencing message afterwards.
                ModelContent::AttachmentRef { .. } => {}
                ModelContent::ProviderOpaque {
                    provider_id,
                    protocol,
                    route_digest,
                    ..
                } => {
                    let expected_route = route.digest();
                    if provider_id != route.provider_id
                        || protocol != route.protocol
                        || route_digest != expected_route
                    {
                        return Err(ModelError::invalid("opaque_item_cannot_cross_provider"));
                    }
                    return Err(ModelError::invalid("provider_opaque_item_unsupported"));
                }
            }
        }
        if let Some(continuation) = &message.continuation {
            continuation.validate()?;
            if continuation.provider_id != route.provider_id
                || continuation.protocol != route.protocol
            {
                return Err(ModelError::invalid("opaque_item_cannot_cross_provider"));
            }
            if !allow_continuation || continuation.route_digest != route.digest() {
                return Err(ModelError::invalid("provider_continuation_unsupported"));
            }
        }
        message.text = text;
        message.tool_calls = tool_calls;
        message.tool_call_id = tool_call_id;
        message.content.clear();
        message.continuation = None;
    }
    Ok(request)
}
/// Apply the requested response format to a compiled provider wire body.
///
/// Every supported form is recorded here, separately from tool choice: adding `tools` never
/// changes this patch and a structured format never suppresses or merges into the tool catalog.
/// A protocol that cannot express the requested form fails before encoding, so the caller gets a
/// stable rejection instead of a silent fallback to text. `Text` is always a no-op, keeping the
/// default request shape byte-identical to the pre-structured-output compiler.
fn apply_response_format(
    body: &mut Value,
    protocol: ModelProtocol,
    format: &ModelResponseFormat,
) -> Result<(), ModelError> {
    match format {
        ModelResponseFormat::Text => {}
        ModelResponseFormat::JsonObject => match protocol {
            ModelProtocol::OpenAiChat => body["response_format"] = json!({"type":"json_object"}),
            ModelProtocol::OpenAiResponses => {
                body["text"] = json!({"format":{"type":"json_object"}})
            }
            ModelProtocol::OllamaChat => body["format"] = json!("json"),
            ModelProtocol::GeminiInteractions => {
                body["response_format"] = json!({"type":"text","mime_type":"application/json"})
            }
            ModelProtocol::AnthropicMessages => {
                return Err(ModelError::invalid("anthropic_json_object_requires_schema"))
            }
            ModelProtocol::Legacy => {
                return Err(ModelError::invalid(
                    "model_structured_output_protocol_unsupported",
                ))
            }
        },
        ModelResponseFormat::JsonSchema { name, schema } => match protocol {
            ModelProtocol::OpenAiChat => {
                body["response_format"] = json!({"type":"json_schema","json_schema":{"name":name,"schema":schema,"strict":true}})
            }
            ModelProtocol::OpenAiResponses => {
                body["text"] = json!({"format":{"type":"json_schema","name":name,"schema":schema,"strict":true}})
            }
            ModelProtocol::OllamaChat => body["format"] = schema.clone(),
            ModelProtocol::AnthropicMessages => {
                body["output_config"] = json!({"format":{"type":"json_schema","schema":schema}})
            }
            ModelProtocol::GeminiInteractions => {
                body["response_format"] =
                    json!({"type":"text","mime_type":"application/json","schema":schema})
            }
            ModelProtocol::Legacy => {
                return Err(ModelError::invalid(
                    "model_structured_output_protocol_unsupported",
                ))
            }
        },
    }
    Ok(())
}
fn anthropic_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    names: &ToolNameMap,
    replay: &crate::replay::ReplayInjection,
) -> Result<Value, ModelError> {
    let mut messages = Vec::new();
    for (index, message) in request.messages.iter().enumerate() {
        match message.role {
            ModelRole::System => {}
            ModelRole::User => messages.push(json!({"role":"user","content":message.text})),
            ModelRole::Assistant => {
                let mut content = Vec::new();
                // Private thinking/signature blocks come first, exactly as captured.
                for item in replay.items_for(index) {
                    content.push(item.clone());
                }
                if !message.text.is_empty() {
                    content.push(json!({"type":"text","text":message.text}));
                }
                for call in &message.tool_calls {
                    content.push(json!({"type":"tool_use","id":call.id,"name":names.wire_name(&call.name)? ,"input":call.arguments}));
                }
                if content.is_empty() {
                    return Err(ModelError::invalid("model_empty_assistant_item"));
                }
                messages.push(json!({"role":"assistant","content":content}));
            }
            ModelRole::Tool => {
                let failed = serde_json::from_str::<Value>(&message.text)
                    .ok()
                    .is_some_and(|value| value["error"].is_string() || value["success"] == false);
                let block = json!({"type":"tool_result","tool_use_id":message.tool_call_id,"content":message.text,"is_error":failed});
                if messages
                    .last()
                    .is_some_and(|m| m["role"] == "user" && m["content"].is_array())
                {
                    messages.last_mut().unwrap()["content"]
                        .as_array_mut()
                        .unwrap()
                        .push(block);
                } else {
                    messages.push(json!({"role":"user","content":[block]}));
                }
            }
        }
    }
    let tools=tools.iter().map(|tool|json!({"name":tool["name"],"description":tool["description"],"input_schema":tool["parameters"]})).collect::<Vec<_>>();
    let mut body = json!({"messages":messages,"system":system});
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    Ok(body)
}
fn chat_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    names: &ToolNameMap,
    replay: &crate::replay::ReplayInjection,
) -> Result<Value, ModelError> {
    let mut messages = Vec::new();
    if !system.is_empty() {
        messages.push(json!({"role":"system","content":system}));
    }
    for (index, message) in request.messages.iter().enumerate() {
        match message.role {
            ModelRole::System => {}
            ModelRole::User => messages.push(json!({"role":"user","content":message.text})),
            ModelRole::Assistant => {
                let mut item = json!({"role":"assistant","content":message.text});
                // DeepSeek-compatible reasoning content is one private string, resent verbatim.
                let private = replay.items_for(index);
                if !private.is_empty() {
                    if private.len() != 1 {
                        return Err(ModelError::invalid("protected_replay_item_count_invalid"));
                    }
                    item["reasoning_content"] = private[0].clone();
                }
                if !message.tool_calls.is_empty() {
                    item["tool_calls"] = json!(message
                        .tool_calls
                        .iter()
                        .map(|call| {
                            Ok(json!({"id":call.id,"type":"function","function":{"name":names.wire_name(&call.name)?,"arguments":call.arguments.to_string()}}))
                        })
                        .collect::<Result<Vec<_>, ModelError>>()?);
                }
                messages.push(item);
            }
            ModelRole::Tool => messages.push(
                json!({"role":"tool","tool_call_id":message.tool_call_id,"content":message.text}),
            ),
        }
    }
    let mut body = json!({"messages":messages,"n":1});
    if !tools.is_empty() {
        body["tools"] = json!(tools
            .iter()
            .map(|tool| json!({"type":"function","function":tool}))
            .collect::<Vec<_>>());
    }
    Ok(body)
}
fn responses_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    names: &ToolNameMap,
    replay: &crate::replay::ReplayInjection,
) -> Result<Value, ModelError> {
    let mut input = Vec::new();
    for (index, message) in request.messages.iter().enumerate() {
        match message.role {
            ModelRole::System=>{},
            ModelRole::User=>input.push(json!({"role":"user","content":message.text})),
            ModelRole::Assistant=>{
                // Reasoning items precede the text/function_call items of the same turn.
                for item in replay.items_for(index) {
                    input.push(item.clone());
                }
                if !message.text.is_empty(){input.push(json!({"role":"assistant","content":message.text}));}
                for call in &message.tool_calls {
                    input.push(json!({"type":"function_call","call_id":call.id,"name":names.wire_name(&call.name)?,"arguments":call.arguments.to_string()}));
                }
            },
            ModelRole::Tool=>input.push(json!({"type":"function_call_output","call_id":message.tool_call_id,"output":message.text})),
        }
    }
    let mut body = json!({"input":input,"instructions":system,"store":false});
    if !tools.is_empty() {
        body["tools"]=json!(tools.iter().map(|tool|json!({"type":"function","name":tool["name"],"description":tool["description"],"parameters":tool["parameters"]})).collect::<Vec<_>>());
    }
    Ok(body)
}
pub(crate) fn ollama_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    names: &ToolNameMap,
    replay: &crate::replay::ReplayInjection,
) -> Result<Value, ModelError> {
    let mut body = chat_body(request, system, tools, names, replay)?;
    body.as_object_mut().unwrap().remove("n");
    let mut call_names = BTreeMap::new();
    for message in &request.messages {
        for call in &message.tool_calls {
            call_names.insert(call.id.clone(), names.wire_name(&call.name)?.to_owned());
        }
    }
    for message in body["messages"].as_array_mut().unwrap() {
        if let Some(calls) = message["tool_calls"].as_array_mut() {
            for call in calls {
                call["function"]["arguments"] =
                    serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or(""))
                        .map_err(|_| ModelError::invalid("model_tool_arguments_invalid"))?;
            }
        }
        if message["role"] == "tool" {
            let id = message["tool_call_id"]
                .as_str()
                .ok_or_else(|| ModelError::invalid("model_history_orphan_tool_result"))?;
            message["tool_name"] = json!(call_names
                .get(id)
                .ok_or_else(|| ModelError::invalid("model_history_orphan_tool_result"))?);
            message.as_object_mut().unwrap().remove("tool_call_id");
        }
    }
    Ok(body)
}
fn gemini_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    tool_names: &ToolNameMap,
    replay: &crate::replay::ReplayInjection,
) -> Result<Value, ModelError> {
    // Interactions uses a flat sequence of typed steps, including every local tool result.
    let mut input = Vec::new();
    let mut names = BTreeMap::new();
    for message in &request.messages {
        for call in &message.tool_calls {
            names.insert(
                call.id.clone(),
                tool_names.wire_name(&call.name)?.to_owned(),
            );
        }
    }
    for (index, message) in request.messages.iter().enumerate() {
        match message.role {
            ModelRole::System => {}
            ModelRole::User => input
                .push(json!({"type":"user_input","content":[{"type":"text","text":message.text}]})),
            ModelRole::Assistant => {
                // Thought steps precede the public output/function_call steps of the same turn.
                for step in replay.items_for(index) {
                    input.push(step.clone());
                }
                if !message.text.is_empty() {
                    input.push(json!({"type":"model_output","content":[{"type":"text","text":message.text}]}));
                }
                for call in &message.tool_calls {
                    input.push(json!({"type":"function_call","id":call.id,"name":tool_names.wire_name(&call.name)?,"arguments":call.arguments}));
                }
            }
            ModelRole::Tool => {
                let id = message
                    .tool_call_id
                    .as_ref()
                    .ok_or_else(|| ModelError::invalid("model_history_orphan_tool_result"))?;
                let name = names
                    .get(id)
                    .ok_or_else(|| ModelError::invalid("model_history_orphan_tool_result"))?;
                let is_error = serde_json::from_str::<Value>(&message.text)
                    .ok()
                    .is_some_and(|value| {
                        value["error"].is_string()
                            || value["success"] == false
                            || (value["schema"] == "kiana.tool-observation.v1"
                                && value["status"]
                                    .as_str()
                                    .is_some_and(|status| status != "succeeded"))
                    });
                input.push(json!({"type":"function_result","name":name,"call_id":id,"result":[{"type":"text","text":message.text}],"is_error":is_error}));
            }
        }
    }
    let mut body = json!({"input":input,"system_instruction":system,"store":false});
    if !tools.is_empty() {
        body["tools"]=json!(tools.iter().map(|tool|json!({"type":"function","name":tool["name"],"description":tool["description"],"parameters":tool["parameters"]})).collect::<Vec<_>>());
    }
    Ok(body)
}

#[cfg(test)]
mod gemini_interactions_tests {
    use super::*;

    #[test]
    fn gemini_interaction_parameters_are_resubmitted_each_turn() {
        let mut body = json!({"stream":false,"store":true});
        gemini_generation_config(&mut body, 2048);
        assert_eq!(body["stream"], true);
        assert_eq!(body["store"], false);
        assert_eq!(body["generation_config"]["max_output_tokens"], 2048);
    }

    #[test]
    fn gemini_streaming_route_is_forced_for_sse_transport() {
        let mut route = ModelRoute {
            provider_id: "gemini".to_owned(),
            protocol: ModelProtocol::GeminiInteractions,
            connection_id: "fixture".to_owned(),
            model_id: "gemini-2.5-flash".to_owned(),
            profile: "default".to_owned(),
            configuration_revision: "fixture.v1".to_owned(),
            streaming: false,
        };
        require_gemini_streaming(&mut route);
        assert!(route.streaming);
    }

    #[test]
    fn gemini_stateless_function_result_round_trip() {
        let tools = tool_schemas();
        let names = ToolNameMap::from_tools(&tools, true).expect("tool name map");
        let request = ModelRequest {
            messages: vec![
                ModelMessage::user("check the workspace"),
                ModelMessage {
                    role: ModelRole::Assistant,
                    text: String::new(),
                    tool_call_id: None,
                    tool_calls: vec![ModelToolCall {
                        id: "call-gemini-1".to_owned(),
                        name: "shell".to_owned(),
                        arguments: json!({"command":"pwd"}),
                    }],
                    content: Vec::new(),
                    continuation: None,
                },
                ModelMessage::tool(
                    "call-gemini-1",
                    "{\"schema\":\"kiana.tool-observation.v1\",\"status\":\"failed_known\",\"error_code\":\"denied\"}",
                ),
            ],
            tools: tools.clone(),
            sandbox: "read-only".to_owned(),
        };
        validate_model_history(&request.messages).expect("paired local function result");
        let body = gemini_body(
            &request,
            "system",
            &tool_map(&tools, &names).unwrap(),
            &names,
            &crate::replay::ReplayInjection::default(),
        )
        .expect("Interactions request");
        assert_eq!(body["store"], false);
        assert_eq!(body["system_instruction"], "system");
        assert_eq!(body["input"][1]["type"], "function_call");
        assert_eq!(body["input"][1]["id"], "call-gemini-1");
        assert_eq!(body["input"][2]["type"], "function_result");
        assert_eq!(body["input"][2]["call_id"], "call-gemini-1");
        assert_eq!(body["input"][2]["name"], "shell");
        assert_eq!(body["input"][2]["is_error"], true);
        assert_eq!(
            body["input"][2]["result"][0]["text"],
            request.messages[2].text
        );
        // Stateless: every turn resends the complete ordered local history. No server-side
        // interaction handle is accepted as a substitute for local history, so a missing local
        // function step cannot be papered over by provider state.
        assert_eq!(body["input"][0]["type"], "user_input");
        assert_eq!(body["input"].as_array().map(Vec::len), Some(3));
        assert!(body.get("previous_interaction_id").is_none());
        assert!(body.get("interaction_id").is_none());
    }
}

/// Reject unsupported schema keywords before sending; never silently weaken a contract.
pub(crate) fn check_schema(schema: &Value, depth: usize) -> Result<(), ModelError> {
    if depth > 24 {
        return Err(ModelError::invalid("model_schema_depth_exceeded"));
    }
    let object = schema
        .as_object()
        .ok_or_else(|| ModelError::invalid("model_schema_invalid"))?;
    for (key, value) in object {
        match key.as_str() {
            "type" => {
                if !value.as_str().is_some_and(|v| {
                    matches!(
                        v,
                        "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
                    )
                }) {
                    return Err(ModelError::invalid("model_schema_type_unsupported"));
                }
            }
            "properties" => {
                for child in value
                    .as_object()
                    .ok_or_else(|| ModelError::invalid("model_schema_invalid"))?
                    .values()
                {
                    check_schema(child, depth + 1)?;
                }
            }
            "items" => check_schema(value, depth + 1)?,
            "required" | "enum" => {
                if !value.is_array() {
                    return Err(ModelError::invalid("model_schema_invalid"));
                }
            }
            "additionalProperties" => {
                if !value.is_boolean() {
                    return Err(ModelError::invalid(
                        "model_schema_additional_properties_unsupported",
                    ));
                }
            }
            "minimum" | "maximum" | "minLength" | "maxLength" | "minItems" | "maxItems" => {
                if !value.is_number() {
                    return Err(ModelError::invalid("model_schema_invalid"));
                }
            }
            "description" | "title" | "$schema" => {
                if !value.is_string() {
                    return Err(ModelError::invalid("model_schema_invalid"));
                }
            }
            _ => {
                return Err(ModelError::invalid(format!(
                    "model_schema_keyword_unsupported:{key}"
                )))
            }
        }
    }
    Ok(())
}
pub(crate) fn validate_output(schema: &Value, value: &Value) -> Result<(), ModelError> {
    check_schema(schema, 0)?;
    kiana_domain::validate_schema_value(value, schema)
        .map_err(|_| format_repair_error("model_structured_output_invalid"))?;
    if let (Some(object), Some(properties)) = (value.as_object(), schema["properties"].as_object())
    {
        if schema["additionalProperties"] == false
            && object.keys().any(|key| !properties.contains_key(key))
        {
            return Err(format_repair_error(
                "model_structured_output_extra_property",
            ));
        }
        for (key, child) in properties {
            if let Some(item) = object.get(key) {
                validate_output(child, item)?;
            }
        }
    }
    if let Some(array) = value.as_array() {
        if schema["minItems"]
            .as_u64()
            .is_some_and(|n| array.len() < (n as usize))
            || schema["maxItems"]
                .as_u64()
                .is_some_and(|n| array.len() > (n as usize))
        {
            return Err(format_repair_error("model_structured_output_array_length"));
        }
        if let Some(items) = schema.get("items") {
            for item in array {
                validate_output(items, item)?;
            }
        }
    }
    if let Some(text) = value.as_str() {
        let n = text.chars().count() as u64;
        if schema["minLength"].as_u64().is_some_and(|min| n < min)
            || schema["maxLength"].as_u64().is_some_and(|max| n > max)
        {
            return Err(format_repair_error("model_structured_output_text_length"));
        }
    }
    if let Some(n) = value.as_f64() {
        if schema["maximum"].as_f64().is_some_and(|max| n > max) {
            return Err(format_repair_error("model_structured_output_number_range"));
        }
    }
    Ok(())
}

fn format_repair_error(code: &str) -> ModelError {
    let mut error =
        ModelError::invalid(code).with_recovery_disposition(ModelRecoveryDisposition::FormatRepair);
    error.request_sent = true;
    error
}

/// Parse and validate one complete structured response. This helper deliberately has no repair
/// loop: malformed, empty and schema-invalid output are returned to the Harness as separate
/// failures so any follow-up call is an explicit budgeted `OutputRepair` invocation.
pub(crate) fn parse_structured_output(
    format: &ModelResponseFormat,
    text: &str,
) -> Result<Option<Value>, ModelError> {
    let value = match format {
        ModelResponseFormat::Text => return Ok(None),
        _ if text.trim().is_empty() => {
            return Err(format_repair_error("model_structured_output_empty"))
        }
        _ => serde_json::from_str::<Value>(text)
            .map_err(|_| format_repair_error("model_structured_output_invalid_json"))?,
    };
    match format {
        ModelResponseFormat::Text => unreachable!(),
        ModelResponseFormat::JsonObject => {
            if !value.is_object() {
                return Err(format_repair_error(
                    "model_structured_output_object_required",
                ));
            }
        }
        ModelResponseFormat::JsonSchema { schema, .. } => validate_output(schema, &value)?,
    }
    Ok(Some(value))
}

#[cfg(test)]
mod p4_j7_21_structured_output_tests {
    use super::*;

    #[test]
    fn response_format_metadata_rejects_empty_schema_names() {
        let format = ModelResponseFormat::JsonSchema {
            name: " ".to_owned(),
            schema: json!({"type":"object"}),
        };
        assert_eq!(
            format.validate().unwrap_err().code,
            "model_response_schema_invalid"
        );
    }

    #[test]
    fn structured_output_errors_are_typed_format_repair_without_running_a_repair() {
        let format = ModelResponseFormat::JsonSchema {
            name: "answer".to_owned(),
            schema: json!({
                "type":"object",
                "properties":{"answer":{"type":"string"}},
                "required":["answer"]
            }),
        };
        for (text, expected_code) in [
            ("", "model_structured_output_empty"),
            ("{\"answer\":", "model_structured_output_invalid_json"),
            ("{\"answer\":7}", "model_structured_output_invalid"),
        ] {
            let error = parse_structured_output(&format, text).unwrap_err();
            assert_eq!(error.code, expected_code);
            assert_eq!(
                error.recovery_disposition,
                ModelRecoveryDisposition::FormatRepair
            );
            assert_eq!(error.retry_class, ModelRetryClass::Never);
            assert!(error.request_sent);
            assert_eq!(error.side_effect_state, ModelSideEffectState::None);
        }

        let object_error =
            parse_structured_output(&ModelResponseFormat::JsonObject, "[]").unwrap_err();
        assert_eq!(object_error.code, "model_structured_output_object_required");
        assert_eq!(
            object_error.recovery_disposition,
            ModelRecoveryDisposition::FormatRepair
        );
        assert!(object_error.request_sent);
        assert_eq!(object_error.retry_class, ModelRetryClass::Never);

        assert_eq!(
            parse_structured_output(&format, "{\"answer\":\"ok\"}")
                .unwrap()
                .unwrap()["answer"],
            "ok"
        );
    }

    #[test]
    fn invalid_response_schema_remains_terminal() {
        let error = check_schema(&json!({"type":"unsupported"}), 0).unwrap_err();

        assert_eq!(error.code, "model_schema_type_unsupported");
        assert_eq!(
            error.recovery_disposition,
            ModelRecoveryDisposition::Terminal
        );
        assert!(!error.request_sent);
    }

    fn answer_schema() -> Value {
        json!({
            "type":"object",
            "properties":{"answer":{"type":"string"}},
            "required":["answer"],
            "additionalProperties":false
        })
    }

    /// Each supported response form is recorded per protocol as its own wire patch, and `text`
    /// stays a byte-identical no-op. This is the per-form matrix the card asks for: the same
    /// requested schema lands on five different protocol shapes instead of one shared fallback.
    #[test]
    fn structured_response_format_wire_matrix_is_recorded_per_protocol() {
        let schema = answer_schema();
        let named = ModelResponseFormat::JsonSchema {
            name: "answer".to_owned(),
            schema: schema.clone(),
        };
        let rows = [
            (
                ModelProtocol::OpenAiChat,
                ModelResponseFormat::JsonObject,
                json!({"response_format":{"type":"json_object"}}),
            ),
            (
                ModelProtocol::OpenAiResponses,
                ModelResponseFormat::JsonObject,
                json!({"text":{"format":{"type":"json_object"}}}),
            ),
            (
                ModelProtocol::OllamaChat,
                ModelResponseFormat::JsonObject,
                json!({"format":"json"}),
            ),
            (
                ModelProtocol::GeminiInteractions,
                ModelResponseFormat::JsonObject,
                json!({"response_format":{"type":"text","mime_type":"application/json"}}),
            ),
            (
                ModelProtocol::OpenAiChat,
                named.clone(),
                json!({"response_format":{"type":"json_schema","json_schema":{"name":"answer","schema":schema.clone(),"strict":true}}}),
            ),
            (
                ModelProtocol::OpenAiResponses,
                named.clone(),
                json!({"text":{"format":{"type":"json_schema","name":"answer","schema":schema.clone(),"strict":true}}}),
            ),
            (
                ModelProtocol::OllamaChat,
                named.clone(),
                json!({"format":schema.clone()}),
            ),
            (
                ModelProtocol::AnthropicMessages,
                named.clone(),
                json!({"output_config":{"format":{"type":"json_schema","schema":schema.clone()}}}),
            ),
            (
                ModelProtocol::GeminiInteractions,
                named,
                json!({"response_format":{"type":"text","mime_type":"application/json","schema":schema.clone()}}),
            ),
        ];
        for (protocol, format, expected) in rows {
            let mut body = json!({});
            apply_response_format(&mut body, protocol, &format).expect("supported response form");
            assert_eq!(body, expected, "{protocol:?} {format:?}");
        }
        for protocol in [
            ModelProtocol::Legacy,
            ModelProtocol::AnthropicMessages,
            ModelProtocol::OpenAiChat,
            ModelProtocol::OpenAiResponses,
            ModelProtocol::OllamaChat,
            ModelProtocol::GeminiInteractions,
        ] {
            let mut body = json!({"model":"fixture"});
            apply_response_format(&mut body, protocol, &ModelResponseFormat::Text)
                .expect("text is always accepted");
            assert_eq!(body, json!({"model":"fixture"}), "{protocol:?} text");
        }
    }

    #[test]
    fn unsupported_structured_format_is_rejected_before_encoding() {
        let schema_format = ModelResponseFormat::JsonSchema {
            name: "answer".to_owned(),
            schema: answer_schema(),
        };
        for (protocol, format) in [
            (ModelProtocol::Legacy, ModelResponseFormat::JsonObject),
            (ModelProtocol::Legacy, schema_format.clone()),
            (
                ModelProtocol::AnthropicMessages,
                ModelResponseFormat::JsonObject,
            ),
        ] {
            let mut body = json!({});
            let error = apply_response_format(&mut body, protocol, &format).unwrap_err();
            let expected = match (protocol, &format) {
                (ModelProtocol::AnthropicMessages, _) => "anthropic_json_object_requires_schema",
                _ => "model_structured_output_protocol_unsupported",
            };
            assert_eq!(error.code, expected, "{protocol:?} {format:?}");
            assert!(!error.request_sent, "{protocol:?} {format:?}");
            assert_eq!(error.retry_class, ModelRetryClass::Never);
            assert_eq!(
                error.recovery_disposition,
                ModelRecoveryDisposition::Terminal
            );
            assert_eq!(error.side_effect_state, ModelSideEffectState::None);
            assert_eq!(
                body,
                json!({}),
                "{protocol:?} {format:?} left no partial patch"
            );
        }
    }
}

#[cfg(test)]
mod p4_j7_22_image_input_tests {
    use super::*;

    fn admitted() -> ImageInputAdmission {
        let payload = b"fixture-image-payload".to_vec();
        let digest = image_payload_digest(&payload);
        let route = ModelRoute {
            provider_id: "fixture-provider".to_owned(),
            protocol: ModelProtocol::OpenAiChat,
            connection_id: "fixture-connection".to_owned(),
            model_id: "vision-model".to_owned(),
            profile: "default".to_owned(),
            configuration_revision: "fixture.v1".to_owned(),
            streaming: true,
        };
        let artifact = ArtifactRef {
            schema: "kiana.artifact-ref.v1".to_owned(),
            artifact_id: ArtifactId::new(),
            version: 1,
            artifact_schema: "image/png".to_owned(),
            content_hash: digest.clone(),
            scope_digest: format!("sha256:{}", "b".repeat(64)),
            provenance: ArtifactProvenance {
                producer_kind: "fixture".to_owned(),
                producer_id: "image-fixture".to_owned(),
                source_event_id: None,
                source_run_id: None,
                recorded_by: "fixture".to_owned(),
            },
        };
        let grant = ProcessingGrant {
            id: "grant-image".to_owned(),
            source_path: "assets/image.png".to_owned(),
            content_hash: digest,
            class: DataClass::Restricted,
            purpose: Purpose {
                id: "vision-review".to_owned(),
                description: "fixture vision review".to_owned(),
            },
            retention: Retention {
                expires_at_ms: Some(10_000),
                retain_audit_metadata: true,
            },
            parent_ids: Vec::new(),
            created_by: "principal:fixture".to_owned(),
            revoked: false,
        };
        let capabilities = ModelCapabilities {
            tools: CapabilitySupport::Supported,
            streaming: CapabilitySupport::Supported,
            structured_output: CapabilitySupport::Supported,
            images: CapabilitySupport::Supported,
            reasoning_replay: CapabilitySupport::Unsupported,
            context_window: 16_384,
            max_output: 1_024,
            source: "fixture".to_owned(),
            revision: "fixture.v1".to_owned(),
        };
        ImageInputAdmission::new(
            artifact,
            grant,
            "assets/image.png",
            "image/png",
            payload,
            route,
            capabilities,
            format!("sha256:{}", "c".repeat(64)),
            1,
            1,
            "vision-review",
            vec![DataClass::Restricted],
            100,
        )
        .expect("image fixture")
    }

    #[test]
    fn image_payload_limit_applies_after_encoding() {
        let error = image_data(&admitted(), 4).unwrap_err();
        assert_eq!(error.code, "image_payload_limit_applies_after_encoding");
    }

    #[test]
    fn admitted_image_wire_data_is_base64_and_never_a_path() {
        let (encoded, data_url, _) = image_data(&admitted(), 4096).expect("encoded image");
        assert!(!encoded.contains("assets/image.png"));
        assert!(data_url.starts_with("data:image/png;base64,"));
    }

    /// 供 placement 用例构造消息的附件引用块；字段来自 fixture admission 本身。
    fn attachment_block(image: &ImageInputAdmission) -> ModelContent {
        ModelContent::AttachmentRef {
            artifact_ref: image.attachment_ref(),
            media_type: image.media_type.clone(),
            digest: image.artifact.content_hash.clone(),
        }
    }

    /// 一个引用 fixture 图片的用户消息：文本块 + 附件引用块。
    fn image_user_message(image: &ImageInputAdmission, text: &str) -> ModelMessage {
        ModelMessage::with_content(
            ModelRole::User,
            vec![
                ModelContent::Text {
                    text: text.to_owned(),
                },
                attachment_block(image),
            ],
        )
        .expect("user image message")
    }

    /// 五个规范工具之一的最小 schema，供工具名映射使用。
    fn tools() -> Vec<Value> {
        vec![json!({
            "name":"shell",
            "description":"fixture shell",
            "parameters":{"type":"object","properties":{}}
        })]
    }

    #[test]
    fn attachment_ref_without_admission_is_rejected_before_encoding() {
        let image = admitted();
        let request = ModelRequest {
            messages: vec![image_user_message(&image, "look")],
            tools: Vec::new(),
            sandbox: "read-only".to_owned(),
        };
        let error = image_placements(&request, &[], 4096).unwrap_err();

        assert_eq!(error.code, "image_admission_required");
        assert!(!error.request_sent);
    }

    #[test]
    fn attachment_ref_outside_user_and_tool_turns_is_rejected() {
        let image = admitted();
        let request = ModelRequest {
            messages: vec![ModelMessage::with_content(
                ModelRole::Assistant,
                vec![attachment_block(&image)],
            )
            .expect("assistant attachment message")],
            tools: Vec::new(),
            sandbox: "read-only".to_owned(),
        };
        let error = image_placements(&request, std::slice::from_ref(&image), 4096).unwrap_err();

        assert_eq!(error.code, "image_attachment_role_unsupported");
    }

    #[test]
    fn user_image_is_placed_in_the_referencing_user_turn() {
        let image = admitted();
        let request = ModelRequest {
            messages: vec![
                ModelMessage::with_content(
                    ModelRole::User,
                    vec![ModelContent::Text {
                        text: "first".to_owned(),
                    }],
                )
                .expect("first user message"),
                image_user_message(&image, "second"),
            ],
            tools: Vec::new(),
            sandbox: "read-only".to_owned(),
        };
        let route = image.route.clone();
        let normalized =
            normalize_structured_request(request.clone(), &route, false).expect("normalized");
        let names = ToolNameMap::from_tools(&request.tools, true).expect("tool names");
        let mut body = chat_body(
            &normalized,
            "",
            &[],
            &names,
            &crate::replay::ReplayInjection::default(),
        )
        .expect("chat body");
        let placements =
            image_placements(&request, std::slice::from_ref(&image), 4096).expect("placements");
        inject_image_placements(&mut body, ModelProtocol::OpenAiChat, &placements)
            .expect("injected image");

        let messages = body["messages"].as_array().expect("chat messages");
        assert_eq!(messages[0]["content"], json!("first"));
        let content = messages[1]["content"]
            .as_array()
            .expect("referencing user content blocks");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "second");
        assert_eq!(content[1]["type"], "image_url");
        assert!(content[1]["image_url"]["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("data:image/png;base64,")));
    }

    #[test]
    fn tool_result_image_is_placed_inside_the_anthropic_tool_result() {
        let image = admitted();
        let call = ModelToolCall {
            id: "call-image".to_owned(),
            name: "shell".to_owned(),
            arguments: json!({"command":"fixture-screenshot"}),
        };
        let request = ModelRequest {
            messages: vec![
                ModelMessage::with_content(
                    ModelRole::Assistant,
                    vec![ModelContent::ToolCall { call: call.clone() }],
                )
                .expect("assistant tool call"),
                ModelMessage::with_content(
                    ModelRole::Tool,
                    vec![
                        ModelContent::ToolResult {
                            call_id: call.id.clone(),
                            content: "{\"status\":\"succeeded\"}".to_owned(),
                            is_error: false,
                        },
                        attachment_block(&image),
                    ],
                )
                .expect("tool image result"),
            ],
            tools: tools(),
            sandbox: "read-only".to_owned(),
        };
        let route = image.route.clone();
        let normalized =
            normalize_structured_request(request.clone(), &route, false).expect("normalized");
        let names = ToolNameMap::from_tools(&request.tools, true).expect("tool names");
        let mapped = tool_map(&request.tools, &names).expect("tool schemas");
        let mut body = anthropic_body(
            &normalized,
            "",
            &mapped,
            &names,
            &crate::replay::ReplayInjection::default(),
        )
        .expect("anthropic body");
        let placements =
            image_placements(&request, std::slice::from_ref(&image), 4096).expect("placements");
        inject_image_placements(&mut body, ModelProtocol::AnthropicMessages, &placements)
            .expect("injected tool image");

        let messages = body["messages"].as_array().expect("anthropic messages");
        let tool_result = messages
            .iter()
            .flat_map(|message| message["content"].as_array().into_iter().flatten())
            .find(|block| block["type"] == "tool_result")
            .expect("tool result block");
        assert_eq!(tool_result["tool_use_id"].as_str(), Some("call-image"));
        let content = tool_result["content"]
            .as_array()
            .expect("tool result content blocks");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "{\"status\":\"succeeded\"}");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert!(content[1]["source"]["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty()));
    }

    #[test]
    fn tool_result_image_is_rejected_without_a_protocol_tool_image_shape() {
        let image = admitted();
        let call = ModelToolCall {
            id: "call-image".to_owned(),
            name: "shell".to_owned(),
            arguments: json!({"command":"fixture-screenshot"}),
        };
        let request = ModelRequest {
            messages: vec![
                ModelMessage::with_content(
                    ModelRole::Assistant,
                    vec![ModelContent::ToolCall { call: call.clone() }],
                )
                .expect("assistant tool call"),
                ModelMessage::with_content(
                    ModelRole::Tool,
                    vec![
                        ModelContent::ToolResult {
                            call_id: call.id.clone(),
                            content: "{\"status\":\"succeeded\"}".to_owned(),
                            is_error: false,
                        },
                        attachment_block(&image),
                    ],
                )
                .expect("tool image result"),
            ],
            tools: tools(),
            sandbox: "read-only".to_owned(),
        };
        let placements =
            image_placements(&request, std::slice::from_ref(&image), 4096).expect("placements");
        let mut body = json!({"messages":[]});
        let error =
            inject_image_placements(&mut body, ModelProtocol::OpenAiChat, &placements).unwrap_err();

        assert_eq!(error.code, "image_tool_result_protocol_unsupported");
        assert!(!error.request_sent);
    }
}

#[cfg(test)]
mod p4_j7_20_protected_replay_tests {
    use super::*;
    use crate::replay::{capture_from_value, ProtectedReplayService};

    const ANTHROPIC_RESPONSE: &str = r#"{
        "content": [
            {"type": "thinking", "thinking": "private chain", "signature": "sig-anthropic"},
            {"type": "text", "text": "working"},
            {"type": "tool_use", "id": "call-1", "name": "shell", "input": {"command": "pwd"}}
        ],
        "stop_reason": "tool_use"
    }"#;

    const CHAT_RESPONSE: &str = r#"{
        "choices": [
            {
                "message": {
                    "role": "assistant",
                    "reasoning_content": "deepseek-private-chain",
                    "content": "working",
                    "tool_calls": [
                        {"id": "call-1", "type": "function", "function": {"name": "shell", "arguments": "{\"command\":\"pwd\"}"}}
                    ]
                }
            }
        ]
    }"#;

    const RESPONSES_RESPONSE: &str = r#"{
        "output": [
            {"type": "reasoning", "id": "rs-1", "encrypted_content": "enc-1"},
            {"type": "message", "content": [{"type": "output_text", "text": "working"}]},
            {"type": "function_call", "call_id": "call-1", "name": "shell", "arguments": "{\"command\":\"pwd\"}"}
        ],
        "status": "completed"
    }"#;

    const GEMINI_RESPONSE: &str = r#"{
        "id": "interaction-1",
        "status": "requires_action",
        "steps": [
            {"type": "thought", "signature": "sig-gemini", "content": [{"type": "text", "text": "private chain"}]},
            {"type": "model_output", "content": [{"type": "text", "text": "working"}]},
            {"type": "function_call", "id": "call-1", "name": "shell", "arguments": {"command": "pwd"}}
        ]
    }"#;

    fn scope(
        route: &ModelRoute,
        kind: ReasoningReplayKind,
        call_id: RequestId,
        expires_at_unix_ms: u64,
    ) -> ProtectedReplayScope {
        ProtectedReplayScope {
            connection_id: route.connection_id.clone(),
            protocol: kind.protocol(),
            model_id: route.model_id.clone(),
            route_digest: route.digest(),
            effort: Some("medium".to_owned()),
            prompt_digest: "sha256:prompt".to_owned(),
            tool_catalog_digest: "sha256:tools".to_owned(),
            data_revision: "data.v1".to_owned(),
            source_call_id: call_id,
            expires_at_unix_ms,
        }
    }

    /// A real resolved connection plus the material a provider response captured for it.
    fn fixture(
        provider: &str,
        model: &str,
        response: &str,
        call_id: RequestId,
    ) -> (
        Connection,
        ModelRoute,
        ModelRequest,
        ModelCallSpec,
        ProtectedReplayService,
    ) {
        let connection =
            crate::config::test_connection(provider, model).expect("fixture connection");
        let mut route = connection.route.clone();
        require_gemini_streaming(&mut route);
        let value: Value = serde_json::from_str(response).expect("fixture response");
        let capture = capture_from_value(&route, &value)
            .expect("capture")
            .expect("private material");
        let kind = capture.kind();
        let material = capture
            .material(scope(&route, kind, call_id, 60_000))
            .expect("scoped material");
        let mut service = ProtectedReplayService::new(&[21u8; 32]).expect("service");
        let reference = service.store("artifact-1", &material).expect("store");

        let mut assistant = ModelMessage::assistant("working");
        assistant.tool_calls = vec![ModelToolCall {
            id: "call-1".to_owned(),
            name: "shell".to_owned(),
            arguments: json!({"command": "pwd"}),
        }];
        assistant.continuation = Some(ProviderContinuation {
            schema: PROVIDER_CONTINUATION_SCHEMA.to_owned(),
            provider_id: route.provider_id.clone(),
            protocol: route.protocol,
            route_digest: route.digest(),
            item_ref: "artifact-1".to_owned(),
        });
        let request = ModelRequest {
            messages: vec![
                ModelMessage::user("continue the tool loop"),
                assistant,
                ModelMessage::tool("call-1", "{\"ok\":true}"),
            ],
            tools: tool_schemas(),
            sandbox: "read-only".to_owned(),
        };
        let spec = ModelCallSpec {
            call_id,
            attempt_id: RequestId::new(),
            model_attempt_id: None,
            step_id: None,
            step: 2,
            purpose: ModelPurpose::Task,
            assignment: None,
            response_format: ModelResponseFormat::Text,
            replay: vec![reference],
            deadline_unix_ms: u64::MAX,
        };
        // The adapter travels with the fixture: a caller that resumes must hold the protected
        // store that owns the bytes, never just the reference.
        (connection, route, request, spec, service)
    }

    #[test]
    fn signed_reasoning_survives_tool_continuation_byte_for_byte() {
        let call_id = RequestId::new();
        for (provider, model, response, expected) in [
            (
                "anthropic",
                "claude-sonnet-4-6",
                ANTHROPIC_RESPONSE,
                "anthropic",
            ),
            ("openai", "gpt-4o-mini", CHAT_RESPONSE, "deepseek"),
            (
                "openai-responses",
                "gpt-4.1",
                RESPONSES_RESPONSE,
                "responses",
            ),
            ("gemini", "gemini-2.5-flash", GEMINI_RESPONSE, "gemini"),
        ] {
            let (connection, route, request, spec, service) =
                fixture(provider, model, response, call_id);
            let prepared =
                compile_with_protected_replay(&connection, request, spec, &[], &service, 1_000)
                    .expect("protected replay compiler accepts the stored material");
            assert_eq!(prepared.route.digest(), route.digest());
            let body = &prepared.wire_body;
            match expected {
                "anthropic" => {
                    let messages = body["messages"].as_array().expect("messages");
                    let assistant = messages
                        .iter()
                        .find(|message| message["role"] == "assistant")
                        .expect("assistant message");
                    let content = assistant["content"].as_array().expect("content");
                    // Private thinking/signature first, public text and tool call after it.
                    assert_eq!(content[0]["type"], "thinking");
                    assert_eq!(content[0]["thinking"], "private chain");
                    assert_eq!(content[0]["signature"], "sig-anthropic");
                    assert_eq!(content[1]["type"], "text");
                    assert_eq!(content[1]["text"], "working");
                    assert_eq!(content[2]["type"], "tool_use");
                    assert_eq!(content[2]["id"], "call-1");
                    let result = messages.last().expect("tool result message");
                    assert_eq!(result["content"][0]["type"], "tool_result");
                    assert_eq!(result["content"][0]["tool_use_id"], "call-1");
                }
                "deepseek" => {
                    let messages = body["messages"].as_array().expect("messages");
                    let assistant = messages
                        .iter()
                        .find(|message| message["role"] == "assistant")
                        .expect("assistant message");
                    assert_eq!(assistant["reasoning_content"], "deepseek-private-chain");
                    assert_eq!(assistant["content"], "working");
                    assert_eq!(assistant["tool_calls"][0]["id"], "call-1");
                    assert_eq!(messages.last().expect("tool result")["role"], "tool");
                }
                "responses" => {
                    let input = body["input"].as_array().expect("input");
                    let index = input
                        .iter()
                        .position(|item| item["type"] == "reasoning")
                        .expect("reasoning item");
                    assert_eq!(input[index]["id"], "rs-1");
                    assert_eq!(input[index]["encrypted_content"], "enc-1");
                    assert_eq!(input[index + 1]["role"], "assistant");
                    assert_eq!(input[index + 1]["content"], "working");
                    assert_eq!(input[index + 2]["type"], "function_call");
                    assert_eq!(input[index + 2]["call_id"], "call-1");
                    assert_eq!(
                        input.last().expect("tool output")["type"],
                        "function_call_output"
                    );
                }
                "gemini" => {
                    let input = body["input"].as_array().expect("input");
                    let index = input
                        .iter()
                        .position(|item| item["type"] == "thought")
                        .expect("thought step");
                    assert_eq!(input[index]["signature"], "sig-gemini");
                    assert_eq!(input[index]["content"][0]["text"], "private chain");
                    assert_eq!(input[index + 1]["type"], "model_output");
                    assert_eq!(input[index + 2]["type"], "function_call");
                    assert_eq!(
                        input.last().expect("tool result")["type"],
                        "function_result"
                    );
                }
                other => panic!("unknown fixture {other}"),
            }
        }
    }

    #[test]
    fn continuation_without_a_protected_adapter_is_refused_before_dispatch() {
        let call_id = RequestId::new();
        let (connection, _route, request, spec, _service) = fixture(
            "anthropic",
            "claude-sonnet-4-6",
            ANTHROPIC_RESPONSE,
            call_id,
        );

        // A declared reference without an adapter must never be compiled from guesswork.
        assert_eq!(
            compile(&connection, request.clone(), spec.clone())
                .unwrap_err()
                .code,
            "model_replay_storage_unavailable"
        );

        // A continuation message without a reference is not silently dropped either.
        let mut undeclared = spec;
        undeclared.replay = Vec::new();
        assert_eq!(
            compile(&connection, request, undeclared).unwrap_err().code,
            "provider_continuation_unsupported"
        );
    }

    #[test]
    fn revoked_material_never_reaches_the_wire_body() {
        let call_id = RequestId::new();
        let (connection, _route, request, spec, mut service) = fixture(
            "anthropic",
            "claude-sonnet-4-6",
            ANTHROPIC_RESPONSE,
            call_id,
        );
        service
            .revoke("artifact-1", "data_governance_revocation")
            .expect("revoke");
        assert_eq!(
            compile_with_protected_replay(&connection, request, spec, &[], &service, 1_000)
                .unwrap_err()
                .code,
            "replay_material_revoked"
        );
    }
}
