use kiana_daemon::eval_runtime::DenyByDefaultEvalBroker;
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityErrorCode, CapabilityKind, CapabilityRequest, RequestId,
};
use kiana_ports::CapabilityBrokerPort;
use serde_json::json;

fn request(capability: CapabilityKind, operation: &str) -> AuthorizedCapabilityRequest {
    AuthorizedCapabilityRequest::new(
        "policy:eq11",
        CapabilityRequest::new(RequestId::new(), capability, operation, json!({})),
    )
    .unwrap()
}

#[tokio::test]
async fn forbidden_capability_never_reaches_real_executor() {
    let broker = DenyByDefaultEvalBroker::new();
    let cases = [
        (CapabilityKind::Network, "http.request", "network"),
        (CapabilityKind::Secret, "secret.read", "secret"),
        (CapabilityKind::Network, "mcp.call", "mcp"),
        (
            CapabilityKind::Other("payment".to_owned()),
            "payment.charge",
            "payment",
        ),
        (CapabilityKind::Network, "publish.release", "publish"),
        (CapabilityKind::Computer, "desktop.click", "desktop"),
    ];

    for (capability, operation, category) in &cases {
        let request = request(capability.clone(), operation);
        let request_id = request.request.request_id;
        let result = broker.execute(request).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.request_id, request_id);
        assert_eq!(
            result.failure_code(),
            Some(CapabilityErrorCode::PermissionDenied)
        );
        assert_eq!(
            result.output["error"],
            format!("permission_denied:eval_effect_denied:{category}")
        );
        assert_eq!(result.output["not_executed"], json!(true));
        assert_eq!(result.output["effect_started"], json!(false));
    }
    assert_eq!(broker.denied_calls(), cases.len());
}

#[tokio::test]
async fn deny_reason_is_stable_and_cancellation_cannot_dispatch() {
    let broker = DenyByDefaultEvalBroker::new();
    let (sender, receiver) = tokio::sync::watch::channel(true);
    let request = request(CapabilityKind::Network, "mcp.discover");
    let result = broker.execute_cancellable(request, receiver).await.unwrap();
    drop(sender);
    assert_eq!(
        result.output["error"],
        "permission_denied:eval_effect_denied:mcp"
    );
    assert_eq!(broker.denied_calls(), 1);
}

#[tokio::test]
async fn unknown_effects_are_denied_by_default() {
    let broker = DenyByDefaultEvalBroker::new();
    let result = broker
        .execute(request(CapabilityKind::Query, "unclassified.effect"))
        .await
        .unwrap();
    assert_eq!(
        result.output["error"],
        "permission_denied:eval_effect_denied:unknown"
    );
    assert_eq!(broker.denied_calls(), 1);
}
