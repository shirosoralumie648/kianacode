#[test]
fn h08_cancellation_fence_covers_model_and_effect_boundaries() {
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let harness = include_str!("../src/harness.rs");
    let runner_tests = include_str!("h08_cancellation.rs");
    let shell = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let mcp = include_str!("../../kiana-daemon/src/mcp_stdio.rs");
    let dispatch = include_str!("../../kiana-core/src/dispatch.rs");
    for marker in [
        "complete_admitted_cancellable",
        "complete_prepared_cancellable",
        "model_cancelled_before_response",
        "cancellation_signal",
        "tokio::time::timeout",
        "cancel_during_retry_wait_prevents_next_attempt",
        "silent_model_is_interrupted_by_deadline",
        "terminate_process_group",
        "PROCESS_GROUP_EXIT_GRACE",
        "wait_for_cancellation",
        "mcp_request_stopped",
        "mcp_stop_unconfirmed",
        "result_unknown:cancel_stop_unconfirmed",
        "stop_confirmed",
    ] {
        assert!(
            ports.contains(marker)
                || harness.contains(marker)
                || runner_tests.contains(marker)
                || shell.contains(marker)
                || mcp.contains(marker)
                || dispatch.contains(marker),
            "H08 marker missing: {marker}"
        );
    }
    let cancellable = ports
        .find("complete_admitted_cancellable")
        .expect("model admission must have a cancellation fence");
    let model_call = ports
        .find("self.complete_admitted(prepared")
        .expect("cancellable adapter must wrap the admitted path");
    assert!(cancellable < model_call);
    assert!(harness.contains("cancellation.subscribe()"));
    assert!(harness.contains("stream.cancel()"));
    for forbidden in [
        "cancelled_without_stop_confirmation_is_success",
        "ignore_cancellation",
        "drop_child_without_reap",
        "retry_after_cancel",
    ] {
        assert!(
            !ports.contains(forbidden)
                && !harness.contains(forbidden)
                && !shell.contains(forbidden)
                && !mcp.contains(forbidden),
            "forbidden H08 cancellation bypass: {forbidden}"
        );
    }
}
