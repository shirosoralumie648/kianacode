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
    if !spec.replay.is_empty() {
        for reference in &spec.replay {
            reference.validate_for_call(&route, spec.call_id, spec.deadline_unix_ms)?;
        }
        // The provider crate only receives reference metadata. A deployment without a protected
        // artifact adapter must refuse before network dispatch rather than guessing at a replay
        // payload or persisting private reasoning in the request/receipt.
        return Err(ModelError::invalid("model_replay_storage_unavailable"));
    }
    validate_image_admissions(&request, &route, connection, images)?;
    let original_request = request.clone();
    let request = normalize_structured_request(request, &route, connection, images)?;
    let context = ModelRequestContext::for_request(&request);
    let tool_names = ToolNameMap::from_tools(&request.tools, true)?;
    let tools = tool_map(&request.tools, &tool_names)?;
    let mut body = match route.protocol {
        ModelProtocol::AnthropicMessages => {
            anthropic_body(&request, &context.system_prompt, &tools, &tool_names)
        }
        ModelProtocol::OpenAiChat => {
            chat_body(&request, &context.system_prompt, &tools, &tool_names)
        }
        ModelProtocol::OpenAiResponses => {
            responses_body(&request, &context.system_prompt, &tools, &tool_names)
        }
        ModelProtocol::OllamaChat => {
            ollama_body(&request, &context.system_prompt, &tools, &tool_names)
        }
        ModelProtocol::GeminiInteractions => {
            gemini_body(&request, &context.system_prompt, &tools, &tool_names)
        }
        ModelProtocol::Legacy => {
            return Err(ModelError::invalid(
                "provider_legacy_protocol_not_networked",
            ))
        }
    }?;
    inject_admitted_images(
        &mut body,
        &route,
        &original_request,
        images,
        connection.limits.max_body,
    )?;
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
    match &spec.response_format {
        ModelResponseFormat::Text => {}
        ModelResponseFormat::JsonObject => match route.protocol {
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
                return Err(ModelError::invalid("model_structured_output_protocol_unsupported"))
            }
            _ => {}
        },
        ModelResponseFormat::JsonSchema { name, schema } => match route.protocol {
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
                return Err(ModelError::invalid("model_structured_output_protocol_unsupported"))
            }
            _ => {}
        },
    }
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
    for image in images {
        image.validate_for_route(
            route,
            &connection.capabilities,
            image.admitted_at_unix_ms,
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
        return Err(ModelError::invalid("image_payload_limit_applies_after_encoding"));
    }
    let data_url = format!("data:{};base64,{}", image.media_type, encoded);
    if data_url.len() > limits.max_request_bytes {
        return Err(ModelError::invalid("image_payload_limit_applies_after_encoding"));
    }
    Ok((encoded, data_url, image.media_type.clone()))
}

fn push_text_and_images(
    content: &mut Value,
    images: &[(String, String, String)],
    kind: &str,
) -> Result<(), ModelError> {
    if content.is_string() {
        let text = content.take();
        *content = json!([{"type":"text","text":text}]);
    }
    let values = content
        .as_array_mut()
        .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
    for (encoded, data_url, _) in images {
        values.push(match kind {
            "anthropic" => json!({
                "type":"image",
                "source":{"type":"base64","media_type":"","data":encoded}
            }),
            "chat" => json!({"type":"image_url","image_url":{"url":data_url}}),
            "responses" => json!({"type":"input_image","image_url":data_url}),
            _ => return Err(ModelError::invalid("image_wire_protocol_unsupported")),
        });
    }
    Ok(())
}

fn inject_admitted_images(
    body: &mut Value,
    route: &ModelRoute,
    original_request: &ModelRequest,
    images: &[ImageInputAdmission],
    max_request_bytes: usize,
) -> Result<(), ModelError> {
    if images.is_empty() {
        return Ok(());
    }
    let mut selected = Vec::new();
    for image in images {
        let referenced = original_request.messages.iter().any(|message| {
            message.content_blocks().ok().is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    matches!(
                        block,
                        ModelContent::AttachmentRef {
                            artifact_ref,
                            media_type,
                            digest,
                        } if image.matches_attachment(artifact_ref, media_type, digest)
                    )
                })
            })
        });
        if referenced {
            selected.push(image_data(image, max_request_bytes)?);
        }
    }
    if selected.is_empty() {
        return Err(ModelError::invalid("image_admission_unreferenced"));
    }
    match route.protocol {
        ModelProtocol::AnthropicMessages => {
            let messages = body["messages"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            let index = messages
                .iter()
                .position(|message| message["role"] == "user")
                .ok_or_else(|| ModelError::invalid("image_wire_user_message_missing"))?;
            let content = &mut messages[index]["content"];
            if content.is_string() {
                let text = content.take();
                *content = json!([{"type":"text","text":text}]);
            }
            let values = content
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            for (encoded, _, media_type) in &selected {
                values.push(json!({
                    "type":"image",
                    "source":{"type":"base64","media_type":media_type,"data":encoded}
                }));
            }
        }
        ModelProtocol::OpenAiChat => {
            let messages = body["messages"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            let index = messages
                .iter()
                .position(|message| message["role"] == "user")
                .ok_or_else(|| ModelError::invalid("image_wire_user_message_missing"))?;
            push_text_and_images(&mut messages[index]["content"], &selected, "chat")?;
        }
        ModelProtocol::OpenAiResponses => {
            let input = body["input"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            let index = input
                .iter()
                .position(|item| item["role"] == "user")
                .ok_or_else(|| ModelError::invalid("image_wire_user_message_missing"))?;
            push_text_and_images(&mut input[index]["content"], &selected, "responses")?;
        }
        ModelProtocol::OllamaChat => {
            let messages = body["messages"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            let index = messages
                .iter()
                .position(|message| message["role"] == "user")
                .ok_or_else(|| ModelError::invalid("image_wire_user_message_missing"))?;
            let values = messages[index]
                .as_object_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?
                .entry("images")
                .or_insert_with(|| json!([]));
            let values = values
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            for (encoded, _, _) in &selected {
                values.push(json!(encoded));
            }
        }
        ModelProtocol::GeminiInteractions => {
            let input = body["input"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            let index = input
                .iter()
                .position(|item| item["type"] == "user_input")
                .ok_or_else(|| ModelError::invalid("image_wire_user_message_missing"))?;
            let values = input[index]["content"]
                .as_array_mut()
                .ok_or_else(|| ModelError::invalid("image_wire_content_shape_invalid"))?;
            for (encoded, _, media_type) in &selected {
                values.push(json!({
                    "type":"inline_data",
                    "inline_data":{"mime_type":media_type,"data":encoded}
                }));
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
    connection: &Connection,
    images: &[ImageInputAdmission],
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
                ModelContent::AttachmentRef {
                    artifact_ref,
                    media_type,
                    digest,
                } => {
                    if connection.capabilities.images != CapabilitySupport::Supported {
                        return Err(ModelError::invalid(
                            "unsupported_content_block_fails_before_request",
                        ));
                    }
                    if !images
                        .iter()
                        .any(|image| image.matches_attachment(&artifact_ref, &media_type, &digest))
                    {
                        return Err(ModelError::invalid("image_admission_required"));
                    }
                }
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
            return Err(ModelError::invalid("provider_continuation_unsupported"));
        }
        message.text = text;
        message.tool_calls = tool_calls;
        message.tool_call_id = tool_call_id;
        message.content.clear();
        message.continuation = None;
    }
    Ok(request)
}
fn anthropic_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    names: &ToolNameMap,
) -> Result<Value, ModelError> {
    let mut messages = Vec::new();
    for message in &request.messages {
        match message.role {
            ModelRole::System => {}
            ModelRole::User => messages.push(json!({"role":"user","content":message.text})),
            ModelRole::Assistant => {
                let mut content = Vec::new();
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
) -> Result<Value, ModelError> {
    let mut messages = Vec::new();
    if !system.is_empty() {
        messages.push(json!({"role":"system","content":system}));
    }
    for message in &request.messages {
        match message.role {
            ModelRole::System => {}
            ModelRole::User => messages.push(json!({"role":"user","content":message.text})),
            ModelRole::Assistant => {
                let mut item = json!({"role":"assistant","content":message.text});
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
) -> Result<Value, ModelError> {
    let mut input = Vec::new();
    for message in &request.messages {
        match message.role {
            ModelRole::System=>{},
            ModelRole::User=>input.push(json!({"role":"user","content":message.text})),
            ModelRole::Assistant=>{
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
fn ollama_body(
    request: &ModelRequest,
    system: &str,
    tools: &[Value],
    names: &ToolNameMap,
) -> Result<Value, ModelError> {
    let mut body = chat_body(request, system, tools, names)?;
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
    for message in &request.messages {
        match message.role {
            ModelRole::System => {}
            ModelRole::User => input
                .push(json!({"type":"user_input","content":[{"type":"text","text":message.text}]})),
            ModelRole::Assistant => {
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
        .map_err(|_| ModelError::invalid("model_structured_output_invalid"))?;
    if let (Some(object), Some(properties)) = (value.as_object(), schema["properties"].as_object())
    {
        if schema["additionalProperties"] == false
            && object.keys().any(|key| !properties.contains_key(key))
        {
            return Err(ModelError::invalid(
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
            return Err(ModelError::invalid("model_structured_output_array_length"));
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
            return Err(ModelError::invalid("model_structured_output_text_length"));
        }
    }
    if let Some(n) = value.as_f64() {
        if schema["maximum"].as_f64().is_some_and(|max| n > max) {
            return Err(ModelError::invalid("model_structured_output_number_range"));
        }
    }
    Ok(())
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
            return Err(ModelError::invalid("model_structured_output_empty"))
        }
        _ => serde_json::from_str(text)
            .map_err(|_| ModelError::invalid("model_structured_output_invalid_json"))?,
    };
    match format {
        ModelResponseFormat::Text => unreachable!(),
        ModelResponseFormat::JsonObject => {
            if !value.is_object() {
                return Err(ModelError::invalid("model_structured_output_object_required"));
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
    fn structured_parser_has_no_implicit_repair_path() {
        let format = ModelResponseFormat::JsonSchema {
            name: "answer".to_owned(),
            schema: json!({
                "type":"object",
                "properties":{"answer":{"type":"string"}},
                "required":["answer"]
            }),
        };
        assert_eq!(
            parse_structured_output(&format, "")
                .unwrap_err()
                .code,
            "model_structured_output_empty"
        );
        assert_eq!(
            parse_structured_output(&format, "{\"answer\":")
                .unwrap_err()
                .code,
            "model_structured_output_invalid_json"
        );
        assert_eq!(
            parse_structured_output(&format, "{\"answer\":\"ok\"}")
                .unwrap()
                .unwrap()["answer"],
            "ok"
        );
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
}
