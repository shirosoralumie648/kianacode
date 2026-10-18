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

#[test]
fn ui21_page_audit_scope_and_reconcile_stay_display_only() {
    let page = include_str!("../src/web_page.html");
    for marker in [
        "HUMAN_ACTION_AUDIT_SCHEMA",
        "humanActionAuditFor",
        "humanAuditDispositionText",
        "action_audit",
        "effect_dispatch",
        "decision_only",
        "unmapped",
        "dangerous",
        "humanActionResultActions",
        "查询原 command（同一 idempotency key）",
    ] {
        assert!(
            page.contains(marker),
            "UI-21 audit marker missing: {marker}"
        );
    }

    // A dangerous action is only collapsed with its redacted effect scope in the summary; the
    // wrapper never submits anything by itself.
    let dangerous = page
        .split("if (audit && audit.dangerous)")
        .nth(1)
        .and_then(|rest| rest.split("card.appendChild(details);").next())
        .expect("dangerous action wrapper");
    for marker in [
        "document.createElement('details')",
        "document.createElement('summary')",
        "effect_paths",
        "details.appendChild(controls)",
    ] {
        assert!(
            dangerous.contains(marker),
            "collapse marker missing: {marker}"
        );
    }
    assert!(!dangerous.contains("submitHumanAction"));
    assert!(!dangerous.contains("/api/"));

    // An unknown result may only be reconciled by the user querying the original command with the
    // same stable identity; nothing here re-dispatches the effect.
    let follow_up = page
        .split("const followUp = document.getElementById('humanActionResultActions');")
        .nth(1)
        .and_then(|rest| rest.split("return response;").next())
        .expect("reconcile follow-up");
    for marker in [
        "response.web_result_class === 'unknown'",
        "response.status === 'result_unknown'",
        "submitCommand('human.resolve', intent, requested, submissionId)",
    ] {
        assert!(
            follow_up.contains(marker),
            "reconcile marker missing: {marker}"
        );
    }
    assert!(!follow_up.contains("/api/run"));
    assert!(!follow_up.contains("/api/cancel"));
    assert!(!follow_up.contains("new KianaHarness"));
}

#[test]
fn ui21_server_annotation_and_client_presenter_share_one_deny_first_contract() {
    let server = include_str!("../src/web_inbox.rs");
    let client = include_str!("../../kiana-client/src/web_inbox.rs");
    for marker in [
        "WEB_HUMAN_RESERVED_INTENT_FIELDS",
        "web_human_action_reserved_field",
        "WEB_HUMAN_DISPOSITION_UNMAPPED",
        "WEB_HUMAN_AUDIT_RULES",
        "human_effect_scope",
        "WEB_HUMAN_MAX_EFFECT_PATHS",
    ] {
        assert!(
            server.contains(marker),
            "UI-21 server boundary marker missing: {marker}"
        );
    }
    let annotation = server
        .split("pub(crate) fn annotate_human_inbox_response(")
        .nth(1)
        .and_then(|rest| rest.split("fn human_field_type(").next())
        .expect("server annotation");
    for marker in [
        "expected_version",
        "expires_at_unix_ms",
        "\"revoked\": false",
        "payload_digest",
        "allowed_decisions",
    ] {
        assert!(
            annotation.contains(marker),
            "annotation marker missing: {marker}"
        );
    }
    for forbidden in ["CapabilityBroker", "KianaHarness", "ControlPlane"] {
        assert!(
            !annotation.contains(forbidden),
            "the annotation gained authority: {forbidden}"
        );
    }
    for marker in [
        "WebHumanActionState",
        "pub fn action_state(",
        "pub fn submission_key(",
        "web_human_field_type_unknown",
        "web_human_field_value_not_allowed",
    ] {
        assert!(
            client.contains(marker),
            "UI-21 client presenter marker missing: {marker}"
        );
    }
    let submission_key = client
        .split("pub fn submission_key(")
        .nth(1)
        .and_then(|rest| rest.split("pub fn prepare_intent(").next())
        // Stop at the next documented item: the following doc comment explains that the *intent*
        // omits actor/scope material, which is not the submission key's body.
        .and_then(|rest| rest.split("\n    ///").next())
        .expect("submission key derivation");
    assert!(submission_key.contains("web-human:"));
    assert!(submission_key.contains("json_digest"));
    for forbidden in ["actor", "scope", "approval_grant"] {
        assert!(
            !submission_key.contains(forbidden),
            "the submission key must not carry {forbidden}"
        );
    }
}

#[test]
fn ui21_route_marks_result_class_only_after_the_authoritative_command() {
    let web = include_str!("../src/web.rs");
    let action = web
        .split("async fn command_action(")
        .nth(1)
        .and_then(|rest| rest.split("#[derive(Deserialize)]").next())
        .expect("command action handler");
    assert!(action.contains("if human_resolve {"));
    assert!(action.contains("response_value[\"web_result_class\"] = json!(result_class);"));
    let dispatch = action
        .find("command_envelope_on_host")
        .expect("authoritative dispatch");
    let class = action
        .find("human_result_class")
        .expect("result classification");
    let cache = action
        .find("complete_action_submission")
        .expect("cached submission result");
    let claim = action
        .find("claim_action_submission")
        .expect("idempotency claim");
    assert!(
        claim < dispatch,
        "the idempotency slot must be claimed before the command runs"
    );
    assert!(
        dispatch < class && class < cache,
        "the result class must be derived from the authoritative response before it is cached"
    );
}
