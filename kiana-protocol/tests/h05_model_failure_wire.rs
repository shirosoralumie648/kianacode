//! H05 wire fixture：停止原因、恢复 disposition 与 retry class 到稳定 wire 码的映射。
//!
//! 本测试直接调用 `kiana-protocol` 的 DTO 构造器和 Serde，不经过 versioned request
//! envelope、Client/Daemon 或真实生命周期；它证明分类契约与会话结果一致，但不证明任何
//! 真实 provider 行为、自动修复执行或外部副作用。

use kiana_protocol::{
    CapabilityErrorCode, ExecutionStatus, ModelError, ModelFailureClass, ModelFailureSurface,
    ModelOutcome, ModelRecoveryDisposition, ModelRetryClass, ModelSideEffectState, ModelStopReason,
    RequestId, ResponseEnvelope, MODEL_FAILURE_SCHEMA, MODEL_FAILURE_UNCLASSIFIED_CODE,
    MODEL_OUTCOME_SCHEMA, PROTOCOL_SCHEMA,
};
use serde_json::Value;

fn envelope(status: ExecutionStatus, error: Option<&str>) -> ResponseEnvelope {
    ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status,
        output: Value::Null,
        error: error.map(str::to_owned),
    }
}

#[test]
fn stop_reasons_classify_to_stable_wire_codes_and_terminal_retry() {
    let cases = [
        (ModelStopReason::Length, "model_output_truncated"),
        (ModelStopReason::Refusal, "model_refused"),
        (ModelStopReason::Pause, "model_pause_requires_explicit_continue"),
        (ModelStopReason::Incomplete, "model_transport_incomplete"),
        (ModelStopReason::Unknown, "model_stop_reason_unknown"),
    ];
    for (stop, code) in cases {
        let class = ModelFailureClass::from_stop_reason(stop).expect("failure stop has a class");
        assert_eq!(class.stable_code(), Some(code), "{stop:?}");
        assert_eq!(ModelFailureClass::from_code(code), Some(class));
        let surface = ModelFailureSurface::from_code(code);
        assert_eq!(surface.failure, class, "{code}");
        assert_eq!(surface.wire_code, code, "{code}");
        assert_eq!(surface.stop_reason, stop, "{code}");
        assert_eq!(surface.disposition, ModelRecoveryDisposition::Terminal, "{code}");
        assert_eq!(surface.retry_class, ModelRetryClass::Never, "{code}");
        assert!(!surface.retryable, "no wire failure class authorizes automatic retry: {code}");
        assert!(surface.requires_new_authorization, "{code}");
        assert!(!surface.requires_reconciliation, "{code}");
        assert_eq!(surface.cli_exit, 1, "{code}");
        assert_eq!(surface.http_status, 500, "{code}");
        assert_eq!(surface.capability_error, CapabilityErrorCode::ExecutionFailed, "{code}");
        surface.validate().expect("stop surface validates");
    }
    for class in ModelFailureClass::ALL {
        if let Some(code) = class.stable_code() {
            assert_eq!(ModelFailureClass::from_code(code), Some(class), "{code}");
        }
    }
    assert_eq!(
        ModelFailureClass::from_code("model_output_truncated:detail"),
        Some(ModelFailureClass::Truncated)
    );
    assert_eq!(
        ModelFailureClass::from_code("port_failed:model_refused"),
        Some(ModelFailureClass::Refused)
    );
    assert_eq!(ModelFailureClass::from_stop_reason(ModelStopReason::EndTurn), None);
    assert_eq!(ModelFailureClass::from_stop_reason(ModelStopReason::ToolUse), None);
    assert!(ModelFailureSurface::try_from_code("end_turn").is_none());
}

#[test]
fn outcomes_resolve_success_to_none_and_stop_failures_to_their_wire_code() {
    let success = ModelOutcome {
        schema: MODEL_OUTCOME_SCHEMA.to_owned(),
        stop_reason: ModelStopReason::EndTurn,
        phase: "completed".to_owned(),
        retry_class: ModelRetryClass::Never,
        recovery_disposition: ModelRecoveryDisposition::Terminal,
        request_sent: true,
        side_effect_state: ModelSideEffectState::None,
        error_code: None,
        safe_message: String::new(),
    };
    assert!(ModelFailureSurface::from_outcome(&success).is_none());

    let mut truncated = ModelError::invalid("model_output_truncated");
    truncated.request_sent = true;
    let surface = ModelFailureSurface::from_outcome(&truncated.outcome())
        .expect("truncation is a failure surface");
    assert_eq!(surface.failure, ModelFailureClass::Truncated);
    assert_eq!(surface.wire_code, "model_output_truncated");
    assert_eq!(surface.stop_reason, ModelStopReason::Length);
    assert_eq!(surface.disposition, ModelRecoveryDisposition::Terminal);
    assert_eq!(surface.retry_class, ModelRetryClass::Never);
    assert_eq!(surface.request_sent, Some(true));
    assert_eq!(surface.side_effect_state, Some(ModelSideEffectState::None));
    assert!(!surface.retryable);
    surface.validate().expect("truncation surface validates");
}

#[test]
fn repair_dispositions_survive_outcome_projection_while_stop_failures_stay_terminal() {
    for (disposition, wire_code) in [
        (ModelRecoveryDisposition::FormatRepair, "model_format_repair_unavailable"),
        (ModelRecoveryDisposition::ToolRepair, "model_tool_repair_unavailable"),
        (ModelRecoveryDisposition::ContextRepair, "model_context_repair_unavailable"),
    ] {
        let mut error = ModelError::invalid("model_structured_output_invalid")
            .with_recovery_disposition(disposition);
        error.request_sent = true;
        let surface = ModelFailureSurface::from_outcome(&error.outcome())
            .expect("repair failures keep their disposition");
        assert_eq!(surface.failure.stable_code(), Some(wire_code), "{disposition:?}");
        assert_eq!(surface.wire_code, wire_code, "{disposition:?}");
        assert_eq!(surface.disposition, disposition, "{disposition:?}");
        assert_eq!(surface.retry_class, ModelRetryClass::Never, "{disposition:?}");
        // A repair surface must not widen the existing conservative error policy.
        assert_eq!(
            surface.capability_error,
            CapabilityErrorCode::ExecutionFailed,
            "{disposition:?}"
        );
        assert_eq!(surface.cli_exit, 1, "{disposition:?}");
        assert_eq!(surface.http_status, 500, "{disposition:?}");
        assert!(!surface.retryable, "repair is never an automatic retry: {disposition:?}");
        assert!(surface.requires_new_authorization, "{disposition:?}");
        surface.validate().expect("repair surface validates");
    }

    let mut terminal = ModelError::invalid("model_output_truncated");
    terminal.request_sent = true;
    let surface = ModelFailureSurface::from_outcome(&terminal.outcome())
        .expect("a stop failure is a failure surface");
    assert_eq!(surface.disposition, ModelRecoveryDisposition::Terminal);
    assert!(
        ModelFailureClass::from_repair_disposition(surface.disposition).is_none(),
        "non-repair failures must not claim a repair disposition"
    );
    assert_eq!(surface.capability_error, CapabilityErrorCode::ExecutionFailed);
}

#[test]
fn unknown_or_contradictory_failures_fail_closed_to_unclassified() {
    let unclassified = ModelFailureSurface::from_code("model_not_a_real_code");
    assert_eq!(unclassified, ModelFailureSurface::unclassified());
    assert_eq!(unclassified.failure, ModelFailureClass::Unclassified);
    assert_eq!(unclassified.wire_code, MODEL_FAILURE_UNCLASSIFIED_CODE);
    assert_eq!(unclassified.stop_reason, ModelStopReason::Unknown);
    assert_eq!(unclassified.disposition, ModelRecoveryDisposition::Terminal);
    assert_eq!(unclassified.retry_class, ModelRetryClass::Never);
    assert!(!unclassified.retryable);
    unclassified.validate().expect("deny-first surface validates");
    assert!(ModelFailureSurface::try_from_code("model_not_a_real_code").is_none());

    let mut malformed = ModelError::invalid("model_output_truncated").outcome();
    malformed.phase = "   ".to_owned();
    let surface = ModelFailureSurface::from_outcome(&malformed)
        .expect("a malformed outcome is still a failure");
    assert_eq!(surface, ModelFailureSurface::unclassified());

    let mut contradictory = ModelError::invalid("model_output_truncated").outcome();
    contradictory.recovery_disposition = ModelRecoveryDisposition::TransportRetry;
    contradictory.phase = "transport".to_owned();
    contradictory.retry_class = ModelRetryClass::BeforeSend;
    contradictory.request_sent = false;
    contradictory.side_effect_state = ModelSideEffectState::None;
    contradictory
        .validate()
        .expect("the contradictory fixture must be schema-valid to reach classification");
    let surface = ModelFailureSurface::from_outcome(&contradictory)
        .expect("stop failure plus transport retry must fail closed");
    assert_eq!(surface, ModelFailureSurface::unclassified());
}

#[test]
fn transport_retry_outcomes_keep_typed_retry_but_wire_never_authorizes_it() {
    let error = ModelError::transport("connect_failed", ModelRetryClass::BeforeSend, false);
    let surface = ModelFailureSurface::from_outcome(&error.outcome())
        .expect("a transport failure is a model failure surface");
    assert_eq!(surface.failure, ModelFailureClass::TransportRetry);
    assert_eq!(surface.disposition, ModelRecoveryDisposition::TransportRetry);
    assert_eq!(surface.retry_class, ModelRetryClass::BeforeSend);
    assert_eq!(surface.wire_code, "connect_failed");
    assert_eq!(surface.stop_reason, ModelStopReason::Unknown);
    assert!(!surface.retryable, "wire classification never authorizes a retry by itself");
    surface.validate().expect("transport surface validates");
}

#[test]
fn response_envelope_exposes_recognized_model_failures_without_collapsing_generic_errors() {
    let completed = envelope(ExecutionStatus::Completed, None);
    assert!(completed.model_failure().is_none());

    let cancelled = envelope(ExecutionStatus::Cancelled, Some("model_output_truncated"));
    assert!(
        cancelled.model_failure().is_none(),
        "lifecycle cancellation stays authoritative over the diagnostic code"
    );
    assert_eq!(cancelled.failure_code(), Some(CapabilityErrorCode::Cancelled));

    let failed = envelope(ExecutionStatus::Failed, Some("model_output_truncated"));
    let surface = failed.model_failure().expect("recognized stop failure");
    assert_eq!(surface.wire_code, "model_output_truncated");
    assert_eq!(surface.disposition, ModelRecoveryDisposition::Terminal);

    let repair = envelope(ExecutionStatus::Failed, Some("model_tool_repair_unavailable"));
    assert_eq!(
        repair.model_failure().map(|surface| surface.disposition),
        Some(ModelRecoveryDisposition::ToolRepair)
    );

    let generic = envelope(ExecutionStatus::Failed, Some("project_untrusted"));
    assert!(generic.model_failure().is_none());
    assert_eq!(
        generic.failure_code(),
        Some(CapabilityErrorCode::ProjectUntrusted),
        "the generic failure classification must stay unchanged"
    );

    let unknown = envelope(ExecutionStatus::Failed, Some("model_surprise_failure"));
    assert!(unknown.model_failure().is_none());
    assert_eq!(unknown.failure_code(), Some(CapabilityErrorCode::ExecutionFailed));
}

#[test]
fn wire_code_table_matches_the_runner_producers() {
    // 源守卫：wire 表里的稳定码必须仍由 runner 的停止门与 repair 映射实际产生；
    // 这里只读源码标记，不执行 runner/provider。
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let retry = include_str!("../../kiana-runner/src/retry.rs");
    for code in [
        "model_output_truncated",
        "model_refused",
        "model_pause_requires_explicit_continue",
        "model_transport_incomplete",
        "model_stop_reason_unknown",
    ] {
        assert!(
            harness.contains(code) || model.contains(code),
            "stop failure code lost its producer: {code}"
        );
    }
    for code in [
        "model_format_repair_unavailable",
        "model_tool_repair_unavailable",
        "model_context_repair_unavailable",
    ] {
        assert!(
            retry.contains(code),
            "repair disposition lost its fail-closed outcome code: {code}"
        );
    }
}

#[test]
fn model_failure_surface_roundtrips_as_bounded_wire_data() {
    let surface = ModelFailureSurface::from_code("model_format_repair_unavailable");
    let encoded = serde_json::to_value(&surface).expect("surface serializes");
    assert_eq!(encoded["schema"], MODEL_FAILURE_SCHEMA);
    assert_eq!(encoded["failure"], "format_repair");
    assert_eq!(encoded["wire_code"], "model_format_repair_unavailable");
    assert_eq!(encoded["disposition"], "format_repair");
    assert_eq!(encoded["retry_class"], "never");
    assert_eq!(encoded["capability_error"], "execution_failed");
    assert!(
        encoded.get("request_sent").is_none(),
        "a code-only surface carries no request evidence"
    );
    assert!(encoded.get("safe_message").is_none());
    assert!(encoded.get("provider_response_id").is_none());
    let decoded: ModelFailureSurface = serde_json::from_value(encoded).expect("surface decodes");
    assert_eq!(decoded, surface);

    let mut extra = serde_json::to_value(&surface).expect("surface serializes");
    extra["raw_provider_body"] = serde_json::json!("leak");
    assert!(
        serde_json::from_value::<ModelFailureSurface>(extra).is_err(),
        "unknown provider payload fields must be rejected"
    );
}
