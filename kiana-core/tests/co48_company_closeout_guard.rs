//! CO-48 source guard for CompanyOS model closeout and handoff.

#[test]
fn company_closeout_keeps_fake_golden_and_real_model_limits_explicit() {
    let company = include_str!("../../kiana-domain/src/company.rs");
    let business = include_str!("../../kiana-domain/src/company_business.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/company_governance.rs");
    let fake = include_str!("../../kiana-daemon/tests/p3_i06_company_golden.rs");
    let baseline = include_str!("../../docs/roadmap/co48-company-closeout-baseline.md");
    for marker in [
        "CompanyState",
        "CompanyCommand",
        "Review",
        "Delivery",
        "ClosingReceipt",
        "result_unknown",
        "reconcile",
        "evidence",
        "owner",
    ] {
        assert!(
            company.contains(marker)
                || business.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker)
                || fake.contains(marker),
            "CO-48 source marker missing: {marker}"
        );
    }
    for marker in [
        "fake-model",
        "real model",
        "live",
        "Receipt",
        "Review",
        "Delivery",
        "Closing",
        "partial",
        "handoff",
    ] {
        assert!(
            baseline.contains(marker),
            "CO-48 baseline marker missing: {marker}"
        );
    }
    assert!(!core.contains("CapabilityBroker::new"));
    assert!(!business.contains("println!("));
}
