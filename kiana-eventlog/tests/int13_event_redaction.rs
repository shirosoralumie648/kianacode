//! INT-13 event boundary fixtures: the EventStore adapter refuses credential shapes that
//! `validate_secret_free()` alone cannot see, and accepts the already-redacted projection.
//!
//! The production producer path (`kiana-core::prepare_event_payload`) redacts and scans before it
//! hands a payload to the adapter. These fixtures exercise the adapter's own storage boundary, so
//! a non-Core writer cannot persist text shapes that `redact_text()` does not rewrite
//! (`sk-` prefixed keys, a bare `proxy-authorization:` marker, NUL bytes) and already-redacted
//! probes still round-trip byte-stable. They do not prove cross-process durability, do not read
//! external provider logs and do not replace the Core projection/receipt fixtures.

use kiana_domain::{RequestId, RuntimeEvent};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_ports::EventStorePort;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SK_SENTINEL: &str = "sk-int13-event-secret-000000";
const PROXY_SENTINEL: &str = "int13-proxy-credential";

fn event(data: serde_json::Value) -> RuntimeEvent {
    RuntimeEvent::new(RequestId::new(), 1, "run.accepted", data).expect("fixture event")
}

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-int13-eventlog-{label}-{}-{stamp}.jsonl",
        std::process::id()
    ))
}

fn cleanup(path: &PathBuf) {
    let _ = fs::remove_file(path);
    let _ = fs::remove_file(path.with_extension("jsonl.lock"));
}

/// Deny first: the storage boundary rejects credential text that `redact_text()` leaves byte
/// identical, and a denied append creates no journal facts and no file.
#[tokio::test]
async fn event_store_refuses_shapes_that_redaction_alone_cannot_rewrite() {
    let candidates = [
        json!({"note": format!("rotate key {SK_SENTINEL} before the next run")}),
        json!({"note": format!("proxy-authorization: {PROXY_SENTINEL}")}),
        json!({"note": "carriage \u{0} return"}),
        // Sensitive field names keep their existing fail-closed behaviour as well.
        json!({"token": "int13-raw-token"}),
    ];
    let memory = MemoryEventLog::new();
    for candidate in &candidates {
        let error = memory
            .append(event(candidate.clone()))
            .await
            .expect_err("shape must be refused");
        let message = error.to_string();
        assert!(
            message.contains("secret_sentinel_detected") || message.contains("storage_secret"),
            "unexpected denial code for {candidate}: {message}"
        );
        for sentinel in [SK_SENTINEL, PROXY_SENTINEL, "int13-raw-token"] {
            assert!(
                !message.contains(sentinel),
                "denial message leaked {sentinel}: {message}"
            );
        }
    }
    assert!(
        memory.read_all().await.expect("empty store").is_empty(),
        "denied appends must not create facts"
    );

    let path = temp_path("shapes");
    let store = JsonlEventLog::open(&path).expect("open fixture journal");
    for candidate in &candidates {
        assert!(
            store.append(event(candidate.clone())).await.is_err(),
            "jsonl store accepted {candidate}"
        );
    }
    assert!(
        !path.exists(),
        "denied append must not create a journal file"
    );
    cleanup(&path);
}

/// Success path: an already-redacted payload with only opaque references and numeric token metrics
/// is accepted by both adapters and comes back unchanged.
#[tokio::test]
async fn redacted_event_payloads_round_trip_without_retaining_the_candidate() {
    let safe = json!({
        "note": "shell inspect",
        "text": "Authorization: Bearer [REDACTED] token=[REDACTED]",
        "secret_ref": "vault://connector/int13-opaque",
        "tokens": 3,
    });
    let memory = MemoryEventLog::new();
    memory
        .append(event(safe.clone()))
        .await
        .expect("redacted payload is accepted");
    let stored = memory.read_all().await.expect("stored event");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].data, safe);
    let encoded = serde_json::to_string(&stored[0]).expect("event json");
    assert!(encoded.contains("[REDACTED]"), "{encoded}");

    let path = temp_path("roundtrip");
    let store = JsonlEventLog::open(&path).expect("open fixture journal");
    store
        .append(event(safe.clone()))
        .await
        .expect("redacted payload is accepted");
    let stored = store.read_all().await.expect("stored event");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].data, safe);
    cleanup(&path);
}

/// The adapter scan is not a second authority: it only reads the candidate payload, keeps the
/// existing `validate_secret_free()` call in place, and never rewrites stored facts.
#[test]
fn the_storage_boundary_keeps_both_secret_checks_wired() {
    let boundary = include_str!("../src/event_store_core.rs");
    for marker in [
        "validate_secret_free",
        "scan_secret_value_with_limit",
        "SecretScanChannel::Event",
        "MAX_JOURNAL_EVENT_BYTES",
        "eventlog_",
    ] {
        assert!(
            boundary.contains(marker),
            "event storage boundary marker missing: {marker}"
        );
    }
    for forbidden in [
        "async fn execute",
        "write_jsonl",
        "redact_text(",
        "std::net",
        "reqwest",
        "unsafe {",
    ] {
        assert!(
            !boundary.contains(forbidden),
            "storage boundary must not gain a second execution path: {forbidden}"
        );
    }
    let core_events = include_str!("../../kiana-core/src/events.rs");
    for marker in [
        "prepare_event_payload",
        "event_redaction_not_stable",
        "scan_secret_value_with_limit",
        "SecretScanChannel::Event",
    ] {
        assert!(
            core_events.contains(marker),
            "core event producer marker missing: {marker}"
        );
    }
}
