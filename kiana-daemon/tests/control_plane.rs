use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::DaemonHost;
use kiana_protocol::{ExecutionStatus, RequestEnvelope, RequestMetadata, ResponseEnvelope};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
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

fn trusted_metadata() -> RequestMetadata {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.project_trusted = true;
    metadata
}

#[tokio::test]
async fn in_process_client_reaches_core_through_daemon() {
    let host = Arc::new(DaemonHost::local().unwrap());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .command(trusted_metadata(), "system.architecture", Value::Null)
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Completed);
    assert_eq!(response.output["control_plane"], "kiana-core");
    assert_eq!(response.output["composition_root"], "kiana-daemon");
}

#[tokio::test]
async fn unknown_and_untrusted_commands_are_blocked() {
    let host = Arc::new(DaemonHost::local().unwrap());
    let client = KianaClient::new(InProcessTransport { host });
    let unknown = client
        .command(trusted_metadata(), "unknown.command", Value::Null)
        .await
        .unwrap();
    assert_eq!(unknown.status, ExecutionStatus::Blocked);
    assert_eq!(unknown.error.as_deref(), Some("command_unregistered"));

    let untrusted = client
        .command(
            RequestMetadata::local("session-2", "/repo"),
            "system.architecture",
            Value::Null,
        )
        .await
        .unwrap();
    assert_eq!(untrusted.status, ExecutionStatus::Blocked);
    assert_eq!(untrusted.error.as_deref(), Some("project_untrusted"));

    let untrusted_query = client
        .command(
            RequestMetadata::local("session-3", "/repo"),
            "context.query.v1",
            json!({
                "operation": "repo_map",
                "output": "json",
                "options": {},
            }),
        )
        .await
        .unwrap();
    assert_eq!(untrusted_query.status, ExecutionStatus::Denied);
    assert_eq!(untrusted_query.error.as_deref(), Some("project_untrusted"));
}

#[tokio::test]
async fn malformed_envelope_is_rejected_before_core() {
    let host = DaemonHost::local().unwrap();
    let valid = RequestEnvelope::command(trusted_metadata(), "system.architecture", Value::Null);
    let mut invalid = valid.clone();
    invalid.schema = "kiana.protocol.v0".to_owned();
    let rejected = host.handle(invalid).await;
    assert_eq!(rejected.status, ExecutionStatus::Blocked);
    assert_eq!(
        rejected.error.as_deref(),
        Some("protocol_schema_unsupported")
    );

    let accepted = host.handle(valid).await;
    assert_eq!(accepted.status, ExecutionStatus::Completed);
}

#[tokio::test]
async fn repo_map_reaches_query_handler_and_uses_metadata_project_root() {
    let root = fixture_root("repo-map");
    let forged = fixture_root("forged-root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(&forged).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "pub struct RoutedWidget;\nfn routed_render() {}\n",
    )
    .unwrap();
    fs::write(forged.join("secret.txt"), "must not be scanned\n").unwrap();

    let host = Arc::new(DaemonHost::local().unwrap());
    let client = KianaClient::new(InProcessTransport { host });
    let mut metadata = RequestMetadata::local("session-query", root.to_string_lossy());
    metadata.project_trusted = true;
    let response = client
        .command(
            metadata,
            "context.query.v1",
            json!({
                "operation": "repo_map",
                "output": "json",
                "options": { "max_tokens": 1000 },
                "project_root": forged,
            }),
        )
        .await
        .unwrap();

    assert_eq!(response.status, ExecutionStatus::Completed);
    let result = &response.output["command_result"];
    assert_eq!(result["output_type"], "text");
    let map: Value = serde_json::from_str(result["value"].as_str().unwrap()).unwrap();
    assert_eq!(map["token_budget"], 1000);
    assert_eq!(map["files"][0]["path"], "src/lib.rs");
    assert!(map["files"][0]["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol == "struct RoutedWidget"));
    assert!(!result["value"].as_str().unwrap().contains("secret.txt"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(forged);
}

#[tokio::test]
async fn malformed_repo_map_intent_is_blocked_before_the_handler() {
    let host = Arc::new(DaemonHost::local().unwrap());
    let client = KianaClient::new(InProcessTransport { host });
    let response = client
        .command(
            trusted_metadata(),
            "context.query.v1",
            json!({
                "operation": "repo_map",
                "output": "json",
                "options": { "max_tokens": 0 },
            }),
        )
        .await
        .unwrap();
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(response.error.as_deref(), Some("command_arguments_invalid"));
}

fn fixture_root(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "kiana-daemon-{label}-{}-{nanos}",
        std::process::id()
    ))
}
