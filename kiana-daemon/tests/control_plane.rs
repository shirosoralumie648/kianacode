use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_daemon::DaemonHost;
use kiana_protocol::{ExecutionStatus, RequestEnvelope, RequestMetadata, ResponseEnvelope};
use serde_json::Value;
use std::sync::Arc;

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
    let host = Arc::new(DaemonHost::local());
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
    let host = Arc::new(DaemonHost::local());
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
}

#[tokio::test]
async fn malformed_envelope_is_rejected_before_core() {
    let host = DaemonHost::local();
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
