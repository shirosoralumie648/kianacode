//! BQ-24 entrypoint budget presenter.
//!
//! CLI, Workbench, Web and Desktop all render the budget card through this one module, so a
//! fourth "just this surface" is the only place a divergent number could be introduced — and the
//! [`BudgetCardParity`](kiana_commands::billing_card::BudgetCardParity) check refuses it.
//!
//! The presenter is display-only. It holds no budget state, computes no remainder, and re-derives
//! nothing from a stream. It also refuses a caller-supplied actor/project outright, because the
//! card's identity came from the server and a surface that can overwrite it has widened authority
//! over whose money is on screen.
//!
//! Proof ceiling: `source`. Rendering a card does not prove the numbers behind it were correct.

use kiana_commands::billing_card::{
    budget_card_json, budget_card_parity, BudgetAmount, BudgetCard, BudgetCardFrame,
    BudgetCardParity, BudgetCorrectionCard, BudgetSurface, MAX_BUDGET_CARD_QUEUE_ROWS,
};
use kiana_domain::{BillingQueryResponse, ProjectId};
use serde_json::Value;

pub const BUDGET_PRESENTER_SCHEMA: &str = "kiana.budget-presenter.v1";

/// Render the card text a terminal surface prints.
pub fn render_budget_text(card: &BudgetCard) -> Result<String, String> {
    card.validate()?;
    Ok(card.render_text())
}

/// Render the card JSON a Web surface hydrates. It is a straight projection; the Web renderer must
/// not add a computed remainder of its own.
pub fn render_budget_json(card: &BudgetCard) -> Result<Value, String> {
    budget_card_json(card)
}

/// Build the four-surface parity set for one card. Every surface renders the same text, so the
/// check is about wiring, not about four hand-written presenters agreeing by luck.
pub fn budget_surface_parity(card: &BudgetCard) -> Result<BudgetCardParity, String> {
    budget_card_parity(card)
}

/// Record one real surface's rendering. A surface that renders something other than the card's own
/// text is refused at construction, so a divergent number cannot reach a screen.
pub fn budget_surface_frame(
    card: &BudgetCard,
    surface: BudgetSurface,
) -> Result<BudgetCardFrame, String> {
    BudgetCardFrame::new(surface, card, card.render_text())
}

/// Parse a card out of a wire value. This is the only decode path; it validates rather than
/// trusting whatever a surface was handed.
pub fn budget_card_from_value(value: &Value) -> Result<BudgetCard, String> {
    let card: BudgetCard = serde_json::from_value(value.clone())
        .map_err(|_| "budget_card_decode_failed".to_owned())?;
    card.validate()?;
    Ok(card)
}

/// Re-check a card against the response it claims to render. A card carried across a request, or
/// edited to a friendlier remainder, fails here.
pub fn budget_card_matches_response(
    card: &BudgetCard,
    response: &BillingQueryResponse,
) -> Result<(), String> {
    card.validate_against(response)
}

/// Render the correction approval row. A card without an approval id prints `pending_approval`; it
/// never implies the correction is approved, and it never offers an apply button of its own.
pub fn render_correction_text(card: &BudgetCorrectionCard) -> Result<String, String> {
    card.validate()?;
    Ok(card.render_text())
}

/// A surface-supplied identity override is always rejected. Both fields are server-resolved; a
/// `--actor`/`--project` flag reaching the renderer means the caller tried to widen the scope.
pub fn reject_identity_override(
    actor_id: Option<&str>,
    project_id: Option<ProjectId>,
) -> Result<(), String> {
    if actor_id.is_some() || project_id.is_some() {
        return Err("budget_card_identity_override_rejected".to_owned());
    }
    Ok(())
}

/// Upper bound on what one screen may show, re-exported so a surface cannot pick a larger one.
pub const MAX_PRESENTER_QUEUE_ROWS: usize = MAX_BUDGET_CARD_QUEUE_ROWS;

/// Amounts are carried, never computed. A surface asks the server-side card for the value; this
/// helper only exists so a renderer has one canonical spelling.
pub fn render_amount(amount: Option<&BudgetAmount>) -> String {
    amount.map_or_else(
        || kiana_commands::billing_card::UNKNOWN_TOKEN.to_owned(),
        BudgetAmount::render,
    )
}
