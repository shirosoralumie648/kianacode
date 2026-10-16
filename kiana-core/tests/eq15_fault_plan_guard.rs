#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn evaluation_fault_plan_is_deterministic_and_never_starts_effects() {
    let runtime =
        source_guard::production_source(include_str!("../../kiana-daemon/src/eval_runtime.rs"));
    let boundary = [
        source_guard::item_source(&runtime, "pub enum EvalFaultKind"),
        source_guard::item_source(&runtime, "pub enum EvalFaultDisposition"),
        source_guard::item_source(&runtime, "pub struct EvalFaultPlan"),
        source_guard::item_source(&runtime, "pub struct EvalFaultEvidence"),
        source_guard::item_source(&runtime, "impl EvalFaultPlan"),
    ]
    .join("\n");
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
            !boundary.contains(forbidden),
            "fault plan side effect marker: {forbidden}"
        );
    }
}
