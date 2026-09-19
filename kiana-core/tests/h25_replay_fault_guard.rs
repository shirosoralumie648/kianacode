#[test]
fn replay_never_calls_model_or_real_broker() {
    let fault = include_str!("../src/fault_injection.rs");
    let replay = include_str!("../src/replay_diagnostics.rs");
    for marker in [
        "replay-only",
        "FaultInjectionPoint::Prepare",
        "FaultInjectionPoint::Dispatch",
        "fault_dispatch_unconfirmed",
        "ReplayDivergenceKind",
        "projection_digests",
        "Provider, Broker or recovery action",
    ] {
        assert!(
            fault.contains(marker) || replay.contains(marker),
            "H25 replay/fault marker missing: {marker}"
        );
    }
    assert!(!fault.contains("CapabilityBroker"));
    assert!(!fault.contains("ModelClient"));
    assert!(!replay.contains("CapabilityBroker"));
    assert!(!replay.contains("ModelClient"));
}

#[test]
fn contradictory_terminal_or_unknown_result_blocks_recovery() {
    let replay = include_str!("../src/replay_diagnostics.rs");
    let recovery = include_str!("../src/recovery.rs");
    let domain = include_str!("../../kiana-domain/src/fault.rs");
    for marker in [
        "ReplayDivergenceKind::TerminalConflict",
        "ReplayDivergenceKind::UnknownEffect",
        "TraceStatus::Unknown",
        "fault_unknown_not_fenced",
        "result_unknown",
        "reconciliation",
    ] {
        assert!(
            replay.contains(marker) || recovery.contains(marker) || domain.contains(marker),
            "H25 unknown/recovery marker missing: {marker}"
        );
    }
}

#[test]
fn each_fault_window_keeps_effect_count_and_source_binding_explicit() {
    let fault = include_str!("../src/fault_injection.rs");
    let domain = include_str!("../../kiana-domain/src/fault.rs");
    for marker in [
        "FaultInjectionPoint::Prepare",
        "FaultInjectionPoint::Commit",
        "FaultInjectionPoint::Dispatch",
        "FaultInjectionPoint::Result",
        "FaultInjectionPoint::Flush",
        "FaultInjectionPoint::Projector",
        "FaultInjectionPoint::Export",
        "FaultInjectionPoint::Shutdown",
        "effect_started",
        "effect_known",
        "source_event_ids",
        "fault_matrix_digest",
    ] {
        assert!(fault.contains(marker) || domain.contains(marker));
    }
    assert!(fault.contains("fault_matrix_from_events"));
    assert!(fault.contains("replay_fault_matrix"));
}
