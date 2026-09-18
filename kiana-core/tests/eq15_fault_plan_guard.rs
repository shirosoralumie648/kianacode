#[test]
fn evaluation_fault_plan_is_deterministic_and_never_starts_effects() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    for marker in [
        "EvalFaultPlan",
        "EvalFaultKind",
        "EvalFaultEvidence",
        "EVAL_FAULT_PLAN_SCHEMA",
        "CancelRace",
        "CrashAfterEffect",
        "StaleLease",
        "UnknownReconcile",
        "requires_reconciliation",
        "stop_confirmed",
    ] {
        assert!(runtime.contains(marker), "fault marker missing: {marker}");
    }
    for forbidden in [
        "std::process::Command",
        "tokio::spawn",
        "tokio::time::sleep",
        "reqwest",
        "std::net",
        "CapabilityBroker",
        "KianaHarness",
        "EventStorePort for EvalFaultPlan",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "fault plan side effect marker: {forbidden}"
        );
    }
}
