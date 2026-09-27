//! BQ-24 source guard.
//!
//! The card's whole value is what it *refuses* to compute, so the source itself is asserted: the
//! module must not gain a subtraction, a subtraction helper, an identity parameter it trusts, or
//! an apply path. This is a source-shape guard, not a behavioural one — the behaviour lives in
//! `kiana-commands/tests/bq24_budget_card.rs` and
//! `kiana-entrypoints/tests/bq24_budget_presenter.rs`.

#[test]
fn budget_card_module_keeps_its_schema_and_deny_first_shape() {
    let card = include_str!("../src/billing_card.rs");
    let command = include_str!("../src/budget.rs");

    for marker in [
        "pub const BUDGET_CARD_SCHEMA: &str = \"kiana.budget-card.v1\"",
        "pub const BUDGET_CARD_PARITY_SCHEMA: &str = \"kiana.budget-card-parity.v1\"",
        "pub const BUDGET_CORRECTION_CARD_SCHEMA: &str = \"kiana.budget-correction-card.v1\"",
        "pub const BUDGET_CORRECTION_INTENT_SCHEMA: &str = \"kiana.budget-correction-intent.v1\"",
        "pub const UNKNOWN_TOKEN: &str = \"unknown\"",
        "pub const MAX_BUDGET_CARD_QUEUE_ROWS: usize = 64",
        "pub const MAX_BUDGET_CARD_TEXT: usize = 256",
        "#[serde(deny_unknown_fields)]",
        "pub struct BudgetCard {",
        "pub struct BudgetAmount {",
        "pub struct BudgetQueueRow {",
        "pub struct BudgetCardFrame {",
        "pub struct BudgetCardParity {",
        "pub struct BudgetCorrectionCard {",
        "pub struct BudgetCorrectionIntent {",
        "pub struct BudgetCardIdentityOverride {",
        "pub enum BudgetState {",
        "pub enum OverageReason {",
        "pub enum BudgetSurface {",
        "pub fn from_response(",
        "pub fn validate_against(",
        "pub fn validate_against_command(",
        "pub fn render_text(&self)",
        "pub fn digest(&self)",
        "pub fn committed_spend(",
        "pub fn overage_reason_for_unknown(",
        "pub fn budget_card_parity(",
        "pub fn budget_card_json(",
        "budget_card_identity_override_rejected",
        "budget_card_parity_render_mismatch",
        "budget_card_parity_surface_count",
        "budget_card_parity_surface_missing",
        "budget_card_unknown_state_numeric",
        "budget_card_pending_state_invalid",
        "budget_card_queue_over_limit",
        "budget_card_queue_duplicate_ticket",
        "budget_card_remaining_limit_binding_invalid",
        "budget_card_digest_mismatch",
        "budget_correction_card_evidence_secret",
        "budget_correction_card_evidence_unredacted",
        "budget_correction_command_binding_mismatch",
        "budget_correction_intent_invalid",
        "scan_secret_sentinels(SecretScanChannel::Event",
        "redact_text(reference)",
        "self.command != kiana_domain::COST_CORRECTION_COMMAND",
        "self.approval_id != command.approval.as_ref().map(|a| a.approval_id)",
        "frame.rendered_text != card.render_text()",
        "if !seen.contains(&surface)",
    ] {
        assert!(
            card.contains(marker) || command.contains(marker),
            "BQ-24 marker missing: {marker}"
        );
    }
}

#[test]
fn budget_card_never_computes_a_remainder_or_an_amount() {
    let card = include_str!("../src/billing_card.rs");
    let command = include_str!("../src/budget.rs");
    for forbidden in [
        // A remainder is a subtraction the ledger already performed. If one appears here the card
        // can drift from the authoritative rollup.
        "checked_sub",
        "saturating_sub",
        "wrapping_sub",
        "micros - ",
        " - self.spent",
        "self.limit - ",
        // An unknown rendered as zero is a lie that reads as "you have budget left".
        "unwrap_or(0",
        "unwrap_or_default()",
        ".or(Some(",
    ] {
        assert!(
            !card.contains(forbidden),
            "BQ-24 card computed or defaulted a value: {forbidden}"
        );
    }
    // The only addition permitted is summing the four server rollup buckets.
    assert!(card.contains("existing.checked_add(amount)?"));
    assert!(!card.contains("existing.checked_add(amount)\n                .map_err"));
}

#[test]
fn budget_card_is_a_projection_and_owns_no_authority() {
    let card = include_str!("../src/billing_card.rs");
    let command = include_str!("../src/budget.rs");
    for forbidden in [
        "DaemonHost",
        "CapabilityBroker",
        "KianaHarness",
        "EventLog",
        "EventStorePort",
        "tokio::spawn",
        "std::process::Command",
        "reqwest::",
        "std::fs::",
        "sqlx",
        // No mutation of billing state from a display path.
        "append(",
        "reserve(",
        "release_reservation",
        "consume_approval",
        "stage_approval",
        // A surface may name the existing command; it may not mint an approval.
        "CostCorrectionApproval::new",
    ] {
        assert!(
            !card.contains(forbidden),
            "BQ-24 card gained authority: {forbidden}"
        );
    }
    // Identity comes from the server; a caller override is a rejected type, not an optional field.
    assert!(card.contains("pub fn reject(&self) -> Result<(), String>"));
    assert!(card.contains("if self.actor_id.is_some() || self.project_id.is_some()"));
}

#[test]
fn budget_command_only_names_versioned_commands_and_carries_no_identity_flag() {
    let command = include_str!("../src/budget.rs");
    for marker in [
        "pub const BUDGET_CARD_COMMAND: &str = \"budget.query.v1\"",
        "pub const BUDGET_CORRECTION_COMMAND: &str = kiana_domain::COST_CORRECTION_COMMAND",
        "pub fn budget_query_request(",
        "pub fn render_budget_card(",
        "pub fn render_budget_text(",
        "pub fn correction_intent(",
        "pub struct ServerBudgetFacts {",
        "read_only: true",
        "budget_identity_override_rejected",
        "command_requires_control_plane",
        "CommandRoute::ControlPlane { name, arguments }",
        "kiana_domain::COST_CORRECTION_COMMAND",
    ] {
        assert!(
            command.contains(marker),
            "BQ-24 command marker missing: {marker}"
        );
    }
    // A silently-ignored `--actor` would leave the user believing the card showed their scope, so
    // the flag is refused by name and never parsed into the arguments.
    assert!(command.contains("_ if arg.starts_with(\"--actor\") || arg.starts_with(\"--project\")"));
    assert!(!command.contains("\"actor_id\": actor"));
    assert!(!command.contains("\"project_id\": project"));

    for forbidden in [
        "checked_sub",
        "saturating_sub",
        "unwrap_or(0",
        // The surface routes a command; it never drives the runtime itself.
        "DaemonHost",
        "CapabilityBroker",
        "KianaHarness",
        "tokio::spawn",
        "std::process::Command",
        "reqwest::",
        "std::fs::",
        // A surface may name the existing command; it may not mint an approval.
        "CostCorrectionApproval",
    ] {
        assert!(
            !command.contains(forbidden),
            "BQ-24 command computed or widened: {forbidden}"
        );
    }
}
