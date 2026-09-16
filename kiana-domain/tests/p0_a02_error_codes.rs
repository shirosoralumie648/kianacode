use kiana_domain::{CapabilityErrorCode, CapabilityResult, RequestId};
use serde_json::json;

#[test]
fn path_escape_and_result_unknown_use_stable_non_retryable_policies() {
    let path_escape = CapabilityResult::failure(RequestId::new(), "path_escape");
    assert_eq!(
        path_escape.failure_code(),
        Some(CapabilityErrorCode::PathEscape)
    );
    let path_policy = CapabilityErrorCode::PathEscape.policy();
    assert_eq!(path_policy.cli_exit, 3);
    assert_eq!(path_policy.http_status, 403);
    assert!(!path_policy.retryable);
    assert!(path_policy.requires_new_authorization);

    let unknown = CapabilityResult {
        request_id: RequestId::new(),
        success: false,
        output: json!({"error":"result_unknown:provider_timeout"}),
        evidence_refs: Vec::new(),
    };
    assert_eq!(
        unknown.failure_code(),
        Some(CapabilityErrorCode::ResultUnknown)
    );
    assert!(
        unknown
            .failure_code()
            .unwrap()
            .policy()
            .requires_reconciliation
    );
    assert!(!unknown.failure_code().unwrap().policy().retryable);
}

#[test]
fn unknown_reason_keeps_conservative_execution_failed_classification() {
    assert_eq!(
        CapabilityErrorCode::from_reason("future_error:opaque"),
        CapabilityErrorCode::ExecutionFailed
    );
    assert_eq!(
        CapabilityErrorCode::from_reason("port_conflict:future_error"),
        CapabilityErrorCode::Conflict
    );
}
