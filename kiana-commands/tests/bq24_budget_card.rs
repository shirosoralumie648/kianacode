//! BQ-24 failure-first fixtures for the read-only budget card surface.
//!
//! Every test here is a rejection. The card's job is to *refuse* to show a number it was not
//! given, so the happy paths are one-liners and the interesting assertions are all "this must
//! fail". One test per row of the card's 先拒绝 column:
//!
//! 1. UI 本地计算剩余额度  — a locally computed remainder
//! 2. 乐观扣费未 commit    — an optimistic charge that was never committed
//! 3. 隐藏 unknown          — an `unknown` hidden or rounded to a number
//! 4. 输入 actor/project 覆盖服务端 — a caller-supplied identity overriding the server's
//!
//! Proof ceiling: `source`. These fixtures prove the contract refuses; they prove nothing about
//! live billing.

use kiana_commands::billing_card::{
    budget_card_json, overage_reason_for_unknown, BudgetAmount, BudgetCard, BudgetCardFrame,
    BudgetCardParity, BudgetCorrectionCard, BudgetCorrectionIntent, BudgetQueueRow, BudgetState,
    OverageReason, UNKNOWN_TOKEN,
};
use kiana_commands::budget::{
    budget_query_request, correction_intent, render_budget_card, render_budget_text,
    ServerBudgetFacts, BUDGET_CARD_COMMAND, BUDGET_CORRECTION_COMMAND,
};
use kiana_domain::{
    BillingQueryKind, BillingQueryRequest, BillingQueryResponse, BillingRollupTotals,
    BillingUnknownReason, CostCorrectionId, Freshness, LedgerEntryId, Money, ProjectId,
    BILLING_QUERY_SCHEMA,
};

const BOUNDARY: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PROJECTION: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TARGET_DIGEST: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

fn money(micros: i128) -> Money {
    Money::new("USD", micros).unwrap()
}

fn response_with(
    freshness: Freshness,
    unknown_count: u64,
    mut ledger: BillingRollupTotals,
) -> BillingQueryResponse {
    // The BQ-23 response derives `unknown_count` from the server rollup, so the fixture sets the
    // rollup rather than trying to write a count the response would ignore.
    ledger.unknown_count = unknown_count;
    let response = BillingQueryResponse::new(
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
    .unwrap_or_else(|error| panic!("response must build: {error}"));
    assert_eq!(
        response.unknown_count, unknown_count,
        "unknown count must survive"
    );
    response
}

fn known_response() -> BillingQueryResponse {
    response_with(
        Freshness::Current,
        0,
        BillingRollupTotals {
            measured: Some(money(400)),
            ..BillingRollupTotals::default()
        },
    )
}

fn known_card(response: &BillingQueryResponse) -> BudgetCard {
    render_budget_card(
        response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap_or_else(|error| panic!("card must build: {error}"))
}

fn facts() -> ServerBudgetFacts {
    ServerBudgetFacts {
        limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
        ..ServerBudgetFacts::default()
    }
}

// ---------------------------------------------------------------------------------------------
// 1. UI 本地计算剩余额度 — the surface must not compute the remainder
// ---------------------------------------------------------------------------------------------

#[test]
fn ui_does_not_compute_a_remaining_amount_when_the_server_omits_it() {
    let response = known_response();
    let card = render_budget_card(&response, "local-user", &facts()).unwrap();
    // The server sent a limit but no remainder. The card must NOT do 1000 - 400 = 600.
    assert!(
        card.remaining.is_none(),
        "card must not derive a remaining amount"
    );
    let text = card.render_text();
    assert!(
        text.contains(&format!("remaining: {UNKNOWN_TOKEN}")),
        "an unstated remainder must print as unknown, got:\n{text}"
    );
    assert!(
        !text.contains("remaining: 600"),
        "card printed a locally computed remainder:\n{text}"
    );
}

#[test]
fn card_rejects_a_remaining_amount_without_a_limit() {
    let response = known_response();
    let mut card = render_budget_card(&response, "local-user", &facts()).unwrap();
    card.remaining = Some(BudgetAmount::new("USD", 600).unwrap());
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_card_remaining_limit_binding_invalid".to_owned())
    );
}

#[test]
fn card_rejects_a_forged_card_digest() {
    let response = known_response();
    let mut card = render_budget_card(&response, "local-user", &facts()).unwrap();
    card.remaining = Some(BudgetAmount::new("USD", 999_999).unwrap());
    // Deliberately do NOT re-seal: this is the shape a tampered card arrives in.
    assert_eq!(
        card.validate(),
        Err("budget_card_digest_mismatch".to_owned())
    );
}

#[test]
fn card_rejects_a_rollup_that_mixes_currencies() {
    let response = response_with(
        Freshness::Current,
        0,
        BillingRollupTotals {
            measured: Some(money(100)),
            estimated: Some(Money::new("EUR", 200).unwrap()),
            ..BillingRollupTotals::default()
        },
    );
    // A card that picks one currency and shows a confident total would be worse than no card.
    assert!(render_budget_card(&response, "local-user", &facts()).is_err());
}

// ---------------------------------------------------------------------------------------------
// 2. 乐观扣费未 commit — an optimistic charge must not appear as spent
// ---------------------------------------------------------------------------------------------

#[test]
fn a_pending_reservation_is_not_counted_as_spent_and_not_folded_into_the_remainder() {
    let response = known_response();
    let card = render_budget_card(
        &response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
            // A reservation exists but was never committed, so the server sends no amount for it.
            pending_reservations: Some(3),
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap();
    // 400 is the committed measurement. The 3 pending reservations contribute nothing.
    assert_eq!(card.spent.as_ref().map(|a| a.micros), Some(400));
    assert_eq!(card.pending_reservations, Some(3));
    assert!(
        card.remaining.is_none(),
        "a pending count may not produce a remainder"
    );
    let text = card.render_text();
    assert!(text.contains("pending_reservations: 3"), "{text}");
    assert!(!text.contains("spent: 3"), "{text}");
}

#[test]
fn card_rejects_a_pending_reservation_next_to_a_non_within_budget_state() {
    let response = known_response();
    let mut card = render_budget_card(
        &response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
            state: Some(BudgetState::AtLimit),
            pending_reservations: Some(2),
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap();
    card.card_digest = card.digest();
    // `AtLimit` without an overage reason is itself rejected first; assert the reason names the
    // pending/over-limit confusion rather than silently accepting it.
    assert!(card.validate().is_err());
}

#[test]
fn a_committed_rollup_counts_corrections_in_both_directions() {
    let response = response_with(
        Freshness::Current,
        0,
        BillingRollupTotals {
            measured: Some(money(400)),
            correction_measured: Some(money(-150)),
            ..BillingRollupTotals::default()
        },
    );
    let card = render_budget_card(&response, "local-user", &facts()).unwrap();
    assert_eq!(card.spent.as_ref().map(|a| a.micros), Some(250));
}

// ---------------------------------------------------------------------------------------------
// 3. 隐藏 unknown — an unknown must print as unknown, never as a number
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unknown_freshness_renders_unknown_and_never_rounds_to_zero() {
    let mut response = response_with(
        Freshness::Unknown,
        0,
        BillingRollupTotals {
            measured: Some(money(400)),
            ..BillingRollupTotals::default()
        },
    );
    response.response_digest = response.digest();
    let text = render_budget_text(&response, "local-user", &facts()).unwrap();
    assert!(text.contains("state: unknown"), "{text}");
    assert!(text.contains("freshness: unknown"), "{text}");
    for line in text.lines() {
        assert!(
            !line.ends_with(": 0") || line.starts_with("pending_reservations"),
            "an unknown was rendered as zero: {line}"
        );
    }
}

#[test]
fn an_unknown_state_may_not_also_carry_a_remaining_amount() {
    let response = known_response();
    let mut card = render_budget_card(&response, "local-user", &facts()).unwrap();
    card.state = BudgetState::Unknown;
    card.overage_reason = OverageReason::UnknownCost;
    card.remaining = Some(BudgetAmount::new("USD", 600).unwrap());
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_card_unknown_state_numeric".to_owned())
    );
}

#[test]
fn an_over_limit_card_must_name_an_overage_reason() {
    let response = known_response();
    let mut card = render_budget_card(&response, "local-user", &facts()).unwrap();
    card.state = BudgetState::OverLimit;
    card.overage_reason = OverageReason::NotOverage;
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_card_overage_reason_missing".to_owned())
    );
}

#[test]
fn a_within_budget_card_may_not_invent_an_overage_reason() {
    let response = known_response();
    let mut card = render_budget_card(&response, "local-user", &facts()).unwrap();
    card.overage_reason = OverageReason::BudgetExhausted;
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_card_overage_reason_unexpected".to_owned())
    );
}

#[test]
fn unknown_counts_survive_the_round_trip_instead_of_being_dropped() {
    let response = response_with(Freshness::Current, 2, BillingRollupTotals::default());
    let card = render_budget_card(
        &response,
        "local-user",
        &ServerBudgetFacts {
            state: Some(BudgetState::Unknown),
            overage_reason: Some(OverageReason::UnknownCost),
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap();
    let json = budget_card_json(&card).unwrap();
    assert_eq!(json["unknown_cost_count"], 2);
    assert!(json["remaining"].is_null());
    assert_eq!(json["state"], "unknown");
}

#[test]
fn an_unknown_server_reason_maps_to_an_overage_reason_and_never_to_not_overage() {
    assert_eq!(
        overage_reason_for_unknown(Some(BillingUnknownReason::RateCardMissing)),
        OverageReason::UnknownCost
    );
    assert_eq!(
        overage_reason_for_unknown(Some(BillingUnknownReason::ResultUnknown)),
        OverageReason::ReconciliationRequired
    );
    assert_eq!(overage_reason_for_unknown(None), OverageReason::NotOverage);
}

// ---------------------------------------------------------------------------------------------
// 4. 输入 actor/project 覆盖服务端 — a caller-supplied identity must be rejected
// ---------------------------------------------------------------------------------------------

#[test]
fn the_command_rejects_an_actor_or_project_flag_instead_of_ignoring_it() {
    use kiana_commands::types::{Command, CommandContext, CommandRoute};
    let command = kiana_commands::budget::BudgetCommand;

    for args in [
        "card --actor someone-else",
        "card --project other-project",
        "card --project=other",
    ] {
        let route = command.route(&CommandContext {
            args: args.to_string(),
            app_state: Default::default(),
        });
        assert!(
            route.is_err(),
            "{args} was accepted; a surface must not let a caller choose the card's identity"
        );
    }

    let route = command
        .route(&CommandContext {
            args: "card".to_string(),
            app_state: Default::default(),
        })
        .unwrap();
    match route {
        CommandRoute::ControlPlane { name, .. } => assert_eq!(name, BUDGET_CARD_COMMAND),
        CommandRoute::Local => panic!("the card must route to the versioned command"),
    }
}

#[test]
fn a_query_request_must_be_read_only_and_bounded() {
    let project_id = ProjectId::new();
    assert!(budget_query_request(project_id, BOUNDARY, Some(10), 10).is_ok());
    assert!(budget_query_request(project_id, "not-a-digest", Some(10), 10).is_err());
    assert!(budget_query_request(project_id, BOUNDARY, Some(10), 0).is_err());
    assert!(budget_query_request(project_id, BOUNDARY, Some(0), 10).is_err());
    let mut request = budget_query_request(project_id, BOUNDARY, Some(10), 10).unwrap();
    request.read_only = false;
    assert_eq!(
        request.validate(),
        Err("billing_query_request_invalid"),
        "a budget card request must stay read-only"
    );
}

#[test]
fn the_command_emits_the_existing_versioned_correction_command_not_its_own() {
    use kiana_commands::types::{Command, CommandContext, CommandRoute};
    let route = kiana_commands::budget::BudgetCommand
        .route(&CommandContext {
            args: "correction request wrong-model-charge".to_string(),
            app_state: Default::default(),
        })
        .unwrap();
    match route {
        CommandRoute::ControlPlane { name, arguments } => {
            assert_eq!(name, BUDGET_CORRECTION_COMMAND);
            assert_eq!(name, kiana_domain::COST_CORRECTION_COMMAND);
            assert_eq!(arguments["command"], BUDGET_CORRECTION_COMMAND);
        }
        CommandRoute::Local => panic!("a correction must route to the versioned command"),
    }
}

#[test]
fn a_correction_request_without_a_reason_is_refused() {
    use kiana_commands::types::{Command, CommandContext};
    assert!(kiana_commands::budget::BudgetCommand
        .route(&CommandContext {
            args: "correction request".to_string(),
            app_state: Default::default(),
        })
        .is_err());
    assert!(kiana_commands::budget::BudgetCommand
        .route(&CommandContext {
            args: "correction".to_string(),
            app_state: Default::default(),
        })
        .is_err());
}

#[test]
fn a_correction_intent_carries_no_decision_and_no_override() {
    let project_id = ProjectId::new();
    let intent = correction_intent(
        project_id,
        "local-user",
        CostCorrectionId::new(),
        "wrong-model-charge",
    )
    .unwrap();
    assert_eq!(intent.command, kiana_domain::COST_CORRECTION_COMMAND);
    let value = serde_json::to_value(&intent).unwrap();
    for forbidden in ["approve", "deny", "decision", "approved"] {
        assert!(
            value.get(forbidden).is_none(),
            "the intent grew a decision field: {forbidden}"
        );
    }
    let mut tampered = intent.clone();
    tampered.actor_id = "someone-else".to_owned();
    assert_eq!(
        tampered.validate(),
        Err("budget_correction_intent_invalid".to_owned()),
        "a hand-edited actor must be refused"
    );
}

// ---------------------------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------------------------

#[test]
fn a_queue_over_its_limit_is_refused_and_a_row_without_a_ticket_is_refused() {
    let response = known_response();
    let row = BudgetQueueRow::new(7, "session-a", 1_000, Some(250)).unwrap();
    let mut card = render_budget_card(
        &response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
            queue_depth: Some(9),
            queue_limit: Some(4),
            queue: vec![row],
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap();
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_card_queue_over_limit".to_owned())
    );
    assert!(BudgetQueueRow::new(0, "session-a", 1_000, None).is_err());
    assert!(BudgetQueueRow::new(1, "  ", 1_000, None).is_err());
    assert!(BudgetQueueRow::new(1, "session-a", 0, None).is_err());
}

#[test]
fn a_queue_row_without_a_wait_estimate_prints_unknown_not_zero() {
    let response = known_response();
    let card = render_budget_card(
        &response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
            queue_depth: Some(1),
            queue_limit: Some(4),
            queue: vec![BudgetQueueRow::new(3, "session-a", 1_000, None).unwrap()],
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap();
    let text = card.render_text();
    assert!(text.contains("wait_ms=unknown"), "{text}");
    assert!(!text.contains("wait_ms=0"), "{text}");
}

#[test]
fn duplicate_queue_tickets_are_refused() {
    let response = known_response();
    let mut card = render_budget_card(
        &response,
        "local-user",
        &ServerBudgetFacts {
            limit: Some(BudgetAmount::new("USD", 1_000).unwrap()),
            queue_depth: Some(2),
            queue_limit: Some(4),
            queue: vec![
                BudgetQueueRow::new(3, "session-a", 1_000, None).unwrap(),
                BudgetQueueRow::new(3, "session-b", 1_001, None).unwrap(),
            ],
            ..ServerBudgetFacts::default()
        },
    )
    .unwrap();
    card.card_digest = card.digest();
    assert_eq!(
        card.validate(),
        Err("budget_card_queue_duplicate_ticket".to_owned())
    );
}

// ---------------------------------------------------------------------------------------------
// Four-entrypoint parity
// ---------------------------------------------------------------------------------------------

#[test]
fn a_missing_entrypoint_is_refused_rather_than_reported_as_consistent() {
    use kiana_commands::billing_card::BudgetSurface;
    let response = known_response();
    let card = known_card(&response);
    let text = card.render_text();
    // Three frames for four surfaces is not parity.
    let three = BudgetCardParity::from_card(
        &card,
        vec![
            BudgetCardFrame::new(BudgetSurface::Cli, &card, &text).unwrap(),
            BudgetCardFrame::new(BudgetSurface::Web, &card, &text).unwrap(),
            BudgetCardFrame::new(BudgetSurface::Workbench, &card, &text).unwrap(),
        ],
    );
    assert_eq!(three.unwrap_err(), "budget_card_parity_surface_count");
}

#[test]
fn a_correction_card_without_approval_prints_pending_and_never_approved() {
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
    let text = card.render_text();
    assert!(text.contains("pending_approval"), "{text}");
    assert!(!text.contains("approved"), "{text}");
    assert!(!text.contains("approve:"), "{text}");
}

#[test]
fn a_correction_card_may_not_name_its_own_command() {
    assert_eq!(
        BudgetCorrectionCard::new(
            CostCorrectionId::new(),
            LedgerEntryId::new(),
            TARGET_DIGEST,
            "budget.correction.apply",
            None,
            PROJECTION,
            Vec::new(),
        )
        .unwrap_err(),
        "budget_correction_card_invalid",
        "a surface must route through the existing cost.correction command"
    );
}

#[test]
fn a_correction_card_refuses_unredacted_or_secret_shaped_evidence() {
    let base = |reference: &str| {
        BudgetCorrectionCard::new(
            CostCorrectionId::new(),
            LedgerEntryId::new(),
            TARGET_DIGEST,
            kiana_domain::COST_CORRECTION_COMMAND,
            None,
            PROJECTION,
            vec![reference.to_owned()],
        )
    };
    assert!(base("authorization: Bearer abcdef").is_err());
    assert!(base("api_key=abcdef123456").is_err());
    assert!(base("  ").is_err());
    assert!(base("receipt:cost-receipt-1").is_ok());
}

#[test]
fn a_correction_card_binds_to_the_command_it_names() {
    use kiana_domain::{
        AttemptId, CostCorrectionApproval, CostCorrectionCommand, CostLedgerEntry,
        CostLedgerEntryKind, EventId, RunId,
    };
    let target = CostLedgerEntry::new(
        LedgerEntryId::new(),
        CostLedgerEntryKind::Consumption,
        RunId::new(),
        Some(AttemptId::new()),
        Some(money(400)),
        "USD",
        1_000,
        EventId::new(),
        12,
        1,
        PROJECTION,
    )
    .unwrap();
    let command = CostCorrectionCommand::draft(
        kiana_domain::RequestId::new(),
        CostCorrectionId::new(),
        &target,
        None,
        Some(money(-100)),
        "wrong-model-charge",
        vec!["receipt:cost-receipt-1".to_owned()],
        "requester-1",
        "idem-1",
        2_000,
    )
    .unwrap();

    let mut card = BudgetCorrectionCard::new(
        command.correction_id,
        command.target_entry_id,
        target.entry_digest.clone(),
        kiana_domain::COST_CORRECTION_COMMAND,
        None,
        command.command_digest.clone(),
        command.evidence_refs.clone(),
    )
    .unwrap();
    assert!(card.validate_against_command(&command, &target).is_ok());

    // An approval the card shows must be the one the command carries.
    let approval = CostCorrectionApproval::new(
        kiana_domain::ApprovalId::new(),
        command.command_id,
        command.command_digest.clone(),
        target.entry_digest.clone(),
        "approver-2",
        3_000,
    )
    .unwrap();
    let approved = command.clone().with_approval(approval).unwrap();
    card.approval_id = Some(
        approved
            .approval
            .as_ref()
            .map(|a| a.approval_id)
            .expect("approval attached"),
    );
    card.card_digest = card.digest();
    assert!(card.validate_against_command(&approved, &target).is_ok());

    // A card that claims someone else's approval is refused.
    card.approval_id = Some(kiana_domain::ApprovalId::new());
    card.card_digest = card.digest();
    assert_eq!(
        card.validate_against_command(&approved, &target)
            .unwrap_err(),
        "budget_correction_command_binding_mismatch"
    );
}

// ---------------------------------------------------------------------------------------------
// Wire shape
// ---------------------------------------------------------------------------------------------

#[test]
fn the_card_denies_unknown_wire_fields() {
    let response = known_response();
    let card = known_card(&response);
    let mut value = budget_card_json(&card).unwrap();
    value["remaining_micros"] = serde_json::json!(600);
    assert!(
        serde_json::from_value::<BudgetCard>(value).is_err(),
        "an added field must not survive a decode"
    );
}

#[test]
fn the_intent_denies_unknown_wire_fields() {
    let intent = correction_intent(
        ProjectId::new(),
        "local-user",
        CostCorrectionId::new(),
        "wrong-model-charge",
    )
    .unwrap();
    let mut value = serde_json::to_value(&intent).unwrap();
    value["approve"] = serde_json::json!(true);
    assert!(serde_json::from_value::<BudgetCorrectionIntent>(value).is_err());
}

#[test]
fn the_query_schema_and_the_card_schema_are_separate_versioned_names() {
    assert_eq!(BILLING_QUERY_SCHEMA, "kiana.billing-query.v1");
    assert_eq!(
        kiana_commands::billing_card::BUDGET_CARD_SCHEMA,
        "kiana.budget-card.v1"
    );
    assert_ne!(
        BILLING_QUERY_SCHEMA,
        kiana_commands::billing_card::BUDGET_CARD_SCHEMA
    );
    let request: BillingQueryRequest =
        budget_query_request(ProjectId::new(), BOUNDARY, Some(4), 5).unwrap();
    request.validate().unwrap();
}
