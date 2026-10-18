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

#[test]
fn ui21_core_recheck_order_stays_deny_first_before_any_authority() {
    let source = include_str!("../src/platform.rs");
    for marker in [
        "human_item_not_found",
        "human_action_unavailable",
        "human_action_fields_required",
    ] {
        assert!(
            source.contains(marker),
            "UI-21 core refusal missing: {marker}"
        );
    }
    let resolver = source
        .split("async fn resolve_human_item(")
        .nth(1)
        .expect("human resolver");
    // Every refusal is checked against the authoritative projection before the command dispatch:
    // stale inbox, missing item, unavailable action, payload digest, card revision, hidden field,
    // missing required field.
    let ordered = [
        "human_inbox_stale",
        "human_item_not_found",
        "human_action_unavailable",
        "human_action_payload_digest_mismatch",
        "human_action_revision_mismatch",
        "human_action_field_denied",
        "human_action_fields_required",
        "match action.command.as_str()",
    ];
    let mut previous = 0;
    for marker in ordered {
        let position = resolver
            .find(marker)
            .unwrap_or_else(|| panic!("core recheck marker missing: {marker}"));
        assert!(
            position > previous,
            "core recheck order changed at: {marker}"
        );
        previous = position;
    }
    // The recheck reads the opaque server payload and the committed inbox digest, never a value
    // supplied by the browser card.
    assert!(resolver.contains("json_digest(&action.arguments)"));
    assert!(resolver.contains("json_digest(&json!(items))"));
}
