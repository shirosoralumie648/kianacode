//! CAP-28 source guard for controlled egress and minimum credential injection.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-28 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn egress_and_credentials_are_server_bound_and_deny_first() {
    let network = include_str!("../../kiana-domain/src/network_policy.rs");
    let credentials = include_str!("../../kiana-domain/src/credentials.rs");
    let identity = include_str!("../../kiana-domain/src/identity_contracts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let provider_credentials = include_str!("../../kiana-provider/src/credentials.rs");
    let provider_config = include_str!("../../kiana-provider/src/config.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let oauth = include_str!("../../kiana-provider/src/oauth.rs");
    let daemon = &[
        source_guard::production_source(include_str!("../../kiana-daemon/src/model_client.rs")),
        source_guard::production_source(include_str!("../../kiana-provider/src/lib.rs")),
        source_guard::production_source(include_str!("../../kiana-provider/src/config.rs")),
        source_guard::production_source(include_str!("../../kiana-provider/src/request.rs")),
    ]
    .concat();
    let model_route = source_guard::item_source(
        include_str!("../../kiana-daemon/src/model_client.rs"),
        "pub(crate) fn from_config(",
    );
    assert!(model_route.contains("kiana_provider::ProviderGateway::from_env"));
    assert!(model_route.contains("UnavailableModel::new"));
    let locked_client = source_guard::item_source(
        include_str!("../../kiana-provider/src/config.rs"),
        "fn connection_with_credential_env(",
    );
    let client_code = source_guard::compact_code(&locked_client);
    assert!(client_code.contains("reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).no_proxy().retry(reqwest::retry::never())"));
    let sc14 = include_str!("sc14_network_policy_guard.rs");
    let sc18 = include_str!("sc18_secret_ref_guard.rs");
    let sc19 = include_str!("sc19_secret_rotation_guard.rs");
    let ci07 = &[
        include_str!("ci07_secret_store.rs"),
        include_str!("../../kiana-provider/tests/ci07_secret_store.rs"),
    ]
    .concat();
    let ci09 = include_str!("ci09_oauth_guard.rs");
    let baseline = &[
        include_str!("../../docs/roadmap/cap28-egress-credentials-baseline.md"),
        include_str!("../../docs/roadmap/capability.md"),
    ]
    .concat();

    require(
        network,
        &[
            "NetworkPolicy",
            "NetworkEndpointObservation",
            "allowed_hosts",
            "resolved_addresses",
            "network_endpoint_host_not_allowlisted",
            "network_resolution_local_or_metadata_denied",
            "network_resolution_host_mismatch",
            "require_tls",
        ],
        "network policy",
    );
    require(
        credentials,
        &[
            "CredentialLease",
            "SecretRef",
            "validate_for",
            "credential_lease_provider_mismatch",
            "credential_lease_audience_mismatch",
            "credential_lease_endpoint_mismatch",
            "credential_lease_replayed",
            "pub fn consume",
        ],
        "credential lease",
    );
    require(
        identity,
        &[
            "SecretRef",
            "reference_digest",
            "secret_ref_purpose",
            "secret_ref_audience",
            "generation",
            "contains_raw_secret",
        ],
        "SecretRef",
    );
    require(
        ports,
        &[
            "CredentialResolver",
            "never returns raw secret bytes or strings",
            "CredentialRotationPort",
        ],
        "ports",
    );
    require(
        broker,
        &[
            "validate_network_observation",
            "network_resolution_observation_required",
            "network_policy_digest_mismatch",
            "consume_credential_lease",
        ],
        "broker boundary",
    );
    require(
        provider_credentials,
        &[
            "trait SecretStore",
            "EnvSecretStore",
            "KeyringSecretStore",
            "FileSecretStore",
            "OsSecretStore",
            "impl Drop for SecretMaterial",
            "credential_backend_unsupported",
        ],
        "secret store",
    );
    require(
        provider_config,
        &[
            "credential_ref: Option<SecretRef>",
            "credential_store: std::sync::Arc<dyn SecretStore>",
            "credential_revision",
            "SecretRef::new",
        ],
        "provider config",
    );
    require(
        transport,
        &[
            "credential_store.issue",
            "validate_for",
            ".consume(lease_now)",
            "connection.endpoint",
        ],
        "transport binding",
    );
    require(
        oauth,
        &[
            "pkce_challenge",
            "oauth_state_mismatch",
            "oauth_redirect_mismatch",
            "GenerationConflict",
            "pub(crate) async fn rotate",
            "pub(crate) async fn revoke",
        ],
        "OAuth rotation",
    );
    require(
        daemon,
        &[
            "configuration_snapshot",
            "model_credential_unavailable",
            "credential_revision",
            "opaque_item_cannot_cross_provider",
        ],
        "daemon provider boundary",
    );
    require(
        sc14,
        &[
            "network_resolution_local_or_metadata_denied",
            "network_policy_digest_mismatch",
        ],
        "SC-14 regression",
    );
    require(
        sc18,
        &[
            "CredentialLease",
            "credential_lease_endpoint_mismatch",
            "never returns raw secret bytes or strings",
        ],
        "SC-18 regression",
    );
    require(
        sc19,
        &[
            "oauth_generation_conflict",
            "credential_revision",
            "CredentialRotationPort",
        ],
        "SC-19 regression",
    );
    require(
        ci07,
        &[
            "SecretStore",
            "missing_resolution_fails_closed",
            "secret_value",
        ],
        "CI-07 regression",
    );
    require(
        ci09,
        &[
            "oauth_generation_conflict",
            "OAuthTokenMetadata",
            "redirect",
        ],
        "CI-09 regression",
    );
    require(
        baseline,
        &[
            "direct_connect_and_proxy_env_override_cannot_bypass_egress",
            "dns_rebinding_or_redirect_to_private_address_is_denied",
            "credential_for_one_origin_is_not_forwarded_to_another",
        ],
        "CAP-28 acceptance card",
    );
    for source in [
        network,
        credentials,
        identity,
        ports,
        broker,
        include_str!("../../kiana-daemon/src/model_client.rs"),
    ] {
        assert!(
            !source_guard::production_source(source).contains("reqwest::Client"),
            "CAP-28 unmediated HTTP client outside provider connection"
        );
    }
    for source in [
        network,
        credentials,
        identity,
        ports,
        provider_config,
        transport,
        broker,
        daemon,
    ] {
        let source = source_guard::production_source(source);
        for forbidden in [
            "access_token: String",
            "refresh_token: Option<String>",
            "credential: Option<String>",
            "ToSocketAddrs",
        ] {
            assert!(
                !source.contains(forbidden),
                "CAP-28 forbidden raw/I-O marker: {forbidden}"
            );
        }
    }
}
