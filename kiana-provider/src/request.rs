use crate::config::Connection;
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
    validate_model_history(&request.messages)?;
    if !request.tools.is_empty() && connection.capabilities.tools != CapabilitySupport::Supported {
        return Err(ModelError::invalid(
            "model_tool_capability_unknown_or_unsupported",
        ));
    }
    if spec.response_format != ModelResponseFormat::Text
        && connection.capabilities.structured_output != CapabilitySupport::Supported
    {
        return Err(ModelError::invalid("model_structured_output_unsupported"));
    }
    if let ModelResponseFormat::JsonSchema { schema, .. } = &spec.response_format {
        check_schema(schema, 0)?;
    }
    if !spec.replay.is_empty() {
        return Err(ModelError::invalid(
            "model_replay_requires_protected_material",
        ));
    }
    let mut route = connection.route.clone();
    if let Some(assignment) = &spec.assignment {
        route.profile = assignment.profile.clone();
    }
    let request = normalize_structured_request(request, &route, connection)?;
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
            body["generation_config"] = json!({"max_output_tokens":connection.max_output})
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

fn normalize_structured_request(
    mut request: ModelRequest,
    route: &ModelRoute,
    connection: &Connection,
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
                ModelContent::AttachmentRef { .. } => {
                    if connection.capabilities.images != CapabilitySupport::Supported {
                        return Err(ModelError::invalid(
                            "unsupported_content_block_fails_before_request",
                        ));
                    }
                    return Err(ModelError::invalid(
                        "model_attachment_wire_mapping_unsupported",
                    ));
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
                input.push(json!({"type":"function_result","name":name,"call_id":id,"result":[{"type":"text","text":message.text}]}));
            }
        }
    }
    let mut body = json!({"input":input,"system_instruction":system,"store":false});
    if !tools.is_empty() {
        body["tools"]=json!(tools.iter().map(|tool|json!({"type":"function","name":tool["name"],"description":tool["description"],"parameters":tool["parameters"]})).collect::<Vec<_>>());
    }
    Ok(body)
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
