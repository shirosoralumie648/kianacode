#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source, production_source};

#[test]
fn ci12_product_gate_reuses_one_spine_and_keeps_live_opt_in_explicit() {
    let domain = include_str!("../../kiana-domain/src/ci12_product_gate.rs");
    let core = include_str!("../src/ci12_product_gate.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let entrypoint = include_str!("../../kiana-entrypoints/src/company_user_flow.rs");
    let route = include_str!("../../kiana-entrypoints/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/ci12-product-gate-baseline.md");
    for marker in [
        "Ci12ProductGate",
        "MissingAuthentication",
        "UntrustedProject",
        "CrossProjectScope",
        "ExpiredApproval",
        "SecretLeak",
        "ToctouDrift",
        "ResultUnknown",
        "FakeProviderSuccess",
        "LiveOptIn",
        "DaemonHost",
        "ControlPlane",
        "main_from_args",
        "approval:",
        "validate_ci12_product_gate",
    ] {
        assert!(
            domain.contains(marker)
                || core.contains(marker)
                || daemon.contains(marker)
                || entrypoint.contains(marker)
                || baseline.contains(marker),
            "CI-12 marker missing: {marker}"
        );
    }
    assert!(production_source(route).contains("pub mod company_user_flow;"));
    let main = item_source(entrypoint, "pub async fn main_from_args(");
    assert!(main.contains("DaemonHost::local("));
    assert!(compact_code(&main).contains("host.handle(envelope).await"));
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "auto_approve",
        "raw_secret",
    ] {
        assert!(
            !production_source(domain).contains(forbidden)
                && !production_source(core).contains(forbidden),
            "CI-12 bypass marker present: {forbidden}"
        );
    }
}
