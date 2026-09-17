//! One bounded accumulator for streamed and non-streamed model responses.
//!
//! Providers may deliver text, tool-argument, usage and stop fragments in different orders. The
//! accumulator owns that assembly and emits a complete `ModelOutput` only after identities,
//! limits, JSON arguments and terminal markers agree. It has no model, Broker or EventLog access.

use kiana_domain::{
    parse_bounded_json, validate_model_calls, ModelAttemptId, ModelDelta, ModelOutput,
    ModelToolCall, ModelUsage,
};
use std::collections::BTreeMap;

pub const STREAM_NORMALIZER_MAX_DELTAS: usize = 4_096;
pub const STREAM_NORMALIZER_MAX_TEXT_BYTES: usize = 512 * 1024;
pub const STREAM_NORMALIZER_MAX_TOOL_BYTES: usize = 256 * 1024;
pub const STREAM_NORMALIZER_MAX_TOOL_BLOCKS: usize = 32;

#[derive(Clone, Debug)]
struct ToolBuffer {
    id: String,
    name: String,
    partial_json: String,
}

/// Deterministic per-attempt stream accumulator. A cancelled/terminal accumulator cannot accept
/// late deltas, so UI output and tool dispatch cannot diverge from the complete response.
#[derive(Clone, Debug)]
pub struct ModelStreamAccumulator {
    attempt_id: ModelAttemptId,
    text: String,
    text_seen: bool,
    tools: BTreeMap<u32, ToolBuffer>,
    usage: Option<ModelUsage>,
    stop_reason: Option<String>,
    delta_count: usize,
    cancelled: bool,
}

impl ModelStreamAccumulator {
    pub fn new(attempt_id: ModelAttemptId) -> Self {
        Self {
            attempt_id,
            text: String::new(),
            text_seen: false,
            tools: BTreeMap::new(),
            usage: None,
            stop_reason: None,
            delta_count: 0,
            cancelled: false,
        }
    }

    pub fn attempt_id(&self) -> ModelAttemptId {
        self.attempt_id
    }

    pub fn cancel(&mut self) {
        self.cancelled = true;
    }

    pub fn push(&mut self, delta: ModelDelta) -> Result<(), String> {
        if self.cancelled {
            return Err("late_delta_after_cancel_is_discarded".to_owned());
        }
        if self.stop_reason.is_some() {
            return Err("stream_delta_after_stop".to_owned());
        }
        self.delta_count = self.delta_count.saturating_add(1);
        if self.delta_count > STREAM_NORMALIZER_MAX_DELTAS {
            return Err("stream_delta_limit".to_owned());
        }
        match delta {
            ModelDelta::Text { text } => {
                if self.text.len().saturating_add(text.len()) > STREAM_NORMALIZER_MAX_TEXT_BYTES {
                    return Err("stream_text_limit".to_owned());
                }
                self.text_seen = true;
                self.text.push_str(&text);
            }
            ModelDelta::ToolArguments {
                index,
                id,
                name,
                partial_json,
            } => {
                if index as usize >= STREAM_NORMALIZER_MAX_TOOL_BLOCKS
                    || id.trim().is_empty()
                    || id.len() > 256
                    || name.trim().is_empty()
                    || name.len() > 256
                    || partial_json.len() > STREAM_NORMALIZER_MAX_TOOL_BYTES
                {
                    return Err("stream_tool_fragment_invalid".to_owned());
                }
                let entry = self.tools.entry(index).or_insert_with(|| ToolBuffer {
                    id: id.clone(),
                    name: name.clone(),
                    partial_json: String::new(),
                });
                if entry.id != id || entry.name != name {
                    return Err("stream_tool_identity_conflict".to_owned());
                }
                if entry.partial_json.len().saturating_add(partial_json.len())
                    > STREAM_NORMALIZER_MAX_TOOL_BYTES
                {
                    return Err("stream_tool_arguments_limit".to_owned());
                }
                entry.partial_json.push_str(&partial_json);
            }
            ModelDelta::Usage { usage } => {
                if let Some(previous) = &self.usage {
                    if usage.input_tokens < previous.input_tokens
                        || usage.output_tokens < previous.output_tokens
                    {
                        return Err("stream_usage_regressed".to_owned());
                    }
                }
                self.usage = Some(usage);
            }
            ModelDelta::Stop { reason } => {
                if reason.trim().is_empty() || reason.len() > 128 {
                    return Err("stream_stop_invalid".to_owned());
                }
                if self.stop_reason.replace(reason).is_some() {
                    return Err("stream_duplicate_stop".to_owned());
                }
            }
            _ => return Err("model_delta_unsupported".to_owned()),
        }
        Ok(())
    }

    pub fn finish(mut self, mut output: ModelOutput) -> Result<ModelOutput, String> {
        if self.cancelled {
            return Err("stream_cancelled_before_complete".to_owned());
        }
        if self.text_seen && output.text != self.text {
            return Err("stream_text_aggregate_mismatch".to_owned());
        }
        if !self.text_seen
            && self.text.is_empty()
            && output.text.len() > STREAM_NORMALIZER_MAX_TEXT_BYTES
        {
            return Err("stream_text_limit".to_owned());
        }
        if let Some(reason) = &self.stop_reason {
            if output
                .stop_reason
                .as_ref()
                .is_some_and(|existing| existing != reason)
            {
                return Err("stream_stop_conflict".to_owned());
            }
            output.stop_reason = Some(reason.clone());
        }
        if output.stop_reason.is_none() {
            return Err("eof_without_stop_never_completes".to_owned());
        }
        if let (Some(observed), Some(reported)) = (&self.usage, &output.usage) {
            if reported.input_tokens < observed.input_tokens
                || reported.output_tokens < observed.output_tokens
            {
                return Err("stream_usage_regressed".to_owned());
            }
        } else if output.usage.is_none() {
            output.usage = self.usage.take();
        }

        if !self.tools.is_empty() {
            let mut calls = Vec::with_capacity(self.tools.len());
            for (index, buffer) in self.tools {
                let arguments = parse_bounded_json(buffer.partial_json.as_bytes())
                    .map_err(|_| format!("split_invalid_tool_json_has_zero_dispatches:{index}"))?;
                if !arguments.is_object() {
                    return Err(format!("stream_tool_arguments_object_required:{index}"));
                }
                calls.push(ModelToolCall {
                    id: buffer.id,
                    name: buffer.name,
                    arguments,
                });
            }
            if output.tool_calls.is_empty() {
                output.tool_calls = calls;
            } else if output.tool_calls != calls {
                return Err("stream_tool_aggregate_mismatch".to_owned());
            }
        }
        validate_model_calls(&output.tool_calls).map_err(|error| error.to_string())?;
        output.content_blocks().map_err(|error| error.to_string())?;
        let has_tools = !output.tool_calls.is_empty()
            || output
                .content
                .iter()
                .any(|block| matches!(block, kiana_domain::ModelContent::ToolCall { .. }));
        kiana_domain::ModelFinish::parse(output.stop_reason.as_deref(), has_tools, false)
            .map_err(|error| error.to_string())?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::ModelToolCall;
    use serde_json::json;

    #[test]
    fn interleaved_tool_deltas_equal_nonstream_response() {
        let mut accumulator = ModelStreamAccumulator::new(ModelAttemptId::new());
        accumulator
            .push(ModelDelta::Text {
                text: "before".to_owned(),
            })
            .unwrap();
        accumulator
            .push(ModelDelta::ToolArguments {
                index: 0,
                id: "call-1".to_owned(),
                name: "shell".to_owned(),
                partial_json: "{\"command\":\"ls\"}".to_owned(),
            })
            .unwrap();
        accumulator
            .push(ModelDelta::Stop {
                reason: "tool_use".to_owned(),
            })
            .unwrap();
        let output = accumulator
            .finish(ModelOutput {
                text: "before".to_owned(),
                tool_calls: vec![ModelToolCall {
                    id: "call-1".to_owned(),
                    name: "shell".to_owned(),
                    arguments: json!({"command":"ls"}),
                }],
                stop_reason: Some("tool_use".to_owned()),
                ..ModelOutput::default()
            })
            .unwrap();
        assert_eq!(output.text, "before");
        assert_eq!(output.tool_calls.len(), 1);
    }

    #[test]
    fn invalid_json_eof_and_late_cancel_are_rejected() {
        let mut invalid = ModelStreamAccumulator::new(ModelAttemptId::new());
        invalid
            .push(ModelDelta::ToolArguments {
                index: 0,
                id: "call-1".to_owned(),
                name: "shell".to_owned(),
                partial_json: "{\"command\":".to_owned(),
            })
            .unwrap();
        invalid
            .push(ModelDelta::Stop {
                reason: "tool_use".to_owned(),
            })
            .unwrap();
        assert!(invalid
            .finish(ModelOutput {
                stop_reason: Some("tool_use".to_owned()),
                ..ModelOutput::default()
            })
            .unwrap_err()
            .starts_with("split_invalid_tool_json_has_zero_dispatches"));

        let mut eof = ModelStreamAccumulator::new(ModelAttemptId::new());
        eof.push(ModelDelta::Text {
            text: "partial".to_owned(),
        })
        .unwrap();
        assert_eq!(
            eof.finish(ModelOutput {
                text: "partial".to_owned(),
                ..ModelOutput::default()
            })
            .unwrap_err(),
            "eof_without_stop_never_completes"
        );

        let mut cancelled = ModelStreamAccumulator::new(ModelAttemptId::new());
        cancelled.cancel();
        assert_eq!(
            cancelled
                .push(ModelDelta::Text {
                    text: "late".to_owned(),
                })
                .unwrap_err(),
            "late_delta_after_cancel_is_discarded"
        );
    }
}
