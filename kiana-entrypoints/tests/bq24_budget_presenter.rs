//! BQ-24 four-entrypoint parity fixtures for the read-only budget card.
//!
//! The card's success criterion is "同一 Run 在四入口显示一致". These fixtures prove the shared
//! presenter makes a divergent surface *refusable*; they do not prove a real daemon serves the
//! same numbers to four real surfaces.
//!
//! Proof ceiling: `source`.

use kiana_commands::billing_card::{
    BudgetCard, BudgetCardParity, BudgetCorrectionCard, BudgetSurface, BUDGET_CARD_PARITY_SCHEMA,
};
use kiana_commands::budget::{render_budget_card, ServerBudgetFacts};
use kiana_domain::{
    BillingQueryKind, BillingQueryResponse, BillingRollupTotals, CostCorrectionId, Freshness,
    LedgerEntryId, Money, ProjectId,
};
use kiana_entrypoints::budget_presenter::{
    budget_card_from_value, budget_card_matches_response, budget_surface_frame,
    budget_surface_parity, reject_identity_override, render_amount, render_budget_json,
    render_budget_text, render_correction_text, BUDGET_PRESENTER_SCHEMA,
};

const BOUNDARY: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECTION: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TARGET_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn known_response(freshness: Freshness) -> BillingQueryResponse {
    let ledger = BillingRollupTotals {
        measured: Some(Money::new("USD", 400).unwrap()),
        ..BillingRollupTotals::default()
    };
    BillingQueryResponse::new(
        BillingQueryKind::BudgetSummary,
        ProjectId::new(),
        BOUNDARY,
        10,
        PROJECTION,
        freshness,
        0,
        ledger,
        1,
        0,
        None,
        None,
    )
    .unwrap()
}

fn card_for(response: &BillingQueryResponse) -> BudgetCard {
    render_budget_card(
        response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(kiana_commands::billing_card::BudgetAmount::new("USD", 1_000).unwrap()),
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap()
}

#[test]
fn the_presenter_refuses_a_caller_supplied_actor_or_project() {
    assert!(reject_identity_override(None, None).is_ok());
    assert_eq!(
        reject_identity_override(Some("someone-else"), None).unwrap_err(),
        "budget_card_identity_override_rejected"
    );
    assert_eq!(
        reject_identity_override(None, Some(ProjectId::new())).unwrap_err(),
        "budget_card_identity_override_rejected"
    );
}

#[test]
fn a_card_bound_to_another_project_or_response_is_refused() {
    let first = known_response(Freshness::Current);
    let card = card_for(&first);
    let second = known_response(Freshness::Stale);
    assert_eq!(
        budget_card_matches_response(&card, &second).unwrap_err(),
        "budget_card_response_binding_invalid"
    );
    assert!(budget_card_matches_response(&card, &first).is_ok());
}

#[test]
fn all_four_entrypoints_render_the_same_card_and_parity_refuses_a_divergent_one() {
    let response = known_response(Freshness::Current);
    let card = card_for(&response);

    let parity = budget_surface_parity(&card).unwrap();
    assert_eq!(parity.frames.len(), 4);
    let mut surfaces: Vec<BudgetSurface> =
        parity.frames.iter().map(|frame| frame.surface).collect();
    surfaces.sort();
    surfaces.dedup();
    assert_eq!(surfaces.len(), 4, "a surface was counted twice");
    parity.validate_against(&card).unwrap();

    let honest = card.render_text();
    for frame in &parity.frames {
        assert_eq!(frame.rendered_text, honest);
        assert_eq!(frame.state, card.state);
        assert_eq!(frame.overage_reason, card.overage_reason);
    }
    assert!(honest.contains("remaining: unknown"), "{honest}");

    // A surface that prints its own friendlier remainder is refused at validation.
    let mut frames = parity.frames.clone();
    frames[0].rendered_text = format!("{honest}\nremaining: 999999");
    let mut forged = BudgetCardParity {
        schema: BUDGET_CARD_PARITY_SCHEMA.to_owned(),
        card_digest: parity.card_digest.clone(),
        response_digest: parity.response_digest.clone(),
        frames,
        parity_digest: String::new(),
    };
    forged.parity_digest = forged.digest();
    assert_eq!(
        forged.validate_against(&card).unwrap_err(),
        "budget_card_parity_render_mismatch"
    );
}

#[test]
fn a_frame_bound_to_another_card_is_refused() {
    let card = card_for(&known_response(Freshness::Current));
    let other = card_for(&known_response(Freshness::Stale));
    let mut frame = budget_surface_frame(&card, BudgetSurface::Cli).unwrap();
    frame.card_digest = other.card_digest.clone();
    frame.frame_digest = frame.digest();
    let mut forged = BudgetCardParity {
        schema: BUDGET_CARD_PARITY_SCHEMA.to_owned(),
        card_digest: card.card_digest.clone(),
        response_digest: card.response_digest.clone(),
        frames: vec![frame],
        parity_digest: String::new(),
    };
    forged.parity_digest = forged.digest();
    assert!(forged.validate_against(&card).is_err());
}

#[test]
fn a_decoded_card_is_re_validated_rather_than_trusted() {
    let card = card_for(&known_response(Freshness::Current));
    let json = render_budget_json(&card).unwrap();
    assert_eq!(budget_card_from_value(&json).unwrap(), card);

    let mut tampered = json.clone();
    tampered["actor_id"] = serde_json::json!("someone-else");
    assert!(budget_card_from_value(&tampered).is_err());

    let mut forged_number = json;
    forged_number["remaining"] = serde_json::json!({"currency": "USD", "micros": 600});
    assert!(
        budget_card_from_value(&forged_number).is_err(),
        "a hand-written remainder must be refused on decode"
    );
}

#[test]
fn the_presenter_prints_unknown_for_an_unstated_amount() {
    let card = card_for(&known_response(Freshness::Current));
    let text = render_budget_text(&card).unwrap();
    assert!(text.contains("remaining: unknown"), "{text}");
    assert!(text.contains("queue_limit: unknown"), "{text}");
    assert_eq!(render_amount(None), "unknown");
    assert_eq!(
        render_amount(Some(
            &kiana_commands::billing_card::BudgetAmount::new("USD", 12).unwrap()
        )),
        "12 USD"
    );
}

#[test]
fn a_correction_card_without_approval_prints_pending_in_every_surface() {
    let card = BudgetCorrectionCard::new(
        CostCorrectionId::new(),
        LedgerEntryId::new(),
        TARGET_DIGEST,
        kiana_domain::COST_CORRECTION_COMMAND,
        None,
        PROJECTION,
        vec!["receipt:cost-receipt-1".to_owned()],
    )
    .unwrap();
    let text = render_correction_text(&card).unwrap();
    assert!(text.contains("pending_approval"), "{text}");
    assert!(!text.contains("approved"), "{text}");
}

#[test]
fn a_correction_card_with_a_nil_approval_id_is_refused() {
    let mut card = BudgetCorrectionCard::new(
        CostCorrectionId::new(),
        LedgerEntryId::new(),
        TARGET_DIGEST,
        kiana_domain::COST_CORRECTION_COMMAND,
        None,
        PROJECTION,
        Vec::new(),
    )
    .unwrap();
    card.approval_id = Some(kiana_domain::ApprovalId::from_uuid(uuid::Uuid::nil()));
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_correction_card_invalid".to_owned())
    );
}

#[test]
fn the_presenter_schema_is_its_own_versioned_name() {
    assert_eq!(BUDGET_PRESENTER_SCHEMA, "kiana.budget-presenter.v1");
    assert_ne!(
        BUDGET_PRESENTER_SCHEMA,
        kiana_commands::billing_card::BUDGET_CARD_SCHEMA
    );
    assert_ne!(BUDGET_PRESENTER_SCHEMA, kiana_domain::BILLING_QUERY_SCHEMA);
}
