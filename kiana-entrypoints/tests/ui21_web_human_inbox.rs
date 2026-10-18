//! UI-21 Web Human Inbox declarative fixture.
//!
//! The fixture publishes the server-owned card projection, the deny-first refusal matrix and the
//! audit/disposition mapping.  Each row is checked against the source that implements it, so the
//! fixture can never drift into a second, softer contract.

use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/ui21-web-human-inbox.json"))
        .expect("valid UI-21 Human Inbox fixture")
}

/// Whitespace-insensitive source matching: rustfmt line wrapping must not change the check, but
/// token order and literal text must.
fn compact(source: &str) -> String {
    source.split_whitespace().collect()
}

fn disposition_constant(disposition: &str) -> &'static str {
    match disposition {
        "effect_dispatch" => "WEB_HUMAN_DISPOSITION_EFFECT_DISPATCH",
        "decision_only" => "WEB_HUMAN_DISPOSITION_DECISION_ONLY",
        "unmapped" => "WEB_HUMAN_DISPOSITION_UNMAPPED",
        other => panic!("unknown fixture disposition: {other}"),
    }
}

#[test]
fn ui21_fixture_captures_server_card_and_deny_first_matrix() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], "kiana.ui-human-inbox.v1");
    assert_eq!(fixture["card_schema"], "kiana.ui-human-action-card.v1");
    assert_eq!(fixture["intent_schema"], "kiana.ui-human-action-intent.v1");
    assert_eq!(fixture["audit_schema"], "kiana.web-human-audit.v1");
    for field in [
        "reason",
        "scope",
        "expires_at_unix_ms",
        "expected_revision",
        "fields",
        "allowed_decisions",
        "payload_digest",
        "action_cards",
        "action_audit",
        "web_result_class",
    ] {
        assert!(fixture["server_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == field));
    }
    for denial in [
        "duplicate_card",
        "colon_delimited_identity_collision",
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
        "forged_actor_scope_or_grant_field_refused",
        "unknown_field_type_fails_closed",
        "illegal_field_value_never_forms_intent",
        "denied_approval_never_dispatches_effect",
        "replay_returns_recorded_result",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == denial));
    }
    for success in [
        "approve",
        "deny",
        "original_command_result_on_replay",
        "unknown_visible_with_query_original",
        "audit_event_and_disposition_visible",
        "dangerous_action_collapsed_with_effect_scope",
    ] {
        assert!(fixture["success"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == success));
    }
}

#[test]
fn ui21_audit_matrix_matches_the_server_owned_rule_table() {
    let fixture = fixture();
    let source = compact(include_str!("../src/web_inbox.rs"));
    assert!(
        source.contains("[WebHumanAuditRule;14]"),
        "the audit rule table declares its bounded size"
    );
    let rows = fixture["audit_matrix"].as_array().expect("audit matrix");
    assert_eq!(
        rows.len(),
        14,
        "every audited command/action pair must be declared exactly once"
    );
    let mut dangerous = 0;
    for row in rows {
        let command = row["command"].as_str().expect("command");
        let audit_event = row["audit_event"].as_str().expect("audit event");
        let disposition = row["disposition"].as_str().expect("disposition");
        let constant = disposition_constant(disposition);
        let dangerous_flag = row["dangerous"].as_bool().expect("dangerous flag");
        if dangerous_flag {
            dangerous += 1;
        }
        match row["action_id"].as_str() {
            Some(action_id) => {
                let expected = format!(
                    "audit_rule(\"{command}\",\"{action_id}\",\"{audit_event}\",{constant},{dangerous_flag},)"
                );
                assert!(
                    source.contains(&expected),
                    "audit rule missing or changed: {expected}"
                );
                if matches!(action_id, "deny" | "reject" | "waive") {
                    assert!(
                        !dangerous_flag,
                        "a denial can never dispatch an effect: {command}/{action_id}"
                    );
                }
            }
            None => {
                // A command-wide rule matches every action id of that command.
                let expected = format!(
                    "WebHumanAuditRule{{command:\"{command}\",action:None,audit_event:\"{audit_event}\",disposition:{constant},dangerous:{dangerous_flag},}}"
                );
                assert!(
                    source.contains(&expected),
                    "command-wide audit rule missing or changed: {expected}"
                );
            }
        }
    }
    assert_eq!(
        dangerous, 2,
        "only approving a staged effect and dispatching a delivery are dangerous"
    );
    assert!(source.contains("\"approval\",\"approve\",\"approval.approved\""));
    assert!(source.contains("\"company.business\",\"dispatch_delivery\",\"company.business.v2\""));
    assert!(source.contains("WEB_HUMAN_DISPOSITION_UNMAPPED"));
}

#[test]
fn ui21_field_arms_match_the_display_type_helper() {
    let fixture = fixture();
    let source = compact(include_str!("../src/web_inbox.rs"));
    let arms = fixture["field_arms"].as_array().expect("field arms");
    assert!(!arms.is_empty());
    for arm in arms {
        let field_type = arm["field_type"].as_str().expect("field type");
        let names = arm["names"].as_array().expect("names");
        assert!(!names.is_empty());
        let pattern = names
            .iter()
            .map(|name| format!("\"{}\"", name.as_str().expect("name")))
            .collect::<Vec<_>>()
            .join("|");
        let expected = format!("{pattern}=>\"{field_type}\"");
        assert!(
            source.contains(&expected),
            "display field type arm missing or changed: {expected}"
        );
    }
    let default = fixture["default_field_type"]
        .as_str()
        .expect("default field type");
    assert!(
        source.contains(&format!("_=>\"{default}\"")),
        "unknown field names must fall back to the reviewed default"
    );
}

#[test]
fn ui21_reserved_intent_fields_match_the_server_guard() {
    let fixture = fixture();
    let source = compact(include_str!("../src/web_inbox.rs"));
    let reserved = fixture["reserved_intent_fields"]
        .as_array()
        .expect("reserved intent fields");
    assert_eq!(reserved.len(), 10);
    let literal = reserved
        .iter()
        .map(|field| format!("\"{}\"", field.as_str().expect("reserved field")))
        .collect::<Vec<_>>()
        .join(",");
    let expected = format!("WEB_HUMAN_RESERVED_INTENT_FIELDS:[&str;10]=[{literal}");
    assert!(
        source.contains(&format!("{expected},]")) || source.contains(&format!("{expected}]")),
        "the server-reserved intent field guard changed: {expected}"
    );
    assert!(source.contains("web_human_action_reserved_field"));
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
        "item.item_id.as_str(), item.action_id.as_str()",
    ] {
        assert!(
            protocol.contains(marker),
            "UI-21 protocol marker missing: {marker}"
        );
    }
    for marker in [
        "WEB_HUMAN_INBOX_SCHEMA",
        "WebHumanInbox",
        "WebHumanActionState",
        "pub fn action_state(",
        "pub fn submission_key(",
        "prepare_intent",
        "can_mutate()",
        "web_human_inbox_owner_required",
        "web_human_action_expired",
        "web_human_action_revoked",
        "web_human_field_denied",
        "web_human_field_type_unknown",
        "web_human_field_value_not_allowed",
        "web_human_field_value_limit",
        "web_human_required_field_missing",
        "web_human_inbox_stale_revision",
        "item_id.len()",
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
        "WEB_HUMAN_AUDIT_RULES",
        "WEB_HUMAN_RESERVED_INTENT_FIELDS",
        "WEB_HUMAN_DISPOSITION_DECISION_ONLY",
        "WEB_HUMAN_DISPOSITION_EFFECT_DISPATCH",
        "human_audit_entry",
        "human_effect_scope",
        "human_result_class",
        "UI_HUMAN_ACTION_CARD_SCHEMA",
    ] {
        assert!(web.contains(marker), "UI-21 Web marker missing: {marker}");
    }
    let web_route = include_str!("../src/web.rs");
    for marker in [
        "validate_human_action_command",
        "annotate_human_inbox_response",
        "human_result_class",
        "web_result_class",
        "claim_action_submission",
        "web_human_action_reserved_field",
    ] {
        assert!(
            web_route.contains(marker),
            "UI-21 Web route marker missing: {marker}"
        );
    }
}
