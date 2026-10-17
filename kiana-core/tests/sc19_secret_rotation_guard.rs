#[test]
fn sc19_secret_rotation_and_revocation_are_generation_fenced() {
    let domain = include_str!("../../kiana-domain/src/oauth.rs");
    let credentials = include_str!("../../kiana-provider/src/credentials.rs");
    let transport = include_str!("../../kiana-provider/src/transport.rs");
    let oauth = include_str!("../../kiana-provider/src/oauth.rs");
    let provider = include_str!("../../kiana-provider/src/lib.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let leases = include_str!("../src/resource_leases.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let daemon_leases = include_str!("../../kiana-daemon/src/secret_leases.rs");
    let daemon_fixture = include_str!("../../kiana-daemon/tests/sc19_secret_lease.rs");
    let fixture = include_str!("../../kiana-domain/tests/ci09_oauth_contracts.rs");
    let guard = include_str!("ci09_oauth_guard.rs");

    for marker in [
        "OAuthTokenMetadata",
        "pub fn rotate",
        "pub fn revoke",
        "oauth_generation_conflict",
        "oauth_token_status_fenced",
        "generation",
        "needs_refresh",
        "deny_unknown_fields",
    ] {
        assert!(
            domain.contains(marker),
            "SC-19 domain marker missing: {marker}"
        );
    }
    for marker in [
        "current_revision",
        "CredentialLease::issue",
        "impl Drop for SecretMaterial",
        "credential_lease_replayed",
        "credential_revision",
    ] {
        assert!(
            credentials.contains(marker),
            "SC-19 credential store marker missing: {marker}"
        );
    }
    for marker in [
        "credential_store.issue",
        "credential_revision",
        "credential_lease_reference_mismatch",
        ".consume(lease_now)",
        "model_credential_revision_changed",
    ] {
        assert!(
            transport.contains(marker),
            "SC-19 transport marker missing: {marker}"
        );
    }
    for marker in [
        "refreshing: bool",
        "refresh_notify",
        "GenerationConflict",
        "oauth_generation_conflict",
        "pub(crate) async fn rotate",
        "pub(crate) async fn revoke",
        "refresh_cooldown_until",
        "sync_all",
        "rename",
        "0o600",
    ] {
        assert!(
            oauth.contains(marker),
            "SC-19 OAuth provider marker missing: {marker}"
        );
    }
    assert!(provider.contains("mod oauth;"));
    for marker in [
        "CredentialRotationPort",
        "observed_generation",
        "rotate_credential",
        "revoke_credential",
        "never returns raw secret bytes or strings",
    ] {
        assert!(
            ports.contains(marker),
            "SC-19 port marker missing: {marker}"
        );
    }
    for marker in [
        "issue_resource_lease",
        "validate_resource_lease",
        "validate_current",
        "authority_epoch",
        "session_id",
    ] {
        assert!(
            leases.contains(marker),
            "SC-19 lease fence marker missing: {marker}"
        );
    }
    for marker in [
        "validate_protected_ingress",
        "credential_ref",
        "ingress_protected_credentials_required",
    ] {
        assert!(
            daemon.contains(marker),
            "SC-19 daemon ingress marker missing: {marker}"
        );
    }
    for source in [domain, credentials, transport, ports, leases, daemon] {
        for forbidden in ["access_token: String", "refresh_token: Option<String>"] {
            assert!(
                !source.contains(forbidden),
                "SC-19 raw token boundary marker: {forbidden}"
            );
        }
    }
    for marker in [
        "SecretLeaseAuthority",
        "pub fn issue_lease",
        "pub fn consume_lease",
        "pub fn rotate",
        "pub fn revoke",
        "pub fn export_journal",
        "pub fn restore_from_journal",
        "secret_lease_revoked",
        "secret_lease_generation_conflict",
        "secret_lease_generation_rollback",
        "secret_lease_restore_plaintext_rejected",
        "SECRET_ROTATION_RECEIPT_SCHEMA",
        "SECRET_REVOCATION_RECEIPT_SCHEMA",
        "SECRET_LEASE_JOURNAL_SCHEMA",
        "deny_unknown_fields",
    ] {
        assert!(
            daemon_leases.contains(marker),
            "SC-19 daemon lease marker missing: {marker}"
        );
    }
    for marker in [
        "expired_lease_is_denied_before_injection",
        "one_shot_lease_cannot_be_replayed",
        "revoked_slot_denies_further_lease_consume_and_rotate",
        "rotation_receipt_is_generation_fenced",
        "restore_rejects_plaintext_and_round_trips_secret_free",
        "secret_lease_restore_plaintext_rejected",
    ] {
        assert!(
            daemon_fixture.contains(marker),
            "SC-19 daemon fixture marker missing: {marker}"
        );
    }
    for source in [daemon_leases, daemon_fixture] {
        for forbidden in [
            "secret_value: String",
            "access_token: String",
            "refresh_token: Option<String>",
        ] {
            assert!(
                !source.contains(forbidden),
                "SC-19 daemon raw secret boundary marker: {forbidden}"
            );
        }
    }
    assert!(fixture.contains("token_metadata_uses_generation_cas_expiry_status"));
    assert!(guard.contains("GenerationConflict"));
}
