use kiana_domain::*;
use std::collections::BTreeMap;

fn request(decision: MilestoneAcceptanceDecision) -> MilestoneAcceptanceRequest {
    let mut request = MilestoneAcceptanceRequest {
        schema: MILESTONE_ACCEPTANCE_SCHEMA.to_owned(),
        acceptance_id: "milestone-acceptance-1".to_owned(),
        project_id: "project-1".to_owned(),
        milestone_id: "milestone-1".to_owned(),
        milestone_version: 2,
        required_packet_ids: vec!["packet-1".to_owned()],
        accepted_packet_ids: vec!["packet-1".to_owned()],
        packet_acceptance_digests: BTreeMap::from([(
            "packet-1".to_owned(),
            format!("sha256:{}", "a".repeat(64)),
        )]),
        criteria_refs: vec!["criterion:milestone".to_owned()],
        evidence_refs: vec!["evidence:packet-1".to_owned()],
        decision,
        decided_at: 100,
        digest: String::new(),
    };
    request.digest = request.canonical_digest();
    request
}

#[test]
fn milestone_acceptance_cannot_use_other_milestone_evidence_or_skip_required_packets() {
    let mut missing = request(MilestoneAcceptanceDecision::Accept);
    missing.accepted_packet_ids.clear();
    missing.digest = missing.canonical_digest();
    assert_eq!(
        missing.validate().unwrap_err(),
        "milestone_acceptance_packet_missing"
    );

    let mut foreign = request(MilestoneAcceptanceDecision::Accept);
    foreign.evidence_refs = vec!["evidence:packet-foreign".to_owned()];
    foreign.digest = foreign.canonical_digest();
    assert_eq!(
        foreign.validate().unwrap_err(),
        "milestone_acceptance_foreign_evidence"
    );
}

#[test]
fn first_milestone_can_be_accepted_before_dependent_milestone_starts() {
    let mut ledger = MilestoneAcceptanceLedger::default();
    ledger
        .record(request(MilestoneAcceptanceDecision::Accept))
        .expect("record m1");
    assert!(ledger.accepted("milestone-acceptance-1"));
    // The contract names only packet-1; no M2 run or evidence is required or mutated here.
    assert_eq!(
        ledger.requests["milestone-acceptance-1"].required_packet_ids,
        vec!["packet-1"]
    );
}

#[test]
fn rejected_milestone_does_not_unlock_dependents() {
    let mut ledger = MilestoneAcceptanceLedger::default();
    ledger
        .record(request(MilestoneAcceptanceDecision::Reject))
        .expect("record reject");
    assert!(!ledger.accepted("milestone-acceptance-1"));
}
