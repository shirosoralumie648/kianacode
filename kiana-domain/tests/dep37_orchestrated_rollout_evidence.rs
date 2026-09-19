use kiana_domain::{
    OrchestratedBackend, OrchestratedRolloutAction, OrchestratedRolloutEvidence,
    OrchestratedRolloutEvidenceStatus, OrchestratedRolloutPhase, OrchestratedRolloutProofLevel,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn evidence(
    backend: OrchestratedBackend,
    proof_level: OrchestratedRolloutProofLevel,
    status: OrchestratedRolloutEvidenceStatus,
    result_unknown: bool,
    receipts: bool,
    limitations: Vec<String>,
) -> Result<OrchestratedRolloutEvidence, String> {
    OrchestratedRolloutEvidence::new(
        "rollout-1",
        backend,
        OrchestratedRolloutPhase::Promoted,
        OrchestratedRolloutAction::Promote,
        digest('a'),
        digest('b'),
        digest('c'),
        digest('d'),
        "revision-target",
        digest('e'),
        10_000,
        2,
        proof_level,
        status,
        (status == OrchestratedRolloutEvidenceStatus::Verified)
            .then(|| "approval:rollout".to_owned()),
        receipts.then(|| digest('f')),
        receipts.then(|| digest('0')),
        result_unknown,
        limitations,
    )
}

#[test]
fn simulation_records_source_bound_rollout_evidence() {
    let evidence = evidence(
        OrchestratedBackend::Simulation,
        OrchestratedRolloutProofLevel::LocalBehavior,
        OrchestratedRolloutEvidenceStatus::Simulated,
        false,
        false,
        vec!["simulation does not change traffic or fence workers".to_owned()],
    )
    .expect("simulation evidence");
    evidence.validate().expect("simulation evidence validates");
}

#[test]
fn target_backend_and_unknown_result_cannot_verify() {
    let target = evidence(
        OrchestratedBackend::KubernetesTarget,
        OrchestratedRolloutProofLevel::Source,
        OrchestratedRolloutEvidenceStatus::Verified,
        false,
        true,
        Vec::new(),
    );
    assert_eq!(
        target.expect_err("target backend cannot verify"),
        "orchestrated_target_backend_cannot_verify"
    );

    let unknown = evidence(
        OrchestratedBackend::Simulation,
        OrchestratedRolloutProofLevel::Live,
        OrchestratedRolloutEvidenceStatus::Verified,
        true,
        true,
        Vec::new(),
    );
    assert_eq!(
        unknown.expect_err("unknown rollout needs reconciliation"),
        "orchestrated_unknown_cannot_verify"
    );
}

#[test]
fn verified_rollout_requires_approval_health_and_drain_receipts() {
    let value = evidence(
        OrchestratedBackend::Simulation,
        OrchestratedRolloutProofLevel::Live,
        OrchestratedRolloutEvidenceStatus::Verified,
        false,
        true,
        Vec::new(),
    )
    .expect("verified evidence shape");
    value.validate().expect("verified evidence validates");
}
