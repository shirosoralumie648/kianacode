#[test]
fn dep15_diagnostics_is_redacted_read_only_and_mode_bound() {
    let domain = include_str!("../../kiana-domain/src/ops_diagnostics.rs");
    let core = include_str!("../src/ops_diagnostics.rs");
    for marker in [
        "OpsDiagnosticsInput",
        "OpsDiagnosticFact",
        "OpsDiagnosticsReport",
        "render_json",
        "render_human",
        "reproduction",
        "exit_code",
        "read_only_diagnostics_no_fact_mutation",
        "ops_diagnostics_mode_mismatch",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-15 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "std::env",
        "std::process",
        "EventStore::append",
        "CapabilityBroker",
        "KianaHarness",
        "ProviderGateway",
        "tokio::",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-15 diagnostics crossed effect boundary: {forbidden}"
        );
    }
}
