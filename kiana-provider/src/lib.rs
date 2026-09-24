//! One admitted model attempt. No Agent loop, tool execution, hidden retries or legacy runtime edges.
mod capacity;
mod connector_quota;
mod config;
mod fallback;
mod credentials;
mod oauth;
mod request;
mod resolver;
mod response;
mod telemetry;
mod transport;
mod usage;
mod usage_adapters;
use async_trait::async_trait;
pub use config::ProviderConfig;
pub use connector_quota::{
    validate_connector_quota_lease, CONNECTOR_QUOTA_PROVIDER_BOUNDARY_SCHEMA,
};
pub use fallback::{validate_fallback_attempt, FALLBACK_PROVIDER_BOUNDARY_SCHEMA};
use kiana_domain::*;
use kiana_ports::{ModelBudgetPort, ModelClient};
pub use resolver::{
    redacted_workspace_value, ConfigResolver, WorkspaceConfig, WorkspaceProfile,
    MAX_WORKSPACE_CONFIG_BYTES,
};
use std::collections::BTreeMap;
pub use telemetry::{safe_prepared_metadata, MODEL_ATTEMPT_TELEMETRY_SCHEMA};
pub use usage::normalize_model_reply;
pub use usage_adapters::{
    normalize_provider_usage, MAX_PROVIDER_USAGE_TOKENS, PROVIDER_USAGE_ADAPTER_SCHEMA,
};

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

    /// Return only the secret-free identity that a per-connection live evidence record may bind.
    /// This does not execute a request or turn metadata into provider proof.
    pub fn live_connection_metadata(
        &self,
        profile: &str,
    ) -> Result<ProviderLiveConnectionMetadata, ModelError> {
        let connection = self
            .connections
            .get(profile)
            .ok_or_else(|| ModelError::invalid("model_profile_unconfigured"))?;
        ProviderLiveConnectionMetadata::new(
            connection.route.clone(),
            connection.capabilities.clone(),
            self.configuration.snapshot_digest.clone(),
            connection.credential_revision.clone(),
            connection.provider_account.clone(),
        )
        .map_err(ModelError::invalid)
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

    /// Prepare a request with explicitly admitted image artifacts.  The bytes are supplied by the
    /// caller's Artifact/data-governance adapter; this method never resolves a path or fetches a
    /// URL.  A normal `prepare_call` remains deny-first for AttachmentRef values without this
    /// admission list.
    pub fn prepare_call_with_images(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
        images: Vec<ImageInputAdmission>,
    ) -> Result<PreparedModelCall, ModelError> {
        request::compile_with_images(self.connection(&spec)?, request, spec, &images)
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
        let admission_now = transport::unix_ms()?;
        permit.validate_for_prepared(&prepared, admission_now)?;
        if let Some(secret_ref) = &connection.credential_ref {
            let current_revision = connection.credential_store.current_revision(secret_ref)?;
            if current_revision != connection.credential_revision {
                return Err(ModelError::invalid("model_credential_revision_changed"));
            }
        } else if connection.credential_revision != "none" {
            return Err(ModelError::invalid("model_credential_revision_missing"));
        }
        admission
            .consume_prepared(&prepared, &permit)
            .await
            .map_err(|e| ModelError::invalid(e.to_string()))?;
        transport::send(connection, prepared, sink).await
    }
}
