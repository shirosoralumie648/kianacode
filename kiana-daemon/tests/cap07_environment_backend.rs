//! CAP-07 plan determinism, environment identity independence and fail-closed backend deny
//! fixtures for the container `EnvironmentPort` adapter.
//!
//! The plan fixtures select a runtime path that cannot exist on the test host, so a
//! successful plan is itself evidence that `plan` is pure: it never spawns the runtime.
//! Prepared-environment cases only exercise the pre-execution identity boundary; no
//! container is created, started or inspected here.

use kiana_daemon::container_environment::{
    ContainerEnvironmentAdapter, ContainerExecOperation, ContainerLaunchConfig, CONTAINER_BACKEND,
};
use kiana_ports::{
    EnvironmentPlan, EnvironmentPort, EnvironmentProbe, EnvironmentScope, PortError,
    PreparedEnvironment,
};
use std::collections::BTreeMap;

const IMAGE: &str =
    "ghcr.io/kiana/ci-fixture@sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Runtime path that cannot exist on the test host; planning with it proves `plan` never
/// spawns the selected runtime binary.
fn config_without_runtime() -> ContainerLaunchConfig {
    let workspace = std::env::current_dir().expect("workspace");
    let runtime_dir = std::env::temp_dir().join("kiana-cap07-no-such-runtime");
    let mut config = ContainerLaunchConfig::new(IMAGE, workspace);
    config.runtime = runtime_dir.join("kiana-container-runtime");
    config
}

fn adapter_fixture() -> ContainerEnvironmentAdapter {
    ContainerEnvironmentAdapter::new(config_without_runtime()).expect("valid adapter")
}

fn scope(owner_id: &str, scope_digest: &str) -> EnvironmentScope {
    let root = std::env::current_dir().expect("workspace");
    EnvironmentScope {
        owner_id: owner_id.to_owned(),
        scope_digest: scope_digest.to_owned(),
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

/// Container identity derived from the server-owned plan digest, mirroring the adapter's
/// `kiana-<12 hex>` name construction.
fn environment_name(plan: &EnvironmentPlan) -> String {
    assert!(
        plan.plan_digest.starts_with("sha256:") && plan.plan_digest.len() > 19,
        "plan digest must be a sha256 fingerprint: {}",
        plan.plan_digest
    );
    format!("kiana-{}", &plan.plan_digest[7..19])
}

fn assert_dimension_unavailable(result: Result<EnvironmentPlan, PortError>) {
    assert!(matches!(
        result,
        Err(PortError::Unavailable(reason))
            if reason == "container_required_dimension_unavailable"
    ));
}

#[tokio::test]
async fn prepared_identity_from_another_environment_is_denied_before_container_effect() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");
    let scope_b = scope("owner-2", "scope-2");
    let probe = supported_probe();
    let plan_a = adapter.plan(&scope_a, &probe).await.expect("plan a");
    let plan_b = adapter.plan(&scope_b, &probe).await.expect("plan b");

    // Prepared identity material copied from environment B must not be accepted for A's plan.
    let swapped = PreparedEnvironment {
        plan_digest: plan_a.plan_digest.clone(),
        backend: CONTAINER_BACKEND.to_owned(),
        backend_version: plan_a.backend_version.clone(),
        enforced_dimensions: plan_a.required_dimensions.clone(),
        environment_id: Some(environment_name(&plan_b)),
    };
    let operation = ContainerExecOperation {
        argv: vec!["/bin/true".to_owned()],
        cwd: "/workspace".to_owned(),
        environment: BTreeMap::new(),
        timeout_ms: 1_000,
    };
    assert!(matches!(
        adapter.execute_argv(&swapped, operation).await,
        Err(PortError::Conflict(reason)) if reason == "container_environment_identity_mismatch"
    ));
}

#[tokio::test]
async fn plan_rejects_backend_that_reports_itself_disabled() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");
    let mut probe = supported_probe();
    probe.failure_reason = Some("fixture_backend_disabled".to_owned());
    assert_dimension_unavailable(adapter.plan(&scope_a, &probe).await);
}

#[tokio::test]
async fn plan_rejects_runsc_when_gvisor_dimension_is_not_enforced() {
    let mut config = config_without_runtime();
    config.runtime_name = "runsc".to_owned();
    let adapter = ContainerEnvironmentAdapter::new(config).expect("valid adapter");
    let scope_a = scope("owner-1", "scope-1");
    let mut probe = supported_probe();
    probe.capabilities.insert("gvisor_runtime".to_owned(), true);
    probe.enforced.insert("gvisor_runtime".to_owned(), false);
    assert_dimension_unavailable(adapter.plan(&scope_a, &probe).await);

    // A missing enforced marker must deny exactly like an explicit false marker.
    probe.enforced.remove("gvisor_runtime");
    assert_dimension_unavailable(adapter.plan(&scope_a, &probe).await);

    // Positive control: with the gvisor dimension enforced, the same selection plans.
    probe.enforced.insert("gvisor_runtime".to_owned(), true);
    let plan = adapter.plan(&scope_a, &probe).await.expect("runsc plan");
    assert_eq!(plan.backend, CONTAINER_BACKEND);
    assert_eq!(plan.backend_version, probe.backend_version);
}

#[tokio::test]
async fn probe_reports_missing_runtime_binary_with_stable_unavailable_code() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");
    match adapter.probe(&scope_a).await {
        Err(PortError::Unavailable(reason)) => {
            assert!(reason.starts_with("container_runtime_unavailable:"), "got {reason}");
        }
        other => panic!("missing runtime binary must fail closed, got {other:?}"),
    }
}

#[tokio::test]
async fn environment_plan_is_deterministic_for_same_scope() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");
    let probe = supported_probe();

    let first = adapter.plan(&scope_a, &probe).await.expect("first plan");
    let second = adapter.plan(&scope_a, &probe).await.expect("second plan");
    assert_eq!(first, second);
    assert_eq!(first.plan_digest, second.plan_digest);
    assert_eq!(environment_name(&first), environment_name(&second));
    assert_eq!(first.backend, CONTAINER_BACKEND);
    assert_eq!(first.owner_id, "owner-1");
    assert_eq!(first.scope_digest, "scope-1");

    // The digest is derived from server-owned inputs, not process-local adapter state.
    let fresh = adapter_fixture();
    let third = fresh.plan(&scope_a, &probe).await.expect("fresh plan");
    assert_eq!(first.plan_digest, third.plan_digest);

    // A different scope must not share the same plan identity or container name.
    let scope_b = scope("owner-1", "scope-2");
    let other = adapter.plan(&scope_b, &probe).await.expect("other plan");
    assert_ne!(first.plan_digest, other.plan_digest);
    assert_ne!(environment_name(&first), environment_name(&other));
    assert_eq!(other.scope_digest, "scope-2");
}

#[tokio::test]
async fn two_environments_on_same_host_keep_independent_identity_and_private_tmp() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");
    let scope_b = scope("owner-2", "scope-2");
    let probe = supported_probe();
    let plan_a = adapter.plan(&scope_a, &probe).await.expect("plan a");
    let plan_b = adapter.plan(&scope_b, &probe).await.expect("plan b");
    assert_ne!(plan_a.plan_digest, plan_b.plan_digest);
    let name_a = environment_name(&plan_a);
    let name_b = environment_name(&plan_b);
    assert_ne!(name_a, name_b);

    let config = adapter.config().clone();
    let workspace = config.workspace.canonicalize().expect("workspace");
    let expected_mount = format!("type=bind,src={},dst=/workspace,rw", workspace.display());
    let args_a = ContainerEnvironmentAdapter::create_args(
        &config,
        &plan_a.owner_id,
        &plan_a.scope_digest,
        &plan_a.plan_digest,
        &name_a,
    )
    .expect("create args a");
    let args_b = ContainerEnvironmentAdapter::create_args(
        &config,
        &plan_b.owner_id,
        &plan_b.scope_digest,
        &plan_b.plan_digest,
        &name_b,
    )
    .expect("create args b");
    assert_ne!(args_a, args_b);

    for (args, plan, name) in [(&args_a, &plan_a, &name_a), (&args_b, &plan_b, &name_b)] {
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--name" && pair[1] == name.as_str()));
        assert!(args.contains(&format!("io.kiana.plan={}", plan.plan_digest)));
        assert!(args.contains(&format!("io.kiana.owner={}", plan.owner_id)));
        // Each container gets its own private /tmp tmpfs; no host tmp path is bound in.
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "--tmpfs" && pair[1] == "/tmp:rw,noexec,nosuid,nodev"));
        let mount_sources = args
            .windows(2)
            .filter(|pair| pair[0] == "--mount")
            .map(|pair| pair[1].as_str())
            .collect::<Vec<_>>();
        assert_eq!(mount_sources, vec![expected_mount.as_str()]);
        assert!(!args.iter().any(|arg| arg.contains("src=/tmp")));
    }
}

/// The card-named deny path: a missing, wrong or self-reported-disabled backend must never be
/// accepted and must never degrade into the host environment. Planning runs against a runtime
/// binary that does not exist, so any host fallback would surface as a runtime I/O error instead
/// of the stable dimension code asserted here.
#[tokio::test]
async fn missing_or_wrong_backend_never_falls_back_to_host() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");

    // Wrong backend label: even with every required dimension claimed enforced, plan denies.
    let mut wrong_backend = supported_probe();
    wrong_backend.backend = String::from("bwrap");
    assert_dimension_unavailable(adapter.plan(&scope_a, &wrong_backend).await);

    // A backend that reports itself disabled must be denied before any plan digest exists.
    let mut disabled = supported_probe();
    disabled.failure_reason = Some(String::from("fixture_backend_disabled"));
    assert_dimension_unavailable(adapter.plan(&scope_a, &disabled).await);

    // A required dimension that is absent rather than explicitly false is still a denial, not a
    // silent host fallback.
    let mut missing = supported_probe();
    missing.capabilities.remove("network_none");
    missing.enforced.remove("network_none");
    assert_dimension_unavailable(adapter.plan(&scope_a, &missing).await);

    // No denied plan left recoverable state, so a fabricated plan cannot be prepared either.
    let forged = EnvironmentPlan {
        owner_id: scope_a.owner_id.clone(),
        scope_digest: scope_a.scope_digest.clone(),
        backend: CONTAINER_BACKEND.to_owned(),
        backend_version: String::from("fixture"),
        plan_digest: String::from(
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        ),
        required_dimensions: Vec::new(),
    };
    assert!(matches!(
        adapter.prepare(&forged).await,
        Err(PortError::Unavailable(reason)) if reason == "container_plan_not_found"
    ));
}

/// The card-named deny path: a backend may not report an unenforced required dimension as
/// supported. Admission requires the capability claim and an explicit `enforced=true` marker to
/// agree for every required dimension before a plan digest is created.
#[tokio::test]
async fn backend_cannot_report_unenforced_required_dimension_as_supported() {
    let adapter = adapter_fixture();
    let scope_a = scope("owner-1", "scope-1");

    for dimension in [
        "oci_container",
        "immutable_image",
        "non_root_user",
        "network_none",
        "resource_limits",
        "owner_scope_labels",
        "volume_root_identity",
        "no_host_control_socket",
    ] {
        // Capability claimed but no enforcement marker: denied, exactly like explicit false.
        let mut unmarked = supported_probe();
        unmarked.enforced.remove(dimension);
        assert_dimension_unavailable(adapter.plan(&scope_a, &unmarked).await);

        let mut unenforced = supported_probe();
        unenforced.enforced.insert(dimension.to_owned(), false);
        assert_dimension_unavailable(adapter.plan(&scope_a, &unenforced).await);

        // A false capability claim cannot be rescued by an `enforced=true` marker.
        let mut unsupported = supported_probe();
        unsupported.capabilities.insert(dimension.to_owned(), false);
        assert_dimension_unavailable(adapter.plan(&scope_a, &unsupported).await);
    }

    // Positive control: the fully supported, fully enforced probe still plans.
    let plan = adapter
        .plan(&scope_a, &supported_probe())
        .await
        .expect("supported backend plans");
    assert_eq!(plan.backend, CONTAINER_BACKEND);
}
