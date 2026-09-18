#[test]
fn evaluation_target_has_no_second_runner_or_authority_path() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    for marker in [
        "pub struct EvalTarget",
        "DaemonHost",
        "ControlPlane",
        "RequestEnvelope",
        "ResponseEnvelope",
        "self.host.handle(request).await",
        "EVAL_TARGET_SPINE_SCHEMA",
    ] {
        assert!(runtime.contains(marker), "spine marker missing: {marker}");
    }
    for forbidden in [
        "KianaHarness",
        "RunnerPort",
        "CapabilityBroker {",
        "tokio::spawn",
        "reqwest",
        "Command::new",
        "std::net",
        "PolicyEngine",
        "GateEngine",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "second path marker found: {forbidden}"
        );
    }
}
