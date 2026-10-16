//! H05 CLI presenter fixture：不同停止/恢复 disposition 不能在错误输出里折叠成同一行。
//!
//! 本测试直接调用 `kiana-entrypoints` 的呈现器与 `kiana-protocol` 的响应 DTO，不启动
//! daemon、不执行模型；它证明输出映射不会把失败变成功，也不证明真实 provider 失败、
//! 自动修复执行或进程退出码已完成端到端接线。

use kiana_entrypoints::model_failure_presenter::{failure_suffix, render_model_failure};
use kiana_protocol::{ExecutionStatus, ModelFailureSurface, RequestId, ResponseEnvelope};
use serde_json::Value;
use std::collections::BTreeSet;

fn failed(error: &str) -> ResponseEnvelope {
    ResponseEnvelope {
        schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status: ExecutionStatus::Failed,
        output: Value::Null,
        error: Some(error.to_owned()),
    }
}

#[test]
fn distinct_model_dispositions_render_distinct_cli_lines() {
    let cases = [
        ("model_output_truncated", "terminal"),
        ("model_refused", "terminal"),
        ("model_pause_requires_explicit_continue", "terminal"),
        ("model_transport_incomplete", "terminal"),
        ("model_stop_reason_unknown", "terminal"),
        ("model_format_repair_unavailable", "format_repair"),
        ("model_tool_repair_unavailable", "tool_repair"),
        ("model_context_repair_unavailable", "context_repair"),
    ];
    let mut rendered = Vec::new();
    for (code, disposition) in cases {
        let suffix = failure_suffix(&failed(code));
        assert!(suffix.contains(&format!("code={code}")), "{code}: {suffix}");
        assert!(suffix.contains(&format!("disposition={disposition}")), "{code}: {suffix}");
        assert!(suffix.contains("retry=never"), "{code}: {suffix}");
        assert!(suffix.contains("retryable=false"), "{code}: {suffix}");
        rendered.push(suffix);
    }
    let unique: BTreeSet<&str> = rendered.iter().map(String::as_str).collect();
    assert_eq!(unique.len(), rendered.len(), "distinct failures must not collapse: {rendered:?}");
    assert_ne!(
        failure_suffix(&failed("model_output_truncated")),
        failure_suffix(&failed("model_format_repair_unavailable")),
        "terminal truncation and format repair are different dispositions"
    );
}

#[test]
fn unknown_failures_render_a_generic_code_with_explicit_no_retry() {
    let surface = ModelFailureSurface::from_code("model_surprise_stop");
    let line = render_model_failure(&surface).expect("unclassified surfaces still render");
    assert!(line.contains("code=model_failure_unclassified"), "{line}");
    assert!(line.contains("disposition=terminal"), "{line}");
    assert!(line.contains("retry=never"), "{line}");
    assert!(line.contains("retryable=false"), "{line}");
}

#[test]
fn non_model_failures_keep_the_generic_cli_error_path() {
    assert_eq!(failure_suffix(&failed("project_untrusted")), "");
    let completed = ResponseEnvelope {
        schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status: ExecutionStatus::Completed,
        output: Value::Null,
        error: None,
    };
    assert_eq!(failure_suffix(&completed), "");
}

#[test]
fn cli_and_harness_error_outputs_route_through_the_presenter() {
    let cli = include_str!("../src/cli.rs");
    assert!(
        cli.matches("model_failure_presenter::failure_suffix(&response)")
            .count()
            >= 2,
        "print-mode JSON and human failure lines must both annotate the classification"
    );
    let harness_run = include_str!("../src/harness_run.rs");
    assert!(
        harness_run.contains("model_failure_presenter::failure_suffix(&response)"),
        "the owned-harness result adapter must keep the classification"
    );
    let presenter = include_str!("../src/model_failure_presenter.rs");
    for forbidden in [
        "DaemonHost",
        "ControlPlane",
        "CapabilityBroker",
        "KianaHarness",
        "tokio",
        "std::process",
        "std::fs::",
    ] {
        assert!(
            !presenter.contains(forbidden),
            "presenter gained execution authority: {forbidden}"
        );
    }
}
