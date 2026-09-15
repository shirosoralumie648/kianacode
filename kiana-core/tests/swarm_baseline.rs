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
        "697c11d7c0f0be37adc5aeac6617e0920190b9c895574449994c4aec3c26ba0d",
    ),
    (
        "kiana-domain/src/packet_graph.rs",
        "8f56ce5fb8ba80f50aabff3e6fb6221718289437b1a511123821c0d98509c8a0",
    ),
    (
        "kiana-domain/src/work_packets.rs",
        "2cc8b27f3710512b50bef970715dade5d76f27d010c77e14e3c22823d2b8b1e6",
    ),
    (
        "kiana-core/src/swarm.rs",
        "71a3b50e7971cb3c3eac4fece08a8e94f4a4ca83ec3c0a6a7c1d7407460fc9bb",
    ),
    (
        "kiana-core/src/cell_registry.rs",
        "218ae2f9efec2c173881cd5d33d41d414c7e76f9db9e130d6518bfde602d9663",
    ),
    (
        "kiana-core/src/collaboration.rs",
        "49e61cf89add855ca4fc3687104d666388477848d63397b9ea50b0404021dabf",
    ),
    (
        "kiana-ports/src/lib.rs",
        "88ff290c7a848e45af34985c42b0f7c0fb5719f8ae42708076e73c3290706ab1",
    ),
    (
        "kiana-protocol/src/lib.rs",
        "19b06c1e828bef31cf6fdbd52283e0bf2ed54b69ca4d1c6fed94dc385e376ace",
    ),
    (
        "kiana-core/src/commands.rs",
        "305d94e31ef5fceb494284dde0fa0b0bdfa0a31cda8d9609e890e7ca593063d8",
    ),
    (
        "kiana-daemon/src/lib.rs",
        "f262e808ab239eed0e01c1e28aef75cb5d78b819d809c1d07592ab34d367d281",
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
        .find("self.handle_company_command(")
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
