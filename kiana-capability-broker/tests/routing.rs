use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthenticatedPrincipalRef, AuthorizedCapabilityRequest, CapabilityKind, CapabilityRequest,
    CapabilityResult, ExecutionScope, ProjectIdentity, RequestId, RiskLevel, ScopeDimension,
    ScopeLimit, ScopeSet, SessionId,
};
use kiana_ports::{CapabilityBrokerPort, ExecutionPermitVerifierPort, PortError};
use serde_json::json;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

struct RecordingHandler {
    calls: Arc<AtomicUsize>,
    binding_version: &'static str,
}

struct NoopHandler {
    binding_version: &'static str,
}

struct TestPermitVerifier;

#[async_trait]
impl ExecutionPermitVerifierPort for TestPermitVerifier {
    async fn verify_and_consume(
        &self,
        _request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

#[async_trait]
impl CapabilityHandler for NoopHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(CapabilityResult::success(
            request.request.request_id,
            serde_json::Value::Null,
        ))
    }
}

#[async_trait]
impl CapabilityHandler for RecordingHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

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

fn sealed_broker(handler: Arc<dyn CapabilityHandler>) -> CapabilityBroker {
    sealed_broker_for(handler, "memory.search")
}

fn sealed_broker_for(
    handler: Arc<dyn CapabilityHandler>,
    target_operation: &str,
) -> CapabilityBroker {
    let mut broker = CapabilityBroker::new();
    for operation in kiana_domain::ACTION_OPERATIONS {
        let descriptor = kiana_domain::capability_action_descriptor(operation)
            .expect("catalog operation has a descriptor");
        let handler: Arc<dyn CapabilityHandler> = if *operation == target_operation {
            handler.clone()
        } else {
            Arc::new(NoopHandler {
                binding_version: descriptor.binding_version,
            })
        };
        broker
            .register_static(descriptor.capability, *operation, handler)
            .unwrap();
    }
    broker.validate_catalog_bindings().unwrap();
    broker.set_permit_verifier(Arc::new(TestPermitVerifier));
    broker
}

fn action_scope(request: &CapabilityRequest) -> ExecutionScope {
    let permission_scope = ScopeSet::new(
        ScopeDimension::Restricted(vec![request.operation.clone()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .unwrap();
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        kiana_domain::json_digest(&json!("trusted")),
    )
    .unwrap();
    let mut scope = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal,
        project,
        session_id: SessionId::new("routing-test"),
        run_id: None,
        turn_id: None,
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec!["/repo".to_owned()],
        write_roots: Vec::new(),
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision: kiana_domain::json_digest(&json!("trusted")),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest: kiana_domain::capability_action_digest(request),
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    scope
}

#[tokio::test]
async fn registered_handler_is_selected_by_exact_capability_and_operation() {
    // 注册键由能力种类和完整操作名共同决定，成功路由后才允许 Handler 执行。
    let calls = Arc::new(AtomicUsize::new(0));
    let descriptor = kiana_domain::capability_action_descriptor("memory.search").unwrap();
    assert_eq!(descriptor.capability, CapabilityKind::Query);
    let broker = sealed_broker(Arc::new(RecordingHandler {
        calls: calls.clone(),
        binding_version: descriptor.binding_version,
    }));
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        "memory.search",
        json!({ "query": "architecture" }),
    )
    .with_risk(RiskLevel::ReadOnly);
    kiana_domain::normalize_capability_action(&mut request).unwrap();
    request.execution_scope = Some(action_scope(&request));
    let authorized = AuthorizedCapabilityRequest::new("policy:query-1", request).unwrap();
    let result = broker.execute(authorized).await.unwrap();
    assert_eq!(result.output["operation"], "memory.search");
    assert_eq!(result.output["authorization_id"], "policy:query-1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn sealed_broker_routes_spec_kinds_without_fallback() {
    for operation in ["apply_patch.preview", "connector.mcp_handshake"] {
        let descriptor = kiana_domain::capability_action_descriptor(operation).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let broker = sealed_broker_for(
            Arc::new(RecordingHandler {
                calls: calls.clone(),
                binding_version: descriptor.binding_version,
            }),
            operation,
        );

        let mut correct = CapabilityRequest::new(
            RequestId::new(),
            descriptor.capability.clone(),
            operation,
            json!({}),
        )
        .with_risk(RiskLevel::ReadOnly);
        kiana_domain::normalize_capability_action(&mut correct).unwrap();
        correct.execution_scope = Some(action_scope(&correct));
        let result = broker
            .execute(AuthorizedCapabilityRequest::new("policy:cap01", correct).unwrap())
            .await
            .unwrap();
        assert_eq!(result.output["operation"], operation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let wrong_kind = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            operation,
            json!({}),
        )
        .with_risk(RiskLevel::ReadOnly);
        assert_eq!(
            broker
                .execute(AuthorizedCapabilityRequest::new("policy:wrong-kind", wrong_kind).unwrap())
                .await
                .unwrap_err(),
            PortError::Failed("action_capability_mismatch".to_owned())
        );

        let unknown = CapabilityRequest::new(
            RequestId::new(),
            descriptor.capability,
            format!("{operation}.unknown"),
            json!({}),
        )
        .with_risk(RiskLevel::ReadOnly);
        assert!(matches!(
            broker
                .execute(AuthorizedCapabilityRequest::new("policy:unknown", unknown).unwrap())
                .await,
            Err(PortError::Failed(reason)) if reason == "action_operation_unknown"
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn near_aliases_are_rejected_without_invoking_a_handler() {
    // 未知或近似操作名不得模糊匹配到已有 Handler，避免借路由扩大能力面。
    let calls = Arc::new(AtomicUsize::new(0));
    let descriptor = kiana_domain::capability_action_descriptor("memory.search").unwrap();
    let broker = sealed_broker(Arc::new(RecordingHandler {
        calls: calls.clone(),
        binding_version: descriptor.binding_version,
    }));

    for (capability, operation, arguments, expected) in [
        (
            CapabilityKind::Query,
            "memory.search ",
            json!({ "query": "architecture" }),
            "action_operation_unknown",
        ),
        (
            CapabilityKind::Query,
            "memory.search.extra",
            json!({ "query": "architecture" }),
            "action_operation_unknown",
        ),
        (
            CapabilityKind::Query,
            "shell.exec",
            json!({ "command": "pwd", "sandbox": "read-only" }),
            "action_capability_mismatch",
        ),
    ] {
        let request = CapabilityRequest::new(RequestId::new(), capability, operation, arguments);
        let error = broker
            .execute(AuthorizedCapabilityRequest::new("policy:test", request).unwrap())
            .await
            .unwrap_err();
        assert_eq!(error, PortError::Failed(expected.to_owned()));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn asynchronous_registration_rejects_duplicate_keys() {
    // 动态注册与静态注册必须共享重复键规则，不能靠注册顺序覆盖既有执行含义。
    let descriptor = kiana_domain::capability_action_descriptor("memory.write").unwrap();
    assert_eq!(descriptor.capability, CapabilityKind::Filesystem);
    let broker = CapabilityBroker::new();
    let handler = Arc::new(RecordingHandler {
        calls: Arc::new(AtomicUsize::new(0)),
        binding_version: descriptor.binding_version,
    });
    broker
        .register(
            descriptor.capability.clone(),
            descriptor.operation,
            handler.clone(),
        )
        .await
        .unwrap();
    let error = broker
        .register(descriptor.capability, descriptor.operation, handler)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        PortError::Conflict("capability_handler_already_registered".to_owned())
    );
}
