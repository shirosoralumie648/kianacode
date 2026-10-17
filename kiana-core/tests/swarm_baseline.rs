//! `SW-00` Swarm source-only reconciliation guard.
//!
//! These tests intentionally inspect source and the reconciliation document. They do not run
//! the product path or prove durable behavior; runtime acceptance belongs to later SW cards and
//! GitHub CI.

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
        "kiana-domain/src/swarm.rs",
        "3fbcc14095e0889c407727a92816880ff3b08c4c7e1544a9f24cee4aed7b8bf1",
    ),
    (
        "kiana-domain/src/packet_graph.rs",
        "a0f862215d2df285f26191e51a89029640f0925ab1a8072f369e70b03562afeb",
    ),
    (
        "kiana-domain/src/work_packets.rs",
        "e44097f23647d76a57b908d807ed065eb317db0b9e577407094c511fbd6aaa07",
    ),
    (
        "kiana-core/src/swarm.rs",
        "5d54a4e7f41c2edf1233fe0d095e86348213e759d951aa2e5f23b87ce95b0773",
    ),
    (
        "kiana-core/src/cell_registry.rs",
        "2de1f229e57f63b290abb312191d3fbbaaa4787c048adbbf80f68e63703ae40d",
    ),
    (
        "kiana-core/src/collaboration.rs",
        "22bfc2df7cbc267e5db4fe6e13bea72f5aa13f3145d913bd1b42896aecaa5a4e",
    ),
    (
        "kiana-ports/src/lib.rs",
        "b1a5f7502a83eccc5192629c502b517864095b8bb95a3c9cdbe98eb71e8a2e46",
    ),
    (
        "kiana-protocol/src/lib.rs",
        "7752afe54407cdef0f1d3077ebf57d3e46c19303b5ec2fd362e3add238b236c8",
    ),
    (
        "kiana-core/src/commands.rs",
        "6dcd0bfc89fcd4eed4eb20c075bd47391cc1e58023141a7d70b11b30ae4c2b4a",
    ),
    (
        "kiana-daemon/src/lib.rs",
        "f6d8ac42958c18744af1fe823329a8e49efbb8488942d50a25dfe62e5ab36246",
    ),
];

#[test]
fn swarm_source_baseline_is_reproducible() {
    let root = workspace_root();
    for (relative, expected) in BASELINE {
        let actual = sha256_of(&root.join(relative));
        assert_eq!(
            actual, *expected,
            "{relative} drifted from the SW-00 source snapshot; update the reconciliation document and this table together"
        );
    }
}

#[test]
fn swarm_reconciliation_does_not_claim_durable_from_in_memory_cas() {
    let root = workspace_root();
    let registry = read_source(&root, "kiana-core/src/cell_registry.rs");
    let ports = read_source(&root, "kiana-ports/src/lib.rs");
    let commands = read_source(&root, "kiana-core/src/commands.rs");
    let swarm = read_source(&root, "kiana-core/src/swarm.rs");
    let baseline = read_source(&root, "docs/roadmap/swarm-baseline.md");

    assert!(registry.contains("state is intentionally process-local"));
    assert!(registry.contains("pub struct MemoryCellRegistry"));
    assert!(ports.contains("trait 本身不承诺 durable 恢复"));
    assert!(commands.contains("Arc::new(MemoryCellRegistry::new())"));

    let committed = swarm
        .find("let committed = self.commit_swarm(&context, event)")
        .expect("swarm fact commit boundary");
    let dispatch = swarm
        .find(".handle_company_command(")
        .expect("Company StartRun dispatch boundary");
    assert!(
        committed < dispatch,
        "SW-00 must preserve the documented event-before-dispatch reconciliation window"
    );
    assert!(swarm.contains("ExecutionStatus::ResultUnknown"));

    for marker in [
        "MemoryCellRegistry",
        "不承诺 durable recovery",
        "event-before-dispatch",
        "P4-J6-01",
        "CO-43",
        "CO-44",
        "proof-level",
    ] {
        assert!(
            baseline.contains(marker),
            "SW-00 reconciliation document is missing marker {marker:?}"
        );
    }
}
