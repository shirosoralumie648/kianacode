//! H05 模型失败呈现器：把 wire 分类变成 CLI/SDK 错误行上的一段稳定文本。
//!
//! CLI、SDK 与后续界面都通过这一个模块打印停止/恢复分类，避免两个 surface 把
//! `format_repair` 失败和 `terminal` 截断折叠成同一行。呈现器只做展示：不持有重试状态、
//! 不做 I/O、不把失败变成功；未知错误码由 protocol 层落到 `model_failure_unclassified`
//! 并显式给出 `terminal`/`never` disposition。
//!
//! 证明上限：`source`。渲染一次分类不证明失败确实发生、修复已执行或重试会成功。

use kiana_protocol::{
    ModelFailureSurface, ModelRecoveryDisposition, ModelRetryClass, ResponseEnvelope,
};

/// 呈现器稳定标识；后续界面 hydrate 时用它区分展示版本。
pub const MODEL_FAILURE_PRESENTER_SCHEMA: &str = "kiana.model-failure-presenter.v1";

/// 渲染终态 surface 打印的有界分类行。
///
/// 该行始终带 wire 码、恢复 disposition、retry class 与显式 `retryable` 标记，
/// 因此不同 disposition 不会渲染成同一段文本。
pub fn render_model_failure(failure: &ModelFailureSurface) -> Result<String, String> {
    failure.validate()?;
    Ok(format!(
        "[model-failure code={} disposition={} retry={} retryable={}]",
        failure.wire_code,
        disposition_token(failure.disposition),
        retry_token(failure.retry_class),
        failure.retryable
    ))
}

/// 给 CLI 错误行追加模型失败分类；非模型失败返回空串。
///
/// 非模型失败保持原有错误文本与退出行为不变；模型失败在原文后追加一条
/// [`render_model_failure`] 行，供人和脚本同时读取稳定的失败码与 disposition。
pub fn failure_suffix(response: &ResponseEnvelope) -> String {
    response
        .model_failure()
        .and_then(|failure| render_model_failure(&failure).ok())
        .map(|line| format!(" {line}"))
        .unwrap_or_default()
}

/// disposition 的 wire 拼写与 `ModelRecoveryDisposition` 的 serde 名一致。
fn disposition_token(disposition: ModelRecoveryDisposition) -> &'static str {
    match disposition {
        ModelRecoveryDisposition::TransportRetry => "transport_retry",
        ModelRecoveryDisposition::FormatRepair => "format_repair",
        ModelRecoveryDisposition::ToolRepair => "tool_repair",
        ModelRecoveryDisposition::ContextRepair => "context_repair",
        ModelRecoveryDisposition::Terminal => "terminal",
    }
}

/// retry class 的 wire 拼写与 `ModelRetryClass` 的 serde 名一致。
fn retry_token(retry_class: ModelRetryClass) -> &'static str {
    match retry_class {
        ModelRetryClass::Never => "never",
        ModelRetryClass::BeforeSend => "before_send",
        ModelRetryClass::Rejected => "rejected",
    }
}
