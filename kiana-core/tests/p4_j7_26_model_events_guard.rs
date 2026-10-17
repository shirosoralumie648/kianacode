/// P4-J7-26 守卫：模型事实是带版本的审计载荷，不是请求体、授权或第二条执行路径。
/// 本测试只做源码级标记检查，不调用模型、网络、工具或 EventStore。
#[test]
fn model_event_boundary_has_lifecycle_redaction_and_commit_guards() {
    let model_event = include_str!("../../kiana-domain/src/model_event.rs");
    let registry = include_str!("../../kiana-domain/src/event_contracts.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    for marker in [
        "MODEL_EVENT_SCHEMA",
        "ModelFactCommitment",
        "MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_EFFECT",
        "MODEL_FACT_PERSISTENCE_REQUIRED_BEFORE_COMPLETION",
        "ProviderTraceMetadata",
        "ProviderDeltaLedger",
        "StreamingRedactor",
        "into_runtime_event",
        "model.prepared",
        "model.denied",
        "model.attempt_started",
        "model.retry_scheduled",
        "model.finished",
        "model.usage_correction",
        "model_usage_correction_digest_mismatch",
        "model_fact_append_transition_invalid",
        "pub fn digest(&self) -> String",
        "provider_request_id_status",
        "provider_response_id_status",
        "ModelEventSequences",
        "ui_sequence",
        "upstream_sequence",
        "durable_sequence",
    ] {
        assert!(
            model_event.contains(marker) || registry.contains(marker) || harness.contains(marker),
            "P4-J7-26 marker missing: {marker}"
        );
    }
    for forbidden in [
        "Authorization:",
        "Bearer ",
        "reqwest::Client",
        "tokio::spawn",
        "std::net::TcpStream",
    ] {
        assert!(
            !model_event.contains(forbidden),
            "model fact boundary contains provider effect or secret marker: {forbidden}"
        );
    }
    // 模型事实只能携带摘要和引用；带原始请求/提示/密钥的字段会让 transcript 变成第二事实源。
    for forbidden in ["pub prompt:", "pub request:", "pub api_key:", "pub secret:"] {
        assert!(
            !model_event.contains(forbidden),
            "model fact persists raw prompt/credential material: {forbidden}"
        );
    }
}

/// 本卡的拒绝/成功夹具与手动 lane 必须保持接线，且域层边界不得自行打开副作用。
#[test]
fn model_event_fixtures_are_wired_into_the_manual_lane_and_keep_effect_boundaries() {
    let fixtures = include_str!("../../kiana-domain/tests/p4_j7_26_model_events.rs");
    let model_event = include_str!("../../kiana-domain/src/model_event.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-26-model-events-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-26-model-events.yml");

    for fixture in [
        "provider_secrets_split_across_deltas_are_redacted",
        "provider_error_body_cannot_leak_authorization",
        "model_fact_persistence_failure_prevents_tool_dispatch",
        "model_attempt_to_invocation_receipt_chain_is_complete",
        "provider_trace_records_actual_model_and_config_version",
        "provider_delta_ledger_granularity_remains_bounded",
        "every_registered_model_kind_round_trips_through_the_event_registry",
    ] {
        assert!(
            fixtures.contains(fixture),
            "P4-J7-26 fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "P4-J7-26 baseline does not record fixture: {fixture}"
        );
    }
    assert!(
        !fixtures.contains("#[ignore"),
        "P4-J7-26 fixtures must not be ignored"
    );
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "reqwest",
        "TcpListener",
        "ProviderGateway",
        "EventStorePort",
        "CapabilityBroker",
    ] {
        assert!(
            !model_event.contains(forbidden) && !fixtures.contains(forbidden),
            "model fact boundary opened an effect surface: {forbidden}"
        );
    }

    // 手动 lane 必须只读、先 fetch/fmt，再跑本卡两个 target 并编译改动 crate。
    assert!(!workflow.contains("push:"), "manual lane must not auto-run");
    assert!(workflow.contains("workflow_dispatch"));
    assert!(workflow.contains("contents: read"));
    assert!(workflow.contains("cargo fetch --locked"));
    assert!(workflow.contains("cargo fmt --all --check"));
    assert!(workflow.contains("cargo test -p kiana-domain --test p4_j7_26_model_events"));
    assert!(workflow.contains("cargo test -p kiana-core --test p4_j7_26_model_events_guard"));
    assert!(workflow.contains("cargo check -p kiana-domain -p kiana-core --tests --locked"));
    assert!(workflow.contains("--test-threads=1"));
}
