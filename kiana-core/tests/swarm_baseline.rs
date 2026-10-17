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
        "575b2dab25740f4e218fb011b84dd554b1ba5a7000d56d82c69e37d5c22b6727",
    ),
    (
        "kiana-core/src/collaboration.rs",
        "22bfc2df7cbc267e5db4fe6e13bea72f5aa13f3145d913bd1b42896aecaa5a4e",
    ),
    (
        "kiana-ports/src/lib.rs",
        "a58b9c59f2c548820f5ac75da734d302859220f822441794c51887b888728b2f",
    ),
    (
        "kiana-protocol/src/lib.rs",
        "2cc01c142489b593ea641012d787c0c3c20eb6402096a511ab50f3bbb938d7e8",
    ),
    (
        "kiana-core/src/commands.rs",
        "630039c87be6dc85064c43aeea6f0129607435ae20cf91311297d2c8f79f75df",
    ),
    (
        "kiana-daemon/src/lib.rs",
        "384db8716260eaa3a450dfa6263eb3b4c3b28eb95d2b3aa0b204d21f55919826",
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
