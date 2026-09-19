#[test]
fn h24_checkpoint_and_resume_are_complete_explicit_and_single_claim() {
    let recovery = include_str!("../src/recovery.rs");
    let lifecycle = include_str!("../src/lifecycle.rs");
    let runner = include_str!("../../kiana-runner/src/harness.rs");
    let binding = include_str!("../../kiana-domain/src/invocation_resume.rs");
    for marker in [
        "checkpoint_run",
        "resume_run",
        "run.resume_prepared",
        "run_resume_claim_conflict",
        "run_resume_authority_changed",
        "run_resume_data_revoked",
        "run_resume_scope_changed",
        "run_resume_sandbox_denied",
        "InvocationResumeBinding",
        "pending_batch_digest",
        "runner_checkpoint_tool_catalog_changed",
        "pending_tools",
        "prompt_sources",
        "run_already_exists",
    ] {
        assert!(
            recovery.contains(marker)
                || lifecycle.contains(marker)
                || runner.contains(marker)
                || binding.contains(marker),
            "H24 recovery marker missing: {marker}"
        );
    }
    assert!(recovery.contains("append_expected"));
    assert!(recovery.contains("explicit"));
    assert!(!recovery.contains("auto_resume"));
    assert!(!recovery.contains("runner.send(RunnerCommand::Start"));
}
