use kiana_daemon::eval_runtime::{FakeProviderAdapter, FakeProviderScenario};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{ModelAttemptId, ModelDelta, ModelRequest};
use kiana_ports::ModelClient;
use kiana_protocol::{ExecutionStatus, RequestEnvelope, RequestMetadata};
use kiana_runner::{KianaHarness, ModelStreamAccumulator};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
struct FixedProjectTrustAuthority;

impl ProjectTrustAuthority for FixedProjectTrustAuthority {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn temp_project() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-eq10-host-{stamp}"));
    fs::create_dir_all(&root).expect("project root");
    root
}

fn request() -> ModelRequest {
    ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    }
}

#[tokio::test]
async fn fake_provider_completes_through_daemon_host_control_plane_eventlog() {
    let root = temp_project();
    let provider = Arc::new(
        FakeProviderAdapter::new(FakeProviderScenario::Complete {
            text: "evaluation host reply".to_owned(),
        })
        .unwrap(),
    );
    let host = Arc::new(
        DaemonHost::with_harness_and_project_authority(
            KianaHarness::new(provider.clone()),
            Arc::new(FixedProjectTrustAuthority),
        )
        .expect("daemon host with fake provider"),
    );
    let metadata = {
        let mut metadata = RequestMetadata::local("eq10-session", root.to_string_lossy());
        metadata.project_trusted = true;
        metadata
    };
    let request_id = metadata.request_id;
    let response = host
        .handle(RequestEnvelope::run(
            metadata,
            "return a deterministic evaluation response",
            None,
        ))
        .await;

    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["output"]["text"], "evaluation host reply");
    assert_eq!(
        provider.calls(),
        1,
        "the host path must perform one provider call"
    );
    let run_id = response.output["run_id"].as_str().expect("run id");

    let events = host
        .persisted_events()
        .await
        .expect("event store")
        .expect("memory event store");
    let lifecycle = ["run.authorized", "run.completed", "run.receipt"];
    for kind in lifecycle {
        let event = events
            .iter()
            .find(|event| event.kind == kind && event.data["run_id"] == run_id)
            .unwrap_or_else(|| panic!("missing {kind} for run {run_id}"));
        assert_eq!(event.request_id, request_id, "{kind} request identity");
        assert_eq!(event.correlation_id, Some(request_id), "{kind} correlation");
    }
    let ui_events = host
        .ui_events()
        .await
        .expect("ui event projection")
        .expect("memory ui event projection");
    assert!(ui_events.iter().any(|event| {
        event.kind == "run.completed"
            && event.data["run_id"] == run_id
            && event.request_id == request_id
    }));
    fs::remove_dir_all(root).expect("cleanup project root");
}

#[tokio::test]
async fn fake_provider_replays_complete_reply_without_network() {
    let provider = FakeProviderAdapter::new(FakeProviderScenario::Complete {
        text: "deterministic reply".to_owned(),
    })
    .unwrap();
    let output = provider.complete(request()).await.unwrap();
    assert_eq!(output.text, "deterministic reply");
    assert_eq!(output.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn fake_provider_streaming_complete_emits_non_empty_text_delta() {
    let provider = FakeProviderAdapter::new(FakeProviderScenario::Complete {
        text: "deterministic reply".to_owned(),
    })
    .unwrap();
    let mut deltas = Vec::new();
    let output = provider
        .complete_streaming(request(), &mut |delta| {
            deltas.push(delta);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        deltas,
        vec![ModelDelta::Text {
            text: output.text.clone(),
        }]
    );
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn fake_provider_replays_normalized_stream() {
    let provider = FakeProviderAdapter::new(FakeProviderScenario::Stream {
        chunks: vec![
            "det".to_owned(),
            "erministic ".to_owned(),
            "stream".to_owned(),
        ],
    })
    .unwrap();
    let mut deltas = Vec::new();
    let output = provider
        .complete_streaming(request(), &mut |delta| {
            deltas.push(delta);
            Ok(())
        })
        .await
        .unwrap();
    let mut accumulator = ModelStreamAccumulator::new(ModelAttemptId::new());
    for delta in deltas.iter().cloned() {
        accumulator.push(delta).unwrap();
    }
    let normalized = accumulator.finish(output.clone()).unwrap();
    let text = deltas
        .into_iter()
        .filter_map(|delta| match delta {
            ModelDelta::Text { text } => Some(text),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, output.text);
    assert_eq!(text, "deterministic stream");
    assert_eq!(normalized, output);
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn fake_provider_streaming_malformed_fails_before_emitting_deltas() {
    let provider = FakeProviderAdapter::new(FakeProviderScenario::Malformed {
        reason: "missing final marker".to_owned(),
    })
    .unwrap();
    let mut deltas = Vec::new();
    let error = provider
        .complete_streaming(request(), &mut |delta| {
            deltas.push(delta);
            Ok(())
        })
        .await
        .unwrap_err();
    assert_eq!(error, "fake_provider_malformed:missing final marker");
    assert!(deltas.is_empty());
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn fake_provider_streaming_propagates_callback_error_and_stops_after_failed_delta() {
    let provider = FakeProviderAdapter::new(FakeProviderScenario::Stream {
        chunks: vec!["first".to_owned(), "second".to_owned(), "third".to_owned()],
    })
    .unwrap();
    let mut callback_count = 0;
    let error = provider
        .complete_streaming(request(), &mut |_delta| {
            callback_count += 1;
            if callback_count == 2 {
                Err("consumer_stopped".to_owned())
            } else {
                Ok(())
            }
        })
        .await
        .unwrap_err();
    assert_eq!(error, "consumer_stopped");
    assert_eq!(callback_count, 2);
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn fake_provider_replays_tool_call_without_executing_it() {
    let provider = FakeProviderAdapter::new(FakeProviderScenario::ToolCall {
        id: "call-1".to_owned(),
        name: "read_fixture".to_owned(),
        arguments: json!({"path": "fixture.txt"}),
    })
    .unwrap();
    let output = provider.complete(request()).await.unwrap();
    assert_eq!(output.tool_calls.len(), 1);
    assert_eq!(output.tool_calls[0].id, "call-1");
    assert_eq!(output.tool_calls[0].name, "read_fixture");
    assert_eq!(
        output.tool_calls[0].arguments,
        json!({"path": "fixture.txt"})
    );
    assert_eq!(provider.calls(), 1);
}

#[tokio::test]
async fn fake_provider_surfaces_malformed_stream_and_provider_error() {
    let malformed = FakeProviderAdapter::new(FakeProviderScenario::Malformed {
        reason: "missing final marker".to_owned(),
    })
    .unwrap();
    let error = malformed.complete(request()).await.unwrap_err();
    assert_eq!(error, "fake_provider_malformed:missing final marker");
    assert_eq!(malformed.calls(), 1);

    let failed = FakeProviderAdapter::new(FakeProviderScenario::Error {
        code: "rate_limited".to_owned(),
    })
    .unwrap();
    let error = failed
        .complete_streaming(request(), &mut |_| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(error, "fake_provider_error:rate_limited");
    assert_eq!(failed.calls(), 1);
}

#[test]
fn fake_provider_scenario_is_strict_and_bounded() {
    let unknown = serde_json::from_value::<FakeProviderScenario>(json!({
        "type": "complete",
        "text": "ok",
        "endpoint": "https://example.invalid"
    }));
    assert!(unknown.is_err());
    assert!(FakeProviderAdapter::new(FakeProviderScenario::Stream { chunks: vec![] }).is_err());
    assert!(FakeProviderAdapter::new(FakeProviderScenario::ToolCall {
        id: "call".to_owned(),
        name: "tool".to_owned(),
        arguments: json!("not an object"),
    })
    .is_err());
}

#[test]
fn fake_provider_rejects_empty_stream_chunk() {
    let result = FakeProviderAdapter::new(FakeProviderScenario::Stream {
        chunks: vec!["first".to_owned(), String::new()],
    });
    assert!(matches!(
        result,
        Err(error) if error == "fake_provider_stream_invalid"
    ));
}

#[test]
fn fake_provider_rejects_stream_over_total_text_limit() {
    let oversized = FakeProviderAdapter::new(FakeProviderScenario::Stream {
        chunks: vec!["x".repeat(16 * 1024); 33],
    });
    assert!(matches!(
        oversized,
        Err(error) if error == "fake_provider_stream_invalid"
    ));

    let at_limit = FakeProviderAdapter::new(FakeProviderScenario::Stream {
        chunks: vec!["x".repeat(16 * 1024); 32],
    })
    .expect("exact stream text limit remains valid");
    assert_eq!(at_limit.calls(), 0);
}
