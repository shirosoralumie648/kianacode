#[test]
fn quota_reservation_has_digest_idempotency_cas_and_fence_markers() {
    let domain = include_str!("../../kiana-domain/src/billing_reservation.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "QuotaReservation",
        "QuotaReservationState",
        "authority_epoch",
        "config_revision",
        "idempotency_key",
        "reservation_digest",
        "revision",
        "quota_reservation_state_transition_invalid",
    ] {
        assert!(
            domain.contains(marker),
            "reservation marker missing: {marker}"
        );
    }
    for marker in [
        "QuotaReservationPort",
        "InMemoryQuotaReservationStore",
        "quota_reservation_digest_or_revision_conflict",
        "quota_reservation_revision_stale",
        "quota_reservation_fence_stale",
    ] {
        assert!(
            ports.contains(marker),
            "reservation port marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest",
        "tokio::spawn",
        "CapabilityBroker",
        "FinancialBudget",
    ] {
        assert!(
            !domain.contains(forbidden),
            "domain authority widened: {forbidden}"
        );
    }
}
