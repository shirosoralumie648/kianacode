//! BQ-24 entrypoint presenter source guard.
//!
//! The presenter's only job is to be the single render path four surfaces share. The guard asserts
//! it stayed that way: no second budget state, no arithmetic, and no route to the runtime. The
//! behaviour it protects lives in `bq24_budget_presenter.rs`.

#[test]
fn entrypoint_presenter_is_the_one_render_path_and_owns_no_state() {
    let presenter = include_str!("../src/budget_presenter.rs");
    for marker in [
        "pub const BUDGET_PRESENTER_SCHEMA: &str = \"kiana.budget-presenter.v1\"",
        "pub fn render_budget_text(",
        "pub fn render_budget_json(",
        "pub fn budget_surface_parity(",
        "pub fn budget_surface_frame(",
        "pub fn budget_card_from_value(",
        "pub fn budget_card_matches_response(",
        "pub fn render_correction_text(",
        "pub fn reject_identity_override(",
        "pub const MAX_PRESENTER_QUEUE_ROWS: usize = MAX_BUDGET_CARD_QUEUE_ROWS",
        "budget_card_identity_override_rejected",
        "kiana_commands::billing_card::UNKNOWN_TOKEN",
        "card.validate_against(response)",
        "card.validate()?",
    ] {
        assert!(
            presenter.contains(marker),
            "BQ-24 presenter marker missing: {marker}"
        );
    }

    for forbidden in [
        // A subtraction here would be a second, local remainder next to the server's.
        "checked_sub",
        "saturating_sub",
        // An unknown rendered as zero is the exact failure this step rejects.
        "unwrap_or(0",
        // The presenter must not reach the runtime; it renders what the daemon already answered.
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "EventLog",
        "tokio::spawn",
        "std::process::Command",
        "reqwest::",
        "std::fs::",
        // The presenter must not keep its own budget state next to the server's.
        "static ",
        "lazy_static",
        "OnceLock",
    ] {
        assert!(
            !presenter.contains(forbidden),
            "BQ-24 presenter gained state or authority: {forbidden}"
        );
    }
}

#[test]
fn every_entrypoint_module_shares_the_one_budget_presenter() {
    // A surface that grew its own card renderer is the failure this step exists to prevent, so the
    // registration is asserted rather than assumed.
    let lib = include_str!("../src/lib.rs");
    assert!(
        lib.contains("pub mod budget_presenter;"),
        "the shared budget presenter must be registered in kiana-entrypoints"
    );

    let cli = include_str!("../src/cli.rs");
    let workbench = include_str!("../src/workbench.rs");
    let web = include_str!("../src/web.rs");
    for (name, surface) in [
        ("cli.rs", cli),
        ("workbench.rs", workbench),
        ("web.rs", web),
    ] {
        assert!(
            !surface.contains("BudgetCard {"),
            "{name} declared its own budget card; surfaces must use kiana_commands::billing_card"
        );
        assert!(
            !surface.contains("checked_sub") && !surface.contains("saturating_sub"),
            "{name} computed a budget remainder locally"
        );
    }
}
