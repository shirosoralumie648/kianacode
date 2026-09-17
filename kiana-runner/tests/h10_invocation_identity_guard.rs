#[test]
fn h10_invocation_identity_is_generated_once_per_complete_batch() {
    let harness = include_str!("../src/harness.rs");
    let tools = include_str!("../src/tools.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let identity = include_str!("../../kiana-domain/src/execution_identity.rs");
    let fixtures = include_str!("h10_invocation_identity.rs");
    for marker in [
        "validate_model_calls",
        "stable_invocation_request_id",
        "assistant_item_id",
        "capability_for_tool_with_request_id",
        "runner_checkpoint_invocation_identity_mismatch",
        "invalid_second_call_prevents_all_batch_dispatch",
        "duplicate_call_id_in_one_message_is_rejected",
        "invocation_identity_survives_queue_approval_and_restore",
        "InvocationIdentity",
        "attempt",
    ] {
        assert!(
            harness.contains(marker)
                || tools.contains(marker)
                || model.contains(marker)
                || identity.contains(marker)
                || fixtures.contains(marker),
            "H10 marker missing: {marker}"
        );
    }
    assert!(harness.contains(".tool_calls") && harness.contains(".enumerate()"));
    assert!(harness.contains("collect::<Result<Vec<_>, _>>()"));
    assert!(harness.contains("stable_invocation_request_id_parts"));
    assert!(tools.contains("capability_for_tool_with_request_id"));
    assert!(!harness.contains("capability_for_tool(call, &run.sandbox"));
    assert!(!harness.contains("capability_for_tool(call, sandbox"));
    for forbidden in [
        "dispatch_first_call_before_batch_validation",
        "regenerate_request_id_on_restore",
        "provider_call_id_is_invocation_id",
        "partial_batch_dispatch",
    ] {
        assert!(
            !harness.contains(forbidden) && !tools.contains(forbidden),
            "forbidden H10 identity bypass: {forbidden}"
        );
    }
}
