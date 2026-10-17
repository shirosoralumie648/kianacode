use kiana_domain::{
    ConnectorHttpsEndpoint, ConnectorHttpsError, ConnectorHttpsErrorCode, ConnectorHttpsPolicy,
    ConnectorHttpsResolution, CONNECTOR_HTTPS_ENDPOINT_SCHEMA, CONNECTOR_HTTPS_MAX_ADDRESSES,
    CONNECTOR_HTTPS_MAX_REDIRECTS, CONNECTOR_HTTPS_POLICY_SCHEMA,
    CONNECTOR_HTTPS_RESOLUTION_SCHEMA,
};
use serde_json::json;

fn policy() -> ConnectorHttpsPolicy {
    ConnectorHttpsPolicy::from_parts(
        "https://api.example.com",
        vec!["api.example.com".to_owned()],
        vec!["93.184.216.34".to_owned()],
        Vec::new(),
        2,
    )
    .expect("policy")
}

/// Same pinned origin without a resolved-address allowlist, so that every *public* address of the
/// pinned host is acceptable.
fn open_policy() -> ConnectorHttpsPolicy {
    ConnectorHttpsPolicy::from_parts(
        "https://api.example.com",
        vec!["api.example.com".to_owned()],
        Vec::new(),
        Vec::new(),
        2,
    )
    .expect("open policy")
}

fn endpoint(policy: &ConnectorHttpsPolicy, url: &str) -> ConnectorHttpsEndpoint {
    policy.endpoint(url).expect("endpoint")
}

#[test]
fn endpoint_rejects_userinfo_non_https_query_fragment_and_origin_drift() {
    let policy = policy();
    for (url, code) in [
        (
            "https://user:secret@api.example.com/v1",
            ConnectorHttpsErrorCode::UrlUserinfoDenied,
        ),
        (
            "http://api.example.com/v1",
            ConnectorHttpsErrorCode::NonHttpsDenied,
        ),
        (
            "https://api.example.com/v1?token=secret",
            ConnectorHttpsErrorCode::EndpointQueryDenied,
        ),
        (
            "https://api.example.com/v1#fragment",
            ConnectorHttpsErrorCode::EndpointFragmentDenied,
        ),
        (
            "https://other.example.com/v1",
            ConnectorHttpsErrorCode::OriginMismatch,
        ),
    ] {
        assert_eq!(policy.endpoint(url).unwrap_err().code, code);
    }
}

#[test]
fn resolution_rejects_ssrf_metadata_and_unallowlisted_addresses() {
    let policy = policy();
    let endpoint = policy
        .endpoint("https://api.example.com/v1")
        .expect("endpoint");
    for (address, code) in [
        ("127.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("10.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        (
            "169.254.169.254",
            ConnectorHttpsErrorCode::MetadataAddressDenied,
        ),
        (
            "93.184.216.35",
            ConnectorHttpsErrorCode::EgressAddressNotAllowlisted,
        ),
    ] {
        assert_eq!(
            policy
                .observe_resolution(&endpoint, &[address.to_owned()])
                .unwrap_err()
                .code,
            code
        );
    }
    let resolution = policy
        .observe_resolution(&endpoint, &["93.184.216.34".to_owned()])
        .expect("pinned public address");
    assert_eq!(resolution.schema, "kiana.connector-https-resolution.v1");
    assert!(resolution.resolution_digest.starts_with("sha256:"));
}

#[test]
fn redirect_and_proxy_are_pinned_and_structured() {
    let policy = policy();
    let endpoint = policy
        .endpoint("https://api.example.com/v1")
        .expect("endpoint");
    let same_origin = policy
        .redirect(&endpoint, "https://api.example.com/v2")
        .expect("same origin redirect");
    assert_eq!(same_origin.schema, CONNECTOR_HTTPS_ENDPOINT_SCHEMA);
    assert_eq!(
        policy
            .redirect(&endpoint, "https://other.example.com/v2")
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::RedirectOriginDenied
    );
    assert_eq!(
        policy
            .validate_proxy(Some("https://proxy.example.com"))
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ProxyDenied
    );
}

#[test]
fn policy_serialization_is_strict_and_digest_bound() {
    let policy = policy();
    let encoded = serde_json::to_value(&policy).expect("encode");
    assert!(serde_json::from_value::<ConnectorHttpsPolicy>(encoded).is_ok());
    let mut tampered = policy.clone();
    tampered.pinned_origin = "https://other.example.com:443".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err().code,
        ConnectorHttpsErrorCode::PolicyInvalid
    );
    assert!(serde_json::from_str::<ConnectorHttpsPolicy>(
        r#"{"schema":"kiana.connector-https-policy.v1","version":1,"pinned_origin":"https://api.example.com","allowed_egress_hosts":["api.example.com"],"max_redirects":0,"policy_digest":"sha256:bad","unknown":true}"#
    )
    .is_err());
}

#[test]
fn policy_bounds_and_unknown_fields_fail_closed() {
    assert_eq!(policy().schema, CONNECTOR_HTTPS_POLICY_SCHEMA);
    assert_eq!(policy().pinned_origin, "https://api.example.com:443");

    // The pinned origin must be a bare https origin: no other scheme, no userinfo, no path,
    // query or fragment can be smuggled into the policy.
    for origin in [
        "http://api.example.com",
        "https://user:secret@api.example.com",
        "https://api.example.com/v1",
        "https://api.example.com?token=secret",
        "https://api.example.com#fragment",
    ] {
        assert_eq!(
            ConnectorHttpsPolicy::from_parts(
                origin,
                vec!["api.example.com".to_owned()],
                Vec::new(),
                Vec::new(),
                0,
            )
            .unwrap_err()
            .code,
            ConnectorHttpsErrorCode::PolicyInvalid,
            "origin {origin}"
        );
    }

    // The pinned host must be on the exact egress allowlist, and the allowlist cannot be empty.
    assert_eq!(
        ConnectorHttpsPolicy::from_parts(
            "https://api.example.com",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            0,
        )
        .unwrap_err()
        .code,
        ConnectorHttpsErrorCode::HostNotAllowlisted
    );
    assert_eq!(
        ConnectorHttpsPolicy::from_parts(
            "https://api.example.com",
            vec!["other.example.com".to_owned()],
            Vec::new(),
            Vec::new(),
            0,
        )
        .unwrap_err()
        .code,
        ConnectorHttpsErrorCode::HostNotAllowlisted
    );

    // The redirect budget is bounded.
    assert_eq!(
        ConnectorHttpsPolicy::from_parts(
            "https://api.example.com",
            vec!["api.example.com".to_owned()],
            Vec::new(),
            Vec::new(),
            CONNECTOR_HTTPS_MAX_REDIRECTS + 1,
        )
        .unwrap_err()
        .code,
        ConnectorHttpsErrorCode::PolicyInvalid
    );

    // A pinned resolved address must parse and may not be local, private or metadata.
    for (address, code) in [
        ("not-an-address", ConnectorHttpsErrorCode::PolicyInvalid),
        ("10.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("2001:db8::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        (
            "169.254.169.254",
            ConnectorHttpsErrorCode::MetadataAddressDenied,
        ),
    ] {
        assert_eq!(
            ConnectorHttpsPolicy::from_parts(
                "https://api.example.com",
                vec!["api.example.com".to_owned()],
                vec![address.to_owned()],
                Vec::new(),
                0,
            )
            .unwrap_err()
            .code,
            code,
            "address {address}"
        );
    }

    // A proxy origin is an explicit https origin, never a URL with credentials or a path.
    for proxy in [
        "http://proxy.example.com",
        "https://proxy.example.com/forward",
        "https://user:secret@proxy.example.com",
    ] {
        assert_eq!(
            ConnectorHttpsPolicy::from_parts(
                "https://api.example.com",
                vec!["api.example.com".to_owned()],
                Vec::new(),
                vec![proxy.to_owned()],
                0,
            )
            .unwrap_err()
            .code,
            ConnectorHttpsErrorCode::ProxyOriginInvalid,
            "proxy {proxy}"
        );
    }

    // Unknown fields are refused on every contract object instead of being silently ignored.
    let policy = policy();
    let mut encoded_policy = serde_json::to_value(&policy).expect("policy encode");
    encoded_policy["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorHttpsPolicy>(encoded_policy).is_err());

    let endpoint = endpoint(&policy, "https://api.example.com/v1");
    let mut encoded_endpoint = serde_json::to_value(&endpoint).expect("endpoint encode");
    encoded_endpoint["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorHttpsEndpoint>(encoded_endpoint).is_err());

    let resolution = policy
        .observe_resolution(&endpoint, &["93.184.216.34".to_owned()])
        .expect("resolution");
    let mut encoded_resolution = serde_json::to_value(&resolution).expect("resolution encode");
    encoded_resolution["unexpected"] = json!(true);
    assert!(serde_json::from_value::<ConnectorHttpsResolution>(encoded_resolution).is_err());

    // Digest-bound projections cannot be edited in place.
    let mut tampered_endpoint = endpoint.clone();
    tampered_endpoint.path = "/v2".to_owned();
    assert_eq!(
        policy
            .validate_endpoint(&tampered_endpoint)
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::OriginMismatch
    );
    assert_eq!(
        policy
            .observe_resolution(&tampered_endpoint, &["93.184.216.34".to_owned()])
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::OriginMismatch
    );
    assert_eq!(
        policy
            .endpoint("https://api.example.com:8443/v1")
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::OriginMismatch
    );
}

#[test]
fn dns_observation_is_sorted_bounded_and_policy_bound() {
    let policy = open_policy();
    let endpoint = endpoint(&policy, "https://api.example.com/v1");
    let observation = policy
        .observe_resolution(
            &endpoint,
            &[
                "93.184.216.35".to_owned(),
                "93.184.216.34".to_owned(),
                "93.184.216.34".to_owned(),
            ],
        )
        .expect("sorted observation");
    assert_eq!(observation.schema, CONNECTOR_HTTPS_RESOLUTION_SCHEMA);
    assert_eq!(
        observation.resolved_addresses,
        vec!["93.184.216.34".to_owned(), "93.184.216.35".to_owned()]
    );
    assert_eq!(observation.endpoint_digest, endpoint.endpoint_digest);
    assert_eq!(observation.policy_digest, policy.policy_digest);
    assert_eq!(observation.resolution_digest, observation.digest());
    assert!(policy.validate_resolution(&endpoint, &observation).is_ok());

    // A hand-made observation must stay sorted, digest-bound and bound to this endpoint/policy.
    let mut unsorted = observation.clone();
    unsorted.resolved_addresses = vec!["93.184.216.35".to_owned(), "93.184.216.34".to_owned()];
    unsorted.resolution_digest = unsorted.digest();
    assert_eq!(
        policy
            .validate_resolution(&endpoint, &unsorted)
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionInvalid
    );

    let mut forged_digest = observation.clone();
    forged_digest.resolution_digest = kiana_domain::json_digest(&json!("forged-resolution"));
    assert_eq!(
        policy
            .validate_resolution(&endpoint, &forged_digest)
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionInvalid
    );

    let mut foreign_endpoint = observation.clone();
    foreign_endpoint.endpoint_digest = kiana_domain::json_digest(&json!("foreign-endpoint"));
    foreign_endpoint.resolution_digest = foreign_endpoint.digest();
    assert_eq!(
        policy
            .validate_resolution(&endpoint, &foreign_endpoint)
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionInvalid
    );

    let mut stale_policy = observation.clone();
    stale_policy.policy_digest = kiana_domain::json_digest(&json!("stale-policy"));
    stale_policy.resolution_digest = stale_policy.digest();
    assert_eq!(
        policy
            .validate_resolution(&endpoint, &stale_policy)
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionInvalid
    );

    // An empty or oversized resolver answer fails closed before any socket is opened, and a
    // non-address answer is never accepted as a pinned route.
    assert_eq!(
        policy.observe_resolution(&endpoint, &[]).unwrap_err().code,
        ConnectorHttpsErrorCode::ResolutionRequired
    );
    let oversized: Vec<String> = (0..=CONNECTOR_HTTPS_MAX_ADDRESSES)
        .map(|index| format!("93.184.216.{}", index + 1))
        .collect();
    assert_eq!(
        policy
            .observe_resolution(&endpoint, &oversized)
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionRequired
    );
    assert_eq!(
        policy
            .observe_resolution(&endpoint, &["not-an-address".to_owned()])
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionInvalid
    );
}

#[test]
fn ssrf_and_metadata_addresses_fail_closed_for_every_class() {
    let policy = open_policy();
    let endpoint = endpoint(&policy, "https://api.example.com/v1");
    for (address, code) in [
        ("0.0.0.0", ConnectorHttpsErrorCode::LocalAddressDenied),
        (
            "255.255.255.255",
            ConnectorHttpsErrorCode::LocalAddressDenied,
        ),
        ("127.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("10.1.2.3", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("172.16.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("192.168.1.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("169.254.10.10", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("224.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("100.64.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("192.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("198.18.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("192.0.2.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("198.51.100.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("203.0.113.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("::", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("fc00::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("fe80::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("ff02::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("2001:db8::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        (
            "169.254.169.254",
            ConnectorHttpsErrorCode::MetadataAddressDenied,
        ),
    ] {
        assert_eq!(
            policy
                .observe_resolution(&endpoint, &[address.to_owned()])
                .unwrap_err()
                .code,
            code,
            "address {address}"
        );
    }
    // Control: a public address of the pinned host still resolves.
    assert!(policy
        .observe_resolution(&endpoint, &["93.184.216.34".to_owned()])
        .is_ok());

    // The same classes cannot be pinned into the egress allowlist either.
    for (address, code) in [
        ("127.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("10.0.0.1", ConnectorHttpsErrorCode::LocalAddressDenied),
        ("fc00::1", ConnectorHttpsErrorCode::LocalAddressDenied),
        (
            "169.254.169.254",
            ConnectorHttpsErrorCode::MetadataAddressDenied,
        ),
    ] {
        assert_eq!(
            ConnectorHttpsPolicy::from_parts(
                "https://api.example.com",
                vec!["api.example.com".to_owned()],
                vec![address.to_owned()],
                Vec::new(),
                0,
            )
            .unwrap_err()
            .code,
            code,
            "pinned address {address}"
        );
    }
}

#[test]
fn literal_ip_endpoints_require_exactly_the_observed_address() {
    let policy = ConnectorHttpsPolicy::from_parts(
        "https://93.184.216.34",
        vec!["93.184.216.34".to_owned()],
        Vec::new(),
        Vec::new(),
        0,
    )
    .expect("literal ip policy");
    assert_eq!(policy.pinned_origin, "https://93.184.216.34:443");
    let endpoint = endpoint(&policy, "https://93.184.216.34/v1");
    assert_eq!(endpoint.host, "93.184.216.34");
    assert_eq!(
        endpoint.literal_ip(),
        Some("93.184.216.34".parse().expect("literal ip"))
    );
    assert!(policy
        .observe_resolution(&endpoint, &["93.184.216.34".to_owned()])
        .is_ok());
    assert_eq!(
        policy
            .observe_resolution(&endpoint, &["93.184.216.35".to_owned()])
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionHostMismatch
    );
    assert_eq!(
        policy
            .observe_resolution(
                &endpoint,
                &["93.184.216.34".to_owned(), "93.184.216.35".to_owned()],
            )
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ResolutionHostMismatch
    );
}

#[test]
fn endpoint_host_allowlist_is_exact_and_has_no_wildcards() {
    let policy = open_policy();
    let canonical = endpoint(&policy, "https://API.Example.COM/v1");
    assert_eq!(canonical.host, "api.example.com");
    assert_eq!(canonical.origin, "https://api.example.com:443");
    for url in [
        "https://api.example.com.evil.example.com/v1",
        "https://sub.api.example.com/v1",
        "https://api.example.com:8443/v1",
    ] {
        assert_eq!(
            policy.endpoint(url).unwrap_err().code,
            ConnectorHttpsErrorCode::OriginMismatch,
            "url {url}"
        );
    }
    // A second allowlisted host is still outside the pinned origin.
    let two_hosts = ConnectorHttpsPolicy::from_parts(
        "https://api.example.com",
        vec!["api.example.com".to_owned(), "cdn.example.com".to_owned()],
        Vec::new(),
        Vec::new(),
        0,
    )
    .expect("two host policy");
    assert_eq!(
        two_hosts
            .endpoint("https://cdn.example.com/v1")
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::OriginMismatch
    );
}

#[test]
fn proxy_selection_is_explicit_and_allowlisted() {
    // The local binding must not shadow the `policy()` helper that the last assertion calls.
    let proxy_policy = ConnectorHttpsPolicy::from_parts(
        "https://api.example.com",
        vec!["api.example.com".to_owned()],
        Vec::new(),
        vec!["https://proxy.example.com".to_owned()],
        0,
    )
    .expect("proxy policy");
    // No proxy at all is the default and never means "use the ambient proxy".
    assert!(proxy_policy.validate_proxy(None).is_ok());
    assert!(proxy_policy
        .validate_proxy(Some("https://proxy.example.com"))
        .is_ok());
    assert!(proxy_policy
        .validate_proxy(Some("https://proxy.example.com:443/"))
        .is_ok());
    assert_eq!(
        proxy_policy
            .validate_proxy(Some("https://other-proxy.example.com"))
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ProxyDenied
    );
    assert_eq!(
        proxy_policy
            .validate_proxy(Some("http://proxy.example.com"))
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ProxyOriginInvalid
    );
    assert_eq!(
        proxy_policy
            .validate_proxy(Some("https://proxy.example.com/forward"))
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ProxyOriginInvalid
    );
    // A policy without an explicit proxy allowlist refuses every explicit proxy.
    assert_eq!(
        policy()
            .validate_proxy(Some("https://proxy.example.com"))
            .unwrap_err()
            .code,
        ConnectorHttpsErrorCode::ProxyDenied
    );
}

#[test]
fn error_classification_is_stable_redaction_safe_and_serde_bound() {
    let codes = [
        ConnectorHttpsErrorCode::PolicyInvalid,
        ConnectorHttpsErrorCode::EndpointInvalid,
        ConnectorHttpsErrorCode::UrlUserinfoDenied,
        ConnectorHttpsErrorCode::NonHttpsDenied,
        ConnectorHttpsErrorCode::EndpointQueryDenied,
        ConnectorHttpsErrorCode::EndpointFragmentDenied,
        ConnectorHttpsErrorCode::OriginMismatch,
        ConnectorHttpsErrorCode::HostNotAllowlisted,
        ConnectorHttpsErrorCode::ResolutionRequired,
        ConnectorHttpsErrorCode::ResolutionInvalid,
        ConnectorHttpsErrorCode::LocalAddressDenied,
        ConnectorHttpsErrorCode::MetadataAddressDenied,
        ConnectorHttpsErrorCode::EgressAddressNotAllowlisted,
        ConnectorHttpsErrorCode::ResolutionHostMismatch,
        ConnectorHttpsErrorCode::RedirectOriginDenied,
        ConnectorHttpsErrorCode::RedirectLimitExceeded,
        ConnectorHttpsErrorCode::ProxyDenied,
        ConnectorHttpsErrorCode::ProxyOriginInvalid,
    ];
    let mut stable = std::collections::BTreeSet::new();
    for code in codes {
        let text = code.as_str();
        assert!(text.starts_with("connector_https_"), "code text {text}");
        assert!(stable.insert(text), "duplicate classification {text}");
        let error = ConnectorHttpsError::new(code);
        assert_eq!(error.code(), text);
        assert_eq!(error.to_string(), text);
        assert_eq!(
            serde_json::to_value(&error).expect("encode error"),
            json!({ "code": code })
        );
        assert_eq!(
            serde_json::from_value::<ConnectorHttpsError>(json!({ "code": code }))
                .expect("decode error")
                .code(),
            text
        );
        assert!(serde_json::from_value::<ConnectorHttpsError>(
            json!({ "code": code, "url": "https://api.example.com" })
        )
        .is_err());
    }
    assert_eq!(stable.len(), 18);

    // Error projections carry the stable code only: URL text, credentials and proxy text never
    // travel with the classification.
    let secret = "s3cr3t-token-value";
    let denial = policy()
        .endpoint(&format!("https://user:{secret}@api.example.com/v1"))
        .unwrap_err();
    assert_eq!(denial.code, ConnectorHttpsErrorCode::UrlUserinfoDenied);
    assert!(!denial.to_string().contains(secret));
    assert!(!format!("{denial:?}").contains(secret));
    assert!(!serde_json::to_string(&denial)
        .expect("encode denial")
        .contains(secret));
}
