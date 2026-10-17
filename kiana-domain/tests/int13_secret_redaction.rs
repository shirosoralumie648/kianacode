use kiana_domain::{
    json_digest, project_redacted_error, redact_text, scan_secret_channels, scan_secret_sentinels,
    scan_secret_value, SecretScanChannel, SecretSentinelKind, MAX_SECRET_SENTINELS,
    MAX_SECRET_SENTINEL_SCAN_BYTES, SCHEMA_CONTRACTS, SECRET_SENTINEL_SCAN_SCHEMA,
};
use serde_json::json;

const TOKEN: &str = "INT13_TOKEN_SENTINEL";
const API_KEY: &str = "INT13_API_KEY_SENTINEL";
const JWT: &str = "eyJpbnQxMyI6InNlbnNpdGl2ZSJ9.eyJzZWNyZXQiOiJpbnQxMyJ9.INT13_SIGNATURE";

fn all_channels() -> [SecretScanChannel; 9] {
    [
        SecretScanChannel::Prompt,
        SecretScanChannel::Transcript,
        SecretScanChannel::Event,
        SecretScanChannel::Receipt,
        SecretScanChannel::Stdout,
        SecretScanChannel::Stderr,
        SecretScanChannel::Argv,
        SecretScanChannel::Env,
        SecretScanChannel::Cache,
    ]
}

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

/// Deny-first matrix: every projection channel refuses every credential shape, and the finding
/// never carries the candidate value back to the caller.
#[test]
fn every_channel_refuses_every_credential_shape_without_retaining_the_candidate() {
    let shapes = [
        (
            "header bearer",
            "INT13_MATRIX_HEADER",
            "Authorization: Bearer INT13_MATRIX_HEADER",
        ),
        (
            "api key",
            "INT13_MATRIX_API_KEY",
            "api_key=INT13_MATRIX_API_KEY",
        ),
        (
            "token",
            "INT13_MATRIX_TOKEN",
            "refresh_token=INT13_MATRIX_TOKEN",
        ),
        (
            "url userinfo",
            "INT13_MATRIX_URL",
            "https://matrix:INT13_MATRIX_URL@example.test/v1",
        ),
        ("jwt", "INT13_SIGNATURE", JWT),
        (
            "sk prefix",
            "INT13_MATRIX_SK",
            "artifact/sk-live-INT13_MATRIX_SK",
        ),
    ];
    for channel in all_channels() {
        for (label, sentinel, text) in shapes {
            let finding = scan_secret_sentinels(channel, text)
                .err()
                .unwrap_or_else(|| panic!("{label} must be refused on {channel:?}"));
            assert_eq!(finding.channel, channel, "{label}");
            assert_eq!(finding.schema, SECRET_SENTINEL_SCAN_SCHEMA, "{label}");
            assert!(finding.marker_digest.starts_with("sha256:"), "{label}");
            let display = finding.to_string();
            let encoded = serde_json::to_string(&finding).expect("finding json");
            assert!(
                !display.contains(sentinel),
                "{label}/{channel:?}: {display}"
            );
            assert!(
                !encoded.contains(sentinel),
                "{label}/{channel:?}: {encoded}"
            );
        }
    }
}

/// Structured and bounded-input deny paths share the same channel set; opaque reference and
/// numeric token-metric payloads stay projectable on every channel.
#[test]
fn structured_secret_keys_nul_bytes_and_token_metrics_bind_to_every_channel() {
    let raw = json!({"credential_lease": {"value": "INT13_STRUCTURED_RAW"}});
    let nul = json!({"note": "value\u{0}"});
    let safe = json!({
        "schema": "kiana.credential-lease.v1",
        "secret_ref": {"store": "keyring", "key": "connector/oauth", "reference_digest": json_digest(&json!("opaque"))},
        "credential_ref": "vault://connector/opaque",
        "token_count": 3,
        "credential_generation": 4,
    });
    for channel in all_channels() {
        let finding = scan_secret_value(channel, &raw).expect_err("structured secret");
        assert_eq!(finding.channel, channel);
        assert_eq!(finding.kind, SecretSentinelKind::CredentialLease);
        assert!(!serde_json::to_string(&finding)
            .expect("finding json")
            .contains("INT13_STRUCTURED_RAW"));
        let bounded = scan_secret_value(channel, &nul).expect_err("NUL input");
        assert_eq!(bounded.kind, SecretSentinelKind::ProviderRawError);
        scan_secret_value(channel, &safe).expect("opaque metadata stays projectable");
    }
}

/// Echo scans are bounded: malformed sentinel sets fail closed before any channel is read, and a
/// real echo is reported through the channel/kind pair without embedding the sentinel.
#[test]
fn echo_sentinel_sets_are_bounded_and_findings_never_embed_the_sentinel() {
    let channels = [(SecretScanChannel::Cache, "safe cache")];
    assert!(scan_secret_channels(&channels, &[""]).is_err());
    let too_many: Vec<&str> = (0..=MAX_SECRET_SENTINELS).map(|_| "s").collect();
    assert!(scan_secret_channels(&channels, &too_many).is_err());
    let overlong = "l".repeat(513);
    assert!(scan_secret_channels(&channels, &[overlong.as_str()]).is_err());

    let echoed = [(SecretScanChannel::Transcript, "echo INT13_ECHO_SENTINEL")];
    let finding =
        scan_secret_channels(&echoed, &["INT13_ECHO_SENTINEL"]).expect_err("echo must fail");
    assert_eq!(finding.kind, SecretSentinelKind::Echo);
    assert_eq!(finding.channel, SecretScanChannel::Transcript);
    assert!(!finding.to_string().contains("INT13_ECHO_SENTINEL"));
    assert!(!serde_json::to_string(&finding)
        .expect("finding json")
        .contains("INT13_ECHO_SENTINEL"));

    let secret_in_channel = [(SecretScanChannel::Env, "CLIENT_SECRET=INT13_ENV_SENTINEL")];
    assert!(scan_secret_channels(&secret_in_channel, &[]).is_err());
}

/// Provider raw errors project to a bounded code/digest object: no error body, URL userinfo,
/// header or JWT survives; identical inputs are stable and different channels do not collide.
#[test]
fn provider_raw_errors_are_projected_to_code_and_digest_only() {
    let raw = format!(
        "provider 500 Authorization: Bearer {TOKEN} for https://user:{API_KEY}@example.test/v1?access_token={API_KEY}\njwt {JWT}"
    );
    let projection = project_redacted_error(SecretScanChannel::Stderr, &raw);
    projection.validate().expect("bounded projection");
    assert_eq!(projection.code, "provider_error_redacted");
    assert!(projection.redacted);
    let encoded = serde_json::to_string(&projection).expect("projection json");
    for sentinel in [TOKEN, API_KEY, JWT, "user:"] {
        assert!(!encoded.contains(sentinel), "leaked {sentinel}: {encoded}");
    }
    let repeated = project_redacted_error(SecretScanChannel::Stderr, &raw);
    assert_eq!(repeated.error_digest, projection.error_digest);
    let other_channel = project_redacted_error(SecretScanChannel::Stdout, &raw);
    assert_ne!(other_channel.error_digest, projection.error_digest);

    let benign = project_redacted_error(SecretScanChannel::Stderr, "connect timeout after 30s");
    benign.validate().expect("bounded projection");
    assert_eq!(benign.code, "provider_error");
    assert!(!benign.redacted);
    assert!(!serde_json::to_string(&benign)
        .expect("projection json")
        .contains("timeout"));

    let oversized = format!("{} {TOKEN}", "x".repeat(MAX_SECRET_SENTINEL_SCAN_BYTES));
    let bounded = project_redacted_error(SecretScanChannel::Event, &oversized);
    bounded.validate().expect("bounded projection");
    assert_eq!(bounded.code, "provider_error_redacted");
    assert!(!serde_json::to_string(&bounded)
        .expect("projection json")
        .contains(TOKEN));
}
