//! ER-36 source guard for physical/live handoff boundaries.

#[test]
fn physical_live_handoff_is_explicit_opt_in_and_default_denied() {
    let handoff = include_str!("../../kiana-domain/src/live_handoff.rs");
    let runbook = include_str!("../../docs/roadmap/oa28-live-handoff.md");
    let script = include_str!("../../scripts/oa28-live-handoff-preflight.sh");
    let baseline = include_str!("../../docs/roadmap/er36-physical-live-handoff-baseline.md");
    for marker in [
        "LiveHandoffManifest",
        "LiveHandoffStatus",
        "NotSupported",
        "OptedIn",
        "Verified",
        "Unknown",
        "provider_receipt_ref",
        "operator_approval_ref",
        "cleanup_plan",
        "secret-ref:",
        "live_handoff_verified_evidence_incomplete",
    ] {
        assert!(
            handoff.contains(marker),
            "ER-36 handoff marker missing: {marker}"
        );
    }
    for marker in [
        "KIANA_LIVE_HANDOFF_OPT_IN",
        "live_opt_in_required",
        "raw_credential_ref_rejected",
        "provider receipt",
        "physical effect remains",
        "cleanup",
        "unknown",
    ] {
        assert!(
            runbook.contains(marker) || script.contains(marker) || baseline.contains(marker),
            "ER-36 boundary marker missing: {marker}"
        );
    }
    for marker in [
        "physical",
        "live",
        "not_supported",
        "operator approval",
        "provider receipt",
        "cleanup",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "ER-36 baseline marker missing: {marker}"
        );
    }
    assert!(!script.contains("curl "));
    assert!(!script.contains("docker "));
    assert!(!script.contains("ssh "));
}
