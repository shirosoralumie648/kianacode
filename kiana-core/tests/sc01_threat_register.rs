#[test]
fn threat_register_is_explicit_and_deny_first_without_claiming_enforcement() {
    let register = include_str!("../../docs/roadmap/security-threat-register.md");
    let baseline = include_str!("../../docs/roadmap/security-compliance-baseline.md");
    let constitution = include_str!("../../docs/company-os-security-constitution.md");
    let roadmap = include_str!("../../docs/roadmap/security-compliance.md");
    for marker in [
        "T01",
        "T02",
        "T03",
        "T04",
        "T05",
        "T06",
        "T07",
        "T08",
        "T09",
        "T10",
        "T11",
        "T12",
        "CI-only security fixture catalog",
        "EventLog facts are authoritative",
        "model/UI",
        "external/physical",
        "source snapshot",
        "limitations",
    ] {
        assert!(
            register.contains(marker),
            "threat register marker missing: {marker}"
        );
    }
    assert!(baseline.contains("SC-00"));
    assert!(constitution.contains("SEC-01"));
    assert!(roadmap.contains("step-sc-01"));
    assert!(!register.contains("security certification"));
    assert!(!register.contains("feature_status=live"));
}
