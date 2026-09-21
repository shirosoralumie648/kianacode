use kiana_domain::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn error(code: &str) -> ModelError {
    ModelError::invalid(code)
}
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, ModelError> {
    value[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| error("provider_required_identity_missing"))
}
fn usage(value: &Value, input: &str, output: &str) -> Result<Option<ModelUsage>, ModelError> {
    match (value.get(input), value.get(output)) {
        (None, None) => Ok(None),
        (Some(a), Some(b)) => {
            let input_tokens = a.as_u64().ok_or_else(|| error("provider_usage_invalid"))?;
            let output_tokens = b.as_u64().ok_or_else(|| error("provider_usage_invalid"))?;
            input_tokens
                .checked_add(output_tokens)
                .ok_or_else(|| error("provider_usage_overflow"))?;
            Ok(Some(ModelUsage {
                input_tokens,
                output_tokens,
            }))
        }
        _ => Ok(None),
    }
}

fn update_monotonic(slot: &mut Option<u64>, next: u64) -> Result<(), ModelError> {
    if slot.is_some_and(|previous| next < previous) {
        return Err(error("provider_usage_regressed"));
    }
    *slot = Some(next);
    Ok(())
}
fn call(
    id: String,
    name: &str,
    arguments: Value,
    prepared: &PreparedModelCall,
) -> Result<ModelToolCall, ModelError> {
    let name = crate::request::internal_name(name, &prepared.request.tools)?;
    kiana_domain::validate_tool_arguments(&name, &arguments)
        .map_err(|_| error("provider_tool_schema_invalid"))?;
    Ok(ModelToolCall {
        id,
        name,
        arguments,
    })
}
fn parse_arguments(value: &Value) -> Result<Value, ModelError> {
    let arguments = if let Some(raw) = value.as_str() {
        serde_json::from_str(raw).map_err(|_| error("provider_tool_json_invalid"))?
    } else {
        value.clone()
    };
    if !arguments.is_object() {
        return Err(error("provider_tool_json_object_required"));
    }
    Ok(arguments)
}
pub(crate) fn decode(value: Value, prepared: &PreparedModelCall) -> Result<ModelReply, ModelError> {
    let mut text = String::new();
    let mut calls = Vec::new();
    let reason: String;
    let measured: Option<ModelUsage>;
    let response_id = value["id"].as_str().map(str::to_owned);
    match prepared.route.protocol {
        ModelProtocol::AnthropicMessages => {
            let content = value["content"]
                .as_array()
                .ok_or_else(|| error("provider_content_missing"))?;
            for block in content {
                match block["type"].as_str() {
                    Some("text") => text.push_str(
                        block["text"]
                            .as_str()
                            .ok_or_else(|| error("provider_text_invalid"))?,
                    ),
                    Some("tool_use") => calls.push(call(
                        required(block, "id")?.to_owned(),
                        required(block, "name")?,
                        parse_arguments(&block["input"])?,
                        prepared,
                    )?),
                    Some("thinking" | "redacted_thinking") => {
                        return Err(error("provider_private_replay_not_configured"))
                    }
                    _ => return Err(error("provider_content_block_unsupported")),
                }
            }
            reason = required(&value, "stop_reason")?.to_owned();
            measured = usage(&value["usage"], "input_tokens", "output_tokens")?;
        }
        ModelProtocol::OpenAiChat => {
            let choices = value["choices"]
                .as_array()
                .filter(|v| v.len() == 1)
                .ok_or_else(|| error("provider_choice_count_invalid"))?;
            let choice = &choices[0];
            let message = &choice["message"];
            if message["refusal"].as_str().is_some_and(|s| !s.is_empty()) {
                return Err(error("model_refused"));
            }
            if let Some(content) = message["content"].as_str() {
                text = content.to_owned();
            }
            if message["reasoning_content"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
            {
                return Err(error("provider_private_replay_not_configured"));
            }
            if let Some(items) = message.get("tool_calls") {
                for item in items
                    .as_array()
                    .ok_or_else(|| error("provider_tool_calls_invalid"))?
                {
                    if item["type"] != "function" {
                        return Err(error("provider_hosted_tool_denied"));
                    }
                    calls.push(call(
                        required(item, "id")?.to_owned(),
                        required(&item["function"], "name")?,
                        parse_arguments(&item["function"]["arguments"])?,
                        prepared,
                    )?);
                }
            }
            reason = required(choice, "finish_reason")?.to_owned();
            measured = usage(&value["usage"], "prompt_tokens", "completion_tokens")?;
        }
        ModelProtocol::OpenAiResponses => {
            if value["status"] != "completed" {
                return Err(error("provider_response_incomplete"));
            }
            for item in value["output"]
                .as_array()
                .ok_or_else(|| error("provider_output_missing"))?
            {
                match item["type"].as_str() {
                    Some("message") => {
                        for block in item["content"]
                            .as_array()
                            .ok_or_else(|| error("provider_content_missing"))?
                        {
                            match block["type"].as_str() {
                                Some("output_text") => text.push_str(
                                    block["text"]
                                        .as_str()
                                        .ok_or_else(|| error("provider_text_invalid"))?,
                                ),
                                Some("refusal") => return Err(error("model_refused")),
                                _ => return Err(error("provider_content_block_unsupported")),
                            }
                        }
                    }
                    Some("function_call") => calls.push(call(
                        required(item, "call_id")?.to_owned(),
                        required(item, "name")?,
                        parse_arguments(&item["arguments"])?,
                        prepared,
                    )?),
                    Some("reasoning") => {
                        return Err(error("provider_private_replay_not_configured"))
                    }
                    _ => return Err(error("provider_hosted_tool_denied")),
                }
            }
            reason = if calls.is_empty() {
                "end_turn"
            } else {
                "tool_use"
            }
            .to_owned();
            measured = usage(&value["usage"], "input_tokens", "output_tokens")?;
        }
        ModelProtocol::OllamaChat => {
            if value["done"] != true {
                return Err(error("provider_response_incomplete"));
            }
            let message = &value["message"];
            text = message["content"].as_str().unwrap_or("").to_owned();
            if let Some(items) = message.get("tool_calls") {
                for (ordinal, item) in items
                    .as_array()
                    .ok_or_else(|| error("provider_tool_calls_invalid"))?
                    .iter()
                    .enumerate()
                {
                    let id = item["id"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("ollama:{}:{ordinal}", prepared.spec.call_id));
                    calls.push(call(
                        id,
                        required(&item["function"], "name")?,
                        parse_arguments(&item["function"]["arguments"])?,
                        prepared,
                    )?);
                }
            }
            let raw = required(&value, "done_reason")?;
            reason = if raw == "stop" && !calls.is_empty() {
                "tool_use"
            } else {
                raw
            }
            .to_owned();
            measured = usage(&value, "prompt_eval_count", "eval_count")?;
        }
        ModelProtocol::GeminiInteractions => {
            let status = required(&value, "status")?;
            if !matches!(status, "completed" | "requires_action") {
                return Err(error("provider_response_incomplete"));
            }
            for item in value["steps"]
                .as_array()
                .ok_or_else(|| error("provider_output_missing"))?
            {
                match item["type"].as_str() {
                    Some("model_output") => {
                        for block in item["content"]
                            .as_array()
                            .ok_or_else(|| error("provider_content_missing"))?
                        {
                            if block["type"] != "text" {
                                return Err(error("provider_content_block_unsupported"));
                            }
                            text.push_str(
                                block["text"]
                                    .as_str()
                                    .ok_or_else(|| error("provider_text_invalid"))?,
                            );
                        }
                    }
                    Some("function_call") => calls.push(call(
                        required(item, "id")?.to_owned(),
                        required(item, "name")?,
                        parse_arguments(&item["arguments"])?,
                        prepared,
                    )?),
                    Some("thought" | "thought_summary") => {
                        return Err(error("provider_private_replay_not_configured"))
                    }
                    _ => return Err(error("provider_content_block_unsupported")),
                }
            }
            reason = if status == "requires_action" {
                "tool_use"
            } else {
                "end_turn"
            }
            .to_owned();
            measured = gemini_usage(&value["usage"])?;
        }
        _ => return Err(error("provider_protocol_unsupported")),
    }
    validate_model_calls(&calls)?;
    let finish = ModelFinish::parse(Some(&reason), !calls.is_empty(), false)?;
    finish.require_complete()?;
    let structured = match &prepared.spec.response_format {
        ModelResponseFormat::Text => None,
        format => {
            if !calls.is_empty() {
                None
            } else {
                let parsed: Value = serde_json::from_str(&text)
                    .map_err(|_| error("model_structured_output_invalid_json"))?;
                match format {
                    ModelResponseFormat::JsonObject => {
                        if !parsed.is_object() {
                            return Err(error("model_structured_output_object_required"));
                        }
                    }
                    ModelResponseFormat::JsonSchema { schema, .. } => {
                        crate::request::validate_output(schema, &parsed)?
                    }
                    _ => {}
                }
                Some(parsed)
            }
        }
    };
    Ok(ModelReply {
        output: ModelOutput {
            text,
            tool_calls: calls,
            usage: measured,
            stop_reason: Some(reason),
            model_id: Some(
                value["model"]
                    .as_str()
                    .unwrap_or(&prepared.route.model_id)
                    .to_owned(),
            ),
            content: Vec::new(),
            continuation: None,
        },
        finish,
        structured,
        replay: Vec::new(),
        provider_request_id: None,
        provider_response_id: response_id,
    })
}

#[derive(Default)]
struct Block {
    id: String,
    name: String,
    text: String,
    args: String,
    input: Value,
    closed: bool,
    kind: String,
}
pub(crate) struct Accumulator {
    protocol: ModelProtocol,
    blocks: BTreeMap<usize, Block>,
    started: bool,
    finished: bool,
    reason: Option<String>,
    input: Option<u64>,
    output: Option<u64>,
    model: Option<String>,
    id: Option<String>,
    final_value: Option<Value>,
    frames: usize,
}
impl Accumulator {
    pub(crate) fn new(protocol: ModelProtocol) -> Self {
        Self {
            protocol,
            blocks: BTreeMap::new(),
            started: false,
            finished: false,
            reason: None,
            input: None,
            output: None,
            model: None,
            id: None,
            final_value: None,
            frames: 0,
        }
    }
    pub(crate) fn push(
        &mut self,
        data: &str,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<bool, ModelError> {
        if self.finished {
            return Err(error("provider_frame_after_terminal"));
        }
        self.frames += 1;
        if self.frames > 100_000 {
            return Err(error("provider_event_limit"));
        }
        if data == "[DONE]" {
            if self.protocol != ModelProtocol::OpenAiChat || self.reason.is_none() {
                return Err(error("provider_done_without_finish"));
            }
            self.finished = true;
            return Ok(true);
        }
        let value: Value =
            serde_json::from_str(data).map_err(|_| error("provider_frame_json_invalid"))?;
        match self.protocol {
            ModelProtocol::AnthropicMessages => self.anthropic(value, on_delta)?,
            ModelProtocol::OpenAiChat => self.chat(value, on_delta)?,
            ModelProtocol::OpenAiResponses => self.responses(value, on_delta)?,
            ModelProtocol::OllamaChat => self.ollama(value, on_delta)?,
            ModelProtocol::GeminiInteractions => self.gemini(value, on_delta)?,
            _ => return Err(error("provider_protocol_unsupported")),
        }
        Ok(self.finished)
    }
    fn text(
        &mut self,
        index: usize,
        text: &str,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<(), ModelError> {
        let block = self.blocks.entry(index).or_default();
        if block.closed {
            return Err(error("provider_delta_after_block_stop"));
        }
        if block.text.len().saturating_add(text.len()) > 4 * 1024 * 1024 {
            return Err(error("provider_text_limit"));
        }
        block.kind = "text".to_owned();
        block.text.push_str(text);
        if !text.is_empty() {
            on_delta(ModelDelta::Text {
                text: text.to_owned(),
            })
            .map_err(|message| ModelError::invalid(redact_text(&message)))?;
        }
        Ok(())
    }
    fn anthropic(
        &mut self,
        value: Value,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<(), ModelError> {
        match value["type"].as_str() {
            Some("message_start") => {
                if self.started {
                    return Err(error("provider_duplicate_message_start"));
                }
                self.started = true;
                self.id = Some(required(&value["message"], "id")?.to_owned());
                self.model = Some(required(&value["message"], "model")?.to_owned());
                self.input = value["message"]["usage"]["input_tokens"].as_u64();
                self.output = value["message"]["usage"]["output_tokens"].as_u64();
            }
            Some("content_block_start") => {
                if !self.started || self.reason.is_some() {
                    return Err(error("provider_block_outside_message"));
                }
                let index = index(&value, "index")?;
                if self.blocks.contains_key(&index) {
                    return Err(error("provider_duplicate_block"));
                }
                let content = &value["content_block"];
                match content["type"].as_str() {
                    Some("text") => self.text(
                        index,
                        content["text"]
                            .as_str()
                            .ok_or_else(|| error("provider_text_invalid"))?,
                        sink,
                    )?,
                    Some("tool_use") => {
                        self.blocks.insert(
                            index,
                            Block {
                                kind: "tool_use".to_owned(),
                                id: required(content, "id")?.to_owned(),
                                name: required(content, "name")?.to_owned(),
                                input: content["input"].clone(),
                                ..Default::default()
                            },
                        );
                    }
                    Some("thinking" | "redacted_thinking") => {
                        return Err(error("provider_private_replay_not_configured"))
                    }
                    _ => return Err(error("provider_content_block_unsupported")),
                }
            }
            Some("content_block_delta") => {
                let index = index(&value, "index")?;
                let block = self
                    .blocks
                    .get(&index)
                    .ok_or_else(|| error("provider_delta_without_block"))?;
                if block.closed || self.reason.is_some() {
                    return Err(error("provider_delta_after_block_stop"));
                }
                match value["delta"]["type"].as_str() {
                    Some("text_delta") if block.kind == "text" => self.text(
                        index,
                        value["delta"]["text"]
                            .as_str()
                            .ok_or_else(|| error("provider_text_invalid"))?,
                        sink,
                    )?,
                    Some("input_json_delta") if block.kind == "tool_use" => append_args(
                        self.blocks.get_mut(&index).unwrap(),
                        value["delta"]["partial_json"]
                            .as_str()
                            .ok_or_else(|| error("provider_tool_json_invalid"))?,
                    )?,
                    _ => return Err(error("provider_delta_type_mismatch")),
                }
            }
            Some("content_block_stop") => {
                let block = self
                    .blocks
                    .get_mut(&index(&value, "index")?)
                    .ok_or_else(|| error("provider_stop_without_block"))?;
                if block.closed {
                    return Err(error("provider_duplicate_block_stop"));
                }
                block.closed = true;
            }
            Some("message_delta") => {
                if self.blocks.values().any(|b| !b.closed) {
                    return Err(error("provider_finish_with_open_blocks"));
                }
                if let Some(reason) = value["delta"]["stop_reason"].as_str() {
                    if self.reason.replace(reason.to_owned()).is_some() {
                        return Err(error("provider_duplicate_finish"));
                    }
                }
                if let Some(input) = value["usage"]["input_tokens"].as_u64() {
                    update_monotonic(&mut self.input, input)?;
                }
                if let Some(output) = value["usage"]["output_tokens"].as_u64() {
                    update_monotonic(&mut self.output, output)?;
                }
            }
            Some("message_stop") => {
                if !self.started || self.reason.is_none() || self.blocks.values().any(|b| !b.closed)
                {
                    return Err(error("provider_incomplete_terminal"));
                }
                self.finished = true;
            }
            Some("ping") => {}
            Some("error") => return Err(error("provider_stream_error")),
            _ => return Err(error("provider_unknown_required_event")),
        }
        Ok(())
    }
    fn chat(
        &mut self,
        value: Value,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<(), ModelError> {
        if value.get("error").is_some() {
            return Err(error("provider_stream_error"));
        }
        if let Some(id) = value["id"].as_str() {
            if self.id.as_ref().is_some_and(|old| old != id) {
                return Err(error("provider_response_id_changed"));
            }
            self.id = Some(id.to_owned());
        }
        if let Some(model) = value["model"].as_str() {
            self.model = Some(model.to_owned());
        }
        if let Some(u) = usage(&value["usage"], "prompt_tokens", "completion_tokens")? {
            self.input = Some(u.input_tokens);
            self.output = Some(u.output_tokens);
        }
        let choices = value["choices"]
            .as_array()
            .ok_or_else(|| error("provider_choices_missing"))?;
        if choices.is_empty() {
            return Ok(());
        }
        if choices.len() != 1 || choices[0]["index"] != 0 {
            return Err(error("provider_choice_count_invalid"));
        }
        let choice = &choices[0];
        let delta = &choice["delta"];
        if self.reason.is_some() && delta.as_object().is_some_and(|v| !v.is_empty()) {
            return Err(error("provider_delta_after_finish"));
        }
        if delta["refusal"].as_str().is_some_and(|s| !s.is_empty()) {
            return Err(error("model_refused"));
        }
        if delta["reasoning_content"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
        {
            return Err(error("provider_private_replay_not_configured"));
        }
        if let Some(text) = delta["content"].as_str() {
            self.text(0, text, sink)?;
        }
        if let Some(calls) = delta.get("tool_calls") {
            for item in calls
                .as_array()
                .ok_or_else(|| error("provider_tool_calls_invalid"))?
            {
                let index = index(item, "index")?;
                if index >= 32 {
                    return Err(error("provider_tool_batch_too_large"));
                }
                let block = self.blocks.entry(index + 1000).or_default();
                block.kind = "tool_use".to_owned();
                if let Some(id) = item["id"].as_str() {
                    if !block.id.is_empty() && block.id != id {
                        return Err(error("provider_tool_id_changed"));
                    }
                    block.id = id.to_owned();
                }
                if let Some(kind) = item["type"].as_str() {
                    if kind != "function" {
                        return Err(error("provider_hosted_tool_denied"));
                    }
                }
                if let Some(name) = item["function"]["name"].as_str() {
                    if block.name.is_empty() {
                        block.name = name.to_owned();
                    } else if block.name != name {
                        return Err(error("provider_tool_name_changed"));
                    }
                }
                if let Some(arguments) = item["function"]["arguments"].as_str() {
                    append_args(block, arguments)?;
                }
            }
        }
        if let Some(reason) = choice["finish_reason"].as_str() {
            if self.reason.replace(reason.to_owned()).is_some() {
                return Err(error("provider_duplicate_finish"));
            }
            for block in self.blocks.values_mut() {
                block.closed = true;
            }
        }
        Ok(())
    }
    fn responses(
        &mut self,
        value: Value,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<(), ModelError> {
        match value["type"].as_str() {
            Some("response.created" | "response.in_progress") => {
                self.started = true;
            }
            Some("response.output_item.added") => {
                let n = index(&value, "output_index")?;
                if self.blocks.contains_key(&n) {
                    return Err(error("provider_duplicate_block"));
                }
                let item = &value["item"];
                let kind = required(item, "type")?;
                if !matches!(kind, "message" | "function_call") {
                    return Err(error("provider_private_or_hosted_item_unsupported"));
                }
                self.blocks.insert(
                    n,
                    Block {
                        kind: if kind == "message" {
                            "text"
                        } else {
                            "tool_use"
                        }
                        .to_owned(),
                        id: required(item, "id")?.to_owned(),
                        name: item["name"].as_str().unwrap_or("").to_owned(),
                        input: json!({"call_id":item["call_id"]}),
                        ..Default::default()
                    },
                );
            }
            Some("response.output_text.delta") => {
                let n = index(&value, "output_index")?;
                verify_item(&self.blocks, n, &value)?;
                self.text(
                    n,
                    value["delta"]
                        .as_str()
                        .ok_or_else(|| error("provider_text_invalid"))?,
                    sink,
                )?;
            }
            Some("response.function_call_arguments.delta") => {
                let n = index(&value, "output_index")?;
                verify_item(&self.blocks, n, &value)?;
                let block = self.blocks.get_mut(&n).unwrap();
                if block.kind != "tool_use" {
                    return Err(error("provider_delta_type_mismatch"));
                }
                append_args(
                    block,
                    value["delta"]
                        .as_str()
                        .ok_or_else(|| error("provider_tool_json_invalid"))?,
                )?;
            }
            Some("response.output_item.done") => {
                let n = index(&value, "output_index")?;
                let block = self
                    .blocks
                    .get_mut(&n)
                    .ok_or_else(|| error("provider_stop_without_block"))?;
                if block.closed || value["item"]["id"] != block.id {
                    return Err(error("provider_item_identity_mismatch"));
                }
                if block.kind == "tool_use"
                    && (value["item"]["arguments"].as_str() != Some(block.args.as_str())
                        || value["item"]["call_id"] != block.input["call_id"])
                {
                    return Err(error("provider_final_item_mismatch"));
                }
                block.closed = true;
            }
            Some("response.completed") => {
                if self.blocks.values().any(|b| !b.closed) {
                    return Err(error("provider_finish_with_open_blocks"));
                }
                self.final_value = Some(value["response"].clone());
                self.finished = true;
            }
            Some("response.failed" | "response.incomplete" | "error") => {
                return Err(error("provider_response_incomplete"))
            }
            Some("response.refusal.delta" | "response.refusal.done") => {
                return Err(error("model_refused"))
            }
            Some(
                "response.content_part.added"
                | "response.content_part.done"
                | "response.output_text.done"
                | "response.function_call_arguments.done",
            ) => {}
            _ => return Err(error("provider_unknown_required_event")),
        }
        Ok(())
    }
    fn ollama(
        &mut self,
        value: Value,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<(), ModelError> {
        if value.get("error").is_some() {
            return Err(error("provider_stream_error"));
        }
        if let Some(text) = value["message"]["content"].as_str() {
            self.text(0, text, sink)?;
        }
        if let Some(items) = value["message"].get("tool_calls") {
            for item in items
                .as_array()
                .ok_or_else(|| error("provider_tool_calls_invalid"))?
            {
                let n = self
                    .blocks
                    .keys()
                    .copied()
                    .filter(|v| *v >= 1000)
                    .max()
                    .map_or(1000, |v| v + 1);
                if n >= 1032 {
                    return Err(error("provider_tool_batch_too_large"));
                }
                self.blocks.insert(
                    n,
                    Block {
                        kind: "tool_use".to_owned(),
                        id: item["id"].as_str().unwrap_or("").to_owned(),
                        name: required(&item["function"], "name")?.to_owned(),
                        input: parse_arguments(&item["function"]["arguments"])?,
                        closed: true,
                        ..Default::default()
                    },
                );
            }
        }
        if value["done"] == true {
            self.reason = Some(required(&value, "done_reason")?.to_owned());
            self.input = value["prompt_eval_count"].as_u64();
            self.output = value["eval_count"].as_u64();
            self.model = value["model"].as_str().map(str::to_owned);
            self.finished = true;
        }
        Ok(())
    }
    fn gemini(
        &mut self,
        value: Value,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<(), ModelError> {
        match value["event_type"].as_str() {
            Some("interaction.created") => {
                if self.started {
                    return Err(error("provider_duplicate_message_start"));
                }
                self.started = true;
                self.id = Some(required(&value["interaction"], "id")?.to_owned());
                self.model = value["interaction"]["model"].as_str().map(str::to_owned);
            }
            Some("interaction.status_update") => {
                if !self.started || value["interaction_id"].as_str() != self.id.as_deref() {
                    return Err(error("provider_response_id_changed"));
                }
                if value["status"] != "in_progress" {
                    return Err(error("provider_unexpected_status_update"));
                }
            }
            Some("step.start") => {
                if !self.started {
                    return Err(error("provider_block_outside_message"));
                }
                let n = index(&value, "index")?;
                if self.blocks.contains_key(&n) {
                    return Err(error("provider_duplicate_block"));
                }
                let item = &value["step"];
                match required(item, "type")? {
                    "model_output" => {
                        self.blocks.insert(
                            n,
                            Block {
                                kind: "text".to_owned(),
                                ..Default::default()
                            },
                        );
                        if let Some(content) = item["content"].as_array() {
                            for block in content {
                                if block["type"] != "text" {
                                    return Err(error("provider_content_block_unsupported"));
                                }
                                self.text(
                                    n,
                                    block["text"]
                                        .as_str()
                                        .ok_or_else(|| error("provider_text_invalid"))?,
                                    sink,
                                )?;
                            }
                        }
                    }
                    "function_call" => {
                        self.blocks.insert(
                            n,
                            Block {
                                kind: "tool_use".to_owned(),
                                id: required(item, "id")?.to_owned(),
                                name: required(item, "name")?.to_owned(),
                                input: item["arguments"].clone(),
                                ..Default::default()
                            },
                        );
                    }
                    "thought" => return Err(error("provider_private_replay_not_configured")),
                    _ => return Err(error("provider_private_or_hosted_item_unsupported")),
                }
            }
            Some("step.delta") => {
                let n = index(&value, "index")?;
                let block = self
                    .blocks
                    .get(&n)
                    .ok_or_else(|| error("provider_delta_without_block"))?;
                if block.closed {
                    return Err(error("provider_delta_after_block_stop"));
                }
                match value["delta"]["type"].as_str() {
                    Some("text") if block.kind == "text" => self.text(
                        n,
                        value["delta"]["text"]
                            .as_str()
                            .ok_or_else(|| error("provider_text_invalid"))?,
                        sink,
                    )?,
                    Some("arguments_delta") if block.kind == "tool_use" => append_args(
                        self.blocks.get_mut(&n).unwrap(),
                        value["delta"]["arguments"]
                            .as_str()
                            .ok_or_else(|| error("provider_tool_json_invalid"))?,
                    )?,
                    _ => return Err(error("provider_delta_type_mismatch")),
                }
            }
            Some("step.stop") => {
                let block = self
                    .blocks
                    .get_mut(&index(&value, "index")?)
                    .ok_or_else(|| error("provider_stop_without_block"))?;
                if block.closed {
                    return Err(error("provider_duplicate_block_stop"));
                }
                block.closed = true;
            }
            Some("interaction.completed") => {
                if !self.started || self.blocks.values().any(|b| !b.closed) {
                    return Err(error("provider_finish_with_open_blocks"));
                }
                let interaction = &value["interaction"];
                if interaction["id"].as_str() != self.id.as_deref() {
                    return Err(error("provider_response_id_changed"));
                }
                self.reason = Some(
                    match required(interaction, "status")? {
                        "completed" => "end_turn",
                        "requires_action" => "tool_use",
                        _ => return Err(error("provider_response_incomplete")),
                    }
                    .to_owned(),
                );
                if let Some(u) = gemini_usage(&interaction["usage"])? {
                    self.input = Some(u.input_tokens);
                    self.output = Some(u.output_tokens);
                }
                self.finished = true;
            }
            Some("error" | "interaction.failed" | "interaction.cancelled") => {
                return Err(error("provider_stream_error"))
            }
            _ => return Err(error("provider_unknown_required_event")),
        }
        Ok(())
    }
    pub(crate) fn finish(self, prepared: &PreparedModelCall) -> Result<ModelReply, ModelError> {
        if !self.finished {
            return Err(error("provider_stream_incomplete"));
        }
        if let Some(value) = self.final_value {
            let result = decode(value, prepared)?;
            let text = self
                .blocks
                .values()
                .filter(|b| b.kind == "text")
                .map(|b| b.text.as_str())
                .collect::<String>();
            if text != result.output.text {
                return Err(error("provider_final_text_mismatch"));
            }
            let tools = self
                .blocks
                .values()
                .filter(|b| b.kind == "tool_use")
                .map(|block| {
                    call(
                        block.input["call_id"].as_str().unwrap_or("").to_owned(),
                        &block.name,
                        parse_arguments(&json!(block.args))?,
                        prepared,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            if tools != result.output.tool_calls {
                return Err(error("provider_final_tool_mismatch"));
            }
            return Ok(result);
        }
        let mut text = String::new();
        let mut tools = Vec::new();
        for (ordinal, block) in self.blocks.into_values().enumerate() {
            if block.kind == "text" {
                text.push_str(&block.text);
            } else {
                let input = if block.args.is_empty() {
                    block.input
                } else {
                    parse_arguments(&json!(block.args))?
                };
                let id = if block.id.is_empty() && self.protocol == ModelProtocol::OllamaChat {
                    format!("ollama:{}:{ordinal}", prepared.spec.call_id)
                } else {
                    block.id
                };
                tools.push(call(id, &block.name, input, prepared)?);
            }
        }
        validate_model_calls(&tools)?;
        let mut reason = self
            .reason
            .ok_or_else(|| error("provider_stop_reason_missing"))?;
        if self.protocol == ModelProtocol::OllamaChat && reason == "stop" && !tools.is_empty() {
            reason = "tool_use".to_owned();
        }
        let finish = ModelFinish::parse(Some(&reason), !tools.is_empty(), false)?;
        finish.require_complete()?;
        let output = ModelOutput {
            text,
            tool_calls: tools,
            usage: self
                .input
                .zip(self.output)
                .map(|(input_tokens, output_tokens)| ModelUsage {
                    input_tokens,
                    output_tokens,
                }),
            stop_reason: Some(reason),
            model_id: self.model.or_else(|| Some(prepared.route.model_id.clone())),
            content: Vec::new(),
            continuation: None,
        };
        let mut result = ModelReply {
            output,
            finish,
            structured: None,
            replay: Vec::new(),
            provider_request_id: None,
            provider_response_id: self.id,
        };
        if result.output.tool_calls.is_empty() {
            match &prepared.spec.response_format {
                ModelResponseFormat::Text => {}
                format => {
                    let value: Value = serde_json::from_str(&result.output.text)
                        .map_err(|_| error("model_structured_output_invalid_json"))?;
                    match format {
                        ModelResponseFormat::JsonObject => {
                            if !value.is_object() {
                                return Err(error("model_structured_output_object_required"));
                            }
                        }
                        ModelResponseFormat::JsonSchema { schema, .. } => {
                            crate::request::validate_output(schema, &value)?
                        }
                        _ => {}
                    }
                    result.structured = Some(value);
                }
            }
        }
        Ok(result)
    }
}
fn index(value: &Value, key: &str) -> Result<usize, ModelError> {
    value[key]
        .as_u64()
        .filter(|v| *v < 4096)
        .map(|n| n as usize)
        .ok_or_else(|| error("provider_block_index_invalid"))
}
fn append_args(block: &mut Block, part: &str) -> Result<(), ModelError> {
    if block.closed || block.args.len().saturating_add(part.len()) > 256 * 1024 {
        return Err(error("provider_tool_arguments_limit_or_closed"));
    }
    block.args.push_str(part);
    Ok(())
}
fn verify_item(
    blocks: &BTreeMap<usize, Block>,
    index: usize,
    value: &Value,
) -> Result<(), ModelError> {
    let block = blocks
        .get(&index)
        .ok_or_else(|| error("provider_delta_without_block"))?;
    if value["item_id"] != block.id || block.closed {
        return Err(error("provider_item_identity_mismatch"));
    }
    Ok(())
}

fn gemini_usage(value: &Value) -> Result<Option<ModelUsage>, ModelError> {
    let Some(mut measured) = usage(value, "total_input_tokens", "total_output_tokens")? else {
        return Ok(None);
    };
    if let Some(thought) = value.get("total_thought_tokens") {
        measured.output_tokens = measured
            .output_tokens
            .checked_add(
                thought
                    .as_u64()
                    .ok_or_else(|| error("provider_usage_invalid"))?,
            )
            .ok_or_else(|| error("provider_usage_overflow"))?;
    }
    if let Some(total) = value.get("total_tokens") {
        let total = total
            .as_u64()
            .ok_or_else(|| error("provider_usage_invalid"))?;
        let known = measured
            .input_tokens
            .checked_add(measured.output_tokens)
            .ok_or_else(|| error("provider_usage_overflow"))?;
        if total < known {
            return Err(error("provider_usage_total_inconsistent"));
        }
        // Internal tokens omitted from visible output remain charged to this attempt.
        measured.output_tokens = total - measured.input_tokens;
    }
    Ok(Some(measured))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink<'a>(
        deltas: &'a mut Vec<ModelDelta>,
    ) -> impl FnMut(ModelDelta) -> Result<(), String> + 'a {
        move |delta| {
            deltas.push(delta);
            Ok(())
        }
    }

    #[test]
    fn terminal_finishes_without_socket_eof() {
        let mut accumulator = Accumulator::new(ModelProtocol::OpenAiChat);
        let mut deltas = Vec::new();
        assert!(!accumulator
            .push(
                r#"{"id":"response-1","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap());
        assert!(!accumulator
            .push(
                r#"{"id":"response-1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap());
        assert!(accumulator.push("[DONE]", &mut sink(&mut deltas)).unwrap());
        assert_eq!(
            deltas
                .iter()
                .filter(|delta| matches!(delta, ModelDelta::Text { .. }))
                .count(),
            1
        );
    }

    #[test]
    fn identical_text_deltas_are_not_deduplicated() {
        let mut accumulator = Accumulator::new(ModelProtocol::OpenAiChat);
        let mut deltas = Vec::new();
        for text in ["same", "same"] {
            accumulator
                .push(
                    &format!(
                        r#"{{"choices":[{{"index":0,"delta":{{"content":"{text}"}},"finish_reason":null}}]}}"#
                    ),
                    &mut sink(&mut deltas),
                )
                .unwrap();
        }
        let texts = deltas
            .into_iter()
            .filter_map(|delta| match delta {
                ModelDelta::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(texts, vec!["same", "same"]);
    }

    #[test]
    fn closed_tool_block_cannot_receive_more_arguments() {
        let mut accumulator = Accumulator::new(ModelProtocol::AnthropicMessages);
        let mut deltas = Vec::new();
        accumulator
            .push(
                r#"{"type":"message_start","message":{"id":"message-1","model":"fixture","usage":{"input_tokens":1,"output_tokens":0}}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call-1","name":"shell","input":{}}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"content_block_stop","index":0}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        let error = accumulator
            .push(
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap_err();
        assert_eq!(error.code, "provider_delta_after_block_stop");
    }

    #[test]
    fn terminal_with_open_blocks_is_rejected() {
        let mut accumulator = Accumulator::new(ModelProtocol::AnthropicMessages);
        let mut deltas = Vec::new();
        accumulator
            .push(
                r#"{"type":"message_start","message":{"id":"message-1","model":"fixture","usage":{"input_tokens":1,"output_tokens":0}}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hello"}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        let error = accumulator
            .push(
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap_err();
        assert_eq!(error.code, "provider_finish_with_open_blocks");
    }

    #[test]
    fn done_without_finish_and_late_delta_fail_closed() {
        let mut early_done = Accumulator::new(ModelProtocol::OpenAiChat);
        let mut deltas = Vec::new();
        assert_eq!(
            early_done
                .push("[DONE]", &mut sink(&mut deltas))
                .unwrap_err()
                .code,
            "provider_done_without_finish"
        );

        let mut late = Accumulator::new(ModelProtocol::OpenAiChat);
        late.push(
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
            &mut sink(&mut deltas),
        )
        .unwrap();
        let error = late
            .push(
                r#"{"choices":[{"index":0,"delta":{"content":"late"},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap_err();
        assert_eq!(error.code, "provider_delta_after_finish");
    }

    fn anthropic_prepared() -> PreparedModelCall {
        let tools = tool_schemas();
        let mut prepared = PreparedModelCall {
            schema: MODEL_CALL_SCHEMA.to_owned(),
            spec: ModelCallSpec {
                call_id: RequestId::new(),
                attempt_id: RequestId::new(),
                model_attempt_id: None,
                step_id: None,
                step: 1,
                purpose: ModelPurpose::Task,
                assignment: None,
                response_format: ModelResponseFormat::Text,
                replay: Vec::new(),
                deadline_unix_ms: u64::MAX,
            },
            route: ModelRoute {
                provider_id: "anthropic".to_owned(),
                protocol: ModelProtocol::AnthropicMessages,
                connection_id: "fixture".to_owned(),
                model_id: "claude-fixture".to_owned(),
                profile: "default".to_owned(),
                configuration_revision: "fixture.v1".to_owned(),
                streaming: true,
            },
            request: ModelRequest {
                messages: Vec::new(),
                tools: tools.clone(),
                sandbox: "read-only".to_owned(),
            },
            wire_body: serde_json::json!({}),
            request_hash: String::new(),
            budget: TokenBudget::new(1, 1, 1, 1, 4_096),
            tool_catalog_hash: tool_catalog_hash(&tools),
            provider_account: None,
            credential_revision: None,
        };
        prepared.seal();
        prepared
    }

    #[test]
    fn anthropic_text_tool_result_and_final_answer_decode_without_private_replay() {
        let prepared = anthropic_prepared();
        let reply = decode(
            serde_json::json!({
                "id": "message-1",
                "model": "claude-fixture",
                "content": [
                    {"type": "text", "text": "use the tool"},
                    {"type": "tool_use", "id": "call-1", "name": "shell", "input": {"command": "pwd"}}
                ],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 4, "output_tokens": 3}
            }),
            &prepared,
        )
        .unwrap();
        assert_eq!(reply.output.text, "use the tool");
        assert_eq!(reply.output.tool_calls[0].id, "call-1");
        assert_eq!(reply.output.tool_calls[0].name, "shell");
        assert_eq!(reply.output.tool_calls[0].arguments["command"], "pwd");
        assert_eq!(reply.output.usage.unwrap().output_tokens, 3);
    }

    #[test]
    fn anthropic_stream_requires_message_stop_and_accumulates_usage() {
        let prepared = anthropic_prepared();
        let mut accumulator = Accumulator::new(ModelProtocol::AnthropicMessages);
        let mut deltas = Vec::new();
        accumulator
            .push(
                r#"{"type":"message_start","message":{"id":"message-1","model":"claude-fixture","usage":{"input_tokens":4,"output_tokens":0}}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello"}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"content_block_stop","index":0}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        assert!(accumulator
            .push(r#"{"type":"message_stop"}"#, &mut sink(&mut deltas),)
            .unwrap());
        let reply = accumulator.finish(&prepared).unwrap();
        assert_eq!(reply.output.text, "hello");
        assert_eq!(reply.output.usage.unwrap().output_tokens, 2);
    }

    #[test]
    fn anthropic_usage_regression_is_rejected() {
        let mut accumulator = Accumulator::new(ModelProtocol::AnthropicMessages);
        let mut deltas = Vec::new();
        accumulator
            .push(
                r#"{"type":"message_start","message":{"id":"message-1","model":"claude-fixture","usage":{"input_tokens":4,"output_tokens":0}}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"type":"message_delta","usage":{"output_tokens":3}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        let error = accumulator
            .push(
                r#"{"type":"message_delta","usage":{"output_tokens":2}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap_err();
        assert_eq!(error.code, "provider_usage_regressed");
    }

    #[test]
    fn openai_usage_only_chunk_is_retained() {
        let mut accumulator = Accumulator::new(ModelProtocol::OpenAiChat);
        let mut deltas = Vec::new();
        accumulator
            .push(
                r#"{"id":"response-1","choices":[],"usage":{"prompt_tokens":2,"completion_tokens":3}}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"id":"response-1","choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        accumulator
            .push(
                r#"{"id":"response-1","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        assert!(accumulator.push("[DONE]", &mut sink(&mut deltas)).unwrap());
    }

    #[test]
    fn openai_interleaved_tool_ids_and_names_cannot_swap() {
        let mut accumulator = Accumulator::new(ModelProtocol::OpenAiChat);
        let mut deltas = Vec::new();
        accumulator
            .push(
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-a","type":"function","function":{"name":"shell","arguments":"{"}}]},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        let id_error = accumulator
            .push(
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-b","type":"function","function":{"arguments":"}"}}]},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap_err();
        assert_eq!(id_error.code, "provider_tool_id_changed");

        let mut names = Accumulator::new(ModelProtocol::OpenAiChat);
        names
            .push(
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-a","type":"function","function":{"name":"shell","arguments":"{"}}]},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap();
        let name_error = names
            .push(
                r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"type":"function","function":{"name":"mcp"}}]},"finish_reason":null}]}"#,
                &mut sink(&mut deltas),
            )
            .unwrap_err();
        assert_eq!(name_error.code, "provider_tool_name_changed");
    }

    #[test]
    fn malformed_openai_tool_arguments_never_become_empty_object() {
        assert_eq!(
            parse_arguments(&serde_json::json!("not-json"))
                .unwrap_err()
                .code,
            "provider_tool_json_invalid"
        );
    }
}
