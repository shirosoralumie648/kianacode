#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn eval_deny_broker_never_has_a_real_executor_path() {
    let runtime =
        source_guard::production_source(include_str!("../../kiana-daemon/src/eval_runtime.rs"));
    let boundary = [
        source_guard::item_source(&runtime, "pub struct DenyByDefaultEvalBroker"),
        source_guard::item_source(&runtime, "impl Default for DenyByDefaultEvalBroker"),
        source_guard::item_source(&runtime, "impl DenyByDefaultEvalBroker"),
        source_guard::item_source(
            &runtime,
            "impl CapabilityBrokerPort for DenyByDefaultEvalBroker",
        ),
    ]
    .join("\n");
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
            !boundary.contains(forbidden),
            "real executor marker found: {forbidden}"
        );
    }
}
