#[test]
fn sc14_network_boundary_is_server_owned_and_deny_first() {
    let policy = include_str!("../../kiana-domain/src/network_policy.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let legacy_guard = include_str!("../../kiana-services/src/network_policy.rs");

    for marker in [
        "NetworkPolicy",
        "NetworkEndpointObservation",
        "resolved_addresses",
        "network_endpoint_host_not_allowlisted",
        "network_resolution_local_or_metadata_denied",
        "network_resolution_host_mismatch",
        "NETWORK_ENDPOINT_OBSERVATION_SCHEMA",
    ] {
        assert!(
            policy.contains(marker),
            "SC-14 domain marker missing: {marker}"
        );
    }
    for marker in [
        "validate_network_observation",
        "network_resolution_observation_required",
        "network_policy_digest_mismatch",
        "request.capability != CapabilityKind::Network",
        "ExecutionScope",
    ] {
        assert!(
            broker.contains(marker),
            "SC-14 broker marker missing: {marker}"
        );
    }
    for marker in [
        "sandbox_unsupported",
        "--unshare-all",
        "sandbox_external_symlink_requires_staging",
        "sandbox_env",
    ] {
        assert!(
            sandbox.contains(marker),
            "SC-14 sandbox marker missing: {marker}"
        );
    }
    for marker in [
        "validate_http_url",
        "same_origin_redirects_only",
        "169.254.169.254",
        "is_private_ipv4",
    ] {
        assert!(
            legacy_guard.contains(marker),
            "SC-14 existing network guard marker missing: {marker}"
        );
    }
    for source in [policy, broker, sandbox] {
        for forbidden in ["reqwest::Client", "std::net::ToSocketAddrs"] {
            assert!(
                !source.contains(forbidden),
                "SC-14 pure/broker boundary performs hidden network I/O: {forbidden}"
            );
        }
    }
}
