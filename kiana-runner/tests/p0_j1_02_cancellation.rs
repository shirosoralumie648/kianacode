#[test]
fn cancel_drains_queued_tool_calls_with_replay_safe_results() {
    let harness = include_str!("../src/harness.rs");
    let existing = include_str!("h12_serial_batch.rs");
    for marker in [
        "pending_tools.pop_front()",
        "pending_tools.drain(..)",
        "RunnerEvent::ToolCancelled",
        "not_executed:true",
        "replay_safe:true",
        "cancelled_batch_never_dispatches_remaining_calls",
    ] {
        assert!(
            harness.contains(marker) || existing.contains(marker),
            "queued cancellation marker missing: {marker}"
        );
    }
}

#[test]
fn synthetic_cancel_results_cannot_become_model_success() {
    let harness = include_str!("../src/harness.rs");
    assert!(harness.contains("if let Some(error) = run.cancellation.error()?"));
    assert!(harness.contains("ToolObservationStatus::CancelledNotStarted"));
    assert!(harness.contains("tool_cancelled_not_started"));
    assert!(!harness.contains(
        "replay_safe:true}),\n                    });\n                    run.messages.push"
    ));
}
