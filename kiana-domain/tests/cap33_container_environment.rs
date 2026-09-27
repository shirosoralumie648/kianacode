//! CAP-33 failure-first fixtures: the container environment is optional and fail-closed.
//!
//! These tests do not run a container, pull an image, or talk to a runtime. They exercise the
//! descriptor and the selection decision, so they cannot tell a working OCI backend from a
//! missing one; the daemon adapter and a real runtime remain the only evidence for that.

use kiana_domain::{
    container_receipt_facts, container_target_supported, stop_disposition, unconfirmed_stop_reason,
    ContainerEnvironmentDecision, ContainerEnvironmentDescriptor, ContainerEnvironmentSelection,
    ContainerMount, ContainerResourceLimits, ContainerRuntimeKind, ContainerRuntimeObservation,
    ContainerStopConfirmation, PlatformTarget, CONTAINER_GVISOR_DIMENSION,
    CONTAINER_REQUIRED_DIMENSIONS, CONTAINER_WORKSPACE_MOUNT,
};
use std::collections::BTreeSet;

const IMAGE: &str =
    "ghcr.io/kiana/ci-fixture@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn limits() -> ContainerResourceLimits {
    ContainerResourceLimits::new(
        512 * 1024 * 1024,
        1_000,
        256,
        8 * 1024 * 1024,
        4 * 1024 * 1024 * 1024,
    )
    .expect("limits")
}

fn workspace(host: &str) -> ContainerMount {
    ContainerMount::new(host, CONTAINER_WORKSPACE_MOUNT, false).expect("workspace mount")
}

fn descriptor(
    mounts: Vec<ContainerMount>,
    runtime: ContainerRuntimeKind,
) -> ContainerEnvironmentDescriptor {
    ContainerEnvironmentDescriptor::new(
        "owner-1",
        "sha256:scope",
        IMAGE,
        runtime,
        "65532:65532",
        mounts,
        limits(),
        "kiana-env-1",
    )
    .expect("descriptor")
}

fn observed(
    runtime: ContainerRuntimeKind,
    dimensions: &[&str],
    image_present: bool,
) -> ContainerRuntimeObservation {
    ContainerRuntimeObservation::new(
        "runtime-1",
        true,
        Some(runtime),
        image_present,
        dimensions.iter().map(|value| (*value).to_owned()).collect(),
    )
    .expect("observation")
}

fn full_observation(runtime: ContainerRuntimeKind) -> ContainerRuntimeObservation {
    let mut dimensions = CONTAINER_REQUIRED_DIMENSIONS
        .iter()
        .map(|value| (*value).to_owned())
        .collect::<BTreeSet<_>>();
    if runtime.has_extra_isolation() {
        dimensions.insert(CONTAINER_GVISOR_DIMENSION.to_owned());
    }
    observed(
        runtime,
        &dimensions.iter().map(String::as_str).collect::<Vec<_>>(),
        true,
    )
}

#[test]
fn container_never_mounts_host_control_socket_or_credentials() {
    // Every host control and credential location the adapter refuses is refused here too.
    for host in [
        "/",
        "/root",
        "/home",
        "/run",
        "/run/docker.sock",
        "/var/run",
        "/var/run/docker.sock",
        "/home/user/.docker",
        "/home/user/.ssh",
    ] {
        let mount = ContainerMount::new(host, CONTAINER_WORKSPACE_MOUNT, false);
        assert_eq!(
            mount.unwrap_err(),
            "container_environment_host_control_mount_denied",
            "host path {host} was accepted as a container mount"
        );
    }

    // A mutable image reference is not an immutable environment.
    for image in ["alpine:latest", "alpine", "ghcr.io/kiana/ci-fixture"] {
        let refused = ContainerEnvironmentDescriptor::new(
            "owner-1",
            "sha256:scope",
            image,
            ContainerRuntimeKind::RunC,
            "65532:65532",
            vec![workspace("/srv/project")],
            limits(),
            "kiana-env-1",
        );
        assert_eq!(
            refused.unwrap_err(),
            "container_environment_descriptor_invalid",
            "image {image} was accepted without a digest"
        );
    }

    // A root container is refused rather than downgraded to a warning.
    let root = ContainerEnvironmentDescriptor::new(
        "owner-1",
        "sha256:scope",
        IMAGE,
        ContainerRuntimeKind::RunC,
        "0:0",
        vec![workspace("/srv/project")],
        limits(),
        "kiana-env-1",
    );
    assert!(root.is_err());

    // Two writable mounts would give a host directory a writable surface inside the container.
    let two_writable = ContainerEnvironmentDescriptor::new(
        "owner-1",
        "sha256:scope",
        IMAGE,
        ContainerRuntimeKind::RunC,
        "65532:65532",
        vec![
            workspace("/srv/project"),
            ContainerMount::new("/srv/cache", "/cache", false).expect("cache mount"),
        ],
        limits(),
        "kiana-env-1",
    );
    assert_eq!(
        two_writable.unwrap_err(),
        "container_environment_workspace_mount_required"
    );

    // The honest shape validates and its receipt facts name the image, runtime, user, network
    // and limits, so a Receipt can carry them rather than leaving them implicit.
    let good = descriptor(vec![workspace("/srv/project")], ContainerRuntimeKind::RunC);
    assert!(good.validate().is_ok());
    assert_eq!(good.network_policy, "deny");
    assert!(good.read_only_root);
    let facts = container_receipt_facts(&good).expect("receipt facts");
    assert_eq!(facts.get("image").map(String::as_str), Some(IMAGE));
    assert_eq!(facts.get("runtime").map(String::as_str), Some("runc"));
    assert_eq!(facts.get("user").map(String::as_str), Some("65532:65532"));
    assert_eq!(
        facts.get("network_policy").map(String::as_str),
        Some("deny")
    );
    assert!(facts
        .get("limits")
        .is_some_and(|digest| digest.starts_with("sha256:")));
}

#[test]
fn container_runtime_failure_never_falls_back_to_host() {
    let good = descriptor(vec![workspace("/srv/project")], ContainerRuntimeKind::RunC);

    // No runtime on the host: unavailable, with no substitute named.
    let missing =
        ContainerRuntimeObservation::new("runtime-1", false, None, false, BTreeSet::new())
            .expect("observation");
    let decision = ContainerEnvironmentDecision::evaluate(&good, &missing).expect("decision");
    assert!(!decision.usable());
    assert_eq!(
        decision.selection,
        ContainerEnvironmentSelection::Unavailable
    );
    assert_eq!(decision.reason, "container_environment_unavailable");
    assert!(decision.effective_runtime.is_empty());
    assert!(decision.no_host_fallback);

    // The runtime found is not the runtime the descriptor names. Running the descriptor's
    // configuration under a different runtime would make the descriptor a lie, so this is a
    // refusal and not a silent rewrite.
    let mismatched = observed(ContainerRuntimeKind::RunSc, &[], true);
    let decision = ContainerEnvironmentDecision::evaluate(&good, &mismatched).expect("decision");
    assert_eq!(
        decision.selection,
        ContainerEnvironmentSelection::Unavailable
    );
    assert_eq!(decision.reason, "container_environment_runtime_mismatch");
    assert!(decision.effective_runtime.is_empty());

    // The image is not in the runtime's store. Pulling it at execution time needs its own
    // network authorization, which this decision does not grant.
    let no_image = observed(
        ContainerRuntimeKind::RunC,
        &CONTAINER_REQUIRED_DIMENSIONS,
        false,
    );
    let decision = ContainerEnvironmentDecision::evaluate(&good, &no_image).expect("decision");
    assert_eq!(
        decision.selection,
        ContainerEnvironmentSelection::Unavailable
    );
    assert_eq!(decision.reason, "container_environment_image_unavailable");
    assert!(decision.effective_runtime.is_empty());

    // A decision that names a substitute runtime is refused by its own validation.
    let mut substituted =
        ContainerEnvironmentDecision::evaluate(&good, &missing).expect("decision");
    substituted.effective_runtime = "runc".to_owned();
    assert_eq!(
        substituted.validate_against(&good, &missing).unwrap_err(),
        "container_environment_substitute_runtime_named"
    );

    // gVisor is an optional environment, not a shipped one: naming `runsc` is only a
    // configuration choice, and without the gVisor dimension observed it is not usable.
    let gvisor = descriptor(vec![workspace("/srv/project")], ContainerRuntimeKind::RunSc);
    let runc_only = observed(
        ContainerRuntimeKind::RunSc,
        &CONTAINER_REQUIRED_DIMENSIONS,
        true,
    );
    let decision = ContainerEnvironmentDecision::evaluate(&gvisor, &runc_only).expect("decision");
    assert!(!decision.usable());
    assert_eq!(
        decision.selection,
        ContainerEnvironmentSelection::NotEnforced
    );
    assert_eq!(
        decision.reason,
        "container_environment_required_dimension_unobserved"
    );
    assert!(decision.effective_runtime.is_empty());

    // With the gVisor dimension actually observed, the descriptor is selectable. This is a
    // statement about the decision, not evidence that gVisor confinement works.
    let with_gvisor = full_observation(ContainerRuntimeKind::RunSc);
    let decision = ContainerEnvironmentDecision::evaluate(&gvisor, &with_gvisor).expect("decision");
    assert!(decision.usable());
    assert_eq!(decision.effective_runtime, "runsc");
    assert!(decision
        .enforced_dimensions
        .contains(CONTAINER_GVISOR_DIMENSION));
    assert!(decision.enforced_dimensions.len() == CONTAINER_REQUIRED_DIMENSIONS.len() + 1);

    // Every required dimension is genuinely required: dropping any one of them refuses.
    for missing_dimension in CONTAINER_REQUIRED_DIMENSIONS {
        let reduced = full_observation(ContainerRuntimeKind::RunC);
        let mut dimensions = reduced.observed_dimensions.clone();
        dimensions.remove(missing_dimension);
        let observation = ContainerRuntimeObservation::new(
            "runtime-1",
            true,
            Some(ContainerRuntimeKind::RunC),
            true,
            dimensions,
        )
        .expect("observation");
        let decision =
            ContainerEnvironmentDecision::evaluate(&good, &observation).expect("decision");
        assert!(
            !decision.usable(),
            "dimension {missing_dimension} was treated as optional"
        );
        assert_eq!(
            decision.selection,
            ContainerEnvironmentSelection::NotEnforced
        );
    }
}

#[test]
fn container_cancel_confirms_inner_process_stop() {
    // Only a confirmed stop may be recorded as stopped. An unconfirmed stop is Unknown, with the
    // same `result_unknown:` prefix the daemon adapter uses for its receipts.
    for (confirmation, disposition, reason) in [
        (
            ContainerStopConfirmation::Stopped,
            kiana_domain::ContainerEffectDisposition::Stopped,
            None,
        ),
        (
            ContainerStopConfirmation::Unconfirmed,
            kiana_domain::ContainerEffectDisposition::Unknown,
            Some("result_unknown:container_cancel_unconfirmed"),
        ),
        (
            ContainerStopConfirmation::NotObserved,
            kiana_domain::ContainerEffectDisposition::Unknown,
            Some("result_unknown:container_stop_not_observed"),
        ),
    ] {
        assert_eq!(stop_disposition(confirmation), disposition);
        assert_eq!(unconfirmed_stop_reason(confirmation), reason);
    }

    // A confirmed stop is the only state a caller may act on.
    assert!(ContainerStopConfirmation::Stopped.is_confirmed());
    assert!(!ContainerStopConfirmation::Unconfirmed.is_confirmed());
    assert!(!ContainerStopConfirmation::NotObserved.is_confirmed());

    // The container backend is optional by platform; a target it is not implemented for is
    // reported through the shared platform vocabulary rather than a second one.
    assert!(container_target_supported(PlatformTarget::Linux));
    assert!(!container_target_supported(PlatformTarget::Container));
}
