#[test]
fn policy_bundle_contract_stays_pure_and_consumes_existing_policy_trait() {
    let policy = include_str!("../../kiana-policy/src/security.rs");
    let policy_lib = include_str!("../../kiana-policy/src/lib.rs");
    let core = include_str!("../src/lib.rs");
    for marker in [
        "PolicyBundle",
        "PolicyRevision",
        "DecisionTrace",
        "BundlePolicyEngine",
        "evaluate_with_snapshot",
        "PolicyOperationUnregistered",
        "PolicyAuthorityEpochStale",
        "POLICY_BUNDLE_SCHEMA",
    ] {
        assert!(policy.contains(marker), "policy marker missing: {marker}");
    }
    assert!(policy_lib.contains("pub use security::*;"));
    assert!(core.contains("PolicyEngine"));
    for forbidden in [
        "CapabilityBroker",
        "DaemonHost",
        "std::process",
        "reqwest",
        "authorize_and_execute",
    ] {
        assert!(
            !policy.contains(forbidden),
            "policy contract must not depend on {forbidden}"
        );
    }
}
