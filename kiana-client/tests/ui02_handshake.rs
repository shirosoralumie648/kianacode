use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_protocol::*;
use serde_json::json;

struct HandshakeTransport;

#[async_trait]
impl ClientTransport for HandshakeTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        let name = match &request.body {
            RequestBody::Command(command) => command.name.as_str(),
            _ => "",
        };
        let output = match name {
            "ui.initialize" => serde_json::to_value(UiHandshakeResponse {
                schema: UI_HANDSHAKE_RESPONSE_SCHEMA.to_owned(),
                server_version: "1.0".to_owned(),
                instance_id: "instance-1".to_owned(),
                authority_epoch: 1,
                surface: UiSurface::Cli,
                capabilities: Vec::new(),
                limitations: Vec::new(),
            })
            .unwrap(),
            "ui.health" => serde_json::to_value(UiHealth {
                schema: UI_HEALTH_SCHEMA.to_owned(),
                instance_id: "instance-1".to_owned(),
                authority_epoch: 1,
                status: "ready".to_owned(),
                capabilities: vec!["run".to_owned()],
                limitations: Vec::new(),
            })
            .unwrap(),
            _ => json!({"unexpected": true}),
        };
        Ok(ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: request.metadata.request_id,
            status: ExecutionStatus::Completed,
            output,
            error: None,
        })
    }
}

#[tokio::test]
async fn client_initialize_and_health_return_typed_handshake_data() {
    let client = KianaClient::new(HandshakeTransport);
    let metadata = RequestMetadata::local("session-1", "/repo");
    let handshake = client
        .initialize(
            metadata.clone(),
            UiHandshakeRequest {
                schema: UI_HANDSHAKE_REQUEST_SCHEMA.to_owned(),
                client_version: "1.0".to_owned(),
                surface: UiSurface::Cli,
                requested_capabilities: Vec::new(),
                known_instance_id: None,
                known_authority_epoch: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(handshake.instance_id, "instance-1");
    let health = client.health(metadata).await.unwrap();
    assert_eq!(health.status, "ready");
}

#[tokio::test]
async fn client_rejects_invalid_handshake_before_transport() {
    let client = KianaClient::new(HandshakeTransport);
    let error = client
        .initialize(
            RequestMetadata::local("session-1", "/repo"),
            UiHandshakeRequest {
                schema: "kiana.ui-handshake-request.v0".to_owned(),
                client_version: "1.0".to_owned(),
                surface: UiSurface::Cli,
                requested_capabilities: Vec::new(),
                known_instance_id: None,
                known_authority_epoch: None,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ClientError::Protocol(_)));
}
