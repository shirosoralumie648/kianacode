use serde_json::Value;

#[test]
fn ui21_fixture_captures_server_card_and_deny_first_matrix() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui21-web-human-inbox.json"))
        .expect("valid UI-21 Human Inbox fixture");
    assert_eq!(fixture["schema"], "kiana.ui-human-inbox.v1");
    assert_eq!(fixture["card_schema"], "kiana.ui-human-action-card.v1");
    assert_eq!(fixture["intent_schema"], "kiana.ui-human-action-intent.v1");
    for field in [
        "reason",
        "scope",
        "expires_at_unix_ms",
        "expected_revision",
        "fields",
        "allowed_decisions",
        "payload_digest",
    ] {
        assert!(fixture["server_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == field));
    }
    for denial in [
        "duplicate_card",
        "expired_approval",
        "revoked_approval",
        "stale_inbox_revision",
        "stale_card_revision",
        "tab_owner_mismatch",
        "observer_tab_action",
        "hidden_field_scope_expansion",
        "payload_digest_mutation",
        "idempotency_replay_new_effect",
        "response_unknown_auto_retry",
        "close_tab_cancels_run",
        "incident_or_limitation_hidden",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == denial));
    }
}

#[test]
fn ui21_typed_protocol_client_and_server_contracts_are_wired() {
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let client = include_str!("../../kiana-client/src/web_inbox.rs");
    let web = include_str!("../src/web_inbox.rs");
    for marker in [
        "UI_HUMAN_INBOX_SCHEMA",
        "UI_HUMAN_ACTION_CARD_SCHEMA",
        "UI_HUMAN_ACTION_INTENT_SCHEMA",
        "UiHumanActionCardV1",
        "UiHumanActionIntentV1",
        "payload_digest",
        "allowed_decisions",
        "UiHumanInboxV1",
    ] {
        assert!(
            protocol.contains(marker),
            "UI-21 protocol marker missing: {marker}"
        );
    }
    for marker in [
        "WEB_HUMAN_INBOX_SCHEMA",
        "WebHumanInbox",
        "prepare_intent",
        "web_human_inbox_owner_required",
        "web_human_action_expired",
        "web_human_action_revoked",
        "web_human_field_denied",
        "web_human_inbox_stale_revision",
    ] {
        assert!(
            client.contains(marker),
            "UI-21 client marker missing: {marker}"
        );
    }
    for marker in [
        "validate_human_action_command",
        "annotate_human_inbox_response",
        "WEB_HUMAN_RESOLVE_COMMAND",
        "human_result_class",
        "UI_HUMAN_ACTION_CARD_SCHEMA",
    ] {
        assert!(web.contains(marker), "UI-21 Web marker missing: {marker}");
    }
}
