#[test]
fn sc26_domain_catalog_keeps_manifest_digest_intersection_and_no_execution() {
    let source = include_str!("../src/capability_catalog.rs");
    for marker in [
        "extension_manifest_digest",
        "requested_capabilities",
        "effective_capabilities",
        "intersection(available)",
        "extension_read_only_write_denied",
        "capability_effective_set_widened",
        "provided_capabilities",
    ] {
        assert!(source.contains(marker), "missing SC-26 marker: {marker}");
    }
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("execute("));
}
