use async_trait::async_trait;
use kiana_domain::{ArtifactRef, EvalCase, EvalCaseResult, GoldenTrace, RunId, RuntimeEvent};
use kiana_ports::{
    ArtifactReader, EvalStore, FixtureStore, Judge, MetricsSink, PortError, TraceSource,
};

struct CompileOnlyQualityPorts;

impl EvalStore for CompileOnlyQualityPorts {}

#[async_trait]
impl FixtureStore for CompileOnlyQualityPorts {
    async fn read_fixture(&self, _: &str, _: &str) -> Result<Vec<u8>, PortError> {
        Err(PortError::Unavailable(
            "fixture_store_unsupported".to_owned(),
        ))
    }
}

#[async_trait]
impl TraceSource for CompileOnlyQualityPorts {
    async fn read_trace_events(
        &self,
        _: RunId,
        _: u64,
        _: usize,
    ) -> Result<Vec<RuntimeEvent>, PortError> {
        Err(PortError::Unavailable(
            "trace_source_unsupported".to_owned(),
        ))
    }
}

#[async_trait]
impl ArtifactReader for CompileOnlyQualityPorts {
    async fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        Err(PortError::Unavailable(
            "artifact_reader_unsupported".to_owned(),
        ))
    }
}

#[async_trait]
impl Judge for CompileOnlyQualityPorts {
    async fn judge(&self, _: &EvalCase, _: &GoldenTrace) -> Result<serde_json::Value, PortError> {
        Err(PortError::Unavailable("judge_unsupported".to_owned()))
    }
}

#[async_trait]
impl MetricsSink for CompileOnlyQualityPorts {
    async fn record_eval_result(&self, _: &EvalCaseResult) -> Result<(), PortError> {
        Err(PortError::Unavailable(
            "metrics_sink_unsupported".to_owned(),
        ))
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
