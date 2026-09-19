//! PD-35 source guard for persistence closeout, migration runbook and proof ceilings.

#[test]
fn pd35_closeout_keeps_all_persistence_steps_and_limits_explicit() {
    let closeout = include_str!("../../docs/roadmap/pd35-persistence-closeout.md");
    let design = include_str!("../../docs/roadmap/persistence-data-layer.md");
    let status = include_str!("../../CURRENT_STATUS.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let runbook = include_str!("../../docs/roadmap/dep41-operator-runbook.md");
    let matrix = include_str!("../../docs/roadmap/dep41-capability-proof-matrix.md");
    let script = include_str!("../../scripts/validate-pd35-persistence-closeout.sh");
    let workflow = include_str!("../../.github/workflows/pd35-persistence-closeout.yml");
    let pd33 = include_str!("../../kiana-domain/src/persistence_uat_evidence.rs");
    let pd34 = include_str!("../../kiana-domain/src/persistence_capacity_evidence.rs");

    for number in 0..35 {
        let marker = format!("PD-{number:02}");
        assert!(
            closeout.contains(&marker),
            "PD-35 closeout missing {marker}"
        );
        assert!(
            design.contains(&marker),
            "persistence design missing {marker}"
        );
    }
    for marker in [
        "feature_status",
        "proof_level",
        "partial",
        "target/partial",
        "source",
        "durable",
        "live",
        "physical",
        "StorageRoot",
        "preflight",
        "MigrationRegistry",
        "result_unknown",
        "retention",
        "legal-hold",
        "deletion receipt",
        "limitations",
        "reviewer",
    ] {
        assert!(
            closeout.contains(marker),
            "PD-35 closeout marker missing: {marker}"
        );
    }
    for marker in [
        "validate-pd35-persistence-closeout.sh",
        "cargo fmt --all --check",
        "cargo check --workspace --tests --locked",
        "cargo test -p kiana-core --test pd35_persistence_closeout_guard",
        "git diff --check",
    ] {
        assert!(
            workflow.contains(marker),
            "PD-35 workflow marker missing: {marker}"
        );
    }
    assert!(status.contains("### PD-35"));
    assert!(roadmap.contains("<a id=\"step-pd-35\"></a>`PD-35`"));
    assert!(runbook.contains("result_unknown"));
    assert!(matrix.contains("proof_level"));
    assert!(pd33.contains("PersistenceUatEvidence"));
    assert!(pd34.contains("PersistenceCapacityEvidence"));
    assert!(script.contains("range(35)"));
    assert!(!closeout.contains("PD-35 closeout | implemented | durable"));
}
