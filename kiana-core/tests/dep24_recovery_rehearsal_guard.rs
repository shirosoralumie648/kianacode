//! DEP-24 source guard: the rehearsal decision is a measurement contract bound to a real lease,
//! and neither the domain contract nor the core facade crosses an effect boundary.

const CORE: &str = include_str!("../src/recovery_rehearsal.rs");
const DOMAIN: &str = include_str!("../../kiana-domain/src/recovery_objective.rs");
const BASELINE: &str = include_str!("../../docs/roadmap/dep24-recovery-rehearsal-baseline.md");

#[test]
fn dep24_the_domain_decides_rpo_rpo_cursor_fence_and_evidence() {
    for marker in [
        "RecoveryObjective",
        "RecoveryMeasurement",
        "RecoveryRehearsalReport",
        "RecoveryRehearsalLedger",
        "RecoveryRehearsalEvidence",
        "RecoveryFaultScenario",
        "RecoveryObservation",
        "RecoveryObjectiveStatus",
        "fn evaluate_rpo(",
        "fn evaluate_rto(",
        "fn evaluate_cursor_monotonicity(",
        "fn evaluate_fence_state(",
        "fn evaluate_evidence_completeness(",
        "fn evaluate_durability(",
        // The four the card names, in the order the card names them.
        "recovery_rpo_objective_exceeded",
        "recovery_rto_objective_exceeded",
        "recovery_cursor_ahead_of_source",
        "recovery_cursor_regressed",
        "recovery_stale_lease_still_effective",
        "recovery_stale_fence_token_still_effective",
        "recovery_stale_dispatch_after_restore",
        "recovery_restart_not_durable",
        "recovery_rehearsal_outcome_not_succeeded",
        "recovery_evidence_exit_code_missing",
        "recovery_evidence_limitations_missing",
        "recovery_measurement_objective_binding_invalid",
    ] {
        assert!(
            DOMAIN.contains(marker),
            "DEP-24 domain marker missing: {marker}"
        );
    }
}

#[test]
fn dep24_the_decision_is_re_derived_rather_than_trusted() {
    // A fixed-order reducer, so the same facts always produce the same reason.
    assert!(
        DOMAIN.contains("fn derive("),
        "DEP-24 must decide in one fixed-order function"
    );
    assert!(
        DOMAIN.contains("report.validate_against(objective, measurement)?"),
        "DEP-24 must re-derive the report on validation rather than trust its fields"
    );
    // One gate, and a missed objective never reads as met.
    assert!(
        DOMAIN.contains("fn within_objective(&self) -> bool"),
        "DEP-24 must expose a single within_objective gate"
    );
    assert!(
        DOMAIN.contains("recovery_rehearsal_report_reason_incoherent"),
        "DEP-24 must refuse a report whose status and reason disagree"
    );
    // A periodic rehearsal may not be rewritten under one identity.
    assert!(
        DOMAIN.contains("recovery_rehearsal_identity_digest_conflict"),
        "DEP-24 must treat a contradicting rehearsal under one identity as a conflict"
    );
    // A placeholder in the one field reserved for limitations is not a statement.
    assert!(
        DOMAIN.contains("fn unstated(value: &str) -> bool"),
        "DEP-24 must reject a placeholder limitations field"
    );
}

#[test]
fn dep24_the_core_facade_binds_the_decision_to_a_real_lease() {
    for marker in [
        "RecoveryLeaseObservation",
        "RecoveryRehearsalOutcome",
        "OperationLease",
        "OperationLeaseState",
        "fn evaluate_post_recovery_lease(",
        "fn evaluate_recovery_rehearsal(",
        "fn may_publish(&self) -> bool",
        "RECOVERY_REHEARSAL_OUTCOME_SCHEMA",
        "recovery_rehearsal_lease_root_mismatch",
        "recovery_rehearsal_restored_root_not_ready",
        "recovery_rehearsal_lease_not_active",
        "recovery_rehearsal_fence_token_reused",
        "recovery_rehearsal_authority_epoch_not_advanced",
        "recovery_rehearsal_data_epoch_not_advanced",
        "recovery_rehearsal_outcome_binding_invalid",
        "recovery_rehearsal_outcome_publish_flag_invalid",
        "recovery_rehearsal_outcome_digest_mismatch",
    ] {
        assert!(
            CORE.contains(marker),
            "DEP-24 core marker missing: {marker}"
        );
    }
    // The lease is a value somebody supplied, not one this slice mints or persists: no CAS method
    // is called, only the lease's own validation and fields are read.
    assert!(
        !CORE.contains("OperationLeaseCas::"),
        "DEP-24 must not mint a lease; that is OperationLeaseCas's job and a later step's"
    );
    // The old token is burned, not reissued: the facade compares the real lease against the
    // measurement's superseded token rather than trusting a boolean.
    assert!(
        CORE.contains("if observation.lease.fence_token == measurement.superseded_fence_token"),
        "DEP-24 must compare the serving fence token against the pre-fault one"
    );
    assert!(
        CORE.contains("if observation.lease.state != OperationLeaseState::Active"),
        "DEP-24 must refuse a post-recovery lease that is not the active one"
    );
    // The publish flag is derived from the decision, and re-checked on every validation.
    assert!(
        CORE.contains("self.publishable != report.within_objective()"),
        "DEP-24 must re-check the publish flag against the re-derived decision"
    );
    assert!(
        CORE.contains("outcome.validate_against(objective, measurement, observation)?"),
        "DEP-24 must re-derive the sealed outcome from all three inputs"
    );
}

#[test]
fn dep24_neither_module_crosses_an_effect_boundary() {
    // The contract decides over supplied values. It does not crash, restore, back up, time
    // anything, read the wall clock, append a fact or dispatch a capability.
    for forbidden in [
        "std::fs",
        "std::process",
        "std::net",
        "std::time",
        "std::thread",
        "tokio::",
        "PathBuf",
        "Command::new",
        "SystemTime",
        "Instant",
        "remove_dir",
        "remove_file",
        "rename(",
        "EventStore",
        "EventStorePort",
        "ArtifactStorePort",
        "CapabilityBroker",
        "kiana_ports",
        "handle_command",
    ] {
        assert!(
            !DOMAIN.contains(forbidden),
            "DEP-24 domain contract crossed effect boundary: {forbidden}"
        );
        assert!(
            !CORE.contains(forbidden),
            "DEP-24 core facade crossed effect boundary: {forbidden}"
        );
    }
}

#[test]
fn dep24_the_baseline_states_what_was_not_measured() {
    for marker in [
        "RPO",
        "RTO",
        "fence",
        "rehearsal",
        "SC-42",
        "DEP-25",
        "ControlPlane::handle_command",
        "proof_level",
        "`source`",
        "does not",
    ] {
        assert!(
            BASELINE.contains(marker),
            "DEP-24 baseline marker missing: {marker}"
        );
    }
    // The one sentence that has to be there verbatim: nothing was crashed, restored or backed up,
    // so this slice produced no RPO and no RTO number at all.
    assert!(
        BASELINE.contains(
            "There was no process kill, no restore, no backup and therefore no measured RPO or RTO number in this slice."
        ),
        "DEP-24 baseline must state plainly that no RPO or RTO number was measured"
    );
    // And the phrasings a promotion would need, which this slice must not contain anywhere.
    for forbidden in [
        "verified end to end",
        "runs in production",
        "production rehearsal",
        "proves the objective",
    ] {
        assert!(
            !BASELINE.contains(forbidden),
            "DEP-24 baseline over-claims: {forbidden}"
        );
    }
}
