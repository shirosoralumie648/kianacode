//! NM-18 source guard: the recovery gate stays a pure planner with no connector, Broker or
//! automatic retry authority, and every fixture stays wired into the automatic CI shard plus the
//! manual NM-18 lane.

#[test]
fn notification_recovery_has_no_connector_or_automatic_retry_authority() {
    let core = include_str!("../src/notification_recovery.rs");
    let domain = include_str!("../../kiana-domain/src/notification_recovery.rs");
    for marker in [
        "plan_notification_recovery",
        "plan_notification_outbox_recovery",
        "RetryRequiresReAdmission",
        "AwaitReconciliation",
        "TerminalNoSend",
        "delivery_started",
        "new_authority_and_delivery_key_required",
        "cancel_requested_before_send",
        "delivery_outcome_unknown",
        "NotificationDeadLetter",
        "NotificationDeadLetterKind",
        "case_key",
        "auto_retry_allowed",
        "requires_reconciliation",
        "NotificationRetryAdmission",
        "notification_retry_admission_delivery_key_reused",
        "notification_retry_admission_epoch_not_advanced",
        "NotificationReconciliationOutcome",
        "NotificationRecoveryResolution",
        "TerminalAcknowledged",
        "RetryAdmissionRequired",
        "RemainsDeadLettered",
        "notification_reconciliation_no_case",
        "notification_recovery_foreign_live_claim",
        "NotificationAttemptObservation",
    ] {
        assert!(
            core.contains(marker) || domain.contains(marker),
            "NM-18 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "Connector",
        "reqwest::Client",
        "tokio::spawn",
        "tokio",
        "std::process::Command",
        "std::fs",
        "send(",
        "retry(",
        "EventLog",
        "append_event",
        "NotificationOutboxStore",
        "async fn",
    ] {
        assert!(
            !core.contains(forbidden),
            "NM-18 core authority widened: {forbidden}"
        );
        assert!(
            !domain.contains(forbidden),
            "NM-18 domain authority widened: {forbidden}"
        );
    }
}

#[test]
fn notification_recovery_fixtures_stay_wired_to_the_lane_shard_and_baseline() {
    let fixtures = include_str!("nm18_notification_recovery.rs");
    for fixture in [
        "pending_is_dispatchable_but_known_pre_send_failure_requires_new_admission",
        "in_flight_submitted_and_unknown_never_auto_retry",
        "cancel_revoke_expiry_and_ack_are_no_send_boundaries",
        "cancel_before_send_can_never_be_reconciled_as_delivered",
        "authority_epoch_and_subscription_or_expiry_drift_require_reconciliation",
        "revoke_or_expiry_after_a_possible_send_is_not_reclassified_as_no_send",
        "retry_admission_requires_a_fresh_delivery_key_and_authority_epoch",
        "reconciliation_closes_only_with_evidence_and_never_auto_retries",
        "outbox_bridge_dead_letters_uncertain_sends_and_refuses_foreign_claims",
        "outbox_bridge_cancel_is_terminal_before_send_and_revocation_is_never_dispatched",
        "bounded_retry_limit_and_fixture_fail_closed",
    ] {
        assert!(fixtures.contains(fixture), "fixture missing: {fixture}");
    }
    for forbidden in ["#[ignore", "--ignored"] {
        assert!(
            !fixtures.contains(forbidden),
            "NM-18 fixtures must not be ignored: {forbidden}"
        );
    }

    let fixture = include_str!("fixtures/nm18-notification-recovery.json");
    for marker in [
        "\"proof_ceiling\": \"source\"",
        "\"dead_letter_kinds\"",
        "\"dead_letter_requirements\"",
        "cancel_requested_is_never_reconciled_as_delivered",
        "revoked_or_expired_after_send_started_is_not_terminal_no_send",
        "retry_admission_cannot_reuse_the_previous_delivery_key",
        "retry_admission_cannot_keep_the_previous_authority_epoch",
        "reconciliation_requires_an_evidence_digest",
        "reconciliation_unknown_never_auto_retries",
        "uncertainty_is_dead_lettered_with_a_typed_case",
        "reconciliation_delivered_is_terminal_acknowledged",
        "reconciliation_not_delivered_requires_re_admission",
        "fresh_authority_and_delivery_key_admit_one_retry",
    ] {
        assert!(fixture.contains(marker), "fixture marker missing: {marker}");
    }

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm18_notification_recovery\"",
        "\"nm18_notification_recovery_guard\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm18-notification-recovery-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "NotificationDeadLetter",
        "NotificationRetryAdmission",
        "NotificationReconciliationOutcome",
        "plan_notification_outbox_recovery",
        "case_key",
        "notification_reconciliation_no_case",
        "notification_recovery_foreign_live_claim",
        "TerminalAcknowledged",
        "RetryAdmissionRequired",
        "RemainsDeadLettered",
        "kiana-core-s4/6",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm18-notification-recovery.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm18_notification_recovery --locked",
        "cargo test -p kiana-core --test nm18_notification_recovery_guard --locked",
        "--no-run",
    ] {
        assert!(lane.contains(marker), "NM-18 lane marker missing: {marker}");
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !lane.contains(forbidden),
            "NM-18 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}
