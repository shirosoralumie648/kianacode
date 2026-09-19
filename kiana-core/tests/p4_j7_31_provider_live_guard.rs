//! P4-J7-31 source guard for per-connection live provider evidence.

#[test]
fn provider_live_smoke_requires_real_config_and_does_not_accept_synthetic_claims() {
    let smoke = include_str!("../../scripts/provider-live-smoke.sh");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-31-provider-live-baseline.md");
    for marker in [
        "--required",
        "--skip-if-unconfigured",
        "provider_configured",
        "live",
        "tools",
        "live_text_passed",
        "live_tools_passed",
        "provider_id",
        "model",
        "usage",
        "credential_revision",
    ] {
        assert!(
            smoke.contains(marker)
                || provider.contains(marker)
                || request.contains(marker)
                || response.contains(marker),
            "P4-J7-31 provider marker missing: {marker}"
        );
    }
    for marker in [
        "required_live_smoke_fails_when_selected_connection_is_unconfigured",
        "live_smoke_rejects_synthetic_streaming_claim",
        "live_probe_uses_authorized_gateway_and_budget",
        "unverified",
        "skip-if-unconfigured",
        "not live success",
        "partial",
    ] {
        assert!(
            baseline.contains(marker),
            "P4-J7-31 baseline marker missing: {marker}"
        );
    }
    assert!(!smoke.contains("status == \"passed\"") || smoke.contains("provider_id"));
    assert!(!smoke.contains("fake provider passed"));
}
