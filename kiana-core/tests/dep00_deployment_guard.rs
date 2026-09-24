//! DEP-00 source guard for the deployment/operations inventory boundary.

#[test]
fn inventory_binds_existing_composition_and_fact_sources() {
    let baseline = include_str!("../../docs/roadmap/dep00-deployment-baseline.md");
    for marker in [
        "794b6d44",
        "kiana-daemon/src/lib.rs",
        "kiana-eventlog/src/{lib.rs,event_store_core.rs,journal_core.rs,jsonl.rs,artifact_store.rs}",
        "kiana-domain/src/{contracts.rs,migration.rs,migration_registry.rs,migration_runner.rs}",
        "scripts/release-preflight.sh",
        "entrypoints → client/protocol → DaemonHost → ControlPlane",
        "does not claim durable",
    ] {
        assert!(baseline.contains(marker), "DEP-00 inventory marker missing: {marker}");
    }
}

#[test]
fn deployment_inventory_keeps_supervision_and_legacy_boundaries_explicit() {
    let baseline = include_str!("../../docs/roadmap/dep00-deployment-baseline.md");
    assert!(baseline.contains("no supervisor adapter may become a second execution authority"));
    assert!(baseline.contains("kiana-tools`、`kiana-commands`"));
    assert!(baseline.contains("must not be wired into a new deployment path"));
    assert!(baseline.contains("DEP-01 onward"));
}

