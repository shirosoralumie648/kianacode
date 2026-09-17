#[test]
fn er13_result_delivery_is_committed_before_runner_and_shared_by_three_paths() {
    let dispatch = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let approvals = include_str!("../src/approvals.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "execution.result_committed",
        "result.delivery_claimed",
        "result.deliver",
        "commit_confirmed",
        "result_delivery_already_claimed",
        "cancelled:result_delivery_run_inactive",
        "result_unknown:result_delivery_unconfirmed",
        "finalize_capability_action",
        "dispatch_capability_action",
        "CapabilityResult",
    ] {
        assert!(
            dispatch.contains(marker)
                || capabilities.contains(marker)
                || approvals.contains(marker)
                || lifecycle.contains(marker)
                || runner.contains(marker),
            "ER-13 marker missing: {marker}"
        );
    }
    let claim = dispatch
        .find("result.delivery_claimed")
        .expect("delivery claim");
    let runner_send = dispatch
        .find("RunnerCommand::CapabilityResult")
        .expect("runner result callback");
    assert!(
        claim < runner_send,
        "delivery must be claimed before callback"
    );
    assert!(dispatch.contains("AggregateVersion::new(\"result_delivery\""));
    assert!(dispatch.contains("AggregateVersion::new(\"run\""));
    assert!(capabilities.contains("self.finalize_capability_action"));
    assert!(approvals.contains("self.dispatch_capability_action"));
    assert!(approvals.contains("self.deliver_capability_result"));
    assert!(runner.contains("unexpected_capability_result"));
    for forbidden in [
        "retry_tool_after_delivery_claim",
        "reexecute_model_after_delivery_claim",
        "delivery_claim_grants_authority",
    ] {
        assert!(
            !dispatch.contains(forbidden)
                && !capabilities.contains(forbidden)
                && !approvals.contains(forbidden),
            "delivery must not {forbidden}"
        );
    }
}
