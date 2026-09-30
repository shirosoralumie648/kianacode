//! SC-30 source guard for provider/model/prompt-pack/MCP route attestation.

#[test]
fn sc30_attestation_binds_route_policy_credentials_and_audience() {
    let domain = include_str!("../../kiana-domain/src/route_attestation.rs");
    let fixture = include_str!("../../kiana-domain/tests/sc30_route_attestation.rs");
    let workflow = include_str!("../../.github/workflows/sc30-route-attestation.yml");
    let baseline = include_str!("../../docs/roadmap/sc30-route-attestation-baseline.md");
    let current_status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let card = include_str!("../../docs/roadmap/security-compliance.md");

    for marker in [
        "RouteAttestation",
        "ProviderRouteClaim",
        "PromptPackAttestation",
        "McpRouteAttestation",
        "RouteDataPolicyBinding",
        "route_attestation_route_mismatch",
        "route_attestation_provider_claim_mismatch",
        "route_attestation_prompt_pack_untrusted",
        "route_attestation_data_policy_mismatch",
        "route_attestation_credential_mismatch",
        "route_attestation_account_mismatch",
        "route_attestation_audience_mismatch",
        "route_attestation_mcp_transport_unsupported",
        "route_attestation_provider_unverified",
        "RouteVerificationStatus::Unknown",
    ] {
        assert!(
            domain.contains(marker),
            "SC-30 domain marker missing: {marker}"
        );
    }
    for marker in [
        "provider/model",
        "PromptPackTrust::Untrusted",
        "McpRouteTransport::Http",
        "route_attestation_provider_claim_mismatch",
        "route_attestation_data_policy_mismatch",
        "route_attestation_credential_mismatch",
        "route_attestation_mcp_transport_unsupported",
        "strict_serde_and_digest_fences_reject_forged_values",
    ] {
        assert!(
            fixture.contains(marker),
            "SC-30 fixture marker missing: {marker}"
        );
    }
    for marker in [
        "contents: read",
        "verify-route-attestation.py --self-test",
        "cargo test -p kiana-domain --test sc30_route_attestation",
        "cargo test -p kiana-core --test sc30_route_attestation_guard",
        "cargo fmt --all --check",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-30 workflow marker missing: {marker}"
        );
    }
    for marker in [
        "provider",
        "model",
        "prompt-pack",
        "MCP",
        "route digest",
        "data/use policy",
        "credential",
        "account",
        "audience",
        "Unknown",
        "limitations",
        "reviewer",
    ] {
        assert!(
            baseline.contains(marker),
            "SC-30 baseline marker missing: {marker}"
        );
    }
    assert!(current_status.contains("### SC-30"));
    // 【锚点在专项卡里，不在总队列里】同 SC-28：
    // 总队列 `docs/roadmap.md` 里是指向卡片的链接行，锚点写在
    // `docs/roadmap/security-compliance.md`。当它们合并归一个文件断言时，
    // 卡片在不在变成了队列行在不在，语义不对。
    assert!(card.contains("<a id=\"step-sc-30\"></a>SC-30"));
    assert!(roadmap.contains("[`SC-30`](roadmap/security-compliance.md#step-sc-30)"));
    assert!(!domain.contains("reqwest"));
    assert!(!domain.contains("std::net"));
}
