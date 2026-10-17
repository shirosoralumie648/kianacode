#[test]
fn volatile_normalization_requires_declared_rules_and_has_no_io_path() {
    let source = include_str!("../src/volatile.rs");
    assert!(source.contains("VolatilePolicy"));
    assert!(source.contains("UndeclaredVolatile"));
    assert!(source.contains("replacement_count"));
    assert!(source.contains("replacement_count_for"));
    assert!(source.contains("<TS>"));
    assert!(source.contains("<UUID>"));
    assert!(source.contains("<TEMP_PATH>"));
    assert!(source.contains("<ACTOR>"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
}

#[test]
fn volatile_fixtures_are_wired_into_the_eq19_lane() {
    let fixtures = include_str!("eq19_volatile.rs");
    let workflow = include_str!("../../.github/workflows/eq19-trace-volatile.yml");
    let baseline = include_str!("../../docs/roadmap/evaluation-trace-volatile-baseline.md");
    for name in [
        "declared_timestamp_uuid_temp_path_and_actor_are_replaced_with_counts",
        "undeclared_volatile_value_is_not_silently_normalized",
        "wildcard_rules_cover_array_members_and_replacement_budget_is_bounded",
        "wildcard_array_rule_does_not_normalize_undeclared_siblings",
        "invalid_rule_and_policy_shapes_are_rejected",
        "declared_kind_must_match_the_value_type",
        "source_normalization_version_must_match_before_any_replacement",
        "replacement_ledger_records_paths_kinds_and_per_kind_counts",
    ] {
        assert!(fixtures.contains(name), "fixture missing: {name}");
        assert!(baseline.contains(name), "baseline missing fixture: {name}");
        assert!(workflow.contains(name), "workflow missing fixture: {name}");
    }
    assert!(workflow.contains("--test eq19_volatile"));
    assert!(workflow.contains("--test eq19_quality_guard"));
    assert!(workflow.contains("--test-threads=1"));
    assert!(!fixtures.contains("#[ignore]"));
    assert!(!workflow.contains("continue-on-error"));
}
