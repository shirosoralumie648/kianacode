#[test]
fn cp20_unknown_reconciliation_is_evidence_gated_and_never_an_automatic_retry() {
    let errors = include_str!("../../kiana-domain/src/errors.rs");
    let platform_domain = include_str!("../../kiana-domain/src/platform.rs");
    let connectors = include_str!("../../kiana-domain/src/connectors.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let platform = include_str!("../src/platform.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let connector_daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let reliability = include_str!("p2_k6_01_reliability.rs");
    let connector_fixture = include_str!("../../kiana-domain/tests/p4_k8_01_connector.rs");

    for marker in [
        "ResultUnknown =>",
        "CompensationRequired =>",
        "requires_reconciliation",
        "requires_compensation",
        "automatic retry",
        "never permits automatic retry",
    ] {
        assert!(
            errors.contains(marker),
            "CP-20 error policy marker missing: {marker}"
        );
    }
    for marker in [
        "FailureClass",
        "RecoveryPlan",
        "reconciliation",
        "automatic_retry_allowed",
        "Record reconciliation evidence",
        "automatic_retry_allowed: false",
    ] {
        assert!(
            platform_domain.contains(marker),
            "CP-20 recovery contract marker missing: {marker}"
        );
    }
    for marker in [
        "ProviderOutcome::Unknown",
        "reconciliation_required",
        "idempotency_key",
        "connector.reconciled",
        "connector_invocation_risk",
    ] {
        assert!(
            connectors.contains(marker) || connector_daemon.contains(marker),
            "CP-20 connector marker missing: {marker}"
        );
    }
    for marker in [
        "EffectObservationState::Unknown",
        "idempotency_key_digest",
        "validate_for_scope",
        "from_provider_receipt",
    ] {
        assert!(
            effect.contains(marker),
            "CP-20 effect marker missing: {marker}"
        );
    }
    for marker in [
        "Exactly one attempt",
        "Retries belong to the admitted Harness attempt driver",
        "Unknown usage is never refunded",
    ] {
        assert!(
            ports.contains(marker),
            "CP-20 retry/accounting marker missing: {marker}"
        );
    }
    for marker in [
        "failure.incidents",
        "failure.reconcile",
        "failure.reconciled",
        "failure_reconciliation_evidence_required",
        "runtime_outcome_unchanged",
        "automatic_retry_allowed:false",
        "new_request_required",
        "resource_release_requires_reconciliation",
    ] {
        assert!(
            platform.contains(marker),
            "CP-20 platform marker missing: {marker}"
        );
    }
    for marker in [
        "result_unknown",
        "execution.result_committed",
        "attempt",
        "fenced",
        "effect_known",
        "commit_confirmed",
    ] {
        assert!(
            dispatch.contains(marker),
            "CP-20 dispatch marker missing: {marker}"
        );
    }
    for marker in [
        "FinalizedCapabilityAction::unknown",
        "CapabilityOutcome::Unknown",
        "result_unknown:capability_result",
        "attempt",
    ] {
        assert!(
            capabilities.contains(marker),
            "CP-20 capability marker missing: {marker}"
        );
    }
    assert!(reliability.contains("automatic_retry_allowed:false"));
    assert!(reliability.contains("failure_reconciliation_evidence_required"));
    assert!(connector_fixture.contains("connector_cannot_bypass_the_control_plane"));
    for source in [platform, dispatch, capabilities, connector_daemon] {
        for forbidden in [
            "automatic_retry_unknown",
            "retry_unknown_effect",
            "reexecute_original_action",
            "unknown_effect_is_success",
        ] {
            assert!(
                !source.contains(forbidden),
                "CP-20 bypass marker: {forbidden}"
            );
        }
    }
}
