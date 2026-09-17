//! One admitted model attempt. No Agent loop, tool execution, hidden retries or legacy runtime edges.
mod config;
mod request;
mod resolver;
mod response;
mod telemetry;
mod transport;
use async_trait::async_trait;
pub use config::ProviderConfig;
use kiana_domain::*;
use kiana_ports::{ModelBudgetPort, ModelClient};
pub use resolver::{
    redacted_workspace_value, ConfigResolver, WorkspaceConfig, WorkspaceProfile,
    MAX_WORKSPACE_CONFIG_BYTES,
};
use std::collections::BTreeMap;
pub use telemetry::{safe_prepared_metadata, MODEL_ATTEMPT_TELEMETRY_SCHEMA};

pub struct ProviderGateway {
    connections: BTreeMap<String, config::Connection>,
    explicit_profiles: bool,
    configuration: ProviderConfigSnapshot,
}
impl ProviderGateway {
    pub fn from_env(config: ProviderConfig) -> Result<Self, ModelError> {
        let resolved = resolver::ConfigResolver::resolve(config)?;
        Ok(Self {
            connections: resolved.connections,
            explicit_profiles: resolved.explicit_profiles,
            configuration: resolved.snapshot,
        })
    }
    pub fn catalog(&self) -> serde_json::Value {
        let configuration = self
            .configuration_snapshot()
            .ok()
            .and_then(|snapshot| serde_json::to_value(snapshot).ok());
        let model_catalog = self
            .model_catalog()
            .ok()
            .and_then(|catalog| serde_json::to_value(catalog).ok());
        serde_json::json!({"schema":"kiana.model-catalog.v1","connections":self.connections.values().map(|connection|serde_json::json!({"route":connection.route,"capabilities":connection.capabilities})).collect::<Vec<_>>(),"configuration":configuration,"model_catalog":model_catalog})
    }

    pub fn configuration_snapshot(&self) -> Result<ProviderConfigSnapshot, ModelError> {
        Ok(self.configuration.clone())
    }

    pub fn model_catalog(&self) -> Result<ModelCatalog, ModelError> {
        let entries = self
            .connections
            .values()
            .map(|connection| ModelCatalogEntry {
                schema: MODEL_CATALOG_ENTRY_SCHEMA.to_owned(),
                provider_id: connection.route.provider_id.clone(),
                connection_id: connection.route.connection_id.clone(),
                model_id: connection.route.model_id.clone(),
                capabilities: connection.capabilities.clone(),
                source: if connection.route.profile == "default" {
                    ModelCatalogSource::Builtin
                } else {
                    ModelCatalogSource::Configured
                },
                catalog_revision: connection.route.configuration_revision.clone(),
                expires_at_unix_ms: None,
            })
            .collect();
        ModelCatalog::new(entries).map_err(ModelError::invalid)
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
