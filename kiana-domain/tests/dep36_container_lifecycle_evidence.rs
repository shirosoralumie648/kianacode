use kiana_domain::{
    ContainerLifecycleEvidence, ContainerLifecycleMode, ContainerLifecyclePhase,
    ContainerLifecycleProofLevel, ContainerLifecycleStatus, ContainerPhaseEvidence,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn phases(with_receipts: bool) -> Vec<ContainerPhaseEvidence> {
    [
        ContainerLifecyclePhase::Startup,
        ContainerLifecyclePhase::Readiness,
        ContainerLifecyclePhase::Liveness,
        ContainerLifecyclePhase::Quiesce,
        ContainerLifecyclePhase::Stop,
        ContainerLifecyclePhase::Dispose,
    ]
    .into_iter()
    .enumerate()
    .map(|(index, phase)| ContainerPhaseEvidence {
        phase,
        observation_digest: digest((b'a' + index as u8) as char),
        receipt_digest: with_receipts.then(|| digest((b'0' + index as u8) as char)),
        result_unknown: false,
    })
    .collect()
}

fn evidence(
    mode: ContainerLifecycleMode,
    proof_level: ContainerLifecycleProofLevel,
    status: ContainerLifecycleStatus,
    result_unknown: bool,
    with_receipts: bool,
    limitations: Vec<String>,
) -> Result<ContainerLifecycleEvidence, String> {
    ContainerLifecycleEvidence::new(
        mode,
        proof_level,
        status,
        digest('a'),
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        digest('f'),
        Some(digest('0')),
        phases(with_receipts),
        "SIGTERM",
        true,
        Some(digest('1')),
        Some(digest('2')),
        Some(digest('3')),
        Some(digest('4')),
        result_unknown,
        limitations,
    )
}

#[test]
fn fake_harness_keeps_container_evidence_at_local_behavior() {
    let evidence = evidence(
        ContainerLifecycleMode::FakeHarness,
        ContainerLifecycleProofLevel::LocalBehavior,
        ContainerLifecycleStatus::Verified,
        false,
        true,
        Vec::new(),
    )
    .expect("complete fake lifecycle evidence");
    evidence.validate().expect("fake lifecycle validates");
}

#[test]
fn incomplete_or_unknown_container_lifecycle_cannot_verify() {
    let incomplete = evidence(
        ContainerLifecycleMode::FakeHarness,
        ContainerLifecycleProofLevel::LocalBehavior,
        ContainerLifecycleStatus::Verified,
        false,
        false,
        Vec::new(),
    );
    assert_eq!(
        incomplete.expect_err("verified lifecycle needs receipts"),
        "container_lifecycle_verified_evidence_incomplete"
    );

    let unknown = evidence(
        ContainerLifecycleMode::FakeHarness,
        ContainerLifecycleProofLevel::LocalBehavior,
        ContainerLifecycleStatus::Verified,
        true,
        true,
        Vec::new(),
    );
    assert_eq!(
        unknown.expect_err("unknown lifecycle needs reconciliation"),
        "container_lifecycle_unknown_cannot_verify"
    );
}

#[test]
fn target_only_and_host_fallback_are_not_success_paths() {
    let target = evidence(
        ContainerLifecycleMode::TargetOnly,
        ContainerLifecycleProofLevel::Source,
        ContainerLifecycleStatus::Verified,
        false,
        true,
        Vec::new(),
    );
    assert_eq!(
        target.expect_err("target-only cannot verify"),
        "container_lifecycle_target_cannot_verify"
    );

    let fallback = ContainerLifecycleEvidence::new(
        ContainerLifecycleMode::FakeHarness,
        ContainerLifecycleProofLevel::Source,
        ContainerLifecycleStatus::Partial,
        digest('a'),
        digest('b'),
        digest('c'),
        digest('d'),
        digest('e'),
        digest('f'),
        None,
        vec![ContainerPhaseEvidence {
            phase: ContainerLifecyclePhase::Startup,
            observation_digest: digest('a'),
            receipt_digest: None,
            result_unknown: false,
        }],
        "SIGTERM",
        false,
        None,
        None,
        None,
        None,
        false,
        vec!["host fallback is forbidden".to_owned()],
    );
    assert_eq!(
        fallback.expect_err("host fallback must be rejected"),
        "container_lifecycle_host_fallback_forbidden"
    );
}
