#[test]
fn security_reason_contract_is_stable_and_does_not_authorize_or_echo_errors() {
    let domain = include_str!("../../kiana-domain/src/security_reasons.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    for marker in [
        "SecurityReasonCode",
        "AUTH_CALLER_UNTRUSTED",
        "UNKNOWN_RESULT_UNCONFIRMED",
        "SecurityRetryability",
        "SecurityRemediation",
        "classify_security_reason",
        "detail_digest",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "security reason marker missing: {marker}"
        );
    }
    assert!(protocol.contains("SecurityReasonCode"));
    assert!(protocol.contains("SECURITY_REASON_SCHEMA"));
    assert!(contracts.contains("kiana.security-reason.v1"));
    for forbidden in [
        "CapabilityBroker",
        "authorize_and_execute",
        "raw_error",
        "provider_error",
        "std::process",
    ] {
        assert!(
            !domain.contains(forbidden),
            "reason contract must not depend on {forbidden}"
        );
    }
}
