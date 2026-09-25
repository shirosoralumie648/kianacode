//! CO-47 source guard for the fake-model Company lifecycle and fault fixture.

#[test]
fn company_lifecycle_fixture_keeps_deny_and_effect_boundaries_explicit() {
    let fixture = include_str!("../../kiana-daemon/tests/company_lifecycle.rs");
    let cassette = include_str!("../../kiana-daemon/tests/fixtures/co47-company-lifecycle.json");
    let golden = include_str!("../../kiana-daemon/tests/p3_i06_company_golden.rs");
    let smoke = include_str!("../../scripts/company-os-business-smoke.sh");
    let baseline = include_str!("../../docs/roadmap/co47-company-lifecycle-baseline.md");

    for marker in [
        "DaemonHost",
        "RequestEnvelope::company_command",
        "company_role_denied",
        "company_idempotency_conflict",
        "company.command_rejected",
        "ExecutionStatus::Blocked",
        "persisted_events",
        "OUTPUT.txt",
        "fake_model_coding_project_produces_closing_receipt",
        "ResultUnknown",
        "reconcile",
        "GITHUB_ACTIONS",
        "remote_ci_required",
    ] {
        assert!(
            fixture.contains(marker)
                || cassette.contains(marker)
                || golden.contains(marker)
                || smoke.contains(marker)
                || baseline.contains(marker),
            "CO-47 marker missing: {marker}"
        );
    }
    assert!(!fixture.contains("CapabilityBroker::new"));
    assert!(!fixture.contains("ModelClient::new"));
    assert!(baseline.contains("feature_status=implemented"));
    assert!(baseline.contains("proof_level=source"));
    assert!(baseline.contains("limitations"));
}
