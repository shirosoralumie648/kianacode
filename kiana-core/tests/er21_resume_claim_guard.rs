//! ER-21 source guard for explicit resume preflight, snapshot claims and entrypoint parity.

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
            "runner_restore_unsupported",
            "checkpoint",
        ],
        "runner restore boundary",
    );
    require(
        ports,
        &["async fn restore(", "checkpoint_run", "restore_run"],
        "restore ports",
    );

    for forbidden in ["CapabilityBroker", "ProviderGateway", "tokio::spawn"] {
        assert!(
            !recovery.contains(forbidden),
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
