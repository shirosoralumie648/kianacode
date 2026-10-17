//! OA-00 source-only observability and audit inventory guard.
//!
//! The guard intentionally reads product sources instead of starting a daemon or exercising a
//! runtime. GitHub Actions owns runtime evidence; later OA steps must replace the absence checks
//! as the versioned contracts are introduced.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn read_source(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative))
        .unwrap_or_else(|error| panic!("read {relative}: {error}"))
}

fn sha256_of(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    format!("{:x}", Sha256::digest(&bytes))
}

const BASELINE: &[(&str, &str)] = &[
    (
        "kiana-domain/src/states.rs",
        "c3efa271aa3b70c0a0dfd1637209ed7e0246b2fc6f28b914d4f7832d4724ad8b",
    ),
    (
        "kiana-domain/src/journal.rs",
        "6966da5534b7bb9155b3282e5338aa5195f362aa6a08ab6564a7a8ddfe24324e",
    ),
    (
        "kiana-domain/src/contracts.rs",
        "171ceaf338c1af81512f43a98ceb5b4b6ab67893646d8e0e4d9132fc3dd9e865",
    ),
    (
        "kiana-domain/src/lib.rs",
        "3cf06f5b893a21ee082b2c236de01bf653b174c644c3b9bd4639e3182b3d9eb1",
    ),
    (
        "kiana-core/src/events.rs",
        "1b9100c8f05ecb0d283a1c48980f712fa65d299fb02b6f667fa1259db6d8f782",
    ),
    (
        "kiana-core/src/receipts.rs",
        "3116e1a8f6dfb47c04d2e2716f89c62f7b4a1b74dd56db8c932a417d635cb1fa",
    ),
    (
        "kiana-core/src/projection.rs",
        "a62284905d9ae317d79d4b7ab44b31b0946fc3287f9a4126f7310e67e0e9e72c",
    ),
    (
        "kiana-core/src/recovery.rs",
        "b12dbb0fdddde7bbd9e69088dc5565f06e385a231ecc9993c63deefff053e402",
    ),
    (
        "kiana-core/src/lifecycle.rs",
        "949f618edd85ed818a8f51ee1b2bff6498540b85f19b82e7a77816167b4d6d6b",
    ),
    (
        "kiana-core/src/versioning.rs",
        "b0777088127560167e4452b0e59b83091d6904d2349b0b65723b6dc230996bf6",
    ),
    (
        "kiana-core/src/collaboration.rs",
        "22bfc2df7cbc267e5db4fe6e13bea72f5aa13f3145d913bd1b42896aecaa5a4e",
    ),
    (
        "kiana-eventlog/src/lib.rs",
        "a39842dead47f80e0bb9539a79ccffb0ea13ba65402512aaa86d779a3d337934",
    ),
    (
        "kiana-eventlog/src/event_store_core.rs",
        "bd46b5080bb29f1a19af441f4f6e5756991d0afd7a0b60535134231fea4dc9a8",
    ),
    (
        "kiana-eventlog/src/journal_core.rs",
        "2ea223f60e1edd73e090b4d7c117a8d69579cb277657edf03c7063d000194ead",
    ),
    (
        "kiana-eventlog/src/jsonl.rs",
        "2b3370eadf392086a8d57ddb66be2be92c4e0aaaa89412657b7e6f6e3e577322",
    ),
    (
        "kiana-eventlog/src/memory.rs",
        "d77345e3327ba7a58294f973289ecdb667926edcd6fed7509238f10ac458835d",
    ),
    (
        "kiana-daemon/src/run_stream.rs",
        "42a28b1870b9a03b192fc2cf162a389516a37184c1290267d4e323dad6770c70",
    ),
    (
        "kiana-daemon/src/lib.rs",
        "f6d8ac42958c18744af1fe823329a8e49efbb8488942d50a25dfe62e5ab36246",
    ),
    (
        "kiana-ports/src/lib.rs",
        "b1a5f7502a83eccc5192629c502b517864095b8bb95a3c9cdbe98eb71e8a2e46",
    ),
];

#[test]
fn observability_source_baseline_is_reproducible() {
    let root = workspace_root();
    for (relative, expected) in BASELINE {
        let actual = sha256_of(&root.join(relative));
        assert_eq!(
            actual, *expected,
            "{relative} drifted from the OA-00 source snapshot; update the inventory and this table together"
        );
    }
}

#[test]
fn observability_inventory_preserves_fact_projection_and_open_contracts() {
    let root = workspace_root();
    let states = read_source(&root, "kiana-domain/src/states.rs");
    let events = read_source(&root, "kiana-core/src/events.rs");
    let receipts = read_source(&root, "kiana-core/src/receipts.rs");
    let journal = read_source(&root, "kiana-domain/src/journal.rs");
    let jsonl = read_source(&root, "kiana-eventlog/src/jsonl.rs");
    let run_stream = read_source(&root, "kiana-daemon/src/run_stream.rs");
    let versioning = read_source(&root, "kiana-core/src/versioning.rs");
    let lifecycle = read_source(&root, "kiana-core/src/lifecycle.rs");
    let collaboration = read_source(&root, "kiana-core/src/collaboration.rs");
    let ports = read_source(&root, "kiana-ports/src/lib.rs");
    let baseline = read_source(&root, "docs/roadmap/observability-audit-baseline.md");

    assert!(states.contains("pub kind: String"));
    assert!(states.contains("pub data: Value"));
    let redacted = events
        .find("prepare_event_payload(&data)")
        .expect("central event redaction");
    let constructed = events
        .find("RuntimeEvent::new(request_id, sequence, kind, data.clone())")
        .expect("event construction");
    assert!(
        redacted < constructed,
        "facts must be redacted before construction"
    );
    assert!(receipts.contains("has_result_unknown"));
    assert!(receipts.contains("terminal_count > 1"));
    assert!(receipts.contains("redact_event_value(&receipt)"));
    assert!(journal.contains("pub struct CommandReceipt"));
    assert!(journal.contains("pub command_digest: String"));
    assert!(jsonl.contains("CommitOutcome::Unknown"));
    assert!(jsonl.contains("eventlog_worker_queue_full"));
    assert!(run_stream.contains("Disposable, bounded projections"));
    assert!(run_stream.contains("CommitOutcome::Committed"));
    assert!(run_stream.contains("if !result.replayed"));
    assert!(versioning.contains("golden_trace.captured"));
    assert!(versioning.contains("\"side_effects\":false"));
    assert!(versioning.contains("\"provider_calls\":0"));
    assert!(lifecycle.contains("\"run.receipt\""));
    assert!(collaboration.contains("files.is_empty()"));
    assert!(ports.contains("实现了端口"));

    for marker in [
        "EventLog",
        "RuntimeEvent",
        "P1-J8-01",
        "ER-30",
        "source-only",
        "proof ceiling",
        "golden trace ≠ telemetry trace",
        "Receipt 不是 Outcome",
        "未实现",
    ] {
        assert!(
            baseline.contains(marker),
            "OA-00 inventory is missing marker {marker:?}"
        );
    }

    // OA-01+ introduces these names deliberately. Keep the absence check scoped to contracts
    // that are still intentionally open; implemented signal/reducer/port names are removed from
    // this historical guard as each roadmap step records its own source hash and CI fixture.
    let scoped_sources = BASELINE
        .iter()
        .map(|(relative, _)| read_source(&root, relative))
        .collect::<Vec<_>>()
        .join("\n");
    for future_contract in ["OperationalLog", "MetricSnapshot"] {
        assert!(
            !scoped_sources.contains(future_contract),
            "OA-00 source guard unexpectedly found future contract {future_contract:?}; update the step and inventory together"
        );
    }
}
