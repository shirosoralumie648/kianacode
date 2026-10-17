#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::{compact_code, item_source, production_source};

#[test]
fn sc18_secret_ref_store_and_egress_boundary_is_opaque_and_deny_first() {
    let identity = include_str!("../../kiana-domain/src/identity_contracts.rs");
    let credentials = include_str!("../../kiana-domain/src/credentials.rs");
    let redaction = include_str!("../../kiana-domain/src/redaction.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let provider_credentials = include_str!("../../kiana-provider/src/credentials.rs");
    let provider_request = include_str!("../../kiana-provider/src/request.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let provider_config = include_str!("../../kiana-provider/src/config.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let broker = include_str!("../../kiana-capability-broker/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/model_client.rs");
    let provider_fixture = include_str!("../../kiana-provider/tests/ci07_secret_store.rs");
    let lease_fixture = include_str!("../../kiana-domain/tests/ci07_credential_lease.rs");
    let secret_ref_fixture = include_str!("../../kiana-domain/tests/sc18_secret_ref.rs");
    let secret_store_fixture = include_str!("../../kiana-ports/tests/sc18_secret_store.rs");

    for marker in [
        "pub struct SecretRef",
        "reference_digest",
        "secret_ref_purpose",
        "secret_ref_audience",
        "generation",
        "deny_unknown_fields",
        "contains_raw_secret",
    ] {
        assert!(
            identity.contains(marker),
            "SC-18 SecretRef marker missing: {marker}"
        );
    }
    for marker in [
        "pub struct CredentialLease",
        "validate_for",
        "credential_lease_provider_mismatch",
        "credential_lease_purpose_mismatch",
        "credential_lease_audience_mismatch",
        "credential_lease_endpoint_mismatch",
        "credential_lease_replayed",
        "pub fn consume",
    ] {
        assert!(
            credentials.contains(marker),
            "SC-18 lease marker missing: {marker}"
        );
    }
    for marker in [
        "redact_value",
        "redact_text",
        "redaction_secret_sentinel_detected",
        "Bearer",
        "api_key",
    ] {
        assert!(
            redaction.contains(marker),
            "SC-18 redaction marker missing: {marker}"
        );
    }
    for marker in [
        "CredentialResolver",
        "never returns raw secret bytes or strings",
        "CredentialResolution",
        "resolve_credential",
        "pub trait SecretStore",
        "pub struct SecretStoreBinding",
        "SECRET_STORE_BINDING_SCHEMA",
        "fn store_name",
        "resolve_secret_checked",
        "secret_store_unknown",
        "secret_store_binding_mismatch",
    ] {
        assert!(
            ports.contains(marker),
            "SC-18 port marker missing: {marker}"
        );
    }
    for marker in [
        "trait SecretStore",
        "EnvSecretStore",
        "InlineSecretStore",
        "KeyringSecretStore",
        "FileSecretStore",
        "OsSecretStore",
        "impl Drop for SecretMaterial",
        "credential_backend_unsupported",
    ] {
        assert!(
            provider_credentials.contains(marker),
            "SC-18 provider store marker missing: {marker}"
        );
    }
    for marker in [
        "credential_ref: Option<SecretRef>",
        "credential_store: std::sync::Arc<dyn SecretStore>",
        "credential_revision",
        "SecretRef::new",
        "credential_store.issue",
    ] {
        assert!(
            provider_config.contains(marker) || transport.contains(marker),
            "SC-18 provider config marker missing: {marker}"
        );
    }
    for marker in [
        "credential_store.issue",
        "validate_for",
        ".consume(lease_now)",
        "connection.endpoint",
    ] {
        assert!(
            transport.contains(marker),
            "SC-18 transport marker missing: {marker}"
        );
    }
    for marker in [
        "consume_credential_lease",
        "credential_lease_invalid",
        "CredentialLease",
    ] {
        assert!(
            if marker == "credential_lease_invalid" {
                let consume =
                    compact_code(&item_source(broker, "pub fn consume_credential_lease("));
                consume.contains(
                    ".validate_for(now_unix_ms,provider_account,purpose,audience,endpoint_digest,)",
                ) && consume.contains(".map_err(PortError::Failed)?")
                    && consume.contains("lease.consume(now_unix_ms).map_err(PortError::Failed)")
            } else {
                broker.contains(marker)
            },
            "SC-18 broker marker missing: {marker}"
        );
    }
    for marker in [
        "configuration_snapshot",
        "model_credential_unavailable",
        "credential_revision",
        "opaque_item_cannot_cross_provider",
    ] {
        assert!(
            match marker {
                "configuration_snapshot" =>
                    item_source(provider, "pub fn configuration_snapshot(").contains("snapshot("),
                "model_credential_unavailable" =>
                    production_source(provider_config).contains("model_credential_unavailable"),
                "credential_revision" =>
                    item_source(provider_config, "pub(crate) struct Connection")
                        .contains("pub credential_revision: String"),
                "opaque_item_cannot_cross_provider" => {
                    let normalize = compact_code(&item_source(
                        provider_request,
                        "fn normalize_structured_request(",
                    ));
                    normalize.contains("provider_id!=route.provider_id||protocol!=route.protocol||route_digest!=expected_route") && normalize.contains("opaque_item_cannot_cross_provider")
                }
                _ => production_source(daemon).contains(marker),
            },
            "SC-18 daemon marker missing: {marker}"
        );
    }
    assert!(
        compact_code(&item_source(daemon, "pub(crate) fn from_config("))
            .contains("kiana_provider::ProviderGateway::from_env(")
    );
    for source in [
        identity,
        credentials,
        ports,
        provider_config,
        transport,
        broker,
        daemon,
    ] {
        for forbidden in [
            "pub secret_value",
            "pub secret: String",
            "credential: Option<String>",
            "token_passthrough",
            "credential_passthrough",
        ] {
            assert!(
                !production_source(source).contains(forbidden),
                "SC-18 raw secret/passthrough marker: {forbidden}"
            );
        }
    }
    assert!(lease_fixture.contains("lease_json_has_no_secret_slot"));
    assert!(provider_fixture.contains("missing_resolution_fails_closed"));
    assert!(secret_ref_fixture.contains("not_transferable_across_account_purpose"));
    assert!(secret_ref_fixture.contains("wire_log_event_payload_carries_no_secret_value"));
    assert!(secret_store_fixture.contains("unknown_provider_account_or_purpose"));
    assert!(secret_store_fixture.contains("not_transferable_across_purpose_or_audience"));
    assert!(secret_store_fixture.contains("wire_log_event_payload_carries_no_secret_value"));
}
