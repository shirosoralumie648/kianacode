#[test]
fn daemon_restore_verifier_only_checks_manifest_and_replacement_fence() {
    let daemon = include_str!("../src/restore_verifier.rs");
    for marker in [
        "SnapshotManifest",
        "RestoreVerificationFact",
        "manifest.validate",
        "restore_manifest_fact_binding_invalid",
        "verify_restore",
    ] {
        assert!(
            daemon.contains(marker),
            "PD-23 daemon marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "copy_dir",
        "rename(",
        "acquire_lock",
        "activate_root",
        "revive_trigger",
        "EventStore::append",
    ] {
        assert!(
            !daemon.contains(forbidden),
            "PD-23 daemon verifier executes restore effect: {forbidden}"
        );
    }
}
