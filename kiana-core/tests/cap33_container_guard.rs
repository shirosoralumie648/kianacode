//! CAP-33 source guard for the controlled OCI/gVisor environment boundary.

#[test]
fn container_backend_is_fail_closed_and_identity_bound() {
    let source = include_str!("../../kiana-daemon/src/container_environment.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/cap33-container-baseline.md");

    for marker in [
        "EnvironmentPort",
        "ContainerEnvironmentAdapter",
        "is_digest_pinned_image",
        "\"--read-only\"",
        "\"--network\"",
        "DEFAULT_CONTAINER_NETWORK",
        "\"--cap-drop\"",
        "no-new-privileges=true",
        "\"--pids-limit\"",
        "\"--memory\"",
        "\"--cpus\"",
        "\"--user\"",
        "LABEL_OWNER",
        "LABEL_SCOPE",
        "LABEL_PLAN",
        "execute_argv",
        "container_environment_not_recoverable",
        "container_runtime_failure_no_host_fallback",
        "result_unknown:container_cancel_unconfirmed",
        "runsc",
        "env_clear",
        "DOCKER_HOST",
    ] {
        assert!(
            source.contains(marker),
            "CAP-33 source marker missing: {marker}"
        );
    }
    for marker in [
        "EnvironmentPort",
        "prepare",
        "execute",
        "quiesce",
        "dispose",
        "environment_id",
    ] {
        assert!(
            ports.contains(marker),
            "shared environment marker missing: {marker}"
        );
    }
    for marker in [
        "container_never_mounts_host_control_socket_or_credentials",
        "container_runtime_failure_never_falls_back_to_host",
        "container_cancel_confirms_inner_process_stop",
        "partial",
        "behavior_verified=false",
        "restart recovery",
        "gVisor",
    ] {
        assert!(
            baseline.contains(marker),
            "CAP-33 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
