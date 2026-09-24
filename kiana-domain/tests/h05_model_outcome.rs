use kiana_domain::{
    ModelFinish, ModelMessage, ModelOutput, ModelReply, ModelRetryClass, ModelSideEffectState,
    ModelStopReason,
};

#[test]
fn length_stop_never_dispatches_tools_or_completes_turn() {
    let output = ModelOutput {
        text: "partial".to_owned(),
        tool_calls: vec![kiana_domain::ModelToolCall {
            id: "call-length".to_owned(),
            name: "shell".to_owned(),
            arguments: serde_json::json!({"command": "echo blocked"}),
        }],
        usage: None,
        stop_reason: Some("length".to_owned()),
        model_id: None,
        content: Vec::new(),
        continuation: None,
        structured: None,
    };
    assert_eq!(output.normalized_stop_reason(), ModelStopReason::Length);
    let error = ModelReply::legacy(output).unwrap_err();
    assert_eq!(error.code, "model_output_truncated");
    assert_eq!(error.outcome().stop_reason, ModelStopReason::Length);
}

#[test]
fn refusal_is_not_success() {
    let output = ModelOutput {
        text: "no".to_owned(),
        tool_calls: Vec::new(),
        usage: None,
        stop_reason: Some("refusal".to_owned()),
        model_id: None,
        content: Vec::new(),
        continuation: None,
        structured: None,
    };
    assert_eq!(output.normalized_stop_reason(), ModelStopReason::Refusal);
    let error = ModelReply::legacy(output).unwrap_err();
    assert_eq!(error.code, "model_refused");
    assert_eq!(error.side_effect_state, ModelSideEffectState::None);
}

#[test]
fn unknown_stop_reason_fails_closed() {
    let output = ModelOutput::text("ambiguous");
    let mut output = output;
    output.stop_reason = Some("future_stop".to_owned());
    assert_eq!(output.normalized_stop_reason(), ModelStopReason::Unknown);
    let error = ModelReply::legacy(output).unwrap_err();
    assert_eq!(error.code, "model_stop_reason_unknown");
    assert_eq!(error.retry_class, ModelRetryClass::Never);
}

#[test]
fn normal_text_stop_finishes_and_tool_stop_continues() {
    let text = ModelOutput {
        text: "done".to_owned(),
        stop_reason: Some("end_turn".to_owned()),
        ..ModelOutput::text("")
    };
    let text_reply = ModelReply::legacy(text).unwrap();
    assert_eq!(text_reply.finish, ModelFinish::EndTurn);

    let tool = ModelOutput::with_tool("run", "shell", serde_json::json!({"command": "pwd"}));
    let mut tool = tool;
    tool.stop_reason = Some("tool_use".to_owned());
    let tool_reply = ModelReply::legacy(tool).unwrap();
    assert_eq!(tool_reply.finish, ModelFinish::ToolUse);
    assert_eq!(tool_reply.output.tool_calls.len(), 1);
    let _ = ModelMessage::assistant("compatibility");
}
