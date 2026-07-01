use std::net::{Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpNetworkSurface {
    WebFetch,
    WebSearch,
    HttpMcp,
}

#[derive(Debug, Clone, Copy)]
struct HttpNetworkPolicy {
    allow_loopback: bool,
    allow_private: bool,
    allow_link_local: bool,
    same_origin_redirects_only: bool,
}

impl HttpNetworkSurface {
    fn policy(self) -> HttpNetworkPolicy {
        match self {
            HttpNetworkSurface::WebFetch | HttpNetworkSurface::WebSearch => HttpNetworkPolicy {
                allow_loopback: false,
                allow_private: false,
                allow_link_local: false,
                same_origin_redirects_only: false,
            },
            HttpNetworkSurface::HttpMcp => HttpNetworkPolicy {
                allow_loopback: true,
                allow_private: true,
                allow_link_local: false,
                same_origin_redirects_only: true,
            },
        }
    }
}

pub fn validate_http_url(surface: HttpNetworkSurface, raw_url: &str) -> Result<Url, String> {
    let url = Url::parse(raw_url).map_err(|error| deny_url(format!("invalid URL ({error})")))?;
    validate_http_url_target(surface, None, &url)?;
    Ok(url)
}

pub fn validate_http_redirect(
    surface: HttpNetworkSurface,
    previous: &Url,
    next: &Url,
) -> Result<(), String> {
    validate_http_url_target(surface, Some(previous), next)
}

fn validate_http_url_target(
    surface: HttpNetworkSurface,
    previous: Option<&Url>,
    url: &Url,
) -> Result<(), String> {
    match url.scheme() {
        "http" | "https" => {}
        _ => {
            return Err(deny_url(
                "only http:// and https:// URLs are allowed".to_string(),
            ));
        }
    }

    if let Some(previous) = previous {
        let policy = surface.policy();
        if policy.same_origin_redirects_only && !same_origin(previous, url) {
            return Err(deny_redirect(format!(
                "redirect from '{}' to '{}' escapes the configured origin",
                previous, url
            )));
        }
    }

    let Some(host) = url.host() else {
        return Err(deny_url("host is required".to_string()));
    };

    match host {
        Host::Domain(domain) => validate_domain(surface, domain),
        Host::Ipv4(address) => validate_ipv4(surface, address),
        Host::Ipv6(address) => validate_ipv6(surface, address),
    }
}

fn validate_domain(surface: HttpNetworkSurface, domain: &str) -> Result<(), String> {
    let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(deny_url("host is required".to_string()));
    }
    if surface != HttpNetworkSurface::HttpMcp
        && (normalized == "localhost" || normalized.ends_with(".localhost"))
    {
        return Err(deny_url(format!(
            "host '{domain}' resolves to local-only scope"
        )));
    }
    Ok(())
}

fn validate_ipv4(surface: HttpNetworkSurface, address: Ipv4Addr) -> Result<(), String> {
    if address.is_unspecified() || address.is_broadcast() || address.is_documentation() {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }

    let policy = surface.policy();
    if !policy.allow_loopback && address.is_loopback() {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }
    if !policy.allow_link_local && address.is_link_local() {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }
    if !policy.allow_private && is_private_ipv4(address) {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }
    if address == Ipv4Addr::new(169, 254, 169, 254) {
        return Err(deny_url(
            "metadata IP 169.254.169.254 is not allowed".to_string(),
        ));
    }

    Ok(())
}

fn validate_ipv6(surface: HttpNetworkSurface, address: Ipv6Addr) -> Result<(), String> {
    if address.is_unspecified() || is_documentation_ipv6(address) {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }
    let policy = surface.policy();
    if !policy.allow_loopback && address.is_loopback() {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }
    if !policy.allow_link_local && address.is_unicast_link_local() {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }
    if !policy.allow_private && address.is_unique_local() {
        return Err(deny_url(format!(
            "IP address {address} is not a public network target"
        )));
    }

    Ok(())
}

fn is_private_ipv4(address: Ipv4Addr) -> bool {
    let octets = address.octets();
    address.is_private()
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 198 && (18..=19).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
}

fn same_origin(previous: &Url, next: &Url) -> bool {
    previous.scheme() == next.scheme()
        && previous.host_str() == next.host_str()
        && previous.port_or_known_default() == next.port_or_known_default()
}

fn is_documentation_ipv6(address: Ipv6Addr) -> bool {
    let segments = address.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn deny_url(reason: String) -> String {
    format!("network policy denied URL: {reason}")
}

fn deny_redirect(reason: String) -> String {
    format!("network policy denied redirect: {reason}")
}
