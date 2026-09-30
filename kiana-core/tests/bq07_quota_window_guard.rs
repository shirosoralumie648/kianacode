#[test]
fn quota_window_and_group_contracts_use_trusted_utc_clock() {
    let quota = include_str!("../../kiana-domain/src/billing_quota.rs");
    // `clock_untrusted` 由 clock.rs 发出，billing_quota.rs 只调用
    // `clock.require_trusted()?`。信任栅栏在，两边都要断，所以并上 clock。
    let clock = include_str!("../../kiana-domain/src/clock.rs");
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
        assert!(
            quota.contains(marker) || clock.contains(marker),
            "quota marker missing: {marker}"
        );
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
