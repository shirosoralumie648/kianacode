//! UI-10 boundary guard: the CLI command/output contract must stay a client-side normalization
//! layer.  It may bound input, resolve a caller-supplied workspace/session and render help text,
//! but it must never own a daemon, broker, event store, process spawn or model loop.

#[test]
fn cli_contract_is_a_client_boundary_and_has_no_execution_authority() {
    let contract = include_str!("../../kiana-client/src/cli_contract.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let cli = include_str!("../src/cli.rs");

    for marker in [
        "CliCommand",
        "CliInvocation",
        "CliOutput",
        "CLI_COMMAND_SCHEMA",
        "CLI_COMMAND_SPECS",
        "CliCommandSpec",
        "CliFacade",
        "CliArgumentSpec",
        "CliArgumentError",
        "CliSessionRef",
        "CLI_SESSION_LATEST",
        "normalize_legacy_args",
        "cli_help_text",
        "cli_session_required",
        "cli_session_unknown",
        "cli_interactive_requires_tty",
        "cli_tty_output_requires_tty",
        "cli_implicit_retry_forbidden",
        "cli_output_mode_unsupported",
        "cli_argument_missing",
        "cli_argument_unknown",
        "cli_argument_invalid",
        "cli_argument_oversized",
        "cli_legacy_flag_retired",
        "contains_secret_key",
        "contains_secret_text_value",
        "run",
        "run.status",
        "run.events",
        "approval.approve",
        "approval.deny",
        "run.cancel",
        "run.resume",
        "run.receipt",
        "audit.export",
        "session.query",
    ] {
        assert!(
            contract.contains(marker),
            "UI-10 contract marker missing: {marker}"
        );
    }
    assert!(client.contains("KianaClient"));
    assert!(client.contains("pub use cli_contract"));
    assert!(cli.contains("KianaClient"));

    for forbidden in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "EventStorePort",
        "tokio::spawn",
        "std::process::Command",
        "std::fs::",
        ".append(",
    ] {
        assert!(
            !contract.contains(forbidden),
            "CLI contract gained execution authority: {forbidden}"
        );
    }
    assert!(!contract.contains("#[ignore"));
    assert!(!contract.contains("--ignored"));
}

/// Completion slice (2026-10-17): the declared contract fixture, the client fixtures, the manual
/// lane, the baseline document and the whole-crate shards must stay wired together, and the
/// entrypoint crate must not grow a second command table.
#[test]
fn cli_command_contract_stays_wired_to_the_typed_clients_and_the_manual_lane() {
    let contract = include_str!("../../kiana-client/src/cli_contract.rs");
    let fixtures = include_str!("../../kiana-client/tests/ui10_cli_contract.rs");
    let fixture_raw = include_str!("../../kiana-client/tests/fixtures/ui10-cli-contract.json");
    let baseline = include_str!("../../docs/roadmap/ui10-cli-contract-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui10-cli-contract.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");
    let entrypoints_lib = include_str!("../src/lib.rs");

    // 1. the contract tables and fences stay in the client source.
    for marker in [
        "cli_argument_oversized",
        "cli_arguments_too_many",
        "CliFacade::Query",
        "CliFacade::Feed",
        "CliFacade::Action",
        "CliFacade::Artifact",
        "follow_up_operation",
        "validate_command_arguments",
        "normalize_workspace",
        "classify_legacy_flag",
        "CliLegacyDisposition::Retired",
        "CLI_MAX_ARGV_TOKENS: usize = 4_096;",
        "CLI_MAX_MIGRATION_NOTICES: usize = 32;",
    ] {
        assert!(
            contract.contains(marker),
            "UI-10 completion marker missing from the client contract: {marker}"
        );
    }

    // 2. the fixture describes the same ten commands and every declared fixture is a real test.
    let fixture: serde_json::Value =
        serde_json::from_str(fixture_raw).expect("the UI-10 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.cli-command-contract.v1");
    assert_eq!(fixture["step"], "UI-10");
    assert_eq!(fixture["command_schema"], "kiana.cli-command.v1");
    assert_eq!(fixture["session_latest"], "latest");
    let commands: Vec<&str> = fixture["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|row| row["command"].as_str().expect("command"))
        .collect();
    assert_eq!(
        commands,
        vec![
            "run", "status", "events", "approve", "deny", "cancel", "resume", "receipt", "export",
            "session"
        ]
    );
    for row in fixture["commands"].as_array().expect("commands") {
        assert!(row["operation"].as_str().is_some());
        assert!(!row["output_modes"].as_array().expect("modes").is_empty());
    }
    assert_eq!(fixture["budgets"]["max_argv_tokens"].as_u64(), Some(4_096));
    assert_eq!(
        fixture["budgets"]["max_migration_notices"].as_u64(),
        Some(32)
    );
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert!(
        declared.len() >= 12,
        "the completion slice keeps every named fixture"
    );
    for name in &declared {
        assert!(
            fixtures.contains(&format!("fn {name}(")),
            "UI-10 fixture is not defined in kiana-client/tests/ui10_cli_contract.rs: {name}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    for name in [
        "command_specs_match_the_declared_contract_fixture",
        "per_command_arguments_fail_closed_before_dispatch",
        "output_mode_support_is_enforced_per_command",
        "session_resolution_refuses_unknown_ids_from_the_server_snapshot",
        "legacy_args_rewrite_or_fail_closed_per_the_migration_table",
        "legacy_normalization_bounds_and_passthrough_are_enforced",
        "typed_validation_maps_to_the_same_codes_as_the_string_api",
    ] {
        assert!(
            declared.contains(&name),
            "the fixture must keep the completion-slice test: {name}"
        );
    }
    assert!(!fixture["denied"].as_array().expect("denied").is_empty());
    assert!(!fixture["limitations"]
        .as_array()
        .expect("limitations")
        .is_empty());

    // 3. the baseline documents the completion slice and the card carries the same acceptance.
    for marker in [
        "UI-10",
        "cli_legacy_flag_retired",
        "cli_session_unknown",
        "ui10-cli-contract.yml",
        "ui10_cli_contract",
        "proof_level",
        "limitations",
        "command_specs_match_the_declared_contract_fixture",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-10 baseline is missing: {marker}"
        );
    }
    assert!(card.contains("UI-10"));
    assert!(card.contains("command_specs_match_the_declared_contract_fixture"));

    // 4. the entrypoint crate keeps using the client facade instead of a second command table.
    for forbidden in [
        "CLI_COMMAND_SPECS",
        "CliCommandSpec",
        "cli_argument_missing",
    ] {
        assert!(
            !entrypoints_lib.contains(forbidden),
            "kiana-entrypoints must not redeclare the CLI contract: {forbidden}"
        );
    }

    // 5. the manual lane stays dispatch-only, read-only and bound to the same targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:"));
    assert!(lane.contains("contents: read"));
    for forbidden in [
        "pull_request",
        "  push:",
        "schedule:",
        "continue-on-error",
        "|| true",
        "--ignored",
    ] {
        assert!(
            !lane.contains(forbidden),
            "UI-10 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-client --test ui10_cli_contract --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui10_cli_boundary_guard --locked -- --test-threads=1",
        "cargo test -p kiana-client -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-10 lane is missing: {command}");
    }

    // 6. the automatic shard book keeps the whole-crate shards that discover the new targets.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    for crate_name in ["kiana-client", "kiana-entrypoints"] {
        assert!(
            shards[crate_name]["targets"].is_null(),
            "{crate_name} must stay a whole-crate shard so new UI-10 targets are discovered"
        );
    }
}
