use async_trait::async_trait;
use kiana_domain::{
    ModelError, ModelOutput, ModelRecoveryDisposition, ModelReply, ModelRequest, ModelRetryClass,
    ModelSideEffectState, RunId,
};
use kiana_runner::{KianaHarness, ModelClient, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct ClassifiedRecoveryModel {
    first_error: ModelError,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for ClassifiedRecoveryModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("prepared_model_call_required".to_owned())
    }

    async fn complete_prepared(
        &self,
        prepared: kiana_domain::PreparedModelCall,
        _on_delta: &mut (dyn FnMut(kiana_domain::ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        prepared.validate()?;
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(self.first_error.clone());
        }
        let mut output = ModelOutput::text("recovered");
        output.stop_reason = Some("end_turn".to_owned());
        ModelReply::legacy(output)
    }
}

fn recovery_model(error: ModelError) -> (Arc<KianaHarness>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let harness = Arc::new(KianaHarness::new(Arc::new(ClassifiedRecoveryModel {
        first_error: error,
        calls: calls.clone(),
    })));
    (harness, calls)
}

#[tokio::test]
async fn length_stop_never_dispatches_tools_or_completes_turn() {
    let mut output = ModelOutput::with_tool(
        "partial response",
        "shell",
        json!({"command": "echo must-not-run"}),
    );
    output.stop_reason = Some("length".to_owned());
    let harness = KianaHarness::new(Arc::new(ScriptedModel::new(vec![output])));
    let run_id = RunId::new();

    let events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "return a truncated tool call",
            "/repo",
            "read-only",
        ))
        .await
        .expect("a rejected model stop is represented as a RunnerEvent");

    assert!(
        events.iter().any(|event| {
            matches!(
                event,
                RunnerEvent::Failed {
                    run_id: failed_run,
                    error,
                } if *failed_run == run_id && error == "model_output_truncated"
            )
        }),
        "length stop must produce a stable failure: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, RunnerEvent::CapabilityRequested { .. }))
            .count(),
        0,
        "length stop must not hand any tool call to ControlPlane/Broker: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, RunnerEvent::Completed { .. }))
            .count(),
        0,
        "length stop must not complete the turn: {events:?}"
    );
}

#[tokio::test]
async fn incomplete_stop_never_dispatches_tools_or_completes_turn() {
    let mut output = ModelOutput::with_tool(
        "partial response",
        "shell",
        json!({"command": "echo must-not-run"}),
    );
    output.stop_reason = Some("incomplete".to_owned());
    let harness = KianaHarness::new(Arc::new(ScriptedModel::new(vec![output])));
    let run_id = RunId::new();

    let events = harness
        .send(RunnerCommand::start_in(
            run_id,
            "return an incomplete tool call",
            "/repo",
            "read-only",
        ))
        .await
        .expect("an incomplete model stop is represented as a RunnerEvent");

    assert!(events.iter().any(|event| {
        matches!(
            event,
            RunnerEvent::Failed { run_id: failed_run, error }
                if *failed_run == run_id && error == "model_transport_incomplete"
        )
    }));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, RunnerEvent::CapabilityRequested { .. }))
            .count(),
        0,
        "incomplete output must not hand tool calls to ControlPlane/Broker: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, RunnerEvent::Completed { .. }))
            .count(),
        0,
        "incomplete output must not complete the turn: {events:?}"
    );
}

#[tokio::test]
async fn typed_recovery_disposition_bounds_runner_routing() {
    let mut rejected = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
    rejected.side_effect_state = ModelSideEffectState::None;
    let cases = [
        (
            "transport_before_send",
            ModelError::transport("connect_failed", ModelRetryClass::BeforeSend, false),
            2,
            None,
        ),
        ("transport_rejected", rejected, 2, None),
        (
            "format_repair",
            ModelError::invalid("format_rejected")
                .with_recovery_disposition(ModelRecoveryDisposition::FormatRepair),
            1,
            Some("model_format_repair_unavailable"),
        ),
        (
            "tool_repair",
            ModelError::invalid("tool_rejected")
                .with_recovery_disposition(ModelRecoveryDisposition::ToolRepair),
            1,
            Some("model_tool_repair_unavailable"),
        ),
        (
            "context_repair",
            ModelError::invalid("context_rejected")
                .with_recovery_disposition(ModelRecoveryDisposition::ContextRepair),
            1,
            Some("model_context_repair_unavailable"),
        ),
        (
            "terminal",
            ModelError::invalid("terminal_failure"),
            1,
            Some("terminal_failure"),
        ),
    ];

    for (case, error, expected_calls, expected_failure) in cases {
        let (harness, calls) = recovery_model(error);
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start_in(run_id, case, "/repo", "read-only"))
            .await
            .expect("model errors are returned as RunnerEvents");

        assert_eq!(calls.load(Ordering::SeqCst), expected_calls, "{case}");
        if let Some(expected_failure) = expected_failure {
            assert!(
                events.iter().any(|event| {
                    matches!(event, RunnerEvent::Failed { run_id: failed_run, error }
                    if *failed_run == run_id && error == expected_failure)
                }),
                "{case}: {events:?}"
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, RunnerEvent::Completed { .. })),
                "{case}"
            );
        } else {
            assert!(
                events.iter().any(|event| {
                    matches!(event, RunnerEvent::Completed { run_id: completed_run, .. }
                    if *completed_run == run_id)
                }),
                "{case}: {events:?}"
            );
        }
    }
}

#[test]
fn harness_stop_and_retry_paths_are_typed_and_fail_closed() {
    let harness = include_str!("../src/harness.rs");
    let retry = include_str!("../src/retry.rs");
    let retry_policy = include_str!("../../kiana-domain/src/retry_policy.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/response.rs");
    let retry_runtime = retry.split("#[cfg(test)]").next().unwrap_or(retry);
    let retry_policy_runtime = retry_policy
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(retry_policy);
    for marker in [
        "normalized_stop_reason",
        "model_output_truncated",
        "model_refused",
        "model_transport_incomplete",
    ] {
        assert!(
            harness.contains(marker) || model.contains(marker) || provider.contains(marker),
            "missing H05 stop/retry marker {marker}"
        );
    }
    assert!(
        retry_runtime.contains("RetryPolicy::new(") && retry_runtime.contains("policy.classify("),
        "runner retry path must delegate to the bounded domain classifier"
    );
    for marker in [
        "self.class == ModelRetryClass::BeforeSend",
        "&& !self.request_sent",
        "self.class == ModelRetryClass::Rejected",
        "&& self.request_sent",
        "self.side_effect_state == ModelSideEffectState::None",
        "observation.safe_pre_send() || observation.safe_rejection()",
    ] {
        assert!(
            retry_policy_runtime.contains(marker),
            "missing production retry-classifier marker {marker}"
        );
    }
    assert!(model.contains("ModelOutcome"));
    assert!(model.contains("side_effect_state"));
    assert!(model.contains("MODEL_OUTCOME_SCHEMA"));
    for marker in [
        "ModelRecoveryDisposition",
        "FormatRepair",
        "ToolRepair",
        "ContextRepair",
        "Terminal",
    ] {
        assert!(
            model.contains(marker),
            "missing typed recovery disposition {marker}"
        );
    }
    assert!(model.contains("pub struct ModelOutcome"));
    assert!(model.contains("recovery_disposition: self.recovery_disposition"));
    assert!(model.contains("recovery_disposition: ModelRecoveryDisposition::Terminal"));
    assert!(retry_runtime
        .contains("error.recovery_disposition == ModelRecoveryDisposition::TransportRetry"));
    assert!(retry_runtime.contains("unsupported_recovery_reason"));
    assert!(harness.contains("unsupported_recovery_reason(&error)"));
    for reason in [
        "model_format_repair_unavailable",
        "model_tool_repair_unavailable",
        "model_context_repair_unavailable",
    ] {
        assert!(
            retry_runtime.contains(reason),
            "missing fail-closed repair outcome {reason}"
        );
    }
    assert!(!harness.contains("contains(\"result_unknown"));
}
