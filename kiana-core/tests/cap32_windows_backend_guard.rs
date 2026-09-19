//! CAP-32 partial guard: Windows behavior remains unverified until target CI/backend exists.

#[test]
fn windows_backend_requirements_and_no_fake_success_are_explicit() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let job = include_str!("../../kiana-domain/src/job_handle.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let baseline = include_str!("../../docs/roadmap/cap32-windows-backend-baseline.md");
    let platform = include_str!("../../kiana-domain/src/platform_backend.rs");
    for marker in ["EnvironmentPort", "ProcessSupervisor", "JobHandle"] {
        assert!(ports.contains(marker) || job.contains(marker) || supervisor.contains(marker));
    }
    for marker in [
        "windows_reparse_or_unc_path_cannot_escape_scope",
        "windows_child_cannot_break_away_from_job",
        "windows_cross_process_lock_is_real_or_operation_is_denied",
        "partial",
        "behavior_verified=false",
        "target Windows CI",
        "no fake success",
    ] {
        assert!(baseline.contains(marker), "CAP-32 marker missing: {marker}");
    }
    for marker in [
        "PlatformTarget",
        "Windows",
        "TargetOnly",
        "behavior_verified",
    ] {
        assert!(
            platform.contains(marker),
            "platform marker missing: {marker}"
        );
    }
}
