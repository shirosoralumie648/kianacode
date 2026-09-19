use kiana_domain::{
    PersistenceCapacityEvidence, PersistenceCapacityEvidenceStatus, PersistenceCapacityProofLevel,
    PersistenceCapacityStatus,
};

const A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn capacity_fixture_retains_source_limitations() {
    let value = PersistenceCapacityEvidence::new(
        A,
        B,
        PersistenceCapacityStatus::Ready,
        PersistenceCapacityProofLevel::Source,
        PersistenceCapacityEvidenceStatus::Fixture,
        None,
        None,
        false,
        true,
        false,
        "capacity-reviewer",
        vec!["CI fixture timings are not production capacity receipts".to_owned()],
    )
    .expect("fixture evidence");
    value.validate().expect("fixture validates");
}

#[test]
fn verified_capacity_requires_benchmark_and_resource_receipts() {
    let incomplete = PersistenceCapacityEvidence::new(
        A,
        B,
        PersistenceCapacityStatus::Ready,
        PersistenceCapacityProofLevel::Durable,
        PersistenceCapacityEvidenceStatus::Verified,
        None,
        None,
        true,
        true,
        true,
        "capacity-reviewer",
        Vec::new(),
    );
    assert_eq!(
        incomplete.expect_err("verified capacity needs receipts"),
        "persistence_capacity_verified_evidence_incomplete"
    );
}
