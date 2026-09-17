#[test]
fn security_contracts_stay_domain_only_and_do_not_create_an_execution_path() {
    let domain = include_str!("../../kiana-domain/src/security_contracts.rs");
    let domain_lib = include_str!("../../kiana-domain/src/lib.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "SecuritySchemaRegistry",
        "SecurityObjectEnvelope",
        "upcast_security_object",
        "security_schema_unknown_major",
        "security_object_secret_field",
        "validate_successor",
        "SCHEMA_CONTRACTS",
    ] {
        assert!(domain.contains(marker), "domain marker missing: {marker}");
    }
    assert!(domain_lib.contains("mod security_contracts;"));
    assert!(protocol.contains("SecuritySchemaRegistry"));
    assert!(protocol.contains("SECURITY_OBJECT_SCHEMA"));
    for forbidden in [
        "CapabilityBroker",
        "DaemonHost",
        "tokio",
        "reqwest",
        "std::process",
    ] {
        assert!(
            !domain.contains(forbidden),
            "security domain contract must not depend on {forbidden}"
        );
    }
}
