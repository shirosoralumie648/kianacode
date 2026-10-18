//! INT-24 daemon ingress route: verified occurrence → `ControlPlane` command.
//!
//! This is the production bridge that was missing before: a signed Webhook/A2A event is verified
//! by [`ConnectorIngressVerifier`], bound to one server-owned connector account as a
//! [`kiana_domain::ConnectorIngressIntent`], and then submitted through the *existing* command
//! path with `DaemonHost::handle`.
//!
//! ```text
//! untrusted event + server now
//!     -> ConnectorIngressVerifier::verify_durable()   (structure/source/tenant/clock/HMAC/replay)
//!     -> payload digest re-check against the verified occurrence
//!     -> server-resolved binding (EventLog) + intent (account/audience/operation)
//!     -> RequestEnvelope::command("connector.invoke")
//!     -> DaemonHost::handle -> ControlPlane -> policy/gates/Broker
//! ```
//!
//! Boundaries:
//!
//! - No second execution loop: the route never calls a connector adapter, the Broker or the
//!   EventLog directly. Every effect-bearing step is the normal `ControlPlane` command path.
//! - Every denial happens before the `ControlPlane` call and reports `broker_calls = 0`.
//! - A replay store `Duplicate` outcome returns without touching `ControlPlane`, so a redelivery
//!   is idempotent even across a daemon restart.
//! - The inbound event payload is used as the connector payload only after its digest is checked
//!   against the verified occurrence; the signed bytes are the invoked bytes.

use crate::{ConnectorIngressVerifier, DaemonHost};
use kiana_domain::{
    connector_bindings, ConnectorCommand, ConnectorCommandRequest, ConnectorIngressEvent,
    ConnectorIngressIntent, ConnectorIngressOccurrence, CONNECTOR_STREAM,
};
use kiana_ports::PortError;
use kiana_protocol::{PermissionProfile, RequestEnvelope, RequestMetadata, ResponseEnvelope};
use std::path::Path;
use std::sync::Arc;

/// Stable schema for the route result. It is a local diagnostic projection, not a wire contract.
pub const CONNECTOR_INGRESS_ROUTE_SCHEMA: &str = "kiana.connector-ingress-route-result.v1";

/// Server-owned ingress route configuration.
///
/// A route pins one allowlisted source to one already-bound connector account/operation. Nothing
/// in this struct is read from the inbound event: source, project, binding and operation are the
/// daemon's own configuration, so a signed payload cannot select another account.
#[derive(Clone)]
pub struct ConnectorIngressRouteConfig {
    pub source_id: String,
    pub project_root: String,
    pub project_id: String,
    pub binding_id: String,
    pub operation: String,
    pub session_id: String,
}

impl std::fmt::Debug for ConnectorIngressRouteConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorIngressRouteConfig")
            .field("source_id", &self.source_id)
            .field("project_root", &self.project_root)
            .field("project_id", &self.project_id)
            .field("binding_id", &self.binding_id)
            .field("operation", &self.operation)
            .field("session_id", &self.session_id)
            .finish()
    }
}

impl ConnectorIngressRouteConfig {
    fn validate(&self) -> Result<(), PortError> {
        for (value, field) in [
            (&self.source_id, "connector_ingress_route_source_invalid"),
            (
                &self.project_root,
                "connector_ingress_route_project_invalid",
            ),
            (&self.project_id, "connector_ingress_route_project_invalid"),
            (&self.binding_id, "connector_ingress_route_binding_invalid"),
            (&self.operation, "connector_ingress_route_operation_invalid"),
            (&self.session_id, "connector_ingress_route_session_invalid"),
        ] {
            if value.trim().is_empty() || value.len() > 4_096 {
                return Err(PortError::Failed(field.to_owned()));
            }
        }
        Ok(())
    }
}

/// One configured route plus the verifier that owns its signing keys and replay window.
#[derive(Clone)]
pub(crate) struct ConnectorIngressRoute {
    verifier: Arc<ConnectorIngressVerifier>,
    config: ConnectorIngressRouteConfig,
}

impl std::fmt::Debug for ConnectorIngressRoute {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConnectorIngressRoute")
            .field("config", &self.config)
            .field("verifier", &self.verifier)
            .finish()
    }
}

/// Outcome of a route call that was not refused.
#[derive(Debug)]
pub struct ConnectorIngressRouteResult {
    pub schema: &'static str,
    pub source_id: String,
    pub event_id: String,
    /// True when the durable replay store recognised an idempotent redelivery. In that case the
    /// `ControlPlane` was not called and `response` is `None`.
    pub duplicate: bool,
    /// Always zero on a duplicate: a replayed delivery cannot carry a fresh effect.
    pub broker_calls: u32,
    /// Server-derived intent digest for an admitted occurrence.
    pub intent_digest: Option<String>,
    /// The exact `ControlPlane` response for an admitted occurrence.
    pub response: Option<ResponseEnvelope>,
}

impl DaemonHost {
    /// Attach the INT-24 signed ingress route.
    ///
    /// The verifier must be the durable variant
    /// ([`ConnectorIngressVerifier::new_durable`]) in production. An in-process verifier is
    /// accepted here only so embedded/test callers can exercise the same route, and its replay
    /// memory is not durable.
    pub fn with_connector_ingress_route(
        mut self,
        verifier: ConnectorIngressVerifier,
        config: ConnectorIngressRouteConfig,
    ) -> Result<Self, PortError> {
        config.validate()?;
        if verifier.ingress_audience(&config.source_id).is_none() {
            return Err(PortError::Conflict(
                "connector_ingress_source_not_allowlisted".to_owned(),
            ));
        }
        self.connector_ingress_route = Some(ConnectorIngressRoute {
            verifier: Arc::new(verifier),
            config,
        });
        Ok(self)
    }

    /// Whether a signed ingress route is configured.
    pub fn has_connector_ingress_route(&self) -> bool {
        self.connector_ingress_route.is_some()
    }

    /// Verify one untrusted ingress event and, if admitted, submit it to `ControlPlane`.
    ///
    /// `now_unix_ms` is the server clock at admission time; it is never derived from the event.
    /// Deny-first order: route availability, structure/source/tenant/clock/HMAC, durable replay,
    /// source route binding, payload digest, connector binding resolution, intent derivation,
    /// project trust, then the normal command path. Every `Err` is a stable refusal code and
    /// happened before `ControlPlane` was entered, so no capability or connector was reached.
    pub async fn handle_connector_ingress(
        &self,
        event: ConnectorIngressEvent,
        now_unix_ms: u64,
    ) -> Result<ConnectorIngressRouteResult, PortError> {
        let Some(route) = self.connector_ingress_route.as_ref() else {
            return Err(PortError::Unavailable(
                "connector_ingress_route_unavailable".to_owned(),
            ));
        };
        let source_id = event.source_id.clone();
        let event_id = event.event_id.clone();
        if source_id != route.config.source_id {
            return Err(PortError::Conflict(
                "connector_ingress_source_route_mismatch".to_owned(),
            ));
        }
        // Keep the signed payload for the invocation; it stays untrusted until the occurrence
        // digest below proves it is the same value that was authenticated.
        let payload = event.payload.clone();
        let occurrence = route.verifier.verify_durable(event, now_unix_ms).await?;
        let Some(occurrence) = occurrence else {
            return Ok(ConnectorIngressRouteResult {
                schema: CONNECTOR_INGRESS_ROUTE_SCHEMA,
                source_id,
                event_id,
                duplicate: true,
                broker_calls: 0,
                intent_digest: None,
                response: None,
            });
        };
        if kiana_domain::json_digest(&payload) != occurrence.payload_digest {
            return Err(PortError::Conflict(
                "connector_ingress_payload_digest_mismatch".to_owned(),
            ));
        }
        // Project trust is re-checked here so an untrusted project is refused before the route
        // builds or submits any command. The daemon still re-checks it during `handle`.
        let trusted = self
            .project_authority
            .project_trusted(Path::new(&route.config.project_root))
            .map_err(|_| PortError::Failed("project_trust_unavailable".to_owned()))?;
        if !trusted {
            return Err(PortError::Conflict("project_untrusted".to_owned()));
        }
        let binding = self.resolve_ingress_binding(&route.config).await?;
        let audience = route
            .verifier
            .ingress_audience(&route.config.source_id)
            .ok_or_else(|| {
                PortError::Conflict("connector_ingress_source_not_allowlisted".to_owned())
            })?
            .to_owned();
        let idempotency_key = ingress_idempotency_key(&occurrence);
        let intent = ConnectorIngressIntent::from_verified_occurrence(
            &occurrence,
            &binding,
            &route.config.project_id,
            &audience,
            &audience,
            &route.config.operation,
            &idempotency_key,
        )
        .map_err(connector_ingress_refusal)?;
        intent.validate().map_err(connector_ingress_refusal)?;

        let mut command = ConnectorCommandRequest::new(ConnectorCommand::Invoke);
        command.binding_id = Some(intent.binding_id.clone());
        command.operation = Some(intent.operation.clone());
        command.payload = Some(payload);
        command.idempotency_key = Some(idempotency_key);
        let arguments = command.to_arguments().map_err(connector_ingress_refusal)?;
        let mut metadata = RequestMetadata::local(
            route.config.session_id.clone(),
            route.config.project_root.clone(),
        );
        metadata.project_trusted = true;
        metadata.permission_profile = PermissionProfile::Balanced;
        metadata.entrypoint = Some(kiana_protocol::EntryPointKind::Connector);
        let envelope =
            RequestEnvelope::command(metadata, ConnectorCommand::Invoke.wire_name(), arguments);
        // Keep the large `DaemonHost::handle` future off this async frame. On Windows the debug
        // stack is 1 MiB and the composed ControlPlane path does not fit; boxing the future keeps
        // the route's own frame small without changing the single-spine call.
        let response = Box::pin(self.handle(envelope)).await;
        Ok(ConnectorIngressRouteResult {
            schema: CONNECTOR_INGRESS_ROUTE_SCHEMA,
            source_id,
            event_id,
            duplicate: false,
            broker_calls: 0,
            intent_digest: Some(intent.intent_digest.clone()),
            response: Some(response),
        })
    }

    async fn resolve_ingress_binding(
        &self,
        config: &ConnectorIngressRouteConfig,
    ) -> Result<kiana_domain::ConnectorBindingSnapshot, PortError> {
        let events = self
            .persisted_events()
            .await?
            .ok_or_else(|| PortError::Unavailable("connector_registry_unavailable".to_owned()))?;
        let history: Vec<kiana_domain::RuntimeEvent> = events
            .into_iter()
            .filter(|event| {
                event.aggregate_type.as_deref() == Some(CONNECTOR_STREAM)
                    && event.aggregate_id.as_deref() == Some(config.project_root.as_str())
            })
            .collect();
        let (_, bindings) =
            connector_bindings(&history).map_err(|reason| PortError::Failed(reason.to_owned()))?;
        let binding = bindings
            .get(&config.binding_id)
            .cloned()
            .ok_or_else(|| PortError::Conflict("connector_binding_missing".to_owned()))?;
        if binding.project_root != config.project_root {
            return Err(PortError::Conflict(
                "connector_binding_scope_mismatch".to_owned(),
            ));
        }
        if binding.status != "active" {
            return Err(PortError::Conflict("connector_binding_revoked".to_owned()));
        }
        if binding.operation(&config.operation).is_err() {
            return Err(PortError::Conflict(
                "connector_operation_unregistered".to_owned(),
            ));
        }
        Ok(binding)
    }
}

/// Stable idempotency key derived from the verified occurrence identity. A redelivery of the same
/// signed event produces the same key, so the connector route replays rather than re-dispatches.
fn ingress_idempotency_key(occurrence: &ConnectorIngressOccurrence) -> String {
    format!(
        "connector-ingress:{}:{}",
        occurrence.source_id, occurrence.event_id
    )
}

fn connector_ingress_refusal(reason: String) -> PortError {
    PortError::Conflict(reason)
}
