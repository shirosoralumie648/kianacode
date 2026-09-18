#[test]
fn eval_runtime_is_an_isolation_adapter_not_a_second_runner() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "EvalRuntimeSandbox",
        "KIANA_HOME",
        "workspace",
        "ClockObservation",
        "deterministic_bytes",
        "with_process_environment",
        "create_dir",
        "remove_dir_all",
    ] {
        assert!(
            runtime.contains(marker),
            "eval runtime marker missing: {marker}"
        );
    }
    assert!(daemon.contains("pub mod eval_runtime"));
    assert!(!runtime.contains("KianaHarness"));
    assert!(!runtime.contains("CapabilityBroker"));
    assert!(!runtime.contains("tokio::spawn"));
    assert!(!runtime.contains("reqwest"));
}
