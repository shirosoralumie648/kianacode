#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn normalized_usage_adapter_is_a_bounded_observation_boundary() {
    let provider =
        source_guard::production_source(include_str!("../../kiana-provider/src/usage_adapters.rs"));
    let usage = source_guard::production_source(include_str!("../../kiana-provider/src/usage.rs"));
    for marker in [
        "PROVIDER_USAGE_ADAPTER_SCHEMA",
        "normalize_provider_usage",
        "provider_usage_containment",
        "ProviderUsageContainment::declared",
        "check_containment",
        "prepared.route.model_id.clone()",
        "provider_usage_total_inconsistent",
        "provider_usage_containment_violated:",
        "UsageConfidence::Partial",
        "BillingUnknownReason::ProviderUnreported",
        "audio_output_tokens: None",
    ] {
        assert!(provider.contains(marker), "BQ-10 marker missing: {marker}");
    }
    let normalization = source_guard::compact_code(&source_guard::item_source(
        &usage,
        "pub fn normalize_model_reply(",
    ));
    assert!(normalization.contains("None=>(None,None,UsageConfidence::Unknown,Some(BillingUnknownReason::ProviderUnreported),)"));
    for marker in [
        "cache_read_tokens: None",
        "reasoning_output_tokens: None",
        "audio_input_tokens: None",
        "audio_output_tokens: None",
        "served_model_id = reply.output.model_id.clone()",
    ] {
        assert!(
            usage.contains(marker),
            "legacy adapter marker missing: {marker}"
        );
    }
    for forbidden in [
        "EventStore",
        "ControlPlane",
        "CapabilityBroker",
        "CapabilityRequest",
        "ApprovalRequest",
        "ModelCallPermit",
        "std::process::Command",
        "std::net",
        "TcpStream",
        "tokio::",
        "reqwest::",
        "secret",
        "credential",
        "permit",
        "authoriz",
        "grant",
    ] {
        assert!(
            !provider.contains(forbidden),
            "usage adapter gained authority: {forbidden}"
        );
        assert!(
            !usage.contains(forbidden),
            "legacy usage adapter gained authority: {forbidden}"
        );
    }
}

/// BQ-10 的夹具名与手动 lane、自动分片必须一起改动，避免“重命名了夹具、lane 却还在跑旧名字”。
#[test]
fn bq10_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    let lane = include_str!("../../.github/workflows/bq10-normalized-usage.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-provider --test bq10_normalized_usage --locked -- --test-threads=1",
        "cargo test -p kiana-core --test bq10_normalized_usage_guard --locked -- --test-threads=1",
        "cargo test -p kiana-provider --test bq10_normalized_usage --no-run --locked",
        "cargo test -p kiana-core --test bq10_normalized_usage_guard --no-run --locked",
    ] {
        assert!(lane.contains(marker), "BQ-10 lane marker missing: {marker}");
    }
    for forbidden in ["pull_request:", "push:", "schedule:", "continue-on-error"] {
        assert!(
            !lane.contains(forbidden),
            "BQ-10 lane must stay manual-only: {forbidden}"
        );
    }

    let fixtures = include_str!("../../kiana-provider/tests/bq10_normalized_usage.rs");
    for fixture in [
        "all_supported_provider_shapes_map_to_one_neutral_vector",
        "missing_usage_stays_unknown_and_partial_fields_never_become_zero",
        "reported_total_without_components_stays_partial_without_zero_filling",
        "malformed_or_inconsistent_provider_usage_is_rejected_before_settlement",
        "reported_total_contradictions_are_rejected",
        "requested_and_served_model_are_separate_observations_and_the_response_cannot_rewrite_identity",
        "server_run_identity_binding_is_enforced_before_usage_is_recorded",
        "provider_self_reported_fields_cannot_extend_or_authorize_the_observation",
        "declared_containment_matches_every_wire_dialect",
        "containment_contradictions_are_rejected_before_settlement",
        "normalized_digest_is_over_safe_shape_and_does_not_authorize_effects",
        "legacy_reply_without_served_model_keeps_model_observation_unknown",
    ] {
        assert!(
            fixtures.contains(fixture),
            "BQ-10 behaviour fixture missing: {fixture}"
        );
    }

    // 自动分片：kiana-provider 为整 crate 分片（覆盖 bq10_normalized_usage），
    // kiana-core 的分片表显式列出本 guard。
    let shards = include_str!("../../scripts/ci/test-shards.json");
    assert!(shards.contains("\"kiana-provider\""));
    assert!(shards.contains("bq10_normalized_usage_guard"));
}
