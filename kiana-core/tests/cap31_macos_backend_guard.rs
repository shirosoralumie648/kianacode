//! CAP-31 partial guard: Linux code must not be reported as macOS behavior.

#[test]
fn macos_backend_scope_is_explicitly_unverified_until_target_ci_exists() {
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let sandbox = include_str!("../../kiana-daemon/src/harness_sandbox.rs");
    let supervisor = include_str!("../../kiana-daemon/src/process_supervisor.rs");
    let baseline = include_str!("../../docs/roadmap/cap31-macos-backend-baseline.md");
    let platform = include_str!("../../kiana-domain/src/platform_backend.rs");

    for marker in [
        "EnvironmentPort",
        "probe",
        "plan",
        "prepare",
        "execute",
        "quiesce",
        "dispose",
    ] {
        assert!(
            ports.contains(marker),
            "shared environment marker missing: {marker}"
        );
    }
    assert!(sandbox.contains("SANDBOX_BACKEND"));
    assert!(sandbox.contains("target_os = \"linux\""));
    assert!(supervisor.contains("ProcessSupervisor"));
    for marker in [
        "PlatformBackendDisposition",
        "TargetOnly",
        "behavior_verified",
        "platform_backend_host_fallback_forbidden",
    ] {
        assert!(
            platform.contains(marker),
            "platform marker missing: {marker}"
        );
    }
    for marker in [
        "macos_backend_denies_host_secrets_and_gui_escape",
        "macos_descendant_escape_or_stop_failure_is_visible",
        "macos_missing_backend_has_no_host_fallback",
        "partial",
        "behavior_verified=false",
        "target macOS CI",
    ] {
        assert!(
            baseline.contains(marker),
            "CAP-31 limitation marker missing: {marker}"
        );
    }
}
