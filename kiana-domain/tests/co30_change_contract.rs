use kiana_domain::*;

fn impact() -> ChangeImpact {
    let mut impact = ChangeImpact {
        schema: CHANGE_IMPACT_SCHEMA.to_owned(),
        change_id: "change-1".to_owned(),
        project_id: "project-1".to_owned(),
        old_baseline_version: 1,
        new_baseline_version: 2,
        affected_milestones: vec!["milestone-1".to_owned()],
        affected_packets: vec!["packet-1".to_owned()],
        affected_runs: vec!["run-1".to_owned()],
        invalidated_reviews: vec!["review-1".to_owned()],
        invalidated_acceptances: vec!["acceptance-1".to_owned()],
        invalidated_deliveries: vec!["delivery-1".to_owned()],
        affected_budget_ref: "budget:new".to_owned(),
        affected_schedule_ref: "schedule:new".to_owned(),
        impact_digest: String::new(),
    };
    impact.impact_digest = impact.canonical_digest();
    impact
}

fn publication(impact: &ChangeImpact) -> BaselinePublication {
    let mut publication = BaselinePublication {
        schema: BASELINE_PUBLICATION_SCHEMA.to_owned(),
        project_id: impact.project_id.clone(),
        change_id: impact.change_id.clone(),
        old_baseline_version: impact.old_baseline_version,
        new_baseline_version: impact.new_baseline_version,
        charter_ref: "artifact:charter".to_owned(),
        plan_ref: "plan:2".to_owned(),
        packet_refs: impact.affected_packets.clone(),
        criteria_digest: format!("sha256:{}", "d".repeat(64)),
        budget_ref: impact.affected_budget_ref.clone(),
        schedule_ref: impact.affected_schedule_ref.clone(),
        invalidated_refs: vec![
            "review-1".to_owned(),
            "acceptance-1".to_owned(),
            "delivery-1".to_owned(),
        ],
        published_by: "sponsor-1".to_owned(),
        published_at: 100,
        digest: String::new(),
    };
    publication.digest = publication.canonical_digest();
    publication
}

#[test]
fn approved_change_cannot_leave_old_packets_or_approvals_authoritative() {
    let impact = impact();
    let publication = publication(&impact);
    assert!(impact.validate().is_ok());
    assert!(publication.validate(&impact).is_ok());
    assert!(publication
        .invalidated_refs
        .contains(&"review-1".to_owned()));
    assert_eq!(publication.packet_refs, vec!["packet-1"]);
}

#[test]
fn baseline_change_updates_the_affected_graph_and_preserves_historical_decisions() {
    let mut ledger = ChangePublicationLedger::default();
    let impact = impact();
    ledger
        .publish(impact.clone(), publication(&impact))
        .expect("publish");
    ledger
        .publish(impact.clone(), publication(&impact))
        .expect("idempotent");
    assert_eq!(ledger.historical_impacts.len(), 1);
    assert_eq!(ledger.publications.len(), 1);
    let mut next = impact;
    next.change_id = "change-2".to_owned();
    next.new_baseline_version = 3;
    next.impact_digest = next.canonical_digest();
    ledger
        .publish(next.clone(), publication(&next))
        .expect("next baseline");
    assert_eq!(ledger.historical_impacts.len(), 2);
}

#[test]
fn partial_publication_and_stale_baseline_are_rejected() {
    let impact = impact();
    let mut incomplete = publication(&impact);
    incomplete.packet_refs.clear();
    incomplete.digest = incomplete.canonical_digest();
    assert_eq!(
        incomplete.validate(&impact).unwrap_err(),
        "baseline_publication_complete_set_required"
    );
    let mut stale = impact;
    stale.new_baseline_version = 1;
    stale.impact_digest = stale.canonical_digest();
    assert_eq!(
        stale.validate().unwrap_err(),
        "change_impact_version_invalid"
    );
}
