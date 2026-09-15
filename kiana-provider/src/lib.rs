//! One admitted model attempt. No Agent loop, tool execution, hidden retries or legacy runtime edges.
mod config;
mod request;
mod response;
mod telemetry;
mod transport;
use async_trait::async_trait;
pub use config::ProviderConfig;
use kiana_domain::*;
use kiana_ports::{ModelBudgetPort, ModelClient};
use std::collections::BTreeMap;
pub use telemetry::{safe_prepared_metadata, MODEL_ATTEMPT_TELEMETRY_SCHEMA};

pub struct ProviderGateway {
    connections: BTreeMap<String, config::Connection>,
    explicit_profiles: bool,
}
impl ProviderGateway {
    pub fn from_env(config: ProviderConfig) -> Result<Self, ModelError> {
        let (connections, explicit_profiles) = config::connections(config)?;
        Ok(Self {
            connections,
            explicit_profiles,
        })
    }
    pub fn catalog(&self) -> serde_json::Value {
        serde_json::json!({"schema":"kiana.model-catalog.v1","connections":self.connections.values().map(|connection|serde_json::json!({"route":connection.route,"capabilities":connection.capabilities})).collect::<Vec<_>>()})
    }
    fn connection(&self, spec: &ModelCallSpec) -> Result<&config::Connection, ModelError> {
        let assignment = spec
            .assignment
            .as_ref()
            .ok_or_else(|| ModelError::invalid("model_server_assignment_required"))?;
        assignment.validate()?;
        if let Some(connection) = self.connections.get(&assignment.profile) {
            return Ok(connection);
        }
        if self.explicit_profiles {
            return Err(ModelError::invalid("model_profile_unconfigured"));
        }
        self.connections
            .get("default")
            .ok_or_else(|| ModelError::invalid("model_default_route_unavailable"))
    }
}
#[async_trait]
impl ModelClient for ProviderGateway {
    fn prepare_call(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
    ) -> Result<PreparedModelCall, ModelError> {
        request::compile(self.connection(&spec)?, request, spec)
    }
    fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
        ModelRequestContext::for_request(request)
    }
    async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
        Err("provider_requires_model_admission".to_owned())
    }
    async fn complete_prepared(
        &self,
        _prepared: PreparedModelCall,
        _sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        Err(ModelError::invalid("provider_requires_model_admission"))
    }
    async fn complete_admitted(
        &self,
        prepared: PreparedModelCall,
        permit: ModelCallPermit,
        admission: &dyn ModelBudgetPort,
        sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        prepared.validate()?;
        // Validate the allow-listed instrumentation view at the provider boundary.  The value is
        // deliberately not built from wire_body or response content; the runner persists the
        // same safe fields through its committed model-turn event.
        let _telemetry = telemetry::safe_prepared_metadata(&prepared)?;
        let connection = self.connection(&prepared.spec)?;
        if connection.route.configuration_revision != prepared.route.configuration_revision
            || connection.route.connection_id != prepared.route.connection_id
        {
            return Err(ModelError::invalid("model_route_changed_after_admission"));
        }
        admission
            .consume_prepared(&prepared, &permit)
            .await
            .map_err(|e| ModelError::invalid(e.to_string()))?;
        transport::send(connection, prepared, sink).await
    }
}
