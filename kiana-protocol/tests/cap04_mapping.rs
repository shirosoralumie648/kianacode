use kiana_domain::{CapabilityErrorCode, ExecutionStatus, RequestId};
use kiana_protocol::{ResponseEnvelope, PROTOCOL_SCHEMA};
use serde_json::json;

#[test]
fn protocol_failure_mapping_keeps_cli_http_and_retry_policy_consistent() {
    let response = ResponseEnvelope {
        schema: PROTOCOL_SCHEMA.to_owned(),
        request_id: RequestId::new(),
        status: ExecutionStatus::Failed,
        output: json!({"error_code": "execution_failed", "retryable": false}),
        error: Some("execution_failed:shell_exit".to_owned()),
    };
    assert_eq!(
        response.failure_code(),
        Some(CapabilityErrorCode::ExecutionFailed)
    );
    let policy = response
        .failure_policy()
        .expect("failed response has policy");
    assert_eq!(policy.cli_exit, 1);
    assert_eq!(policy.http_status, 500);
    assert!(!policy.retryable);
    assert!(policy.requires_new_authorization);
}
