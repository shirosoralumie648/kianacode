use kiana_daemon::container_environment::{
    ContainerEnvironmentAdapter, ContainerExecOperation, ContainerLaunchConfig, ContainerProbeKind,
    ContainerProbeRequest, CONTAINER_STOP_SIGNAL,
};
use kiana_ports::{
    EnvironmentPlan, EnvironmentPort, EnvironmentProbe, EnvironmentScope, PortError,
    PreparedEnvironment,
};
use serde_json::json;
use std::collections::BTreeMap;

const IMAGE: &str =
    "ghcr.io/kiana/ci-fixture@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn config() -> ContainerLaunchConfig {
    let mut config = ContainerLaunchConfig::new(IMAGE, std::env::current_dir().expect("workspace"));
    config.environment = BTreeMap::from([(String::from("CI_FIXTURE"), String::from("1"))]);
    config
}

fn scope() -> EnvironmentScope {
    let root = std::env::current_dir().expect("workspace");
    EnvironmentScope {
        owner_id: String::from("owner-1"),
        scope_digest: String::from("scope-1"),
        project_root: root.to_string_lossy().into_owned(),
        sandbox: String::from("container"),
        path_allow: Vec::new(),
    }
}

fn supported_probe() -> EnvironmentProbe {
    EnvironmentProbe {
        backend: String::from("oci-container"),
        backend_version: String::from("fixture"),
        capabilities: BTreeMap::from([
            (String::from("oci_container"), true),
            (String::from("immutable_image"), true),
            (String::from("non_root_user"), true),
            (String::from("network_none"), true),
            (String::from("resource_limits"), true),
            (String::from("owner_scope_labels"), true),
            (String::from("volume_root_identity"), true),
            (String::from("no_host_control_socket"), true),
            (String::from("gvisor_runtime"), false),
        ]),
        enforced: BTreeMap::from([
            (String::from("oci_container"), true),
            (String::from("immutable_image"), true),
            (String::from("non_root_user"), true),
            (String::from("network_none"), true),
            (String::from("resource_limits"), true),
            (String::from("owner_scope_labels"), true),
            (String::from("volume_root_identity"), true),
            (String::from("no_host_control_socket"), true),
        ]),
        failure_reason: None,
    }
}

fn assert_dimension_unavailable(result: Result<EnvironmentPlan, PortError>) {
    assert!(matches!(
        result,
        Err(PortError::Unavailable(reason))
            if reason == "container_required_dimension_unavailable"
    ));
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

#[test]
fn container_wire_shapes_reject_unknown_fields() {
    let mut operation = serde_json::to_value(ContainerExecOperation {
        argv: vec![String::from("/bin/true")],
        cwd: String::from("/workspace"),
        environment: BTreeMap::new(),
        timeout_ms: 1_000,
    })
    .expect("encode operation");
    operation["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ContainerExecOperation>(operation).is_err());

    let mut launch = serde_json::to_value(config()).expect("encode launch config");
    launch["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ContainerLaunchConfig>(launch).is_err());
}

#[tokio::test]
async fn plan_rejects_capabilities_that_are_not_enforced() {
    let adapter = ContainerEnvironmentAdapter::new(config()).expect("valid adapter");
    let mut probe = supported_probe();
    probe.enforced.insert(String::from("network_none"), false);
    assert_dimension_unavailable(adapter.plan(&scope(), &probe).await);
}

#[tokio::test]
async fn plan_rejects_required_capability_with_missing_enforced_marker() {
    let adapter = ContainerEnvironmentAdapter::new(config()).expect("valid adapter");
    let mut probe = supported_probe();
    probe.enforced.remove("resource_limits");
    assert_dimension_unavailable(adapter.plan(&scope(), &probe).await);
}

#[tokio::test]
async fn plan_rejects_invalid_backend_version() {
    let adapter = ContainerEnvironmentAdapter::new(config()).expect("valid adapter");
    let mut probe = supported_probe();
    probe.backend_version.clear();
    assert!(matches!(
        adapter.plan(&scope(), &probe).await,
        Err(PortError::Failed(reason)) if reason == "container_backend_version_invalid"
    ));
}

#[tokio::test]
async fn forged_prepared_backend_is_denied_before_container_exec() {
    let adapter = ContainerEnvironmentAdapter::new(config()).expect("valid adapter");
    let plan = adapter.plan(&scope(), &supported_probe()).await.unwrap();
    let prepared = PreparedEnvironment {
        plan_digest: plan.plan_digest.clone(),
        backend: "forged-backend".to_owned(),
        backend_version: plan.backend_version,
        enforced_dimensions: Vec::new(),
        environment_id: Some(format!("kiana-{}", &plan.plan_digest[7..19])),
    };
    let operation = ContainerExecOperation {
        argv: vec!["/bin/true".to_owned()],
        cwd: "/tmp".to_owned(),
        environment: BTreeMap::new(),
        timeout_ms: 1_000,
    };
    assert!(matches!(
        adapter.execute_argv(&prepared, operation).await,
        Err(PortError::Conflict(reason)) if reason == "container_environment_identity_mismatch"
    ));
}
