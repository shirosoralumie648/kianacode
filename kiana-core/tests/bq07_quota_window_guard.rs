#[test]
fn quota_window_and_group_contracts_use_trusted_utc_clock() {
    let quota = include_str!("../../kiana-domain/src/billing_quota.rs");
    for marker in [
        "QuotaWindow",
        "QuotaGroupKey",
        "QuotaWindowBudget",
        "ClockObservation",
        "clock_untrusted",
        "quota_clock_rollback",
        "timezone: \"UTC\"",
        "retry_after_ms",
        "alias",
        "credential_group",
    ] {
        assert!(quota.contains(marker), "quota marker missing: {marker}");
    }
    for forbidden in [
        "SystemTime",
        "reqwest",
        "tokio::spawn",
        "CapabilityBroker",
        "FinancialBudget",
    ] {
        assert!(
            !quota.contains(forbidden),
            "quota boundary widened: {forbidden}"
        );
    }
}
