//! PD-30 / PD-32 / PD-34 core-side source guard.
//!
//! The three matrices live in `kiana-ports`, next to PD-31's adapter conformance suite, because
//! they are decisions about *adapters*: a fault classification names a PD-31 `AdapterKind`, a
//! platform cell is cross-checked against PD-28's platform record, and a capacity budget is
//! declared against DEP-17's `CapacityEnvelope`. Putting them in the control plane would have made
//! them reachable without a port, which is the drift the repository boundaries warn about.
//!
//! This guard therefore asserts two things from the core side: the three modules are reachable
//! from the `kiana-ports` crate root (so a downstream crate can actually call them), and the
//! control plane does not re-declare any of their vocabularies. A second copy of
//! `StorageCapacitySubject` or `StoragePlatformTarget` in `kiana-core` would be exactly the
//! "second vocabulary for something that already has one" the cards forbid.

#[test]
fn the_three_matrices_are_reachable_from_the_ports_crate_root() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    for marker in [
        "mod storage_fault_matrix;",
        "mod storage_platform_matrix;",
        "mod storage_capacity_budget;",
        "pub use storage_fault_matrix::{",
        "pub use storage_platform_matrix::{",
        "pub use storage_capacity_budget::{",
        "StorageFaultMatrix",
        "StoragePlatformMatrix",
        "StorageDegradationReport",
        "admit_fault_restart",
        "admit_platform_expiry",
        "storage_fault_scope",
        "storage_platform_scope",
    ] {
        assert!(
            ports.contains(marker),
            "PD-30/32/34 root export missing: {marker}"
        );
    }
}

#[test]
fn the_control_plane_does_not_redeclare_the_matrix_vocabulary() {
    let core = include_str!("../src/lib.rs");
    // If any of these appear in `kiana-core`, a second vocabulary exists. They belong in
    // `kiana-ports`, beside the adapter conformance suite they extend.
    for parallel in [
        "pub enum StorageFaultKind",
        "pub enum StoragePlatformTarget",
        "pub enum StorageCapacitySubject",
        "pub enum BudgetMeasurement",
        "pub struct StorageFaultMatrix",
        "pub struct StoragePlatformMatrix",
        "pub struct StorageDegradationReport",
    ] {
        assert!(
            !core.contains(parallel),
            "PD-30/32/34 must not be redeclared in kiana-core: {parallel}"
        );
    }
    // The core keeps its own existing contracts; this slice added nothing to the control plane and
    // introduced no second execution path, port or adapter.
    assert!(!core.contains("kiana-ports::storage_fault_matrix::"));
    assert!(!core.contains("kiana-ports::storage_platform_matrix::"));
    assert!(!core.contains("kiana-ports::storage_capacity_budget::"));
}

#[test]
fn the_baselines_are_present_and_state_the_proof_ceiling() {
    for baseline in [
        include_str!("../../docs/roadmap/pd30-storage-fault-matrix-baseline.md"),
        include_str!("../../docs/roadmap/pd32-storage-platform-matrix-baseline.md"),
        include_str!("../../docs/roadmap/pd34-capacity-budget-baseline.md"),
    ] {
        // Each baseline names its proof ceiling, what it does not prove, and the file it describes,
        // so a reader arriving from the roadmap can tell what the slice is and what it is not.
        for marker in ["proof_level", "proof", "does NOT prove", "kiana-ports/src/"] {
            assert!(
                baseline.contains(marker),
                "PD-30/32/34 baseline marker missing: {marker}"
            );
        }
    }
}
