//! ER-21 source guard for explicit resume preflight, snapshot claims and entrypoint parity.

#[path = "support/source_guard.rs"]
mod source_guard;
use source_guard::production_source;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "ER-21 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn resume_preflight_rechecks_scope_epoch_and_claims_once() {
    let recovery = include_str!("../src/recovery.rs");
    let projection = include_str!("../src/invocation_projection.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    require(
        recovery,
        &[
            "pub async fn resume_run",
            "resolve_run_id",
            "read_all_events",
            "run.snapshot",
            "snapshot_event_id",
            "run.resume_prepared",
            "append_expected(claim, Some(last_version))",
            "run_resume_authority_changed",
            "run_resume_data_revoked",
            "run_resume_scope_changed",
            "run_resume_sandbox_denied",
            "run_snapshot_stale",
            "run_resume_claim_conflict",
            "rebuild_pending_invocation",
            "self.runner.restore(run_id, snapshot.runner_state)",
        ],
        "resume preflight/claim",
    );
    require(
        projection,
        &[
            "execution.result_committed",
            "CapabilityExecutionState::Succeeded",
            "CapabilityExecutionState::Unknown",
            "terminal_conflict",
        ],
        "invocation projection",
    );
    require(
        lifecycle,
        &[
            "pub(crate) async fn drive_run",
            "checkpoint_run",
            "pending_invocation_missing",
        ],
        "runner lifecycle",
    );
    require(
        runner,
        &[
            "async fn restore",
            "runner_checkpoint_invalid",
            "checkpoint",
        ],
        "runner restore boundary",
    );
    require(
        ports,
        &[
            "async fn restore(",
            "checkpoint_run",
            "restore_run",
            "runner_restore_unsupported",
        ],
        "restore ports",
    );

    for forbidden in ["CapabilityBroker", "ProviderGateway", "tokio::spawn"] {
        assert!(
            !production_source(recovery).contains(forbidden),
            "ER-21 recovery widened authority: {forbidden}"
        );
    }
}

#[test]
fn resume_is_one_shared_protocol_command_across_surfaces() {
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let harness = include_str!("../../kiana-entrypoints/src/harness_run.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let page = include_str!("../../kiana-entrypoints/src/web_page.html");
    let product = include_str!("../../kiana-entrypoints/src/product_command.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");

    require(
        protocol,
        &[
            "RequestBody::Resume(ResumeRequest",
            "pub struct ResumeRequest",
            "pub fn resume_run(metadata: RequestMetadata, run_id: Option<RunId>)",
        ],
        "resume protocol",
    );
    require(
        daemon,
        &[
            "RequestBody::Resume(run) => self.core.resume_run(context, run.run_id)",
            "ResponseEnvelope::from_core",
        ],
        "daemon route",
    );
    require(
        harness,
        &["resume_envelope_on_host", ".resume_run(metadata, run_id)"],
        "entrypoint harness",
    );
    require(
        workbench,
        &["ChatAction::Resume", "resume_envelope_on_host"],
        "Workbench route",
    );
    require(
        web,
        &[
            "/api/resume",
            "async fn resume_turn",
            "resume_envelope_on_host",
        ],
        "Web route",
    );
    require(page, &["/api/resume", "恢复此会话"], "Web client");
    require(
        product,
        &["\"resume\"", "RequestEnvelope::resume_run"],
        "product command",
    );
    require(
        cli,
        &["--continue", "--resume", "parse_resume_cli_args"],
        "CLI route",
    );
}

#[test]
fn resume_denials_and_replay_fixtures_remain_ci_bound() {
    let cp19 = include_str!("cp19_explicit_resume_guard.rs");
    let p0 = include_str!("p0_g03_resume_guard.rs");
    let h14 = include_str!("../../kiana-domain/tests/h14_invocation_resume.rs");
    let cli = include_str!("../../kiana-entrypoints/tests/cli_resume.rs");

    require(
        cp19,
        &[
            "claims_once_before_drive_run",
            "run_resume_authority_changed",
            "run_resume_data_revoked",
            "run_resume_scope_changed",
        ],
        "CP-19 fixture",
    );
    require(
        p0,
        &[
            "resume_run_reuses_the_same_drive_run_path",
            "resume_requires_snapshot_and_rejects_stale_scope",
        ],
        "P0-G-03 fixture",
    );
    require(
        h14,
        &[
            "InvocationResumeBinding",
            "invocation_resume_binding_changed",
        ],
        "H14 fixture",
    );
    require(
        cli,
        &["--resume", "--continue", "resume_without_prompt"],
        "CLI fixture",
    );
}

/// ER-21 behavioural fixtures must exist in the shared `control_plane` target and stay deny-first;
/// the source guard above only proves the shape of the resume read path.
#[test]
fn resume_behavioural_fixtures_are_wired() {
    let fixtures = include_str!("control_plane.rs");
    let workflow = include_str!("../../.github/workflows/er21-resume-claim.yml");
    for name in [
        "async fn resume_context_override_is_denied",
        "async fn resume_after_data_revocation_is_denied",
        "async fn resume_same_snapshot_twice_is_idempotent",
        "async fn resume_stale_snapshot_is_rejected",
        "async fn resume_concurrent_second_claim_conflicts",
        "async fn resume_runner_restore_failure_is_unknown",
        "async fn resume_without_pending_approval_continues_through_drive_run",
    ] {
        assert!(
            fixtures.contains(name),
            "ER-21 behavioural fixture missing: {name}"
        );
    }
    assert!(
        !fixtures.contains("#[ignore]"),
        "ER-21 behavioural fixtures must not be ignored"
    );
    assert!(
        workflow.contains("--test control_plane"),
        "ER-21 workflow must run the behavioural control_plane fixtures"
    );
    for forbidden in ["issue_permit(", "consume_approval("] {
        assert!(
            !fixtures.contains(forbidden),
            "ER-21 resume fixture widened authority: {forbidden}"
        );
    }
}
