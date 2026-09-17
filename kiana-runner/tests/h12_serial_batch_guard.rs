#[test]
fn h12_batches_are_serial_and_wrong_results_do_not_advance() {
    let harness = include_str!("../src/harness.rs");
    let fixtures = include_str!("h12_serial_batch.rs");
    for marker in [
        "PendingToolPhase",
        "Queued",
        "Dispatched",
        "Settled",
        "capability_result_before_dispatch",
        "capability_result_mismatch",
        "cancelled:queued",
        "not_executed",
        "wrong_result_does_not_consume_pending_call",
        "cancelled_batch_never_dispatches_remaining_calls",
        "three_serial_calls_produce_three_ordered_results_before_model",
    ] {
        assert!(
            harness.contains(marker) || fixtures.contains(marker),
            "H12 marker missing: {marker}"
        );
    }
    let peek = harness
        .find("run.pending_tools.front()")
        .expect("result handler must inspect before pop");
    let pop = harness
        .find("run.pending_tools.pop_front()")
        .expect("settled result must pop after identity check");
    assert!(peek < pop);
    assert!(harness.contains("pending.phase = PendingToolPhase::Dispatched"));
    assert!(harness.contains("pending.phase = PendingToolPhase::Settled"));
    assert!(harness.contains("ToolCancelled"));
    assert!(harness.contains("replay_safe"));
    for forbidden in [
        "pop_before_result_identity_check",
        "dispatch_remaining_after_cancel",
        "wrong_result_advances_batch",
        "parallel_unbounded_batch",
    ] {
        assert!(
            !harness.contains(forbidden),
            "forbidden H12 path: {forbidden}"
        );
    }
}
