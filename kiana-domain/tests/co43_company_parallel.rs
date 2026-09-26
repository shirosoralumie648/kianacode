use kiana_domain::*;
use std::collections::BTreeMap;

fn plan() -> CompanyParallelPlan {
    let mut value = CompanyParallelPlan {
        schema: COMPANY_PARALLEL_SCHEMA.to_owned(),
        parallel_id: "parallel-1".to_owned(),
        project_id: "project-1".to_owned(),
        parent_packet_id: "packet-parent".to_owned(),
        merge_owner: "integrator-1".to_owned(),
        partition_ids: vec!["partition-a".to_owned(), "partition-b".to_owned()],
        input_version: 3,
        authority_epoch: 7,
        isolation_digest: format!("sha256:{}", "i".repeat(64)),
        output_contract: "receipt-only".to_owned(),
        max_concurrency: 2,
        expires_at: 100,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn swarm_rejects_overlapping_writers_duplicate_fingerprints_and_excess_delegation() {
    let mut invalid = plan();
    invalid.partition_ids.push("partition-a".to_owned());
    invalid.digest = invalid.canonical_digest();
    assert_eq!(
        invalid.validate().unwrap_err(),
        "company_parallel_partition_identity_invalid"
    );
    let mut unknown_plan = plan();
    unknown_plan.authority_epoch = 0;
    unknown_plan.digest = unknown_plan.canonical_digest();
    assert_eq!(
        unknown_plan.validate().unwrap_err(),
        "company_parallel_header_invalid"
    );
}

#[test]
fn bounded_swarm_completes_independent_packets_with_a_full_responsibility_chain() {
    let plan = plan();
    let outcomes = BTreeMap::from([
        ("partition-a".to_owned(), CompanyPartitionOutcome::Succeeded),
        ("partition-b".to_owned(), CompanyPartitionOutcome::Succeeded),
    ]);
    let settlement = CompanyParallelSettlement::from_outcomes(
        &plan,
        outcomes,
        vec!["evidence:child-a".to_owned(), "evidence:child-b".to_owned()],
    )
    .expect("settlement");
    assert!(settlement.all_settled);
    assert!(settlement.merge_allowed);
    settlement.validate_against(&plan).expect("binding");
}

#[test]
fn result_unknown_or_partial_children_never_become_mergeable() {
    let plan = plan();
    let partial = BTreeMap::from([
        ("partition-a".to_owned(), CompanyPartitionOutcome::Succeeded),
        ("partition-b".to_owned(), CompanyPartitionOutcome::Pending),
    ]);
    let settlement = CompanyParallelSettlement::from_outcomes(
        &plan,
        partial,
        vec!["evidence:partial".to_owned()],
    )
    .expect("partial");
    assert!(!settlement.all_settled);
    assert!(!settlement.merge_allowed);
    let unknown = BTreeMap::from([
        ("partition-a".to_owned(), CompanyPartitionOutcome::Succeeded),
        (
            "partition-b".to_owned(),
            CompanyPartitionOutcome::ResultUnknown,
        ),
    ]);
    let settlement = CompanyParallelSettlement::from_outcomes(
        &plan,
        unknown,
        vec!["evidence:unknown".to_owned()],
    )
    .expect("unknown");
    assert!(settlement.all_settled);
    assert!(!settlement.merge_allowed);
}
