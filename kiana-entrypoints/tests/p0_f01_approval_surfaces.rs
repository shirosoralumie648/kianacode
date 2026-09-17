#[test]
fn three_surfaces_list_and_answer_the_same_pending_approval() {
    let helper = include_str!("../src/harness_run.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    let one_shot = include_str!("../src/product_command.rs");

    for surface in [workbench, web, one_shot] {
        assert!(
            surface.contains("pending_approvals")
                || surface.contains("RequestEnvelope::pending_approvals"),
            "surface must use the shared pending approval query"
        );
    }
    assert!(workbench.contains("decide_approval_envelope_on_host"));
    assert!(web.contains("decide_approval_envelope_on_host"));
    assert!(one_shot.contains("RequestEnvelope::approval_decision_with_proof"));
    assert!(helper.contains("client.pending_approvals(metadata, run_id)"));
    assert!(helper.contains("client.approval_decision_with_proof"));
    assert!(helper.contains("challenge.request_hash"));
    assert!(helper.contains("challenge.nonce"));
}

#[test]
fn approval_surfaces_do_not_auto_approve_or_create_a_second_loop() {
    let workbench = include_str!("../src/workbench_chat.rs");
    let web = include_str!("../src/web.rs");
    let one_shot = include_str!("../src/product_command.rs");
    for surface in [workbench, web, one_shot] {
        assert!(!surface.contains("auto_approve"));
    }
}
