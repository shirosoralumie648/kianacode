use kiana_domain::{ModelOutput, RunId};
use kiana_runner::{KianaHarness, ScriptedModel};
use kiana_runner_protocol::{RunnerCommand, RunnerEvent};
use serde_json::json;
use std::sync::Arc;

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
        retry_runtime.contains("RetryPolicy::new(")
            && retry_runtime.contains("policy.classify("),
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
    assert!(!harness.contains("contains(\"result_unknown"));
}
