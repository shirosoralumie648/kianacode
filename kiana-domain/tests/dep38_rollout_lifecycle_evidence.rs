use kiana_domain::{
    OrchestratedBackend, OrchestratedRolloutAction, RolloutLifecycleEvidence,
    RolloutLifecycleEvidenceStatus, RolloutLifecyclePhase, RolloutLifecycleProofLevel,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn evidence(
    backend: OrchestratedBackend,
    status: RolloutLifecycleEvidenceStatus,
    phase: RolloutLifecyclePhase,
    deletion_eligible: bool,
    result_unknown: bool,
    limitations: Vec<String>,
) -> Result<RolloutLifecycleEvidence, String> {
    RolloutLifecycleEvidence::new(
        backend,
        phase,
        OrchestratedRolloutAction::Promote,
        digest('a'),
        digest('b'),
        Some(digest('c')),
        Some(digest('d')),
        digest('e'),
        digest('f'),
        true,
        deletion_eligible,
        0,
        0,
        RolloutLifecycleProofLevel::LocalBehavior,
        status,
        (status == RolloutLifecycleEvidenceStatus::Verified)
            .then(|| "approval:rollout-lifecycle".to_owned()),
        result_unknown,
        limitations,
    )
}

#[test]
fn simulated_lifecycle_keeps_health_and_retirement_evidence_explicit() {
    let value = evidence(
        OrchestratedBackend::Simulation,
        RolloutLifecycleEvidenceStatus::Simulated,
        RolloutLifecyclePhase::Promoted,
        false,
        false,
        vec!["simulation did not mutate workers or delete old roots".to_owned()],
    )
    .expect("simulated lifecycle evidence");
    value.validate().expect("simulated lifecycle validates");
}

#[test]
fn target_unknown_and_invalid_deletion_cannot_verify() {
    let target = evidence(
        OrchestratedBackend::KubernetesTarget,
        RolloutLifecycleEvidenceStatus::Verified,
        RolloutLifecyclePhase::Promoted,
        false,
        false,
        Vec::new(),
    );
    assert_eq!(
        target.expect_err("target lifecycle cannot verify"),
        "rollout_lifecycle_target_cannot_verify"
    );

    let unknown = evidence(
        OrchestratedBackend::Simulation,
        RolloutLifecycleEvidenceStatus::Verified,
        RolloutLifecyclePhase::Promoted,
        false,
        true,
        Vec::new(),
    );
    assert_eq!(
        unknown.expect_err("unknown lifecycle needs reconciliation"),
        "rollout_lifecycle_unknown_cannot_verify"
    );

    let simulation = evidence(
        OrchestratedBackend::Simulation,
        RolloutLifecycleEvidenceStatus::Verified,
        RolloutLifecyclePhase::Promoted,
        false,
        false,
        Vec::new(),
    );
    assert_eq!(
        simulation.expect_err("simulation lifecycle cannot verify live"),
        "rollout_lifecycle_simulation_cannot_verify"
    );

    let invalid_delete = evidence(
        OrchestratedBackend::Simulation,
        RolloutLifecycleEvidenceStatus::Simulated,
        RolloutLifecyclePhase::Promoted,
        true,
        false,
        Vec::new(),
    );
    assert_eq!(
        invalid_delete.expect_err("promoted root cannot be deletion eligible"),
        "rollout_lifecycle_deletion_gate_invalid"
    );
}

#[test]
fn verified_lifecycle_requires_typed_operator_approval() {
    let mut value = evidence(
        OrchestratedBackend::Simulation,
        RolloutLifecycleEvidenceStatus::Simulated,
        RolloutLifecyclePhase::Promoted,
        false,
        false,
        vec!["simulation remains source-bound".to_owned()],
    )
    .expect("simulation evidence shape");
    value.status = RolloutLifecycleEvidenceStatus::Verified;
    value.operator_approval_ref = Some("operator-approval".to_owned());
    value.evidence_digest = value.digest();
    assert_eq!(
        value
            .validate()
            .expect_err("verified approval must be typed"),
        "rollout_lifecycle_operator_approval_ref_invalid"
    );
}
