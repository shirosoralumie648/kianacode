use kiana_domain::{
    PersistenceUatEvidence, PersistenceUatEvidenceStatus, PersistenceUatProofLevel,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn source_fixture_keeps_durable_limits_explicit() {
    let evidence = PersistenceUatEvidence::new(
        A,
        B,
        PersistenceUatProofLevel::Source,
        PersistenceUatEvidenceStatus::Fixture,
        Vec::new(),
        false,
        false,
        false,
        "persistence-reviewer",
        vec!["no durable backup/restore/restart/delete execution".to_owned()],
    )
    .expect("fixture evidence");
    evidence.validate().expect("fixture validates");
}

#[test]
fn verified_persistence_evidence_requires_receipts_and_reconciliation() {
    let incomplete = PersistenceUatEvidence::new(
        A,
        B,
        PersistenceUatProofLevel::Durable,
        PersistenceUatEvidenceStatus::Verified,
        Vec::new(),
        false,
        false,
        false,
        "persistence-reviewer",
        Vec::new(),
    );
    assert_eq!(
        incomplete.expect_err("verified persistence needs all evidence"),
        "persistence_uat_verified_evidence_incomplete"
    );

    let verified = PersistenceUatEvidence::new(
        A,
        B,
        PersistenceUatProofLevel::Durable,
        PersistenceUatEvidenceStatus::Verified,
        vec![A.to_owned()],
        true,
        true,
        true,
        "persistence-reviewer",
        Vec::new(),
    )
    .expect("verified evidence shape");
    verified.validate().expect("verified validates");
}

#[test]
fn local_behavior_cannot_verify_persistence_uat() {
    let local = PersistenceUatEvidence::new(
        A,
        B,
        PersistenceUatProofLevel::LocalBehavior,
        PersistenceUatEvidenceStatus::Verified,
        vec![A.to_owned()],
        true,
        true,
        true,
        "persistence-reviewer",
        Vec::new(),
    );
    assert_eq!(
        local.expect_err("persistence verification needs durable proof"),
        "persistence_uat_verified_evidence_incomplete"
    );
}
