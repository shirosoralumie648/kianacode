//! ER-17 source guard for serializable RunSnapshot and pending writes.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-17 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn er_snapshot_digest_or_scope_mismatch_is_denied() {
    let domain = include_str!("../../kiana-domain/src/capabilities.rs");
    let binding = include_str!("../../kiana-domain/src/invocation_resume.rs");
    let recovery = include_str!("../src/recovery.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let fixture = include_str!("cp18_run_snapshot_guard.rs");

    require(
        domain,
        &[
            "pub struct RunSnapshot",
            "schema: String",
            "run_id: RunId",
            "context: RequestContext",
            "sandbox: String",
            "role_prompt_hash",
            "runner_state_digest",
            "pending_invocation",
            "resumable",
        ],
        "RunSnapshot",
    );
    require(
        binding,
        &[
            "InvocationResumeBinding",
            "parameter_digest",
            "catalog_digest",
            "authority_epoch",
            "owner_id",
            "project_digest",
            "sandbox_digest",
            "pending_batch_digest",
            "validate_against",
            "invocation_resume_binding_changed",
        ],
        "resume binding",
    );
    require(
        recovery,
        &[
            "checkpoint_run",
            "run.snapshot",
            "runner_state_digest",
            "json_digest(&snapshot.runner_state)",
            "run_snapshot_invalid",
            "run_resume_authority_changed",
            "run_resume_data_revoked",
            "run_resume_scope_changed",
            "run_snapshot_stale",
            "append_expected(claim, Some(last_version))",
        ],
        "ControlPlane recovery",
    );
    require(recovery, &["kiana.run-snapshot.v1"], "snapshot schema");
    require(
        runner,
        &[
            "HarnessCheckpoint",
            "pending_tools",
            "prompt_sources",
            "tool_catalog_digest",
            "runner_checkpoint_busy",
            "runner_checkpoint_invalid",
            "runner_checkpoint_tool_catalog_changed",
            "runner_checkpoint_invocation_identity_mismatch",
            "async fn restore",
        ],
        "Runner pending writes",
    );
    require(
        ports,
        &[
            "checkpoint_run",
            "restore_run",
            "runner_checkpoint_unsupported",
            "runner_restore_unsupported",
        ],
        "checkpoint ports",
    );
    require(
        fixture,
        &[
            "checkpoint_restore_invalidates_approvals_and_runner_context",
            "run_resume_authority_changed",
        ],
        "checkpoint fixture",
    );
}

#[test]
fn er_snapshot_with_redacted_input_is_non_resumable() {
    let recovery = include_str!("../src/recovery.rs");
    let redaction = include_str!("../src/redaction.rs");
    let domain = include_str!("../../kiana-domain/src/states.rs");
    let binding = include_str!("../../kiana-domain/src/invocation_resume.rs");
    let fixture = include_str!("p0_f03_resume_guard.rs");

    require(
        recovery,
        &[
            "let raw =",
            "let mut safe = redact_event_value(&raw)",
            "if safe != raw",
            "safe[\"resumable\"] = json!(false)",
            "!snapshot.resumable",
            "approval_continuation_unavailable",
        ],
        "snapshot redaction",
    );
    require(
        redaction,
        &[
            "redact_event_value",
            "redact_event_text",
            "redact_capability_result",
        ],
        "redaction boundary",
    );
    require(
        domain,
        &[
            "payload_recoverable",
            "redaction_profile",
            "event_redacted_payload_recoverable",
        ],
        "event recovery metadata",
    );
    require(
        binding,
        &[
            "canonical_action_input_digest",
            "pending_batch_digest",
            "binding_digest",
        ],
        "pending input binding",
    );
    require(
        fixture,
        &[
            "fresh_process_resume_reconstructs_pending_approval",
            "approval_continuation_unavailable",
        ],
        "resume material fixture",
    );
}

#[test]
fn er_snapshot_after_terminal_is_not_restorable() {
    let recovery = include_str!("../src/recovery.rs");
    let projection = include_str!("../src/projection.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");

    require(
        recovery,
        &[
            "run_snapshot_stale",
            "run.resume_prepared",
            "run_snapshot_pending_invocation_missing",
            "run_resume_scope_changed",
            "self.runner.restore(run_id, snapshot.runner_state)",
        ],
        "resume terminal/stale fence",
    );
    require(
        projection,
        &[
            "RunPhase::Terminal",
            "RunProjectionError",
            "TerminalConflict",
            "run_terminal_conflict",
        ],
        "terminal projection",
    );
    require(
        lifecycle,
        &[
            "checkpoint_run",
            "record_terminal_event",
            "terminal",
            "run.completed",
            "run.failed",
        ],
        "lifecycle terminal",
    );
    require(
        runner,
        &[
            "runner_checkpoint_invalid",
            "checkpoint.run_id",
            "checkpoint.pending_tools",
        ],
        "runner restore fence",
    );
    for source in [recovery, projection, lifecycle] {
        assert!(!source.contains("auto_resume_terminal"));
        assert!(!source.contains("restore_and_execute_without_claim"));
    }
}
