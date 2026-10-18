//! UI-21 Web Human Inbox client contract fixtures.
//!
//! Every case starts from the server card and asserts the deny-first refusals (owner, expiry,
//! revocation, revision, hidden field, illegal value) before any happy-path intent exists.  The
//! produced intent never carries actor, scope or approval material.

use kiana_client::{WebClientTab, WebHumanActionState, WebHumanInbox};
use kiana_protocol::{
    AuthenticatedPrincipalRef, SessionId, UiHumanActionCardV1, UiHumanActionFieldV1,
    UiHumanActionScopeV1, UiHumanInboxV1, UiTabSessionDisposition, UiTabSessionV1,
    UI_HUMAN_ACTION_CARD_SCHEMA, UI_HUMAN_INBOX_SCHEMA, UI_TAB_SESSION_SCHEMA,
};
use serde_json::{json, Value};

const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn field(name: &str, field_type: &str, required: bool) -> UiHumanActionFieldV1 {
    UiHumanActionFieldV1 {
        name: name.to_owned(),
        label: name.to_owned(),
        field_type: field_type.to_owned(),
        required,
        allowed_values: Vec::new(),
    }
}

fn card(item_id: &str, action_id: &str) -> UiHumanActionCardV1 {
    UiHumanActionCardV1 {
        schema: UI_HUMAN_ACTION_CARD_SCHEMA.to_owned(),
        item_id: item_id.to_owned(),
        action_id: action_id.to_owned(),
        command: "approval".to_owned(),
        target_id: format!("approval:{item_id}"),
        reason: "fixture card".to_owned(),
        scope: UiHumanActionScopeV1 {
            summary: "fixture scope".to_owned(),
            digest: Some(DIGEST.to_owned()),
            paths: Vec::new(),
        },
        expires_at_unix_ms: None,
        revoked: false,
        expected_revision: Some(7),
        fields: Vec::new(),
        allowed_decisions: vec![action_id.to_owned()],
        payload_digest: DIGEST.to_owned(),
    }
}

fn inbox(cards: Vec<UiHumanActionCardV1>) -> UiHumanInboxV1 {
    UiHumanInboxV1 {
        schema: UI_HUMAN_INBOX_SCHEMA.to_owned(),
        revision: DIGEST.to_owned(),
        items: cards,
        limitations: vec!["display projection only".to_owned()],
    }
}

fn loaded(cards: Vec<UiHumanActionCardV1>) -> WebHumanInbox {
    let mut inbox_state = WebHumanInbox::default();
    inbox_state.replace(inbox(cards)).expect("fixture inbox");
    inbox_state
}

fn owner_tab() -> WebClientTab {
    let mut tab = WebClientTab::new("ui21-tab", "ui21-session").expect("tab");
    tab.server_lease = Some(UiTabSessionV1 {
        schema: UI_TAB_SESSION_SCHEMA.to_owned(),
        principal: AuthenticatedPrincipalRef::local(),
        session_id: SessionId::new("ui21-session"),
        tab_id: "ui21-tab".to_owned(),
        owner_tab_id: "ui21-tab".to_owned(),
        lease_epoch: 1,
        token_generation: 1,
        disposition: UiTabSessionDisposition::Owner,
        feed_only: false,
    });
    tab
}

fn prepare(state: &WebHumanInbox, card: &UiHumanActionCardV1, fields: Value) -> Result<(), String> {
    state
        .prepare_intent(
            &owner_tab(),
            &card.item_id,
            &card.action_id,
            fields,
            "ui21-idempotency",
            DIGEST,
            card.expected_revision,
            1_000,
        )
        .map(|_| ())
}

#[test]
fn ui21_card_identity_and_duplicate_cards_fail_closed() {
    // Distinct opaque IDs that share the `item:action` delimiter must not collapse into one entry.
    let first = card("a:b", "c");
    let second = card("a", "b:c");
    let state = loaded(vec![first.clone(), second.clone()]);
    assert_eq!(state.cards().count(), 2);
    assert!(state.get("a:b", "c").is_some());
    assert!(state.get("a", "b:c").is_some());

    let mut duplicate = WebHumanInbox::default();
    // The versioned inbox shape rejects duplicate card identity before this presenter ever
    // builds a button, and a failed replace leaves no partial projection behind.
    assert_eq!(
        duplicate.replace(inbox(vec![first.clone(), first])),
        Err("ui_human_inbox_duplicate".to_owned())
    );
    assert!(duplicate.cards().next().is_none());
}

#[test]
fn ui21_action_state_is_deny_first_and_expired_revoked_never_actionable() {
    let mut actionable = card("approval:1", "approve");
    actionable.expected_revision = Some(1);
    let mut expired = card("approval:2", "approve");
    expired.expires_at_unix_ms = Some(1_000);
    let mut revoked = card("approval:3", "deny");
    revoked.revoked = true;
    let state = loaded(vec![actionable, expired, revoked]);

    assert_eq!(
        state.action_state("approval:1", "approve", 1_000),
        Some(WebHumanActionState::Actionable)
    );
    assert_eq!(
        state.action_state("approval:2", "approve", 1_000),
        Some(WebHumanActionState::Expired)
    );
    assert_eq!(
        state.action_state("approval:3", "deny", 1_000),
        Some(WebHumanActionState::Revoked)
    );
    assert_eq!(state.action_state("approval:9", "approve", 1_000), None);

    let actionable_ids = state
        .actionable(1_000)
        .into_iter()
        .map(|card| card.item_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(actionable_ids, vec!["approval:1".to_owned()]);
    assert!(WebHumanActionState::ALL
        .iter()
        .all(|state| { (state.is_actionable()) == (*state == WebHumanActionState::Actionable) }));
    assert_eq!(WebHumanActionState::Expired.as_str(), "expired");
}

#[test]
fn ui21_prepare_intent_refuses_ownerless_stale_and_forbidden_decisions() {
    let mut state = WebHumanInbox::default();
    state
        .replace(inbox(vec![card("approval:1", "approve")]))
        .unwrap();
    let active = card("approval:1", "approve");

    // An observer without the owner lease can never produce an intent.
    let observer = WebClientTab::new("ui21-tab", "ui21-session").unwrap();
    assert_eq!(
        state
            .prepare_intent(
                &observer,
                "approval:1",
                "approve",
                json!({}),
                "key",
                DIGEST,
                Some(7),
                1_000,
            )
            .unwrap_err(),
        "web_human_inbox_owner_required".to_owned()
    );

    // A stale inbox revision is refused before the card is looked up.
    assert_eq!(
        state
            .prepare_intent(
                &owner_tab(),
                "approval:1",
                "approve",
                json!({}),
                "key",
                "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                Some(7),
                1_000,
            )
            .unwrap_err(),
        "web_human_inbox_stale_revision".to_owned()
    );

    // A card that does not offer the requested decision can never be submitted through it.
    let mut limited = card("approval:1", "approve");
    limited.allowed_decisions = vec!["deny".to_owned()];
    let mut state = WebHumanInbox::default();
    state.replace(inbox(vec![limited])).unwrap();
    assert_eq!(
        prepare(&state, &active, json!({})).unwrap_err(),
        "web_human_decision_not_allowed".to_owned()
    );

    // Expired and revoked cards stay refusals even when a caller replays an old observation.
    let mut expired = card("approval:1", "approve");
    expired.expires_at_unix_ms = Some(1_000);
    let mut state = WebHumanInbox::default();
    state.replace(inbox(vec![expired.clone()])).unwrap();
    assert_eq!(
        prepare(&state, &expired, json!({})).unwrap_err(),
        "web_human_action_expired".to_owned()
    );
    let mut revoked = card("approval:1", "approve");
    revoked.revoked = true;
    let mut state = WebHumanInbox::default();
    state.replace(inbox(vec![revoked.clone()])).unwrap();
    assert_eq!(
        prepare(&state, &revoked, json!({})).unwrap_err(),
        "web_human_action_revoked".to_owned()
    );
}

#[test]
fn ui21_field_values_are_denied_before_an_intent_exists() {
    // Unknown declared types fail closed before any value is interpreted.
    let mut unknown_type = card("approval:1", "approve");
    unknown_type.fields = vec![field("mystery", "exec", true)];
    let mut state = WebHumanInbox::default();
    state.replace(inbox(vec![unknown_type.clone()])).unwrap();
    assert_eq!(
        prepare(&state, &unknown_type, json!({"mystery": 1})).unwrap_err(),
        "web_human_field_type_unknown".to_owned()
    );

    let mut typed = card("approval:2", "approve");
    let verdict = field("verdict", "boolean", true);
    let mut reason = field("reason_code", "string", true);
    reason.allowed_values = vec!["quality_approved".to_owned(), "rejected".to_owned()];
    typed.fields = vec![verdict, reason, field("evidence_refs", "string_list", true)];
    let mut state = WebHumanInbox::default();
    state.replace(inbox(vec![typed.clone()])).unwrap();

    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": "yes", "reason_code": "rejected", "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_field_value_type_invalid".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": 3, "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_field_value_type_invalid".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": "  ", "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_field_value_empty".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": "whatever", "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_field_value_not_allowed".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": "rejected", "evidence_refs": [1]})
        )
        .unwrap_err(),
        "web_human_field_value_type_invalid".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": "rejected", "evidence_refs": (0..65)
                .map(|index| format!("artifact:{index}")).collect::<Vec<_>>()})
        )
        .unwrap_err(),
        "web_human_field_value_limit".to_owned()
    );

    // Hidden fields can never widen the server scope, and a valid value set is accepted.
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": "rejected", "path_allow": ["."]})
        )
        .unwrap_err(),
        "web_human_field_denied".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"verdict": true, "reason_code": "rejected", "evidence_refs": ["artifact:1"]})
        ),
        Ok(())
    );
}

#[test]
fn ui21_submission_key_is_stable_and_carries_no_authority() {
    let state = loaded(vec![card("approval:1", "approve")]);
    let first = state
        .submission_key("approval:1", "approve", &json!({"reason_code": "rejected"}))
        .unwrap();
    let repeat = state
        .submission_key("approval:1", "approve", &json!({"reason_code": "rejected"}))
        .unwrap();
    let changed = state
        .submission_key(
            "approval:1",
            "approve",
            &json!({"reason_code": "quality_approved"}),
        )
        .unwrap();
    assert_eq!(
        first, repeat,
        "repeated clicks must reuse one submission key"
    );
    assert_ne!(first, changed, "a changed decision must produce a new key");
    assert!(first.starts_with("web-human:sha256:"));
    for forbidden in ["actor", "scope", "approval_grant", "path_allow"] {
        assert!(
            !first.contains(forbidden),
            "the submission key must not carry {forbidden}"
        );
    }
    assert_eq!(
        state.submission_key("approval:9", "approve", &json!({})),
        Err("web_human_action_not_found".to_owned())
    );
}

#[test]
fn ui21_unsupported_edit_restore_continue_are_refused_with_a_stable_code() {
    // The backend honors only approve/deny inside `human.resolve`: approval amend, workspace
    // restore and run continue are separate commands.  A card claiming one of those ids must be
    // refused before any intent exists, never silently accepted or classified as unmapped.
    for action_id in kiana_client::WEB_HUMAN_UNSUPPORTED_ACTIONS {
        let mut unsupported = card("approval:1", action_id);
        unsupported.command = "approval".to_owned();
        let state = loaded(vec![unsupported.clone()]);
        assert_eq!(
            prepare(&state, &unsupported, json!({})).unwrap_err(),
            format!("web_human_resolve_action_unsupported:{action_id}"),
            "{action_id} must be refused with its own stable code"
        );
        assert_eq!(
            state
                .submission_key("approval:1", action_id, &json!({}))
                .map(|key| key.starts_with("web-human:")),
            Ok(true),
            "the submission key stays derivable so a lost response can still be queried"
        );
    }
    assert_eq!(
        kiana_client::WEB_HUMAN_UNSUPPORTED_ACTIONS,
        ["edit", "restore", "continue"]
    );
    assert_eq!(
        kiana_client::WEB_HUMAN_REFUSAL_ACTION_UNSUPPORTED,
        "web_human_resolve_action_unsupported"
    );
}

#[test]
fn ui21_required_and_illegal_values_never_form_an_intent() {
    let mut typed = card("approval:1", "approve");
    typed.fields = vec![
        field("reason_code", "string", true),
        field("evidence_refs", "string_list", true),
    ];
    let state = loaded(vec![typed.clone()]);

    // A missing required field is refused before any value is interpreted.
    assert_eq!(
        prepare(&state, &typed, json!({"evidence_refs": []})).unwrap_err(),
        "web_human_required_field_missing".to_owned()
    );
    // An explicit `null` counts as present but cannot clear a required field.
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"reason_code": null, "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_required_field_missing".to_owned()
    );
    // Illegal values of each declared type never produce an intent.
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"reason_code": 7, "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_field_value_type_invalid".to_owned()
    );
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"reason_code": "x".repeat(4 * 1024 + 1), "evidence_refs": []})
        )
        .unwrap_err(),
        "web_human_field_value_too_long".to_owned()
    );
    // Only a fully typed, fully present form becomes an intent.
    assert_eq!(
        prepare(
            &state,
            &typed,
            json!({"reason_code": "accepted", "evidence_refs": ["artifact:1"]})
        ),
        Ok(())
    );
}

#[test]
fn ui21_prepare_intent_succeeds_without_actor_scope_or_grant_material() {
    let mut change = card("company-change:1", "decide_change");
    change.command = "company.business".to_owned();
    change.expected_revision = Some(4);
    change.fields = vec![
        field("approve", "boolean", true),
        field("decision_ref", "string", true),
    ];
    let state = loaded(vec![change.clone()]);

    let intent = state
        .prepare_intent(
            &owner_tab(),
            &change.item_id,
            &change.action_id,
            json!({"approve": true, "decision_ref": "artifact:ref-1"}),
            "web-human:stable-key",
            DIGEST,
            Some(4),
            1_000,
        )
        .expect("valid fixture intent");
    assert_eq!(intent.payload_digest, DIGEST);
    assert_eq!(intent.expected_revision, Some(4));
    let wire = serde_json::to_value(&intent).unwrap();
    for forbidden in ["actor", "actor_id", "scope", "path_allow", "approval_grant"] {
        assert!(
            wire.get(forbidden).is_none(),
            "an intent must never carry {forbidden}"
        );
    }
    assert_eq!(wire["fields"]["approve"], json!(true));
}
