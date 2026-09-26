use kiana_domain::*;

fn sha(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn card() -> CompanyInboxCard {
    let mut value = CompanyInboxCard {
        schema: COMPANY_INBOX_CARD_SCHEMA.to_owned(),
        card_id: "card-1".to_owned(),
        task_id: "task-1".to_owned(),
        kind: CompanyInboxCardKind::Acceptance,
        project_id: "project-1".to_owned(),
        target_kind: "acceptance".to_owned(),
        target_id: "acceptance-1".to_owned(),
        target_revision: 3,
        target_digest: sha('t'),
        scope_digest: sha('s'),
        decider_ref: "principal:sponsor-1".to_owned(),
        options: vec!["accept".to_owned(), "reject".to_owned()],
        evidence_refs: vec!["event:acceptance-requested".to_owned()],
        created_at: 10,
        due_at: 20,
        expires_at: 100,
        status: CompanyInboxCardStatus::Pending,
        decision_ref: None,
        decided_by: None,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn human_inbox_rejects_stale_target_wrong_decider_and_double_consumption() {
    let pending = card();
    assert_eq!(
        pending
            .consume(
                "principal:other",
                "accept",
                "decision:1",
                30,
                3,
                &pending.target_digest,
                &pending.scope_digest,
            )
            .unwrap_err(),
        "company_inbox_wrong_decider"
    );
    assert_eq!(
        pending
            .consume(
                "principal:sponsor-1",
                "accept",
                "decision:1",
                30,
                4,
                &pending.target_digest,
                &pending.scope_digest,
            )
            .unwrap_err(),
        "company_inbox_stale_target"
    );
    let decided = pending
        .consume(
            "principal:sponsor-1",
            "accept",
            "decision:1",
            30,
            3,
            &pending.target_digest,
            &pending.scope_digest,
        )
        .expect("decision");
    assert_eq!(
        decided
            .consume(
                "principal:sponsor-1",
                "accept",
                "decision:2",
                31,
                3,
                &pending.target_digest,
                &pending.scope_digest,
            )
            .unwrap_err(),
        "company_inbox_double_consumption"
    );
}

#[test]
fn one_human_decision_is_visible_and_consumed_consistently_across_clients() {
    let mut ledger = CompanyInboxLedger::default();
    ledger.publish(card()).expect("publish");
    ledger.publish(card()).expect("idempotent publish");
    assert_eq!(ledger.pending_for("principal:sponsor-1", 30).len(), 1);
    let target_digest = ledger.cards["card-1"].target_digest.clone();
    let scope_digest = ledger.cards["card-1"].scope_digest.clone();
    ledger
        .consume(
            "card-1",
            "principal:sponsor-1",
            "reject",
            "decision:2",
            30,
            3,
            &target_digest,
            &scope_digest,
        )
        .expect("consume");
    assert!(ledger.pending_for("principal:sponsor-1", 30).is_empty());
    assert_eq!(
        ledger.cards["card-1"].status,
        CompanyInboxCardStatus::Decided
    );
}

#[test]
fn expired_card_cannot_be_approved_by_elapsed_time_or_preselected_option() {
    let mut expired = card();
    expired.expires_at = 25;
    expired.digest = expired.canonical_digest();
    assert_eq!(
        expired
            .consume(
                "principal:sponsor-1",
                "accept",
                "decision:expired",
                25,
                3,
                &expired.target_digest,
                &expired.scope_digest,
            )
            .unwrap_err(),
        "company_inbox_target_expired"
    );
}
