#[test]
fn sc26_domain_catalog_keeps_manifest_digest_intersection_and_no_execution() {
    let source = include_str!("../src/capability_catalog.rs");
    for marker in [
        "extension_manifest_digest",
        "provided_capabilities",
        "requested_capabilities",
        "effective_capabilities",
        "intersection(available)",
        "entry_for_manifest",
        "authorize_manifest",
        "check_manifest_drift",
        "pub fn authorize(",
        "capability_catalog_extension_unknown",
        "capability_catalog_extension_identity_drift",
        "capability_catalog_manifest_version_drift",
        "capability_catalog_manifest_capability_drift",
        "capability_catalog_manifest_digest_drift",
        "capability_catalog_entry_denied",
        "capability_not_effective",
        "capability_catalog_request_invalid",
        "extension_read_only_write_denied",
        "capability_effective_set_widened",
        "capability_catalog_entry_digest_mismatch",
        "capability_catalog_digest_mismatch",
        "capability_catalog_extension_duplicate",
        "capability_catalog_header_invalid",
        "capability_catalog_allow_reason_invalid",
        "capability_catalog_deny_reason_invalid",
        "deny_unknown_fields",
    ] {
        assert!(source.contains(marker), "missing SC-26 marker: {marker}");
    }
    // Descriptive declarations stay out of the grant path: the catalog never reads
    // skill `allowed_tools`, never touches the filesystem, never calls Broker and never
    // executes anything.
    assert!(!source.contains("allowed_tools"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("CapabilityBroker"));
    assert!(!source.contains("execute("));
}

#[test]
fn sc26_deny_first_fixtures_are_wired_into_the_catalog_lane() {
    let fixtures = include_str!("sc26_capability_catalog.rs");
    let lane = include_str!("../../.github/workflows/sc26-capability-catalog.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    for fixture in [
        "read_only_manifest_cannot_request_write_and_provided_tools_do_not_authorize",
        "allowed_tools_declarations_are_descriptive_and_never_enter_the_grant_set",
        "version_capability_and_digest_drift_cannot_reuse_a_catalog_entry",
        "denied_or_unrequested_capabilities_never_authorize",
        "duplicate_identity_and_invalid_catalog_shapes_are_rejected",
        "widened_or_tampered_catalog_entry_is_rejected",
        "catalog_binds_manifest_digest_and_intersects_all_authority_sets",
    ] {
        assert!(
            fixtures.contains(&format!("fn {fixture}(")),
            "missing SC-26 fixture: {fixture}"
        );
        assert!(
            lane.contains(fixture),
            "SC-26 lane does not run fixture: {fixture}"
        );
    }
    assert!(lane.contains(
        "cargo test -p kiana-domain --test sc26_capability_catalog --locked -- --test-threads=1"
    ));
    assert!(lane.contains(
        "cargo test -p kiana-domain --test sc26_capability_catalog_guard --locked -- --test-threads=1"
    ));
    assert!(lane.contains("\"$fixture\" --locked -- --exact --test-threads=1"));
    assert!(lane.contains("--no-run"));
    assert!(!lane.contains("continue-on-error"));
    assert!(!fixtures.contains("#[ignore]"));
    for target in ["sc26_capability_catalog", "sc26_capability_catalog_guard"] {
        assert!(
            shards.contains(target),
            "SC-26 target is not registered in test shards: {target}"
        );
    }
}
