#[test]
fn signed_skill_scopes_are_server_bound_and_rechecked_before_dispatch() {
    let domain = include_str!("../../kiana-domain/src/extensions.rs");
    let daemon = include_str!("../../kiana-daemon/src/extensions.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_skills.rs");
    let runner_tools = include_str!("../../kiana-runner/src/tools.rs");
    let fixture = include_str!("../../kiana-domain/tests/ext25_signed_skill_scope.rs");

    for marker in [
        "pub struct ExtensionExecutionScope",
        "registry_generation",
        "capability_diff_digest",
        "network_policy_digest",
        "secret_policy_digest",
        "expires_at_unix_ms",
        "validate_at",
        "matches_manifest",
        "extension_scope_generation_stale",
        "extension_scope_policy_digest_mismatch",
        "request_extension_scopes",
    ] {
        assert!(
            domain.contains(marker),
            "missing EXT-25 domain marker: {marker}"
        );
    }
    for marker in [
        "skill_context",
        "fold_registry",
        "ExtensionExecutionScope::new",
        "extension_scope_expiry_overflow",
        "contract.validate_at",
        "contract.matches_manifest",
        "extension_execution_snapshot_inactive",
    ] {
        assert!(
            daemon.contains(marker),
            "missing EXT-25 daemon marker: {marker}"
        );
    }
    for marker in [
        "admit_extensions",
        "request_extension_scopes",
        "ExtensionAdmission",
        "contract.check",
    ] {
        assert!(
            broker.contains(marker),
            "missing EXT-25 broker marker: {marker}"
        );
    }
    assert!(harness.contains("bundle.extensions"));
    assert!(harness.contains("skill_context"));
    assert!(!runner_tools.contains("_extension_scopes"));
    assert!(fixture.contains("stale registry generation"));
    assert!(fixture.contains("role drift"));
}
