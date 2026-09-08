use async_trait::async_trait;
use kiana_client::{ClientError, ClientTransport, KianaClient};
use kiana_protocol::{
    ApprovalDecision, ConversationMessage, ConversationRole, ExecutionStatus, RequestBody,
    RequestEnvelope, RequestMetadata, ResponseEnvelope, WorkPacket,
};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct RecordingTransport {
    requests: Arc<Mutex<Vec<RequestEnvelope>>>,
    fail: bool,
}

#[async_trait]
impl ClientTransport for RecordingTransport {
    async fn send(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, ClientError> {
        self.requests.lock().unwrap().push(request.clone());
        if self.fail {
            return Err(ClientError::Transport("offline".to_owned()));
        }
        Ok(ResponseEnvelope {
            schema: kiana_protocol::PROTOCOL_SCHEMA.to_owned(),
            request_id: request.metadata.request_id,
            status: ExecutionStatus::Completed,
            output: Value::Null,
            error: None,
        })
    }
}

#[tokio::test]
async fn every_public_client_method_delegates_a_typed_envelope() {
    // 客户端只负责构造协议 envelope；每个公开方法都应经过同一个 transport。
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = KianaClient::new(RecordingTransport {
        requests: requests.clone(),
        fail: false,
    });
    let metadata = RequestMetadata::local("session-1", "/repo");
    client
        .command(metadata.clone(), "system.architecture", Value::Null)
        .await
        .unwrap();
    client
        .approval_decision_with_proof(
            metadata.clone(),
            kiana_protocol::ApprovalId::new(),
            ApprovalDecision::Approve,
            Some("hash".to_owned()),
            Some("nonce".to_owned()),
        )
        .await
        .unwrap();
    client
        .run_with_history(
            metadata.clone(),
            "run",
            vec![ConversationMessage {
                role: ConversationRole::User,
                text: "history".to_owned(),
                tool_call_id: None,
            }],
            Some("read-only".to_owned()),
        )
        .await
        .unwrap();
    let run_id = kiana_protocol::RunId::new();
    client
        .continue_run(metadata.clone(), "continue", None, Some(run_id))
        .await
        .unwrap();
    client
        .cancel_run(metadata.clone(), Some(run_id), "user")
        .await
        .unwrap();
    client
        .receipt(metadata.clone(), Some(run_id))
        .await
        .unwrap();
    client
        .spawn(
            metadata.clone(),
            WorkPacket::builder_task("wp-1", "test"),
            Some("workspace-write".to_owned()),
        )
        .await
        .unwrap();
    client
        .convene(metadata.clone(), "decide", true, 2, None)
        .await
        .unwrap();
    client
        .review(metadata.clone(), "builder-session", Some(run_id))
        .await
        .unwrap();
    client
        .close(metadata, "builder-session", Some(run_id))
        .await
        .unwrap();

    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 10);
    assert!(matches!(&requests[0].body, RequestBody::Command(_)));
    assert!(matches!(
        &requests[1].body,
        RequestBody::ApprovalDecision(_)
    ));
    assert!(matches!(&requests[2].body, RequestBody::Run(_)));
    assert!(matches!(&requests[3].body, RequestBody::Continue(_)));
    assert!(matches!(&requests[4].body, RequestBody::Cancel(_)));
    assert!(matches!(&requests[5].body, RequestBody::Receipt(_)));
    assert!(matches!(&requests[6].body, RequestBody::Spawn(_)));
    assert!(matches!(&requests[7].body, RequestBody::Symposium(_)));
    assert!(matches!(&requests[8].body, RequestBody::Review(_)));
    assert!(matches!(&requests[9].body, RequestBody::Close(_)));
}

#[tokio::test]
async fn transport_errors_are_returned_without_becoming_business_responses() {
    // transport 故障必须原样返回 ClientError，不能伪造 Completed 或 Blocked 响应。
    let client = KianaClient::new(RecordingTransport {
        requests: Arc::new(Mutex::new(Vec::new())),
        fail: true,
    });
    let error = client
        .command(
            RequestMetadata::local("session-1", "/repo"),
            "system.architecture",
            Value::Null,
        )
        .await
        .unwrap_err();
    assert_eq!(error, ClientError::Transport("offline".to_owned()));
}
