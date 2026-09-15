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
        "dca28dc71ef75e5dd92a25bed399349ca286bc7c5c0e514e11de20d6d1491ae3",
    ),
    (
        "kiana-domain/src/journal.rs",
        "4dbd656d03ec12e7821ffac254307227419423cbaf74ccb99a2b819c5eeedd48",
    ),
    (
        "kiana-domain/src/contracts.rs",
        "dee652cef9bf1e4822f6bece7bef3ea9fbbd2255df9f559febb45334df1f6780",
    ),
    (
        "kiana-domain/src/lib.rs",
        "c0002ed07ed1a084fec9ed0ac8535efe2ae84e1fc90e51ee91b71240c0c6a269",
    ),
    (
        "kiana-core/src/events.rs",
        "7d1852ad4a9d93792256288a01a25e06c677e0f6641274b2575b718c87020679",
    ),
    (
        "kiana-core/src/receipts.rs",
        "dda33c346b1ffd94389093e2856f99433ea083a3c61b35cf562484f9f4bc7e1e",
    ),
    (
        "kiana-core/src/projection.rs",
        "20d84eb8fa0ac77ac85b48acb10e2fcc78214cc1c3c10be339b540230b1ff68",
    ),
    (
        "kiana-core/src/recovery.rs",
        "f964c69bb32e940782ef52b399afd9ad6902778b4900f6b7164cb79f3dcc7d26",
    ),
    (
        "kiana-core/src/lifecycle.rs",
        "346ecb8ea2879b24a53a2ee238bb46c3f7793ffa6211c126ec86ad49c47bc237",
    ),
    (
        "kiana-core/src/versioning.rs",
        "679c88757469189f93b20163bbfccb9d65a161ab24ec9f6d9e73a483586108ab",
    ),
    (
        "kiana-core/src/collaboration.rs",
        "49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf",
    ),
    (
        "kiana-eventlog/src/lib.rs",
        "61aa7bcc9c221474d3ffa75a323ba7931f1da8e7ce96107da1d1d7cb7fabd20c",
    ),
    (
        "kiana-eventlog/src/event_store_core.rs",
        "506a8222a757a89e6516a12b6260daa7f35af2b3da49c17a23ca7cdbcc9d0b1e",
    ),
    (
        "kiana-eventlog/src/journal_core.rs",
        "84ef64bc8208b2ead45d1d05feb0265e94129b898cd67d04f5ed189d388bba56",
    ),
    (
        "kiana-eventlog/src/jsonl.rs",
        "f549e5de5bc4e197f268b865327bd1d0d7a4c10208daa3e7c13ceb68188e5c5f",
    ),
    (
        "kiana-eventlog/src/memory.rs",
        "bcb98daa0a9548384a9dafdd9d5af2aefacca3d01d699056ba86477d3276e9e9",
    ),
    (
        "kiana-daemon/src/run_stream.rs",
        "750bb78e46e5f1c42d3a773d0afcf468cc6713032ef47e30ffa100f3313db5c6",
    ),
    (
        "kiana-daemon/src/lib.rs",
        "f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281",
    ),
    (
        "kiana-ports/src/lib.rs",
        "557332aa2115b8c98d7aa261eddb3549c07b6805dd53aa41bf1b3ccc8e0ba723",
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
        .find("let data = redact_event_value(&data)")
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

    // OA-01+ introduces these names deliberately. Keeping the absence check scoped to the
    // current product sources makes the migration point explicit without blocking on docs.
    let scoped_sources = BASELINE
        .iter()
        .map(|(relative, _)| read_source(&root, relative))
        .collect::<Vec<_>>()
        .join("\n");
    for future_contract in [
        "ObservabilityPort",
        "TraceSink",
        "MetricSink",
        "AuditQueryPort",
        "HealthProbePort",
        "OperationalLog",
        "MetricPoint",
        "MetricSnapshot",
        "AuditRecord",
        "HealthSnapshot",
        "span_id",
        "source_cursor",
        "source_event_ids",
    ] {
        assert!(
            !scoped_sources.contains(future_contract),
            "OA-00 source guard unexpectedly found future contract {future_contract:?}; update the step and inventory together"
        );
    }
}
