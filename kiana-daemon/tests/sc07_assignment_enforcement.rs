//! SC-07 DaemonHost server-assignment enforcement fixtures.
//!
//! Deny-first: an enforcement host (`with_assignment_enforcement`) must reject effect requests
//! that cannot resolve an active server assignment (missing / wire-role-drifted / revoked)
//! before any model call, authority synchronization or session binding. A matching assignment
//! proceeds through the existing Core path. Denial responses carry only the stable reason code
//! and never echo caller material.

use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{
    AssignmentId, AuthenticatedPrincipalRef, OrganizationId, ProjectAssignment,
    ProjectAssignmentId, RoleAssignment, RoleAssignmentStatus,
};
use kiana_ports::AssignmentDirectoryPort;
use kiana_protocol::{
    ExecutionStatus, PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope, RoleSpec,
};
use kiana_runner::{KianaHarness, ModelClient, ModelOutput, ModelRequest, ScriptedModel};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

struct InProcessTransport {
    host: Arc<DaemonHost>,
}

#[async_trait]
impl ClientTransport for InProcessTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        Ok(self.host.handle(request).await)
    }
}

#[derive(Clone, Copy)]
struct FixedProjectTrustAuthority {
    trusted: bool,
}

impl ProjectTrustAuthority for FixedProjectTrustAuthority {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(self.trusted)
    }
}

/// Model double that counts dispatches and always fails: a fixture that reaches the model after
/// an expected assignment denial is itself a failure.
struct CountingModel {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelClient for CountingModel {
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err("assignment_denial_must_precede_model_dispatch".to_owned())
    }
}

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-{label}-{stamp}"));
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn host_with_trust(model: Arc<dyn ModelClient>, trusted: bool) -> DaemonHost {
    DaemonHost::with_harness_and_project_authority(
        KianaHarness::new(model),
        Arc::new(FixedProjectTrustAuthority { trusted }),
    )
    .expect("fixture daemon")
}

fn trusted_host(model: Arc<dyn ModelClient>) -> DaemonHost {
    host_with_trust(model, true)
}

fn trusted_metadata(label: &str, root: &Path) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(label, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn apply_patch_cassette() -> serde_json::Value {
    json!([
        {
            "text": "writing",
            "tool_calls": [{
                "id": "c1",
                "name": "apply_patch",
                "arguments": {
                    "patch": "*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"
                }
            }]
        },
        {"text": "created GOLDEN_PATH.txt"}
    ])
}

/// Register one active `builder` role + project assignment for the daemon's own principal and the
/// daemon-resolved project identity. The organization id must be the one the host enforces.
async fn provision_builder_assignment(
    host: &DaemonHost,
    project_root: &Path,
    organization_id: OrganizationId,
) -> AssignmentId {
    let principal = AuthenticatedPrincipalRef::local();
    let project = host
        .project_identity(&project_root.to_string_lossy())
        .expect("daemon project identity");
    let assignment_id = AssignmentId::new();
    let role = RoleAssignment::new(
        assignment_id,
        principal.clone(),
        organization_id,
        "builder",
        "executing",
        vec![project.project_id],
        1,
        u64::MAX,
        RoleAssignmentStatus::Active,
        1,
        1,
    )
    .expect("role assignment fixture");
    host.assignment_directory()
        .register_role(role)
        .await
        .expect("register role assignment");
    let project_assignment = ProjectAssignment::new(
        ProjectAssignmentId::new(),
        assignment_id,
        principal,
        organization_id,
        project.project_id,
        1,
        u64::MAX,
        false,
        1,
        2,
    )
    .expect("project assignment fixture");
    host.assignment_directory()
        .register_project(project_assignment)
        .await
        .expect("register project assignment");
    assignment_id
}

#[tokio::test]
async fn missing_assignment_is_rejected_before_any_effect() {
    let root = temp_project("sc07-missing");
    let calls = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        trusted_host(Arc::new(CountingModel {
            calls: calls.clone(),
        }))
        .with_assignment_enforcement(OrganizationId::new()),
    );
    let client = KianaClient::new(InProcessTransport { host: host.clone() });

    let mut request_metadata = trusted_metadata("sc07-missing", &root);
    request_metadata.permission_profile = PermissionProfile::Balanced;
    let response = client
        .run(
            request_metadata,
            "sc07-sentinel-token 5f4dcc3b",
            Some("workspace-write".to_owned()),
        )
        .await
        .expect("daemon response");

    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("assignment_expired_or_missing")
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "assignment denial must precede model dispatch"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
    assert!(response.output.is_null());
    let rendered = serde_json::to_string(&response).expect("response json");
    assert!(
        !rendered.contains("sc07-sentinel-token"),
        "denial response leaked caller material: {rendered}"
    );

    let events = host
        .persisted_events()
        .await
        .expect("event store read")
        .expect("memory event store supports full read");
    assert!(
        !events
            .iter()
            .any(|event| event.kind == "session.assigned" || event.kind.starts_with("run.")),
        "assignment denial must precede session binding and run events: {events:?}"
    );
}

#[tokio::test]
async fn wire_role_drift_from_server_assignment_is_rejected_without_effect() {
    let root = temp_project("sc07-drift");
    let organization_id = OrganizationId::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let host = trusted_host(Arc::new(CountingModel {
        calls: calls.clone(),
    }))
    .with_assignment_enforcement(organization_id);
    provision_builder_assignment(&host, &root, organization_id).await;
    let host = Arc::new(host);
    let client = KianaClient::new(InProcessTransport { host });

    let mut request_metadata = trusted_metadata("sc07-drift", &root);
    request_metadata.permission_profile = PermissionProfile::Balanced;
    request_metadata.assign_role(&RoleSpec::pm());
    let response = client
        .run(
            request_metadata,
            "retitle the module",
            Some("workspace-write".to_owned()),
        )
        .await
        .expect("daemon response");

    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("assignment_role_mismatch"));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "wire role drift must not reach the model"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn revoked_assignment_is_rejected_after_earlier_resolution() {
    let root = temp_project("sc07-revoked");
    let organization_id = OrganizationId::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let host = trusted_host(Arc::new(CountingModel {
        calls: calls.clone(),
    }))
    .with_assignment_enforcement(organization_id);
    let assignment_id = provision_builder_assignment(&host, &root, organization_id).await;

    let resolved = host
        .resolve_assignment_for_project(
            &root.to_string_lossy(),
            organization_id,
            "builder",
            now_ms(),
        )
        .await
        .expect("assignment resolves before revocation");
    assert_eq!(resolved.role_id, "builder");

    host.assignment_directory()
        .revoke_assignment(assignment_id)
        .await
        .expect("revoke assignment");

    let host = Arc::new(host);
    let client = KianaClient::new(InProcessTransport { host });
    let mut request_metadata = trusted_metadata("sc07-revoked", &root);
    request_metadata.permission_profile = PermissionProfile::Balanced;
    let response = client
        .run(
            request_metadata,
            "write after revocation",
            Some("workspace-write".to_owned()),
        )
        .await
        .expect("daemon response");

    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("assignment_expired_or_missing")
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "revoked assignment must not reach the model"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn forged_wire_trust_with_valid_assignment_is_rejected_by_server_snapshot() {
    let root = temp_project("sc07-trust");
    let organization_id = OrganizationId::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let host = host_with_trust(
        Arc::new(CountingModel {
            calls: calls.clone(),
        }),
        false,
    )
    .with_assignment_enforcement(organization_id);
    provision_builder_assignment(&host, &root, organization_id).await;
    let host = Arc::new(host);
    let client = KianaClient::new(InProcessTransport { host });

    let mut request_metadata = trusted_metadata("sc07-trust", &root);
    request_metadata.project_trusted = true;
    request_metadata.permission_profile = PermissionProfile::Balanced;
    let response = client
        .run(
            request_metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .expect("daemon response");

    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(response.error.as_deref(), Some("project_untrusted"));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "untrusted project must not reach the model"
    );
    assert!(!root.join("GOLDEN_PATH.txt").exists());
}

#[tokio::test]
async fn matching_assignment_proceeds_through_core() {
    let root = temp_project("sc07-valid");
    let organization_id = OrganizationId::new();
    let scripted = ScriptedModel::from_json(&apply_patch_cassette()).expect("scripted model");
    let host = trusted_host(Arc::new(scripted)).with_assignment_enforcement(organization_id);
    provision_builder_assignment(&host, &root, organization_id).await;
    let host = Arc::new(host);
    let client = KianaClient::new(InProcessTransport { host });

    let mut request_metadata = trusted_metadata("sc07-valid", &root);
    request_metadata.permission_profile = PermissionProfile::Balanced;
    let response = client
        .run(
            request_metadata,
            "create a file named GOLDEN_PATH.txt containing hello",
            Some("workspace-write".to_owned()),
        )
        .await
        .expect("daemon response");

    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    assert_eq!(response.output["role_id"], "builder");
    assert_eq!(response.output["department_id"], "executing");
    assert_eq!(response.output["files_changed"][0], "GOLDEN_PATH.txt");
    assert_eq!(
        std::fs::read_to_string(root.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn daemon_handle_calls_server_assignment_enforcement() {
    let daemon = include_str!("../src/lib.rs");
    for marker in [
        "with_assignment_enforcement",
        "assignment_enforcement",
        ".enforce_assignment(",
    ] {
        assert!(
            daemon.contains(marker),
            "daemon assignment enforcement marker missing: {marker}"
        );
    }
}
