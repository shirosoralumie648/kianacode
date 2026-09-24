use serde_json::Value;

#[test]
fn ui19_fixture_captures_owner_and_multi_tab_contract() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/ui19-session-tabs.json"))
        .expect("valid UI-19 session/tab fixture");
    assert_eq!(fixture["schema"], "kiana.ui-tab-session.v1");
    assert_eq!(fixture["submission_schema"], "kiana.ui-action-submission.v1");
    assert_eq!(fixture["authority"], "DaemonHost -> ControlPlane -> Broker");
    for field in ["principal", "session_id", "tab_id", "lease_epoch", "token_generation"] {
        assert!(fixture["scope"].as_array().unwrap().iter().any(|item| item == field));
    }
    for denial in [
        "tab_a_operates_tab_b_private_session",
        "stale_card_after_feed_revision",
        "duplicate_click_two_effects",
        "closed_tab_cancels_run",
        "token_rotation_old_credential",
        "owner_exit_without_cancel",
        "cas_race_second_writer",
        "observer_tab_action",
    ] {
        assert!(fixture["deny_first"].as_array().unwrap().iter().any(|item| item == denial));
    }
    for success in [
        "tab_local_draft_and_submission",
        "original_command_receipt_on_replay",
        "feed_only_cross_tab_observation",
        "owner_tab_refresh_rehydrates",
        "browser_sleep_reconnects_feed",
    ] {
        assert!(fixture["success"].as_array().unwrap().iter().any(|item| item == success));
    }
}

#[test]
fn ui19_source_exposes_server_lease_and_replay_fences() {
    let web = include_str!("../src/web.rs");
    let page = include_str!("../src/web_page.html");
    let client = include_str!("../../kiana-client/src/web_contract.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    for marker in [
        "WEB_TAB_SESSION_SCHEMA",
        "WEB_ACTION_SUBMISSION_SCHEMA",
        "token_generation",
        "owner_tab_id",
        "claim_session_tab",
        "require_session_owner",
        "authorize_feed_tab",
        "claim_action_submission",
        "complete_action_submission",
        "ui_action_in_flight",
        "ui_action_owner_mismatch",
        "rotate_web_token",
        "DaemonHost",
        "ControlPlane",
    ] {
        assert!(web.contains(marker), "UI-19 web marker missing: {marker}");
    }
    for marker in [
        "UI_TAB_SESSION_SCHEMA",
        "UI_ACTION_SUBMISSION_SCHEMA",
        "UiTabSessionV1",
        "UiActionSubmissionV1",
        "feed_only",
    ] {
        assert!(protocol.contains(marker), "UI-19 protocol marker missing: {marker}");
    }
    for marker in [
        "WebClientTab",
        "WebClientDraft",
        "WebClientSubmission",
        "can_mutate",
        "WEB_CLIENT_MAX_DRAFT_BYTES",
    ] {
        assert!(client.contains(marker), "UI-19 client marker missing: {marker}");
    }
    for marker in [
        "tabOwnerId",
        "tabDrafts",
        "submissionIds",
        "stableSubmissionId",
        "x-kiana-ui-tab",
        "tab_id=" ,
        "owner tab",
        "original server result",
    ] {
        assert!(page.contains(marker), "UI-19 page marker missing: {marker}");
    }
}
