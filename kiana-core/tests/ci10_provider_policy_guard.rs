#[test]
fn provider_policy_probe_and_entrypoint_diagnostics_are_fail_closed() {
    let policy = include_str!("../../kiana-policy/src/provider.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let diagnostics = include_str!("../../kiana-entrypoints/src/provider_diagnostics.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let tui = include_str!("../../kiana-entrypoints/src/tui.rs");

    for marker in [
        "ProviderPolicyBundle",
        "ProviderPolicyRule",
        "PROVIDER_USE_OPERATION",
        "provider_scope_insufficient",
        "provider_credential_missing",
        "provider_credential_expired",
        "provider_reauth_required",
        "provider_credential_revoked",
        "provider_credential_backend_unsupported",
        "provider_credential_unknown",
        "ProviderPolicyEffect::Deny",
    ] {
        assert!(policy.contains(marker), "policy marker missing: {marker}");
    }
    for marker in [
        "ProviderCredentialProbeRequest",
        "ProviderCredentialProbeResponse",
        "ProviderUsePolicyView",
        "PROVIDER_CREDENTIAL_PROBE_REQUEST_SCHEMA",
        "PROVIDER_CREDENTIAL_PROBE_RESPONSE_SCHEMA",
        "PROVIDER_USE_POLICY_VIEW_SCHEMA",
        "deny_unknown_fields",
    ] {
        assert!(
            protocol.contains(marker),
            "protocol marker missing: {marker}"
        );
    }
    for marker in [
        "sanitize_auth_status",
        "sanitize_command_output",
        "key_preview",
        "access_token",
        "refresh_token",
        "presence",
    ] {
        assert!(
            diagnostics.contains(marker),
            "diagnostics marker missing: {marker}"
        );
    }
    assert!(cli.contains("sanitize_command_output(\"auth\""));
    assert!(cli.contains("sanitize_auth_status(&value)"));
    assert!(tui.contains("sanitize_auth_status(&value)"));
    assert!(!diagnostics.contains("raw_secret"));
    assert!(!diagnostics.contains("access_token: String"));
}
