#[test]
fn ui21_human_resolve_rechecks_server_payload_and_card_revision() {
    let source = include_str!("../src/platform.rs");
    for marker in [
        "payload_digest",
        "human_action_payload_digest_mismatch",
        "expected_revision",
        "human_action_revision_mismatch",
        "human_inbox_stale",
        "human_action_field_denied",
        "human.resolve",
    ] {
        assert!(
            source.contains(marker),
            "UI-21 core marker missing: {marker}"
        );
    }
    let resolver = source
        .split("async fn resolve_human_item(")
        .nth(1)
        .expect("human resolver");
    let payload_check = resolver
        .find("human_action_payload_digest_mismatch")
        .expect("payload digest check");
    let authority = resolver
        .find("match action.command.as_str()")
        .expect("authority dispatch");
    assert!(
        payload_check < authority,
        "payload must be checked before authority dispatch"
    );
    assert!(!resolver.contains("CapabilityBroker"));
    assert!(!resolver.contains("KianaHarness"));
}
