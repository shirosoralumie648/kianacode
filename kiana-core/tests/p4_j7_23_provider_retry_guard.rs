#[test]
fn p4_j7_23_keeps_retry_single_layer_deadline_and_unknown_effect_boundaries() {
    let provider = include_str!("../../kiana-provider/src/transport.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let retry = include_str!("../../kiana-runner/src/retry.rs");
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let config = include_str!("../../kiana-provider/src/config.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-23-provider-retry-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-23-provider-retry.yml");

    for marker in [
        "parse_retry_after(value: &str, now: SystemTime)",
        "ModelRetryClass::Rejected",
        "ModelSideEffectState::None",
        "retry_after_http_date_uses_injected_clock",
        "tls_failure_is_not_transient",
    ] {
        assert!(
            provider.contains(marker),
            "provider retry boundary missing {marker}"
        );
    }
    for marker in [
        "MAX_PROVIDER_ATTEMPTS",
        "is_safe_to_retry(&error, observed_delta)",
        "retry_delay(&error, attempt, attempt_id)",
        "remaining.saturating_sub(started.elapsed())",
        "cancellation.cancelled()",
        "guard.reserve_prepared(&prepared)",
    ] {
        assert!(
            runner.contains(marker),
            "runner retry boundary missing {marker}"
        );
    }
    assert!(
        !runner.contains("reserve_repair(&budget_scope, budget_limits)?"),
        "transport retries must consume the attempt budget, not the separate repair budget"
    );
    for marker in [
        "ModelRetryClass::BeforeSend",
        "ModelRetryClass::Rejected",
        "ModelRetryClass::Never => false",
        "!observed_delta",
    ] {
        assert!(
            retry.contains(marker),
            "typed retry classifier missing {marker}"
        );
    }
    assert!(ports.contains("Exactly one attempt"));
    assert!(config.contains(".retry(reqwest::retry::never())"));
    for marker in [
        "post_send_unknown_is_not_retried_automatically",
        "retryable_429_then_success_records_two_attempts",
        "assert_ne!(attempt_ids[0], attempt_ids[1])",
        "cancel_during_retry_backoff_prevents_next_attempt",
        "oversized_retry_after_does_not_retry_early",
        "observed_delta_prevents_retry_even_for_retryable_rejection",
        "provider_retries_do_not_consume_the_separate_repair_budget",
        "cancelling_mid_stream_never_completes_or_emits_a_late_delta",
        "SDK implicit retries are disabled",
        "cargo test -p kiana-runner --test p4_j7_23_retry",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "evidence missing {marker}"
        );
    }
}
