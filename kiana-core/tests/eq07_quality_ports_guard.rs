#[test]
fn quality_port_traits_remain_below_core_and_have_no_provider_or_filesystem_dependency() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let core = include_str!("../src/lib.rs");
    for marker in [
        "EvalStore",
        "FixtureStore",
        "TraceSource",
        "ArtifactReader",
        "Judge",
        "MetricsSink",
        "Clock",
        "eval_store_unsupported",
        "fixture_store_unsupported",
        "trace_source_unsupported",
        "artifact_reader_unsupported",
        "judge_unsupported",
        "metrics_sink_unsupported",
    ] {
        assert!(ports.contains(marker), "port marker missing: {marker}");
    }
    for forbidden in ["kiana_daemon", "kiana_provider", "PathBuf", "reqwest"] {
        assert!(
            !ports.contains(forbidden),
            "forbidden quality port dependency: {forbidden}"
        );
    }
    assert!(!core.contains("struct QualityPortExecutionLoop"));
}
