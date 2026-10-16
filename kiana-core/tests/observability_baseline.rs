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
        "bdcc46900a2530c526c68773b5fda7055efc0abd2d7718b7eb5bec399c163df1",
    ),
    (
        "kiana-domain/src/lib.rs",
        "852dd1c5cceebba034a10a0e11f8f15bedb57de8b264e05017c0e3b1766498c7",
    ),
    (
        "kiana-core/src/events.rs",
        "1b9100c8f05ecb0d283a1c48980f712fa65d299fb02b6f667fa1259db6d8f782",
    ),
    (
        "kiana-core/src/receipts.rs",
        "b5ebf37858e212616523b9d771489c60d1bb087cac74604b2330caad0122fd67",
    ),
    (
        "kiana-core/src/projection.rs",
        "a62284905d9ae317d79d4b7ab44b31b0946fc3287f9a4126f7310e67e0e9e72c",
    ),
    (
        "kiana-core/src/recovery.rs",
        "5169aecfa239d023af97ea5ddccf0019204dbfdf01d5a14e8e3c3972c9ec1721",
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
        "fe98b8bffdf86cc49ce6630bdf3240b8d92b9590024ce3b2aea7edb132b090f4",
    ),
    (
        "kiana-eventlog/src/event_store_core.rs",
        "1e1b0ee932b7603c6a31d0e0a0061da694e66c36fb6180a23f338ed267cc09fe",
    ),
    (
        "kiana-eventlog/src/journal_core.rs",
        "2c4a7cd0b73e781e4261672be549d252470fece2620223f89a703044406ad529",
    ),
    (
        "kiana-eventlog/src/jsonl.rs",
        "614ef68ba99f8df06dd56c90ca3864956ec1619bef2623bdfba111f7b4315efc",
    ),
    (
        "kiana-eventlog/src/memory.rs",
        "2af7733b0ba0333c9e13dcd71d63224d0d0fe2a3a72943adff60463738c05774",
    ),
    (
        "kiana-daemon/src/run_stream.rs",
        "34c22ff79d94f9b13bf2e531d37eb02ce1a971162ff69a7115d4c8c872b03628",
    ),
    (
        "kiana-daemon/src/lib.rs",
        "adccf8fb406ccb509eab25e416b4a3607860621192fd5c93778bf599b9c39149",
    ),
    (
        "kiana-ports/src/lib.rs",
        "313d3d95987d9343fa0c821faa162645c4cb7221acb541970a75ebfae8797dec",
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
