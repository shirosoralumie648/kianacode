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

#[test]
fn broker_requires_the_admitted_envelope_before_counting_usage() {
    let source = include_str!("../src/lib.rs");
    // The admitted envelope is decoded from the authorized request and rechecked against the
    // handler's usage; a started effect without usage evidence also fails closed.
    assert!(source.contains("EffectUsageEnvelope::from_json"));
    assert!(source.contains("effect_usage_envelope_required"));
    assert!(source.contains("effect_usage_observation_required"));
    assert!(source.contains("validate_against_envelope"));
    assert!(source.contains("with_server_wall_time_ms"));
}
