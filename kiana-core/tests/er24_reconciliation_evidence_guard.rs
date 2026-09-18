//! ER-24 source guard for reconciliation commands and evidence.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-24 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn reconciliation_is_explicit_evidence_bound_and_idempotent() {
    let platform = include_str!("../src/platform.rs");
    let domain = include_str!("../../kiana-domain/src/platform.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");
    let connectors = include_str!("../../kiana-domain/src/connectors.rs");
    let connector_daemon = include_str!("../../kiana-daemon/src/connectors.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let baseline = include_str!("../../docs/roadmap/p2-k6-01-reliability-baseline.md");
    let cp20 = include_str!("cp20_unknown_reconcile_guard.rs");

    require(
        platform,
        &[
            "failure.reconcile",
            "observed_succeeded",
            "observed_failed",
            "no_effect",
            "reconcile_failure",
            "failure_already_reconciled",
            "reconciliation_evidence_exists",
            "reconciliation_epoch_current",
            "failure_reconciliation_evidence_required",
            "authority_revision",
            "data_epoch",
            "evidence_scope",
            "source_event_id",
            "runtime_outcome_unchanged",
            "platform_replay",
            "commit_platform",
        ],
        "ControlPlane reconciliation",
    );
    require(
        domain,
        &["FailureIncident", "RecoveryPlan", "requires_reconciliation"],
        "incident contract",
    );
    require(
        effect,
        &[
            "EffectObservationState::Unknown",
            "validate_for_scope",
            "from_provider_receipt",
        ],
        "effect observation",
    );
    require(
        connectors,
        &["reconciliation_required", "idempotency_key"],
        "connector evidence",
    );
    require(
        connector_daemon,
        &["ProviderOutcome::Unknown"],
        "connector outcome",
    );
    require(
        dispatch,
        &[
            "result_unknown",
            "effect_known",
            "fenced",
            "commit_confirmed",
        ],
        "dispatch outcome",
    );
    require(
        baseline,
        &["failure.reconcile", "不改写原始运行结果", "真实外部效果"],
        "reliability baseline",
    );
    require(
        cp20,
        &[
            "failure_reconciliation_evidence_required",
            "runtime_outcome_unchanged",
            "automatic_retry_allowed:false",
        ],
        "CP-20 regression",
    );
}

#[test]
fn reconciliation_never_becomes_retry_or_an_authority_bypass() {
    let platform = include_str!("../src/platform.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let daemon_connectors = include_str!("../../kiana-daemon/src/connectors.rs");

    for source in [platform, capabilities, daemon_connectors] {
        for forbidden in [
            "automatic_retry_unknown",
            "retry_unknown_effect",
            "reexecute_original_action",
            "runtime_outcome_changed",
            "reconcile_as_retry",
            "CapabilityBroker::new",
            "ModelClient::new",
        ] {
            assert!(
                !source.contains(forbidden),
                "ER-24 bypass marker present: {forbidden}"
            );
        }
    }
    assert!(platform.contains("failure_reconciliation_evidence_or_epoch_invalid"));
    assert!(platform.contains("failure_reconciliation_evidence_required"));
}
