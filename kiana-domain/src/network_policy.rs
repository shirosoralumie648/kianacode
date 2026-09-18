//! Server-owned network endpoint policy and resolved-address observation.
//!
//! DNS resolution is deliberately outside the domain crate. An adapter must provide the
//! resolved address set to this pure contract, which validates every address before a Broker
//! handler can use the observation. The observation digest is suitable for a later effect receipt
//! and makes a resolver refresh explicit instead of silently following a DNS change.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::{IpAddr, Ipv4Addr};
use url::{Host, Url};

pub const NETWORK_POLICY_SCHEMA: &str = "kiana.network-policy.v1";
pub const NETWORK_ENDPOINT_OBSERVATION_SCHEMA: &str = "kiana.network-endpoint-observation.v1";
pub const NETWORK_POLICY_VERSION: u64 = 1;
pub const MAX_NETWORK_HOSTS: usize = 64;
pub const MAX_NETWORK_ADDRESSES: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkPolicy {
    pub schema: String,
    pub version: u64,
    pub allowed_hosts: Vec<String>,
    pub allow_loopback: bool,
    pub allow_private: bool,
    pub allow_link_local: bool,
    pub require_tls: bool,
    pub policy_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkEndpointObservation {
    pub schema: String,
    pub version: u64,
    pub endpoint_digest: String,
    pub host: String,
    pub port: u16,
    pub resolved_addresses: Vec<String>,
    pub resolution_digest: String,
    pub policy_digest: String,
}

impl NetworkPolicy {
    pub fn from_scope_hosts(hosts: &[String]) -> Result<Self, String> {
        Self::new(hosts.to_vec(), false, false, false, true)
    }

    pub fn new(
        mut allowed_hosts: Vec<String>,
        allow_loopback: bool,
        allow_private: bool,
        allow_link_local: bool,
        require_tls: bool,
    ) -> Result<Self, String> {
        for host in &mut allowed_hosts {
            *host = normalize_host(host)?;
        }
        allowed_hosts.sort_unstable();
        allowed_hosts.dedup();
        let mut policy = Self {
            schema: NETWORK_POLICY_SCHEMA.to_owned(),
            version: NETWORK_POLICY_VERSION,
            allowed_hosts,
            allow_loopback,
            allow_private,
            allow_link_local,
            require_tls,
            policy_digest: String::new(),
        };
        policy.validate()?;
        policy.policy_digest = policy.digest();
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NETWORK_POLICY_SCHEMA
            || self.version != NETWORK_POLICY_VERSION
            || self.allowed_hosts.is_empty()
            || self.allowed_hosts.len() > MAX_NETWORK_HOSTS
            || self.allowed_hosts.windows(2).any(|pair| pair[0] >= pair[1])
            || self
                .allowed_hosts
                .iter()
                .any(|host| normalize_host(host).is_err())
            || (!self.policy_digest.is_empty() && self.policy_digest != self.digest())
        {
            return Err("network_policy_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "allowed_hosts": self.allowed_hosts,
            "allow_loopback": self.allow_loopback,
            "allow_private": self.allow_private,
            "allow_link_local": self.allow_link_local,
            "require_tls": self.require_tls,
        }))
    }

    /// Validate an adapter-supplied DNS result set without performing DNS or network I/O.
    pub fn observe(
        &self,
        endpoint: &str,
        resolved_addresses: &[String],
    ) -> Result<NetworkEndpointObservation, String> {
        self.validate()?;
        let url = Url::parse(endpoint).map_err(|_| "network_endpoint_invalid".to_owned())?;
        if !matches!(url.scheme(), "http" | "https")
            || self.require_tls && url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("network_endpoint_scheme_or_credentials_denied".to_owned());
        }
        let host = url
            .host()
            .map(host_text)
            .ok_or_else(|| "network_endpoint_host_required".to_owned())?;
        if !self.allowed_hosts.iter().any(|allowed| allowed == &host) {
            return Err("network_endpoint_host_not_allowlisted".to_owned());
        }
        let port = url
            .port_or_known_default()
            .ok_or_else(|| "network_endpoint_port_required".to_owned())?;
        if resolved_addresses.is_empty() || resolved_addresses.len() > MAX_NETWORK_ADDRESSES {
            return Err("network_resolution_empty_or_too_large".to_owned());
        }
        let mut addresses = resolved_addresses
            .iter()
            .map(|value| {
                value
                    .parse::<IpAddr>()
                    .map_err(|_| "network_resolution_address_invalid".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        addresses.sort_by_key(ToString::to_string);
        addresses.dedup();
        for address in &addresses {
            validate_address(
                *address,
                self.allow_loopback,
                self.allow_private,
                self.allow_link_local,
            )?;
        }
        if let Host::Ipv4(expected) = url.host().expect("host checked") {
            if addresses != vec![IpAddr::V4(expected)] {
                return Err("network_resolution_host_mismatch".to_owned());
            }
        }
        if let Host::Ipv6(expected) = url.host().expect("host checked") {
            if addresses != vec![IpAddr::V6(expected)] {
                return Err("network_resolution_host_mismatch".to_owned());
            }
        }
        let address_text = addresses
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        Ok(NetworkEndpointObservation {
            schema: NETWORK_ENDPOINT_OBSERVATION_SCHEMA.to_owned(),
            version: NETWORK_POLICY_VERSION,
            endpoint_digest: json_digest(&json!({
                "scheme": url.scheme(),
                "host": host,
                "port": port,
                "path": url.path(),
            })),
            host,
            port,
            resolution_digest: json_digest(&json!({"addresses":address_text})),
            resolved_addresses: address_text,
            policy_digest: self.policy_digest.clone(),
        })
    }
}

fn normalize_host(raw: &str) -> Result<String, String> {
    let host = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host.contains('\0')
        || host.starts_with('.')
        || host.ends_with('.')
        || host.contains("..")
        || host
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':')))
    {
        return Err("network_policy_host_invalid".to_owned());
    }
    Ok(host)
}

fn host_text(host: Host<&str>) -> String {
    match host {
        Host::Domain(value) => value.trim_end_matches('.').to_ascii_lowercase(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => value.to_string(),
    }
}

fn validate_address(
    address: IpAddr,
    allow_loopback: bool,
    allow_private: bool,
    allow_link_local: bool,
) -> Result<(), String> {
    let forbidden_metadata = address == IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254));
    let (unusable, loopback, private, link_local) = match address {
        IpAddr::V4(value) => (
            value.is_unspecified() || value.is_broadcast(),
            value.is_loopback(),
            is_private_v4(value),
            value.is_link_local(),
        ),
        IpAddr::V6(value) => (
            value.is_unspecified(),
            value.is_loopback(),
            value.is_unique_local(),
            value.is_unicast_link_local(),
        ),
    };
    if forbidden_metadata
        || unusable
        || loopback && !allow_loopback
        || private && !allow_private
        || link_local && !allow_link_local
    {
        return Err("network_resolution_local_or_metadata_denied".to_owned());
    }
    Ok(())
}

fn is_private_v4(value: Ipv4Addr) -> bool {
    let octets = value.octets();
    value.is_private()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
}
