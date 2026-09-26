use kiana_domain::*;

fn provenance(predecessor: &str, successor: &str, state: ReworkAttemptState) -> ReworkProvenance {
    let mut value = ReworkProvenance {
        schema: REWORK_PROVENANCE_SCHEMA.to_owned(),
        predecessor_packet_id: predecessor.to_owned(),
        successor_packet_id: successor.to_owned(),
        rejection_ref: "review:rejected-1".to_owned(),
        rejection_digest: format!("sha256:{}", "a".repeat(64)),
        baseline_digest: format!("sha256:{}", "b".repeat(64)),
        successor_packet_digest: format!("sha256:{}", "c".repeat(64)),
        reason: "review requested changes".to_owned(),
        max_attempts: 3,
        attempts_used: 1,
        remaining_budget: 100,
        predecessor_state: state,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn rework_cannot_change_acceptance_reset_budget_or_revive_a_terminal_attempt() {
    let mut ledger = ReworkLedger::default();
    let first = provenance("packet-1", "packet-2", ReworkAttemptState::Rejected);
    ledger.record(first.clone()).expect("record");
    assert_eq!(ledger.successor("packet-1"), Some("packet-2"));
    assert_eq!(
        ledger
            .record(provenance(
                "packet-2",
                "packet-3",
                ReworkAttemptState::Succeeded
            ))
            .unwrap_err(),
        "rework_terminal_attempt_cannot_revive"
    );
    let mut changed = first.clone();
    changed.baseline_digest = format!("sha256:{}", "d".repeat(64));
    changed.digest = changed.canonical_digest();
    assert_eq!(
        ledger.record(changed).unwrap_err(),
        "rework_duplicate_provenance"
    );
}

#[test]
fn rejected_packet_can_pass_a_new_review_while_preserving_prior_failures() {
    let mut ledger = ReworkLedger::default();
    ledger
        .record(provenance(
            "packet-1",
            "packet-2",
            ReworkAttemptState::Rejected,
        ))
        .expect("first");
    ledger
        .record(provenance(
            "packet-2",
            "packet-3",
            ReworkAttemptState::Failed,
        ))
        .expect("second");
    assert_eq!(ledger.successor("packet-1"), Some("packet-2"));
    assert_eq!(ledger.successor("packet-2"), Some("packet-3"));
    assert!(ledger.successors.contains_key("packet-1"));
}

#[test]
fn unknown_attempt_and_successor_cycle_are_rejected() {
    let mut ledger = ReworkLedger::default();
    assert_eq!(
        ledger
            .record(provenance(
                "packet-1",
                "packet-2",
                ReworkAttemptState::Unknown
            ))
            .unwrap_err(),
        "rework_attempt_not_reclaimable"
    );
    ledger
        .record(provenance(
            "packet-1",
            "packet-2",
            ReworkAttemptState::Rejected,
        ))
        .expect("first");
    ledger
        .record(provenance(
            "packet-2",
            "packet-3",
            ReworkAttemptState::Rejected,
        ))
        .expect("second");
    assert_eq!(
        ledger
            .record(provenance(
                "packet-3",
                "packet-1",
                ReworkAttemptState::Rejected
            ))
            .unwrap_err(),
        "rework_successor_cycle"
    );
}
