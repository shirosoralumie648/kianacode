#[test]
fn broker_checks_usage_after_handler_and_keeps_server_owned_binding() {
    let source = include_str!("../src/lib.rs");
    assert!(source.contains("validate_effect_usage_result"));
    assert!(source.contains("effect_usage_lease_required"));
    assert!(source.contains("effect_usage_resource_required"));
    assert!(source.contains("validate_binding"));
    assert!(source.contains("Instant::now"));
    assert!(!source.contains("ModelOutput"));
    assert!(!source.contains("reqwest"));
}
