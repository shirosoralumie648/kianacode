use async_trait::async_trait;
use kiana_domain::{
    AgentTemplate, CapabilityGrantId, CapabilityKind, CapabilityRequest, CellId, CellLifecycle,
    CellSpec, RequestContext, RequestId, RetirementRecord, RunId, RuntimeEvent, SpawnPlanId,
};
use kiana_ports::{
    AllowAllPreToolHooks, CellRegistryPort, EventStorePort, PortError, PreToolHookDecision,
    PreToolHookPort, SpawnReservation, SpawnReservationRequest,
};
use serde_json::Value;
use std::sync::Mutex;

struct UnsupportedCellRegistry;

#[async_trait]
impl CellRegistryPort for UnsupportedCellRegistry {
    async fn resolve_template(
        &self,
        _role_id: &str,
        _version: &str,
    ) -> Result<AgentTemplate, PortError> {
        Err(PortError::Failed("stub".to_owned()))
    }

    async fn reserve_spawn(
        &self,
        _request: SpawnReservationRequest,
    ) -> Result<SpawnReservation, PortError> {
        Err(PortError::Failed("stub".to_owned()))
    }

    async fn transition_cell(
        &self,
        _cell_id: CellId,
        _expected: CellLifecycle,
        _next: CellLifecycle,
    ) -> Result<CellSpec, PortError> {
        Err(PortError::Failed("stub".to_owned()))
    }

    async fn abort_spawn(&self, _plan_id: SpawnPlanId, _reason: &str) -> Result<(), PortError> {
        Err(PortError::Failed("stub".to_owned()))
    }

    async fn retire_cell(
        &self,
        _cell_id: CellId,
        _reason: &str,
    ) -> Result<RetirementRecord, PortError> {
        Err(PortError::Failed("stub".to_owned()))
    }

    async fn cell_for_run(&self, _run_id: RunId) -> Result<Option<CellId>, PortError> {
        Err(PortError::Failed("stub".to_owned()))
    }
}

#[derive(Default)]
struct MemoryEventStore {
    events: Mutex<Vec<RuntimeEvent>>,
}

#[async_trait]
impl EventStorePort for MemoryEventStore {
    async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }

    async fn read_request(&self, request_id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| &event.request_id == request_id)
            .cloned()
            .collect())
    }

    async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
        Ok(self.events.lock().unwrap().clone())
    }
}

#[tokio::test]
async fn cell_registry_advanced_defaults_fail_closed() {
    // 未升级的注册表不能默默跳过能力围栏、结算、提交或权威查询。
    let registry = UnsupportedCellRegistry;
    let request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Query,
        "search",
        Value::Null,
    );
    assert_eq!(
        registry
            .begin_capability(
                CellId::new(),
                CapabilityGrantId::new(),
                kiana_domain::BudgetLeaseId::new(),
                &request,
            )
            .await
            .unwrap_err(),
        PortError::Failed("cell_registry_capability_fence_unsupported".to_owned())
    );
    assert_eq!(
        registry.commit_spawn(SpawnPlanId::new()).await.unwrap_err(),
        PortError::Failed("cell_registry_commit_unsupported".to_owned())
    );
    assert_eq!(
        registry
            .reservation_for_cell(CellId::new())
            .await
            .unwrap_err(),
        PortError::Failed("cell_registry_lookup_unsupported".to_owned())
    );
}

#[tokio::test]
async fn event_store_defaults_preserve_append_and_stream_boundaries() {
    // 简单事件适配器可以使用无条件追加和默认流过滤，但不能伪装支持 CAS 或幂等。
    let store = MemoryEventStore::default();
    let request_id = RequestId::new();
    let first = RuntimeEvent::new(request_id, 1, "accepted", Value::Null)
        .unwrap()
        .with_stream_metadata("request", request_id.to_string(), 1);
    store.append_expected(first.clone(), None).await.unwrap();
    assert_eq!(
        store
            .append_expected(first.clone(), Some(1))
            .await
            .unwrap_err(),
        PortError::Failed("event_store_expected_version_unsupported".to_owned())
    );
    assert_eq!(
        store.append_idempotent(first.clone()).await.unwrap_err(),
        PortError::Failed("event_store_idempotency_unsupported".to_owned())
    );
    let other = RuntimeEvent::new(RequestId::new(), 1, "other", Value::Null)
        .unwrap()
        .with_stream_metadata("request", "other", 1);
    store.append(other).await.unwrap();
    let stream = store
        .read_stream("request", &request_id.to_string())
        .await
        .unwrap();
    assert_eq!(stream, vec![first]);
}

#[tokio::test]
async fn allow_all_hook_only_returns_a_non_authorizing_allow_decision() {
    // AllowAll 钩子只表示没有额外拦截，后续控制面仍必须继续执行策略和审批。
    let context = RequestContext::local("session-1", "/repo");
    let request = CapabilityRequest::new(
        context.request_id,
        CapabilityKind::Query,
        "search",
        Value::Null,
    );
    assert_eq!(
        AllowAllPreToolHooks
            .decide(&context, &request)
            .await
            .unwrap(),
        PreToolHookDecision::Allow
    );
}
