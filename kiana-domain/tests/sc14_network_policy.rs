use kiana_domain::NetworkPolicy;

#[test]
fn endpoint_observation_binds_allowlist_tls_and_resolution_digest() {
    let policy = NetworkPolicy::new(
        vec!["api.example.com".to_owned()],
        false,
        false,
        false,
        true,
    )
    .unwrap();
    let observation = policy
        .observe("https://api.example.com/v1", &["93.184.216.34".to_owned()])
        .unwrap();
    assert_eq!(observation.host, "api.example.com");
    assert_eq!(observation.port, 443);
    assert_eq!(observation.resolved_addresses, vec!["93.184.216.34"]);
    assert_eq!(observation.policy_digest, policy.policy_digest);
    assert!(observation.resolution_digest.starts_with("sha256:"));
}

#[test]
fn endpoint_observation_rejects_local_metadata_and_rebinding_candidates() {
    let policy = NetworkPolicy::new(
        vec!["api.example.com".to_owned()],
        false,
        false,
        false,
        true,
    )
    .unwrap();
    for address in ["127.0.0.1", "10.0.0.1", "169.254.169.254", "::1"] {
        assert!(
            policy
                .observe("https://api.example.com", &[address.to_owned()])
                .is_err(),
            "local or metadata address must be denied: {address}"
        );
    }
    assert!(policy
        .observe(
            "https://api.example.com",
            &["93.184.216.34".to_owned(), "10.0.0.1".to_owned()]
        )
        .is_err());
}

#[test]
fn endpoint_observation_rejects_unallowlisted_host_unsafe_url_and_empty_scope() {
    let policy = NetworkPolicy::new(
        vec!["api.example.com".to_owned()],
        false,
        false,
        false,
        true,
    )
    .unwrap();
    assert!(policy
        .observe("http://api.example.com", &["93.184.216.34".to_owned()])
        .is_err());
    assert!(policy
        .observe("https://other.example.com", &["93.184.216.34".to_owned()])
        .is_err());
    assert!(policy
        .observe(
            "https://api.example.com?token=secret",
            &["93.184.216.34".to_owned()]
        )
        .is_err());
    assert!(NetworkPolicy::from_scope_hosts(&[]).is_err());
}
