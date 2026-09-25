use std::fs;

#[test]
fn extension_recovery_matrix_covers_crash_points_and_unknown_fences() {
    let recovery = fs::read_to_string("../kiana-domain/src/extension_recovery.rs")
        .expect("extension recovery source");
    for marker in [
        "EXTENSION_RECOVERY_CASE_SCHEMA",
        "PackageBeforeRename",
        "RenameBeforeEvent",
        "EventBeforeProjection",
        "UpgradeSwitch",
        "HookRunning",
        "ApprovalExpiring",
        "BrokerResultUnknown",
        "QuarantinePackage",
        "RetryIdempotentCommand",
        "RebuildRegistryAndSnapshot",
        "AwaitApproval",
        "ReconcileUnknown",
        "extension_recovery_broker_unknown_must_reconcile",
        "extension_recovery_hook_must_not_auto_retry",
        "extension_recovery_matrix_coverage_missing",
    ] {
        assert!(recovery.contains(marker), "EXT-30 marker missing: {marker}");
    }
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::spawn",
        "reqwest::Client",
        "CapabilityBroker::new",
        "EventStorePort",
        "ProviderGateway",
        "remove_file",
    ] {
        assert!(
            !recovery.contains(forbidden),
            "extension recovery contract gained an effect path: {forbidden}"
        );
    }
}

#[test]
fn extension_recovery_reuses_lifecycle_state_receipt_and_event_boundaries() {
    let lifecycle = fs::read_to_string("../kiana-domain/src/extension_lifecycle.rs")
        .expect("extension lifecycle source");
    let state = fs::read_to_string("../kiana-domain/src/extension_state.rs")
        .expect("extension state source");
    let commands = fs::read_to_string("../kiana-domain/src/extension_commands.rs")
        .expect("extension command source");
    let core = fs::read_to_string("../kiana-core/src/recovery.rs").expect("core recovery source");
    for (name, source, markers) in [
        (
            "lifecycle",
            lifecycle,
            vec![
                "ExtensionLifecycleRecord",
                "valid_transition",
                "registry_revision",
            ],
        ),
        (
            "state",
            state,
            vec![
                "ExtensionStateMigrationReceipt",
                "config_digest",
                "migration",
            ],
        ),
        (
            "commands",
            commands,
            vec!["ExtensionCommandReceipt", "idempotency_key", "replayed"],
        ),
        (
            "core",
            core,
            vec![
                "resume_run",
                "run_resume_claim_conflict",
                "run_resume_authority_changed",
            ],
        ),
    ] {
        for marker in markers {
            assert!(
                source.contains(marker),
                "{name} recovery marker missing: {marker}"
            );
        }
    }
}

#[test]
fn extension_recovery_fixture_does_not_claim_durable_or_live_evidence() {
    let fixture =
        fs::read_to_string("../kiana-domain/tests/fixtures/ext30-extension-recovery.json")
            .expect("recovery fixture");
    assert!(fixture.contains("\"new_process_reopen_proven\": false"));
    assert!(fixture.contains("\"automatic_retry_unknown\": false"));
    assert!(!fixture.contains("remove_file") && !fixture.contains("http://"));
}
