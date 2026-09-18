//! CP-28 source guard for explicit migration, compatibility adapters and legacy bypass closure.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CP-28 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn cp_legacy_authority_records_require_reauthorization() {
    let events = include_str!("../../kiana-domain/src/event_contracts.rs");
    let storage = include_str!("../../kiana-domain/src/storage_schema.rs");
    let identity = include_str!("../../kiana-domain/src/identity_contracts.rs");
    let approvals = include_str!("../../kiana-daemon/src/journal_approvals.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let recovery = include_str!("../src/recovery.rs");

    require(
        events,
        &[
            "event_kind_spec",
            "event_kind_is_required",
            "event_migration",
            "unknown_required_event_kind",
            "legacy_run_event_v0_to_v1",
            "legacy_approval_event_v0_to_v1",
            "event_schema_version_incompatible",
        ],
        "RuntimeEvent migration",
    );
    require(
        storage,
        &[
            "StorageSchemaRegistry",
            "validate_schema_registry",
            "canonical_storage_bytes",
            "canonical_storage_digest",
            "upcast_storage_value",
            "upcast_memory_record",
            "storage_schema_unknown_major",
            "storage_migration_non_migratable_field",
            "SCHEMA_MIGRATIONS",
        ],
        "storage upcaster",
    );
    require(
        identity,
        &[
            "IdentityMigration",
            "legacy_local_user_migration",
            "migration_digest",
            "not an authentication assertion",
            "principal_id",
        ],
        "identity migration",
    );
    require(
        approvals,
        &[
            "approval_legacy_reauthorization_required",
            "approval_clock_rollback",
            "redact_value",
            "approval_payload_unrecoverable",
        ],
        "approval migration",
    );
    require(
        protocol,
        &[
            "continue_run",
            "new_turn",
            "run.turn.v2",
            "check_schema_compatibility",
            "PROTOCOL_SCHEMA",
        ],
        "wire compatibility",
    );
    require(
        recovery,
        &[
            "resume_run",
            "run_resume_authority_changed",
            "run_resume_scope_changed",
            "run_snapshot_stale",
            "payload_unrecoverable",
            "read_all_events",
            "reauthor",
        ],
        "recovery reauthorization",
    );
}

#[test]
fn cp_old_writer_cannot_mutate_new_journal() {
    let jsonl = include_str!("../../kiana-eventlog/src/jsonl.rs");
    let journal = include_str!("../../kiana-eventlog/src/journal_core.rs");
    let memory = include_str!("../../kiana-eventlog/src/memory.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/persistence-data-layer.md");

    require(
        jsonl,
        &[
            "encode_legacy_record",
            "writer_version",
            "eventlog_legacy_writer_after_upgrade",
            "event_store_atomic_transitions_unsupported",
            "eventlog_file_replaced",
            "eventlog_committed_prefix_truncated",
            "sync_directory",
        ],
        "JSONL writer fence",
    );
    require(
        journal,
        &[
            "plan_legacy",
            "apply_legacy",
            "JOURNAL_WRITER_VERSION",
            "eventlog_legacy_writer_after_upgrade",
            "writer_version",
        ],
        "journal state",
    );
    require(
        memory,
        &[
            "plan_legacy",
            "apply_legacy",
            "event_store_atomic_transitions_unsupported",
        ],
        "memory adapter",
    );
    require(
        ports,
        &[
            "MigrationRunnerPort",
            "migration_runner_unsupported",
            "from_format_version",
            "registry_digest",
            "event_store_atomic_transitions_unsupported",
        ],
        "migration port",
    );
    require(
        baseline,
        &[
            "run migration preflight (read-only)",
            "migration",
            "unknown",
        ],
        "migration baseline",
    );
}

#[test]
fn cp_every_product_constructor_enforces_the_same_boundary() {
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let harness = include_str!("../../kiana-entrypoints/src/harness_run.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let architecture = include_str!("../../kiana-entrypoints/tests/cli_architecture.rs");
    let boundaries = include_str!("dependency_boundaries.rs");
    let spine = include_str!("../../kiana-daemon/tests/eq12_daemon_spine.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");

    require(
        daemon,
        &[
            "new_with_project_authority",
            "DaemonHost",
            "with_harness_and_project_authority",
            "legacy_local_user_migration",
            "project_authority",
            "EventStorePort",
        ],
        "DaemonHost constructors",
    );
    require(
        harness,
        &[
            "new_local_host_with_options",
            "DaemonHost::local_with_model_config",
            "client_on_host",
            "RequestEnvelope",
            "DaemonHost",
        ],
        "harness route",
    );
    require(
        cli,
        &[
            "harness_run",
            "RequestEnvelope",
            "system.architecture",
            "supports_non_interactive",
        ],
        "CLI route",
    );
    require(
        architecture,
        &[
            "composition_root",
            "legacy_prompt_loop",
            "brokered",
            "run_assistant_turn",
        ],
        "architecture fixture",
    );
    require(
        boundaries,
        &[
            "STRICT_ALLOWED",
            "LEGACY_EDGES",
            "legacy_implementation_edges_can_only_decrease",
        ],
        "dependency boundary",
    );
    require(
        spine,
        &[
            "DaemonHost::new(core)",
            "ControlPlane",
            "CapabilityBroker",
            "KianaHarness",
        ],
        "daemon spine fixture",
    );
    require(
        client,
        &[
            "ClientTransport",
            "RequestEnvelope",
            "transport",
            "ControlPlane",
        ],
        "client boundary",
    );

    for source in [harness, cli, client] {
        assert!(!source.contains("run_assistant_turn"));
        assert!(!source.contains("CapabilityBroker::new"));
    }
}
