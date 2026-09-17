#[test]
fn eval_fixture_loader_is_scoped_and_deterministic_without_a_second_runner() {
    let loader = include_str!("../../kiana-commands/src/eval_fixtures.rs");
    let commands = include_str!("../../kiana-commands/src/eval.rs");
    for marker in [
        "EvalFixtureManifest",
        "EvalFixtureCase",
        "LoadedFixtureManifest",
        "load_fixture_manifest",
        "EVAL_FIXTURE_MANIFEST_SCHEMA",
        "EVAL_MAX_CASE_FIXTURE_BYTES",
        "resolve_declared_path",
        "canonicalize",
        "starts_with(root)",
        "fixture_schema_unknown",
        "fixture_manifest_case_id_invalid",
    ] {
        assert!(
            loader.contains(marker),
            "fixture loader marker missing: {marker}"
        );
    }
    assert!(commands.contains("run_suite"));
    assert!(!loader.contains("tokio::spawn"));
    assert!(!loader.contains("reqwest"));
    assert!(!loader.contains("CapabilityBroker"));
}
