//! CAP-31/CAP-32 source guard: an unproven platform backend must refuse, and must not substitute.
//!
//! The honesty requirement for these two cards is one-directional and worth stating in the guard
//! itself: a macOS or Windows backend that is selected on this Linux host and "succeeds" would be
//! a lie about confinement. Refusing is the correct behaviour, so the guard asserts that the
//! refusal is expressible, sealed, and that the contract has no path by which a weaker backend
//! could be named in its place.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-31/32 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn backend_selection_refuses_and_names_no_substitute() {
    let selection = include_str!("../../kiana-domain/src/backend_selection.rs");
    let report = include_str!("../../kiana-domain/src/platform_backend.rs");
    require(
        report,
        &[
            "PlatformTarget",
            "PlatformBackendDisposition",
            "PlatformBackendReport",
            "TargetOnly",
            "NotSupported",
            "behavior_verified",
            "platform_backend_implemented_requires_behavior",
            "platform_backend_unverified_disposition_mismatch",
            "platform_backend_host_fallback_forbidden",
            "platform_backend_limitation_required",
        ],
        "platform disposition",
    );
    require(
        selection,
        &[
            "BACKEND_SELECTION_SCHEMA",
            "BACKEND_SELECTION_DECISION_SCHEMA",
            "BACKEND_SELECTION_VERSION",
            "BackendSelectionRequest",
            "BackendSelectionDecision",
            "BackendSelection",
            "TargetOnly",
            "NotSupported",
            "Blocked",
            "Selected",
            "is_usable",
            "HOST_EXECUTION_BACKEND",
            "backend_is_selectable",
            "target_only_limitations",
            "no_host_fallback: bool",
            "grants_execution: bool",
            "effective_backend: String",
        ],
        "selection contract",
    );
    // The specific refusals, each named so a regression is legible in CI output.
    require(
        selection,
        &[
            "backend_selection_target_mismatch",
            "backend_selection_unknown_backend",
            "backend_selection_report_invalid",
            "backend_selection_target_only_backend",
            "backend_selection_behavior_unverified",
            "backend_selection_blocked",
            "backend_selection_not_supported",
            "backend_selection_substitute_backend_named",
            "backend_selection_effective_backend_mismatch",
            "backend_selection_host_fallback_forbidden",
            "backend_selection_grants_execution",
        ],
        "refusal codes",
    );
    require(
        selection,
        &[
            "pub fn evaluate(",
            "pub fn validate_against(",
            "pub fn usable(",
            "fn derive(",
        ],
        "fixed decision order",
    );
    // The decision must not consult the host, name a sandbox, or reach an OS. `host_target` is a
    // supplied fact precisely so this module never goes looking.
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStorePort",
        "CapabilityBroker",
        "std::env",
        "sandbox_exec",
        "seatbelt",
    ] {
        assert!(
            !selection.contains(forbidden),
            "CAP-31/32 selection crossed the effect boundary: {forbidden}"
        );
    }
}

#[test]
fn the_declared_host_backend_still_matches_the_sandbox_implementation() {
    // `HOST_EXECUTION_BACKEND` is declared so the selection contract and the sandbox
    // implementation cannot drift apart silently. If the product backend ever changes, this
    // assertion is what has to be updated with it.
    let selection = include_str!("../../kiana-domain/src/backend_selection.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    assert!(
        selection.contains("pub const HOST_EXECUTION_BACKEND: &str = \"bwrap\";"),
        "CAP-31/32 declared host backend drifted from the sandbox constant"
    );
    assert!(
        sandbox.contains("pub const SANDBOX_BACKEND: &str = \"bwrap\";"),
        "CAP-31/32 sandbox backend constant changed; re-check HOST_EXECUTION_BACKEND"
    );
    // The product sandbox is still Linux-gated; a non-Linux build has no bwrap implementation,
    // which is exactly why the macOS and Windows backends stay unproven here.
    assert!(sandbox.contains("target_os = \"linux\""));
    assert!(sandbox.contains("sandbox_backend_unsupported:bwrap"));
}

#[test]
fn cap31_and_cap32_cases_are_fixtures_and_stay_unverified() {
    let cap31 = include_str!("../../kiana-domain/tests/cap31_macos_backend.rs");
    require(
        cap31,
        &[
            "fn macos_missing_backend_has_no_host_fallback()",
            "fn macos_backend_denies_host_secrets_and_gui_escape()",
            "fn macos_descendant_escape_or_stop_failure_is_visible()",
        ],
        "CAP-31 rejected-first fixtures",
    );
    let cap32 = include_str!("../../kiana-domain/tests/cap32_windows_backend.rs");
    require(
        cap32,
        &[
            "fn windows_reparse_or_unc_path_cannot_escape_scope()",
            "fn windows_child_cannot_break_away_from_job()",
            "fn windows_cross_process_lock_is_real_or_operation_is_denied()",
        ],
        "CAP-32 rejected-first fixtures",
    );
    let baseline31 = include_str!("../../docs/roadmap/cap31-macos-backend-baseline.md");
    for marker in [
        "partial",
        "behavior_verified = false",
        "target-machine receipt",
        "proves nothing about macOS",
    ] {
        assert!(
            baseline31.contains(marker),
            "CAP-31 baseline limitation marker missing: {marker}"
        );
    }
    let baseline32 = include_str!("../../docs/roadmap/cap32-windows-backend-baseline.md");
    for marker in [
        "partial",
        "target-machine receipt",
        "proves nothing about Windows",
        "That report is fixture data, not evidence",
    ] {
        assert!(
            baseline32.contains(marker),
            "CAP-32 baseline limitation marker missing: {marker}"
        );
    }
    // The one fixture that reaches `Selected` constructs a behaviour-verified report by hand.
    // That must be stated as fixture data in the baseline, not left to look like a receipt.
    assert!(
        cap32.contains("That report is fixture data"),
        "CAP-32 fixture must label its behaviour-verified report as fixture data"
    );
}
