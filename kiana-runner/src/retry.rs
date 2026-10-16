use kiana_domain::{
    ModelError, ModelRecoveryDisposition, ModelRetryClass, ModelSideEffectState, RequestId,
    RetryDecision, RetryObservation, RetryPolicy,
};
use std::time::Duration;

// 单次 invoke_model 最多尝试 3 次，包含首次调用，因此最多重试 2 次。
// 这是 attempt 数量上限，不是跨 run 的 provider 总配额。
pub(crate) const MAX_PROVIDER_ATTEMPTS: u32 = 3;

// ===== 重试决策的三层检查 =====
//
// 1. is_safe_to_retry() 只检查错误分类是否具备重试条件：
//    recovery_disposition 必须为 TransportRetry，phase 必须为 transport；
//    还需满足 domain RetryPolicy 的 pre-send 或明确 408/429 拒绝条件。
//    side_effect_state 为 None 不单独证明请求未发送；Unknown 或已收到 delta 都拒绝重试。
// 2. classify_retry() 使用调用方提供的 attempt/request_count、幂等性和绝对 deadline 决策。
//    FormatRepair、ToolRepair、ContextRepair 不允许重试；具体的 *_repair_unavailable
//    错误由 harness 调用 unsupported_recovery_reason() 后返回，不是分类器的返回值。
// 3. retry_delay() 计算指数退避与 RequestId 派生的确定性 jitter，且不缩短 Retry-After。
//    harness 还会核对含 jitter 的延迟是否小于剩余时间，并在等待中监听取消。

/// 快速检查错误分类和已观察 delta 是否允许重试；true 不是执行许可。
///
/// 此处使用固定 attempt/request_count/now 和 `idempotent=true` 探测分类规则，
/// 不读取真实次数或时钟。调用方仍须通过 `classify_retry()` 检查实际预算、幂等性和
/// deadline，并在下一次 attempt 重新走准入。Unknown、副作用证据不符、已收到 delta
/// 或未接入的修复类型都会被拒绝。
pub(crate) fn is_safe_to_retry(error: &ModelError, observed_delta: bool) -> bool {
    error.recovery_disposition == ModelRecoveryDisposition::TransportRetry
        && error.phase == "transport"
        && RetryPolicy::new(
            MAX_PROVIDER_ATTEMPTS,
            MAX_PROVIDER_ATTEMPTS,
            5_000,
            u64::MAX,
        )
        .ok()
        .and_then(|policy| {
            policy
                .classify(
                    1,
                    1,
                    1,
                    &RetryObservation::from_model_error(error, observed_delta, true),
                )
                .ok()
        })
        .is_some_and(|decision| decision.retry)
}

/// 将当前未接入的修复类型映射成稳定错误码。
///
/// FormatRepair、ToolRepair、ContextRepair 分别返回格式、工具和上下文修复
/// unavailable；本函数不分析原始错误，也不推断缺少哪种修复策略。
/// TransportRetry 和 Terminal 返回 None，由调用方继续按原始错误处理。
pub(crate) fn unsupported_recovery_reason(error: &ModelError) -> Option<&'static str> {
    match error.recovery_disposition {
        ModelRecoveryDisposition::FormatRepair => Some("model_format_repair_unavailable"),
        ModelRecoveryDisposition::ToolRepair => Some("model_tool_repair_unavailable"),
        ModelRecoveryDisposition::ContextRepair => Some("model_context_repair_unavailable"),
        ModelRecoveryDisposition::TransportRetry | ModelRecoveryDisposition::Terminal => None,
    }
}

/// 使用共享的域级重试分类器，与 ControlPlane 和 Transport 采用相同的 deadline。
///
/// 这是一个**纯功能函数**：给定错误、当前尝试次数、现在的时间、deadline，
/// 返回一个 `RetryDecision`，说「现在应该重试吗？」。
///
/// harness 循环本身的所有者是本 crate（只有 runner），所以这个函数**只返回决策值**，
/// 不负责实际的重试逻辑（等待、重新提交等）。
///
/// ## 参数详解
/// - `error`：模型返回的错误信息
/// - `observed_delta`：本次调用是否收到过任意 ModelDelta，不等于工具副作用已执行
/// - `attempt`：这是第几次尝试（从 1 开始）
/// - `request_count`：调用方提供的请求额度计数；当前 harness 与 attempt 一样传入
///   本次 invoke_model 内从 1 开始的尝试序号，包含发送前失败，并非实际网络发送次数
/// - `now_unix_ms`：当前时间（Unix 毫秒时间戳）
/// - `deadline_unix_ms`：必须完成的绝对 deadline（超过就放弃）
/// - `idempotent`：这个请求是否**幂等**（重复执行不会产生重复效果）
///
/// ## 返回值
/// - `Ok(RetryDecision { retry: true, ... })`：可以重试
/// - `Ok(RetryDecision { retry: false, ... })`：应该放弃
/// - `Err(...)`：策略或错误观察字段不合法；例如 deadline 为零、phase/code 格式无效
pub(crate) fn classify_retry(
    error: &ModelError,
    observed_delta: bool,
    attempt: u32,
    request_count: u32,
    now_unix_ms: u64,
    deadline_unix_ms: u64,
    idempotent: bool,
) -> Result<RetryDecision, String> {
    let policy = RetryPolicy::new(
        MAX_PROVIDER_ATTEMPTS,
        MAX_PROVIDER_ATTEMPTS,
        5_000,
        deadline_unix_ms,
    )?;
    policy.classify(
        attempt,
        request_count,
        now_unix_ms,
        &RetryObservation::from_model_error(error, observed_delta, idempotent),
    )
}

/// 根据错误类型、重试次数和 RequestId 计算**应该等多长**。
///
/// 返回值是一个 `Duration`，harness 应该在发起下一次请求前等待这么长时间。
///
/// ## 退避算法（Exponential Backoff with Jitter）
///
/// 基础公式：`backoff_ms = min(100 * 2^min(retry_index, 5), 5000)`。
/// 指数先截在 5，所以当前可达到的最大基础退避是 3200ms，5000ms 防护不会被触发。
/// retry_index 从 0 开始，依次得到 100、200、400、800、1600、3200ms；之后仍为 3200ms。
/// 当前 invoke_model 最多重试两次，正常调用只用到 index 0 和 1。
///
/// 再取 `max(backoff_ms, error.retry_after_ms.unwrap_or_default())`，不缩短服务端
/// Retry-After；最后饱和加上 0..=100ms 的 jitter。因此总等待时间不以 5000ms 为上限。
///
/// jitter 由 attempt_id 前两个字节确定，同一 ID 的结果相同，不是每次重新抽样。
/// 不同 ID 可分散同时重试，但不保证没有碰撞；此函数也不检查 deadline 或取消。
pub(crate) fn retry_delay(error: &ModelError, retry_index: u32, attempt_id: RequestId) -> Duration {
    let exponent = retry_index.min(5); // 指数上界设在 5（防止 2^large 溢出）
    let backoff_ms = 100u64.saturating_mul(1u64 << exponent).min(5_000); // 当前指数截断使基础退避最多 3200ms
    let server_delay_ms = error.retry_after_ms.unwrap_or_default(); // 服务器建议的延迟（如有）
    let base_ms = backoff_ms.max(server_delay_ms); // 取较大值：尊重服务器的 Retry-After
    let bytes = attempt_id.as_uuid().as_bytes().to_owned(); // 从 UUID 提取 jitter 源
    let jitter_ms = (u16::from(bytes[0]) << 8 | u16::from(bytes[1])) % 101; // 0~100ms jitter
    Duration::from_millis(base_ms.saturating_add(u64::from(jitter_ms)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_requires_typed_pre_send_or_explicit_rejection_without_unknown_effect() {
        let before_send =
            ModelError::transport("connect_failed", ModelRetryClass::BeforeSend, false);
        assert!(is_safe_to_retry(&before_send, false));

        let mut rejected =
            ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
        rejected.side_effect_state = ModelSideEffectState::None;
        assert!(is_safe_to_retry(&rejected, false));
        rejected.code = "provider_http_500".to_owned();
        assert!(!is_safe_to_retry(&rejected, false));
        rejected.code = "provider_http_429".to_owned();

        let unknown =
            ModelError::transport("response_lost_after_send", ModelRetryClass::Never, true);
        assert!(!is_safe_to_retry(&unknown, false));
        assert!(!is_safe_to_retry(&rejected, true));

        rejected.side_effect_state = ModelSideEffectState::Unknown;
        assert!(!is_safe_to_retry(&rejected, false));
    }

    #[test]
    fn retry_delay_never_shortens_retry_after_and_is_bounded_jitter() {
        let mut error = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
        error.side_effect_state = ModelSideEffectState::None;
        error.retry_after_ms = Some(8_000);

        let delay = retry_delay(&error, 0, RequestId::new());
        assert!((Duration::from_millis(8_000)..=Duration::from_millis(8_100)).contains(&delay));
    }

    #[test]
    fn recovery_dispositions_only_retry_transport_and_refuse_unwired_repairs() {
        let mut transport =
            ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
        transport.side_effect_state = ModelSideEffectState::None;
        assert!(is_safe_to_retry(&transport, false));
        assert_eq!(unsupported_recovery_reason(&transport), None);

        for (disposition, reason) in [
            (
                ModelRecoveryDisposition::FormatRepair,
                "model_format_repair_unavailable",
            ),
            (
                ModelRecoveryDisposition::ToolRepair,
                "model_tool_repair_unavailable",
            ),
            (
                ModelRecoveryDisposition::ContextRepair,
                "model_context_repair_unavailable",
            ),
        ] {
            let error =
                ModelError::invalid("repair_requested").with_recovery_disposition(disposition);
            assert!(!is_safe_to_retry(&error, false));
            assert_eq!(unsupported_recovery_reason(&error), Some(reason));
        }

        let terminal = ModelError::invalid("terminal_failure");
        assert!(!is_safe_to_retry(&terminal, false));
        assert_eq!(unsupported_recovery_reason(&terminal), None);
    }
}
