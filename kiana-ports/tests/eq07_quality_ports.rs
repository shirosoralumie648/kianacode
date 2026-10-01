use kiana_ports::{
    ArtifactReader, Clock, EvalStore, FixtureStore, Judge, MetricsSink, TraceSource,
};

struct CompileOnlyQualityPorts;

impl EvalStore for CompileOnlyQualityPorts {}
impl FixtureStore for CompileOnlyQualityPorts {}
impl TraceSource for CompileOnlyQualityPorts {}
impl ArtifactReader for CompileOnlyQualityPorts {}
impl Judge for CompileOnlyQualityPorts {}
impl MetricsSink for CompileOnlyQualityPorts {}
impl Clock for CompileOnlyQualityPorts {
    fn now_unix_ms(&self) -> u64 {
        0
    }
}

#[test]
fn quality_ports_are_free_of_daemon_or_provider_types() {
    let _ = CompileOnlyQualityPorts;
    let source = include_str!("../src/lib.rs");
    for marker in [
        "pub trait EvalStore",
        "pub trait FixtureStore",
        "pub trait TraceSource",
        "pub trait ArtifactReader",
        "pub trait Judge",
        "pub trait MetricsSink",
        "pub trait Clock",
        "eval_store_unsupported",
        "fixture_store_unsupported",
        "trace_source_unsupported",
        "artifact_reader_unsupported",
        "judge_unsupported",
        "metrics_sink_unsupported",
    ] {
        assert!(
            source.contains(marker),
            "quality port marker missing: {marker}"
        );
    }
    assert!(!source.contains("kiana_daemon"));
    assert!(!source.contains("kiana_provider"));
    assert!(!source.contains("PathBuf"));
    assert!(!source.contains("reqwest"));
}
