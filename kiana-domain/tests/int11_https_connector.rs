use kiana_domain::{
    ConnectorHttpsErrorCode, ConnectorHttpsPolicy, CONNECTOR_HTTPS_ENDPOINT_SCHEMA,
};

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
