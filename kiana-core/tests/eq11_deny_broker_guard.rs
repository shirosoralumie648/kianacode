#[test]
fn eval_deny_broker_never_has_a_real_executor_path() {
    let runtime = include_str!("../../kiana-daemon/src/eval_runtime.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    for marker in [
        "DenyByDefaultEvalBroker",
        "EVAL_DENY_BROKER_SCHEMA",
        "CapabilityBrokerPort",
        "eval_effect_denied",
        "CapabilityErrorCode::PermissionDenied",
        "execute_cancellable",
        "denied_calls",
    ] {
        assert!(
            runtime.contains(marker),
            "deny broker marker missing: {marker}"
        );
    }
    assert!(daemon.contains("pub mod eval_runtime"));
    for forbidden in [
        "reqwest",
        "TcpStream",
        "std::net",
        "Command::new",
        "SecretStore",
        "KianaHarness",
        "CapabilityBroker {",
        "tokio::spawn",
        "std::env::var",
    ] {
        assert!(
            !runtime.contains(forbidden),
            "real executor marker found: {forbidden}"
        );
    }
}
