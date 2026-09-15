#[test]
fn capability_entry_paths_share_one_authorization_pipeline() {
    let approvals = include_str!("../src/approvals.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let baseline = include_str!("../../docs/roadmap/control-plane-entry-path-baseline.md");

    for marker in [
        "authorize_and_execute",
        "resume_approved_invocation",
        "execute_authorized_request",
        "prepare_capability_action",
        "authorize_capability_action",
        "stage_capability_action",
        "dispatch_capability_action",
        "finalize_capability_action",
    ] {
        assert!(
            approvals.contains(marker) || capabilities.contains(marker),
            "missing shared pipeline marker {marker}"
        );
    }
    assert!(approvals.contains("decision_context"));
    assert!(approvals.contains("Some((approval_id, &validated.challenge.reason))"));
    assert!(approvals.contains("self.prepare_capability_action"));
    assert!(approvals.contains("self.dispatch_capability_action"));
    assert!(capabilities.contains("self.prepare_capability_action_cancellable"));
    assert!(capabilities.contains("self.authorize_capability_action_cancellable"));
    assert!(capabilities.contains("self.dispatch_authorized"));
    assert!(capabilities.contains("self.finalize_capability_action"));
    assert!(lifecycle.contains("broker_harness_capability"));
    assert!(lifecycle.contains("RunnerEvent::CapabilityRequested"));
    assert!(dispatch.contains("ExecutionPermitVerifierPort"));
    assert!(dispatch.contains("verify_and_consume"));
    assert!(!capabilities.contains("self.capabilities.execute"));
    assert!(baseline.contains("cp_three_entry_paths_have_identical_authorization_semantics"));
    assert!(baseline.contains("prepare → authorize → stage/dispatch → finalize"));
    assert!(baseline.contains("result_unknown"));
    assert!(baseline.contains("direct command"));
}
