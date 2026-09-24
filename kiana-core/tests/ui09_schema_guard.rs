#[test]
fn ui09_schema_gate_is_data_only_and_generated_catalog_is_bound() {
    let validator = include_str!("../../scripts/validate-ui09-schemas.py");
    let lock = include_str!("../../docs/schemas/ui/schema-lock.v1.json");
    let matrix = include_str!("../../docs/schemas/ui/compatibility-matrix.v1.json");
    let catalog = include_str!("../../kiana-client/src/ui_schema_generated.rs");
    for marker in [
        "schema-lock.v1",
        "generated_rust",
        "schema_sha256",
        "unknown_field_example",
        "deprecated_fields",
        "max_bytes",
        "same_version_policy",
        "unknown_major_policy",
        "reject_before_decode",
        "additive_only",
        "UI_SCHEMA_CONTRACTS",
    ] {
        assert!(
            validator.contains(marker)
                || lock.contains(marker)
                || matrix.contains(marker)
                || catalog.contains(marker),
            "UI-09 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane::new",
        "DaemonHost",
        "tokio::spawn",
        "ModelClient",
        "std::fs::",
        "reqwest",
    ] {
        assert!(
            !validator.contains(forbidden),
            "schema gate must not execute {forbidden}"
        );
        assert!(
            !catalog.contains(forbidden),
            "generated catalog must not execute {forbidden}"
        );
    }
}
