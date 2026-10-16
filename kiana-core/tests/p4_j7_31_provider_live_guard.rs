//! P4-J7-31 source guard for per-connection live provider evidence.

#[test]
fn provider_live_smoke_requires_real_config_and_does_not_accept_synthetic_claims() {
    let smoke = include_str!("../../scripts/provider-live-smoke.sh");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let request = include_str!("../../kiana-provider/src/request.rs");
    let response = include_str!("../../kiana-provider/src/response.rs");
    let live = include_str!("../../kiana-domain/src/provider_live.rs");
    let live_evidence = include_str!("../../kiana-domain/tests/p4_j7_31_live_evidence.rs");
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
        "ProviderLiveConnectionEvidence",
        "ProviderLiveConnectionMetadata",
        "provider_live_synthetic_evidence",
        "live_connection_metadata",
    ] {
        assert!(
            smoke.contains(marker)
                || provider.contains(marker)
                || request.contains(marker)
                || response.contains(marker)
                || live.contains(marker),
            "P4-J7-31 provider marker missing: {marker}"
        );
    }
    for marker in [
        "fails when no selected connection is configured",
        "synthetic_streaming_cannot_be_marked_verified",
        "authorized connection supplies an isolated account, fixed protocol/model/profile, budget",
        "unverified",
        "skip-if-unconfigured",
        "never a live success",
        "partial",
    ] {
        assert!(
            baseline.contains(marker) || live_evidence.contains(marker),
            "P4-J7-31 baseline marker missing: {marker}"
        );
    }
    assert!(!smoke.contains("status == \"passed\"") || smoke.contains("provider_id"));
    assert!(!smoke.contains("fake provider passed"));
}
