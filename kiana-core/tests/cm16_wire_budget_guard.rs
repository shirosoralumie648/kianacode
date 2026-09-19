#[test]
fn wire_budget_and_cache_binding_stay_in_domain_contracts() {
    let source = include_str!("../../kiana-domain/src/request_budget.rs");
    for marker in [
        "ExactTokenizer",
        "ConservativeUtf8",
        "from_final_wire",
        "provider_framing_bytes",
        "output_reserved_tokens",
        "configuration_revision",
        "authority_epoch",
        "cache_hit_allowed",
        "stable_prefix_authority_revoked",
        "wire_budget_exceeded",
    ] {
        assert!(
            source.contains(marker),
            "CM-16 source marker missing: {marker}"
        );
    }
    assert!(!source.contains("ModelClient"));
    assert!(!source.contains("CapabilityBroker"));
}
