#[test]
fn ui21_web_card_renderer_is_server_driven_and_deny_first() {
    let page = include_str!("../src/web_page.html");
    let web = include_str!("../src/web.rs");
    let web_inbox = include_str!("../src/web_inbox.rs");
    for marker in [
        "HUMAN_INBOX_SCHEMA",
        "HUMAN_ACTION_CARD_SCHEMA",
        "HUMAN_ACTION_INTENT_SCHEMA",
        "humanActionCardFor",
        "humanActionExpired",
        "validateHumanActionFields",
        "renderHumanActionResult",
        "human_action_field_denied",
        "approval_expired",
        "approval_revoked",
        "result_unknown",
        "不能自动重试",
        "scope",
        "reason",
        "allowed_decisions",
        "payload_digest",
        "owner",
    ] {
        assert!(page.contains(marker), "UI-21 page marker missing: {marker}");
    }
    assert!(web.contains("validate_human_action_command"));
    assert!(web.contains("claim_action_submission"));
    assert!(web.contains("require_session_owner"));
    assert!(web_inbox.contains("UiHumanActionIntentV1"));
    assert!(web_inbox.contains("annotate_human_inbox_response"));
    for forbidden in [
        "new KianaHarness",
        "new CapabilityBroker",
        "ControlPlane.execute",
        "sendBeacon",
        "window.actor",
        "window.scope",
    ] {
        assert!(
            !page.contains(forbidden),
            "Web card renderer gained authority: {forbidden}"
        );
    }
}

#[test]
fn ui21_submission_guard_validates_before_idempotency_claim() {
    let web = include_str!("../src/web.rs");
    let action = web
        .split("async fn command_action(")
        .nth(1)
        .and_then(|rest| rest.split("#[derive(Deserialize)]").next())
        .expect("command action handler");
    let validation = action
        .find("validate_human_action_command")
        .expect("typed human action validation");
    let claim = action
        .find("claim_action_submission")
        .expect("idempotency claim");
    assert!(
        validation < claim,
        "malformed intent must not consume replay key"
    );
    let page = include_str!("../src/web_page.html");
    let submit = page
        .split("async function submitHumanAction(")
        .nth(1)
        .and_then(|rest| rest.split("async function refreshApprovals").next())
        .expect("typed human action submitter");
    assert!(submit.contains("payload_digest"));
    assert!(submit.contains("idempotency_key"));
    assert!(submit.contains("expected_revision"));
    assert!(!submit.contains("actor"));
    assert!(!submit.contains("scope:"));
    assert!(!submit.contains("approval_grant"));
}

#[test]
fn ui21_unknown_incident_and_limitations_stay_visible_without_retry() {
    let page = include_str!("../src/web_page.html");
    let renderer = page
        .split("function renderHumanActionResult(")
        .nth(1)
        .and_then(|rest| rest.split("async function submitHumanAction").next())
        .expect("human action result renderer");
    for marker in [
        "unknown",
        "result_unknown",
        "incident",
        "limitations",
        "textContent",
        "不能自动重试",
    ] {
        assert!(renderer.contains(marker), "result marker missing: {marker}");
    }
    assert!(!renderer.contains("/api/run"));
    assert!(!renderer.contains("/api/cancel"));
    assert!(!renderer.contains("retry"));
}
