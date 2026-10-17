//! SC-30 source guard for provider/model/prompt-pack/MCP route attestation.

const SC30_DOMAIN_REASON_CODES: [&str; 23] = [
    "route_attestation_route_mismatch",
    "route_attestation_provider_claim_mismatch",
    "route_attestation_prompt_pack_mismatch",
    "route_attestation_prompt_pack_untrusted",
    "route_attestation_prompt_pack_unknown",
    "route_attestation_mcp_route_mismatch",
    "route_attestation_mcp_transport_unsupported",
    "route_attestation_mcp_disabled",
    "route_attestation_data_policy_mismatch",
    "route_attestation_credential_mismatch",
    "route_attestation_account_mismatch",
    "route_attestation_audience_mismatch",
    "route_attestation_downstream_token_missing",
    "route_attestation_downstream_token_mismatch",
    "route_attestation_downstream_token_account_mismatch",
    "route_attestation_downstream_token_audience_mismatch",
    "route_attestation_downstream_token_purpose_mismatch",
    "route_attestation_release_binding_mismatch",
    "route_attestation_provider_unverified",
    "route_attestation_purpose_not_permitted",
    "route_attestation_data_classes_invalid",
    "route_attestation_data_class_unknown",
    "route_attestation_data_class_not_permitted",
];

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
        "DownstreamTokenAttestation",
        "with_downstream_token",
        "authorize_use",
        "RouteDataUseDecision",
        "route_attestation_route_mismatch",
        "route_attestation_provider_claim_mismatch",
        "route_attestation_prompt_pack_untrusted",
        "route_attestation_data_policy_mismatch",
        "route_attestation_credential_mismatch",
        "route_attestation_account_mismatch",
        "route_attestation_audience_mismatch",
        "route_attestation_mcp_transport_unsupported",
        "route_attestation_downstream_token_missing",
        "route_attestation_downstream_token_mismatch",
        "route_attestation_purpose_not_permitted",
        "route_attestation_data_class_not_permitted",
        "route_attestation_provider_unverified",
        "RouteVerificationStatus::Unknown",
    ] {
        assert!(
            domain.contains(marker),
            "SC-30 domain marker missing: {marker}"
        );
    }
    // The metadata boundary stays pure: no provider client, network stack, process spawn,
    // filesystem access or unsafe block lives in the contract.
    for forbidden in [
        "reqwest",
        "std::net",
        "std::process",
        "std::fs",
        "tokio::",
        "Command::new",
        "unsafe",
    ] {
        assert!(
            !domain.contains(forbidden),
            "SC-30 domain contract must stay offline and side-effect free: {forbidden}"
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
        "workflow_dispatch:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "python3 -B scripts/tests/test_sc30_route_attestation.py",
        "python3 scripts/verify-route-attestation.py --self-test",
        "bash scripts/validate-sc30-route-attestation.sh",
        "cargo test -p kiana-domain --test sc30_route_attestation",
        "cargo test -p kiana-core --test sc30_route_attestation_guard",
        "--no-run",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-30 workflow marker missing: {marker}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "SC-30 lane must stay manual and fail-closed: {forbidden}"
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

#[test]
fn sc30_deny_first_fixtures_codes_and_lane_are_wired() {
    let domain = include_str!("../../kiana-domain/src/route_attestation.rs");
    let fixture = include_str!("../../kiana-domain/tests/sc30_route_attestation.rs");
    let verifier = include_str!("../../scripts/verify-route-attestation.py");
    let python_fixture = include_str!("../../scripts/tests/test_sc30_route_attestation.py");
    let validator = include_str!("../../scripts/validate-sc30-route-attestation.sh");
    let workflow = include_str!("../../.github/workflows/sc30-route-attestation.yml");
    let baseline = include_str!("../../docs/roadmap/sc30-route-attestation-baseline.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let shards: serde_json::Value =
        serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
            .expect("test shard book");

    // Every refusal reason is a literal in the domain contract, not a re-built string, so the
    // guard matrix below is the same text CI executes.
    for code in SC30_DOMAIN_REASON_CODES {
        assert!(
            domain.contains(code),
            "SC-30 domain reason code missing: {code}"
        );
    }

    // Deny-first: the single success fixture is the last domain fixture in the file.
    let domain_fixtures = [
        "provider_claim_and_route_drift_are_blocked_before_dispatch",
        "unreviewed_prompt_packs_and_unverified_providers_are_denied_or_unknown",
        "downstream_token_mixing_across_account_audience_and_purpose_is_refused",
        "data_use_policy_denies_out_of_scope_purpose_and_class_escalation",
        "policy_credential_account_audience_and_mcp_transport_drift_is_denied",
        "strict_serde_and_digest_fences_reject_forged_values",
        "exact_provider_model_prompt_policy_and_mcp_binding_verifies",
    ];
    let success = domain_fixtures[domain_fixtures.len() - 1];
    let success_at = fixture
        .find(success)
        .unwrap_or_else(|| panic!("SC-30 success fixture missing: {success}"));
    for fixture_name in &domain_fixtures[..domain_fixtures.len() - 1] {
        let at = fixture
            .find(*fixture_name)
            .unwrap_or_else(|| panic!("SC-30 deny fixture missing: {fixture_name}"));
        assert!(
            at < success_at,
            "SC-30 deny fixture must run before the success fixture: {fixture_name}"
        );
    }

    let verifier_fixtures = [
        "test_missing_or_malformed_evidence_is_refused",
        "test_unknown_fields_are_refused_before_any_binding_check",
        "test_provider_reported_models_and_routes_cannot_override_the_snapshot",
        "test_unreviewed_prompt_pack_trust_and_drift_are_refused_or_unknown",
        "test_downstream_token_mixing_accounts_audiences_and_purposes_is_refused",
        "test_data_use_policy_scope_and_digest_fences_are_enforced",
        "test_mcp_descriptor_transport_and_disabled_routes_are_refused",
        "test_release_binding_version_and_bounds_are_enforced",
        "test_verifier_stays_offline_and_does_not_mutate_evidence",
        "test_valid_fixture_verifies_last_and_output_is_deterministic",
    ];
    let success = verifier_fixtures[verifier_fixtures.len() - 1];
    let success_at = python_fixture
        .find(success)
        .unwrap_or_else(|| panic!("SC-30 verifier success fixture missing: {success}"));
    for fixture_name in &verifier_fixtures[..verifier_fixtures.len() - 1] {
        let at = python_fixture
            .find(*fixture_name)
            .unwrap_or_else(|| panic!("SC-30 verifier deny fixture missing: {fixture_name}"));
        assert!(
            at < success_at,
            "SC-30 verifier deny fixture must run before the success fixture: {fixture_name}"
        );
    }

    // The verifier mirrors the domain refusal codes and stays offline.
    for code in [
        "route_attestation_route_mismatch",
        "route_attestation_provider_claim_mismatch",
        "route_attestation_prompt_pack_mismatch",
        "route_attestation_prompt_pack_untrusted",
        "route_attestation_prompt_pack_unknown",
        "route_attestation_mcp_route_mismatch",
        "route_attestation_mcp_transport_unsupported",
        "route_attestation_mcp_disabled",
        "route_attestation_data_policy_mismatch",
        "route_attestation_downstream_token_missing",
        "route_attestation_downstream_token_mismatch",
        "route_attestation_release_binding_mismatch",
        "route_attestation_binding_mismatch",
        "route_downstream_token_digest_mismatch",
    ] {
        assert!(
            verifier.contains(code),
            "SC-30 verifier reason code missing: {code}"
        );
    }
    for forbidden in [
        "subprocess",
        "socket",
        "urllib",
        "requests",
        "http.client",
        "os.system",
        "eval(",
    ] {
        assert!(
            !verifier.contains(forbidden),
            "SC-30 verifier must stay offline: {forbidden}"
        );
    }

    // The manual lane and the unified gate execute the same fixtures.
    for marker in [
        "python3 -B scripts/tests/test_sc30_route_attestation.py",
        "python3 scripts/verify-route-attestation.py --self-test",
        "bash scripts/validate-sc30-route-attestation.sh",
        "cargo test -p kiana-domain --test sc30_route_attestation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test sc30_route_attestation_guard --locked -- --test-threads=1",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-30 workflow marker missing: {marker}"
        );
    }
    assert!(validator.contains("python3 -B scripts/tests/test_sc30_route_attestation.py"));
    assert!(validator.contains("python3 scripts/verify-route-attestation.py --self-test"));
    assert!(baseline.contains("2026-10-17 completion slice (CI-only)"));

    // The automatic shards keep claiming both targets; the book is not edited by this step.
    let domain_targets = shards["kiana-domain-s4/4"]["targets"]
        .as_array()
        .expect("kiana-domain-s4/4 targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("sc30_route_attestation")));
    let core_targets = shards["kiana-core-s6/6"]["targets"]
        .as_array()
        .expect("kiana-core-s6/6 targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("sc30_route_attestation_guard")));

    // Queue row and card both stay anchored to the deny-first completion slice.
    assert!(roadmap.contains("[`SC-30`](roadmap/security-compliance.md#step-sc-30)"));
    assert!(roadmap.contains("route_attestation_downstream_token_mismatch"));
    assert!(roadmap.contains("route_attestation_purpose_not_permitted"));
}
