use kiana_domain::{json_digest, CredentialLease, SecretRef, CREDENTIAL_LEASE_SCHEMA};

fn reference() -> SecretRef {
    SecretRef::new(
        "env",
        "CI07_SENTINEL",
        "provider.request",
        "fake-provider",
        1,
    )
    .expect("valid credential reference")
}

#[test]
fn lease_is_opaque_short_lived_and_one_shot() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    let mut lease = CredentialLease::issue(
        reference(),
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest.clone(),
        1_000,
        100,
    )
    .expect("valid lease");
    assert_eq!(lease.schema, CREDENTIAL_LEASE_SCHEMA);
    lease
        .validate_for(
            1_050,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest,
        )
        .expect("bound lease");
    lease.consume(1_050).expect("first consumption");
    assert_eq!(
        lease.consume(1_051).unwrap_err(),
        "credential_lease_replayed"
    );
    assert_eq!(
        lease.validate_at(1_050).unwrap_err(),
        "credential_lease_invalid"
    );
}

#[test]
fn lease_rejects_expiry_and_binding_drift_before_effect() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    let lease = CredentialLease::issue(
        reference(),
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest.clone(),
        1_000,
        100,
    )
    .expect("valid lease");
    assert_eq!(
        lease.validate_at(1_100).unwrap_err(),
        "credential_lease_expired"
    );
    assert_eq!(
        lease
            .validate_for(
                1_050,
                "other-provider",
                "provider.request",
                "fake-provider",
                &endpoint_digest,
            )
            .unwrap_err(),
        "credential_lease_provider_mismatch"
    );
    assert_eq!(
        lease
            .validate_for(
                1_050,
                "fake-provider",
                "provider.request",
                "fake-provider",
                &json_digest(&serde_json::json!("https://other.invalid/v1")),
            )
            .unwrap_err(),
        "credential_lease_endpoint_mismatch"
    );
}

#[test]
fn lease_json_has_no_secret_slot_and_rejects_unknown_fields() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    let lease = CredentialLease::issue(
        reference(),
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest,
        1_000,
        100,
    )
    .expect("valid lease");
    let encoded = serde_json::to_string(&lease).expect("lease json");
    assert!(!encoded.contains("CI07_SECRET_VALUE"));
    assert!(!encoded.contains("secret_value"));

    let mut value = serde_json::to_value(lease).expect("lease value");
    value["secret_value"] = serde_json::json!("CI07_SECRET_VALUE");
    assert!(serde_json::from_value::<CredentialLease>(value).is_err());
}

#[test]
fn malformed_endpoint_digest_cannot_be_issued_or_consumed() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    for invalid in [
        format!("sha256:{}", "g".repeat(64)),
        format!("sha256:{}", "a".repeat(63)),
        format!("sha256:{}", "a".repeat(65)),
        format!("sha512:{}", "a".repeat(64)),
    ] {
        assert_eq!(
            CredentialLease::issue(
                reference(),
                "fake-provider",
                "provider.request",
                "fake-provider",
                invalid.clone(),
                1_000,
                100,
            )
            .unwrap_err(),
            "credential_lease_invalid"
        );
        let mut lease = CredentialLease::issue(
            reference(),
            "fake-provider",
            "provider.request",
            "fake-provider",
            endpoint_digest.clone(),
            1_000,
            100,
        )
        .expect("valid lease");
        lease.endpoint_digest = invalid;
        lease.lease_digest = lease.digest();
        let before = lease.clone();
        assert_eq!(
            lease.consume(1_001).unwrap_err(),
            "credential_lease_invalid"
        );
        assert_eq!(lease, before, "denial must not consume the lease");
    }
}

#[test]
fn reusable_metadata_cannot_authorize_a_credential_effect() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    let mut lease = CredentialLease::issue(
        reference(),
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest.clone(),
        1_000,
        100,
    )
    .expect("valid lease");
    lease.one_shot = false;
    lease.lease_digest = lease.digest();
    let before = lease.clone();
    assert_eq!(
        lease
            .validate_for(
                1_001,
                "fake-provider",
                "provider.request",
                "fake-provider",
                &endpoint_digest,
            )
            .unwrap_err(),
        "credential_lease_invalid"
    );
    assert_eq!(
        lease.consume(1_001).unwrap_err(),
        "credential_lease_invalid"
    );
    assert_eq!(lease, before, "denial must not consume the lease");
}
