//! SC-20 redaction exit-boundary behavior fixture.
//!
//! The card "kiana-domain redaction、Broker/Provider/Runner/Event boundaries" requires a
//! fixture that covers every egress point with deny-first and success evidence. These tests
//! drive the domain redaction contract that the Event, Receipt/Broker, Provider and Runner
//! exits call into, and assert that unresolved secret shapes fail closed instead of
//! returning the raw input. `kiana-core/tests/sc20_redaction_boundary_guard.rs` pins the
//! source-level use of these functions at each boundary; this file proves their behavior.
//!
//! Deny paths named by the card: key 外 secret、Bearer/API key、provider echo、
//! argv/env/stdout/stderr 未脱敏.

use kiana_domain::{
    encode_bounded_text_for, encode_bounded_value_for, project_redacted_error, redact_text,
    redact_value, scan_secret_channels, scan_secret_sentinels, scan_secret_value, RedactionProfile,
    RedactionSignal, SecretScanChannel, StreamingRedactionSink, REDACTED_ERROR_PROJECTION_SCHEMA,
    REDACTION_PROFILE_SCHEMA, SECRET_SENTINEL_SCAN_SCHEMA,
};
use serde_json::json;

/// Deny-first then success for one text publication channel: the raw value is rejected by
/// the sentinel scanner, while the channel-bound encoder publishes only a placeholder.
fn assert_channel_redacts(channel: SecretScanChannel, sentinel: &str, text: &str) {
    assert!(scan_secret_sentinels(channel, text).is_err());
    let profile = RedactionProfile::for_channel(channel);
    let encoded = encode_bounded_text_for(&profile, channel, text).unwrap();
    assert!(!encoded.text.contains(sentinel));
    assert_eq!(encoded.profile_digest, profile.profile_digest);
}

/// Event and Receipt/Broker exits redact keyed credentials and secrets that appear in the
/// text of a benign, non-secret field ("key 外 secret"), and keep opaque references.
#[test]
fn event_and_receipt_exits_redact_keyed_and_unkeyed_secrets() {
    let value = json!({
        "api_key": "sc20-event-key-sentinel",
        "notes": "token=sc20-event-unkeyed-sentinel",
        "nested": [{"authorization": "Bearer sc20-event-bearer-sentinel"}],
        "secret_ref": "vault://provider/anthropic",
    });
    for channel in [SecretScanChannel::Event, SecretScanChannel::Receipt] {
        let profile = RedactionProfile::for_channel(channel);
        let encoded = encode_bounded_value_for(&profile, channel, &value).unwrap();
        let text = encoded.value.to_string();
        for sentinel in [
            "sc20-event-key-sentinel",
            "sc20-event-unkeyed-sentinel",
            "sc20-event-bearer-sentinel",
        ] {
            assert!(!text.contains(sentinel));
        }
        assert!(text.contains("[REDACTED]"));
        assert_eq!(encoded.value["secret_ref"], "vault://provider/anthropic");
        scan_secret_value(channel, &encoded.value).unwrap();
    }
}

/// Unkeyed text secrets are masked even under a field whose name carries no secret marker.
#[test]
fn unkeyed_text_secret_is_masked_under_a_benign_key() {
    let redacted = redact_value(&json!({"notes": "token=sc20-benign-key-secret"}));
    assert_eq!(redacted["notes"], "token=[REDACTED]");
    let rendered = redacted.to_string();
    assert!(!rendered.contains("sc20-benign-key-secret"));
    assert_eq!(redact_text("bearer sc20-plain-bearer tail"), "bearer [REDACTED] tail");
}

/// Provider exit: a raw Provider/transport error is projected into a bounded object that
/// never carries the original text, and echoed sentinels on a channel are rejected.
#[test]
fn provider_exit_projects_errors_without_raw_echo_or_credentials() {
    let raw = "provider failed Authorization: Bearer sc20-provider-bearer";
    let projection = project_redacted_error(SecretScanChannel::Stderr, raw);
    projection.validate().unwrap();
    assert_eq!(projection.schema, REDACTED_ERROR_PROJECTION_SCHEMA);
    assert_eq!(projection.code, "provider_error_redacted");
    assert!(projection.redacted);
    let encoded = serde_json::to_string(&projection).unwrap();
    assert!(!encoded.contains("sc20-provider-bearer"));
    assert!(!encoded.contains("provider failed"));

    let clean = [(SecretScanChannel::Transcript, "provider returned ok")];
    scan_secret_channels(&clean, &["sc20-echo-sentinel"]).unwrap();
    let echoed = [(SecretScanChannel::Transcript, "echo sc20-echo-sentinel")];
    assert!(scan_secret_channels(&echoed, &["sc20-echo-sentinel"]).is_err());
}

/// Runner exit: streamed text is masked across chunk boundaries and the emitted bytes stay
/// bound to the channel redaction profile.
#[test]
fn runner_stream_exit_masks_split_secrets_and_binds_profile() {
    let mut sink = StreamingRedactionSink::for_channel(SecretScanChannel::Transcript);
    let mut streamed = String::new();
    streamed.push_str(&sink.push("token=sc20-stream").unwrap());
    streamed.push_str(&sink.push("-split-value, visible").unwrap());
    let tail = sink.finish().unwrap();
    streamed.push_str(&tail.tail);

    assert_eq!(streamed, "token=[REDACTED], visible");
    assert!(!streamed.contains("sc20-stream"));
    assert_eq!(tail.emitted_bytes, streamed.len());
    let expected = RedactionProfile::for_channel(SecretScanChannel::Transcript);
    assert_eq!(tail.profile_digest, expected.profile_digest);
}

/// argv/env/stdout/stderr exits never publish a raw value: each channel is denied while raw
/// and redacted only through its bound profile.
#[test]
fn argv_env_stdout_stderr_exits_never_publish_raw_secret_values() {
    assert_channel_redacts(
        SecretScanChannel::Stdout,
        "sc20-stdout-pass",
        "https://user:sc20-stdout-pass@example.test/path",
    );
    assert_channel_redacts(
        SecretScanChannel::Stderr,
        "sc20-stderr-jwt",
        "jwt=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzYzIwIn0.sc20-stderr-jwt",
    );
    assert_channel_redacts(
        SecretScanChannel::Argv,
        "sc20-argv-token",
        "--token=sc20-argv-token-value",
    );
    assert_channel_redacts(
        SecretScanChannel::Env,
        "sc20-env-secret",
        "CLIENT_SECRET=sc20-env-secret",
    );
}

/// Failure path: when the redactor cannot resolve a value it returns a stable error code and
/// never falls back to the raw input (fail closed).
#[test]
fn unresolved_secret_shapes_fail_closed_without_returning_raw_input() {
    let profile = RedactionProfile::for_signal(RedactionSignal::Log);
    // A `sk-` prefixed key is not a text marker, so plain redaction cannot clean it; the
    // encoder must reject rather than publish the raw value.
    let sk = "prefix sk-sc20unresolvedkeysuffix";
    assert_eq!(
        encode_bounded_text_for(&profile, SecretScanChannel::Event, sk).unwrap_err(),
        "redaction_secret_sentinel_detected"
    );
    // Malformed JSON-like text that still names a secret key is rejected instead of passed
    // through as prose.
    let malformed = r#"{"token": "sc20-malformed-json",}"#;
    assert_eq!(
        encode_bounded_text_for(&profile, SecretScanChannel::Prompt, malformed).unwrap_err(),
        "redaction_secret_sentinel_detected"
    );
    // NUL bytes are refused before any redaction is attempted.
    assert_eq!(
        encode_bounded_text_for(&profile, SecretScanChannel::Prompt, "nul\0here").unwrap_err(),
        "redaction_nul_forbidden"
    );
}

/// The redaction schemas are versioned and closed, so a profile with an injected field is
/// refused on deserialization.
#[test]
fn redaction_schemas_are_versioned_and_closed() {
    assert_eq!(REDACTION_PROFILE_SCHEMA, "kiana.redaction-profile.v1");
    assert_eq!(SECRET_SENTINEL_SCAN_SCHEMA, "kiana.secret-sentinel-scan.v1");
    assert_eq!(REDACTED_ERROR_PROJECTION_SCHEMA, "kiana.redacted-error-projection.v1");
    let profile = RedactionProfile::for_signal(RedactionSignal::Audit);
    profile.validate().unwrap();
    let encoded = serde_json::to_string(&profile).unwrap();
    let forged = encoded.replace('}', ",\"untrusted\":true}");
    assert!(serde_json::from_str::<RedactionProfile>(&forged).is_err());
}
