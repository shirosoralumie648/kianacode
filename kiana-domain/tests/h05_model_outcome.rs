use kiana_domain::{
    ModelError, ModelFinish, ModelMessage, ModelOutput, ModelRecoveryDisposition, ModelReply,
    ModelRetryClass, ModelSideEffectState, ModelStopReason, RetryDenyReason, RetryObservation,
    RetryPolicy,
};

#[test]
fn length_stop_is_rejected_by_legacy_reply_conversion() {
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
fn explicit_incomplete_stop_is_not_success_even_with_tool_calls() {
    let mut output = ModelOutput::with_tool(
        "partial response",
        "shell",
        serde_json::json!({"command": "echo blocked"}),
    );
    output.stop_reason = Some("incomplete".to_owned());

    assert_eq!(output.normalized_stop_reason(), ModelStopReason::Incomplete);
    let error = ModelReply::legacy(output).unwrap_err();
    assert_eq!(error.code, "model_transport_incomplete");
    assert_eq!(error.outcome().stop_reason, ModelStopReason::Incomplete);
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

#[test]
fn recovery_dispositions_are_typed_and_survive_error_round_trips() {
    for disposition in [
        ModelRecoveryDisposition::TransportRetry,
        ModelRecoveryDisposition::FormatRepair,
        ModelRecoveryDisposition::ToolRepair,
        ModelRecoveryDisposition::ContextRepair,
        ModelRecoveryDisposition::Terminal,
    ] {
        let error = if disposition == ModelRecoveryDisposition::TransportRetry {
            ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true)
        } else {
            ModelError::invalid("classified_failure")
        }
        .with_recovery_disposition(disposition);
        let encoded = serde_json::to_value(&error).unwrap();
        let decoded: ModelError = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.recovery_disposition, disposition);

        decoded.outcome().validate().unwrap();
    }
}

#[test]
fn legacy_model_error_and_retry_observation_fail_closed_without_disposition() {
    let legacy = serde_json::json!({
        "code": "provider_http_429",
        "phase": "transport",
        "retry_class": "rejected",
        "request_sent": true,
        "retry_after_ms": null,
        "safe_message": "provider_http_429",
        "side_effect_state": "none"
    });
    let error: ModelError = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        error.recovery_disposition,
        ModelRecoveryDisposition::Terminal
    );
    let observation = RetryObservation::from_model_error(&error, false, true);
    let decision = RetryPolicy::new(3, 3, 5_000, 10_000)
        .unwrap()
        .classify(1, 1, 1_000, &observation)
        .unwrap();
    assert!(!decision.retry);
    assert_eq!(
        decision.deny_reason,
        Some(RetryDenyReason::RecoveryDispositionNotRetryable)
    );

    let legacy_observation = serde_json::json!({
        "class": "rejected",
        "code": "provider_http_429",
        "request_sent": true,
        "side_effect_state": "none",
        "observed_delta": false,
        "idempotent": true,
        "retry_after_ms": null
    });
    let observation: RetryObservation = serde_json::from_value(legacy_observation).unwrap();
    assert!(RetryPolicy::new(3, 3, 5_000, 10_000)
        .unwrap()
        .classify(1, 1, 1_000, &observation)
        .is_err());
}

#[test]
fn unknown_recovery_disposition_is_rejected() {
    let mut encoded = serde_json::to_value(ModelError::invalid("future_failure")).unwrap();
    encoded["recovery_disposition"] = serde_json::json!("future_repair");
    assert!(serde_json::from_value::<ModelError>(encoded).is_err());
}

#[test]
fn retry_policy_denies_non_transport_recovery_dispositions() {
    let policy = RetryPolicy::new(3, 3, 5_000, 10_000).unwrap();
    let mut transport = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
    transport.side_effect_state = ModelSideEffectState::None;
    let transport_decision = policy
        .classify(
            1,
            1,
            1_000,
            &RetryObservation::from_model_error(&transport, false, true),
        )
        .unwrap();
    assert!(transport_decision.retry);

    for disposition in [
        ModelRecoveryDisposition::FormatRepair,
        ModelRecoveryDisposition::ToolRepair,
        ModelRecoveryDisposition::ContextRepair,
        ModelRecoveryDisposition::Terminal,
    ] {
        let mut error = transport.clone();
        error.recovery_disposition = disposition;
        let decision = policy
            .classify(
                1,
                1,
                1_000,
                &RetryObservation::from_model_error(&error, false, true),
            )
            .unwrap();
        assert!(!decision.retry, "{disposition:?} must not retry");
        assert_eq!(
            decision.deny_reason,
            Some(RetryDenyReason::RecoveryDispositionNotRetryable)
        );
    }

    let mut invalid_phase = transport;
    invalid_phase.phase = "validation".to_owned();
    let decision = policy
        .classify(
            1,
            1,
            1_000,
            &RetryObservation::from_model_error(&invalid_phase, false, true),
        )
        .unwrap();
    assert!(!decision.retry);
    assert_eq!(
        decision.deny_reason,
        Some(RetryDenyReason::RecoveryDispositionNotRetryable)
    );
}
