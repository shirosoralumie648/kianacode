#[test]
fn er14_effect_observation_is_owner_bound_and_unknown_safe() {
    let domain = include_str!("../../kiana-domain/src/effect_observation.rs");
    let connector = include_str!("../../kiana-daemon/src/connectors.rs");
    for marker in [
        "EffectObservation",
        "EffectObservationState",
        "EFFECT_OBSERVATION_SCHEMA",
        "from_provider_receipt",
        "owner_digest",
        "audience_digest",
        "idempotency_key_digest",
        "provider_receipt_id",
        "query_digest",
        "evidence_ref_digests",
        "effect_observation_owner_or_audience_mismatch",
        "effect_observation_provider_receipt_required",
        "ProviderOutcome::Unknown",
        "effect_observation",
        "connector_reconciliation_binding_mismatch",
    ] {
        assert!(
            domain.contains(marker) || connector.contains(marker),
            "ER-14 marker missing: {marker}"
        );
    }
    assert!(connector.contains("prior.outcome != ProviderOutcome::Unknown"));
    assert!(connector.contains("receipt.idempotency_key != prior.idempotency_key"));
    assert!(connector.contains("receipt.final_payload_sha256 != prior.final_payload_sha256"));
    for forbidden in [
        "send_external_request",
        "retry_without_idempotency",
        "unknown_is_no_effect",
        "raw_provider_result",
    ] {
        assert!(
            !domain.contains(forbidden),
            "observation must not {forbidden}"
        );
    }
}
