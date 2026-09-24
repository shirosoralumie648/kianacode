//! Connector HTTPS endpoint and egress contracts.
//!
//! This module is intentionally pure.  It parses and validates an operator-owned endpoint,
//! verifies an adapter-supplied DNS observation and keeps redirects/proxy selection bound to one
//! pinned origin.  It never performs DNS, opens a socket or reads proxy environment variables.
//! The transport port below the ControlPlane consumes these observations only after the ordinary
//! connector permit and credential lease have been admitted.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

pub const CONNECTOR_HTTPS_POLICY_SCHEMA: &str = "kiana.connector-https-policy.v1";
pub const CONNECTOR_HTTPS_ENDPOINT_SCHEMA: &str = "kiana.connector-https-endpoint.v1";
pub const CONNECTOR_HTTPS_RESOLUTION_SCHEMA: &str = "kiana.connector-https-resolution.v1";
pub const CONNECTOR_HTTPS_POLICY_VERSION: u64 = 1;
pub const CONNECTOR_HTTPS_MAX_HOSTS: usize = 64;
pub const CONNECTOR_HTTPS_MAX_ADDRESSES: usize = 32;
pub const CONNECTOR_HTTPS_MAX_REDIRECTS: u8 = 8;

/// Stable, redaction-safe error classification for connector endpoint admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorHttpsErrorCode {
    PolicyInvalid,
    EndpointInvalid,
    UrlUserinfoDenied,
    NonHttpsDenied,
    EndpointQueryDenied,
    EndpointFragmentDenied,
    OriginMismatch,
    HostNotAllowlisted,
    ResolutionRequired,
    ResolutionInvalid,
    LocalAddressDenied,
    MetadataAddressDenied,
    EgressAddressNotAllowlisted,
    ResolutionHostMismatch,
    RedirectOriginDenied,
    RedirectLimitExceeded,
    ProxyDenied,
    ProxyOriginInvalid,
}

impl ConnectorHttpsErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PolicyInvalid => "connector_https_policy_invalid",
            Self::EndpointInvalid => "connector_https_endpoint_invalid",
            Self::UrlUserinfoDenied => "connector_https_url_userinfo_denied",
            Self::NonHttpsDenied => "connector_https_non_https_denied",
            Self::EndpointQueryDenied => "connector_https_query_denied",
            Self::EndpointFragmentDenied => "connector_https_fragment_denied",
            Self::OriginMismatch => "connector_https_origin_mismatch",
            Self::HostNotAllowlisted => "connector_https_host_not_allowlisted",
            Self::ResolutionRequired => "connector_https_resolution_required",
            Self::ResolutionInvalid => "connector_https_resolution_invalid",
            Self::LocalAddressDenied => "connector_https_local_address_denied",
            Self::MetadataAddressDenied => "connector_https_metadata_address_denied",
            Self::EgressAddressNotAllowlisted => "connector_https_egress_address_not_allowlisted",
            Self::ResolutionHostMismatch => "connector_https_resolution_host_mismatch",
            Self::RedirectOriginDenied => "connector_https_redirect_origin_denied",
            Self::RedirectLimitExceeded => "connector_https_redirect_limit_exceeded",
            Self::ProxyDenied => "connector_https_proxy_denied",
            Self::ProxyOriginInvalid => "connector_https_proxy_origin_invalid",
        }
    }
}

/// Public error contains only a stable code.  URL text, proxy text and resolver output are never
/// copied into this object or an EventLog/Receipt projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsError {
    pub code: ConnectorHttpsErrorCode,
}

impl ConnectorHttpsError {
    pub const fn new(code: ConnectorHttpsErrorCode) -> Self {
        Self { code }
    }

    pub const fn code(self) -> &'static str {
        self.code.as_str()
    }
}

impl std::fmt::Display for ConnectorHttpsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for ConnectorHttpsError {}

/// Server-owned HTTPS egress policy.  `allowed_egress_hosts` is an exact host allowlist; suffix
/// or wildcard matching is deliberately not supported.  Resolved addresses may be pinned too,
/// which lets a deployment reject a DNS change before the socket is opened.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsPolicy {
    pub schema: String,
    pub version: u64,
    pub pinned_origin: String,
    pub allowed_egress_hosts: BTreeSet<String>,
    #[serde(default)]
    pub allowed_egress_addresses: BTreeSet<String>,
    #[serde(default)]
    pub allowed_proxy_origins: BTreeSet<String>,
    pub max_redirects: u8,
    pub policy_digest: String,
}

impl ConnectorHttpsPolicy {
    /// Construct a strict policy.  The pinned origin host is automatically included in the exact
    /// host allowlist.  Proxy and resolved-address allowlists remain empty, which means no proxy
    /// and any *public* address for the pinned host respectively.
    pub fn new(
        pinned_origin: impl AsRef<str>,
        allowed_egress_hosts: impl IntoIterator<Item = String>,
    ) -> Result<Self, ConnectorHttpsError> {
        Self::from_parts(
            pinned_origin.as_ref(),
            allowed_egress_hosts,
            std::iter::empty::<String>(),
            std::iter::empty::<String>(),
            0,
        )
    }

    pub fn from_parts(
        pinned_origin: &str,
        allowed_egress_hosts: impl IntoIterator<Item = String>,
        allowed_egress_addresses: impl IntoIterator<Item = String>,
        allowed_proxy_origins: impl IntoIterator<Item = String>,
        max_redirects: u8,
    ) -> Result<Self, ConnectorHttpsError> {
        let origin = canonical_origin(pinned_origin, ConnectorHttpsErrorCode::PolicyInvalid)?;
        let origin_host = parse_origin_host(&origin)
            .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::PolicyInvalid))?;
        let allowed_egress_hosts = normalize_hosts(allowed_egress_hosts)?;
        if allowed_egress_hosts.is_empty() || !allowed_egress_hosts.contains(&origin_host) {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::HostNotAllowlisted,
            ));
        }
        let allowed_egress_addresses = normalize_public_addresses(allowed_egress_addresses, false)?;
        let allowed_proxy_origins = normalize_proxy_origins(allowed_proxy_origins)?;
        if max_redirects > CONNECTOR_HTTPS_MAX_REDIRECTS {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::PolicyInvalid,
            ));
        }
        let mut policy = Self {
            schema: CONNECTOR_HTTPS_POLICY_SCHEMA.to_owned(),
            version: CONNECTOR_HTTPS_POLICY_VERSION,
            pinned_origin: origin,
            allowed_egress_hosts,
            allowed_egress_addresses,
            allowed_proxy_origins,
            max_redirects,
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), ConnectorHttpsError> {
        if self.schema != CONNECTOR_HTTPS_POLICY_SCHEMA
            || self.version != CONNECTOR_HTTPS_POLICY_VERSION
            || self.allowed_egress_hosts.is_empty()
            || self.allowed_egress_hosts.len() > CONNECTOR_HTTPS_MAX_HOSTS
            || self
                .allowed_egress_hosts
                .iter()
                .zip(self.allowed_egress_hosts.iter().skip(1))
                .any(|(left, right)| left >= right)
            || self
                .allowed_egress_hosts
                .iter()
                .any(|host| normalize_host(host).is_err())
            || self.allowed_egress_addresses.len() > CONNECTOR_HTTPS_MAX_ADDRESSES
            || self.allowed_egress_addresses.iter().any(|address| {
                address
                    .parse::<IpAddr>()
                    .map(|parsed| address != &parsed.to_string() || is_forbidden_address(parsed))
                    .unwrap_or(true)
            })
            || self.allowed_proxy_origins.len() > CONNECTOR_HTTPS_MAX_HOSTS
            || self.allowed_proxy_origins.iter().any(|origin| {
                canonical_proxy_origin(origin)
                    .map(|canonical| canonical != origin.as_str())
                    .unwrap_or(true)
            })
            || self.max_redirects > CONNECTOR_HTTPS_MAX_REDIRECTS
            || self.policy_digest != self.digest()
        {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::PolicyInvalid,
            ));
        }
        let origin_host = parse_origin_host(&self.pinned_origin)
            .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::PolicyInvalid))?;
        if !self.allowed_egress_hosts.contains(&origin_host) {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::HostNotAllowlisted,
            ));
        }
        // Reparse the origin with strict HTTPS and no path/query/fragment/userinfo constraints.
        let url = Url::parse(&self.pinned_origin)
            .map_err(|_| ConnectorHttpsError::new(ConnectorHttpsErrorCode::PolicyInvalid))?;
        validate_https_url(&url, None)?;
        if url.path() != "/" && !url.path().is_empty() {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::PolicyInvalid,
            ));
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "pinned_origin": self.pinned_origin,
            "allowed_egress_hosts": self.allowed_egress_hosts,
            "allowed_egress_addresses": self.allowed_egress_addresses,
            "allowed_proxy_origins": self.allowed_proxy_origins,
            "max_redirects": self.max_redirects,
        }))
    }

    pub fn endpoint(&self, raw: &str) -> Result<ConnectorHttpsEndpoint, ConnectorHttpsError> {
        self.validate()?;
        let url = Url::parse(raw)
            .map_err(|_| ConnectorHttpsError::new(ConnectorHttpsErrorCode::EndpointInvalid))?;
        validate_https_url(&url, Some(self))?;
        ConnectorHttpsEndpoint::from_url(&url)
    }

    pub fn redirect(
        &self,
        previous: &ConnectorHttpsEndpoint,
        next: &str,
    ) -> Result<ConnectorHttpsEndpoint, ConnectorHttpsError> {
        self.validate_endpoint(previous)?;
        let endpoint = self.endpoint(next).map_err(|error| match error.code {
            ConnectorHttpsErrorCode::OriginMismatch => {
                ConnectorHttpsError::new(ConnectorHttpsErrorCode::RedirectOriginDenied)
            }
            _ => error,
        })?;
        if endpoint.origin != previous.origin || endpoint.origin != self.pinned_origin {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::RedirectOriginDenied,
            ));
        }
        Ok(endpoint)
    }

    pub fn validate_endpoint(
        &self,
        endpoint: &ConnectorHttpsEndpoint,
    ) -> Result<(), ConnectorHttpsError> {
        self.validate()?;
        if endpoint.schema != CONNECTOR_HTTPS_ENDPOINT_SCHEMA
            || endpoint.origin != self.pinned_origin
            || !self.allowed_egress_hosts.contains(&endpoint.host)
            || endpoint.endpoint_digest != endpoint.digest()
        {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::OriginMismatch,
            ));
        }
        Ok(())
    }

    pub fn observe_resolution(
        &self,
        endpoint: &ConnectorHttpsEndpoint,
        resolved_addresses: &[String],
    ) -> Result<ConnectorHttpsResolution, ConnectorHttpsError> {
        self.validate_endpoint(endpoint)?;
        if resolved_addresses.is_empty() || resolved_addresses.len() > CONNECTOR_HTTPS_MAX_ADDRESSES
        {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::ResolutionRequired,
            ));
        }
        let mut addresses = Vec::with_capacity(resolved_addresses.len());
        for raw in resolved_addresses {
            let address = raw.parse::<IpAddr>().map_err(|_| {
                ConnectorHttpsError::new(ConnectorHttpsErrorCode::ResolutionInvalid)
            })?;
            if address == IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)) {
                return Err(ConnectorHttpsError::new(
                    ConnectorHttpsErrorCode::MetadataAddressDenied,
                ));
            }
            if is_forbidden_address(address) {
                return Err(ConnectorHttpsError::new(
                    ConnectorHttpsErrorCode::LocalAddressDenied,
                ));
            }
            if !self.allowed_egress_addresses.is_empty()
                && !self.allowed_egress_addresses.contains(&address.to_string())
            {
                return Err(ConnectorHttpsError::new(
                    ConnectorHttpsErrorCode::EgressAddressNotAllowlisted,
                ));
            }
            addresses.push(address);
        }
        addresses.sort_by_key(ToString::to_string);
        addresses.dedup();
        if let Some(expected) = endpoint.literal_ip() {
            if addresses != vec![expected] {
                return Err(ConnectorHttpsError::new(
                    ConnectorHttpsErrorCode::ResolutionHostMismatch,
                ));
            }
        }
        let address_text = addresses
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let mut observation = ConnectorHttpsResolution {
            schema: CONNECTOR_HTTPS_RESOLUTION_SCHEMA.to_owned(),
            version: CONNECTOR_HTTPS_POLICY_VERSION,
            endpoint_digest: endpoint.endpoint_digest.clone(),
            resolved_addresses: address_text,
            resolution_digest: String::new(),
            policy_digest: self.policy_digest.clone(),
        };
        observation.resolution_digest = observation.digest();
        observation.validate(self, endpoint)?;
        Ok(observation)
    }

    pub fn validate_resolution(
        &self,
        endpoint: &ConnectorHttpsEndpoint,
        observation: &ConnectorHttpsResolution,
    ) -> Result<(), ConnectorHttpsError> {
        observation.validate(self, endpoint)
    }

    pub fn validate_proxy(&self, proxy_origin: Option<&str>) -> Result<(), ConnectorHttpsError> {
        let Some(proxy_origin) = proxy_origin else {
            return Ok(());
        };
        let canonical = canonical_proxy_origin(proxy_origin)?;
        if !self.allowed_proxy_origins.contains(&canonical) {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::ProxyDenied,
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsEndpoint {
    pub schema: String,
    pub origin: String,
    pub host: String,
    pub port: u16,
    pub path: String,
    pub endpoint_digest: String,
}

impl ConnectorHttpsEndpoint {
    fn from_url(url: &Url) -> Result<Self, ConnectorHttpsError> {
        let host = url
            .host()
            .map(host_text)
            .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::EndpointInvalid))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::EndpointInvalid))?;
        let origin = origin_for_url(url)?;
        let path = if url.path().is_empty() {
            "/".to_owned()
        } else {
            url.path().to_owned()
        };
        let mut endpoint = Self {
            schema: CONNECTOR_HTTPS_ENDPOINT_SCHEMA.to_owned(),
            origin,
            host,
            port,
            path,
            endpoint_digest: String::new(),
        };
        endpoint.endpoint_digest = endpoint.digest();
        Ok(endpoint)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "origin": self.origin,
            "host": self.host,
            "port": self.port,
            "path": self.path,
        }))
    }

    pub fn literal_ip(&self) -> Option<IpAddr> {
        self.host.parse().ok()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHttpsResolution {
    pub schema: String,
    pub version: u64,
    pub endpoint_digest: String,
    pub resolved_addresses: Vec<String>,
    pub resolution_digest: String,
    pub policy_digest: String,
}

impl ConnectorHttpsResolution {
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "endpoint_digest": self.endpoint_digest,
            "resolved_addresses": self.resolved_addresses,
            "policy_digest": self.policy_digest,
        }))
    }

    pub fn validate(
        &self,
        policy: &ConnectorHttpsPolicy,
        endpoint: &ConnectorHttpsEndpoint,
    ) -> Result<(), ConnectorHttpsError> {
        policy.validate_endpoint(endpoint)?;
        if self.schema != CONNECTOR_HTTPS_RESOLUTION_SCHEMA
            || self.version != CONNECTOR_HTTPS_POLICY_VERSION
            || self.endpoint_digest != endpoint.endpoint_digest
            || self.policy_digest != policy.policy_digest
            || self.resolved_addresses.is_empty()
            || self.resolved_addresses.len() > CONNECTOR_HTTPS_MAX_ADDRESSES
            || self
                .resolved_addresses
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self.resolved_addresses.iter().any(|raw| {
                raw.parse::<IpAddr>()
                    .map(|address| {
                        !is_forbidden_address(address)
                            && (policy.allowed_egress_addresses.is_empty()
                                || policy.allowed_egress_addresses.contains(raw))
                    })
                    .unwrap_or(false)
                    == false
            })
            || self.resolution_digest != self.digest()
        {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::ResolutionInvalid,
            ));
        }
        if let Some(expected) = endpoint.literal_ip() {
            if self.resolved_addresses.len() != 1
                || self.resolved_addresses[0] != expected.to_string()
            {
                return Err(ConnectorHttpsError::new(
                    ConnectorHttpsErrorCode::ResolutionHostMismatch,
                ));
            }
        }
        Ok(())
    }
}

fn validate_https_url(
    url: &Url,
    policy: Option<&ConnectorHttpsPolicy>,
) -> Result<(), ConnectorHttpsError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::UrlUserinfoDenied,
        ));
    }
    if url.scheme() != "https" {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::NonHttpsDenied,
        ));
    }
    if url.query().is_some() {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::EndpointQueryDenied,
        ));
    }
    if url.fragment().is_some() {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::EndpointFragmentDenied,
        ));
    }
    let host = url
        .host()
        .map(host_text)
        .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::EndpointInvalid))?;
    if let Some(policy) = policy {
        let origin = origin_for_url(url)?;
        if origin != policy.pinned_origin {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::OriginMismatch,
            ));
        }
        if !policy.allowed_egress_hosts.contains(&host) {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::HostNotAllowlisted,
            ));
        }
    }
    Ok(())
}

fn canonical_origin(
    raw: &str,
    code: ConnectorHttpsErrorCode,
) -> Result<String, ConnectorHttpsError> {
    let url = Url::parse(raw).map_err(|_| ConnectorHttpsError::new(code))?;
    validate_https_url(&url, None).map_err(|_| ConnectorHttpsError::new(code))?;
    if url.path() != "/" && !url.path().is_empty() {
        return Err(ConnectorHttpsError::new(code));
    }
    origin_for_url(&url).map_err(|_| ConnectorHttpsError::new(code))
}

fn canonical_proxy_origin(raw: &str) -> Result<String, ConnectorHttpsError> {
    let url = Url::parse(raw)
        .map_err(|_| ConnectorHttpsError::new(ConnectorHttpsErrorCode::ProxyOriginInvalid))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::ProxyOriginInvalid,
        ));
    }
    origin_for_url(&url)
        .map_err(|_| ConnectorHttpsError::new(ConnectorHttpsErrorCode::ProxyOriginInvalid))
}

fn origin_for_url(url: &Url) -> Result<String, ConnectorHttpsError> {
    let host = url
        .host()
        .map(host_text)
        .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::EndpointInvalid))?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host
    };
    let port = url
        .port_or_known_default()
        .ok_or_else(|| ConnectorHttpsError::new(ConnectorHttpsErrorCode::EndpointInvalid))?;
    Ok(format!("https://{host}:{port}"))
}

fn parse_origin_host(origin: &str) -> Option<String> {
    Url::parse(origin)
        .ok()
        .and_then(|url| url.host().map(host_text))
}

fn normalize_hosts(
    values: impl IntoIterator<Item = String>,
) -> Result<BTreeSet<String>, ConnectorHttpsError> {
    let mut hosts = BTreeSet::new();
    for value in values {
        hosts.insert(normalize_host(&value)?);
    }
    if hosts.len() > CONNECTOR_HTTPS_MAX_HOSTS {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::PolicyInvalid,
        ));
    }
    Ok(hosts)
}

fn normalize_host(raw: &str) -> Result<String, ConnectorHttpsError> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || host.contains(['\0', '/', '@', '?', '#'])
        || host
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':')))
    {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::PolicyInvalid,
        ));
    }
    Ok(host)
}

fn normalize_public_addresses(
    values: impl IntoIterator<Item = String>,
    require_nonempty: bool,
) -> Result<BTreeSet<String>, ConnectorHttpsError> {
    let mut addresses = BTreeSet::new();
    for raw in values {
        let address = raw
            .parse::<IpAddr>()
            .map_err(|_| ConnectorHttpsError::new(ConnectorHttpsErrorCode::PolicyInvalid))?;
        if address == IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)) {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::MetadataAddressDenied,
            ));
        }
        if is_forbidden_address(address) {
            return Err(ConnectorHttpsError::new(
                ConnectorHttpsErrorCode::LocalAddressDenied,
            ));
        }
        addresses.insert(address.to_string());
    }
    if require_nonempty && addresses.is_empty() {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::ResolutionRequired,
        ));
    }
    if addresses.len() > CONNECTOR_HTTPS_MAX_ADDRESSES {
        return Err(ConnectorHttpsError::new(
            ConnectorHttpsErrorCode::PolicyInvalid,
        ));
    }
    Ok(addresses)
}

fn normalize_proxy_origins(
    values: impl IntoIterator<Item = String>,
) -> Result<BTreeSet<String>, ConnectorHttpsError> {
    let mut origins = BTreeSet::new();
    for value in values {
        origins.insert(canonical_proxy_origin(&value)?);
    }
    Ok(origins)
}

fn host_text(host: Host<&str>) -> String {
    match host {
        Host::Domain(value) => value.trim_end_matches('.').to_ascii_lowercase(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => value.to_string(),
    }
}

fn is_forbidden_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(value) => {
            value.is_unspecified()
                || value.is_broadcast()
                || value.is_loopback()
                || value.is_private()
                || value.is_link_local()
                || value.is_multicast()
                || value.is_documentation()
                || is_reserved_v4(value)
        }
        IpAddr::V6(value) => {
            value.is_unspecified()
                || value.is_loopback()
                || value.is_unique_local()
                || value.is_unicast_link_local()
                || value.is_multicast()
                || is_documentation_v6(value)
        }
    }
}

fn is_reserved_v4(value: Ipv4Addr) -> bool {
    let octets = value.octets();
    (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 198 && octets[1] == 51 && octets[2] == 100)
        || (octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
}

fn is_documentation_v6(value: Ipv6Addr) -> bool {
    let segments = value.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}
