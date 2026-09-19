//! DEP-36 source guard for container identity, lifecycle probes and stop semantics.

#[test]
fn container_adapter_is_identity_bound_and_probe_semantics_are_explicit() {
    let source = include_str!("../../kiana-daemon/src/container_environment.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/dep36-container-adapter-baseline.md");

    for marker in [
        "EnvironmentPort",
        "ContainerEnvironmentAdapter",
        "LABEL_ROOT",
        "volume_root_identity",
        "\"--stop-signal\"",
        "CONTAINER_STOP_SIGNAL",
        "\"--signal\"",
        "ContainerProbeKind",
        "ContainerProbeRequest",
        "lifecycle_probe",
        "container_startup_probe_must_be_inspect_only",
        "container_runtime_probe_operation_missing",
        "allowed_environment.contains_key(name)",
        "result_unknown:container_probe_timeout",
        "result_unknown:container_cancel_unconfirmed",
        "no_host_control_socket",
    ] {
        assert!(
            source.contains(marker),
            "DEP-36 source marker missing: {marker}"
        );
    }
    for marker in [
        "EnvironmentPort",
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
    for marker in [
        "immutable image",
        "volume/root identity",
        "env allowlist",
        "SIGTERM",
        "startup",
        "readiness",
        "liveness",
        "fake/container harness",
        "partial",
        "result_unknown",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-36 baseline marker missing: {marker}"
        );
    }
    assert!(!source.contains("CapabilityBroker::new"));
}
