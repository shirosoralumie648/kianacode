use kiana_domain::{
    json_digest, project_redacted_error, redact_text, scan_secret_channels, scan_secret_sentinels,
    scan_secret_value, SecretScanChannel, SCHEMA_CONTRACTS,
};
use serde_json::json;

const TOKEN: &str = "INT13_TOKEN_SENTINEL";
const API_KEY: &str = "INT13_API_KEY_SENTINEL";
const JWT: &str = "eyJpbnQxMyI6InNlbnNpdGl2ZSJ9.eyJzZWNyZXQiOiJpbnQxMyJ9.INT13_SIGNATURE";

#[test]
fn all_projection_channels_reject_secret_shapes() {
    assert!(SCHEMA_CONTRACTS.iter().any(|contract| {
        contract.name == "kiana.secret-sentinel-scan.v1" && !contract.allow_unknown_fields
    }));
    assert!(SCHEMA_CONTRACTS.iter().any(|contract| {
        contract.name == "kiana.redacted-error-projection.v1" && !contract.allow_unknown_fields
    }));
    for (channel, value) in [
        (
            SecretScanChannel::Prompt,
            "access_token=INT13_TOKEN_SENTINEL",
        ),
        (
            SecretScanChannel::Transcript,
            "Authorization: Bearer INT13_BEARER",
        ),
        (SecretScanChannel::Event, "api_key=INT13_API_KEY_SENTINEL"),
        (
            SecretScanChannel::Event,
            r#"{"authorization":"Bearer INT13_JSON_HEADER"}"#,
        ),
        (SecretScanChannel::Receipt, "x-api-key: INT13_HEADER"),
        (
            SecretScanChannel::Stdout,
            "https://user:INT13_PASSWORD@example.test/path",
        ),
        (SecretScanChannel::Stderr, JWT),
        (SecretScanChannel::Argv, "--token=INT13_ARGV"),
        (SecretScanChannel::Env, "CLIENT_SECRET=INT13_ENV"),
        (SecretScanChannel::Cache, "refresh_token=INT13_CACHE"),
    ] {
        assert!(
            scan_secret_sentinels(channel, value).is_err(),
            "{channel:?}"
        );
    }
}

#[test]
fn redaction_masks_url_userinfo_and_jwt_without_leaking_input() {
    let text = format!("GET https://user:{TOKEN}@example.test/?access_token={API_KEY} jwt={JWT}");
    let redacted = redact_text(&text);
    for sentinel in [TOKEN, API_KEY, JWT, "user:"] {
        assert!(
            !redacted.contains(sentinel),
            "leaked {sentinel}: {redacted}"
        );
    }
    assert!(scan_secret_sentinels(SecretScanChannel::Transcript, &redacted).is_ok());
}

#[test]
fn credential_lease_projection_keeps_only_opaque_reference_and_metrics() {
    let safe = json!({
        "schema": "kiana.credential-lease.v1",
        "secret_ref": {"store": "keyring", "key": "connector/oauth", "reference_digest": json_digest(&json!("opaque"))},
        "credential_ref": "vault://connector/opaque",
        "token_count": 3,
        "credential_generation": 4,
    });
    scan_secret_value(SecretScanChannel::Event, &safe).expect("opaque metadata is safe");

    let raw = json!({"credential_lease": {"value": "INT13_LEASE_RAW"}});
    assert!(scan_secret_value(SecretScanChannel::Event, &raw).is_err());
}

#[test]
fn echo_sentinel_scan_covers_fixture_channels_and_provider_error_projection_is_bounded() {
    let channels = [
        (SecretScanChannel::Prompt, "safe prompt"),
        (SecretScanChannel::Transcript, "safe transcript"),
        (SecretScanChannel::Event, "safe event"),
        (SecretScanChannel::Receipt, "safe receipt"),
        (SecretScanChannel::Stdout, "safe stdout"),
        (SecretScanChannel::Stderr, "safe stderr"),
        (SecretScanChannel::Argv, "safe argv"),
        (SecretScanChannel::Env, "safe env"),
        (SecretScanChannel::Cache, "safe cache"),
    ];
    scan_secret_channels(&channels, &["INT13_ECHO_SENTINEL"]).expect("fixture is clean");
    let mut echoed = channels.to_vec();
    echoed[4].1 = "provider returned INT13_ECHO_SENTINEL";
    assert!(scan_secret_channels(&echoed, &["INT13_ECHO_SENTINEL"]).is_err());

    let projection = project_redacted_error(
        SecretScanChannel::Stderr,
        "provider failed Authorization: Bearer INT13_RAW https://u:p@example.test",
    );
    projection.validate().expect("bounded projection");
    assert_eq!(projection.code, "provider_error_redacted");
    let encoded = serde_json::to_string(&projection).expect("projection json");
    assert!(!encoded.contains("INT13_RAW"));
    assert!(!encoded.contains("u:p@"));
    assert!(projection.redacted);
}
