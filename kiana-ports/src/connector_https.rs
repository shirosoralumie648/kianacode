//! HTTPS connector transport port.
//!
//! The port is below the ControlPlane and receives only a validated connector permit, opaque
//! credential lease, canonical payload and the domain-owned pinned-origin/DNS observation.  It
//! has no EventStore, approval or filesystem access and its default implementation is unsupported.

use crate::{CanonicalConnectorPayload, ConnectorPreparedPermit, PortError};
use async_trait::async_trait;
use kiana_domain::{
    ConnectorBindingSnapshot, ConnectorHttpsPolicy, ConnectorHttpsResolution, CredentialLease,
    ProviderReceipt,
};
use serde::{Deserialize, Serialize};

pub const CONNECTOR_HTTPS_PORT_SCHEMA: &str = "kiana.connector-https-port.v1";
pub const CONNECTOR_HTTPS_REQUEST_SCHEMA: &str = "kiana.connector-https-request.v1";

/// Capabilities describe transport guarantees; they do not grant an endpoint or account scope.
/// A concrete adapter must explicitly claim TLS verification, DNS pinning and origin pinning
/// before the checked wrapper will call it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsTransportCapabilities {
    pub tls_verified: bool,
    pub dns_pinned: bool,
    pub origin_pinned: bool,
    pub explicit_proxy: bool,
}

impl Default for ConnectorHttpsTransportCapabilities {
    fn default() -> Self {
        Self {
            tls_verified: false,
            dns_pinned: false,
            origin_pinned: false,
            explicit_proxy: false,
        }
    }
}

impl ConnectorHttpsTransportCapabilities {
    pub fn validate(&self) -> Result<(), PortError> {
        if !self.tls_verified || !self.dns_pinned || !self.origin_pinned {
            return Err(PortError::Unavailable(
                "connector_https_transport_boundary_unsupported".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Effect-time request assembled after the ordinary connector admission path.  `proxy_origin`
/// is explicit and optional; an ambient proxy is never represented and therefore cannot be
/// silently selected by an implementation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsRequest {
    pub schema: String,
    pub permit: ConnectorPreparedPermit,
    pub binding: ConnectorBindingSnapshot,
    pub lease: CredentialLease,
    pub payload: CanonicalConnectorPayload,
    pub policy: ConnectorHttpsPolicy,
    pub endpoint: kiana_domain::ConnectorHttpsEndpoint,
    pub resolution: ConnectorHttpsResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_origin: Option<String>,
    #[serde(default)]
    pub redirects: Vec<kiana_domain::ConnectorHttpsEndpoint>,
}

impl ConnectorHttpsRequest {
    pub fn validate(&self) -> Result<(), PortError> {
        if self.schema != CONNECTOR_HTTPS_REQUEST_SCHEMA {
            return Err(PortError::Failed(
                "connector_https_request_schema_invalid".to_owned(),
            ));
        }
        self.policy
            .validate()
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        self.permit
            .validate_for_binding(&self.binding, &self.payload, &self.lease)?;
        self.policy
            .validate_endpoint(&self.endpoint)
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        self.policy
            .validate_resolution(&self.endpoint, &self.resolution)
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        self.policy
            .validate_proxy(self.proxy_origin.as_deref())
            .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
        if self.redirects.len() > self.policy.max_redirects as usize {
            return Err(PortError::Conflict(
                "connector_https_redirect_limit_exceeded".to_owned(),
            ));
        }
        let mut previous = self.endpoint.clone();
        for redirect in &self.redirects {
            self.policy
                .validate_endpoint(redirect)
                .map_err(|error| PortError::Conflict(error.code().to_owned()))?;
            if redirect.origin != previous.origin || redirect.origin != self.policy.pinned_origin {
                return Err(PortError::Conflict(
                    "connector_https_redirect_origin_denied".to_owned(),
                ));
            }
            previous = redirect.clone();
        }
        Ok(())
    }
}

/// A concrete HTTPS adapter may implement this port, but it cannot add an authorization path or
/// dispatch before `send_checked` has revalidated the permit, lease, endpoint, DNS observation,
/// proxy allowlist and redirect chain.
#[async_trait]
pub trait ConnectorHttpsTransport: Send + Sync {
    fn capabilities(&self) -> ConnectorHttpsTransportCapabilities {
        ConnectorHttpsTransportCapabilities::default()
    }

    async fn send(&self, _request: ConnectorHttpsRequest) -> Result<ProviderReceipt, PortError> {
        Err(PortError::Unavailable(
            "connector_https_transport_unsupported".to_owned(),
        ))
    }

    async fn send_checked(
        &self,
        request: ConnectorHttpsRequest,
    ) -> Result<ProviderReceipt, PortError> {
        let capabilities = self.capabilities();
        capabilities.validate()?;
        request.validate()?;
        if request.proxy_origin.is_some() && !capabilities.explicit_proxy {
            return Err(PortError::Unavailable(
                "connector_https_proxy_transport_unsupported".to_owned(),
            ));
        }
        let binding = request.binding.clone();
        let permit = request.permit.clone();
        let receipt = self.send(request).await?;
        crate::connector::validate_receipt_for_permit(&receipt, &binding, &permit)
            .map_err(|error| PortError::Conflict(error.to_string()))?;
        Ok(receipt)
    }
}
