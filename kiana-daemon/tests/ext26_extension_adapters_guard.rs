#[test]
fn daemon_and_broker_keep_adapter_registry_on_existing_authority_spine() {
    let daemon = include_str!("../src/extensions.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    for marker in [
        "build_component_adapter_registry",
        "component_adapters",
        "ExtensionAdapterRegistry",
        "ExtensionAdmission",
        "check_component_adapter",
        "extension_adapter_binding_stale",
        "extension_adapter_descriptor_stale",
    ] {
        assert!(
            daemon.contains(marker) || broker.contains(marker),
            "missing EXT-26 marker: {marker}"
        );
    }
    assert!(!daemon.contains("std::process::Command"));
    assert!(!broker.contains("std::process::Command"));
    assert!(!broker.contains("tokio::process"));
}
