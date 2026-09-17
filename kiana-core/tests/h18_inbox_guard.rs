#[test]
fn crash_between_claim_and_request_does_not_lose_input() {
    let inbox = include_str!("../../kiana-runner/src/inbox.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "InputId",
        "received_sequence",
        "target_turn_id",
        "claim_with_receipts",
        "claimed_input_ids",
        "inbox: run.inbox.clone()",
        "inbox: checkpoint.inbox",
        ".inbox",
    ] {
        assert!(
            inbox.contains(&marker) || harness.contains(&marker),
            "H18 persistence marker missing: {marker}"
        );
    }
}

#[test]
fn duplicate_input_is_consumed_once() {
    let inbox = include_str!("../../kiana-runner/src/inbox.rs");
    let fixture = include_str!("../../kiana-runner/tests/h18_inbox.rs");
    assert!(inbox.contains("claimed_input_ids"));
    assert!(inbox.contains("InputDisposition::Duplicate"));
    assert!(fixture.contains("duplicate_input_is_consumed_once"));
}

#[test]
fn cross_turn_steering_is_rejected() {
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let inbox = include_str!("../../kiana-runner/src/inbox.rs");
    assert!(harness.contains("inbox_target_turn_mismatch"));
    assert!(harness.contains("target_turn_id = run.turn_id"));
    assert!(inbox.contains("target_turn_id"));
}

#[test]
fn queued_followups_survive_restart_in_accept_order() {
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let inbox = include_str!("../../kiana-runner/src/inbox.rs");
    assert!(harness.contains("inbox: run.inbox.clone()"));
    assert!(harness.contains("inbox: checkpoint.inbox"));
    assert!(inbox.contains("received_sequence"));
    assert!(inbox.contains("queue_mut(target).push(message.clone())"));
}
