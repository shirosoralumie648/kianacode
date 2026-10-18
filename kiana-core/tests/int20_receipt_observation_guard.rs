//! INT-20 source and wiring guard: the provider receipt / effect observation contract is a shared
//! value contract, the Ports and daemon layers consume the server-owned receipt binding instead of
//! self-asserted scope, and the deny-first fixtures stay wired to the manual lane and the automatic
//! CI shards.  `include_str!` assertions are source-level evidence only; they are not runtime
//! receipts.

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/connectors.rs");
const DOMAIN_OBSERVATION: &str = include_str!("../../kiana-domain/src/effect_observation.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int20_receipt_observation.rs");
const PORTS_SOURCE: &str = include_str!("../../kiana-ports/src/connector.rs");
const PORTS_FIXTURES: &str = include_str!("../../kiana-ports/tests/int20_receipt_observation.rs");
const DAEMON_SOURCE: &str = include_str!("../../kiana-daemon/src/connectors.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int20-receipt-observation.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str = include_str!("../../docs/roadmap/int20-receipt-observation-baseline.md");

#[test]
fn receipt_observation_contract_is_shared_and_secret_free() {
    for marker in [
        "PROVIDER_RECEIPT_SCHEMA",
        "PROVIDER_RECEIPT_BINDING_SCHEMA",
        "PROVIDER_RECEIPT_PROJECTION_SCHEMA",
        "pub fn validate(&self)",
        "provider_payload_hash_valid",
        "normalize_provider_payload_hash",
        "provider_receipt_raw_response_forbidden",
        "provider_receipt_result_invalid",
        "provider_receipt_binding_mismatch",
        "provider_receipt_idempotency_mismatch",
        "provider_receipt_payload_mismatch",
        "pub fn validate_for_binding",
        "pub fn canonical_payload_sha256",
        "pub fn projection",
        "payload_sha256",
        "EffectObservationState::ConfirmedSuccess",
        "EffectObservationState::ConfirmedFailure",
        "EffectObservationState::Unknown",
        "validate_for_receipt",
        "validate_for_receipt_binding",
        "from_provider_receipt_for_binding",
        "effect_observation_evidence_refs_unordered",
        "EFFECT_OBSERVATION_PROJECTION_SCHEMA",
        "scan_secret_value",
        "receipt.validate()",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker)
                || DOMAIN_OBSERVATION.contains(marker)
                || PORTS_SOURCE.contains(marker)
                || DAEMON_SOURCE.contains(marker),
            "INT-20 marker missing: {marker}"
        );
    }

    assert!(DOMAIN_OBSERVATION.contains("#[serde(deny_unknown_fields)]"));
    assert!(DOMAIN_SOURCE.contains("final_payload_sha256"));
    assert!(DOMAIN_OBSERVATION.contains("owner_digest"));
    assert!(DOMAIN_OBSERVATION.contains("audience_digest"));
    assert!(PORTS_SOURCE.contains("ProviderReceiptBinding"));
    assert!(PORTS_SOURCE.contains("validate_for_binding"));
    assert!(DAEMON_SOURCE.contains("ProviderReceiptBinding::new"));
    assert!(DAEMON_SOURCE.contains("from_provider_receipt_for_binding"));
    assert!(!PORTS_SOURCE.contains("raw_provider_response"));
    assert!(!DAEMON_SOURCE.contains("raw_provider_response"));
    assert!(!DOMAIN_SOURCE.contains("raw_secret"));
    assert!(!DOMAIN_SOURCE.contains("EventStorePort"));
    assert!(!DOMAIN_OBSERVATION.contains("EventStorePort"));
}

#[test]
fn receipt_observation_completion_slice_stays_wired_to_fixtures_lane_and_shards() {
    // Deny-first domain fixtures: foreign identity/scope first, canonical projection last.
    for fixture in [
        "receipt_from_another_binding_account_operation_or_key_is_refused",
        "observation_scope_attempt_and_receipt_binding_fail_closed",
        "succeeded_failed_and_unknown_project_distinct_stable_evidence",
        "unordered_duplicate_or_forged_evidence_refs_are_refused",
        "raw_provider_response_shapes_never_leave_the_receipt",
        "receipt_projection_is_canonical_and_key_order_independent",
        "receipt_binding_digest_covers_every_identity_and_scope_field",
        "receipt_headers_and_unknown_outcomes_fail_closed",
        "existing_receipt_observation_projection_contract_still_holds",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "INT-20 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "observation_request_derives_its_binding_from_the_permit_only",
        "observation_projection_binds_attempt_scope_and_ordered_evidence",
    ] {
        assert!(
            PORTS_FIXTURES.contains(fixture),
            "INT-20 ports fixture missing: {fixture}"
        );
    }
    for fixtures in [DOMAIN_FIXTURES, PORTS_FIXTURES] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int20_receipt_observation --locked -- --test-threads=1",
        "cargo test -p kiana-ports --test int20_receipt_observation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int20_receipt_observation_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-20 lane step: {step}"
        );
    }
    assert_eq!(MANUAL_LANE.matches("--no-run").count(), 3);
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:\n  contents: read"));
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
        "|| true",
        "cargo test --workspace",
    ] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "INT-20 lane must not contain {forbidden}"
        );
    }

    // Automatic coverage: the domain fixture and the ports fixture are registered in the chunked
    // shards, and the core guard joins the same core shard.
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("test shards parse");
    let shard_map = shards.as_object().expect("shard map");
    let owner = |target: &str| -> Option<String> {
        shard_map.iter().find_map(|(name, entry)| {
            let targets = entry["targets"].as_array()?;
            targets
                .iter()
                .any(|value| value.as_str() == Some(target))
                .then(|| name.clone())
        })
    };
    assert_eq!(
        owner("int20_receipt_observation").as_deref(),
        Some("kiana-domain-s3/4"),
        "the domain fixture must stay in its registered shard"
    );
    assert_eq!(
        owner("int20_receipt_observation_guard").as_deref(),
        Some("kiana-core-s4/6"),
        "the core guard must stay in its registered shard"
    );
    assert!(shards["kiana-ports"]["targets"].is_null());

    assert!(BASELINE.contains("proof_level: source"));
    assert!(BASELINE.contains("provider_receipt_binding_mismatch"));
    assert!(BASELINE.contains("2026-10-17 completion slice (CI-only)"));
}
