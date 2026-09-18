use kiana_daemon::eval_runtime::{FakeProviderAdapter, FakeProviderScenario};
use kiana_domain::{ModelDelta, ModelRequest};
use kiana_ports::ModelClient;
use serde_json::json;

fn request() -> ModelRequest {
    ModelRequest {
        messages: Vec::new(),
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    }
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
    let text = deltas
        .into_iter()
        .filter_map(|delta| match delta {
            ModelDelta::Text { text } => Some(text),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(text, output.text);
    assert_eq!(text, "deterministic stream");
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
