//! P4-J7-18 Ollama NDJSON framing fixtures.
//!
//! These fixtures replay already-captured Ollama `/api/chat` bytes through the same
//! `Framer` + `Accumulator` boundary the live transport uses, so line-by-line decoding,
//! the `done` terminal gate and load/generation timing are exercised end to end without
//! opening a connection. A terminal frame is the only way to finish a run; EOF before
//! `done=true` stays incomplete and is never retried automatically.
use kiana_domain::{
    ModelCallSpec, ModelDelta, ModelMessage, ModelProtocol, ModelPurpose, ModelRequest,
    ModelResponseFormat, ModelRetryClass, ModelRoute, ModelSideEffectState, PreparedModelCall,
    RequestId, TokenBudget,
};
use kiana_provider::replay_stream_fixture;

fn prepared(tools: Vec<serde_json::Value>) -> PreparedModelCall {
    let request = ModelRequest {
        messages: vec![ModelMessage::user("offline ollama ndjson fixture")],
        tools: tools.clone(),
        sandbox: "read-only".to_owned(),
    };
    let mut prepared = PreparedModelCall {
        schema: kiana_domain::MODEL_CALL_SCHEMA.to_owned(),
        spec: ModelCallSpec {
            call_id: RequestId::new(),
            attempt_id: RequestId::new(),
            model_attempt_id: None,
            step_id: None,
            step: 1,
            purpose: ModelPurpose::Task,
            assignment: None,
            response_format: ModelResponseFormat::Text,
            replay: Vec::new(),
            deadline_unix_ms: u64::MAX,
        },
        route: ModelRoute {
            provider_id: "ollama".to_owned(),
            protocol: ModelProtocol::OllamaChat,
            connection_id: "fixture-connection".to_owned(),
            model_id: "local-fixture".to_owned(),
            profile: "fixture".to_owned(),
            configuration_revision: "fixture.v1".to_owned(),
            streaming: true,
        },
        request: request.clone(),
        wire_body: serde_json::json!({"model": "local-fixture", "stream": true, "messages": []}),
        request_hash: String::new(),
        budget: TokenBudget::new(1, 0, 0, 16, 4_096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
        provider_account: None,
        credential_revision: None,
    };
    prepared.seal();
    prepared
}

fn chunks(payload: &str, width: usize) -> Vec<Vec<u8>> {
    payload
        .as_bytes()
        .chunks(width)
        .map(|chunk| chunk.to_vec())
        .collect()
}

fn text_deltas(deltas: &[ModelDelta]) -> String {
    deltas
        .iter()
        .filter_map(|delta| match delta {
            ModelDelta::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

const OLLAMA_TEXT_STREAM: &str = concat!(
    "{\"model\":\"local-fixture\",\"created_at\":\"2026-10-17T00:00:00Z\",\"message\":{\"role\":\"assistant\",\"content\":\"working \"},\"done\":false}\n",
    "{\"model\":\"local-fixture\",\"created_at\":\"2026-10-17T00:00:01Z\",\"message\":{\"role\":\"assistant\",\"content\":\"now\"},\"done\":false,\"prompt_eval_count\":7,\"eval_count\":2}\n",
    "{\"model\":\"local-fixture\",\"created_at\":\"2026-10-17T00:00:02Z\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":7,\"eval_count\":5,\"load_duration\":1000,\"eval_duration\":2000}\n",
);

#[test]
fn ollama_ndjson_split_frames_stream_text_before_the_terminal_done_frame() {
    for width in [1usize, 2, 3, 7, 31, 127] {
        let (reply, deltas) = replay_stream_fixture(
            prepared(Vec::new()),
            &chunks(OLLAMA_TEXT_STREAM, width),
            2_048,
        )
        .expect("ollama ndjson replay");
        assert_eq!(text_deltas(&deltas), "working now", "chunk width {width}");
        assert_eq!(reply.output.text, "working now");
        let usage = reply.output.usage.expect("terminal usage is retained");
        assert_eq!((usage.input_tokens, usage.output_tokens), (7, 5));
        let timing = reply.provider_timing.expect("ollama timing observation");
        assert_eq!(timing.load_duration_ns, Some(1_000));
        assert_eq!(timing.generation_duration_ns, Some(2_000));
    }
}

#[test]
fn ollama_ndjson_eof_without_done_never_completes_a_run() {
    for payload in [
        concat!(
            "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"partial\"},\"done\":false}\n",
            "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\" more\"},\"done\":false}"
        ),
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"partial\"},\"done\":false}\n",
    ] {
        let error = replay_stream_fixture(prepared(Vec::new()), &chunks(payload, 5), 2_048)
            .expect_err("a stream without a terminal done frame is incomplete");
        assert_eq!(error.code, "provider_stream_incomplete");
        assert_eq!(error.retry_class, ModelRetryClass::Never);
        assert!(
            error.request_sent,
            "EOF after dispatch must keep the sent request state"
        );
        assert_eq!(error.side_effect_state, ModelSideEffectState::Unknown);
    }
}

#[test]
fn ollama_ndjson_unterminated_terminal_line_completes_within_frame_limit() {
    let payload = concat!(
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"done\"},\"done\":false}\n",
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\"}"
    );
    let (reply, deltas) = replay_stream_fixture(prepared(Vec::new()), &chunks(payload, 3), 2_048)
        .expect("the bounded flush keeps the final unterminated line");
    assert_eq!(text_deltas(&deltas), "done");
    assert_eq!(reply.output.text, "done");
    assert!(reply.output.usage.is_none());
    assert!(reply.provider_timing.is_none());
}

#[test]
fn ollama_tool_ordinals_stay_stable_across_frame_boundaries() {
    let payload = concat!(
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"calling\"},\"done\":false}\n",
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[",
        "{\"function\":{\"name\":\"shell\",\"arguments\":{\"command\":\"pwd\"}}},",
        "{\"function\":{\"name\":\"shell\",\"arguments\":{\"command\":\"ls\"}}},",
        "{\"id\":\"native-call-9\",\"function\":{\"name\":\"shell\",\"arguments\":{\"command\":\"whoami\"}}}",
        "]},\"done\":true,\"done_reason\":\"stop\"}\n"
    );
    for width in [1usize, 5, 64] {
        let prepared = prepared(kiana_domain::tool_schemas());
        let call_id = prepared.spec.call_id.clone();
        let (reply, _) = replay_stream_fixture(prepared, &chunks(payload, width), 4_096)
            .expect("ollama tool replay");
        let calls = &reply.output.tool_calls;
        assert_eq!(calls.len(), 3, "chunk width {width}");
        assert_eq!(calls[0].id, format!("ollama:{call_id}:0"));
        assert_eq!(calls[1].id, format!("ollama:{call_id}:1"));
        assert_ne!(calls[0].id, calls[1].id);
        assert_eq!(
            calls[2].id, "native-call-9",
            "a wire id that is present is kept as-is"
        );
        assert!(calls.iter().all(|call| call.name == "shell"));
        assert_eq!(calls[0].arguments, serde_json::json!({"command": "pwd"}));
        assert_eq!(calls[1].arguments, serde_json::json!({"command": "ls"}));
        assert_eq!(calls[2].arguments, serde_json::json!({"command": "whoami"}));
    }
}

#[test]
fn ollama_ndjson_error_frame_never_completes_and_keeps_no_partial_reply() {
    let payload = concat!(
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"partial \"},\"done\":false}\n",
        "{\"error\":\"model requires more system memory than is available\"}\n"
    );
    let error = replay_stream_fixture(prepared(Vec::new()), &chunks(payload, 13), 2_048)
        .expect_err("an Ollama error frame must not produce a reply");
    assert_eq!(error.code, "provider_stream_error");
    assert_eq!(error.retry_class, ModelRetryClass::Never);
    assert!(error.request_sent);
}

#[test]
fn ollama_ndjson_done_true_without_done_reason_is_not_a_terminal_completion() {
    let payload = concat!(
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"nearly\"},\"done\":false}\n",
        "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n"
    );
    let error = replay_stream_fixture(prepared(Vec::new()), &chunks(payload, 11), 2_048)
        .expect_err("done=true without done_reason must not complete the run");
    assert_eq!(error.code, "provider_stop_reason_missing");
    assert_eq!(error.retry_class, ModelRetryClass::Never);
}

#[test]
fn ollama_tool_arguments_fail_closed_before_any_call_is_usable() {
    for (payload, code) in [
        (
            concat!(
                "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"\",",
                "\"tool_calls\":[{\"function\":{\"name\":\"shell\",\"arguments\":[1,2]}}]},",
                "\"done\":true,\"done_reason\":\"stop\"}\n"
            ),
            "provider_tool_json_object_required",
        ),
        (
            concat!(
                "{\"model\":\"local-fixture\",\"message\":{\"role\":\"assistant\",\"content\":\"\",",
                "\"tool_calls\":[{\"function\":{\"name\":\"shell\",",
                "\"arguments\":{\"command\":\"pwd\",\"command\":\"rm -rf /\"}}}]},",
                "\"done\":true,\"done_reason\":\"stop\"}\n"
            ),
            "provider_frame_json_invalid",
        ),
    ] {
        let error = replay_stream_fixture(
            prepared(kiana_domain::tool_schemas()),
            &chunks(payload, 7),
            2_048,
        )
        .expect_err("malformed tool arguments must fail closed");
        assert_eq!(error.code, code);
        assert!(error.request_sent);
    }
}
