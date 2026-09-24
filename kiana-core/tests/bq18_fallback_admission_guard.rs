#[test]
fn control_plane_is_the_only_fallback_admission_boundary() {
    let core = include_str!("../src/fallback_admission.rs");
    let lib = include_str!("../src/lib.rs");
    let domain = include_str!("../../kiana-domain/src/fallback_admission.rs");
    for marker in [
        "ControlPlaneFallbackAdmission",
        "re_admit_model_fallback",
        "admit_fallback_attempt",
        "FallbackRouteAllowlist",
        "authority_digest",
        "data_boundary_digest",
        "budget_reservation_digest",
        "rate_card_digest",
    ] {
        assert!(
            core.contains(marker) || domain.contains(marker) || lib.contains(marker),
            "BQ-18 core marker missing: {marker}"
        );
    }
    for forbidden in [
        "tokio::spawn(provider_fallback",
        "tokio::spawn(fallback_loop",
        "send_fallback",
        "CapabilityBrokerPort::dispatch",
        "EventLog::append",
        "auto_success_unknown",
    ] {
        assert!(
            !core.contains(forbidden),
            "BQ-18 second authority/loop marker present: {forbidden}"
        );
    }
}

#[test]
fn fallback_attempt_keeps_unknown_and_permit_fences_visible() {
    let domain = include_str!("../../kiana-domain/src/fallback_admission.rs");
    let lifecycle = include_str!("../../kiana-domain/src/model_attempt_lifecycle.rs");
    let settlement = include_str!("../../kiana-domain/src/billing_settlement_fold.rs");
    for marker in [
        "FallbackAttemptReceipt",
        "FallbackReceiptLedger",
        "FallbackAttemptOutcome::Unknown",
        "fallback_cannot_reuse_original_permit",
        "fallback_rate_card_missing",
        "fallback_data_boundary_mismatch",
        "ModelAttemptState::Unknown",
    ] {
        assert!(
            domain.contains(marker) || lifecycle.contains(marker) || settlement.contains(marker),
            "BQ-18 receipt/fence marker missing: {marker}"
        );
    }
    assert!(domain.contains("Unknown has no success path") || domain.contains("is_success"));
}
