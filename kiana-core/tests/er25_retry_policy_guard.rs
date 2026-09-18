//! ER-25 source guard for bounded retry policy and new attempts.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-25 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn retries_are_new_bounded_attempts_with_deadline_and_accounting() {
    let model = include_str!("../../kiana-domain/src/model.rs");
    let errors = include_str!("../../kiana-domain/src/errors.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let budget = include_str!("../../kiana-runner/src/budget.rs");
    let ports = include_str!("../../kiana-ports/src/model.rs");
    let ports_lib = include_str!("../../kiana-ports/src/lib.rs");
    let identity = include_str!("../../kiana-domain/src/execution_identity.rs");
    let projection = include_str!("../src/model_attempt_projection.rs");

    require(
        model,
        &[
            "ModelCallSpec",
            "attempt_id",
            "deadline_unix_ms",
            "ModelRetryClass",
            "BeforeSend",
            "Rejected",
            "ModelSideEffectState",
            "retry_after_ms",
            "ModelOutcome",
            "model_authority_revision_drift",
        ],
        "model retry contract",
    );
    require(
        errors,
        &[
            "CapabilityErrorPolicy",
            "retryable",
            "requires_new_authorization",
            "ResultUnknown",
        ],
        "capability error policy",
    );
    require(
        harness,
        &[
            "for attempt in 0..3u32",
            "ModelAttemptIdentity",
            "reserve_attempt",
            "settle_attempt",
            "ModelRetryClass::BeforeSend",
            "ModelRetryClass::Rejected",
            "model_retry_deadline_exceeded",
            "model_attempt_limit",
            "retry_after_ms",
            "attempted",
        ],
        "Harness attempt driver",
    );
    require(
        budget,
        &[
            "max_attempts_per_task",
            "reserve_attempt",
            "settle_attempt",
            "unknown_attempts",
        ],
        "attempt budget",
    );
    require(
        ports,
        &["Exactly one attempt", "complete_admitted"],
        "provider attempt port",
    );
    require(ports_lib, &["ModelBudgetPort"], "model admission port");
    require(
        identity,
        &["ModelAttemptIdentity", "attempt", "identity_digest"],
        "attempt identity",
    );
    require(
        projection,
        &[
            "parse_retry_class",
            "retry_class",
            "attempt",
            "model_attempt_id",
        ],
        "attempt projection",
    );
}

#[test]
fn unknown_or_denied_effects_never_auto_retry_and_exhaustion_is_terminal() {
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let errors = include_str!("../../kiana-domain/src/errors.rs");
    let capabilities = include_str!("../src/capabilities.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let h05 = include_str!("../../kiana-runner/tests/h05_stop_guard.rs");
    let h07 = include_str!("../../kiana-runner/tests/h07_budget_guard.rs");
    let h08 = include_str!("../../kiana-runner/tests/h08_cancellation_guard.rs");
    let h11 = include_str!("../../kiana-runner/tests/h11_tool_observation_guard.rs");

    require(
        errors,
        &[
            "Unknown",
            "requires_reconciliation",
            "never permits automatic retry",
            "retryable: $retry",
        ],
        "non-retryable effect policy",
    );
    require(
        harness,
        &[
            "model_attempt_limit",
            "model_retry_deadline_exceeded",
            "ModelRetryClass::BeforeSend",
            "ModelRetryClass::Rejected",
        ],
        "retry exhaustion",
    );
    require(
        capabilities,
        &["CapabilityErrorCode::ResultUnknown", "fenced"],
        "capability Unknown fence",
    );
    require(
        dispatch,
        &[
            "result_unknown",
            "effect_known",
            "fenced",
            "execution_permit_already_consumed",
        ],
        "dispatch admission",
    );
    require(
        h05,
        &["ModelRetryClass::BeforeSend", "ModelRetryClass::Rejected"],
        "H05",
    );
    require(
        h07,
        &["reserve_attempt", "settle_attempt", "provider_retry_free"],
        "H07",
    );
    require(
        h08,
        &[
            "cancel_during_retry_wait_prevents_next_attempt",
            "retry_after_cancel",
        ],
        "H08",
    );
    require(
        h11,
        &[
            "unknown_result_never_triggers_automatic_retry",
            "unknown_effect_auto_retry",
        ],
        "H11",
    );
    for source in [harness, errors, capabilities, dispatch] {
        for forbidden in [
            "unknown_effect_auto_retry",
            "retry_unknown_effect",
            "retry_after_cancel",
            "retry_without_idempotency",
            "unknown_result_auto_retry",
        ] {
            assert!(
                !source.contains(forbidden),
                "ER-25 retry bypass marker present: {forbidden}"
            );
        }
    }
}
