use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, RequestId};
use kiana_ports::{CapabilityBrokerPort, PortError};
use serde_json::Value;
use std::sync::Arc;

struct NoopHandler;

#[async_trait]
impl CapabilityHandler for NoopHandler {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(CapabilityResult::success(
            request.request.request_id,
            Value::Null,
        ))
    }
}

struct MismatchedHandler;

#[async_trait]
impl CapabilityHandler for MismatchedHandler {
    fn binding_version(&self) -> &'static str {
        "kiana.handler-binding.f0"
    }

    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(CapabilityResult::success(
            request.request.request_id,
            Value::Null,
        ))
    }
}

#[test]
fn duplicate_alias_or_operation_is_rejected() {
    let mut broker = CapabilityBroker::new();
    broker
        .register_static(CapabilityKind::Process, "shell.exec", Arc::new(NoopHandler))
        .unwrap();
    assert_eq!(
        broker
            .register_static(CapabilityKind::Process, "shell.exec", Arc::new(NoopHandler),)
            .unwrap_err(),
        PortError::Conflict("capability_handler_already_registered".to_owned())
    );
    assert_eq!(
        broker
            .register_static(CapabilityKind::Process, "shell", Arc::new(NoopHandler))
            .unwrap_err(),
        PortError::Failed("capability_operation_unknown".to_owned())
    );
}

#[test]
fn descriptor_binding_version_mismatch_never_dispatches() {
    let mut broker = CapabilityBroker::new();
    assert_eq!(
        broker
            .register_static(
                CapabilityKind::Process,
                "shell.exec",
                Arc::new(MismatchedHandler),
            )
            .unwrap_err(),
        PortError::Failed("capability_binding_version_mismatch".to_owned())
    );
}

#[tokio::test]
async fn unregistered_operation_does_not_fallback_to_shell() {
    let broker = CapabilityBroker::new();
    let request = AuthorizedCapabilityRequest::new(
        "policy:cap01",
        kiana_domain::CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Process,
            "process.poll",
            serde_json::json!({"process_id":"p"}),
        ),
    )
    .unwrap();
    assert!(matches!(
        broker.execute(request).await,
        Err(PortError::Unavailable(_))
    ));
}
