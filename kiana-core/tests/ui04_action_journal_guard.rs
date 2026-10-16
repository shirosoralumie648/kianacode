//! UI-04 source guard: UI actions are durable ControlPlane journal commands.

#[test]
fn ui_actions_use_server_owned_cas_idempotency_and_unknown_reconciliation() {
    let domain = include_str!("../../kiana-domain/src/ui_action.rs");
    let core = include_str!("../src/ui_actions.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let baseline = include_str!("../../docs/roadmap/ui04-action-cas-baseline.md");
    let ui_bridge_start = daemon
        .find("pub fn claim_ui_action")
        .expect("UI action bridge start missing");
    let ui_bridge_end = daemon[ui_bridge_start..]
        .find("pub async fn ui_snapshot_page")
        .map(|offset| ui_bridge_start + offset)
        .expect("UI action bridge end missing");
    assert!(ui_bridge_start < ui_bridge_end);
    let ui_bridge = &daemon[ui_bridge_start..ui_bridge_end];
    for marker in [
        "UiActionCommand",
        "UiActionJournal",
        "UiActionRecord",
        "idempotency_key",
        "payload_digest",
        "command_digest",
        "owner_id",
        "scope_digest",
        "expected_epoch",
        "expected_cursor",
        "expected_revision",
        "deadline_unix_ms",
        "permit_digest",
        "ui_action_permit_required",
        "ui_action_cancelled",
        "UiActionState::Accepted",
        "UiActionState::Applied",
        "UiActionState::Rejected",
        "UiActionState::Unknown",
        "effect_count",
        "query_original",
        "commit_transition",
        "result_unknown",
        "ui.action.accepted",
        "ui.action.applied",
        "ui_action_receipt_digest",
        "ui_action_applied_receipt_missing",
        "ui_action_receipt_state_mismatch",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker)
                || protocol.contains(marker),
            "UI-04 source marker missing: {marker}"
        );
    }
    for marker in [
        "Action CAS",
        "idempotency",
        "ACK",
        "Unknown",
        "permit",
        "owner",
        "scope",
        "durable",
        "feature_status",
        "proof_level",
        "valid_digest",
        "is_ascii_hexdigit",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-04 baseline marker missing: {marker}"
        );
    }
    assert!(core.contains("if !events.supports_atomic_transitions()"));
    assert!(core.contains("result_unknown:ui_action"));
    assert!(!ui_bridge.contains("CapabilityBroker::new"));
}
