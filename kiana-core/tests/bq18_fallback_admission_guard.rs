//! BQ-18 core source and wiring guard plus ControlPlane facade fixtures (deny first).
//!
//! `include_str!` hits prove source markers, refusal codes, fixture names and CI wiring only; they
//! are not runtime evidence.  The facade fixtures execute `ControlPlaneFallbackAdmission::re_admit`
//! and pin the deny-first codes that must propagate out of the core admission boundary.

#[path = "support/source_guard.rs"]
mod source_guard;

use kiana_core::ControlPlaneFallbackAdmission;
use kiana_domain::*;

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/fallback_admission.rs");
const LIFECYCLE_SOURCE: &str = include_str!("../../kiana-domain/src/model_attempt_lifecycle.rs");
const SETTLEMENT_SOURCE: &str = include_str!("../../kiana-domain/src/billing_settlement_fold.rs");
const CORE_SOURCE: &str = include_str!("../src/fallback_admission.rs");
const CORE_LIB: &str = include_str!("../src/lib.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/bq18_fallback_admission.rs");
const PROVIDER_FIXTURES: &str =
    include_str!("../../kiana-provider/tests/bq18_fallback_admission.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq18-fallback-admission.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn route(provider: &str, model: &str, connection: &str) -> ModelRoute {
    ModelRoute {
        provider_id: provider.to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: connection.to_owned(),
        model_id: model.to_owned(),
        profile: connection.to_owned(),
        configuration_revision: "config.v1".to_owned(),
        streaming: true,
    }
}

fn capabilities() -> ModelCapabilities {
    ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Supported,
        images: CapabilitySupport::Supported,
        reasoning_replay: CapabilitySupport::Supported,
        context_window: 128_000,
        max_output: 4_096,
        source: "fixture".to_owned(),
        revision: "catalog.v1".to_owned(),
    }
}

fn card() -> RateCard {
    let mut card = RateCard::new(
        RateCardId::new(),
        "provider-b",
        "model-b",
        "USD",
        Some(2),
        Some(3),
        1_000,
        Some(10_000),
        2,
        "bq18-core-fixture",
    )
    .expect("card");
    card.request_price = Some(5);
    card.rate_card_digest = card.digest();
    card.validate().expect("valid card");
    card
}

fn requirements() -> FallbackCapabilityRequirements {
    FallbackCapabilityRequirements {
        require_tools: true,
        require_streaming: true,
        require_structured_output: true,
        require_images: true,
        require_reasoning_replay: false,
        min_context_window: 64_000,
        min_output_tokens: 1_024,
    }
}

fn request(original: &ModelRoute) -> FallbackAdmissionRequest {
    FallbackAdmissionRequest::new(
        RunId::new(),
        AttemptId::new(),
        original,
        digest('a'),
        7,
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        requirements(),
        2_000,
    )
    .expect("request")
}

fn candidate() -> FallbackAdmissionCandidate {
    FallbackAdmissionCandidate::new(
        route("provider-b", "model-b", "backup"),
        capabilities(),
        digest('a'),
        7,
        digest('b'),
        Some(digest('g')),
        digest('c'),
        Some(digest('f')),
        Some(card()),
        digest('h'),
    )
}

#[test]
fn control_plane_is_the_only_fallback_admission_boundary() {
    let production = source_guard::production_source(CORE_SOURCE);
    for marker in [
        "ControlPlaneFallbackAdmission",
        "re_admit",
        "re_admit_model_fallback",
        "admit_fallback_attempt",
        "FallbackRouteAllowlist",
        "connection_id",
        "authority_digest",
        "data_boundary_digest",
        "budget_reservation_digest",
        "rate_card_digest",
    ] {
        assert!(
            CORE_SOURCE.contains(marker)
                || DOMAIN_SOURCE.contains(marker)
                || CORE_LIB.contains(marker),
            "BQ-18 core marker missing: {marker}"
        );
    }
    // Core contains exactly one admission call site and exactly one ControlPlane facade.
    assert_eq!(
        production.matches("admit_fallback_attempt(").count(),
        1,
        "BQ-18 core must keep one admission call site"
    );
    assert_eq!(
        production.matches("re_admit_model_fallback").count(),
        1,
        "BQ-18 core must keep one ControlPlane facade"
    );
    assert_eq!(
        production
            .matches("ControlPlaneFallbackAdmission::re_admit(")
            .count(),
        1,
        "BQ-18 core must keep one facade call site"
    );
    for forbidden in [
        "tokio::spawn(provider_fallback",
        "tokio::spawn(fallback_loop",
        "send_fallback",
        "CapabilityBrokerPort::dispatch",
        "EventLog::append",
        "auto_success_unknown",
        "ProviderGateway",
        "reqwest::",
        "sqlx",
    ] {
        assert!(
            !production.contains(forbidden),
            "BQ-18 second authority/loop marker present: {forbidden}"
        );
    }
    // The ControlPlane method is a pure wrapper: it holds no dispatch, permit or event surface.
    let facade = source_guard::item_source(CORE_SOURCE, "pub fn re_admit_model_fallback");
    for forbidden in [
        "await",
        "spawn",
        "append",
        "dispatch",
        "permit_store",
        "provider",
    ] {
        assert!(
            !facade.contains(forbidden),
            "BQ-18 ControlPlane facade must stay pure: {forbidden}"
        );
    }
    let re_admit = source_guard::item_source(CORE_SOURCE, "pub fn re_admit(");
    for forbidden in ["await", "spawn", "append", "dispatch", "reqwest"] {
        assert!(
            !re_admit.contains(forbidden),
            "BQ-18 domain facade must stay pure: {forbidden}"
        );
    }
}

#[test]
fn fallback_attempt_keeps_unknown_and_permit_fences_visible() {
    for marker in [
        "FallbackAttemptReceipt",
        "FallbackReceiptLedger",
        "FallbackAttemptOutcome::Unknown",
        "fallback_cannot_reuse_original_permit",
        "fallback_cannot_reuse_original_budget_reservation",
        "fallback_rate_card_missing",
        "fallback_rate_card_expired",
        "fallback_credential_missing",
        "fallback_data_boundary_mismatch",
        "fallback_authority_epoch_stale",
        "fallback_receipt_usage_route_mismatch",
        "fallback_receipt_cost_usage_mismatch",
        "fallback_receipt_cost_outcome_mismatch",
        "fallback_attempt_receipt_conflict",
        "ModelAttemptState::Unknown",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker)
                || LIFECYCLE_SOURCE.contains(marker)
                || SETTLEMENT_SOURCE.contains(marker),
            "BQ-18 receipt/fence marker missing: {marker}"
        );
    }
    assert!(
        DOMAIN_SOURCE.contains("Unknown has no success path")
            || DOMAIN_SOURCE.contains("is_success")
    );
}

#[test]
fn control_plane_fallback_facade_is_deny_first_and_returns_a_fresh_attempt() {
    let original = route("provider-a", "model-a", "primary");
    let candidate = candidate();
    let allowlist = FallbackRouteAllowlist::from_routes(std::slice::from_ref(&candidate.route))
        .expect("allowlist");
    let request = request(&original);

    let admitted =
        ControlPlaneFallbackAdmission::re_admit(&request, &candidate, &allowlist, AttemptId::new())
            .expect("facade admits a fresh route");
    assert!(admitted.is_fresh_attempt());
    assert_eq!(admitted.allowlist_digest, allowlist.allowlist_digest);
    assert_eq!(
        admitted.original_route_digest,
        request.original_route_digest
    );
    admitted
        .validate()
        .expect("facade admission is self-checking");

    let mut capability = candidate.clone();
    capability.capabilities.tools = CapabilitySupport::Unsupported;
    assert_eq!(
        ControlPlaneFallbackAdmission::re_admit(
            &request,
            &capability,
            &allowlist,
            AttemptId::new()
        )
        .unwrap_err(),
        "fallback_capability_downgrade_rejected"
    );

    let mut reused_permit = candidate.clone();
    reused_permit.permit_digest = request.original_permit_digest.clone();
    assert_eq!(
        ControlPlaneFallbackAdmission::re_admit(
            &request,
            &reused_permit,
            &allowlist,
            AttemptId::new()
        )
        .unwrap_err(),
        "fallback_cannot_reuse_original_permit"
    );

    let mut drifted_data = candidate.clone();
    drifted_data.data_boundary_digest = digest('z');
    assert_eq!(
        ControlPlaneFallbackAdmission::re_admit(
            &request,
            &drifted_data,
            &allowlist,
            AttemptId::new()
        )
        .unwrap_err(),
        "fallback_data_boundary_mismatch"
    );

    let mut no_price = candidate;
    no_price.rate_card = None;
    assert_eq!(
        ControlPlaneFallbackAdmission::re_admit(&request, &no_price, &allowlist, AttemptId::new())
            .unwrap_err(),
        "fallback_rate_card_missing"
    );
}

#[test]
fn bq18_refusal_codes_and_fixture_names_stay_pinned() {
    // Literal stable codes: the string itself must exist in the contract source.
    for code in [
        "fallback_allowlist_invalid",
        "fallback_allowlist_duplicate_route",
        "fallback_allowlist_noncanonical",
        "fallback_route_not_allowlisted",
        "fallback_admission_request_invalid",
        "fallback_capability_requirement_invalid",
        "fallback_candidate_authority_invalid",
        "fallback_candidate_capabilities_invalid",
        "fallback_capability_downgrade_rejected",
        "fallback_authority_recheck_failed",
        "fallback_authority_epoch_stale",
        "fallback_data_boundary_mismatch",
        "fallback_budget_recheck_required",
        "fallback_budget_reservation_missing",
        "fallback_cannot_reuse_original_budget_reservation",
        "fallback_cannot_reuse_original_permit",
        "fallback_credential_missing",
        "fallback_rate_card_missing",
        "fallback_rate_card_expired",
        "fallback_rate_card_scope_mismatch",
        "fallback_route_must_change",
        "fallback_attempt_identity_invalid",
        "fallback_attempt_admission_invalid",
        "fallback_attempt_receipt_invalid",
        "fallback_receipt_usage_identity_mismatch",
        "fallback_receipt_usage_route_missing",
        "fallback_receipt_usage_route_mismatch",
        "fallback_receipt_cost_identity_mismatch",
        "fallback_receipt_cost_usage_mismatch",
        "fallback_receipt_cost_outcome_mismatch",
        "fallback_attempt_receipt_conflict",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(code),
            "BQ-18 domain refusal code missing: {code}"
        );
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-18 domain fixtures never exercise {code}"
        );
    }
    // Shape-derived codes: `required`/`digest` append `_invalid` to the bounded field name, so the
    // source must keep the field prefix while the fixtures pin the full refusal code.
    for (prefix, code) in [
        (
            "fallback_original_route_digest",
            "fallback_original_route_digest_invalid",
        ),
        (
            "fallback_authority_digest",
            "fallback_authority_digest_invalid",
        ),
        (
            "fallback_data_boundary_digest",
            "fallback_data_boundary_digest_invalid",
        ),
        ("fallback_budget_digest", "fallback_budget_digest_invalid"),
        (
            "fallback_original_budget_reservation_digest",
            "fallback_original_budget_reservation_digest_invalid",
        ),
        (
            "fallback_original_permit_digest",
            "fallback_original_permit_digest_invalid",
        ),
        (
            "fallback_admission_allowlist_digest",
            "fallback_admission_allowlist_digest_invalid",
        ),
        (
            "fallback_candidate_credential_revision",
            "fallback_candidate_credential_revision_invalid",
        ),
        (
            "fallback_candidate_budget_reservation_digest",
            "fallback_candidate_budget_reservation_digest_invalid",
        ),
        (
            "fallback_candidate_permit_digest",
            "fallback_candidate_permit_digest_invalid",
        ),
        (
            "fallback_candidate_provider",
            "fallback_candidate_provider_invalid",
        ),
        (
            "fallback_candidate_connection",
            "fallback_candidate_connection_invalid",
        ),
        (
            "fallback_candidate_model",
            "fallback_candidate_model_invalid",
        ),
        (
            "fallback_candidate_profile",
            "fallback_candidate_profile_invalid",
        ),
        (
            "fallback_candidate_configuration_revision",
            "fallback_candidate_configuration_revision_invalid",
        ),
    ] {
        assert!(
            DOMAIN_SOURCE.contains(prefix),
            "BQ-18 domain field prefix missing: {prefix}"
        );
        assert!(
            DOMAIN_FIXTURES.contains(code),
            "BQ-18 domain fixtures never exercise {code}"
        );
    }

    for name in [
        "fallback_rechecks_route_authority_data_budget_credential_price_and_permit",
        "capability_downgrade_is_refused_per_dimension_and_on_the_route_streaming_flag",
        "missing_credential_budget_or_price_is_refused",
        "fallback_route_must_change_and_attempt_identity_must_be_fresh",
        "admission_records_the_original_route_and_allowlist_digest",
        "allowlist_pins_exact_route_and_model_identity",
        "allowlist_rejects_connection_identity_drift",
        "allowlist_can_pin_multiple_connections_for_the_same_provider_and_model",
        "allowlist_tamper_is_refused_before_contains_or_admission",
        "request_and_admission_digests_are_fenced",
        "every_fallback_attempt_has_independent_usage_rate_card_and_receipt",
        "receipt_refuses_foreign_usage_and_cost_observations",
        "receipt_keeps_estimated_measured_and_unknown_separate",
        "fallback_receipt_ledger_rejects_cross_attempt_reuse_and_conflict",
        "fallback_contracts_reject_unknown_fields_and_raw_model_text",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(name),
            "BQ-18 domain fixture missing: {name}"
        );
    }

    for name in [
        "provider_fallback_boundary_is_validation_only_and_route_bound",
        "provider_fallback_rejects_connection_identity_drift",
        "provider_accepts_only_the_exact_admitted_route_credential_and_permit",
    ] {
        assert!(
            PROVIDER_FIXTURES.contains(name),
            "BQ-18 provider fixture missing: {name}"
        );
    }
    for code in [
        "fallback_route_admission_drift",
        "fallback_credential_revision_drift",
        "fallback_permit_admission_drift",
    ] {
        assert!(
            PROVIDER_FIXTURES.contains(code),
            "BQ-18 provider boundary code missing from fixtures: {code}"
        );
    }

    for (name, source) in [("domain", DOMAIN_FIXTURES), ("provider", PROVIDER_FIXTURES)] {
        assert!(
            !source.contains("#[ignore"),
            "BQ-18 {name} fixtures must not be ignored"
        );
    }
}

#[test]
fn bq18_fixtures_are_wired_to_the_manual_lane_and_the_auto_shards() {
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test bq18_fallback_admission --locked -- --test-threads=1",
        "cargo test -p kiana-provider --test bq18_fallback_admission --locked -- --test-threads=1",
        "cargo test -p kiana-provider --lib --locked fallback -- --test-threads=1",
        "cargo test -p kiana-core --test bq18_fallback_admission_guard --locked -- --test-threads=1",
        "cargo test -p kiana-domain --test p4_j7_25_capacity_fallback --locked -- --test-threads=1",
        "cargo test -p kiana-provider --test p4_j7_25_capacity_fallback --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-18 manual lane missing: {command}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-18 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("contents: read"));

    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let domain_targets = shards["kiana-domain-s1/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("bq18_fallback_admission")));
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("bq18_fallback_admission_guard")));
    // The provider crate is a whole-crate shard, so its fixtures and guards are discovered
    // automatically; the regression target is covered by the same shard.
    assert!(shards["kiana-provider"]["targets"].is_null());
}
