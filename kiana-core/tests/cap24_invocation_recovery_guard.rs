//! CAP-24 source guard for event-derived invocation recovery and reconciliation.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        let present = match *marker {
            "dispatch without a terminal result" => {
                let projection = source_guard::item_source(source, "pub fn project_invocations(");
                source_guard::compact_code(&projection).contains(
                    "ifmatches!(entry.state,CapabilityExecutionState::Dispatching|CapabilityExecutionState::Executing){entry.state=CapabilityExecutionState::Unknown;}"
                )
            }
            // The platform module emits the compact JSON literal directly; the CP-20 guard
            // asserts it through the escaped needle `\"automatic_retry_allowed\":false`.
            "automatic_retry_allowed:false" => {
                let production = source_guard::production_source(source);
                production.contains("\"automatic_retry_allowed\":false")
                    || production.contains(r#"\"automatic_retry_allowed\":false"#)
            }
            "new_request_required:true" => {
                source_guard::production_source(source).contains("\"new_request_required\":true")
            }
            "Redacted arguments/history cannot be used" => {
                let checkpoint = source_guard::compact_code(&source_guard::item_source(
                    source,
                    "pub(crate) async fn checkpoint_run(",
                ));
                let resume = source_guard::compact_code(&source_guard::item_source(
                    source,
                    "pub async fn resume_run(",
                ));
                checkpoint.contains("letmutsafe=redact_event_value(&raw);")
                    && checkpoint.contains("ifsafe!=raw{safe[\"resumable\"]=json!(false);}")
                    && resume.contains(
                        "snapshot_value.get(\"resumable\").and_then(Value::as_bool)!=Some(true)",
                    )
            }
            "Unknown effects retain their resource reservation" => {
                let settle = source_guard::compact_code(&source_guard::item_source(
                    source,
                    "pub(crate) async fn settle_resumed_cell(",
                ));
                settle.contains("ExecutionStatus::ResultUnknown=>CellLifecycle::Quarantined,")
                    && settle.contains("elseifstatus!=ExecutionStatus::ResultUnknown{")
                    && settle.contains(".retire_cell(cell_id,\"run_continuation_terminal\")")
            }
            "never permits automatic retry" => {
                let policy = kiana_domain::CapabilityErrorCode::ResultUnknown.policy();
                !policy.retryable
                    && policy.requires_new_authorization
                    && policy.requires_reconciliation
            }
            _ => source.contains(marker),
        };
        assert!(present, "CAP-24 {label} marker missing: {marker}");
    }
}

#[test]
fn invocation_state_is_rebuilt_from_facts_and_stays_fail_closed() {
    let projection = include_str!("../src/invocation_projection.rs");
    let attempts = include_str!("../src/capability_attempt_projection.rs");
    let recovery = include_str!("../src/recovery.rs");
    let resume = include_str!("../../kiana-domain/src/invocation_resume.rs");
    let event_contracts = include_str!("../../kiana-domain/src/event_contracts.rs");
    let h13 = include_str!("h13_invocation_ledger_guard.rs");
    let er09 = include_str!("er09_invocation_projection.rs");

    require(
        projection,
        &[
            "project_invocations",
            "request_from_event",
            "approval_id",
            "terminal_state",
            "invocation_terminal_conflict",
            "CapabilityExecutionState::Unknown",
            "dispatch without a terminal result",
            "entry.state = CapabilityExecutionState::Unknown",
        ],
        "invocation projection",
    );
    require(
        attempts,
        &[
            "project_capability_attempts",
            "attempt",
            "terminal_digest",
            "CapabilityEffectState::Unknown",
            "effect_known",
            "fenced",
            "source_event",
        ],
        "attempt projection",
    );
    require(
        recovery,
        &[
            "cache_invocation_projection",
            "rebuild_pending_invocation",
            "approval_continuation_unavailable",
            "run_resume_claim_conflict",
            "run_resume_authority_changed",
            "run_resume_data_revoked",
            "run_resume_scope_changed",
            "Redacted arguments/history cannot be used",
            "restore(run_id",
            "Unknown effects retain their resource reservation",
        ],
        "recovery boundary",
    );
    require(
        resume,
        &[
            "InvocationResumeBinding",
            "owner_id",
            "parameter_digest",
            "catalog_digest",
            "validate_against",
            "invocation_resume_binding_changed",
        ],
        "continuation binding",
    );
    require(
        event_contracts,
        &[
            "execution.prepared",
            "execution.result_committed",
            "result_unknown",
        ],
        "event contract",
    );
    require(
        h13,
        &[
            "events: vec![dispatching, executing]",
            "execute_cancellable",
        ],
        "H13 regression",
    );
    require(
        er09,
        &[
            "invocation_terminal_conflict",
            "CapabilityExecutionState::Unknown",
        ],
        "ER-09 regression",
    );
    assert!(!projection.contains("CapabilityBrokerPort"));
    assert!(!recovery.contains("auto_retry_unknown"));
    assert!(!recovery.contains("reexecute_original_action"));
}

#[test]
fn retry_and_reconciliation_are_separate_from_replay() {
    let model = include_str!("../../kiana-domain/src/model.rs");
    let errors = include_str!("../../kiana-domain/src/errors.rs");
    let harness = include_str!("../../kiana-runner/src/harness.rs");
    let budget = include_str!("../../kiana-runner/src/budget.rs");
    let platform = include_str!("../src/platform.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let effect = include_str!("../../kiana-domain/src/effect_observation.rs");
    let er24 = include_str!("er24_reconciliation_evidence_guard.rs");
    let er25 = include_str!("er25_retry_policy_guard.rs");
    let cp20 = include_str!("cp20_unknown_reconcile_guard.rs");

    require(
        model,
        &[
            "ModelCallSpec",
            "attempt_id",
            "deadline_unix_ms",
            "ModelRetryClass",
            "retry_after_ms",
        ],
        "model retry contract",
    );
    require(
        errors,
        &[
            "CapabilityErrorPolicy",
            "requires_new_authorization",
            "ResultUnknown",
            "never permits automatic retry",
        ],
        "retry policy",
    );
    require(
        harness,
        &[
            "MAX_PROVIDER_ATTEMPTS",
            "reserve_attempt",
            "settle_attempt",
            "model_attempt_limit",
            "model_retry_deadline_exceeded",
        ],
        "bounded attempt driver",
    );
    require(
        budget,
        &["max_attempts_per_task", "unknown_attempts"],
        "attempt budget",
    );
    require(
        platform,
        &[
            "failure.reconcile",
            "failure_reconciliation_evidence_required",
            "runtime_outcome_unchanged",
            "automatic_retry_allowed:false",
            "new_request_required:true",
            "resource_release_requires_reconciliation",
        ],
        "reconciliation",
    );
    require(
        dispatch,
        &[
            "execution_permit_already_consumed",
            "result_unknown",
            "effect_known",
            "fenced",
        ],
        "dispatch replay fence",
    );
    require(
        effect,
        &[
            "EffectObservationState::Unknown",
            "validate_for_scope",
            "from_provider_receipt",
        ],
        "effect evidence",
    );
    require(
        er24,
        &["failure.reconcile", "runtime_outcome_unchanged"],
        "ER-24 regression",
    );
    require(
        er25,
        &[
            "ModelRetryClass::BeforeSend",
            "unknown_result_never_triggers_automatic_retry",
        ],
        "ER-25 regression",
    );
    require(
        cp20,
        &[
            "automatic_retry_allowed:false",
            "failure_reconciliation_evidence_required",
        ],
        "CP-20 regression",
    );
    for source in [harness, platform, dispatch] {
        for forbidden in [
            "retry_unknown_effect",
            "reexecute_original_action",
            "reconcile_as_retry",
            "unknown_result_auto_retry",
        ] {
            assert!(
                !source_guard::production_source(source).contains(forbidden),
                "CAP-24 bypass marker present: {forbidden}"
            );
        }
    }
}
