//! CAP-33 source guard: the container environment is an optional, fail-closed selection.
//!
//! Two things this guard is really checking. First, that the decision contract can refuse, and
//! can refuse *without naming a substitute*: a container backend that falls back to the host
//! after a runtime failure is a confinement claim that stopped being true. Second, that the host
//! mount deny list in the domain contract and the one in the daemon adapter have not drifted,
//! because a container that could mount the Docker socket is a container that owns the host.

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-33 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn container_environment_decision_is_optional_and_fail_closed() {
    let contract = include_str!("../../kiana-domain/src/container_environment.rs");
    require(
        contract,
        &[
            "CONTAINER_ENVIRONMENT_SCHEMA",
            "CONTAINER_ENVIRONMENT_DECISION_SCHEMA",
            "CONTAINER_ENVIRONMENT_VERSION",
            "ContainerRuntimeKind",
            "ContainerMount",
            "ContainerResourceLimits",
            "ContainerEnvironmentDescriptor",
            "ContainerRuntimeObservation",
            "ContainerEnvironmentSelection",
            "ContainerEnvironmentDecision",
            "ContainerStopConfirmation",
            "ContainerEffectDisposition",
            "CONTAINER_REQUIRED_DIMENSIONS",
            "CONTAINER_GVISOR_DIMENSION",
            "CONTAINER_WORKSPACE_MOUNT",
            "stop_disposition",
            "unconfirmed_stop_reason",
            "container_target_supported",
            "container_receipt_facts",
        ],
        "environment contract",
    );
    require(
        contract,
        &[
            "container_environment_host_control_mount_denied",
            "container_environment_descriptor_invalid",
            "container_environment_workspace_mount_required",
            "container_environment_unavailable",
            "container_environment_runtime_mismatch",
            "container_environment_image_unavailable",
            "container_environment_required_dimension_unobserved",
            "container_environment_substitute_runtime_named",
            "container_environment_decision_binding_invalid",
            "container_environment_accepted",
            "result_unknown:container_cancel_unconfirmed",
            "result_unknown:container_stop_not_observed",
        ],
        "refusal codes",
    );
    require(
        contract,
        &[
            "pub fn evaluate(",
            "pub fn validate_against(",
            "fn derive(",
            "no_host_fallback: bool",
        ],
        "fixed decision order",
    );
    // A descriptor is a description, not a runtime. It must not inspect one, start one, or pull
    // an image; the daemon adapter is the only thing that does that.
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStorePort",
        "CapabilityBroker",
        "std::env",
    ] {
        assert!(
            !contract.contains(forbidden),
            "CAP-33 descriptor crossed the effect boundary: {forbidden}"
        );
    }
}

#[test]
fn the_two_host_mount_deny_lists_have_not_drifted() {
    // The domain contract and the daemon adapter cannot share a private helper without moving
    // it, so the lists are duplicated and pinned here. Each side's exact spellings are asserted,
    // so widening one without the other fails CI rather than silently widening what a container
    // can read off the host.
    let contract = include_str!("../../kiana-domain/src/container_environment.rs");
    let adapter = include_str!("../../kiana-daemon/src/container_environment.rs");
    for host in [
        "\"/\"",
        "\"/root\"",
        "\"/home\"",
        "\"/run\"",
        "\"/var/run\"",
        "\"/run/\"",
        "\"/var/run/\"",
        "\"/.docker\"",
        "\"/.ssh\"",
    ] {
        assert!(
            contract.contains(host),
            "CAP-33 domain mount deny list lost entry: {host}"
        );
        assert!(
            adapter.contains(host),
            "CAP-33 adapter mount deny list lost entry: {host}"
        );
    }
    // The adapter's runtime-facing half stays the one place that talks to a runtime; the domain
    // contract must not grow a second.
    for marker in [
        "ContainerEnvironmentAdapter",
        "execute_argv",
        "container_runtime_failure_no_host_fallback",
        "container_environment_not_recoverable",
    ] {
        assert!(
            adapter.contains(marker),
            "CAP-33 adapter marker missing: {marker}"
        );
    }
    assert!(!contract.contains("ContainerEnvironmentAdapter"));
}

#[test]
fn cap33_card_cases_are_fixtures_and_stay_unproven() {
    let fixtures = include_str!("../../kiana-domain/tests/cap33_container_environment.rs");
    require(
        fixtures,
        &[
            "fn container_never_mounts_host_control_socket_or_credentials()",
            "fn container_runtime_failure_never_falls_back_to_host()",
            "fn container_cancel_confirms_inner_process_stop()",
        ],
        "CAP-33 rejected-first fixtures",
    );
    let baseline = include_str!("../../docs/roadmap/cap33-container-baseline.md");
    for marker in [
        "container_never_mounts_host_control_socket_or_credentials",
        "container_runtime_failure_never_falls_back_to_host",
        "container_cancel_confirms_inner_process_stop",
        "partial",
        "runs no container",
        "not established",
    ] {
        assert!(
            baseline.contains(marker),
            "CAP-33 baseline marker missing: {marker}"
        );
    }
}
