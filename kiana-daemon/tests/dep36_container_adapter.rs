use kiana_daemon::container_environment::{
    ContainerEnvironmentAdapter, ContainerExecOperation, ContainerLaunchConfig, ContainerProbeKind,
    ContainerProbeRequest, CONTAINER_STOP_SIGNAL,
};
use std::collections::BTreeMap;

const IMAGE: &str =
    "ghcr.io/kiana/ci-fixture@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn config() -> ContainerLaunchConfig {
    let mut config = ContainerLaunchConfig::new(IMAGE, std::env::current_dir().expect("workspace"));
    config.environment = BTreeMap::from([(String::from("CI_FIXTURE"), String::from("1"))]);
    config
}

#[test]
fn create_args_bind_root_identity_and_explicit_sigterm() {
    let args = ContainerEnvironmentAdapter::create_args(
        &config(),
        "owner-1",
        "scope-1",
        "plan-1",
        "kiana-plan-1",
    )
    .expect("valid container plan");
    assert!(args
        .windows(2)
        .any(|pair| pair[0] == "--stop-signal" && pair[1] == CONTAINER_STOP_SIGNAL));
    assert!(args
        .iter()
        .any(|arg| arg.starts_with("io.kiana.root=sha256:")));
    assert!(args.iter().any(|arg| arg == "CI_FIXTURE=1"));
}

#[test]
fn lifecycle_probe_kinds_keep_startup_inspect_only() {
    let operation = ContainerExecOperation {
        argv: vec![String::from("/bin/true")],
        cwd: String::from("/workspace"),
        environment: BTreeMap::new(),
        timeout_ms: 1_000,
    };
    assert_eq!(
        ContainerProbeRequest::startup().kind,
        ContainerProbeKind::Startup
    );
    assert!(ContainerProbeRequest::startup().operation.is_none());
    assert_eq!(
        ContainerProbeRequest::readiness(operation.clone()).kind,
        ContainerProbeKind::Readiness
    );
    assert_eq!(
        ContainerProbeRequest::liveness(operation).kind,
        ContainerProbeKind::Liveness
    );
}
