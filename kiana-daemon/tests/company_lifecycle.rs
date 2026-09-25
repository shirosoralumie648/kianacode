use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::*;
use kiana_protocol::{
    CompanyCommand, CompanyCommandRequest, ExecutionStatus, PermissionProfile, RequestEnvelope,
    RequestMetadata, ResponseEnvelope, RoleSpec,
};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy)]
struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-co47-{label}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("project root");
    root
}

fn host() -> DaemonHost {
    let model = ScriptedModel::from_json(&json!([{"text":"unused fixture response"}]))
        .expect("fake model cassette");
    let harness = KianaHarness::new(Arc::new(model));
    DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProject))
        .expect("daemon host")
}

fn metadata(root: &Path, session: &str, role: RoleSpec) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata.permission_profile = PermissionProfile::Balanced;
    metadata.assign_role(&role);
    metadata
}

async fn send(
    host: &DaemonHost,
    root: &Path,
    session: &str,
    role: RoleSpec,
    expected_revision: u64,
    idempotency_key: &str,
    command: CompanyCommand,
) -> ResponseEnvelope {
    let request = CompanyCommandRequest {
        schema: COMPANY_COMMAND_SCHEMA.to_owned(),
        expected_revision,
        idempotency_key: idempotency_key.to_owned(),
        command,
    };
    let envelope = RequestEnvelope::company_command(metadata(root, session, role), request)
        .expect("company command envelope");
    host.handle(envelope).await
}

#[test]
fn fixture_declares_deny_recovery_and_effect_accounting_contract() {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/co47-company-lifecycle.json"))
            .expect("CO-47 fixture");
    assert_eq!(value["schema"], "kiana.company-lifecycle-fixture.v1");
    for marker in ["role denial", "idempotency", "rejected event", "golden"] {
        assert!(value["cases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|case| case.as_str().unwrap().contains(marker)));
    }
}

#[tokio::test]
async fn denied_company_commands_record_rejection_without_runtime_effect() {
    let root = temp_project("deny");
    let host = host();

    let role_denied = send(
        &host,
        &root,
        "builder-session",
        RoleSpec::builder(),
        0,
        "forged-approve",
        CompanyCommand::ApproveProject {
            project_id: "missing-project".to_owned(),
            decision_ref: "artifact:missing".to_owned(),
        },
    )
    .await;
    assert_eq!(role_denied.status, ExecutionStatus::Blocked);
    assert_eq!(role_denied.error.as_deref(), Some("company_role_denied"));

    let missing_project = send(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        0,
        "close-missing",
        CompanyCommand::CloseProject {
            project_id: "missing-project".to_owned(),
            delivery_id: "missing-delivery".to_owned(),
            receipt_id: "closing-missing".to_owned(),
            evidence_refs: vec!["event:missing".to_owned()],
        },
    )
    .await;
    assert_eq!(missing_project.status, ExecutionStatus::Blocked);
    assert!(missing_project.error.is_some());
    assert!(!root.join("OUTPUT.txt").exists());

    let events = host
        .persisted_events()
        .await
        .expect("event read")
        .expect("events");
    assert!(
        events
            .iter()
            .filter(|event| event.kind == "company.command_rejected")
            .count()
            >= 2
    );
    assert!(!events
        .iter()
        .any(|event| event.kind == "company.ProjectClosed"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[tokio::test]
async fn idempotency_payload_drift_is_blocked_without_duplicate_business_fact() {
    let root = temp_project("idempotency");
    fs::write(root.join("charter.json"), "charter fixture\n").expect("charter");
    let host = host();

    let first = send(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        0,
        "register-charter",
        CompanyCommand::RegisterArtifact {
            artifact_id: "charter".to_owned(),
            relative_path: "charter.json".to_owned(),
        },
    )
    .await;
    assert_eq!(first.status, ExecutionStatus::Completed, "{first:?}");

    let drift = send(
        &host,
        &root,
        "sponsor-session",
        RoleSpec::sponsor(),
        0,
        "register-charter",
        CompanyCommand::RegisterArtifact {
            artifact_id: "charter".to_owned(),
            relative_path: "forged.json".to_owned(),
        },
    )
    .await;
    assert_eq!(drift.status, ExecutionStatus::Blocked);
    assert_eq!(drift.error.as_deref(), Some("company_idempotency_conflict"));
    assert!(!root.join("forged.json").exists());

    let events = host
        .persisted_events()
        .await
        .expect("event read")
        .expect("events");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "company.ArtifactRegistered")
            .count(),
        1
    );
    assert!(events
        .iter()
        .any(|event| event.kind == "company.command_rejected"));
    fs::remove_dir_all(root).expect("cleanup");
}
