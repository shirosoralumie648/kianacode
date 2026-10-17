//! NM-10 source guard: notification actions stay a ControlPlane admission + single-consume ledger,
//! never a second execution path, and every fixture stays wired into the auto shards and the
//! manual NM-10 lane.

#[test]
fn notification_actions_are_control_plane_admission_without_a_second_effect_path() {
    let gate = include_str!("../src/notification_action.rs");
    let domain = include_str!("../../kiana-domain/src/notification_actions.rs");
    for marker in [
        "NotificationActionGate",
        "NotificationActionAdmission",
        "NotificationActionTarget",
        "NotificationActionConsumption",
        "NotificationActionWake",
        "HumanTaskActionLedger",
        "HumanTaskActionLedgerError",
        "NotificationActionDecision",
        "NotificationActionWaitKey",
        "NotificationActionAuthorityEffect",
        "NotificationActionOutcome",
        "NotificationActionKind::REGISTERED",
        "from_command",
        "is_presentation_only",
        "authority_effect",
        "UnknownAction",
        "CriteriaConflict",
        "AdmissionMismatch",
        "AlreadyConsumed",
        "IdempotencyConflict",
        "WaitKeyConflict",
        "WaitKeyAmbiguous",
        "WaitKeyConsumed",
        "WaitKeyBindingMismatch",
        "control_plane_required",
        "direct_effect",
        "admit_notification_action",
    ] {
        assert!(
            gate.contains(marker) || domain.contains(marker),
            "NM-10 marker missing: {marker}"
        );
    }
    for source in [gate, domain] {
        for forbidden in [
            "CapabilityBroker",
            "EventStorePort",
            "tokio::spawn",
            "KianaHarness",
            "HumanTaskStatus::Decided",
            ".append(",
            "execute(",
            "NotificationStore",
            "DeliveryReceipt",
            "reqwest",
            "std::fs",
            "std::process",
            "std::net",
        ] {
            assert!(
                !source.contains(forbidden),
                "NM-10 direct effect marker: {forbidden}"
            );
        }
    }
}

#[test]
fn nm10_fixtures_stay_wired_to_the_lane_shards_and_baseline() {
    let core_fixtures = include_str!("nm10_notification_action.rs");
    let domain_fixtures = include_str!("../../kiana-domain/tests/nm10_notification_actions.rs");
    for fixture in [
        "gate_refuses_stale_target_wrong_decider_criteria_epoch_cursor_and_unknown_action",
        "gate_accepts_only_authoritative_fresh_target_and_returns_no_effect",
        "ledger_consumes_once_and_replays_the_first_decision",
        "ledger_wakes_exactly_one_wait_key_per_decision",
        "delivery_and_read_ack_never_forge_an_authority_decision",
        "control_plane_facade_admits_only_bound_notification_actions",
    ] {
        assert!(
            core_fixtures.contains(fixture),
            "core fixture missing: {fixture}"
        );
    }
    for fixture in [
        "fixture_declares_action_command_deny_first_contract",
        "action_command_binds_ref_target_epoch_cursor_and_redacted_payload",
        "action_command_refuses_forged_decision_fields_and_unknown_actions",
        "action_commands_round_trip_and_reject_unknown_fields",
        "presentation_and_governance_actions_never_claim_task_decisions",
        "decision_outcomes_require_their_own_reason_shape",
        "wait_key_binds_one_exact_action_target",
    ] {
        assert!(
            domain_fixtures.contains(fixture),
            "domain fixture missing: {fixture}"
        );
    }
    for forbidden in ["#[ignore", "--ignored"] {
        assert!(
            !core_fixtures.contains(forbidden) && !domain_fixtures.contains(forbidden),
            "NM-10 fixtures must not be ignored: {forbidden}"
        );
    }

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm10_notification_action\"",
        "\"nm10_notification_action_guard\"",
        "\"nm10_notification_actions\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm10-notification-actions-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "Completion slice (2026-10-17, CI-only)",
        "NotificationActionGate",
        "HumanTaskActionLedger",
        "NotificationActionDecision",
        "ledger_wakes_exactly_one_wait_key_per_decision",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm10-notification-actions.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test nm10_notification_actions --locked",
        "cargo test -p kiana-core --test nm10_notification_action --locked",
        "cargo test -p kiana-core --test nm10_notification_action_guard --locked",
        "cargo test -p kiana-domain --test nm10_notification_actions --no-run --locked",
        "cargo test -p kiana-core --test nm10_notification_action --no-run --locked",
        "cargo test -p kiana-core --test nm10_notification_action_guard --no-run --locked",
    ] {
        assert!(lane.contains(marker), "NM-10 lane marker missing: {marker}");
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
            "NM-10 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}
