use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest, CapabilityResult, RequestId,
};
use kiana_ports::{CapabilityBrokerPort, PortError};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct RecordingHandler {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl CapabilityHandler for RecordingHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({
                "operation": request.request.operation,
                "authorization_id": request.authorization_id,
            }),
        ))
    }
}

#[tokio::test]
async fn registered_handler_is_selected_by_exact_capability_and_operation() {
    // 注册键由能力种类和完整操作名共同决定，成功路由后才允许 Handler 执行。
    let broker = CapabilityBroker::new();
    let calls = Arc::new(AtomicUsize::new(0));
    broker
        .register(
            CapabilityKind::Query,
            "search",
            Arc::new(RecordingHandler {
                calls: calls.clone(),
            }),
        )
        .await
        .unwrap();

    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        "search",
        json!({ "query": "architecture" }),
    );
    let authorized = AuthorizedCapabilityRequest::new("policy:query-1", request).unwrap();
    let result = broker.execute(authorized).await.unwrap();
    assert_eq!(result.output["operation"], "search");
    assert_eq!(result.output["authorization_id"], "policy:query-1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn near_aliases_are_rejected_without_invoking_a_handler() {
    // 未知或近似操作名不得模糊匹配到已有 Handler，避免借路由扩大能力面。
    let broker = CapabilityBroker::new();
    let calls = Arc::new(AtomicUsize::new(0));
    broker
        .register(
            CapabilityKind::Query,
            "search",
            Arc::new(RecordingHandler {
                calls: calls.clone(),
            }),
        )
        .await
        .unwrap();

    for operation in ["search ", "search.extra", "shell.exec"] {
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            operation,
            json!({}),
        );
        let error = broker
            .execute(AuthorizedCapabilityRequest::new("policy:test", request).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(error, PortError::Unavailable(reason) if reason.contains(operation)));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn asynchronous_registration_rejects_duplicate_keys() {
    // 动态注册与静态注册必须共享重复键规则，不能靠注册顺序覆盖既有执行含义。
    let broker = CapabilityBroker::new();
    let handler = Arc::new(RecordingHandler {
        calls: Arc::new(AtomicUsize::new(0)),
    });
    broker
        .register(CapabilityKind::Filesystem, "read", handler.clone())
        .await
        .unwrap();
    let error = broker
        .register(CapabilityKind::Filesystem, "read", handler)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PortError::Conflict("capability_handler_already_registered".to_owned())
    );
}
