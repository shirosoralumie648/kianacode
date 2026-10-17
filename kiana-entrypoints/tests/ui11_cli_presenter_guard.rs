//! UI-11 boundary guard: the CLI presenter must stay a pure, client-side projection of one
//! server-owned output DTO, and the fixtures, docs and lane must stay wired to that contract.

use serde_json::Value;

const FIXTURE: &str = include_str!("../../kiana-client/tests/fixtures/ui11-cli-presenter.json");
const CLIENT_TEST: &str = include_str!("../../kiana-client/tests/ui11_cli_presenter.rs");
const LANE: &str = include_str!("../../.github/workflows/ui11-cli-presenter.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const USER_DOC: &str = include_str!("../../USER.md");
const BASELINE: &str = include_str!("../../docs/roadmap/ui11-cli-presenter-baseline.md");
const CARD: &str = include_str!("../../docs/roadmap/ui-entrypoints.md");

#[test]
fn cli_presenter_is_a_pure_projection_and_keeps_authority_in_the_client_server_path() {
    let presenter = include_str!("../../kiana-client/src/cli_presenter.rs");
    let contract = include_str!("../../kiana-client/src/cli_contract.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    let fixture: Value = serde_json::from_str(FIXTURE).expect("UI-11 fixture must stay valid JSON");

    for marker in [
        "CliExitCode",
        "CliSignal",
        "present_cli_output",
        "stdout",
        "stderr",
        "cli_tty_output_requires_tty",
        "cli_pager_requires_tty",
        "kiana.cli-output.v1",
        "ResultUnknown",
        "BrokenPipe",
    ] {
        assert!(
            presenter.contains(marker) || contract.contains(marker),
            "UI-11 presenter marker missing: {marker}"
        );
    }
    assert!(client.contains("present_cli_output"));
    assert!(client.contains("KianaClient"));

    for marker in fixture["required"]
        .as_array()
        .expect("UI-11 fixture required markers")
    {
        let marker = marker.as_str().expect("required marker text");
        assert!(
            presenter.contains(marker) || contract.contains(marker),
            "UI-11 required marker missing from the client boundary: {marker}"
        );
    }

    let mut forbidden: Vec<String> = fixture["forbidden"]
        .as_array()
        .expect("UI-11 fixture forbidden markers")
        .iter()
        .map(|marker| marker.as_str().expect("forbidden marker text").to_owned())
        .collect();
    forbidden.extend([
        "DaemonHost".to_owned(),
        "ControlPlane".to_owned(),
        "CapabilityBroker".to_owned(),
        "KianaHarness".to_owned(),
        "tokio::spawn".to_owned(),
        "std::process::Command".to_owned(),
        "std::fs::".to_owned(),
        "reqwest::".to_owned(),
    ]);
    for forbidden in forbidden {
        assert!(
            !presenter.contains(&forbidden),
            "CLI presenter gained execution or transport authority: {forbidden}"
        );
    }

    // The presenter is the only place that decides the shell class; the frozen entrypoint keeps
    // no second exit table and no second presenter, so CLI/Web/Workbench cannot drift.
    let cli = include_str!("../src/cli.rs");
    for marker in [
        "CLI_EXIT_CODE_TABLE",
        "CliPagerPlan",
        "CliStreamFault",
        "CliExitCode",
        "present_cli_output",
        "write_cli_presentation",
    ] {
        assert!(
            !cli.contains(marker),
            "the CLI entrypoint declared a second presenter or exit table: {marker}"
        );
    }
}

#[test]
fn presenter_fixtures_docs_and_lane_stay_wired_to_the_ui11_contract() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("UI-11 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui11-cli-presenter.v1");
    assert_eq!(
        fixture["exit_codes"]
            .as_array()
            .expect("exit code table")
            .len(),
        11
    );
    assert!(fixture["cases"].as_array().expect("golden cases").len() >= 12);
    assert!(fixture["denied"].as_array().expect("deny matrix").len() >= 12);
    assert!(fixture["parity"].as_array().expect("parity rows").len() >= 11);
    assert_eq!(
        fixture["stream_faults"]
            .as_array()
            .expect("stream faults")
            .len(),
        3
    );

    // Every acceptance fixture the card names must exist in the client target.
    for fixture_name in [
        "the_documented_exit_table_matches_the_code_that_produces_it",
        "lifecycle_scenarios_use_the_declared_exit_classes",
        "signals_and_stream_faults_map_to_stable_codes_without_installing_handlers",
        "locale_classification_is_presentation_only",
        "exit_classes_follow_the_protocol_error_table_and_fail_closed",
        "golden_outputs_match_the_declared_cases",
        "denied_inputs_fail_closed_before_any_rendering",
        "pager_plan_rejects_budgets_outside_the_contract",
        "writes_stop_at_the_first_fault_and_never_hide_the_response_class",
        "cli_and_web_agree_on_the_same_server_response_without_touching_authority",
    ] {
        assert!(
            CLIENT_TEST.contains(fixture_name),
            "UI-11 client fixture is missing: {fixture_name}"
        );
    }
    for golden in [
        "json_success_is_one_dto_on_stdout_and_warnings_on_stderr",
        "json_failure_is_one_dto_on_stderr_and_never_zero",
        "tty_success_zh_renders_a_bounded_text_view",
        "tty_failure_keeps_the_body_and_diagnostics_off_stdout",
        "tty_output_is_truncated_at_the_adapter_bound",
        "quiet_denial_suppresses_stdout_and_keeps_the_reason",
        "tty_pagination_splits_on_lines_without_losing_bytes",
        "tty_pagination_keeps_an_over_budget_line_whole_on_its_own_page",
    ] {
        assert!(
            FIXTURE.contains(golden) || CLIENT_TEST.contains(golden),
            "UI-11 golden case is missing: {golden}"
        );
    }

    // The manual lane runs the two UI-11 targets behind the format gate and never turns the
    // step into an automatic push/pull_request trigger.
    assert!(LANE.contains("workflow_dispatch:"));
    assert!(LANE.contains("contents: read"));
    assert!(LANE.contains("cargo fetch --locked"));
    assert!(LANE.contains("cargo fmt --all --check"));
    assert!(LANE.contains("cargo test -p kiana-client --test ui11_cli_presenter"));
    assert!(LANE.contains("cargo test -p kiana-entrypoints --test ui11_cli_presenter_guard"));
    assert!(LANE.contains("--no-run"));
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !LANE.contains(forbidden),
            "UI-11 lane must not weaken the gate: {forbidden}"
        );
    }

    // Automatic coverage stays with the unified gate: both crates are whole-crate shards, so a
    // new acceptance target cannot be silently skipped.
    let shards: Value = serde_json::from_str(SHARDS).expect("test shards must stay valid JSON");
    for crate_name in ["kiana-client", "kiana-entrypoints"] {
        assert!(
            shards[crate_name]["targets"].is_null(),
            "{crate_name} must keep its whole-crate shard so UI-11 targets are auto-discovered"
        );
    }

    // The user-facing `--json/--quiet` contract documents the same stable table and bounds.
    for marker in [
        "kiana.cli-output.v1",
        "--json",
        "--quiet",
        "退出码",
        "130",
        "141",
        "TTY",
    ] {
        assert!(
            USER_DOC.contains(marker),
            "USER.md no longer documents the CLI output contract marker: {marker}"
        );
    }
    for marker in [
        "CLI_EXIT_CODE_TABLE",
        "CliPagerPlan",
        "write_cli_presentation",
        "cli_pager_too_many_pages",
        ".github/workflows/ui11-cli-presenter.yml",
        "kiana-client/tests/fixtures/ui11-cli-presenter.json",
    ] {
        assert!(
            BASELINE.contains(marker),
            "UI-11 baseline no longer records: {marker}"
        );
    }
    assert!(CARD.contains("step-ui-11"));
}
