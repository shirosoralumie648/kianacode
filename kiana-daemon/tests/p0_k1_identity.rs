use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::ExecutionStatus;
use kiana_protocol::{RequestEnvelope, RequestMetadata, ROLE_PM};
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

fn temp_project() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-p0-k1-{stamp}"));
    fs::create_dir_all(&root).expect("project directory");
    root
}

#[tokio::test]
async fn wire_actor_cannot_grant_role_or_department() {
    let harness = KianaHarness::new(Arc::new(
        ScriptedModel::from_json(&json!([{"text":"ok"}])).expect("cassette"),
    ));
    let host = DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProject))
        .expect("daemon");
    let root = temp_project();

    let mut first = RequestMetadata::local("p0-k1-session", root.to_string_lossy());
    first.project_trusted = true;
    let initial = host
        .handle(RequestEnvelope::run(
            first,
            "establish the server-owned assignment",
            Some("read-only".to_owned()),
        ))
        .await;
    assert_eq!(initial.status, ExecutionStatus::Completed, "{initial:?}");

    // A caller can declare a different actor, role and department on the wire, but an existing
    // session must remain bound to the assignment established by the authenticated daemon.
    let mut forged = RequestMetadata::local("p0-k1-session", root.to_string_lossy());
    forged.project_trusted = true;
    forged.actor_id = Some("wire-attacker".to_owned());
    forged.role_id = ROLE_PM.to_owned();
    forged.department_id = "planning".to_owned();
    let rejected = host
        .handle(RequestEnvelope::run(
            forged,
            "try to switch the assigned role",
            Some("read-only".to_owned()),
        ))
        .await;
    assert_eq!(rejected.status, ExecutionStatus::Blocked, "{rejected:?}");
    assert_eq!(
        rejected.error.as_deref(),
        Some("session_assignment_mismatch")
    );
    assert_eq!(rejected.output, serde_json::Value::Null);

    let _ = fs::remove_dir_all(root);
}
