//! Authorized capability routing for Kiana daemon adapters.

use async_trait::async_trait;
use kiana_domain::{AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult};
use kiana_ports::{CapabilityBrokerPort, PortError};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

type HandlerKey = (CapabilityKind, String);

#[async_trait]
pub trait CapabilityHandler: Send + Sync {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError>;
}

#[derive(Default)]
pub struct CapabilityBroker {
    handlers: RwLock<HashMap<HandlerKey, Arc<dyn CapabilityHandler>>>,
}

impl CapabilityBroker {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(
        &self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
    ) -> Result<(), PortError> {
        let mut handlers = self.handlers.write().await;
        insert_handler(&mut handlers, capability, operation.into(), handler)
    }

    pub fn register_static(
        &mut self,
        capability: CapabilityKind,
        operation: impl Into<String>,
        handler: Arc<dyn CapabilityHandler>,
    ) -> Result<(), PortError> {
        insert_handler(
            self.handlers.get_mut(),
            capability,
            operation.into(),
            handler,
        )
    }
}

fn insert_handler(
    handlers: &mut HashMap<HandlerKey, Arc<dyn CapabilityHandler>>,
    capability: CapabilityKind,
    operation: String,
    handler: Arc<dyn CapabilityHandler>,
) -> Result<(), PortError> {
    let key = (capability, operation);
    if handlers.contains_key(&key) {
        return Err(PortError::Conflict(
            "capability_handler_already_registered".to_owned(),
        ));
    }
    handlers.insert(key, handler);
    Ok(())
}

#[async_trait]
impl CapabilityBrokerPort for CapabilityBroker {
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let key = (
            request.request.capability.clone(),
            request.request.operation.clone(),
        );
        let handler = self.handlers.read().await.get(&key).cloned();
        let Some(handler) = handler else {
            return Err(PortError::Unavailable(format!(
                "capability_unregistered:{:?}:{}",
                key.0, key.1
            )));
        };
        handler.execute(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::{CapabilityRequest, RequestId};
    use serde_json::Value;

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

    #[tokio::test]
    async fn unregistered_capability_fails_closed() {
        let broker = CapabilityBroker::new();
        let request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Query,
            "search",
            Value::Null,
        );
        let request = AuthorizedCapabilityRequest::new("policy:test", request).unwrap();
        assert!(matches!(
            broker.execute(request).await,
            Err(PortError::Unavailable(_))
        ));
    }

    #[test]
    fn static_registration_rejects_duplicate_handler_keys() {
        let mut broker = CapabilityBroker::new();
        broker
            .register_static(CapabilityKind::Query, "search", Arc::new(NoopHandler))
            .unwrap();
        assert_eq!(
            broker
                .register_static(CapabilityKind::Query, "search", Arc::new(NoopHandler))
                .unwrap_err(),
            PortError::Conflict("capability_handler_already_registered".to_owned())
        );
    }
}
